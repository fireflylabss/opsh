use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::shell::{Shell, is_truthy_env};

pub(crate) const RESET: &str = "\x1b[0m";
pub(crate) const BOLD: &str = "\x1b[1m";
pub(crate) const DIM: &str = "\x1b[2m";
pub(crate) const CYAN: &str = "\x1b[38;5;81m";
pub(crate) const BLUE: &str = "\x1b[38;5;75m";
pub(crate) const GREEN: &str = "\x1b[38;5;114m";
pub(crate) const YELLOW: &str = "\x1b[38;5;221m";
pub(crate) const RED: &str = "\x1b[38;5;210m";
pub(crate) const VIOLET: &str = "\x1b[38;5;183m";

impl Shell {
    pub(crate) fn prompt_pair(&self) -> (String, String) {
        let template = prompt_template();
        let raw = self.render_prompt(&template, false);
        let styled = self.render_prompt(&template, true);
        (raw, styled)
    }

    pub(crate) fn render_prompt(&self, template: &str, styled: bool) -> String {
        let cwd = env::current_dir()
            .ok()
            .and_then(|path| compact_path(&path))
            .unwrap_or_else(|| "?".into());
        let cwd_full = env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "?".into());
        let status = if self.last_status == 0 {
            String::new()
        } else if styled {
            format!(
                " {}×{}{}",
                self.ansi_err(),
                self.last_status,
                self.ansi_reset()
            )
        } else {
            format!(" ×{}", self.last_status)
        };
        let mark = if styled {
            let color = if self.last_status == 0 {
                self.ansi_ok()
            } else {
                self.ansi_err()
            };
            format!("{color}◆{}", self.ansi_reset())
        } else {
            "◆".into()
        };
        let prompt = if styled {
            format!("{}›{}", self.ansi_mark(), self.ansi_reset())
        } else {
            "›".into()
        };
        let stack = self.stack_token(styled);
        let git = self.git_token(styled);
        let elapsed = self.elapsed_token(styled);
        let cwd_styled = if styled {
            format!("{}{cwd}{}", self.ansi_path(), self.ansi_reset())
        } else {
            cwd.clone()
        };
        let cwd_full_styled = if styled {
            format!("{}{cwd_full}{}", self.ansi_path(), self.ansi_reset())
        } else {
            cwd_full.clone()
        };

        template
            .replace("{cwd:full}", &cwd_full_styled)
            .replace("{cwd}", &cwd_styled)
            .replace("{git}", &git)
            .replace("{elapsed}", &elapsed)
            .replace("{status}", &status)
            .replace("{mark}", &mark)
            .replace("{prompt}", &prompt)
            .replace("{stack}", &stack)
    }

    fn stack_token(&self, styled: bool) -> String {
        if self.directory_stack.is_empty() {
            return String::new();
        }
        let depth = self.directory_stack.len();
        if styled {
            format!(" {}·{depth}{}", self.paint(DIM), self.ansi_reset())
        } else {
            format!(" ·{depth}")
        }
    }

    fn git_token(&self, styled: bool) -> String {
        let Some(branch) = git_branch_info() else {
            return String::new();
        };
        let label = if branch.dirty {
            format!("{}*", branch.name)
        } else {
            branch.name
        };
        if styled {
            format!(" {}{label}{}", self.paint(DIM), self.ansi_reset())
        } else {
            format!(" {label}")
        }
    }

    fn elapsed_token(&self, styled: bool) -> String {
        let Some(elapsed) = self.last_elapsed else {
            return String::new();
        };
        let Some(formatted) = format_elapsed(elapsed) else {
            return String::new();
        };
        if styled {
            format!(" {}{formatted}{}", self.paint(DIM), self.ansi_reset())
        } else {
            format!(" {formatted}")
        }
    }

    pub(crate) fn paint(&self, code: &str) -> String {
        if self.color {
            code.to_owned()
        } else {
            String::new()
        }
    }

    pub(crate) fn ansi_reset(&self) -> String {
        self.paint(RESET)
    }

    pub(crate) fn ansi_ok(&self) -> String {
        self.env_color("OPSH_COLOR_OK", GREEN)
    }

    pub(crate) fn ansi_err(&self) -> String {
        self.env_color("OPSH_COLOR_ERR", RED)
    }

    pub(crate) fn ansi_path(&self) -> String {
        self.env_color("OPSH_COLOR_PATH", BLUE)
    }

    pub(crate) fn ansi_mark(&self) -> String {
        self.env_color("OPSH_COLOR_MARK", YELLOW)
    }

    pub(crate) fn ansi_accent(&self) -> String {
        self.env_color("OPSH_COLOR_ACCENT", VIOLET)
    }

    fn env_color(&self, key: &str, default: &str) -> String {
        if !self.color {
            return String::new();
        }
        match env::var(key) {
            Ok(value) => parse_color_value(&value).unwrap_or_else(|| default.to_owned()),
            Err(_) => default.to_owned(),
        }
    }
}

pub(crate) fn compact_path(path: &Path) -> Option<String> {
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

pub(crate) fn prompt_template() -> String {
    if let Ok(template) = env::var("OPSH_PROMPT") {
        let trimmed = template.trim_matches(|character| character == '\'' || character == '"');
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    match env::var("OPSH_PROMPT_STYLE")
        .unwrap_or_else(|_| "double".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "single" | "one" | "1" => "{mark} {cwd}{git}{status}{elapsed}{stack} {prompt} ".into(),
        _ => "{mark} {cwd}{git}{status}{elapsed}{stack}\n{prompt} ".into(),
    }
}

struct GitBranch {
    name: String,
    dirty: bool,
}

fn git_branch_info() -> Option<GitBranch> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8(output.stdout).ok()?;
    let name = name.trim();
    if name.is_empty() || name == "HEAD" {
        return None;
    }
    let dirty = git_dirty_check_enabled()
        && Command::new("git")
            .args(["status", "--porcelain", "--untracked-files=no"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()
            .is_some_and(|status| status.status.success() && !status.stdout.is_empty());
    Some(GitBranch {
        name: name.to_owned(),
        dirty,
    })
}

/// `OPSH_GIT_DIRTY=0` skips the `git status` dirty marker in `{git}`.
pub(crate) fn git_dirty_check_enabled() -> bool {
    env::var("OPSH_GIT_DIRTY").map_or(true, |value| is_truthy_env(&value))
}

/// Format elapsed time for the prompt. Returns `None` below 10ms to stay quiet.
fn format_elapsed(elapsed: Duration) -> Option<String> {
    let millis = elapsed.as_millis();
    if millis < 10 {
        return None;
    }
    if millis < 1_000 {
        Some(format!("{millis}ms"))
    } else {
        Some(format!("{:.1}s", elapsed.as_secs_f64()))
    }
}

fn parse_color_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || matches!(
            trimmed.to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | "no" | "none"
        )
    {
        return Some(String::new());
    }
    if trimmed.starts_with('\u{1b}') || trimmed.starts_with("\\x1b") || trimmed.starts_with("\x1b")
    {
        if let Some(rest) = trimmed.strip_prefix("\\x1b") {
            return Some(format!("\x1b{rest}"));
        }
        return Some(trimmed.to_owned());
    }
    if trimmed
        .chars()
        .all(|character| character.is_ascii_digit() || character == ';')
    {
        return Some(format!("\x1b[{trimmed}m"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::History;
    use crate::shell::CWD_LOCK;

    #[test]
    fn parses_color_values() {
        assert_eq!(parse_color_value("off"), Some(String::new()));
        assert_eq!(parse_color_value("38;5;114"), Some("\x1b[38;5;114m".into()));
        assert_eq!(parse_color_value("\x1b[31m"), Some("\x1b[31m".into()));
        assert_eq!(parse_color_value("not-a-color"), None);
    }

    #[test]
    fn prompt_templates_respect_style_and_override() {
        unsafe {
            env::remove_var("OPSH_PROMPT");
            env::set_var("OPSH_PROMPT_STYLE", "single");
        }
        assert!(prompt_template().contains("{prompt} "));
        assert!(!prompt_template().contains('\n'));
        unsafe {
            env::set_var("OPSH_PROMPT_STYLE", "double");
        }
        assert!(prompt_template().contains('\n'));
        unsafe {
            env::set_var("OPSH_PROMPT", "{mark} {cwd} > ");
        }
        assert_eq!(prompt_template(), "{mark} {cwd} > ");
        unsafe {
            env::remove_var("OPSH_PROMPT");
            env::remove_var("OPSH_PROMPT_STYLE");
        }
    }

    #[test]
    fn formats_elapsed_quietly() {
        assert_eq!(format_elapsed(Duration::from_millis(9)), None);
        assert_eq!(
            format_elapsed(Duration::from_millis(42)),
            Some("42ms".into())
        );
        assert_eq!(
            format_elapsed(Duration::from_millis(1500)),
            Some("1.5s".into())
        );
    }

    #[test]
    fn default_prompt_includes_git_and_elapsed() {
        unsafe {
            env::remove_var("OPSH_PROMPT");
            env::set_var("OPSH_PROMPT_STYLE", "single");
        }
        let template = prompt_template();
        assert!(template.contains("{git}"));
        assert!(template.contains("{elapsed}"));
        unsafe {
            env::remove_var("OPSH_PROMPT_STYLE");
        }
    }

    fn test_shell(label: &str) -> Shell {
        let history = History {
            path: std::env::temp_dir().join(format!("opsh-{label}-{}", std::process::id())),
            entries: Vec::new(),
        };
        Shell::new(history)
    }

    #[test]
    fn renders_prompt_placeholders_raw() {
        let _guard = CWD_LOCK.lock().unwrap();
        let mut shell = test_shell("prompt-raw");
        let cwd = env::current_dir().unwrap();

        assert_eq!(
            shell.render_prompt("{cwd:full}", false),
            cwd.display().to_string()
        );
        assert_eq!(
            shell.render_prompt("{cwd}", false),
            compact_path(&cwd).unwrap()
        );
        assert_eq!(shell.render_prompt("{mark}{prompt}", false), "◆›");
        assert_eq!(shell.render_prompt("[{status}]", false), "[]");
        assert_eq!(shell.render_prompt("[{elapsed}]", false), "[]");
        assert_eq!(shell.render_prompt("[{stack}]", false), "[]");
        assert_eq!(shell.render_prompt("plain text", false), "plain text");
        assert_eq!(shell.render_prompt("{unknown}", false), "{unknown}");

        shell.last_status = 42;
        assert_eq!(shell.render_prompt("[{status}]", false), "[ ×42]");

        shell.last_elapsed = Some(Duration::from_millis(250));
        assert_eq!(shell.render_prompt("[{elapsed}]", false), "[ 250ms]");
        shell.last_elapsed = Some(Duration::from_millis(3));
        assert_eq!(shell.render_prompt("[{elapsed}]", false), "[]");

        shell.directory_stack.push(cwd.clone());
        shell.directory_stack.push(cwd.clone());
        assert_eq!(shell.render_prompt("[{stack}]", false), "[ ·2]");

        assert_eq!(
            shell.render_prompt("{mark}{status}{stack} {prompt} ", false),
            "◆ ×42 ·2 › "
        );
    }

    #[test]
    fn styled_prompt_matches_raw_without_color() {
        let _guard = CWD_LOCK.lock().unwrap();
        let mut shell = test_shell("prompt-styled");
        shell.color = false;
        shell.last_status = 2;
        shell.last_elapsed = Some(Duration::from_millis(1200));
        let template = "{mark} {cwd}{status}{elapsed}{stack} {prompt} ";
        assert_eq!(
            shell.render_prompt(template, true),
            shell.render_prompt(template, false)
        );
    }

    #[test]
    fn styled_prompt_wraps_placeholders_in_color() {
        let _guard = CWD_LOCK.lock().unwrap();
        const COLOR_KEYS: [&str; 4] = [
            "OPSH_COLOR_OK",
            "OPSH_COLOR_ERR",
            "OPSH_COLOR_PATH",
            "OPSH_COLOR_MARK",
        ];
        struct RestoreEnv(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for RestoreEnv {
            fn drop(&mut self) {
                for (key, value) in self.0.drain(..) {
                    match value {
                        Some(value) => unsafe { env::set_var(key, value) },
                        None => unsafe { env::remove_var(key) },
                    }
                }
            }
        }
        let _restore = RestoreEnv(
            COLOR_KEYS
                .iter()
                .map(|key| (*key, env::var_os(key)))
                .collect(),
        );
        for key in COLOR_KEYS {
            unsafe { env::remove_var(key) };
        }
        let mut shell = test_shell("prompt-color");
        shell.color = true;
        assert_eq!(
            shell.render_prompt("{mark}", true),
            format!("{GREEN}◆{RESET}")
        );
        assert_eq!(
            shell.render_prompt("{prompt}", true),
            format!("{YELLOW}›{RESET}")
        );
        shell.last_status = 7;
        assert_eq!(
            shell.render_prompt("{mark}{status}", true),
            format!("{RED}◆{RESET} {RED}×7{RESET}")
        );
        let cwd = env::current_dir().unwrap();
        assert_eq!(
            shell.render_prompt("{cwd:full}", true),
            format!("{BLUE}{}{RESET}", cwd.display())
        );
    }

    #[test]
    fn format_elapsed_boundaries() {
        assert_eq!(format_elapsed(Duration::ZERO), None);
        assert_eq!(format_elapsed(Duration::from_millis(9)), None);
        assert_eq!(
            format_elapsed(Duration::from_millis(10)),
            Some("10ms".into())
        );
        assert_eq!(
            format_elapsed(Duration::from_millis(999)),
            Some("999ms".into())
        );
        assert_eq!(
            format_elapsed(Duration::from_millis(1000)),
            Some("1.0s".into())
        );
        assert_eq!(
            format_elapsed(Duration::from_millis(1049)),
            Some("1.0s".into())
        );
        assert_eq!(
            format_elapsed(Duration::from_millis(2550)),
            Some("2.5s".into())
        );
        assert_eq!(
            format_elapsed(Duration::from_secs(90)),
            Some("90.0s".into())
        );
        assert_eq!(
            format_elapsed(Duration::from_micros(10_499)),
            Some("10ms".into())
        );
    }

    #[test]
    fn parse_color_value_handles_disabling_keywords() {
        for value in [
            "", "   ", "0", "off", "OFF", "false", "False", "no", "none", " none ",
        ] {
            assert_eq!(parse_color_value(value), Some(String::new()), "{value:?}");
        }
    }

    #[test]
    fn parse_color_value_handles_sgr_and_escapes() {
        assert_eq!(parse_color_value("31"), Some("\x1b[31m".into()));
        assert_eq!(parse_color_value("1;31"), Some("\x1b[1;31m".into()));
        assert_eq!(
            parse_color_value("  38;5;75  "),
            Some("\x1b[38;5;75m".into())
        );
        assert_eq!(parse_color_value("\x1b[1;32m"), Some("\x1b[1;32m".into()));
        assert_eq!(parse_color_value("\\x1b[35m"), Some("\x1b[35m".into()));
        assert_eq!(parse_color_value("  \\x1b[35m  "), Some("\x1b[35m".into()));
    }

    #[test]
    fn parse_color_value_rejects_garbage() {
        assert_eq!(parse_color_value("red"), None);
        assert_eq!(parse_color_value("38;5;x"), None);
        assert_eq!(parse_color_value("#ff0000"), None);
        assert_eq!(parse_color_value("[31m"), None);
        assert_eq!(parse_color_value("1 31"), None);
        assert_eq!(parse_color_value("true"), None);
    }
}
