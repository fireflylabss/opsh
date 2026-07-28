use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_HISTORY: usize = 1_000;

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
    last_status: i32,
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
        let mut file = File::create(&self.path)
            .map_err(|error| format!("could not write {}: {error}", self.path.display()))?;
        for entry in &self.entries {
            writeln!(file, "{entry}")
                .map_err(|error| format!("could not write history: {error}"))?;
        }
        Ok(())
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
            last_status: 0,
        }
    }

    pub fn repl(&mut self) -> Result<i32, String> {
        if self.interactive {
            self.banner();
        }
        let stdin = io::stdin();
        let mut lines = stdin.lock().lines();
        loop {
            if self.interactive {
                print!("{}", self.prompt());
                io::stdout().flush().map_err(|error| error.to_string())?;
            }
            let Some(line) = lines.next() else { break };
            let line = line.map_err(|error| error.to_string())?;
            match self.execute(&line) {
                Ok(Flow::Continue(code)) => self.last_status = code,
                Ok(Flow::Exit(code)) => {
                    self.save_history();
                    return Ok(code);
                }
                Err(error) if self.interactive => {
                    self.last_status = 1;
                    eprintln!("{}opsh:{} {error}", self.paint(RED), self.paint(RESET));
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
        let words = split_words(command);
        match words.first().map(String::as_str) {
            Some("cd") => self.cd(words.get(1).map(String::as_str)),
            Some("pwd") => self.pwd(),
            Some("history") => self.history(),
            Some("status") => self.status(),
            Some("which") => self.which(words.get(1).map(String::as_str)),
            Some("mkdir") => self.mkdir(&words[1..]),
            Some("mkcd") => self.mkcd(words.get(1).map(String::as_str)),
            Some("touch") => self.touch(&words[1..]),
            Some("open") => self.open(words.get(1).map(String::as_str)),
            Some("set") => self.set(&words[1..]),
            Some("unset") => self.unset(words.get(1).map(String::as_str)),
            Some("source") | Some(".") => self.source(words.get(1).map(String::as_str)),
            Some("clear") => self.clear(),
            Some("help") => self.help(),
            Some("about") => self.about(),
            Some("exit") => Ok(Flow::Exit(parse_exit_code(words.get(1))?)),
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
        let path = expand_home(path.ok_or("source: expected a file")?)?;
        let file =
            File::open(&path).map_err(|error| format!("source: {}: {error}", path.display()))?;
        let mut status = 0;
        for line in io::BufReader::new(file).lines() {
            match self.execute(&line.map_err(|error| error.to_string())?)? {
                Flow::Continue(code) => status = code,
                Flow::Exit(code) => return Ok(Flow::Exit(code)),
            }
        }
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
            "{}◆ opsh built-ins{}\n\n  {cd} [DIR]       change directory ({}cd -{} returns)\n  {pwd}            print current directory\n  {history}        show saved commands\n  {status}         show the last exit status\n  {which} CMD      find a built-in or executable\n  {mkdir} DIR...   create directories\n  {mkcd} DIR       create a directory and enter it\n  {touch} FILE...  create files if needed\n  {open} PATH      open with the desktop default app\n  {set} NAME VALUE set an environment variable\n  {unset} NAME     remove an environment variable\n  {source} FILE    run a local opsh file\n  {clear}          clear the screen\n  {about}          show project information\n  {exit} [N]       leave opsh\n\n{}External commands are executed by $SHELL, with normal pipes and redirects.{}",
            self.paint(BOLD),
            self.paint(RESET),
            self.paint(DIM),
            self.paint(RESET),
            self.paint(DIM),
            self.paint(RESET),
            cd = self.command("cd"),
            pwd = self.command("pwd"),
            history = self.command("history"),
            status = self.command("status"),
            which = self.command("which"),
            mkdir = self.command("mkdir"),
            mkcd = self.command("mkcd"),
            touch = self.command("touch"),
            open = self.command("open"),
            set = self.command("set"),
            unset = self.command("unset"),
            source = self.command("source"),
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
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let status = Command::new(shell)
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

    fn prompt(&self) -> String {
        let directory = env::current_dir()
            .ok()
            .and_then(|path| compact_path(&path))
            .unwrap_or_else(|| "?".into());
        let (mark, status) = if self.last_status == 0 {
            (GREEN, String::new())
        } else {
            (
                RED,
                format!(
                    " {}×{}{}",
                    self.paint(RED),
                    self.last_status,
                    self.paint(RESET)
                ),
            )
        };
        format!(
            "{}◆{} {}{}{}{}\n{}›{} ",
            self.paint(mark),
            self.paint(RESET),
            self.paint(BLUE),
            directory,
            self.paint(RESET),
            status,
            self.paint(YELLOW),
            self.paint(RESET)
        )
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

enum Flow {
    Continue(i32),
    Exit(i32),
}

fn split_words(command: &str) -> Vec<String> {
    command.split_whitespace().map(str::to_owned).collect()
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
    matches!(
        command,
        "cd" | "pwd"
            | "history"
            | "status"
            | "which"
            | "mkdir"
            | "mkcd"
            | "touch"
            | "open"
            | "set"
            | "unset"
            | "source"
            | "."
            | "clear"
            | "help"
            | "about"
            | "exit"
    )
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
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!is_builtin("ls"));
    }
}
