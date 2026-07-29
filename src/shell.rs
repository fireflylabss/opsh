use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::hint::HistoryHinter;
use rustyline::{Config, Context, Editor, Helper, Highlighter, Hinter, Validator};

const MAX_HISTORY: usize = 1_000;
const MAX_SOURCE_DEPTH: usize = 32;

const BUILTINS: &[&str] = &[
    ".", "about", "cd", "clear", "dirs", "exit", "get", "help", "history", "mkcd", "mkdir", "open",
    "path", "popd", "pushd", "pwd", "repeat", "set", "source", "status", "time", "touch", "unset",
    "which",
];

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[38;5;81m";
const BLUE: &str = "\x1b[38;5;75m";
const GREEN: &str = "\x1b[38;5;114m";
const YELLOW: &str = "\x1b[38;5;221m";
const RED: &str = "\x1b[38;5;210m";
const VIOLET: &str = "\x1b[38;5;183m";

pub struct Shell {
    history: History,
    interactive: bool,
    color: bool,
    previous_dir: Option<PathBuf>,
    directory_stack: Vec<PathBuf>,
    last_status: i32,
    source_depth: usize,
}

pub struct History {
    path: PathBuf,
    entries: Vec<String>,
}

impl History {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let entries = match fs::read_to_string(&path) {
            Ok(contents) => contents.lines().map(str::to_owned).collect(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(format!("could not read {}: {error}", path.display())),
        };
        Ok(Self { path, entries })
    }

    fn push(&mut self, command: &str) {
        if command.is_empty() || self.entries.last().is_some_and(|last| last == command) {
            return;
        }
        self.entries.push(command.into());
        if self.entries.len() > MAX_HISTORY {
            self.entries.drain(..self.entries.len() - MAX_HISTORY);
        }
    }

    fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        let temp_path = temporary_history_path(&self.path);
        {
            let mut file = File::create(&temp_path)
                .map_err(|error| format!("could not write {}: {error}", temp_path.display()))?;
            for entry in &self.entries {
                writeln!(file, "{entry}")
                    .map_err(|error| format!("could not write history: {error}"))?;
            }
            file.sync_all()
                .map_err(|error| format!("could not sync history: {error}"))?;
        }
        fs::rename(&temp_path, &self.path).map_err(|error| {
            let _ = fs::remove_file(&temp_path);
            format!("could not replace {}: {error}", self.path.display())
        })?;
        Ok(())
    }
}

fn temporary_history_path(path: &Path) -> PathBuf {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    PathBuf::from(temporary)
}

#[derive(Helper, rustyline::Completer, Hinter, Highlighter, Validator)]
struct OpshHelper {
    #[rustyline(Completer)]
    completer: OpshCompleter,
    #[rustyline(Hinter)]
    hinter: HistoryHinter,
}

struct OpshCompleter {
    files: FilenameCompleter,
}

impl Completer for OpshCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let before = &line[..pos];
        let start = before
            .rfind(|character: char| character.is_whitespace())
            .map(|index| index + 1)
            .unwrap_or(0);
        let prefix = &before[start..];
        let is_first_word = before[..start].chars().all(char::is_whitespace);

        if is_first_word {
            if prefix.starts_with('.') || prefix.starts_with('/') || prefix.starts_with('~') {
                return self.files.complete(line, pos, ctx);
            }

            let mut matches = BUILTINS
                .iter()
                .filter(|name| name.starts_with(prefix) && **name != ".")
                .map(|name| Pair {
                    display: (*name).to_owned(),
                    replacement: (*name).to_owned(),
                })
                .collect::<Vec<_>>();

            for name in path_executables_matching(prefix) {
                if matches
                    .iter()
                    .any(|candidate| candidate.replacement == name)
                {
                    continue;
                }
                matches.push(Pair {
                    display: name.clone(),
                    replacement: name,
                });
            }

            matches.sort_by(|left, right| left.replacement.cmp(&right.replacement));
            if !matches.is_empty() {
                return Ok((start, matches));
            }
        }

        self.files.complete(line, pos, ctx)
    }
}

impl Shell {
    pub fn new(history: History) -> Self {
        let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
        let color = interactive
            && env::var_os("NO_COLOR").is_none()
            && env::var("TERM").is_ok_and(|term| term != "dumb");
        Self {
            history,
            interactive,
            color,
            previous_dir: None,
            directory_stack: Vec::new(),
            last_status: 0,
            source_depth: 0,
        }
    }

    pub fn repl(&mut self) -> Result<i32, String> {
        if self.interactive {
            self.banner();
            return self.interactive_repl();
        }
        self.batch_repl()
    }

    fn interactive_repl(&mut self) -> Result<i32, String> {
        let config = Config::builder().history_ignore_space(true).build();
        let helper = OpshHelper {
            completer: OpshCompleter {
                files: FilenameCompleter::new(),
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
                    let _ = editor.add_history_entry(line.as_str());
                    match self.execute(&line) {
                        Ok(Flow::Continue(code)) => self.last_status = code,
                        Ok(Flow::Exit(code)) => {
                            self.save_history();
                            return Ok(code);
                        }
                        Err(error) => {
                            self.last_status = 1;
                            eprintln!("{}opsh:{} {error}", self.paint(RED), self.paint(RESET));
                        }
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C");
                    self.last_status = 130;
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
        self.history.push(command);
        self.dispatch(command)
    }

    fn dispatch(&mut self, command: &str) -> Result<Flow, String> {
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
            "history" => self.history(),
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
            "source" | "." => self.source(words.get(1).map(String::as_str)),
            "repeat" => self.repeat(command),
            "time" => self.time(command),
            "clear" => self.clear(),
            "help" => self.help(),
            "about" => self.about(),
            "exit" => Ok(Flow::Exit(parse_exit_code(words.get(1))?)),
            _ => self.external(command),
        }
    }

    fn cd(&mut self, argument: Option<&str>) -> Result<Flow, String> {
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

    fn pwd(&self) -> Result<Flow, String> {
        println!(
            "{}",
            env::current_dir()
                .map_err(|error| error.to_string())?
                .display()
        );
        Ok(Flow::Continue(0))
    }

    fn pushd(&mut self, argument: Option<&str>) -> Result<Flow, String> {
        let current = env::current_dir().map_err(|error| error.to_string())?;
        self.cd(argument)?;
        self.directory_stack.push(current);
        self.dirs()
    }

    fn popd(&mut self) -> Result<Flow, String> {
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

    fn dirs(&self) -> Result<Flow, String> {
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

    fn history(&self) -> Result<Flow, String> {
        for (index, entry) in self.history.entries.iter().enumerate() {
            println!(
                "{}{:>4}{}  {entry}",
                self.paint(DIM),
                index + 1,
                self.paint(RESET)
            );
        }
        Ok(Flow::Continue(0))
    }

    fn status(&self) -> Result<Flow, String> {
        let (color, label) = if self.last_status == 0 {
            (GREEN, "ok")
        } else {
            (RED, "failed")
        };
        println!(
            "{}{}{}  exit {}",
            self.paint(color),
            label,
            self.paint(RESET),
            self.last_status
        );
        Ok(Flow::Continue(0))
    }

    fn which(&self, command: Option<&str>) -> Result<Flow, String> {
        let command = command.ok_or("which: expected a command")?;
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

    fn path(&self) -> Result<Flow, String> {
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

    fn get(&self, key: Option<&str>) -> Result<Flow, String> {
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

    fn mkdir(&self, paths: &[String]) -> Result<Flow, String> {
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

    fn mkcd(&mut self, path: Option<&str>) -> Result<Flow, String> {
        let path = path.ok_or("mkcd: expected a directory")?;
        let expanded = expand_home(path)?;
        fs::create_dir_all(&expanded)
            .map_err(|error| format!("mkcd: {}: {error}", expanded.display()))?;
        self.cd(Some(path))
    }

    fn touch(&self, paths: &[String]) -> Result<Flow, String> {
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

    fn open(&self, path: Option<&str>) -> Result<Flow, String> {
        let path = path.ok_or("open: expected a path")?;
        let path = expand_home(path)?;
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let status = Command::new(opener)
            .arg(&path)
            .status()
            .map_err(|error| format!("open: {opener}: {error}"))?;
        Ok(Flow::Continue(status.code().unwrap_or(1)))
    }

    fn set(&self, words: &[String]) -> Result<Flow, String> {
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

    fn unset(&self, key: Option<&str>) -> Result<Flow, String> {
        let key = key.ok_or("unset: expected a variable name")?;
        if !is_valid_env_key(key) {
            return Err(format!("unset: invalid variable name: {key}"));
        }
        // The shell is single-threaded; changing its process environment is intentional here.
        unsafe { env::remove_var(key) };
        Ok(Flow::Continue(0))
    }

    fn source(&mut self, path: Option<&str>) -> Result<Flow, String> {
        if self.source_depth >= MAX_SOURCE_DEPTH {
            return Err(format!(
                "source: nested deeper than {MAX_SOURCE_DEPTH} levels"
            ));
        }
        let path = expand_home(path.ok_or("source: expected a file")?)?;
        let file =
            File::open(&path).map_err(|error| format!("source: {}: {error}", path.display()))?;
        let mut status = 0;
        self.source_depth += 1;
        let result = (|| {
            for line in io::BufReader::new(file).lines() {
                match self.execute(&line.map_err(|error| error.to_string())?)? {
                    Flow::Continue(code) => status = code,
                    Flow::Exit(code) => return Ok(Flow::Exit(code)),
                }
            }
            Ok(Flow::Continue(status))
        })();
        self.source_depth -= 1;
        result
    }

    fn repeat(&mut self, input: &str) -> Result<Flow, String> {
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

    fn time(&mut self, input: &str) -> Result<Flow, String> {
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

    fn clear(&self) -> Result<Flow, String> {
        if self.interactive {
            print!("\x1b[2J\x1b[H");
            io::stdout().flush().map_err(|error| error.to_string())?;
        }
        Ok(Flow::Continue(0))
    }

    fn help(&self) -> Result<Flow, String> {
        println!(
            "{}◆ opsh built-ins{}\n\n  {cd} [DIR]       change directory ({}cd -{} returns)\n  {pwd}            print current directory\n  {pushd} DIR      enter a directory and save the current one\n  {popd}           return to the last saved directory\n  {dirs}           show the directory stack\n  {history}        show saved commands\n  {status}         show the last exit status\n  {which} CMD      find a built-in or executable\n  {path}           print PATH entries\n  {get} NAME       print one environment variable\n  {mkdir} DIR...   create directories\n  {mkcd} DIR       create a directory and enter it\n  {touch} FILE...  create files if needed\n  {open} PATH      open with the desktop default app\n  {set} NAME VALUE set an environment variable\n  {unset} NAME     remove an environment variable\n  {source} FILE    run a local opsh file\n  {repeat} N CMD   run a command N times\n  {time} CMD       run a command and show elapsed time\n  {clear}          clear the screen\n  {about}          show project information\n  {exit} [N]       leave opsh\n\n{}&& || ; chains run inside opsh (so cd persists). Pipes and redirects use\n/bin/sh (or $OPSH_SHELL / a non-fish $SHELL), never fish built-ins.{}",
            self.paint(BOLD),
            self.paint(RESET),
            self.paint(DIM),
            self.paint(RESET),
            self.paint(DIM),
            self.paint(RESET),
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
            source = self.command("source"),
            repeat = self.command("repeat"),
            time = self.command("time"),
            clear = self.command("clear"),
            about = self.command("about"),
            exit = self.command("exit")
        );
        Ok(Flow::Continue(0))
    }

    fn about(&self) -> Result<Flow, String> {
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
            "{}◆ opsh{}  {}local shell{}\n  {}built-ins{}  cd · mkcd · which · source · help\n",
            self.paint(BOLD),
            self.paint(RESET),
            self.paint(DIM),
            self.paint(RESET),
            self.paint(VIOLET),
            self.paint(RESET)
        );
    }

    fn prompt_pair(&self) -> (String, String) {
        let directory = env::current_dir()
            .ok()
            .and_then(|path| compact_path(&path))
            .unwrap_or_else(|| "?".into());
        let raw_status = if self.last_status == 0 {
            String::new()
        } else {
            format!(" ×{}", self.last_status)
        };
        let styled_status = if self.last_status == 0 {
            String::new()
        } else {
            format!(
                " {}×{}{}",
                self.paint(RED),
                self.last_status,
                self.paint(RESET)
            )
        };
        let mark = if self.last_status == 0 { GREEN } else { RED };
        let raw = format!("◆ {directory}{raw_status}\n› ");
        let styled = format!(
            "{}◆{} {}{}{}{}\n{}›{} ",
            self.paint(mark),
            self.paint(RESET),
            self.paint(BLUE),
            directory,
            self.paint(RESET),
            styled_status,
            self.paint(YELLOW),
            self.paint(RESET)
        );
        (raw, styled)
    }

    fn command(&self, name: &str) -> String {
        format!("{}{}{}", self.paint(CYAN), name, self.paint(RESET))
    }

    fn save_history(&self) {
        if let Err(error) = self.history.save() {
            if self.interactive {
                eprintln!(
                    "{}opsh:{} history disabled: {error}",
                    self.paint(YELLOW),
                    self.paint(RESET)
                );
            }
        }
    }

    fn paint(&self, code: &'static str) -> &'static str {
        if self.color { code } else { "" }
    }
}

#[derive(Debug)]
enum Flow {
    Continue(i32),
    Exit(i32),
}

impl Flow {
    fn code(self) -> i32 {
        match self {
            Self::Continue(code) | Self::Exit(code) => code,
        }
    }
}

fn split_words(command: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    while let Some(character) = chars.next() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        match character {
            '\\' if !in_single => {
                if in_double {
                    match chars.peek() {
                        Some('"' | '\\' | '$' | '`') => escaped = true,
                        _ => current.push('\\'),
                    }
                } else {
                    escaped = true;
                }
            }
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            character if character.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
        }
    }

    if in_single || in_double {
        return Err("unclosed quote".into());
    }
    if escaped {
        return Err("trailing backslash".into());
    }
    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

fn command_tail(input: &str, skip_words: usize) -> Option<&str> {
    let trimmed = input.trim_start();
    let mut rest = trimmed;
    for _ in 0..skip_words {
        rest = skip_one_word(rest)?;
        rest = rest.trim_start();
    }
    if rest.is_empty() { None } else { Some(rest) }
}

fn skip_one_word(input: &str) -> Option<&str> {
    let mut chars = input.char_indices().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut started = false;

    while let Some((index, character)) = chars.next() {
        if escaped {
            escaped = false;
            started = true;
            continue;
        }
        match character {
            '\\' if !in_single => {
                escaped = true;
                started = true;
            }
            '\'' if !in_double => {
                in_single = !in_single;
                started = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                started = true;
            }
            character if character.is_whitespace() && !in_single && !in_double => {
                if started {
                    return Some(&input[index..]);
                }
            }
            _ => started = true,
        }
    }

    if started && !in_single && !in_double && !escaped {
        Some("")
    } else {
        None
    }
}

fn parse_exit_code(value: Option<&String>) -> Result<i32, String> {
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

fn compact_path(path: &Path) -> Option<String> {
    let home = env::var_os("HOME").map(PathBuf::from);
    if home.as_ref().is_some_and(|home| path == home) {
        return Some("~".into());
    }
    if let Some(relative) =
        home.and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf))
    {
        return Some(format!("~/{}", relative.display()));
    }
    Some(path.display().to_string())
}

fn is_builtin(command: &str) -> bool {
    BUILTINS.binary_search(&command).is_ok()
}

/// Shell used for external / compound command lines.
///
/// Fish is skipped on purpose: its built-ins are not part of opsh. Real apps and
/// POSIX syntax go through `/bin/sh`, unless `OPSH_SHELL` or a non-fish `$SHELL`
/// is set.
fn command_shell() -> String {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChainOp {
    And,
    Or,
    Seq,
}

fn has_chain_operators(command: &str) -> bool {
    scan_operators(command).any(|operator| {
        matches!(
            operator,
            ScannedOperator::And | ScannedOperator::Or | ScannedOperator::Seq
        )
    })
}

/// Pipes, redirects, subshells and background jobs still need a POSIX shell.
fn needs_posix_shell(command: &str) -> bool {
    scan_operators(command).any(|operator| {
        matches!(
            operator,
            ScannedOperator::Pipe
                | ScannedOperator::Redirect
                | ScannedOperator::Substitution
                | ScannedOperator::Subshell
                | ScannedOperator::Background
        )
    })
}

#[derive(Clone, Copy)]
enum ScannedOperator {
    And,
    Or,
    Seq,
    Pipe,
    Redirect,
    Substitution,
    Subshell,
    Background,
}

fn scan_operators(command: &str) -> impl Iterator<Item = ScannedOperator> + '_ {
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut previous = None::<char>;
    std::iter::from_fn(move || {
        while let Some(character) = chars.next() {
            if escaped {
                escaped = false;
                previous = Some(character);
                continue;
            }
            let operator = !in_single && !in_double;
            let found = match character {
                '\\' if !in_single => {
                    escaped = true;
                    None
                }
                '\'' if !in_double => {
                    in_single = !in_single;
                    None
                }
                '"' if !in_single => {
                    in_double = !in_double;
                    None
                }
                '|' if operator => {
                    if chars.peek() == Some(&'|') {
                        chars.next();
                        Some(ScannedOperator::Or)
                    } else {
                        Some(ScannedOperator::Pipe)
                    }
                }
                ';' if operator => Some(ScannedOperator::Seq),
                '<' | '>' if operator => Some(ScannedOperator::Redirect),
                '`' if operator => Some(ScannedOperator::Substitution),
                '&' if operator => {
                    if chars.peek() == Some(&'&') {
                        chars.next();
                        Some(ScannedOperator::And)
                    } else {
                        let at_token_boundary = previous.is_none_or(char::is_whitespace);
                        let next_is_boundary = chars.peek().is_none_or(|next| next.is_whitespace());
                        if at_token_boundary || next_is_boundary {
                            Some(ScannedOperator::Background)
                        } else {
                            None
                        }
                    }
                }
                '$' if operator && chars.peek() == Some(&'(') => {
                    Some(ScannedOperator::Substitution)
                }
                '(' if operator => Some(ScannedOperator::Subshell),
                _ => None,
            };
            previous = Some(character);
            if found.is_some() {
                return found;
            }
        }
        None
    })
}

fn split_chain(command: &str) -> Result<(Vec<String>, Vec<ChainOp>), String> {
    let mut segments = Vec::new();
    let mut operators = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    while let Some(character) = chars.next() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        let operator = !in_single && !in_double;
        match character {
            '\\' if !in_single => {
                current.push('\\');
                escaped = true;
            }
            '\'' if !in_double => {
                in_single = !in_single;
                current.push('\'');
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push('"');
            }
            '&' if operator && chars.peek() == Some(&'&') => {
                chars.next();
                push_chain_segment(&mut segments, &mut current)?;
                operators.push(ChainOp::And);
            }
            '|' if operator && chars.peek() == Some(&'|') => {
                chars.next();
                push_chain_segment(&mut segments, &mut current)?;
                operators.push(ChainOp::Or);
            }
            ';' if operator => {
                push_chain_segment(&mut segments, &mut current)?;
                operators.push(ChainOp::Seq);
            }
            _ => current.push(character),
        }
    }

    if in_single || in_double {
        return Err("unclosed quote".into());
    }
    push_chain_segment(&mut segments, &mut current)?;
    if segments.len() != operators.len() + 1 {
        return Err("invalid command chain".into());
    }
    Ok((segments, operators))
}

fn push_chain_segment(segments: &mut Vec<String>, current: &mut String) -> Result<(), String> {
    let segment = std::mem::take(current);
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return Err("empty command in chain".into());
    }
    segments.push(trimmed.to_owned());
    Ok(())
}

fn path_executables_matching(prefix: &str) -> Vec<String> {
    const MAX_MATCHES: usize = 64;
    let Some(path) = env::var_os("PATH") else {
        return Vec::new();
    };
    let mut names = std::collections::BTreeSet::new();
    for directory in env::split_paths(&path) {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() && !file_type.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.starts_with(prefix) || names.contains(name) {
                continue;
            }
            if is_executable(&entry.path()) {
                names.insert(name.to_owned());
                if names.len() >= MAX_MATCHES {
                    return names.into_iter().collect();
                }
            }
        }
    }
    names.into_iter().collect()
}

fn is_valid_env_key(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(character) if character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn find_in_path(command: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(command))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

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
    fn extracts_command_tails() {
        assert_eq!(command_tail("time echo hello", 1), Some("echo hello"));
        assert_eq!(command_tail("repeat 3 echo hello", 1), Some("3 echo hello"));
        assert_eq!(command_tail(r#"time echo "a b""#, 1), Some(r#"echo "a b""#));
        assert_eq!(command_tail("repeat", 2), None);
    }

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
    fn splits_quoted_words() {
        assert_eq!(
            split_words(r#"mkdir "my dir""#).unwrap(),
            vec!["mkdir".to_owned(), "my dir".to_owned()]
        );
        assert_eq!(
            split_words("touch 'a b' c").unwrap(),
            vec!["touch".to_owned(), "a b".to_owned(), "c".to_owned()]
        );
        assert!(split_words(r#"echo "open"#).is_err());
    }

    #[test]
    fn classifies_chain_versus_posix_operators() {
        assert!(has_chain_operators("cd /tmp && ls"));
        assert!(has_chain_operators("mkdir a; cd a"));
        assert!(has_chain_operators("false || true"));
        assert!(!has_chain_operators("echo hi | wc"));
        assert!(!needs_posix_shell("cd /tmp && ls"));
        assert!(needs_posix_shell("echo hi | wc"));
        assert!(needs_posix_shell("cat < file"));
        assert!(needs_posix_shell("echo $(pwd)"));
        assert!(!needs_posix_shell("cd /tmp"));
        assert!(!has_chain_operators("mkdir 'a && b'"));
        assert!(!needs_posix_shell(r#"echo "a|b""#));
        assert!(!needs_posix_shell("set NAME a&b"));
    }

    #[test]
    fn history_save_is_atomic() {
        let directory = std::env::temp_dir().join(format!("opsh-history-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("history");
        let mut history = History {
            path: path.clone(),
            entries: vec!["one".into(), "two".into()],
        };
        history.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        assert!(!temporary_history_path(&path).exists());
        history.push("three");
        history.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\nthree\n");
        let _ = fs::remove_dir_all(&directory);
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
}
