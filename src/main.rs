mod shell;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use option_sdk::{App, migrate_file};
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
    let raw: Vec<String> = env::args().skip(1).collect();
    if let Some(first) = raw.first() {
        if first == "doctor" {
            let json = raw.iter().any(|a| a == "--json");
            if raw.iter().any(|a| a != "doctor" && a != "--json") {
                return Err("usage: opsh doctor [--json]".into());
            }
            return cmd_doctor(json).map(|_| 0);
        }
    }
    let mut args = raw.into_iter();
    let mut command = None;
    let mut quiet = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--command" => {
                command = Some(args.next().ok_or("missing command after -c")?);
            }
            "-q" | "--quiet" => quiet = true,
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
    let mut shell = Shell::with_options(history, quiet);
    match command {
        Some(command) => shell.run_command(&command),
        None => shell.repl(),
    }
}

fn has_binary(name: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    for dir in env::split_paths(&paths) {
        if dir.join(name).is_file() {
            return true;
        }
    }
    false
}

fn cmd_doctor(json: bool) -> Result<(), String> {
    let _ = App::OPSH.ensure();
    let dir = App::OPSH.dir();
    let dir_ok = dir.is_dir();
    let history = history_path();
    let rc = env::var_os("OPSH_RC")
        .map(PathBuf::from)
        .unwrap_or_else(|| App::OPSH.path("rc"));
    let sh_bin = env::var("OPSH_SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| env::var("SHELL").ok().filter(|s| !s.contains("fish")))
        .unwrap_or_else(|| "/bin/sh".to_string());
    let sh_ok = has_binary(&sh_bin) || PathBuf::from(&sh_bin).is_file() || sh_bin == "/bin/sh";
    if json {
        println!(
            "{{\"state_dir\":\"{}\",\"state_ok\":{dir_ok},\"history\":\"{}\",\"rc\":\"{}\",\"shell\":\"{sh_bin}\",\"shell_ok\":{sh_ok}}}",
            dir.display(),
            history.display(),
            rc.display(),
        );
        return Ok(());
    }
    println!("{} doctor", App::OPSH.mark());
    println!(
        "  state dir: {} ({})",
        dir.display(),
        if dir_ok { "ok" } else { "missing" }
    );
    println!("  history: {}", history.display());
    println!("  rc: {}", rc.display());
    println!(
        "  shell: {sh_bin} ({})",
        if sh_ok { "ok" } else { "missing" }
    );
    Ok(())
}

fn history_path() -> PathBuf {
    if let Some(path) = env::var_os("OPSH_HISTORY") {
        return PathBuf::from(path);
    }

    let _ = App::OPSH.ensure();
    let canonical = App::OPSH.path("history");

    let legacy = env::var_os("XDG_STATE_HOME")
        .map(|dir| PathBuf::from(dir).join("opsh/history"))
        .or_else(|| {
            env::var_os("HOME").map(|dir| PathBuf::from(dir).join(".local/state/opsh/history"))
        });
    if let Some(legacy) = legacy {
        let _ = migrate_file(&legacy, &canonical);
    }

    canonical
}

fn print_help() {
    println!(
        "opsh — small local shell\n\nUSAGE:\n    opsh [OPTIONS]\n    opsh doctor [--json]\n\nOPTIONS:\n    -c, --command <COMMAND>  Run one command and exit\n    -q, --quiet              Hide the startup banner\n    -h, --help               Print this help\n    -V, --version            Print version\n\nBUILT-INS:\n    cd [DIR]          change directory (cd - returns)\n    pwd               print current directory\n    pushd DIR         enter a directory and save the current one\n    popd              return to the last saved directory\n    dirs              show the directory stack\n    history [N|clear|QUERY]  list, trim, clear or search history\n    status            show the last exit status\n    which CMD         find a built-in or executable\n    path              print PATH entries\n    get NAME          print one environment variable\n    mkdir DIR...      create directories\n    mkcd DIR          create a directory and enter it\n    touch FILE...     create files if needed\n    open PATH         open with the desktop default app\n    set NAME VALUE    set an environment variable\n    unset NAME        remove an environment variable\n    alias [NAME[=V]]  list or define aliases\n    unalias NAME      remove aliases\n    config            show active UI / config knobs\n    source FILE       run a local opsh file\n    repeat N CMD      run a command N times\n    time CMD          run a command and show elapsed time\n    clear             clear the screen\n    about             show project information\n    help              show shell help\n    exit [N]          leave opsh\n\nHISTORY:\n    !!                expand to the previous command\n    !N                expand to history entry N\n    Ctrl+R            incremental history search (interactive)\n\nCONFIG (via environment or set in ~/.option/opsh/rc):\n    OPSH_PROMPT         prompt template with {{cwd}} {{git}} {{elapsed}} {{status}} {{mark}} {{prompt}} {{stack}}\n    OPSH_PROMPT_STYLE   double (default) or single\n    OPSH_BANNER         0/false/off to hide the startup banner\n    OPSH_COLOR_OK/ERR/PATH/MARK/ACCENT   ANSI codes (e.g. 38;5;114)\n    OPSH_RC OPSH_HISTORY OPSH_SHELL\n\nInteractive sessions load ~/.option/opsh/rc (or $OPSH_RC). External commands\nrun through /bin/sh (or $OPSH_SHELL / a non-fish $SHELL). && || ; chains stay\ninside opsh so cd persists; pipes and redirects use that POSIX shell."
    );
}
