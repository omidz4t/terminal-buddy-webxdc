//! Gated interactive shell for whitelist / approve modes.
//!
//! Modes:
//! - **whitelist**: only listed binaries may run
//! - **approve**: every command waits for host TUI approval (file gate)
//!
//! Uses **rustyline** for line editing + Tab completion (commands + paths).
//! YOLO (full shell) does not use this process.

use std::borrow::Cow;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config, Context, Editor, Helper};
use serde_json::{json, Value};

/// Entry: `restricted-shell --mode whitelist|approve [--allowed a,b] --gate-dir PATH`
pub fn run(args: &[String]) -> Result<()> {
    let opts = Arc::new(parse_opts(args)?);
    fs::create_dir_all(&opts.gate_dir).context("create gate dir")?;
    fs::create_dir_all(opts.gate_dir.join("req")).ok();
    fs::create_dir_all(opts.gate_dir.join("res")).ok();

    println!("TerminalBuddy gated shell");
    match opts.mode.as_str() {
        "approve" => {
            println!("mode: APPROVE — each command waits for host approval");
        }
        _ => {
            println!("mode: WHITELIST");
            println!("allowed: {}", opts.allowed.join(", "));
        }
    }
    println!("builtins: help, exit, clear  ·  Tab = complete command / path");
    println!("one command per line · no pipes / redirects / shell ops");
    println!();

    let config = Config::builder()
        .completion_type(CompletionType::List)
        .tab_stop(4)
        .build();
    let mut rl: Editor<GateHelper, DefaultHistory> = Editor::with_config(config)?;
    rl.set_helper(Some(GateHelper {
        opts: opts.clone(),
        file: FilenameCompleter::new(),
    }));

    // Optional history file under gate dir
    let hist = opts.gate_dir.join("history");
    let _ = rl.load_history(&hist);

    loop {
        let readline = rl.readline("$ ");
        match readline {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(line.as_str());

                match line.as_str() {
                    "exit" | "quit" | "q" => break,
                    "help" | "?" => {
                        print_help(&opts);
                        continue;
                    }
                    "clear" => {
                        print!("\x1b[2J\x1b[H");
                        let _ = io::stdout().flush();
                        continue;
                    }
                    _ => {}
                }

                if let Err(e) = validate_line(&line) {
                    eprintln!("denied: {e}");
                    continue;
                }

                let tokens = tokenize(&line);
                if tokens.is_empty() {
                    continue;
                }
                let cmd_name = tokens[0].clone();

                match opts.mode.as_str() {
                    "whitelist" => {
                        if !is_allowed(&cmd_name, &opts.allowed) {
                            eprintln!(
                                "denied: `{cmd_name}` not in whitelist ({})",
                                opts.allowed.join(", ")
                            );
                            continue;
                        }
                        run_command(&tokens);
                    }
                    "approve" => {
                        // Live whitelist file (host may add programs while approving).
                        let live_allow = load_live_allow(&opts.gate_dir, &opts.allowed);
                        if is_allowed(&cmd_name, &live_allow) {
                            println!("\x1b[32m[whitelisted]\x1b[0m running…");
                            run_command(&tokens);
                            continue;
                        }
                        match request_approval(&opts.gate_dir, &line, &tokens) {
                            Ok(true) => {
                                println!("\x1b[32m[host approved]\x1b[0m running…");
                                run_command(&tokens);
                            }
                            Ok(false) => {
                                eprintln!("\x1b[31m[host denied]\x1b[0m `{line}`");
                            }
                            Err(e) => {
                                eprintln!("approval error: {e:#}");
                            }
                        }
                    }
                    other => eprintln!("unknown mode {other}"),
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C: new prompt
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(e) => {
                eprintln!("readline: {e}");
                break;
            }
        }
    }

    let _ = rl.save_history(&hist);
    Ok(())
}

// ── Tab completion ──────────────────────────────────────────────────────────

struct GateHelper {
    opts: Arc<Opts>,
    file: FilenameCompleter,
}

impl Helper for GateHelper {}
impl Hinter for GateHelper {
    type Hint = String;
    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        None
    }
}
impl Highlighter for GateHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        Cow::Borrowed(line)
    }
    fn highlight_char(&self, _line: &str, _pos: usize, _kind: CmdKind) -> bool {
        false
    }
}
impl Validator for GateHelper {}

impl Completer for GateHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let before = &line[..pos];
        // First token (command) vs later tokens (paths / args).
        let is_first = !before.contains(char::is_whitespace);

        if is_first {
            let start = before.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
            let prefix = &before[start..];
            let mut pairs = command_completions(prefix, &self.opts);
            // Also offer path completion for ./binary etc.
            if let Ok((s, mut path_pairs)) = self.file.complete(line, pos, ctx) {
                if s == start {
                    pairs.append(&mut path_pairs);
                }
            }
            pairs.sort_by(|a, b| a.display.cmp(&b.display));
            pairs.dedup_by(|a, b| a.display == b.display);
            return Ok((start, pairs));
        }

        // Arguments: filename completion
        self.file.complete(line, pos, ctx)
    }
}

fn command_completions(prefix: &str, opts: &Opts) -> Vec<Pair> {
    let mut out = Vec::new();
    let mut candidates: Vec<String> = vec![
        "help".into(),
        "exit".into(),
        "clear".into(),
        "quit".into(),
    ];

    match opts.mode.as_str() {
        "whitelist" => {
            candidates.extend(opts.allowed.iter().cloned());
        }
        "approve" => {
            // PATH executables (basename match)
            candidates.extend(path_executables());
            // Soft whitelist still useful as suggestions
            candidates.extend(opts.allowed.iter().cloned());
        }
        _ => {}
    }

    candidates.sort();
    candidates.dedup();
    for c in candidates {
        if c.starts_with(prefix) {
            out.push(Pair {
                display: c.clone(),
                replacement: c,
            });
        }
    }
    out
}

fn path_executables() -> Vec<String> {
    let mut names = Vec::new();
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let Ok(rd) = fs::read_dir(dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let path = ent.path();
            let Ok(meta) = ent.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            // Unix: executable bit; always suggest if we can read name
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if !name.starts_with('.') {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    names.dedup();
    // Cap size so completion stays snappy
    if names.len() > 4000 {
        names.truncate(4000);
    }
    names
}

// ── Options / execution ─────────────────────────────────────────────────────

struct Opts {
    mode: String,
    allowed: Vec<String>,
    gate_dir: PathBuf,
}

fn parse_opts(args: &[String]) -> Result<Opts> {
    let mut mode = std::env::var("REMOTE_XTERM_MODE").unwrap_or_else(|_| "whitelist".into());
    let mut allowed = std::env::var("REMOTE_XTERM_ALLOW")
        .map(|s| split_allowed_list(&s))
        .unwrap_or_default();
    let mut gate_dir = std::env::var("REMOTE_XTERM_GATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".dc-agent/gate"));

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--mode" => {
                if let Some(v) = args.get(i + 1) {
                    mode = v.clone();
                    i += 2;
                    continue;
                }
            }
            "--allowed" | "--allowed-commands" => {
                if let Some(v) = args.get(i + 1) {
                    allowed.extend(split_allowed_list(v));
                    i += 2;
                    continue;
                }
            }
            "--gate-dir" => {
                if let Some(v) = args.get(i + 1) {
                    gate_dir = PathBuf::from(v);
                    i += 2;
                    continue;
                }
            }
            s if s.starts_with("--mode=") => {
                mode = s.trim_start_matches("--mode=").into();
            }
            s if s.starts_with("--allowed=") || s.starts_with("--allowed-commands=") => {
                let v = s.split_once('=').map(|(_, r)| r).unwrap_or("");
                allowed.extend(split_allowed_list(v));
            }
            s if s.starts_with("--gate-dir=") => {
                gate_dir = PathBuf::from(s.trim_start_matches("--gate-dir="));
            }
            _ => {}
        }
        i += 1;
    }

    allowed.sort();
    allowed.dedup();
    mode = mode.to_lowercase();
    if mode != "whitelist" && mode != "approve" {
        bail!("mode must be whitelist or approve (got {mode})");
    }
    if mode == "whitelist" && allowed.is_empty() {
        bail!("whitelist mode needs at least one --allowed command");
    }
    Ok(Opts {
        mode,
        allowed,
        gate_dir,
    })
}

fn print_help(opts: &Opts) {
    println!("mode: {}", opts.mode);
    if opts.mode == "whitelist" {
        println!("allowed:");
        for c in &opts.allowed {
            println!("  {c}");
        }
    } else {
        println!("every command requires host approval in the agent TUI (Shell tab).");
        println!("Tab completes PATH commands and file paths.");
    }
    println!("builtins: help, exit, clear");
    println!("Tab: complete command names and file paths");
}

fn run_command(tokens: &[String]) {
    let cmd_name = &tokens[0];
    let status = Command::new(cmd_name)
        .args(&tokens[1..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    match status {
        Ok(st) => {
            if !st.success() {
                if let Some(code) = st.code() {
                    eprintln!("[exit {code}]");
                } else {
                    eprintln!("[terminated by signal]");
                }
            }
        }
        Err(e) => eprintln!("failed to run `{cmd_name}`: {e}"),
    }
}

/// Merge startup allow-list with `gate/allow.txt` (updated by host when approving “to whitelist”).
fn load_live_allow(gate_dir: &Path, base: &[String]) -> Vec<String> {
    let mut out = base.to_vec();
    let path = gate_dir.join("allow.txt");
    if let Ok(raw) = fs::read_to_string(path) {
        for part in raw.split(|c: char| c == ',' || c == '\n' || c == ' ') {
            let t = part.trim();
            if !t.is_empty() {
                out.push(t.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn request_approval(gate_dir: &Path, line: &str, tokens: &[String]) -> Result<bool> {
    let id = format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        std::process::id()
    );
    let req_path = gate_dir.join("req").join(format!("{id}.json"));
    let res_path = gate_dir.join("res").join(format!("{id}.json"));
    let _ = fs::remove_file(&res_path);

    let body = json!({
        "id": id,
        "line": line,
        "cmd": tokens[0],
        "args": tokens[1..],
        "ts": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });
    fs::write(&req_path, body.to_string()).context("write approval request")?;

    println!("\x1b[33m⏳ waiting for host approval:\x1b[0m {line}");
    println!("   (agent TUI → Shell tab → Y approve / n deny)");
    io::stdout().flush().ok();

    for _ in 0..6000 {
        if res_path.is_file() {
            let raw = fs::read_to_string(&res_path).unwrap_or_default();
            let _ = fs::remove_file(&res_path);
            let _ = fs::remove_file(&req_path);
            let v: Value = serde_json::from_str(&raw).unwrap_or(json!({}));
            return Ok(v.get("allow").and_then(|x| x.as_bool()).unwrap_or(false));
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = fs::remove_file(&req_path);
    bail!("approval timed out");
}

/// Split `"ls,cat pwd"` into tokens.
pub fn split_allowed_list(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

fn validate_line(line: &str) -> Result<(), String> {
    const FORBIDDEN: &[char] = &[
        '|', '&', ';', '<', '>', '(', ')', '{', '}', '`', '$', '\n', '\r', '!', '*', '?', '[', ']',
        '\\',
    ];
    if line.chars().any(|c| FORBIDDEN.contains(&c)) {
        return Err("shell operators / metacharacters are not allowed".into());
    }
    if line.contains("&&") || line.contains("||") {
        return Err("command chaining is not allowed".into());
    }
    Ok(())
}

fn tokenize(line: &str) -> Vec<String> {
    line.split_whitespace().map(|s| s.to_string()).collect()
}

fn is_allowed(cmd: &str, allowed: &[String]) -> bool {
    let base = Path::new(cmd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd);
    if cmd.contains('/') || cmd.contains('\\') {
        return allowed.iter().any(|a| a == cmd);
    }
    allowed.iter().any(|a| {
        a == cmd
            || a == base
            || Path::new(a)
                .file_name()
                .and_then(|s| s.to_str())
                .map(|b| b == base)
                .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_lists() {
        assert_eq!(
            split_allowed_list("ls,cat, pwd  whoami"),
            vec!["ls", "cat", "pwd", "whoami"]
        );
    }

    #[test]
    fn rejects_meta() {
        assert!(validate_line("ls | cat").is_err());
        assert!(validate_line("ls").is_ok());
    }

    #[test]
    fn allow_match() {
        let a = vec!["ls".into(), "cat".into()];
        assert!(is_allowed("ls", &a));
        assert!(!is_allowed("rm", &a));
    }
}
