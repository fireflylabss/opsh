use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::parser::{command_tail, shell_quote};
use crate::prompt::{
    BOLD, CYAN, DIM, GREEN, RESET, VIOLET, compact_path, git_dirty_check_enabled, prompt_template,
};
use crate::shell::{Flow, Shell, command_shell, rc_path, run_foreground};

const MAX_SOURCE_DEPTH: usize = 32;

pub(crate) const BUILTINS: &[&str] = &[
    ".", "about", "alias", "cd", "clear", "config", "dirs", "exit", "get", "help", "history",
    "mkcd", "mkdir", "open", "path", "popd", "pushd", "pwd", "repeat", "set", "source", "status",
    "time", "touch", "unalias", "unset", "which",
];

pub(crate) fn parse_exit_code(value: Option<&String>) -> Result<i32, String> {
    value.map_or(Ok(0), |code| {
        code.parse()
            .map_err(|_| format!("exit: invalid status: {code}"))
    })
}

fn expand_home(value: &str) -> Result<PathBuf, String> {
    if value == "~" {
        return env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME is not set".into());
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(rest))
            .ok_or("HOME is not set".into());
    }
    Ok(PathBuf::from(value))
}

pub(crate) fn is_builtin(command: &str) -> bool {
    BUILTINS.binary_search(&command).is_ok()
}

fn is_valid_alias_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(character) if character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

fn is_valid_env_key(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(character) if character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

pub(crate) fn find_in_path(command: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(command))
        .find(|candidate| is_executable(candidate))
}

pub(crate) fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

impl Shell {
    pub(crate) fn cd(&mut self, argument: Option<&str>) -> Result<Flow, String> {
        let destination = match argument {
            Some("-") => self
                .previous_dir
                .clone()
                .ok_or("cd: no previous directory")?,
            Some(value) => expand_home(value)?,
            None => env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or("cd: HOME is not set")?,
        };
        let current = env::current_dir().map_err(|error| error.to_string())?;
        env::set_current_dir(&destination)
            .map_err(|error| format!("cd: {}: {error}", destination.display()))?;
        self.previous_dir = Some(current);
        Ok(Flow::Continue(0))
    }

    pub(crate) fn pwd(&self) -> Result<Flow, String> {
        println!(
            "{}",
            env::current_dir()
                .map_err(|error| error.to_string())?
                .display()
        );
        Ok(Flow::Continue(0))
    }

    pub(crate) fn pushd(&mut self, argument: Option<&str>) -> Result<Flow, String> {
        let current = env::current_dir().map_err(|error| error.to_string())?;
        self.cd(argument)?;
        self.directory_stack.push(current);
        self.dirs()
    }

    pub(crate) fn popd(&mut self) -> Result<Flow, String> {
        let destination = self
            .directory_stack
            .pop()
            .ok_or("popd: directory stack is empty")?;
        let current = env::current_dir().map_err(|error| error.to_string())?;
        env::set_current_dir(&destination)
            .map_err(|error| format!("popd: {}: {error}", destination.display()))?;
        self.previous_dir = Some(current);
        self.dirs()
    }

    pub(crate) fn dirs(&self) -> Result<Flow, String> {
        let current = env::current_dir().map_err(|error| error.to_string())?;
        let mut directories =
            vec![compact_path(&current).unwrap_or_else(|| current.display().to_string())];
        directories.extend(
            self.directory_stack
                .iter()
                .rev()
                .map(|path| compact_path(path).unwrap_or_else(|| path.display().to_string())),
        );
        println!("{}", directories.join("  "));
        Ok(Flow::Continue(0))
    }

    pub(crate) fn history_cmd(&mut self, args: &[String]) -> Result<Flow, String> {
        match args {
            [] => self.print_history(0),
            [action] if action == "clear" => {
                self.history.clear();
                self.history_cleared = true;
                self.history.save()?;
                Ok(Flow::Continue(0))
            }
            [count] if count.chars().all(|character| character.is_ascii_digit()) => {
                let count = count
                    .parse::<usize>()
                    .map_err(|_| "history: count is too large".to_owned())?;
                let start = self.history.entries.len().saturating_sub(count);
                self.print_history(start)
            }
            [query] => {
                let mut matched = false;
                for (index, entry) in self.history.entries.iter().enumerate() {
                    if entry.contains(query.as_str()) {
                        matched = true;
                        println!(
                            "{}{:>4}{}  {entry}",
                            self.paint(DIM),
                            index + 1,
                            self.paint(RESET)
                        );
                    }
                }
                Ok(Flow::Continue(if matched { 0 } else { 1 }))
            }
            _ => Err("history: usage: history [N|clear|QUERY]".into()),
        }
    }

    fn print_history(&self, start: usize) -> Result<Flow, String> {
        for (index, entry) in self.history.entries.iter().enumerate().skip(start) {
            println!(
                "{}{:>4}{}  {entry}",
                self.paint(DIM),
                index + 1,
                self.paint(RESET)
            );
        }
        Ok(Flow::Continue(0))
    }

    pub(crate) fn status(&self) -> Result<Flow, String> {
        let (color, label) = if self.last_status == 0 {
            (self.ansi_ok(), "ok")
        } else {
            (self.ansi_err(), "failed")
        };
        println!(
            "{}{}{}  exit {}",
            color,
            label,
            self.ansi_reset(),
            self.last_status
        );
        Ok(Flow::Continue(0))
    }

    pub(crate) fn which(&self, command: Option<&str>) -> Result<Flow, String> {
        let command = command.ok_or("which: expected a command")?;
        if let Ok(aliases) = self.aliases.lock() {
            if let Some(expansion) = aliases.get(command) {
                println!(
                    "{}{}{} is aliased to `{}`",
                    self.paint(VIOLET),
                    command,
                    self.paint(RESET),
                    expansion
                );
                return Ok(Flow::Continue(0));
            }
        }
        if is_builtin(command) {
            println!(
                "{}{}{} is a shell built-in",
                self.paint(VIOLET),
                command,
                self.paint(RESET)
            );
            return Ok(Flow::Continue(0));
        }
        match find_in_path(command) {
            Some(path) => {
                println!("{}", path.display());
                Ok(Flow::Continue(0))
            }
            None => {
                eprintln!("which: {command}: not found");
                Ok(Flow::Continue(1))
            }
        }
    }

    pub(crate) fn path(&self) -> Result<Flow, String> {
        let path = env::var_os("PATH").ok_or("path: PATH is not set")?;
        for (index, directory) in env::split_paths(&path).enumerate() {
            println!(
                "{}{:>2}{}  {}",
                self.paint(DIM),
                index + 1,
                self.paint(RESET),
                directory.display()
            );
        }
        Ok(Flow::Continue(0))
    }

    pub(crate) fn get(&self, key: Option<&str>) -> Result<Flow, String> {
        let key = key.ok_or("get: expected a variable name")?;
        if !is_valid_env_key(key) {
            return Err(format!("get: invalid variable name: {key}"));
        }
        match env::var_os(key) {
            Some(value) => {
                println!("{key}={}", value.to_string_lossy());
                Ok(Flow::Continue(0))
            }
            None => {
                eprintln!("get: {key}: not set");
                Ok(Flow::Continue(1))
            }
        }
    }

    pub(crate) fn mkdir(&self, paths: &[String]) -> Result<Flow, String> {
        if paths.is_empty() {
            return Err("mkdir: expected a path".into());
        }
        for path in paths {
            let path = expand_home(path)?;
            fs::create_dir_all(&path)
                .map_err(|error| format!("mkdir: {}: {error}", path.display()))?;
            println!(
                "{}created{} {}",
                self.paint(GREEN),
                self.paint(RESET),
                path.display()
            );
        }
        Ok(Flow::Continue(0))
    }

    pub(crate) fn mkcd(&mut self, path: Option<&str>) -> Result<Flow, String> {
        let path = path.ok_or("mkcd: expected a directory")?;
        let expanded = expand_home(path)?;
        fs::create_dir_all(&expanded)
            .map_err(|error| format!("mkcd: {}: {error}", expanded.display()))?;
        self.cd(Some(path))
    }

    pub(crate) fn touch(&self, paths: &[String]) -> Result<Flow, String> {
        if paths.is_empty() {
            return Err("touch: expected a path".into());
        }
        for path in paths {
            let path = expand_home(path)?;
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|error| format!("touch: {}: {error}", path.display()))?;
            println!(
                "{}touched{} {}",
                self.paint(CYAN),
                self.paint(RESET),
                path.display()
            );
        }
        Ok(Flow::Continue(0))
    }

    pub(crate) fn open(&self, path: Option<&str>) -> Result<Flow, String> {
        let path = path.ok_or("open: expected a path")?;
        let path = expand_home(path)?;
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let status = run_foreground(Command::new(opener).arg(&path))
            .map_err(|error| format!("open: {opener}: {error}"))?;
        Ok(Flow::Continue(status))
    }

    pub(crate) fn set(&self, words: &[String]) -> Result<Flow, String> {
        if words.len() < 2 {
            return Err("set: usage: set NAME VALUE".into());
        }
        let key = &words[0];
        if !is_valid_env_key(key) {
            return Err(format!("set: invalid variable name: {key}"));
        }
        let value = words[1..].join(" ");
        // The shell is single-threaded; changing its process environment is intentional here.
        unsafe { env::set_var(key, value) };
        Ok(Flow::Continue(0))
    }

    pub(crate) fn unset(&self, key: Option<&str>) -> Result<Flow, String> {
        let key = key.ok_or("unset: expected a variable name")?;
        if !is_valid_env_key(key) {
            return Err(format!("unset: invalid variable name: {key}"));
        }
        // The shell is single-threaded; changing its process environment is intentional here.
        unsafe { env::remove_var(key) };
        Ok(Flow::Continue(0))
    }

    pub(crate) fn alias(&mut self, words: &[String]) -> Result<Flow, String> {
        let mut aliases = self
            .aliases
            .lock()
            .map_err(|_| "alias table is poisoned".to_owned())?;
        if words.is_empty() {
            for (name, value) in aliases.iter() {
                println!("alias {name}={}", shell_quote(value));
            }
            return Ok(Flow::Continue(0));
        }
        if words.len() == 1 {
            let spec = &words[0];
            if let Some((name, value)) = spec.split_once('=') {
                if !is_valid_alias_name(name) {
                    return Err(format!("alias: invalid name: {name}"));
                }
                aliases.insert(name.to_owned(), value.to_owned());
                return Ok(Flow::Continue(0));
            }
            match aliases.get(spec) {
                Some(value) => {
                    println!("alias {spec}={}", shell_quote(value));
                    Ok(Flow::Continue(0))
                }
                None => {
                    eprintln!("alias: {spec}: not found");
                    Ok(Flow::Continue(1))
                }
            }
        } else {
            let name = &words[0];
            if !is_valid_alias_name(name) {
                return Err(format!("alias: invalid name: {name}"));
            }
            aliases.insert(name.clone(), words[1..].join(" "));
            Ok(Flow::Continue(0))
        }
    }

    pub(crate) fn unalias(&mut self, words: &[String]) -> Result<Flow, String> {
        if words.is_empty() {
            return Err("unalias: expected a name".into());
        }
        let mut aliases = self
            .aliases
            .lock()
            .map_err(|_| "alias table is poisoned".to_owned())?;
        let mut status = 0;
        for name in words {
            if aliases.remove(name).is_none() {
                eprintln!("unalias: {name}: not found");
                status = 1;
            }
        }
        Ok(Flow::Continue(status))
    }

    pub(crate) fn source(&mut self, path: Option<&str>) -> Result<Flow, String> {
        let path = expand_home(path.ok_or("source: expected a file")?)?;
        self.source_file(&path)
    }

    pub(crate) fn source_file(&mut self, path: &Path) -> Result<Flow, String> {
        if self.source_depth >= MAX_SOURCE_DEPTH {
            return Err(format!(
                "source: nested deeper than {MAX_SOURCE_DEPTH} levels"
            ));
        }
        let file =
            File::open(path).map_err(|error| format!("source: {}: {error}", path.display()))?;
        let mut status = 0;
        self.source_depth += 1;
        let result = (|| {
            for line in io::BufReader::new(file).lines() {
                let line = line.map_err(|error| error.to_string())?;
                let command = line.trim();
                if command.is_empty() || command.starts_with('#') {
                    continue;
                }
                match self.dispatch(command)? {
                    Flow::Continue(code) => status = code,
                    Flow::Exit(code) => return Ok(Flow::Exit(code)),
                }
            }
            Ok(Flow::Continue(status))
        })();
        self.source_depth -= 1;
        result
    }

    pub(crate) fn repeat(&mut self, input: &str) -> Result<Flow, String> {
        let tail = command_tail(input, 1).ok_or("repeat: usage: repeat COUNT COMMAND")?;
        let mut parts = tail.splitn(2, char::is_whitespace);
        let count = parts
            .next()
            .ok_or("repeat: expected a count")?
            .parse::<usize>()
            .map_err(|_| "repeat: count must be a positive integer")?;
        let command = parts
            .next()
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .ok_or("repeat: expected a command")?;
        if count == 0 {
            return Err("repeat: count must be greater than zero".into());
        }
        let mut status = 0;
        for _ in 0..count {
            status = self.dispatch(command)?.code();
        }
        Ok(Flow::Continue(status))
    }

    pub(crate) fn time(&mut self, input: &str) -> Result<Flow, String> {
        let command = command_tail(input, 1)
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .ok_or("time: expected a command")?;
        let started = Instant::now();
        let status = self.dispatch(command)?.code();
        eprintln!(
            "{}time{}  {:.3}s",
            self.paint(VIOLET),
            self.paint(RESET),
            started.elapsed().as_secs_f64()
        );
        Ok(Flow::Continue(status))
    }

    pub(crate) fn clear(&self) -> Result<Flow, String> {
        if self.interactive {
            print!("\x1b[2J\x1b[H");
            io::stdout().flush().map_err(|error| error.to_string())?;
        }
        Ok(Flow::Continue(0))
    }

    pub(crate) fn help(&self) -> Result<Flow, String> {
        println!(
            "{}◆ opsh built-ins{}\n\n  {cd} [DIR]       change directory ({}cd -{} returns)\n  {pwd}            print current directory\n  {pushd} DIR      enter a directory and save the current one\n  {popd}           return to the last saved directory\n  {dirs}           show the directory stack\n  {history} [N|clear|QUERY]  list, trim, clear or search history\n  {status}         show the last exit status\n  {which} CMD      find a built-in, alias or executable\n  {path}           print PATH entries\n  {get} NAME       print one environment variable\n  {mkdir} DIR...   create directories\n  {mkcd} DIR       create a directory and enter it\n  {touch} FILE...  create files if needed\n  {open} PATH      open with the desktop default app\n  {set} NAME VALUE set an environment variable\n  {unset} NAME     remove an environment variable\n  {alias} [N[=V]]  list or define aliases\n  {unalias} NAME   remove aliases\n  {config}         show active UI / config knobs\n  {source} FILE    run a local opsh file\n  {repeat} N CMD   run a command N times\n  {time} CMD       run a command and show elapsed time\n  {clear}          clear the screen\n  {about}          show project information\n  {exit} [N]       leave opsh\n\n{}History: !! last command · !N entry N · Ctrl+R incremental search\nPrompt placeholders: {{mark}} {{cwd}} {{cwd:full}} {{git}} {{status}} {{elapsed}} {{stack}} {{prompt}}\nInteractive sessions load ~/.option/opsh/rc (or $OPSH_RC).\n&& || ; chains run inside opsh (so cd persists). Pipes and redirects use\n/bin/sh (or $OPSH_SHELL / a non-fish $SHELL), never fish built-ins.{}",
            self.paint(BOLD),
            self.ansi_reset(),
            self.paint(DIM),
            self.ansi_reset(),
            self.paint(DIM),
            self.ansi_reset(),
            cd = self.command("cd"),
            pwd = self.command("pwd"),
            pushd = self.command("pushd"),
            popd = self.command("popd"),
            dirs = self.command("dirs"),
            history = self.command("history"),
            status = self.command("status"),
            which = self.command("which"),
            path = self.command("path"),
            get = self.command("get"),
            mkdir = self.command("mkdir"),
            mkcd = self.command("mkcd"),
            touch = self.command("touch"),
            open = self.command("open"),
            set = self.command("set"),
            unset = self.command("unset"),
            alias = self.command("alias"),
            unalias = self.command("unalias"),
            config = self.command("config"),
            source = self.command("source"),
            repeat = self.command("repeat"),
            time = self.command("time"),
            clear = self.command("clear"),
            about = self.command("about"),
            exit = self.command("exit")
        );
        Ok(Flow::Continue(0))
    }

    pub(crate) fn about(&self) -> Result<Flow, String> {
        println!(
            "{}◆ opsh{}  v{}\n{}small · local-first · no daemon · no telemetry{}",
            self.paint(BOLD),
            self.paint(RESET),
            env!("CARGO_PKG_VERSION"),
            self.paint(DIM),
            self.paint(RESET)
        );
        Ok(Flow::Continue(0))
    }

    pub(crate) fn config(&self) -> Result<Flow, String> {
        let rows = [
            (
                "banner",
                if self.banner_enabled() { "on" } else { "off" }.to_owned(),
            ),
            ("quiet", if self.quiet { "on" } else { "off" }.to_owned()),
            ("color", if self.color { "on" } else { "off" }.to_owned()),
            (
                "prompt_style",
                env::var("OPSH_PROMPT_STYLE").unwrap_or_else(|_| "double".into()),
            ),
            (
                "prompt",
                env::var("OPSH_PROMPT").unwrap_or_else(|_| prompt_template()),
            ),
            (
                "color.ok",
                env::var("OPSH_COLOR_OK").unwrap_or_else(|_| "38;5;114".into()),
            ),
            (
                "color.err",
                env::var("OPSH_COLOR_ERR").unwrap_or_else(|_| "38;5;210".into()),
            ),
            (
                "color.path",
                env::var("OPSH_COLOR_PATH").unwrap_or_else(|_| "38;5;75".into()),
            ),
            (
                "color.mark",
                env::var("OPSH_COLOR_MARK").unwrap_or_else(|_| "38;5;221".into()),
            ),
            (
                "color.accent",
                env::var("OPSH_COLOR_ACCENT").unwrap_or_else(|_| "38;5;183".into()),
            ),
            (
                "git_dirty",
                if git_dirty_check_enabled() {
                    "on"
                } else {
                    "off"
                }
                .to_owned(),
            ),
            ("shell", command_shell()),
            ("history", self.history.path.display().to_string()),
            (
                "rc",
                rc_path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(unset)".into()),
            ),
        ];
        for (key, value) in rows {
            println!(
                "{}{:<14}{} {}",
                self.paint(DIM),
                key,
                self.ansi_reset(),
                value
            );
        }
        Ok(Flow::Continue(0))
    }

    fn command(&self, name: &str) -> String {
        format!("{}{}{}", self.ansi_accent(), name, self.ansi_reset())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::History;
    use crate::shell::CWD_LOCK;

    #[test]
    fn expands_home_paths() {
        let home = env::var_os("HOME").expect("HOME must exist for tests");
        assert_eq!(expand_home("~").unwrap(), PathBuf::from(home));
        assert!(expand_home("~/work").unwrap().ends_with("work"));
    }

    #[test]
    fn validates_environment_keys() {
        assert!(is_valid_env_key("OPSH_THEME"));
        assert!(is_valid_env_key("A1"));
        assert!(!is_valid_env_key("1A"));
        assert!(!is_valid_env_key("NOT-VALID"));
    }

    #[test]
    fn recognizes_builtins() {
        assert!(is_builtin("mkcd"));
        assert!(is_builtin("pushd"));
        assert!(is_builtin("repeat"));
        assert!(is_builtin("."));
        assert!(!is_builtin("ls"));
    }

    #[test]
    fn builtins_are_sorted_for_binary_search() {
        let mut sorted = BUILTINS.to_vec();
        sorted.sort_unstable();
        assert_eq!(BUILTINS, sorted.as_slice());
    }

    #[test]
    fn source_rejects_deep_nesting() {
        let _guard = CWD_LOCK.lock().unwrap();
        let directory = std::env::temp_dir().join(format!("opsh-source-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("loop.opsh");
        fs::write(&path, format!("source {}\n", path.display())).unwrap();

        let history = History {
            path: directory.join("history"),
            entries: Vec::new(),
        };
        let mut shell = Shell::new(history);
        let error = shell
            .source(Some(path.to_str().unwrap()))
            .expect_err("circular source must fail");
        assert!(error.contains("nested deeper"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn quoted_mkdir_creates_spaced_directory() {
        let _guard = CWD_LOCK.lock().unwrap();
        let original = env::current_dir().unwrap();
        let directory = std::env::temp_dir().join(format!("opsh-quotes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        env::set_current_dir(&directory).unwrap();

        let history = History {
            path: directory.join("history"),
            entries: Vec::new(),
        };
        let mut shell = Shell::new(history);
        let flow = shell.dispatch(r#"mkdir "my dir""#).unwrap();
        assert_eq!(flow.code(), 0);
        assert!(directory.join("my dir").is_dir());

        env::set_current_dir(&original).unwrap();
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn recognizes_config_builtin() {
        assert!(is_builtin("config"));
    }

    #[test]
    fn repeat_can_run_builtins() {
        let _guard = CWD_LOCK.lock().unwrap();
        let original = env::current_dir().unwrap();
        let directory = std::env::temp_dir().join(format!("opsh-repeat-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        env::set_current_dir(&directory).unwrap();

        let history = History {
            path: directory.join("history"),
            entries: Vec::new(),
        };
        let mut shell = Shell::new(history);
        let flow = shell.dispatch("repeat 2 mkdir nested-a nested-b").unwrap();
        assert_eq!(flow.code(), 0);
        // mkdir with two args creates both once per iteration — still verifies builtin dispatch
        assert!(directory.join("nested-a").is_dir());
        assert!(directory.join("nested-b").is_dir());
        env::set_current_dir(&original).unwrap();
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn history_lists_searches_and_clears() {
        let directory = std::env::temp_dir().join(format!("opsh-hist-cmd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("history");
        let history = History {
            path: path.clone(),
            entries: vec!["echo alpha".into(), "echo beta".into(), "pwd".into()],
        };
        let mut shell = Shell::new(history);
        assert_eq!(shell.history_cmd(&[]).unwrap().code(), 0);
        assert_eq!(shell.history_cmd(&["1".into()]).unwrap().code(), 0);
        assert_eq!(shell.history_cmd(&["beta".into()]).unwrap().code(), 0);
        assert_eq!(shell.history_cmd(&["zzz".into()]).unwrap().code(), 1);
        assert_eq!(shell.history_cmd(&["clear".into()]).unwrap().code(), 0);
        assert!(shell.history.entries.is_empty());
        assert!(shell.history_cleared);
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        let _ = fs::remove_dir_all(&directory);
    }
}
