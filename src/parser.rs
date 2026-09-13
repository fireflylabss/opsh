pub(crate) fn split_words(command: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut quoted = false;

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
            '\'' if !in_double => {
                in_single = !in_single;
                quoted = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                quoted = true;
            }
            character if character.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() || quoted {
                    words.push(std::mem::take(&mut current));
                }
                quoted = false;
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
    if !current.is_empty() || quoted {
        words.push(current);
    }
    Ok(words)
}

pub(crate) fn command_tail(input: &str, skip_words: usize) -> Option<&str> {
    let trimmed = input.trim_start();
    let mut rest = trimmed;
    for _ in 0..skip_words {
        rest = skip_one_word(rest)?;
        rest = rest.trim_start();
    }
    if rest.is_empty() { None } else { Some(rest) }
}

fn skip_one_word(input: &str) -> Option<&str> {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut started = false;

    for (index, character) in input.char_indices() {
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

pub(crate) fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    if value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '/' | '.' | ':' | '=')
    }) {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChainOp {
    And,
    Or,
    Seq,
}

pub(crate) fn has_chain_operators(command: &str) -> bool {
    scan_operators(command).any(|operator| {
        matches!(
            operator,
            ScannedOperator::And | ScannedOperator::Or | ScannedOperator::Seq
        )
    })
}

/// Pipes, redirects, subshells and background jobs still need a POSIX shell.
pub(crate) fn needs_posix_shell(command: &str) -> bool {
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

pub(crate) fn split_chain(command: &str) -> Result<(Vec<String>, Vec<ChainOp>), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_command_tails() {
        assert_eq!(command_tail("time echo hello", 1), Some("echo hello"));
        assert_eq!(command_tail("repeat 3 echo hello", 1), Some("3 echo hello"));
        assert_eq!(command_tail(r#"time echo "a b""#, 1), Some(r#"echo "a b""#));
        assert_eq!(command_tail("repeat", 2), None);
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
    fn splits_chains_on_operators() {
        let (segments, operators) = split_chain("cd /tmp && ls || echo no; pwd").unwrap();
        assert_eq!(segments, vec!["cd /tmp", "ls", "echo no", "pwd"]);
        assert_eq!(operators, vec![ChainOp::And, ChainOp::Or, ChainOp::Seq]);

        let (segments, operators) = split_chain("just one").unwrap();
        assert_eq!(segments, vec!["just one"]);
        assert!(operators.is_empty());

        let (segments, operators) = split_chain("  a  ;  b  ").unwrap();
        assert_eq!(segments, vec!["a", "b"]);
        assert_eq!(operators, vec![ChainOp::Seq]);
    }

    #[test]
    fn split_chain_keeps_quoted_operators_intact() {
        let (segments, operators) = split_chain("echo 'a && b' && echo \"c; d\"").unwrap();
        assert_eq!(segments, vec!["echo 'a && b'", "echo \"c; d\""]);
        assert_eq!(operators, vec![ChainOp::And]);

        let (segments, operators) = split_chain("echo \"it's\"; echo done").unwrap();
        assert_eq!(segments, vec!["echo \"it's\"", "echo done"]);
        assert_eq!(operators, vec![ChainOp::Seq]);
    }

    #[test]
    fn split_chain_keeps_escaped_operators_intact() {
        let (segments, operators) = split_chain(r"echo a\;b; echo c").unwrap();
        assert_eq!(segments, vec![r"echo a\;b", "echo c"]);
        assert_eq!(operators, vec![ChainOp::Seq]);

        let (segments, operators) = split_chain(r"echo a \&& b").unwrap();
        assert_eq!(segments, vec![r"echo a \&& b"]);
        assert!(operators.is_empty());

        let (segments, operators) = split_chain(r"echo a \|| b").unwrap();
        assert_eq!(segments, vec![r"echo a \|| b"]);
        assert!(operators.is_empty());

        let (segments, _) = split_chain(r"echo 'a\'; echo b").unwrap();
        assert_eq!(segments, vec![r"echo 'a\'", "echo b"]);
    }

    #[test]
    fn split_chain_rejects_malformed_input() {
        assert_eq!(
            split_chain("echo a &&").unwrap_err(),
            "empty command in chain"
        );
        assert_eq!(
            split_chain("&& echo a").unwrap_err(),
            "empty command in chain"
        );
        assert_eq!(
            split_chain("echo a; ; echo b").unwrap_err(),
            "empty command in chain"
        );
        assert_eq!(split_chain("echo 'a && b").unwrap_err(), "unclosed quote");
        assert_eq!(split_chain("echo \"a; b").unwrap_err(), "unclosed quote");
    }

    #[test]
    fn scan_operators_detects_each_kind() {
        assert!(has_chain_operators("a && b"));
        assert!(has_chain_operators("a || b"));
        assert!(has_chain_operators("a; b"));
        assert!(!has_chain_operators("a | b"));
        assert!(!has_chain_operators("a &"));

        assert!(needs_posix_shell("a | b"));
        assert!(needs_posix_shell("a > out"));
        assert!(needs_posix_shell("a >> out"));
        assert!(needs_posix_shell("a < in"));
        assert!(needs_posix_shell("a 2>&1"));
        assert!(needs_posix_shell("echo `date`"));
        assert!(needs_posix_shell("echo $(date)"));
        assert!(needs_posix_shell("(cd /tmp && ls)"));
        assert!(needs_posix_shell("sleep 1 &"));
        assert!(needs_posix_shell("sleep 1 & echo"));
        assert!(needs_posix_shell("& echo"));
        assert!(!needs_posix_shell("a && b || c; d"));
        assert!(!needs_posix_shell("echo $HOME"));
        assert!(!needs_posix_shell("set X a&b"));
    }

    #[test]
    fn scan_operators_ignores_quoted_and_escaped_operators() {
        assert!(!needs_posix_shell("echo 'a | b'"));
        assert!(!needs_posix_shell("echo \"a > b\""));
        assert!(!needs_posix_shell("echo '$(date)'"));
        assert!(!needs_posix_shell("echo \"(x)\""));
        assert!(!needs_posix_shell("echo '`x`'"));
        assert!(!needs_posix_shell("echo 'sleep &'"));
        assert!(!needs_posix_shell(r"echo a\|b"));
        assert!(!needs_posix_shell(r"echo a \> b"));
        assert!(!needs_posix_shell(r"echo \(x\)"));
        assert!(!needs_posix_shell(r"echo \$x"));
        assert!(!has_chain_operators("echo 'a; b'"));
        assert!(!has_chain_operators("echo \"a || b\""));
        assert!(!has_chain_operators(r"echo a\;b"));
        assert!(!has_chain_operators(r"echo a \&& b"));
        assert!(!has_chain_operators("echo \"it's; fine\""));
        assert!(needs_posix_shell("echo 'a' | wc"));
        assert!(has_chain_operators("echo 'a' && echo b"));
    }

    #[test]
    fn shell_quote_leaves_safe_values_bare() {
        assert_eq!(shell_quote("ls"), "ls");
        assert_eq!(shell_quote("ls-la_v2"), "ls-la_v2");
        assert_eq!(shell_quote("/usr/bin/env"), "/usr/bin/env");
        assert_eq!(shell_quote("a.b:c=d"), "a.b:c=d");
        assert_eq!(shell_quote("ABC123"), "ABC123");
    }

    #[test]
    fn shell_quote_wraps_unsafe_values() {
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("ls -la"), "'ls -la'");
        assert_eq!(shell_quote("a|b"), "'a|b'");
        assert_eq!(shell_quote("$HOME"), "'$HOME'");
        assert_eq!(shell_quote("a\"b"), "'a\"b'");
        assert_eq!(shell_quote("a\\b"), "'a\\b'");
        assert_eq!(shell_quote("*"), "'*'");
        assert_eq!(shell_quote("tab\there"), "'tab\there'");
        assert_eq!(shell_quote("ünïcode"), "'ünïcode'");
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote("'"), r"''\'''");
        assert_eq!(shell_quote("a'b'c"), r"'a'\''b'\''c'");
    }

    #[test]
    fn shell_quote_round_trips_through_split_words() {
        for value in [
            "plain",
            "with space",
            "it's",
            "a'b'c",
            "$HOME and `cmd`",
            "quote\"inside",
            "back\\slash",
            "semi;colon && and",
        ] {
            let quoted = shell_quote(value);
            let words = split_words(&format!("echo {quoted}")).unwrap();
            assert_eq!(
                words,
                vec!["echo".to_owned(), value.to_owned()],
                "{value:?}"
            );
        }
    }

    // Known gap: `split_words` drops empty quoted words (`''` / `""`), so an
    // argument that is intentionally empty disappears instead of being passed on.
    #[test]
    fn split_words_keeps_empty_quoted_arguments() {
        assert_eq!(
            split_words("echo ''").unwrap(),
            vec!["echo".to_owned(), String::new()]
        );
        assert_eq!(
            split_words(r#"set NAME """#).unwrap(),
            vec!["set".to_owned(), "NAME".to_owned(), String::new()]
        );
    }
}
