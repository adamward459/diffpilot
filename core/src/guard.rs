//! The check behind the PreToolUse hook of every run: it blocks a Bash command that would push, commit, or
//! use the GitHub CLI.
//!
//! `--disallowedTools Bash(git push:*)` only matches a command that starts with `git push`. This reads the
//! command the way a shell does, so `git -C . push`, `sh -c 'git push'`, `env X=1 git push`, `xargs git push`,
//! `find -exec git push {} ;`, `$(git push)`, a git alias that expands to push, and a script file that runs
//! push are all found.
//!
//! What it cannot see: a program that pushes on its own (`npm run release`, `make deploy`), code built at
//! run time (`eval "$x"`), and any other way to reach the network, such as `curl`. Only a sandbox closes those.

use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The text every block message starts with, so the run log can tell a block from a failed command.
pub const BLOCK_MARKER: &str = "[diffpilot guard]";

const MAX_DEPTH: u32 = 8;
const MAX_FILE_BYTES: u64 = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Violation {
    Push,
    Commit,
    GitHubCli,
    ScriptThatPushes,
    TooNested,
}

impl Violation {
    pub fn message(self) -> String {
        let text = match self {
            Self::Push => {
                "git push is not allowed in a diffpilot run. Leave the changes in the working tree; the reviewer pushes them."
            }
            Self::Commit => {
                "git commit is not allowed in a diffpilot run. Leave the changes in the working tree; the reviewer commits them."
            }
            Self::GitHubCli => {
                "The GitHub CLI is not allowed in a diffpilot run. diffpilot never posts to GitHub from a run."
            }
            Self::ScriptThatPushes => {
                "This command runs code that pushes with git. That is not allowed in a diffpilot run."
            }
            Self::TooNested => "This command is nested too deeply to check, so it is not allowed in a diffpilot run.",
        };
        format!("{BLOCK_MARKER} {text}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    Allow,
    Block(String),
}

/// Decides one PreToolUse hook call. `input` is the JSON Claude Code sends on stdin. `alias` answers
/// `git config alias.<name>` for the repo the run works in.
pub fn decide_hook(
    input: &str,
    fallback_cwd: Option<&Path>,
    alias: &dyn Fn(&Path, &str) -> Option<String>,
) -> HookDecision {
    let Ok(value) = serde_json::from_str::<Value>(input) else {
        return HookDecision::Block(format!(
            "{BLOCK_MARKER} The hook input could not be read, so the command is not allowed."
        ));
    };
    if value.get("tool_name").and_then(Value::as_str) != Some("Bash") {
        return HookDecision::Allow;
    }
    let Some(command) = value.pointer("/tool_input/command").and_then(Value::as_str) else {
        return HookDecision::Block(format!(
            "{BLOCK_MARKER} The Bash call has no command, so it is not allowed."
        ));
    };
    let cwd = value
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| fallback_cwd.map(Path::to_path_buf));
    let lookup = |name: &str| cwd.as_deref().and_then(|dir| alias(dir, name));
    match inspect(command, cwd.as_deref(), &lookup) {
        Some(violation) => HookDecision::Block(violation.message()),
        None => HookDecision::Allow,
    }
}

/// The first reason the command must not run, if any.
pub fn inspect(command: &str, cwd: Option<&Path>, alias: &dyn Fn(&str) -> Option<String>) -> Option<Violation> {
    let checker = Checker {
        cwd,
        alias,
        defined: RefCell::default(),
    };
    checker.script(command, 0)
}

struct Checker<'a> {
    cwd: Option<&'a Path>,
    alias: &'a dyn Fn(&str) -> Option<String>,
    defined: RefCell<HashMap<String, String>>,
}

#[derive(Debug, Default, Clone)]
struct Command {
    words: Vec<String>,
    heredocs: Vec<String>,
    pending: Vec<(String, bool)>,
}

#[derive(Debug, Default)]
struct Parsed {
    commands: Vec<Command>,
    nested: Vec<String>,
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

/// The text between `$(` and its `)`, and the index after it.
fn paren_body(chars: &[char], open: usize) -> (String, usize) {
    let mut depth = 1;
    let mut index = open;
    let mut quote: Option<char> = None;
    while index < chars.len() {
        let c = chars[index];
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => index += 1,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '\\') => index += 1,
            (None, '(') => depth += 1,
            (None, ')') => {
                depth -= 1;
                if depth == 0 {
                    return (chars[open..index].iter().collect(), index + 1);
                }
            }
            _ => {}
        }
        index += 1;
    }
    (chars[open..].iter().collect(), chars.len())
}

fn backtick_body(chars: &[char], open: usize) -> (String, usize) {
    let mut index = open;
    let mut body = String::new();
    while index < chars.len() {
        match chars[index] {
            '\\' if index + 1 < chars.len() => {
                body.push(chars[index + 1]);
                index += 1;
            }
            '`' => return (body, index + 1),
            c => body.push(c),
        }
        index += 1;
    }
    (body, chars.len())
}

fn parse(script: &str) -> Parsed {
    let chars: Vec<char> = script.chars().collect();
    let mut parsed = Parsed::default();
    let mut command = Command::default();
    let mut word = String::new();
    let mut in_word = false;
    let mut index = 0;

    fn flush_word(command: &mut Command, word: &mut String, in_word: &mut bool) {
        if *in_word {
            command.words.push(std::mem::take(word));
            *in_word = false;
        }
    }
    fn flush_command(parsed: &mut Parsed, command: &mut Command, word: &mut String, in_word: &mut bool) {
        flush_word(command, word, in_word);
        if !command.words.is_empty() || !command.pending.is_empty() {
            parsed.commands.push(std::mem::take(command));
        }
    }

    while index < chars.len() {
        let c = chars[index];
        match c {
            '\\' => {
                match chars.get(index + 1) {
                    Some('\n') => {}
                    Some(next) => {
                        word.push(*next);
                        in_word = true;
                    }
                    None => {}
                }
                index += 2;
            }
            '\'' => {
                in_word = true;
                index += 1;
                while index < chars.len() && chars[index] != '\'' {
                    word.push(chars[index]);
                    index += 1;
                }
                index += 1;
            }
            '"' => {
                in_word = true;
                index += 1;
                while index < chars.len() && chars[index] != '"' {
                    match chars[index] {
                        '\\' if matches!(chars.get(index + 1), Some('"' | '\\' | '$' | '`')) => {
                            word.push(chars[index + 1]);
                            index += 2;
                        }
                        '$' if chars.get(index + 1) == Some(&'(') => {
                            let (body, next) = paren_body(&chars, index + 2);
                            word.push_str(&format!("$({body})"));
                            parsed.nested.push(body);
                            index = next;
                        }
                        '`' => {
                            let (body, next) = backtick_body(&chars, index + 1);
                            word.push_str(&format!("`{body}`"));
                            parsed.nested.push(body);
                            index = next;
                        }
                        other => {
                            word.push(other);
                            index += 1;
                        }
                    }
                }
                index += 1;
            }
            '$' if chars.get(index + 1) == Some(&'(') => {
                in_word = true;
                let (body, next) = paren_body(&chars, index + 2);
                word.push_str(&format!("$({body})"));
                parsed.nested.push(body);
                index = next;
            }
            '$' if chars.get(index + 1) == Some(&'\'') => {
                in_word = true;
                index += 2;
                while index < chars.len() && chars[index] != '\'' {
                    if chars[index] == '\\' && index + 1 < chars.len() {
                        index += 1;
                    }
                    word.push(chars[index]);
                    index += 1;
                }
                index += 1;
            }
            '`' => {
                in_word = true;
                let (body, next) = backtick_body(&chars, index + 1);
                word.push_str(&format!("`{body}`"));
                parsed.nested.push(body);
                index = next;
            }
            '#' if !in_word => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '<' if chars.get(index + 1) == Some(&'<') && chars.get(index + 2) != Some(&'<') => {
                flush_word(&mut command, &mut word, &mut in_word);
                index += 2;
                let dash = chars.get(index) == Some(&'-');
                if dash {
                    index += 1;
                }
                while matches!(chars.get(index), Some(' ' | '\t')) {
                    index += 1;
                }
                let mut delimiter = String::new();
                while let Some(&d) = chars.get(index) {
                    if d.is_whitespace() || matches!(d, ';' | '&' | '|' | '(' | ')') {
                        break;
                    }
                    if !matches!(d, '\'' | '"' | '\\') {
                        delimiter.push(d);
                    }
                    index += 1;
                }
                command.pending.push((delimiter, dash));
            }
            '\n' => {
                flush_command(&mut parsed, &mut command, &mut word, &mut in_word);
                index += 1;
                let owner = parsed
                    .commands
                    .last()
                    .filter(|last| !last.pending.is_empty())
                    .map(|_| parsed.commands.len() - 1);
                if let Some(owner) = owner {
                    let pending = std::mem::take(&mut parsed.commands[owner].pending);
                    for (delimiter, dash) in pending {
                        let mut body = String::new();
                        while index < chars.len() {
                            let end = chars[index..]
                                .iter()
                                .position(|&ch| ch == '\n')
                                .map_or(chars.len(), |p| index + p);
                            let line: String = chars[index..end].iter().collect();
                            index = (end + 1).min(chars.len());
                            let compare = if dash {
                                line.trim_start_matches('\t')
                            } else {
                                line.as_str()
                            };
                            if compare == delimiter {
                                break;
                            }
                            body.push_str(&line);
                            body.push('\n');
                        }
                        parsed.commands[owner].heredocs.push(body);
                    }
                }
            }
            ';' | '&' | '|' | '(' | ')' => {
                flush_command(&mut parsed, &mut command, &mut word, &mut in_word);
                index += 1;
            }
            c if c.is_whitespace() => {
                flush_word(&mut command, &mut word, &mut in_word);
                index += 1;
            }
            c => {
                word.push(c);
                in_word = true;
                index += 1;
            }
        }
    }
    flush_command(&mut parsed, &mut command, &mut word, &mut in_word);
    parsed
}

const RESERVED: &[&str] = &[
    "if", "then", "else", "elif", "do", "while", "until", "!", "{", "}", "time", "coproc", "function",
];
const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "ash", "fish", "csh", "tcsh"];
const FORBIDDEN_CLIS: &[&str] = &["gh", "hub", "glab"];
const INTERPRETERS: &[&str] = &[
    "node",
    "nodejs",
    "deno",
    "bun",
    "ruby",
    "perl",
    "php",
    "osascript",
    "lua",
    "awk",
    "gawk",
    "mawk",
    "nawk",
    "tclsh",
    "swift",
    "rscript",
    "pwsh",
    "irb",
];
const GIT_GLOBAL_WITH_VALUE: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--config-env",
    "--super-prefix",
    "--attr-source",
    "--exec-path",
];

fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn is_assignment(word: &str) -> bool {
    let name_end = word.find('=').filter(|&at| at > 0);
    name_end.is_some_and(|at| {
        let name = word[..at].trim_end_matches('+');
        name.chars().next().is_some_and(is_name_start) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn is_dynamic(word: &str) -> bool {
    word.contains(['$', '`'])
}

fn is_redirect(word: &str) -> bool {
    let rest = word
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .trim_start_matches('&');
    rest.starts_with('>') || rest.starts_with('<')
}

fn without_redirects(words: &[String]) -> Vec<String> {
    let mut kept = Vec::new();
    let mut skip_next = false;
    for word in words {
        if skip_next {
            skip_next = false;
            continue;
        }
        if is_redirect(word) {
            let operator = word.trim_start_matches(|c: char| c.is_ascii_digit());
            skip_next = operator.chars().all(|c| matches!(c, '>' | '<' | '&' | '|'));
            continue;
        }
        kept.push(word.clone());
    }
    kept
}

/// Words that mention both `git` and `push`, as a script's text or its arguments would.
fn mentions_git_push(text: &str) -> bool {
    let words: Vec<&str> = text.split(|c: char| !c.is_alphanumeric() && c != '-').collect();
    words.contains(&"git") && words.contains(&"push")
}

fn short_flag_has(arg: &str, letter: char) -> bool {
    arg.starts_with('-') && !arg.starts_with("--") && arg.chars().skip(1).any(|c| c == letter)
}

impl Checker<'_> {
    fn script(&self, script: &str, depth: u32) -> Option<Violation> {
        if depth > MAX_DEPTH {
            return Some(Violation::TooNested);
        }
        let parsed = parse(script);
        for command in &parsed.commands {
            if let Some(violation) = self.command(command, depth) {
                return Some(violation);
            }
        }
        parsed.nested.iter().find_map(|inner| self.script(inner, depth + 1))
    }

    fn command(&self, command: &Command, depth: u32) -> Option<Violation> {
        let words = without_redirects(&command.words);
        let mut rest: &[String] = &words;
        let mut env: Vec<(&str, &str)> = Vec::new();
        loop {
            match rest.first() {
                Some(word) if is_assignment(word) => {
                    env.extend(word.split_once('='));
                    rest = &rest[1..];
                }
                Some(word) if RESERVED.contains(&word.as_str()) => rest = &rest[1..],
                _ => break,
            }
        }
        if rest.is_empty() {
            return None;
        }
        self.program(rest, &env, &command.heredocs, depth)
    }

    fn program(&self, words: &[String], env: &[(&str, &str)], heredocs: &[String], depth: u32) -> Option<Violation> {
        if depth > MAX_DEPTH {
            return Some(Violation::TooNested);
        }
        let program = &words[0];
        let args = &words[1..];
        if is_dynamic(program) {
            return args.iter().any(|arg| arg.contains("push")).then_some(Violation::Push);
        }
        let name = basename(program);
        let lower = name.to_ascii_lowercase();
        if FORBIDDEN_CLIS.contains(&lower.as_str()) {
            return Some(Violation::GitHubCli);
        }
        if name == "git" {
            return self.git(args, env, depth);
        }
        if let Some(sub) = name.strip_prefix("git-") {
            let mut with_sub = vec![sub.to_string()];
            with_sub.extend_from_slice(args);
            return self.git(&with_sub, env, depth);
        }
        if let Some(inner) = unwrap_wrapper(&lower, args) {
            return match inner {
                Unwrapped::Command(command) if !command.is_empty() => self.program(command, env, heredocs, depth + 1),
                Unwrapped::Script(text) => self.script(&text, depth + 1),
                Unwrapped::Command(_) => None,
            };
        }
        if lower == "find" {
            return exec_commands(args)
                .iter()
                .find_map(|command| self.program(command, env, heredocs, depth + 1));
        }
        if lower == "eval" {
            return self.script(&args.join(" "), depth + 1);
        }
        if SHELLS.contains(&lower.as_str()) {
            return self.shell(args, heredocs, depth);
        }
        if lower == "source" || lower == "." {
            return args.first().and_then(|file| self.file(file, depth));
        }
        if lower.starts_with("python") || INTERPRETERS.contains(&lower.as_str()) {
            return self.interpreter(args, heredocs);
        }
        if program.contains('/') {
            return self.file(program, depth);
        }
        None
    }

    fn shell(&self, args: &[String], heredocs: &[String], depth: u32) -> Option<Violation> {
        if let Some(at) = args.iter().position(|arg| short_flag_has(arg, 'c')) {
            return args.get(at + 1).and_then(|script| self.script(script, depth + 1));
        }
        match args.iter().find(|arg| !arg.starts_with('-')) {
            Some(file) => self.file(file, depth),
            None => heredocs.iter().find_map(|body| self.script(body, depth + 1)),
        }
    }

    fn interpreter(&self, args: &[String], heredocs: &[String]) -> Option<Violation> {
        if mentions_git_push(&args.join(" ")) || heredocs.iter().any(|body| mentions_git_push(body)) {
            return Some(Violation::ScriptThatPushes);
        }
        let file = args
            .iter()
            .find(|arg| !arg.starts_with('-') && self.resolve(arg).is_some_and(|path| path.is_file()))?;
        self.read(file)
            .filter(|text| mentions_git_push(text))
            .map(|_| Violation::ScriptThatPushes)
    }

    fn resolve(&self, file: &str) -> Option<PathBuf> {
        let path = PathBuf::from(file);
        if path.is_absolute() {
            return Some(path);
        }
        Some(self.cwd?.join(path))
    }

    fn read(&self, file: &str) -> Option<String> {
        let path = self.resolve(file)?;
        let meta = std::fs::metadata(&path).ok()?;
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        if bytes.iter().take(1024).any(|byte| *byte == 0) {
            return None;
        }
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// A script file a command runs. Shell scripts are read like commands, other languages by their words.
    fn file(&self, file: &str, depth: u32) -> Option<Violation> {
        let text = self.read(file)?;
        let first = text.lines().next().unwrap_or_default();
        let other_language = (first.starts_with("#!")
            && ["python", "node", "ruby", "perl", "php", "deno", "bun", "lua"]
                .iter()
                .any(|name| first.contains(name)))
            || [".py", ".js", ".mjs", ".ts", ".rb", ".pl", ".php", ".lua"]
                .iter()
                .any(|ext| file.ends_with(ext));
        if other_language {
            return mentions_git_push(&text).then_some(Violation::ScriptThatPushes);
        }
        self.script(&text, depth + 1)
    }

    fn git(&self, args: &[String], env: &[(&str, &str)], depth: u32) -> Option<Violation> {
        let mut aliases: HashMap<String, String> = self.defined.borrow().clone();
        for (key, value) in env {
            if let Some(number) = key.strip_prefix("GIT_CONFIG_KEY_")
                && let Some(name) = value.strip_prefix("alias.")
                && let Some((_, text)) = env
                    .iter()
                    .find(|(other, _)| *other == format!("GIT_CONFIG_VALUE_{number}"))
            {
                aliases.insert(name.to_string(), (*text).to_string());
            }
        }
        let mut index = 0;
        while let Some(arg) = args.get(index) {
            if !arg.starts_with('-') {
                break;
            }
            if arg == "-c" || arg == "--config-env" {
                if let Some(setting) = args.get(index + 1)
                    && let Some((name, value)) = setting.strip_prefix("alias.").and_then(|rest| rest.split_once('='))
                {
                    aliases.insert(name.to_string(), value.to_string());
                }
                index += 2;
            } else if GIT_GLOBAL_WITH_VALUE.contains(&arg.as_str()) {
                index += 2;
            } else {
                index += 1;
            }
        }
        let sub = args.get(index)?;
        let rest = &args[index + 1..];
        self.git_sub(sub, rest, &aliases, depth)
    }

    fn git_sub(&self, sub: &str, rest: &[String], aliases: &HashMap<String, String>, depth: u32) -> Option<Violation> {
        match sub {
            "push" | "send-pack" | "http-push" => return Some(Violation::Push),
            "commit" | "commit-tree" => return Some(Violation::Commit),
            "config" => self.remember_aliases(rest),
            "rebase" => {
                if let Some(script) = value_of(rest, &["-x", "--exec"]) {
                    return self.script(&script, depth + 1);
                }
            }
            "submodule" => {
                if let Some(at) = rest.iter().position(|arg| arg == "foreach") {
                    return self.script(&rest[at + 1..].join(" "), depth + 1);
                }
            }
            "bisect" => {
                if let Some(at) = rest.iter().position(|arg| arg == "run") {
                    return self.script(&rest[at + 1..].join(" "), depth + 1);
                }
            }
            "filter-branch" => {
                if let Some(found) = rest
                    .iter()
                    .filter(|arg| arg.contains(' '))
                    .find_map(|arg| self.script(arg, depth + 1))
                {
                    return Some(found);
                }
            }
            _ => {}
        }
        let text = aliases.get(sub).cloned().or_else(|| (self.alias)(sub))?;
        if depth > MAX_DEPTH {
            return Some(Violation::TooNested);
        }
        if let Some(shell) = text.strip_prefix('!') {
            return self.script(shell, depth + 1);
        }
        let mut expanded: Vec<String> = parse(&text)
            .commands
            .into_iter()
            .next()
            .map(|command| command.words)
            .unwrap_or_default();
        expanded.extend_from_slice(rest);
        let mut with_git = vec!["git".to_string()];
        with_git.extend(expanded);
        self.program(&with_git, &[], &[], depth + 1)
    }

    fn remember_aliases(&self, rest: &[String]) {
        let mut found = self.defined.borrow_mut();
        let mut words = rest.iter();
        while let Some(word) = words.next() {
            if let Some(name) = word.strip_prefix("alias.")
                && let Some(value) = words.next()
            {
                found.insert(name.to_string(), value.clone());
            }
        }
    }
}

fn value_of(args: &[String], flags: &[&str]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if flags.contains(&arg.as_str()) {
            return iter.next().cloned();
        }
        for flag in flags.iter().filter(|flag| flag.starts_with("--")) {
            if let Some(value) = arg.strip_prefix(&format!("{flag}=")) {
                return Some(value.to_string());
            }
        }
    }
    None
}

enum Unwrapped<'a> {
    Command(&'a [String]),
    Script(String),
}

/// Programs that run another command given in their arguments. Returns that command.
fn unwrap_wrapper<'a>(name: &str, args: &'a [String]) -> Option<Unwrapped<'a>> {
    let with_value: &[&str] = match name {
        "env" => &["-u", "--unset", "-C", "--chdir"],
        "command" | "builtin" | "nohup" | "setsid" | "unbuffer" | "caffeinate" | "exec" => &["-a", "-t", "-w"],
        "time" => &["-f", "-o"],
        "nice" => &["-n"],
        "ionice" => &["-c", "-n", "-p"],
        "sudo" => &["-u", "-g", "-h", "-p", "-C", "-D", "-R", "-T", "-U", "-r", "-t"],
        "doas" => &["-u", "-C"],
        "stdbuf" => &["-i", "-o", "-e"],
        "timeout" => &["-s", "-k", "--signal", "--kill-after"],
        "xargs" => &[
            "-I",
            "-J",
            "-n",
            "-P",
            "-L",
            "-s",
            "-E",
            "-d",
            "-a",
            "--arg-file",
            "--delimiter",
            "--max-args",
            "--max-procs",
            "--replace",
        ],
        "watch" => &["-n", "-d", "--interval"],
        _ => return None,
    };
    let mut index = 0;
    let mut duration_skipped = false;
    while let Some(arg) = args.get(index) {
        if name == "env" && (arg == "-S" || arg == "--split-string") {
            return args.get(index + 1).map(|text| Unwrapped::Script(text.clone()));
        }
        if !arg.starts_with('-') || arg == "-" {
            if name == "env" && is_assignment(arg) {
                index += 1;
                continue;
            }
            if name == "timeout" && !duration_skipped {
                duration_skipped = true;
                index += 1;
                continue;
            }
            break;
        }
        if arg == "--" {
            index += 1;
            break;
        }
        index += if with_value.contains(&arg.as_str()) { 2 } else { 1 };
    }
    Some(Unwrapped::Command(args.get(index..).unwrap_or_default()))
}

/// The commands `find` runs with `-exec`, `-execdir`, `-ok` or `-okdir`.
fn exec_commands(args: &[String]) -> Vec<&[String]> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if matches!(args[index].as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
            let start = index + 1;
            let end = args[start..]
                .iter()
                .position(|arg| arg == ";" || arg == "+" || arg == "\\;")
                .map_or(args.len(), |at| start + at);
            found.push(&args[start..end]);
            index = end;
        }
        index += 1;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_aliases(_: &str) -> Option<String> {
        None
    }

    fn check(command: &str) -> Option<Violation> {
        inspect(command, None, &no_aliases)
    }

    #[test]
    fn blocks_every_form_of_git_push() {
        let pushes = [
            "git push",
            "git push origin main",
            "git push --force-with-lease origin HEAD:main",
            "git -C . push",
            "git -C /some/repo push origin",
            "git -c user.name=x push",
            "git --no-pager push",
            "git --git-dir=.git --work-tree=. push",
            "git --git-dir .git push",
            "/usr/bin/git push",
            "/opt/homebrew/bin/git -C . push",
            "git-push origin",
            "GIT_TRACE=1 git push",
            "env git push",
            "env -i HOME=/x git push",
            "command git push",
            "exec git push",
            "nohup git push &",
            "time git push",
            "sudo -u deploy git push",
            "nice -n 5 git push",
            "timeout 30 git push",
            "echo origin | xargs git push",
            "xargs -I{} git push {}",
            "find . -name x -exec git push {} \\;",
            "find . -execdir git push {} +",
            "sh -c 'git push'",
            "bash -c \"git -C . push origin main\"",
            "bash -lc 'cd sub && git push'",
            "zsh -c 'echo hi; git push'",
            "sh -c \"sh -c 'git push'\"",
            "eval 'git push'",
            "eval git push",
            "echo hi && git push",
            "false || git push",
            "git status; git push",
            "timeout -s KILL 30 git push",
            "git status\ngit push",
            "(git push)",
            "{ git push; }",
            "if true; then git push; fi",
            "for r in a b; do git push $r; done",
            "echo $(git push)",
            "echo \"$(git push)\"",
            "echo `git push`",
            "diff <(git push) x",
            "git push 2>&1",
            "git push >/dev/null",
            "git \\\npush",
            "git p''ush",
            "git 'push'",
            "\"git\" push",
            "git send-pack origin",
            "git submodule foreach 'git push'",
            "git rebase -x 'git push' main",
            "git rebase --exec='git push' main",
            "git bisect run git push",
            "git -c alias.p=push p",
            "git -c alias.ship='!git push' ship",
            "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=alias.p GIT_CONFIG_VALUE_0=push git p",
            "git config alias.p push && git p",
            "git config --global alias.p '!git push' ; git p",
            "$GIT push",
            "\"$(which git)\" push origin",
            "python3 -c \"import os; os.system('git push')\"",
            "python -c \"import subprocess; subprocess.run(['git','-C','.','push'])\"",
            "node -e \"require('child_process').execSync('git push')\"",
            "ruby -e 'system(\"git push\")'",
            "perl -e 'system(\"git\", \"push\")'",
            "awk 'BEGIN{system(\"git push\")}'",
            "osascript -e 'do shell script \"git push\"'",
            "bash <<EOF\ngit push\nEOF",
            "bash <<'EOF'\n  echo hi\n  git push\nEOF",
            "python3 - <<EOF\nimport os\nos.system('git push')\nEOF",
        ];
        for command in pushes {
            assert!(check(command).is_some(), "should block: {command:?}");
        }
        assert_eq!(check("git -C . push"), Some(Violation::Push));
    }

    #[test]
    fn blocks_commits_and_the_github_cli() {
        for command in [
            "git commit -m x",
            "git -C . commit -am x",
            "sh -c 'git commit --amend'",
            "git commit-tree abc",
        ] {
            assert_eq!(check(command), Some(Violation::Commit), "{command}");
        }
        for command in [
            "gh pr create",
            "gh api repos/x/y",
            "/opt/homebrew/bin/gh auth status",
            "xargs gh pr view",
            "hub push",
            "glab mr create",
        ] {
            assert_eq!(check(command), Some(Violation::GitHubCli), "{command}");
        }
    }

    #[test]
    fn allows_ordinary_commands() {
        let safe = [
            "git status",
            "git diff --merge-base main",
            "git log --oneline -n 5",
            "git log --grep=push",
            "git show HEAD:README.md",
            "git -C . status --short",
            "git add -A",
            "git checkout -- a.ts",
            "git stash list",
            "git fetch origin",
            "git pull --ff-only",
            "git remote -v",
            "git branch --list push-notifications",
            "git config user.name",
            "git config --get alias.co",
            "echo \"do not git push\"",
            "echo 'git push is forbidden'",
            "grep -rn \"git push\" docs/",
            "rg push src/",
            "cat notes.md",
            "cat > release.sh <<'EOF'\n#!/bin/sh\ngit push origin main\nEOF",
            "printf 'git push' > note.txt",
            "npm test",
            "cargo test -p diffpilot-core",
            "make build",
            "ls -la",
            "sed -i '' 's/push/pull/' a.txt",
            "python3 -c \"print('push the button')\"",
            "node -e \"console.log(1)\"",
            "sh -c 'echo hi'",
            "bash -c 'npm run build && npm test'",
            "xargs rm -f",
            "find . -name '*.orig' -exec rm {} \\;",
            "env FOO=1 npm test",
            "timeout 30 cargo test",
            "cd sub && git status",
            "git -c core.pager=cat log",
            "pushd /tmp",
            "# git push",
            "",
        ];
        for command in safe {
            assert_eq!(check(command), None, "should allow: {command:?}");
        }
    }

    #[test]
    fn follows_a_git_alias_from_the_repo_config() {
        let aliases = |name: &str| match name {
            "ship" => Some("push origin HEAD".to_string()),
            "go" => Some("!git push && echo done".to_string()),
            "st" => Some("status -sb".to_string()),
            "chain" => Some("ship".to_string()),
            _ => None,
        };
        assert_eq!(inspect("git ship", None, &aliases), Some(Violation::Push));
        assert_eq!(inspect("git -C . go", None, &aliases), Some(Violation::Push));
        assert_eq!(inspect("git chain", None, &aliases), Some(Violation::Push));
        assert_eq!(inspect("git st", None, &aliases), None);
        assert_eq!(inspect("git log", None, &aliases), None);
    }

    #[test]
    fn a_self_referencing_alias_does_not_loop() {
        let aliases = |name: &str| (name == "loop").then(|| "loop".to_string());
        assert_eq!(inspect("git loop", None, &aliases), Some(Violation::TooNested));
    }

    #[test]
    fn reads_script_files_the_command_runs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("release.sh"),
            "#!/bin/sh\nset -e\ngit -C . push origin main\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("safe.sh"), "#!/bin/sh\necho hi\n").unwrap();
        std::fs::write(
            dir.path().join("deploy.py"),
            "import subprocess\nsubprocess.run(['git', 'push'])\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("tool.py"), "print('hello')\n").unwrap();
        std::fs::write(dir.path().join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
        let check_in = |command: &str| inspect(command, Some(dir.path()), &no_aliases);
        for command in [
            "sh release.sh",
            "bash ./release.sh",
            "./release.sh",
            "source release.sh",
            ". ./release.sh",
            "python3 deploy.py",
            "zsh -x release.sh",
        ] {
            assert!(check_in(command).is_some(), "should block: {command}");
        }
        for command in [
            "sh safe.sh",
            "./safe.sh",
            "python3 tool.py",
            "./blob.bin",
            "sh missing.sh",
        ] {
            assert_eq!(check_in(command), None, "should allow: {command}");
        }
    }

    #[test]
    fn deeply_nested_commands_are_refused() {
        let mut command = "echo hi".to_string();
        for _ in 0..12 {
            command = format!("sh -c '{}'", command.replace('\'', "'\\''"));
        }
        assert_eq!(check(&command), Some(Violation::TooNested));
    }

    fn hook(input: &str) -> HookDecision {
        decide_hook(input, None, &|_, _| None)
    }

    #[test]
    fn the_hook_blocks_a_push_with_a_message_the_log_can_recognise() {
        let input = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":"/r","tool_input":{"command":"git -C . push origin HEAD","description":"push"}}"#;
        let HookDecision::Block(message) = hook(input) else {
            panic!("not blocked");
        };
        assert!(message.starts_with(BLOCK_MARKER));
        assert!(message.contains("git push"));
    }

    #[test]
    fn the_hook_allows_safe_commands_and_other_tools() {
        let bash = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
        assert_eq!(hook(bash), HookDecision::Allow);
        let edit = r#"{"tool_name":"Edit","tool_input":{"file_path":"a","command":"git push"}}"#;
        assert_eq!(hook(edit), HookDecision::Allow);
    }

    #[test]
    fn the_hook_fails_closed_on_input_it_cannot_read() {
        assert!(matches!(hook("not json"), HookDecision::Block(_)));
        assert!(matches!(
            hook(r#"{"tool_name":"Bash","tool_input":{}}"#),
            HookDecision::Block(_)
        ));
    }

    #[test]
    fn the_hook_asks_for_aliases_in_the_run_directory() {
        let input = r#"{"tool_name":"Bash","cwd":"/work/repo","tool_input":{"command":"git ship"}}"#;
        let seen = RefCell::new(None);
        let decision = decide_hook(input, None, &|dir, name| {
            *seen.borrow_mut() = Some((dir.to_path_buf(), name.to_string()));
            Some("push".to_string())
        });
        assert!(matches!(decision, HookDecision::Block(_)));
        assert_eq!(
            seen.into_inner(),
            Some((PathBuf::from("/work/repo"), "ship".to_string()))
        );
    }
}
