use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("opsh-cli-{label}-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_opsh"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("OPSH_") {
                command.env_remove(&key);
            }
        }
        command
            .current_dir(&self.root)
            .env("HOME", &self.root)
            .env("OPTION_HOME", self.root.join("option"))
            .env("OPSH_HISTORY", self.root.join("history"))
            .env("OPSH_RC", self.root.join("rc"))
            .env("OPSH_SHELL", "/bin/sh")
            .env("NO_COLOR", "1")
            .env("TERM", "dumb");
        command
    }

    fn run(&self, script: &str) -> Output {
        self.command()
            .args(["-c", script])
            .output()
            .expect("opsh binary should run")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("process should exit normally")
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}

#[test]
fn exit_builtin_sets_exit_code() {
    let sandbox = Sandbox::new("exit");
    assert_eq!(code(&sandbox.run("exit 3")), 3);
    assert_eq!(code(&sandbox.run("exit")), 0);
    let invalid = sandbox.run("exit nope");
    assert_eq!(code(&invalid), 1);
    assert!(stderr(&invalid).contains("exit: invalid status"));
}

#[test]
fn external_command_status_passes_through() {
    let sandbox = Sandbox::new("external");
    assert_eq!(code(&sandbox.run("true")), 0);
    assert_eq!(code(&sandbox.run("false")), 1);
    assert_eq!(code(&sandbox.run("sh -c 'exit 7'")), 7);
    let output = sandbox.run("echo hello world");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "hello world\n");
}

#[test]
fn and_chain_short_circuits() {
    let sandbox = Sandbox::new("and");
    let output = sandbox.run("true && echo yes");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "yes\n");

    let output = sandbox.run("false && echo yes");
    assert_eq!(code(&output), 1);
    assert_eq!(stdout(&output), "");
}

#[test]
fn or_chain_runs_on_failure() {
    let sandbox = Sandbox::new("or");
    let output = sandbox.run("false || echo fallback");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "fallback\n");

    let output = sandbox.run("true || echo fallback");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "");
}

#[test]
fn seq_chain_runs_everything_and_reports_last_status() {
    let sandbox = Sandbox::new("seq");
    let output = sandbox.run("echo one; false; echo two");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "one\ntwo\n");

    let output = sandbox.run("echo one; false");
    assert_eq!(code(&output), 1);
    assert_eq!(stdout(&output), "one\n");
}

#[test]
fn mixed_chain_respects_operator_order() {
    let sandbox = Sandbox::new("mixed");
    let output = sandbox.run("false && echo a || echo b; echo c");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "b\nc\n");
}

#[test]
fn exit_inside_chain_stops_the_chain() {
    let sandbox = Sandbox::new("chain-exit");
    let output = sandbox.run("echo before; exit 4; echo after");
    assert_eq!(code(&output), 4);
    assert_eq!(stdout(&output), "before\n");
}

#[test]
fn cd_persists_within_chain() {
    let sandbox = Sandbox::new("cd");
    let nested = sandbox.path("nested");
    fs::create_dir_all(&nested).unwrap();

    let output = sandbox.run("cd nested && pwd");
    assert_eq!(code(&output), 0);
    assert_eq!(
        PathBuf::from(stdout(&output).trim_end()),
        canonical(&nested)
    );

    let output = sandbox.run("cd nested && touch inside && cd .. && pwd");
    assert_eq!(code(&output), 0);
    assert!(nested.join("inside").is_file());
    let printed = stdout(&output);
    let mut lines = printed.lines();
    assert_eq!(lines.next(), Some("touched inside"));
    assert_eq!(
        PathBuf::from(lines.next().unwrap()),
        canonical(&sandbox.root)
    );
    assert_eq!(lines.next(), None);
}

#[test]
fn cd_dash_returns_to_previous_directory() {
    let sandbox = Sandbox::new("cd-dash");
    fs::create_dir_all(sandbox.path("a")).unwrap();
    let output = sandbox.run("cd a && cd - && pwd");
    assert_eq!(code(&output), 0);
    assert_eq!(
        PathBuf::from(stdout(&output).trim_end()),
        canonical(&sandbox.root)
    );
}

#[test]
fn cd_to_missing_directory_fails() {
    let sandbox = Sandbox::new("cd-missing");
    let output = sandbox.run("cd does-not-exist && echo reached");
    assert_eq!(code(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).contains("cd:"));
}

#[test]
fn pipes_pass_through_to_posix_shell() {
    let sandbox = Sandbox::new("pipe");
    let output = sandbox.run("printf 'a\\nb\\nc\\n' | wc -l");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output).trim(), "3");

    let output = sandbox.run("echo hello | tr a-z A-Z");
    assert_eq!(stdout(&output), "HELLO\n");
}

#[test]
fn redirects_pass_through_to_posix_shell() {
    let sandbox = Sandbox::new("redirect");
    let output = sandbox.run("echo written > out.txt");
    assert_eq!(code(&output), 0);
    assert_eq!(
        fs::read_to_string(sandbox.path("out.txt")).unwrap(),
        "written\n"
    );

    fs::write(sandbox.path("in.txt"), "from-file\n").unwrap();
    let output = sandbox.run("cat < in.txt");
    assert_eq!(stdout(&output), "from-file\n");
}

#[test]
fn subshell_and_substitution_pass_through() {
    let sandbox = Sandbox::new("subshell");
    let output = sandbox.run("echo $(echo nested)");
    assert_eq!(stdout(&output), "nested\n");

    let output = sandbox.run("(exit 5)");
    assert_eq!(code(&output), 5);
}

#[test]
fn set_get_unset_manage_environment() {
    let sandbox = Sandbox::new("env");
    let output = sandbox.run("set OPSH_TEST_VAR hello && get OPSH_TEST_VAR");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "OPSH_TEST_VAR=hello\n");

    let output = sandbox.run("set OPSH_TEST_VAR a b c && get OPSH_TEST_VAR");
    assert_eq!(stdout(&output), "OPSH_TEST_VAR=a b c\n");

    let output = sandbox.run("set OPSH_TEST_VAR hello && sh -c 'echo $OPSH_TEST_VAR'");
    assert_eq!(stdout(&output), "hello\n");

    let output = sandbox.run("set OPSH_TEST_VAR hello && unset OPSH_TEST_VAR && get OPSH_TEST_VAR");
    assert_eq!(code(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).contains("get: OPSH_TEST_VAR: not set"));
}

#[test]
fn set_and_get_reject_invalid_names() {
    let sandbox = Sandbox::new("env-invalid");
    let output = sandbox.run("set 1BAD value");
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("set: invalid variable name: 1BAD"));

    let output = sandbox.run("get NOT-VALID");
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("get: invalid variable name"));

    let output = sandbox.run("set ONLY_NAME");
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("set: usage: set NAME VALUE"));
}

#[test]
fn alias_expands_first_word_and_keeps_arguments() {
    let sandbox = Sandbox::new("alias");
    let output = sandbox.run("alias say=echo && say hello there");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "hello there\n");

    let output = sandbox.run("alias say echo prefix && say tail");
    assert_eq!(stdout(&output), "prefix tail\n");

    let output = sandbox.run("alias fail=false && fail || echo recovered");
    assert_eq!(stdout(&output), "recovered\n");
}

#[test]
fn alias_lists_and_removes_definitions() {
    let sandbox = Sandbox::new("alias-list");
    let output = sandbox.run("alias ll='ls -la' && alias g=git && alias");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "alias g=git\nalias ll='ls -la'\n");

    let output = sandbox.run("alias ll='ls -la' && alias ll");
    assert_eq!(stdout(&output), "alias ll='ls -la'\n");

    let output = sandbox.run("alias missing");
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("alias: missing: not found"));

    let output = sandbox.run("alias say=echo && unalias say && which say");
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("which: say: not found"));

    let output = sandbox.run("alias say=echo && which say");
    assert_eq!(stdout(&output), "say is aliased to `echo`\n");
}

#[test]
fn history_is_written_to_opsh_history() {
    let sandbox = Sandbox::new("history");
    let output = sandbox.run("echo first");
    assert_eq!(code(&output), 0);
    assert_eq!(
        fs::read_to_string(sandbox.path("history")).unwrap(),
        "echo first\n"
    );
    assert!(!sandbox.path("history.tmp").exists());

    let output = sandbox.run("echo second");
    assert_eq!(code(&output), 0);
    assert_eq!(
        fs::read_to_string(sandbox.path("history")).unwrap(),
        "echo first\necho second\n"
    );
}

#[test]
fn history_bang_refs_expand_from_saved_history() {
    let sandbox = Sandbox::new("bang");
    fs::write(sandbox.path("history"), "echo alpha\necho beta\n").unwrap();
    let output = sandbox.run("!!");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "echo beta\nbeta\n");

    let output = sandbox.run("!1");
    assert_eq!(stdout(&output), "echo alpha\nalpha\n");

    let output = sandbox.run("!9");
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("history only has"));
}

#[test]
fn status_and_which_builtins() {
    let sandbox = Sandbox::new("which");
    let output = sandbox.run("which cd");
    assert_eq!(stdout(&output), "cd is a shell built-in\n");

    let output = sandbox.run("which sh");
    assert_eq!(code(&output), 0);
    assert!(stdout(&output).trim_end().ends_with("/sh"));

    let output = sandbox.run("which definitely-not-a-command-xyz");
    assert_eq!(code(&output), 1);
}

#[test]
fn unknown_option_and_missing_command_fail() {
    let sandbox = Sandbox::new("args");
    let output = sandbox.command().arg("--bogus").output().unwrap();
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("unknown option: --bogus"));

    let output = sandbox.command().arg("-c").output().unwrap();
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("missing command after -c"));
}

#[test]
fn version_and_help_flags() {
    let sandbox = Sandbox::new("flags");
    let output = sandbox.command().arg("--version").output().unwrap();
    assert_eq!(code(&output), 0);
    assert_eq!(
        stdout(&output),
        format!("opsh {}\n", env!("CARGO_PKG_VERSION"))
    );

    let output = sandbox.command().arg("--help").output().unwrap();
    assert_eq!(code(&output), 0);
    assert!(stdout(&output).contains("USAGE:"));
    assert!(stdout(&output).contains("opsh doctor [--json]"));
}

#[test]
fn doctor_json_has_expected_shape() {
    let sandbox = Sandbox::new("doctor");
    let output = sandbox
        .command()
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(code(&output), 0);
    let json = stdout(&output);
    assert_eq!(json.lines().count(), 1, "{json}");
    let fields = parse_flat_json_object(json.trim_end())
        .unwrap_or_else(|error| panic!("doctor --json produced invalid JSON ({error}): {json}"));

    let keys: Vec<&str> = fields.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "state_dir",
            "state_ok",
            "history",
            "rc",
            "shell",
            "shell_ok"
        ]
    );
    let field = |name: &str| {
        fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
            .unwrap()
    };
    assert!(matches!(field("state_dir"), JsonValue::String(_)));
    assert_eq!(field("state_ok"), &JsonValue::Bool(true));
    assert_eq!(
        field("history"),
        &JsonValue::String(sandbox.path("history").display().to_string())
    );
    assert_eq!(
        field("rc"),
        &JsonValue::String(sandbox.path("rc").display().to_string())
    );
    assert_eq!(field("shell"), &JsonValue::String("/bin/sh".into()));
    assert_eq!(field("shell_ok"), &JsonValue::Bool(true));
}

#[test]
fn doctor_json_escapes_special_characters_in_paths() {
    let sandbox = Sandbox::new("doctor-escape");
    let rc = sandbox.path(r#"quote"back\slash"#);
    let output = sandbox
        .command()
        .env("OPSH_RC", &rc)
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(code(&output), 0);
    let json = stdout(&output);
    let fields = parse_flat_json_object(json.trim_end())
        .unwrap_or_else(|error| panic!("doctor --json produced invalid JSON ({error}): {json}"));
    let rc_field = fields.iter().find(|(key, _)| key == "rc").map(|(_, v)| v);
    assert_eq!(rc_field, Some(&JsonValue::String(rc.display().to_string())));
}

#[derive(Debug, PartialEq, Eq)]
enum JsonValue {
    String(String),
    Bool(bool),
}

/// Strict parser for a single-line flat JSON object of string/bool values.
fn parse_flat_json_object(input: &str) -> Result<Vec<(String, JsonValue)>, String> {
    let mut chars = input.chars().peekable();
    let mut fields = Vec::new();

    fn parse_string(
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ) -> Result<String, String> {
        if chars.next() != Some('"') {
            return Err("expected opening quote".into());
        }
        let mut value = String::new();
        loop {
            match chars.next() {
                None => return Err("unterminated string".into()),
                Some('"') => return Ok(value),
                Some('\\') => match chars.next() {
                    Some('"') => value.push('"'),
                    Some('\\') => value.push('\\'),
                    Some('/') => value.push('/'),
                    Some('n') => value.push('\n'),
                    Some('t') => value.push('\t'),
                    Some('r') => value.push('\r'),
                    Some('b') => value.push('\u{8}'),
                    Some('f') => value.push('\u{c}'),
                    Some('u') => {
                        let hex: String = chars.by_ref().take(4).collect();
                        let code = u32::from_str_radix(&hex, 16)
                            .map_err(|_| format!("bad \\u escape: {hex:?}"))?;
                        value.push(char::from_u32(code).ok_or("bad \\u code point")?);
                    }
                    other => return Err(format!("bad escape: {other:?}")),
                },
                Some(control) if (control as u32) < 0x20 => {
                    return Err(format!("unescaped control character {control:?}"));
                }
                Some(character) => value.push(character),
            }
        }
    }

    if chars.next() != Some('{') {
        return Err("expected '{'".into());
    }
    loop {
        let key = parse_string(&mut chars)?;
        if chars.next() != Some(':') {
            return Err(format!("expected ':' after key {key:?}"));
        }
        let value = match chars.peek() {
            Some('"') => JsonValue::String(parse_string(&mut chars)?),
            Some('t') => {
                let word: String = chars.by_ref().take(4).collect();
                if word != "true" {
                    return Err(format!("bad literal {word:?}"));
                }
                JsonValue::Bool(true)
            }
            Some('f') => {
                let word: String = chars.by_ref().take(5).collect();
                if word != "false" {
                    return Err(format!("bad literal {word:?}"));
                }
                JsonValue::Bool(false)
            }
            other => return Err(format!("unsupported value start {other:?} for {key:?}")),
        };
        fields.push((key, value));
        match chars.next() {
            Some(',') => continue,
            Some('}') => break,
            other => return Err(format!("expected ',' or '}}', got {other:?}")),
        }
    }
    if chars.next().is_some() {
        return Err("trailing characters after object".into());
    }
    Ok(fields)
}

#[test]
fn doctor_text_output_and_argument_validation() {
    let sandbox = Sandbox::new("doctor-text");
    let output = sandbox.command().arg("doctor").output().unwrap();
    assert_eq!(code(&output), 0);
    let text = stdout(&output);
    assert!(text.contains("doctor"));
    assert!(text.contains("state dir:"));
    assert!(text.contains("history:"));
    assert!(text.contains("rc:"));
    assert!(text.contains("shell: /bin/sh (ok)"));

    let output = sandbox
        .command()
        .args(["doctor", "extra"])
        .output()
        .unwrap();
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("usage: opsh doctor [--json]"));
}

#[test]
fn quoting_errors_are_reported() {
    let sandbox = Sandbox::new("quotes");
    let output = sandbox.run("echo \"unclosed");
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("unclosed quote"));

    let output = sandbox.run("echo a && ");
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("empty command in chain"));
}
