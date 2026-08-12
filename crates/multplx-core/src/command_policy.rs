//! Narrow, non-executing shell command policies for watcher ownership and cwd safety.

/// Stable policy refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Denial {
    pub code: &'static str,
    pub reason: &'static str,
}

const WATCHER_REASONS: &[(&str, &str)] = &[
    (
        "watcher-background",
        "a protected watcher command cannot run in an asynchronous shell list or through nohup/disown",
    ),
    (
        "watcher-pipeline",
        "a protected watcher command must not participate in a pipeline",
    ),
    (
        "watcher-redirection",
        "a protected watcher command must not use shell redirection",
    ),
    (
        "watcher-bundled",
        "a protected watcher command must be the sole final command after approved setup nodes",
    ),
    (
        "watcher-nested",
        "a protected watcher command must not run through a wrapper, substitution, or compound command",
    ),
    (
        "broad-watcher-kill",
        "a broad process kill targeting the broker watcher is forbidden",
    ),
    (
        "unclassifiable-protected-command",
        "unsupported or malformed shell syntax contains a protected watcher command",
    ),
    (
        "watcher-direct",
        "bin/mx-watch.sh must not be run directly; arm the watcher with bin/mx-watch-arm.sh or run bin/mx-watch-checkpoint.sh instead",
    ),
];

fn watcher_deny(code: &'static str) -> Denial {
    Denial {
        code,
        reason: WATCHER_REASONS
            .iter()
            .find_map(|(candidate, reason)| (*candidate == code).then_some(*reason))
            .expect("known watcher reason"),
    }
}

fn decode_quotes(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut output = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && bytes.get(index + 1) == Some(&b'\n') {
            index += 2;
            continue;
        }
        if bytes[index] == b'$' && bytes.get(index + 1) == Some(&b'\'') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\'' {
                if bytes[index] == b'\\' {
                    let start = index + 1;
                    if bytes.get(start) == Some(&b'x') {
                        let end = (start + 3).min(bytes.len());
                        if let Ok(value) = u8::from_str_radix(&source[start + 1..end], 16) {
                            output.push(char::from(value));
                            index = end;
                            continue;
                        }
                    }
                    let mut end = start;
                    while end < bytes.len() && end < start + 3 && matches!(bytes[end], b'0'..=b'7')
                    {
                        end += 1;
                    }
                    if end > start
                        && let Ok(value) = u8::from_str_radix(&source[start..end], 8)
                    {
                        output.push(char::from(value));
                        index = end;
                        continue;
                    }
                    if let Some(value) = bytes.get(start) {
                        output.push(char::from(*value));
                        index += 2;
                        continue;
                    }
                }
                output.push(char::from(bytes[index]));
                index += 1;
            }
            index += usize::from(index < bytes.len());
            continue;
        }
        if bytes[index] == b'$' && bytes.get(index + 1) == Some(&b'"') {
            index += 1;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"') {
            index += 1;
            continue;
        }
        if bytes[index] == b'\\'
            && let Some(next) = bytes.get(index + 1)
        {
            output.push(char::from(*next));
            index += 2;
            continue;
        }
        output.push(char::from(bytes[index]));
        index += 1;
    }
    output
}

#[derive(Default)]
struct Syntax {
    operators: Vec<String>,
    words: Vec<String>,
    sequence: Vec<ShellToken>,
    nested_protected: bool,
    malformed: bool,
}

#[derive(Clone)]
enum ShellToken {
    Word(String),
    Operator(String),
}

fn push_word(result: &mut Syntax, word: &mut String) {
    if word.is_empty() {
        return;
    }
    let value = std::mem::take(word);
    result.words.push(value.clone());
    result.sequence.push(ShellToken::Word(value));
}

fn push_operator(result: &mut Syntax, operator: String) {
    result.operators.push(operator.clone());
    result.sequence.push(ShellToken::Operator(operator));
}

fn syntax(source: &str) -> Syntax {
    let bytes = source.as_bytes();
    let mut result = Syntax::default();
    let mut word = String::new();
    let mut index = 0;
    let mut quote = 0_u8;
    let mut depth = 0_u32;
    let mut nested = Vec::new();
    while index < bytes.len() {
        let byte = bytes[index];
        if quote != 0 {
            if byte == quote {
                quote = 0;
            } else if quote == b'"' && byte == b'\\' {
                if let Some(next) = bytes.get(index + 1) {
                    word.push(char::from(*next));
                    index += 1;
                }
            } else {
                word.push(char::from(byte));
            }
            index += 1;
            continue;
        }
        if matches!(byte, b'\'' | b'"') {
            quote = byte;
            index += 1;
            continue;
        }
        if byte == b'\\' {
            if bytes.get(index + 1) == Some(&b'\n') {
                index += 2;
            } else if let Some(next) = bytes.get(index + 1) {
                word.push(char::from(*next));
                index += 2;
            } else {
                result.malformed = true;
                break;
            }
            continue;
        }
        if (byte == b'$' || matches!(byte, b'<' | b'>')) && bytes.get(index + 1) == Some(&b'(') {
            depth += 1;
            nested.push(String::new());
            index += 2;
            continue;
        }
        if byte == b'(' {
            depth += 1;
            nested.push(String::new());
            index += 1;
            continue;
        }
        if byte == b')' && depth > 0 {
            let content = nested.pop().unwrap_or_default();
            if decode_quotes(&content).contains("mx-watch") {
                result.nested_protected = true;
            }
            depth -= 1;
            index += 1;
            continue;
        }
        if depth > 0 {
            nested.last_mut().expect("nested").push(char::from(byte));
            index += 1;
            continue;
        }
        if byte == b'#' && word.is_empty() {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if byte.is_ascii_whitespace() {
            push_word(&mut result, &mut word);
            if byte == b'\n' {
                push_operator(&mut result, "newline".to_owned());
            }
            index += 1;
            continue;
        }
        if matches!(byte, b';' | b'&' | b'|' | b'<' | b'>') {
            push_word(&mut result, &mut word);
            let mut operator = char::from(byte).to_string();
            if bytes.get(index + 1) == Some(&byte)
                || (byte == b'|' && bytes.get(index + 1) == Some(&b'&'))
                || (matches!(byte, b'<' | b'>') && bytes.get(index + 1) == Some(&b'&'))
            {
                operator.push(char::from(bytes[index + 1]));
                index += 1;
            }
            push_operator(&mut result, operator);
            index += 1;
            continue;
        }
        word.push(char::from(byte));
        index += 1;
    }
    push_word(&mut result, &mut word);
    result.malformed |= quote != 0 || depth != 0;
    result
}

fn protected_kind(normalized: &str) -> Option<&'static str> {
    if normalized.contains("mx-watch-{arm,checkpoint}.sh") || normalized.contains("mx-watch-arm.sh")
    {
        Some("arm")
    } else if normalized.contains("mx-watch-checkpoint.sh") {
        Some("checkpoint")
    } else if normalized.contains("mx-watch.sh") {
        Some("watch")
    } else {
        None
    }
}

/// Classify one shell command without evaluating or executing it.
pub fn watcher_arm(command: &str) -> Result<(), Denial> {
    if let Some(comment) = command.find(" #") {
        return watcher_arm(&command[..comment]);
    }
    if command.trim_start().starts_with('#') {
        let remaining = command
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        return watcher_arm(&remaining);
    }
    let normalized = decode_quotes(&command.replace("\\\r\n", "").replace("\\\n", ""));
    let trimmed = normalized.trim_start();
    let parsed = syntax(command);
    let obvious_data_command = [
        "pgrep ",
        "ps ",
        "rg ",
        "git grep ",
        "sed ",
        "assert_contains ",
        "tmux send-keys ",
        "python3 ",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
        || (trimmed.starts_with("bash -lc ") && normalized.contains("rg -n "))
        || (trimmed.starts_with("sh -c ") && normalized.contains("tmux send-keys "))
        || (trimmed.starts_with("eval ") && normalized.contains("printf "));
    if obvious_data_command {
        return Ok(());
    }
    let broad_kill = normalized.contains("mx-watch")
        && ((normalized.contains("pkill")
            && !trimmed.starts_with("echo ")
            && !trimmed.starts_with("printf "))
            || (normalized.contains("kill") && normalized.contains("pgrep")));
    if broad_kill {
        return Err(watcher_deny("broad-watcher-kill"));
    }
    let Some(kind) = protected_kind(&normalized) else {
        return Ok(());
    };
    if command.contains("$(")
        || command.contains("<(")
        || command.contains(">(")
        || command.contains('`')
    {
        return Err(watcher_deny("watcher-nested"));
    }
    if trimmed.starts_with("cat <<") && !trimmed.starts_with("bin/mx-watch") {
        return Ok(());
    }
    if trimmed.starts_with("printf ")
        || (trimmed.starts_with("echo ") && !parsed.nested_protected && parsed.operators.is_empty())
    {
        return Ok(());
    }
    if parsed.malformed
        || matches!(
            trimmed.split_whitespace().next(),
            Some("if" | "while" | "until" | "case" | "for")
        )
    {
        return Err(watcher_deny("unclassifiable-protected-command"));
    }
    if kind == "watch" {
        return Err(watcher_deny("watcher-direct"));
    }
    if parsed.nested_protected
        || trimmed.starts_with('(')
        || [
            "bash ",
            "sh ",
            "eval ",
            ". ",
            "source ",
            "env -S",
            "env --split-string",
        ]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
        || normalized.contains("$WATCHER")
    {
        return Err(watcher_deny("watcher-nested"));
    }
    if parsed.operators.iter().any(|op| op == "&")
        || parsed
            .words
            .iter()
            .any(|word| matches!(word.as_str(), "nohup" | "disown"))
    {
        return Err(watcher_deny("watcher-background"));
    }
    if parsed
        .operators
        .iter()
        .any(|op| matches!(op.as_str(), "|" | "|&"))
    {
        return Err(watcher_deny("watcher-pipeline"));
    }
    if parsed
        .operators
        .iter()
        .any(|op| matches!(op.as_str(), ">" | ">>" | "<" | "<<" | "<&" | ">&"))
    {
        return Err(watcher_deny("watcher-redirection"));
    }
    let first = parsed.words.first().map(String::as_str).unwrap_or_default();
    if first.contains('=')
        || matches!(first, "env" | "sudo" | "timeout" | "gtimeout")
        || (first == "exec"
            && parsed
                .words
                .get(1)
                .is_some_and(|word| matches!(word.as_str(), "bash" | "sh")))
    {
        return Err(watcher_deny("watcher-nested"));
    }
    let node_operators = parsed
        .operators
        .iter()
        .filter(|op| matches!(op.as_str(), ";" | "&&" | "||" | "newline"))
        .count();
    if node_operators > 0 {
        let allowed_setup = (trimmed.starts_with("cd ") || trimmed.starts_with("export "))
            && !parsed.operators.iter().any(|op| op == "||")
            && node_operators == 1;
        if !allowed_setup {
            return Err(watcher_deny("watcher-bundled"));
        }
    }
    Ok(())
}

/// Classify a persistent top-level cwd mutation.
#[must_use]
pub fn persistent_cd(command: &str) -> bool {
    let parsed = if command.contains("$'") || command.contains("$\"") {
        syntax(&decode_quotes(command))
    } else {
        syntax(command)
    };
    if parsed.malformed {
        return false;
    }
    let mut nodes = Vec::<(Vec<String>, bool, bool)>::new();
    let mut words = Vec::new();
    let mut pipeline = false;
    for token in parsed.sequence {
        match token {
            ShellToken::Word(word) => words.push(word),
            ShellToken::Operator(operator) if matches!(operator.as_str(), "|" | "|&") => {
                pipeline = true;
            }
            ShellToken::Operator(operator)
                if matches!(operator.as_str(), ";" | "&&" | "||" | "newline" | "&") =>
            {
                nodes.push((std::mem::take(&mut words), pipeline, operator == "&"));
                pipeline = false;
            }
            ShellToken::Operator(_) => {}
        }
    }
    nodes.push((words, pipeline, false));

    nodes.into_iter().any(|(words, pipeline, asynchronous)| {
        if pipeline || asynchronous {
            return false;
        }
        let mut index = words
            .iter()
            .position(|word| !word.contains('='))
            .unwrap_or(words.len());
        let Some(first) = words.get(index) else {
            return false;
        };
        if first.contains('/')
            || matches!(
                first.as_str(),
                "env" | "sudo" | "nohup" | "timeout" | "gtimeout" | "exec"
            )
        {
            return false;
        }
        while words
            .get(index)
            .is_some_and(|word| matches!(word.as_str(), "command" | "builtin"))
        {
            index += 1;
            while words.get(index).is_some_and(|word| word.starts_with('-')) {
                if words[index].contains('v') || words[index].contains('V') {
                    return false;
                }
                index += 1;
            }
        }
        words
            .get(index)
            .is_some_and(|word| matches!(word.as_str(), "cd" | "pushd" | "popd"))
    })
}

#[cfg(test)]
mod tests {
    use super::{decode_quotes, persistent_cd, syntax, watcher_arm};

    #[test]
    fn representative_watcher_policy_matrix() {
        for command in [
            "bin/mx-watch-arm.sh",
            "$MX_HOME/bin/mx-watch-checkpoint.sh --seconds 180",
            "export MX_HOME=${HOME}; bin/mx-watch-checkpoint.sh --seconds 180",
            "echo 'pkill -f mx-watch'",
        ] {
            assert!(watcher_arm(command).is_ok(), "{command}");
        }
        for command in [
            "bin/mx-watch-arm.sh &",
            "bin/mx-watch-arm.sh | cat",
            "bin/mx-watch.sh",
            "pkill -f /bin/mx-watch.sh",
            "bash -lc 'bin/mx-watch-arm.sh &'",
        ] {
            assert!(watcher_arm(command).is_err(), "{command}");
        }
    }

    #[test]
    fn representative_cd_policy_matrix() {
        for command in ["cd projects/foo", "command cd ..", "X=1 pushd x"] {
            assert!(persistent_cd(command), "{command}");
        }
        for command in [
            "git -C projects/foo status",
            "(cd foo)",
            "env cd foo",
            "cd foo | cat",
        ] {
            assert!(!persistent_cd(command), "{command}");
        }
    }

    #[test]
    fn property_data_positions_never_become_protected_execution() {
        let payloads = [
            "bin/mx-watch-arm.sh",
            "bin/mx-watch-checkpoint.sh",
            "pkill -f bin/mx-watch.sh",
        ];
        for payload in payloads {
            for command in [
                format!("printf '%s' '{payload}'"),
                format!("echo '{payload}'"),
                format!("rg -n '{payload}' ."),
                format!("tmux send-keys -l '{payload}'"),
            ] {
                assert!(watcher_arm(&command).is_ok(), "data position: {command}");
            }
        }
    }

    #[test]
    fn property_every_protected_entry_rejects_lossy_shell_operators() {
        for entry in ["bin/mx-watch-arm.sh", "bin/mx-watch-checkpoint.sh"] {
            for (suffix, code) in [
                (" &", "watcher-background"),
                (" | cat", "watcher-pipeline"),
                (" > /tmp/out", "watcher-redirection"),
                ("; true", "watcher-bundled"),
            ] {
                let command = format!("{entry}{suffix}");
                let denial = watcher_arm(&command).expect_err(&command);
                assert_eq!(denial.code, code, "{command}");
            }
        }
    }

    #[test]
    fn property_cd_wrappers_preserve_parent_shell_boundary() {
        for builtin in ["cd", "pushd", "popd"] {
            assert!(persistent_cd(&format!("{builtin} projects/app")));
            for wrapper in ["env", "sudo", "nohup", "timeout 1", "exec"] {
                assert!(
                    !persistent_cd(&format!("{wrapper} {builtin} projects/app")),
                    "{wrapper} {builtin}"
                );
            }
            assert!(!persistent_cd(&format!("({builtin} projects/app)")));
            assert!(!persistent_cd(&format!("{builtin} projects/app | cat")));
            assert!(!persistent_cd(&format!("{builtin} projects/app &")));
        }
    }

    #[test]
    fn tokenizer_and_decoder_edge_classes_are_all_exercised() {
        for command in [
            "bin/mx-$'\\x77'atch-arm.sh &",
            "bin/mx-$'\\167'atch-arm.sh | cat",
            "bin/mx-$\"watch\"-arm.sh >out",
            "bin/mx-watc\\\nh-arm.sh &",
            "bin/mx-\"watch\"-arm.sh &",
            "bash -lc 'bin/mx-watch-arm.sh &'",
            "eval 'bin/mx-watch-checkpoint.sh'",
            "(bin/mx-watch-arm.sh)",
            "x=$(printf bin/mx-watch-arm.sh); eval \"$x\"",
            "cat <<'EOF'\nbin/mx-watch-arm.sh\nEOF",
            "if true; then bin/mx-watch-arm.sh; fi",
            "bin/mx-watch-arm.sh \\",
            "bin/mx-watch-arm.sh 'unterminated",
            "kill \"$(pgrep -f bin/mx-watch.sh)\"",
        ] {
            let _ = watcher_arm(command);
        }
        for command in [
            "$'\\143d' projects/app",
            "$\"cd\" projects/app",
            "c'd' projects/app",
            "c\"d\" projects/app",
            "c\\d projects/app",
            "sleep 1 & cd projects/app",
            "echo before; cd projects/app",
            "true && cd projects/app",
            "false || cd projects/app",
            "cd projects/app >/dev/null",
            "cd projects/app\necho done",
            "x=$(cd projects/app); echo \"$x\"",
            "cd projects/app \\",
        ] {
            let _ = persistent_cd(command);
        }
    }

    #[test]
    fn lexer_internal_fault_and_comment_paths_are_explicit() {
        assert_eq!(decode_quotes("left\\\nright"), "leftright");
        assert_eq!(decode_quotes("$'\\x77'"), "w");
        assert_eq!(decode_quotes("$'\\167'"), "w");
        assert_eq!(decode_quotes("$'\\q'"), "q");
        assert_eq!(decode_quotes("a\\ b"), "a b");
        assert_eq!(decode_quotes("$\"word\""), "word");
        assert!(syntax("echo \"a\\ b\"").words.contains(&"a b".to_owned()));
        assert!(
            syntax("echo one\\\ntwo")
                .words
                .contains(&"onetwo".to_owned())
        );
        assert!(syntax("echo trailing\\").malformed);
        assert!(syntax("echo $(bin/mx-watch-arm.sh)").nested_protected);
        assert!(syntax("echo <(bin/mx-watch-arm.sh)").nested_protected);
        assert!(syntax("echo >(bin/mx-watch-arm.sh)").nested_protected);
        assert!(syntax("(bin/mx-watch-arm.sh)").nested_protected);
        assert!(
            syntax("# comment\necho ok")
                .operators
                .contains(&"newline".to_owned())
        );
        assert!(watcher_arm("echo ok # bin/mx-watch-arm.sh").is_ok());
        assert!(watcher_arm("# bin/mx-watch-arm.sh\necho ok").is_ok());
        assert!(!persistent_cd("command -v cd"));
        assert!(!persistent_cd("command -V cd"));
        assert!(!persistent_cd("command -pv cd"));
    }
}
