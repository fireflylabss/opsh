mod shell;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use shell::{History, Shell};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("opsh: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<i32, String> {
    let mut args = env::args().skip(1);
    let mut command = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--command" => {
                command = Some(args.next().ok_or("missing command after -c")?);
            }
            "-h" | "--help" => {
                print_help();
                return Ok(0);
            }
            "-V" | "--version" => {
                println!("opsh {}", env!("CARGO_PKG_VERSION"));
                return Ok(0);
            }
            _ => return Err(format!("unknown option: {arg}")),
        }
    }

    let history = History::open(history_path())?;
    let mut shell = Shell::new(history);
    match command {
        Some(command) => shell.run_command(&command),
        None => shell.repl(),
    }
}

fn history_path() -> PathBuf {
    env::var_os("OPSH_HISTORY")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("XDG_STATE_HOME").map(|dir| PathBuf::from(dir).join("opsh/history"))
        })
        .or_else(|| {
            env::var_os("HOME").map(|dir| PathBuf::from(dir).join(".local/state/opsh/history"))
        })
        .unwrap_or_else(|| PathBuf::from(".opsh_history"))
}

fn print_help() {
    println!(
        "opsh — small local shell\n\nUSAGE:\n    opsh [OPTIONS]\n\nOPTIONS:\n    -c, --command <COMMAND>  Run one command and exit\n    -h, --help               Print this help\n    -V, --version            Print version\n\nBUILT-INS:\n    cd [DIR]   change directory\n    pwd        print current directory\n    history    show saved commands\n    clear      clear the screen\n    help       show shell help\n    exit [N]   leave opsh\n\nExternal commands run through $SHELL (or /bin/sh), so pipes, redirects and\nvariables work normally. History is saved locally in $XDG_STATE_HOME/opsh."
    );
}
