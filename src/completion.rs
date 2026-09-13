use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::sync::{Arc, Mutex};

use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::HistoryHinter;
use rustyline::{Context, Helper, Hinter, Validator};

use crate::builtins::{BUILTINS, is_executable};

#[derive(Helper, rustyline::Completer, Hinter, Validator)]
pub(crate) struct OpshHelper {
    #[rustyline(Completer)]
    pub(crate) completer: OpshCompleter,
    #[rustyline(Hinter)]
    pub(crate) hinter: HistoryHinter,
}

impl Highlighter for OpshHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        std::borrow::Cow::Owned(format!("\x1b[2m{hint}\x1b[0m"))
    }
}

pub(crate) struct OpshCompleter {
    pub(crate) files: FilenameCompleter,
    pub(crate) aliases: Arc<Mutex<BTreeMap<String, String>>>,
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

            if let Ok(aliases) = self.aliases.lock() {
                for name in aliases.keys() {
                    if name.starts_with(prefix)
                        && !matches
                            .iter()
                            .any(|candidate| candidate.replacement == *name)
                    {
                        matches.push(Pair {
                            display: name.clone(),
                            replacement: name.clone(),
                        });
                    }
                }
            }

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
