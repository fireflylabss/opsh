use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const MAX_HISTORY: usize = 1_000;

pub struct History {
    pub(crate) path: PathBuf,
    pub(crate) entries: Vec<String>,
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

    pub(crate) fn push(&mut self, command: &str) {
        if command.is_empty() || self.entries.last().is_some_and(|last| last == command) {
            return;
        }
        self.entries.push(command.into());
        if self.entries.len() > MAX_HISTORY {
            self.entries.drain(..self.entries.len() - MAX_HISTORY);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn save(&self) -> Result<(), String> {
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

/// Expand `!!` (last entry) and `!N` (1-based entry) outside single quotes.
pub(crate) fn expand_history_refs(
    input: &str,
    entries: &[String],
) -> Result<(String, bool), String> {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut at_word_start = true;
    let mut changed = false;

    while let Some(character) = chars.next() {
        match character {
            '\'' if !in_double => {
                in_single = !in_single;
                output.push(character);
                at_word_start = false;
            }
            '"' if !in_single => {
                in_double = !in_double;
                output.push(character);
                at_word_start = false;
            }
            '!' if !in_single && at_word_start => match chars.peek().copied() {
                Some('!') => {
                    chars.next();
                    let entry = entries
                        .last()
                        .ok_or_else(|| "!!: no history yet".to_owned())?;
                    output.push_str(entry);
                    changed = true;
                    at_word_start = false;
                }
                Some(digit) if digit.is_ascii_digit() => {
                    let mut number = String::new();
                    while matches!(chars.peek(), Some(next) if next.is_ascii_digit()) {
                        number.push(chars.next().expect("peeked digit"));
                    }
                    let index = number
                        .parse::<usize>()
                        .map_err(|_| format!("!{number}: invalid history index"))?;
                    if index == 0 {
                        return Err("!0: history indices start at 1".into());
                    }
                    let entry = entries.get(index - 1).ok_or_else(|| {
                        format!("!{index}: history only has {} entries", entries.len())
                    })?;
                    output.push_str(entry);
                    changed = true;
                    at_word_start = false;
                }
                _ => {
                    output.push('!');
                    at_word_start = false;
                }
            },
            character if character.is_whitespace() => {
                output.push(character);
                at_word_start = true;
            }
            _ => {
                output.push(character);
                at_word_start = false;
            }
        }
    }

    Ok((output, changed))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn expands_history_bang_refs() {
        let entries = vec!["echo one".into(), "echo two".into(), "pwd".into()];
        assert_eq!(
            expand_history_refs("!!", &entries).unwrap(),
            ("pwd".into(), true)
        );
        assert_eq!(
            expand_history_refs("!1", &entries).unwrap(),
            ("echo one".into(), true)
        );
        assert_eq!(
            expand_history_refs("!2 && true", &entries).unwrap(),
            ("echo two && true".into(), true)
        );
        assert_eq!(
            expand_history_refs("echo '!!'", &entries).unwrap(),
            ("echo '!!'".into(), false)
        );
        assert!(expand_history_refs("!!", &[]).is_err());
        assert!(expand_history_refs("!9", &entries).is_err());
        assert!(expand_history_refs("!0", &entries).is_err());
    }

    #[test]
    fn history_refs_expand_only_at_word_start() {
        let entries = vec!["echo one".into(), "echo two".into()];
        assert_eq!(
            expand_history_refs("echo !!", &entries).unwrap(),
            ("echo echo two".into(), true)
        );
        assert_eq!(
            expand_history_refs("  !1  ", &entries).unwrap(),
            ("  echo one  ".into(), true)
        );
        assert_eq!(
            expand_history_refs("a!!", &entries).unwrap(),
            ("a!!".into(), false)
        );
        assert_eq!(
            expand_history_refs("echo hello!", &entries).unwrap(),
            ("echo hello!".into(), false)
        );
        assert_eq!(
            expand_history_refs("echo ! !", &entries).unwrap(),
            ("echo ! !".into(), false)
        );
        assert_eq!(
            expand_history_refs("!1 && !2", &entries).unwrap(),
            ("echo one && echo two".into(), true)
        );
    }

    #[test]
    fn history_refs_respect_quotes() {
        let entries = vec!["pwd".into()];
        assert_eq!(
            expand_history_refs("echo '!1'", &entries).unwrap(),
            ("echo '!1'".into(), false)
        );
        assert_eq!(
            expand_history_refs("echo \"!!\"", &entries).unwrap(),
            ("echo \"!!\"".into(), false)
        );
        assert_eq!(
            expand_history_refs("echo \" !!\"", &entries).unwrap(),
            ("echo \" pwd\"".into(), true)
        );
        assert_eq!(
            expand_history_refs("echo 'x' !!", &entries).unwrap(),
            ("echo 'x' pwd".into(), true)
        );
        assert_eq!(
            expand_history_refs("echo \\!!", &entries).unwrap(),
            ("echo \\!!".into(), false)
        );
    }

    #[test]
    fn history_refs_report_invalid_indices() {
        let entries = vec!["one".into(), "two".into()];
        assert_eq!(
            expand_history_refs("!2", &entries).unwrap(),
            ("two".into(), true)
        );
        let error = expand_history_refs("!3", &entries).unwrap_err();
        assert!(error.contains("!3"), "{error}");
        assert!(error.contains("2 entries"), "{error}");
        let error = expand_history_refs("!0", &entries).unwrap_err();
        assert!(error.contains("start at 1"), "{error}");
        let error = expand_history_refs("!!", &[]).unwrap_err();
        assert!(error.contains("no history yet"), "{error}");
        let error = expand_history_refs("!99999999999999999999999", &entries).unwrap_err();
        assert!(error.contains("invalid history index"), "{error}");
        assert_eq!(
            expand_history_refs("!01", &entries).unwrap(),
            ("one".into(), true)
        );
    }
}
