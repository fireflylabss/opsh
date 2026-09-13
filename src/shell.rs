use std::collections::BTreeMap;
use std::env;
use std::io::{self, BufRead, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustyline::completion::FilenameCompleter;
use rustyline::error::ReadlineError;
use rustyline::hint::HistoryHinter;
use rustyline::{Config, Editor};

use crate::builtins::parse_exit_code;
use crate::completion::{OpshCompleter, OpshHelper};
use crate::history::{History, expand_history_refs};
use crate::parser::{
    ChainOp, command_tail, has_chain_operators, needs_posix_shell, split_chain, split_words,
};
use crate::prompt::{BOLD, DIM};

pub struct Shell {
    pub(crate) history: History,
    pub(crate) interactive: bool,
    pub(crate) color: bool,
    pub(crate) quiet: bool,
    pub(crate) previous_dir: Option<PathBuf>,
    pub(crate) directory_stack: Vec<PathBuf>,
    pub(crate) last_status: i32,
    pub(crate) last_elapsed: Option<Duration>,
    pub(crate) history_cleared: bool,
    pub(crate) source_depth: usize,
    pub(crate) aliases: Arc<Mutex<BTreeMap<String, String>>>,
}

impl Shell {
    #[allow(dead_code)]
    pub fn new(history: History) -> Self {
        Self::with_options(history, false)
    }

    pub fn with_options(history: History, quiet: bool) -> Self {
        let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
        let color = interactive
            && option_sdk::color_enabled()
            && env::var("TERM").is_ok_and(|term| term != "dumb");
        Self {
            history,
            interactive,
            color,
            quiet,
            previous_dir: None,
            directory_stack: Vec::new(),
            last_status: 0,
            last_elapsed: None,
            history_cleared: false,
            source_depth: 0,
            aliases: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn repl(&mut self) -> Result<i32, String> {
        if self.interactive {
            if let Err(error) = self.load_rc() {
                eprintln!("{}opsh:{} {error}", self.ansi_err(), self.ansi_reset());
                self.last_status = 1;
            }
            if self.banner_enabled() {
                self.banner();
            }
            return self.interactive_repl();
        }
        self.batch_repl()
    }

    pub(crate) fn banner_enabled(&self) -> bool {
        if self.quiet {
            return false;
        }
        match env::var("OPSH_BANNER") {
            Ok(value) => !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            ),
            Err(_) => true,
        }
    }

    pub(crate) fn load_rc(&mut self) -> Result<(), String> {
        let Some(path) = rc_path() else {
            return Ok(());
        };
        if !path.is_file() {
            return Ok(());
        }
        match self.source_file(&path)? {
            Flow::Continue(_) => Ok(()),
            Flow::Exit(code) => Err(format!("rc exited with status {code}")),
        }
    }

    fn interactive_repl(&mut self) -> Result<i32, String> {
        let config = Config::builder().history_ignore_space(true).build();
        let helper = OpshHelper {
            completer: OpshCompleter {
                files: FilenameCompleter::new(),
                aliases: Arc::clone(&self.aliases),
            },
            hinter: HistoryHinter::new(),
        };
        let mut editor = Editor::with_config(config)
            .map_err(|error| format!("could not start line editor: {error}"))?;
        editor.set_helper(Some(helper));
        for entry in &self.history.entries {
            let _ = editor.add_history_entry(entry.as_str());
        }

        loop {
            let (raw, styled) = self.prompt_pair();
            match editor.readline(&(raw, styled)) {
                Ok(line) => {
                    let started = Instant::now();
                    let _ = editor.add_history_entry(line.as_str());
                    match self.execute(&line) {
                        Ok(Flow::Continue(code)) => {
                            self.last_status = code;
                            self.last_elapsed = Some(started.elapsed());
                        }
                        Ok(Flow::Exit(code)) => {
                            self.save_history();
                            return Ok(code);
                        }
                        Err(error) => {
                            self.last_status = 1;
                            self.last_elapsed = Some(started.elapsed());
                            eprintln!("{}opsh:{} {error}", self.ansi_err(), self.ansi_reset());
                        }
                    }
                    if self.history_cleared {
                        let _ = editor.clear_history();
                        self.history_cleared = false;
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C");
                    self.last_status = 130;
                    self.last_elapsed = None;
                }
                Err(ReadlineError::Eof) => break,
                Err(error) => return Err(error.to_string()),
            }
        }
        self.save_history();
        Ok(self.last_status)
    }

    fn batch_repl(&mut self) -> Result<i32, String> {
        let stdin = io::stdin();
        let mut lines = stdin.lock().lines();
        loop {
            let Some(line) = lines.next() else { break };
            let line = line.map_err(|error| error.to_string())?;
            match self.execute(&line) {
                Ok(Flow::Continue(code)) => self.last_status = code,
                Ok(Flow::Exit(code)) => {
                    self.save_history();
                    return Ok(code);
                }
                Err(error) => return Err(error),
            }
        }
        self.save_history();
        Ok(self.last_status)
    }

    pub fn run_command(&mut self, command: &str) -> Result<i32, String> {
        let result = match self.execute(command)? {
            Flow::Continue(code) | Flow::Exit(code) => code,
        };
        self.save_history();
        Ok(result)
    }

    fn execute(&mut self, input: &str) -> Result<Flow, String> {
        let command = input.trim();
        if command.is_empty() || command.starts_with('#') {
            return Ok(Flow::Continue(0));
        }
        let (expanded, changed) = expand_history_refs(command, &self.history.entries)?;
        if changed {
            println!("{expanded}");
        }
        self.history.push(&expanded);
        self.dispatch(&expanded)
    }

    pub(crate) fn dispatch(&mut self, command: &str) -> Result<Flow, String> {
        if needs_posix_shell(command) {
            return self.external(command);
        }
        if has_chain_operators(command) {
            return self.run_chain(command);
        }
        self.dispatch_simple(command)
    }

    fn run_chain(&mut self, command: &str) -> Result<Flow, String> {
        let (segments, operators) = split_chain(command)?;
        let mut status = 0;
        for (index, segment) in segments.iter().enumerate() {
            if index > 0 {
                let should_run = match operators[index - 1] {
                    ChainOp::And => status == 0,
                    ChainOp::Or => status != 0,
                    ChainOp::Seq => true,
                };
                if !should_run {
                    continue;
                }
            }
            match self.dispatch_simple(segment)? {
                Flow::Continue(code) => status = code,
                Flow::Exit(code) => return Ok(Flow::Exit(code)),
            }
        }
        Ok(Flow::Continue(status))
    }

    fn dispatch_simple(&mut self, command: &str) -> Result<Flow, String> {
        let expanded = self.expand_alias(command)?;
        self.dispatch_simple_raw(&expanded)
    }

    fn expand_alias(&self, command: &str) -> Result<String, String> {
        let words = split_words(command)?;
        let Some(name) = words.first().map(String::as_str) else {
            return Ok(command.to_owned());
        };
        if name == "alias" || name == "unalias" {
            return Ok(command.to_owned());
        }
        let aliases = self
            .aliases
            .lock()
            .map_err(|_| "alias table is poisoned".to_owned())?;
        let Some(expansion) = aliases.get(name) else {
            return Ok(command.to_owned());
        };
        Ok(match command_tail(command, 1) {
            Some(rest) => format!("{expansion} {rest}"),
            None => expansion.clone(),
        })
    }

    fn dispatch_simple_raw(&mut self, command: &str) -> Result<Flow, String> {
        let words = split_words(command)?;
        let Some(name) = words.first().map(String::as_str) else {
            return Ok(Flow::Continue(0));
        };

        match name {
            "cd" => self.cd(words.get(1).map(String::as_str)),
            "pwd" => self.pwd(),
            "pushd" => self.pushd(words.get(1).map(String::as_str)),
            "popd" => self.popd(),
            "dirs" => self.dirs(),
            "history" => self.history_cmd(&words[1..]),
            "status" => self.status(),
            "which" => self.which(words.get(1).map(String::as_str)),
            "path" => self.path(),
            "get" => self.get(words.get(1).map(String::as_str)),
            "mkdir" => self.mkdir(&words[1..]),
            "mkcd" => self.mkcd(words.get(1).map(String::as_str)),
            "touch" => self.touch(&words[1..]),
            "open" => self.open(words.get(1).map(String::as_str)),
            "set" => self.set(&words[1..]),
            "unset" => self.unset(words.get(1).map(String::as_str)),
            "alias" => self.alias(&words[1..]),
            "unalias" => self.unalias(&words[1..]),
            "source" | "." => self.source(words.get(1).map(String::as_str)),
            "repeat" => self.repeat(command),
            "time" => self.time(command),
            "clear" => self.clear(),
            "config" => self.config(),
            "help" => self.help(),
            "about" => self.about(),
            "exit" => Ok(Flow::Exit(parse_exit_code(words.get(1))?)),
            _ => self.external(command),
        }
    }

    fn external(&self, command: &str) -> Result<Flow, String> {
        let shell = command_shell();
        let status = Command::new(&shell)
            .arg("-c")
            .arg(command)
            .status()
            .map_err(|error| format!("could not run command: {error}"))?;
        Ok(Flow::Continue(status.code().unwrap_or(1)))
    }

    fn banner(&self) {
        println!(
            "{}◆ opsh{}  {}local shell{}\n  {}help{} · Ctrl+R search · Ctrl+D exit\n",
            self.paint(BOLD),
            self.ansi_reset(),
            self.paint(DIM),
            self.ansi_reset(),
            self.ansi_accent(),
            self.ansi_reset()
        );
    }

    fn save_history(&self) {
        if let Err(error) = self.history.save() {
            if self.interactive {
                eprintln!(
                    "{}opsh:{} history disabled: {error}",
                    self.ansi_mark(),
                    self.ansi_reset()
                );
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum Flow {
    Continue(i32),
    Exit(i32),
}

impl Flow {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Continue(code) | Self::Exit(code) => code,
        }
    }
}

pub(crate) fn rc_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("OPSH_RC") {
        return Some(PathBuf::from(path));
    }

    let _ = option_sdk::App::OPSH.ensure();
    let canonical = option_sdk::App::OPSH.path("rc");

    let legacy = env::var_os("XDG_CONFIG_HOME")
        .map(|dir| PathBuf::from(dir).join("opsh/rc"))
        .or_else(|| env::var_os("HOME").map(|dir| PathBuf::from(dir).join(".config/opsh/rc")));
    if let Some(legacy) = legacy {
        let _ = option_sdk::migrate_file(&legacy, &canonical);
    }

    Some(canonical)
}

/// Shell used for external / compound command lines.
///
/// Fish is skipped on purpose: its built-ins are not part of opsh. Real apps and
/// POSIX syntax go through `/bin/sh`, unless `OPSH_SHELL` or a non-fish `$SHELL`
/// is set.
pub(crate) fn command_shell() -> String {
    if let Ok(shell) = env::var("OPSH_SHELL") {
        return shell;
    }
    match env::var("SHELL") {
        Ok(shell) if !is_fish_shell(&shell) => shell,
        _ => "/bin/sh".into(),
    }
}

fn is_fish_shell(shell: &str) -> bool {
    Path::new(shell)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "fish" || name.starts_with("fish-"))
}

#[cfg(test)]
pub(crate) static CWD_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn skips_fish_as_command_shell() {
        assert!(is_fish_shell("/bin/fish"));
        assert!(is_fish_shell("/usr/bin/fish"));
        assert!(is_fish_shell("fish"));
        assert!(!is_fish_shell("/bin/bash"));
        assert!(!is_fish_shell("/bin/sh"));
        assert!(!is_fish_shell("/usr/bin/zsh"));
    }

    #[test]
    fn chain_cd_persists_in_process() {
        let _guard = CWD_LOCK.lock().unwrap();
        let original = env::current_dir().unwrap();
        let directory = std::env::temp_dir().join(format!("opsh-chain-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let nested = directory.join("nested");
        fs::create_dir_all(&nested).unwrap();

        let history = History {
            path: directory.join("history"),
            entries: Vec::new(),
        };
        let mut shell = Shell::new(history);
        let command = format!("cd {} && touch ok", nested.display());
        let flow = shell.dispatch(&command).unwrap();
        assert_eq!(flow.code(), 0);
        assert_eq!(env::current_dir().unwrap(), nested);
        assert!(nested.join("ok").exists());

        env::set_current_dir(&original).unwrap();
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn pipes_still_use_posix_shell() {
        let history = History {
            path: std::env::temp_dir().join(format!("opsh-pipe-hist-{}", std::process::id())),
            entries: Vec::new(),
        };
        let mut shell = Shell::new(history);
        let flow = shell.dispatch("true | true").unwrap();
        assert_eq!(flow.code(), 0);
    }

    #[test]
    fn aliases_expand_once() {
        let history = History {
            path: std::env::temp_dir().join(format!("opsh-alias-hist-{}", std::process::id())),
            entries: Vec::new(),
        };
        let mut shell = Shell::new(history);
        shell.dispatch("alias greet=true").unwrap();
        let flow = shell.dispatch("greet hello").unwrap();
        assert_eq!(flow.code(), 0);
        let which = shell.dispatch("which greet").unwrap();
        assert_eq!(which.code(), 0);
        shell.dispatch("unalias greet").unwrap();
        let missing = shell.dispatch("which greet").unwrap();
        assert_eq!(missing.code(), 1);
    }

    #[test]
    fn loads_rc_aliases_from_file() {
        let directory = std::env::temp_dir().join(format!("opsh-rc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let rc = directory.join("rc");
        fs::write(&rc, "alias hi=true\n").unwrap();
        // The shell is single-threaded; changing its process environment is intentional here.
        unsafe { env::set_var("OPSH_RC", &rc) };

        let history = History {
            path: directory.join("history"),
            entries: Vec::new(),
        };
        let mut shell = Shell::new(history);
        shell.load_rc().unwrap();
        let flow = shell.dispatch("hi").unwrap();
        assert_eq!(flow.code(), 0);

        unsafe { env::remove_var("OPSH_RC") };
        let _ = fs::remove_dir_all(&directory);
    }
}
