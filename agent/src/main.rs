//! TerminalBuddy agent — system PTY over webxdc realtime, with a **Ratatui** TUI.
//!
//! ## Realtime roles (webxdc mesh)
//!
//! - **Peers** (everyone who opens the mini app) propagate what they want:
//!   key input `I`, resize `R`, ping `P`, and presence JSON (`pos` cursors,
//!   `rx` reactions, `join`/`leave`/`writer`). Cursors/reactions fan out on the
//!   Iroh gossip channel **naturally** between webxdc clients — the host does
//!   not rebroadcast them.
//! - **Host** (this agent) is the only writer of terminal content: it owns the
//!   PTY and sends back `O` (output) / `E` (exit). It ignores peer `O`/`E`.
//!   Presence JSON is observed only for the TUI (who is online / writing).
//!
//! ```text
//! terminal_buddy --help
//! terminal_buddy --version
//! terminal_buddy join [invite]
//! terminal_buddy serve [app.xdc]     # optional path; default = embedded .xdc
//! terminal_buddy send-file <path>
//! ```

mod restricted_shell;
mod tui;

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use deltachat::chat::{self, ChatId, ChatItem};
use deltachat::chatlist::Chatlist;
use deltachat::config::Config;
use deltachat::contact::{Contact, ContactId};
use deltachat::context::{Context, ContextBuilder};
use deltachat::message::{Message, MessageState, MsgId, Viewtype};
use deltachat::peer_channels::{
    send_webxdc_realtime_advertisement, send_webxdc_realtime_data,
};
use deltachat::securejoin;
use deltachat::stock_str::StockStrings;
use deltachat::{EventEmitter, EventType, Events};
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use tokio::sync::mpsc;

const DATA_DIR: &str = ".dc-agent";
const DB_NAME: &str = "db.sqlite";
const DEFAULT_SERVER: &str = "nine.testrun.org";
/// Fallback on-disk path when no CLI path and no embed (dev tree).
const DEFAULT_XDC: &str = "../webxdc/terminalbuddy.xdc";
const GATE_DIR: &str = ".dc-agent/gate";
/// Profile display name shown to other Delta Chat users.
const PROFILE_NAME: &str = "TerminalBuddy";
/// Profile / branding avatar (shipped with the agent crate).
const PROFILE_AVATAR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/avatar.png");
/// Project version from repo `VERSION` (set in `build.rs`).
const APP_VERSION: &str = env!("TERMINAL_BUDDY_VERSION");
/// Mini app bytes embedded at build time (`make build` → assets/terminalbuddy.xdc).
const EMBEDDED_XDC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded_xdc.bin"));

const KIND_INPUT: u8 = b'I';
const KIND_OUTPUT: u8 = b'O';
const KIND_EXIT: u8 = b'E';
const KIND_RESIZE: u8 = b'R';
const KIND_PING: u8 = b'P';

const LOG_CAP: usize = 400;

// ── friendly console (pre-TUI bootstrap) ────────────────────────────────────

fn say(msg: &str) {
    println!("  {msg}");
}

fn say_ok(msg: &str) {
    println!("  ✓  {msg}");
}

fn say_step(step: u32, total: u32, msg: &str) {
    println!("  [{step}/{total}]  {msg}");
}

fn say_banner(title: &str) {
    println!();
    println!("  ╭──────────────────────────────────────────────╮");
    println!("  │  {title:<44}│");
    println!("  ╰──────────────────────────────────────────────╯");
    println!();
}

fn human_bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if n >= MB {
        format!("{:.1} MB", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.1} KB", n as f64 / KB as f64)
    } else {
        format!("{n} B")
    }
}

fn smtp_size_hint(msg: &str) -> Option<u64> {
    msg.split("len=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
}

// ── shared host state (read by TUI, written by event loop) ──────────────────

/// Someone who may open the webxdc (chat member) or is live on P2P.
#[derive(Clone, Debug)]
pub struct AccessPeer {
    pub label: String,
    pub addr: String,
    /// Online in realtime channel right now.
    pub online: bool,
    /// Recently typed into the shared PTY.
    pub writing: bool,
    /// May type into the terminal (party mode, allow-list, or host).
    pub can_write: bool,
}

/// A chat or contact the host can send the webxdc to (Share tab).
#[derive(Clone, Debug)]
pub struct ShareTarget {
    pub label: String,
    /// Secondary line: addr or "group" / "direct".
    pub detail: String,
    pub kind: ShareKind,
    pub chat_id: Option<u32>,
    pub contact_id: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareKind {
    Chat,
    Contact,
}

/// How the hosted shell treats commands (Shell tab).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellPolicy {
    /// Full shell / tmux — no command filtering (YOLO).
    Yolo,
    /// Only binaries on the whitelist may run (gated shell).
    Whitelist,
    /// Every binary waits for host approval in the TUI (gated shell).
    Approve,
}

impl ShellPolicy {
    pub fn label(self) -> &'static str {
        match self {
            ShellPolicy::Yolo => "YOLO (all allow)",
            ShellPolicy::Whitelist => "whitelist",
            ShellPolicy::Approve => "approve",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            ShellPolicy::Yolo => "yolo",
            ShellPolicy::Whitelist => "whitelist",
            ShellPolicy::Approve => "approve",
        }
    }
}

/// Pending command waiting for host yes/no (approve mode).
#[derive(Clone, Debug)]
pub struct PendingCmd {
    pub id: String,
    pub line: String,
    pub cmd: String,
}

/// Queued Share-tab action for the background worker.
#[derive(Clone, Debug)]
enum PendingShare {
    /// SecureJoin invite URL → join + send.
    Invite(String),
    /// Existing chat id.
    Chat(u32),
    /// Contact id → create/open 1:1 + send.
    Contact(u32),
}

#[derive(Clone, Debug)]
pub struct HostSnapshot {
    pub identity: String,
    pub chat_id: String,
    pub primary_msg: String,
    pub shell: String,
    pub xdc_path: String,
    pub hosting: usize,
    pub peers_seen: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub pings: u64,
    pub status: String,
    pub logs: Vec<String>,
    /// Chat members (policy: all of them can write if they open the app).
    pub chat_members: Vec<AccessPeer>,
    /// Live P2P presence (from JSON join/writer/pos).
    pub live_peers: Vec<AccessPeer>,
    pub last_writer: String,
    /// Whether the hosted PTY is tmux (vs plain $SHELL).
    pub use_tmux: bool,
    /// tmux available on PATH.
    pub tmux_available: bool,
    /// Whitelist binaries (used when shell_policy is Whitelist).
    pub allowed_commands: Vec<String>,
    /// YOLO / whitelist / approve (Shell tab).
    pub shell_policy: ShellPolicy,
    /// Commands waiting for host y/n (approve mode).
    pub pending_cmds: Vec<PendingCmd>,
    /// Party mode: everyone in the chat may type. Off = only allow-listed addrs.
    pub party_mode: bool,
    /// Invite URL currently bound / being edited (full string).
    pub invite_url: String,
    /// Short status under the invite field (e.g. “joining…”, “sent”).
    pub invite_hint: String,
    /// True while join+send from the TUI is running.
    pub invite_busy: bool,
    /// Chats + contacts for Share tab.
    pub share_targets: Vec<ShareTarget>,
}

impl Default for HostSnapshot {
    fn default() -> Self {
        Self {
            identity: "…".into(),
            chat_id: "…".into(),
            primary_msg: "…".into(),
            shell: "…".into(),
            xdc_path: "…".into(),
            hosting: 0,
            peers_seen: 0,
            bytes_in: 0,
            bytes_out: 0,
            pings: 0,
            status: "starting".into(),
            logs: Vec::new(),
            chat_members: Vec::new(),
            live_peers: Vec::new(),
            last_writer: "—".into(),
            use_tmux: false,
            tmux_available: which_cmd("tmux"),
            allowed_commands: Vec::new(),
            shell_policy: ShellPolicy::Yolo,
            pending_cmds: Vec::new(),
            party_mode: true,
            invite_url: String::new(),
            invite_hint: "i = edit invite · Enter = join + send webxdc".into(),
            invite_busy: false,
            share_targets: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct LivePeer {
    name: String,
    addr: String,
    writing: bool,
    last_seen: std::time::Instant,
    writing_at: Option<std::time::Instant>,
}

struct SharedState {
    snap: Mutex<HostSnapshot>,
    logs: Mutex<VecDeque<String>>,
    live: Mutex<HashMap<String, LivePeer>>,
    peers_seen: AtomicU64,
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    pings: AtomicU64,
    /// Set when user hits `q` / Esc — event loops exit, process stops.
    shutdown: AtomicBool,
    /// Host PTY uses tmux (false by default — plain $SHELL).
    use_tmux: AtomicBool,
    /// TUI / CLI requested a PTY restart (e.g. toggled tmux on Shell tab).
    restart_pty: AtomicBool,
    /// MsgIds we are currently hosting (for restart).
    hosted_msgs: Mutex<Vec<u32>>,
    /// Whitelist command names (Shell tab whitelist mode).
    allowed_commands: Mutex<Vec<String>>,
    shell_policy: Mutex<ShellPolicy>,
    pending_cmds: Mutex<Vec<PendingCmd>>,
    /// (id, allow) for webxdc cmd_done broadcasts.
    cmd_done_queue: Mutex<Vec<(String, bool)>>,
    /// Party mode (true = everyone may type). Default on.
    party_mode: AtomicBool,
    /// Lowercased addrs allowed to type when party_mode is false. Host always allowed.
    write_allow: Mutex<std::collections::HashSet<String>>,
    /// Draft invite URL edited on Share tab (source of truth for the field).
    invite_url: Mutex<String>,
    /// When set, background worker joins invite / sends to chat or contact.
    pending_share: Mutex<Option<PendingShare>>,
    invite_busy: AtomicBool,
}

impl SharedState {
    fn new(use_tmux: bool, allowed: Vec<String>, invite: String) -> Arc<Self> {
        let policy = if allowed.is_empty() {
            ShellPolicy::Yolo
        } else {
            ShellPolicy::Whitelist
        };
        let mut snap = HostSnapshot::default();
        snap.use_tmux = use_tmux && policy == ShellPolicy::Yolo;
        snap.tmux_available = which_cmd("tmux");
        snap.allowed_commands = allowed.clone();
        snap.shell_policy = policy;
        snap.shell = shell_label_for(use_tmux, policy, &allowed);
        snap.party_mode = true;
        snap.invite_url = invite.clone();
        let _ = std::fs::create_dir_all(Path::new(GATE_DIR).join("req"));
        let _ = std::fs::create_dir_all(Path::new(GATE_DIR).join("res"));
        Arc::new(Self {
            snap: Mutex::new(snap),
            logs: Mutex::new(VecDeque::with_capacity(LOG_CAP)),
            live: Mutex::new(HashMap::new()),
            peers_seen: AtomicU64::new(0),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            pings: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            use_tmux: AtomicBool::new(use_tmux && policy == ShellPolicy::Yolo),
            restart_pty: AtomicBool::new(false),
            hosted_msgs: Mutex::new(Vec::new()),
            allowed_commands: Mutex::new(allowed),
            shell_policy: Mutex::new(policy),
            pending_cmds: Mutex::new(Vec::new()),
            cmd_done_queue: Mutex::new(Vec::new()),
            party_mode: AtomicBool::new(true),
            write_allow: Mutex::new(std::collections::HashSet::new()),
            invite_url: Mutex::new(invite),
            pending_share: Mutex::new(None),
            invite_busy: AtomicBool::new(false),
        })
    }

    pub fn shell_policy(&self) -> ShellPolicy {
        *self.shell_policy.lock().unwrap()
    }

    pub fn set_shell_policy(&self, policy: ShellPolicy) {
        let prev = *self.shell_policy.lock().unwrap();
        if prev == policy {
            return;
        }
        if policy == ShellPolicy::Whitelist {
            let n = self.allowed_commands.lock().unwrap().len();
            if n == 0 {
                // Sensible defaults so the gated shell can start.
                let defaults = vec![
                    "ls".into(),
                    "pwd".into(),
                    "whoami".into(),
                    "uname".into(),
                    "cat".into(),
                    "echo".into(),
                    "date".into(),
                ];
                *self.allowed_commands.lock().unwrap() = defaults.clone();
                self.patch(|s| s.allowed_commands = defaults);
                self.log("shell: whitelist seeded with ls,pwd,whoami,uname,cat,echo,date");
            }
        }
        *self.shell_policy.lock().unwrap() = policy;
        let use_tmux = self.use_tmux() && policy == ShellPolicy::Yolo;
        if policy != ShellPolicy::Yolo {
            self.use_tmux.store(false, Ordering::SeqCst);
        }
        let allowed = self.allowed_commands();
        let label = shell_label_for(use_tmux, policy, &allowed);
        self.patch(|s| {
            s.shell_policy = policy;
            s.use_tmux = use_tmux;
            s.shell = label;
            s.allowed_commands = allowed;
        });
        self.sync_allow_file();
        self.log(format!("shell policy → {} (restarting PTY…)", policy.label()));
        self.restart_pty.store(true, Ordering::SeqCst);
    }

    pub fn add_whitelist_cmd(&self, name: &str) {
        self.add_whitelist_cmd_no_restart(name);
        self.sync_allow_file();
        self.log(format!("whitelist + {name}"));
        // Whitelist mode must restart so the gated shell picks up argv --allowed.
        // Approve mode reads allow.txt live — no restart needed.
        if self.shell_policy() == ShellPolicy::Whitelist {
            self.restart_pty.store(true, Ordering::SeqCst);
        }
    }

    pub fn remove_whitelist_cmd(&self, name: &str) {
        let mut list = self.allowed_commands.lock().unwrap();
        list.retain(|c| c != name);
        let out = list.clone();
        drop(list);
        self.patch(|s| s.allowed_commands = out);
        self.sync_allow_file();
        self.log(format!("whitelist − {name}"));
        if self.shell_policy() == ShellPolicy::Whitelist {
            self.restart_pty.store(true, Ordering::SeqCst);
        }
    }

    /// Resolve a pending approval.
    /// - `allow`: run once
    /// - `to_whitelist`: also add the binary to the whitelist (and `allow.txt` for live approve mode)
    pub fn resolve_pending_cmd(&self, id: &str, allow: bool, to_whitelist: bool) {
        let cmd_name = self
            .pending_cmds
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.cmd.clone());

        let res_dir = Path::new(GATE_DIR).join("res");
        let req_dir = Path::new(GATE_DIR).join("req");
        let _ = std::fs::create_dir_all(&res_dir);
        let body = serde_json::json!({ "id": id, "allow": allow });
        let _ = std::fs::write(res_dir.join(format!("{id}.json")), body.to_string());
        let _ = std::fs::remove_file(req_dir.join(format!("{id}.json")));
        self.pending_cmds.lock().unwrap().retain(|p| p.id != id);
        self.patch(|s| s.pending_cmds.retain(|p| p.id != id));
        self.cmd_done_queue
            .lock()
            .unwrap()
            .push((id.to_string(), allow));

        if allow && to_whitelist {
            if let Some(ref name) = cmd_name {
                self.add_whitelist_cmd_no_restart(name);
                self.sync_allow_file();
                self.log(format!("approved + whitelisted `{name}`"));
            }
        } else if allow {
            self.log(format!("approved once `{id}`"));
        } else {
            self.log(format!("denied cmd {id}"));
        }
    }

    /// Add to whitelist without restarting PTY (approve mode reads allow.txt live).
    fn add_whitelist_cmd_no_restart(&self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let mut list = self.allowed_commands.lock().unwrap();
        if !list.iter().any(|c| c == name) {
            list.push(name.to_string());
            list.sort();
            list.dedup();
        }
        let out = list.clone();
        drop(list);
        self.patch(|s| s.allowed_commands = out);
    }

    /// Write current whitelist to gate/allow.txt for the gated shell to pick up live.
    pub fn sync_allow_file(&self) {
        let dir = Path::new(GATE_DIR);
        let _ = std::fs::create_dir_all(dir);
        let list = self.allowed_commands();
        let _ = std::fs::write(dir.join("allow.txt"), list.join("\n"));
    }

    fn take_cmd_done_queue(&self) -> Vec<(String, bool)> {
        std::mem::take(&mut *self.cmd_done_queue.lock().unwrap())
    }

    /// Scan gate/req for new approval requests (approve mode).
    pub fn poll_gate_requests(&self) -> Vec<PendingCmd> {
        let req_dir = Path::new(GATE_DIR).join("req");
        let mut newly = Vec::new();
        let Ok(entries) = std::fs::read_dir(&req_dir) else {
            return newly;
        };
        let mut current = self.pending_cmds.lock().unwrap();
        for ent in entries.flatten() {
            let path = ent.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let id = v
                .get("id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() || current.iter().any(|p| p.id == id) {
                continue;
            }
            let line = v
                .get("line")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let cmd = v
                .get("cmd")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let p = PendingCmd { id, line, cmd };
            newly.push(p.clone());
            current.push(p);
        }
        let snap_list = current.clone();
        drop(current);
        if !newly.is_empty() {
            self.patch(|s| s.pending_cmds = snap_list);
        }
        newly
    }

    pub fn invite_url(&self) -> String {
        self.invite_url.lock().unwrap().clone()
    }

    pub fn set_invite_url(&self, url: String) {
        *self.invite_url.lock().unwrap() = url.clone();
        self.patch(|s| s.invite_url = url);
    }

    pub fn invite_busy(&self) -> bool {
        self.invite_busy.load(Ordering::SeqCst)
    }

    fn begin_share_job(&self, hint: &str) -> bool {
        if self.invite_busy.swap(true, Ordering::SeqCst) {
            self.patch(|s| s.invite_hint = "already joining / sending…".into());
            return false;
        }
        self.patch(|s| {
            s.invite_busy = true;
            s.invite_hint = hint.into();
        });
        true
    }

    /// Queue join+send for this invite (Share tab).
    pub fn submit_invite(&self, url: String) {
        let url = normalize_invite(&url);
        if url.is_empty()
            || !(url.contains("delta.chat") || url.contains("OPENPGP4FPR") || url.contains("openpgp4fpr"))
        {
            self.patch(|s| {
                s.invite_hint =
                    "invalid invite — paste a full i.delta.chat / OPENPGP4FPR link".into();
            });
            self.log("share: rejected invite (empty or not a Delta invite)");
            return;
        }
        if !self.begin_share_job("joining chat and sending mini app…") {
            return;
        }
        self.set_invite_url(url.clone());
        *self.pending_share.lock().unwrap() = Some(PendingShare::Invite(url.clone()));
        self.log(format!(
            "share: invite queued {}",
            url.chars().take(56).collect::<String>()
        ));
        self.set_status("joining invite…");
    }

    /// Send webxdc into an existing chat (Share tab selection).
    pub fn submit_send_chat(&self, chat_id: u32, label: &str) {
        if !self.begin_share_job(&format!("sending mini app to {label}…")) {
            return;
        }
        *self.pending_share.lock().unwrap() = Some(PendingShare::Chat(chat_id));
        self.log(format!("share: send to chat {chat_id} ({label})"));
        self.set_status(format!("sending to {label}…"));
    }

    /// Open/create 1:1 with contact and send webxdc.
    pub fn submit_send_contact(&self, contact_id: u32, label: &str) {
        if !self.begin_share_job(&format!("sending mini app to {label}…")) {
            return;
        }
        *self.pending_share.lock().unwrap() = Some(PendingShare::Contact(contact_id));
        self.log(format!("share: send to contact {contact_id} ({label})"));
        self.set_status(format!("sending to {label}…"));
    }

    fn take_pending_share(&self) -> Option<PendingShare> {
        self.pending_share.lock().unwrap().take()
    }

    fn finish_invite_job(&self, ok: bool, hint: impl AsRef<str>) {
        self.invite_busy.store(false, Ordering::SeqCst);
        let hint = hint.as_ref().to_string();
        self.patch(|s| {
            s.invite_busy = false;
            s.invite_hint = hint.clone();
        });
        if ok {
            self.set_status("hosting");
        }
        self.log(format!("share: {hint}"));
    }

    pub fn allowed_commands(&self) -> Vec<String> {
        self.allowed_commands.lock().unwrap().clone()
    }

    pub fn is_gated_shell(&self) -> bool {
        matches!(
            self.shell_policy(),
            ShellPolicy::Whitelist | ShellPolicy::Approve
        )
    }

    pub fn party_mode(&self) -> bool {
        self.party_mode.load(Ordering::SeqCst)
    }

    pub fn set_party_mode(&self, on: bool) {
        self.party_mode.store(on, Ordering::SeqCst);
        self.patch(|s| s.party_mode = on);
        if on {
            self.log("access: party mode ON — everyone may type");
            self.set_status("party mode — everyone may type");
        } else {
            let n = self.write_allow.lock().unwrap().len();
            self.log(format!(
                "access: party mode OFF — allow-list only ({n} people + host)"
            ));
            self.set_status(format!("allow-list mode ({n} writers + host)"));
        }
    }

    pub fn toggle_write_allow(&self, addr: &str) {
        if addr.is_empty() {
            return;
        }
        let key = addr.to_lowercase();
        // Host always has access — no need to toggle.
        let host = self
            .snap
            .lock()
            .unwrap()
            .identity
            .to_lowercase();
        if !host.is_empty() && key == host {
            self.log("access: host always has write access");
            return;
        }
        let mut set = self.write_allow.lock().unwrap();
        if set.contains(&key) {
            set.remove(&key);
            self.log(format!("access: revoked write for {addr}"));
        } else {
            set.insert(key);
            self.log(format!("access: granted write to {addr}"));
        }
        let n = set.len();
        drop(set);
        if !self.party_mode() {
            self.set_status(format!("allow-list mode ({n} writers + host)"));
        }
    }

    pub fn is_write_allowed(&self, addr: &str) -> bool {
        if self.party_mode() {
            return true;
        }
        if addr.is_empty() {
            return false;
        }
        let key = addr.to_lowercase();
        let host = self.snap.lock().unwrap().identity.to_lowercase();
        if !host.is_empty() && key == host {
            return true;
        }
        self.write_allow.lock().unwrap().contains(&key)
    }

    fn write_allow_set(&self) -> std::collections::HashSet<String> {
        self.write_allow.lock().unwrap().clone()
    }

    fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.set_status("shutting down…");
        self.log("shutdown requested (q)");
    }

    fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    fn use_tmux(&self) -> bool {
        self.use_tmux.load(Ordering::SeqCst)
    }

    /// Enable/disable tmux for the hosted PTY (YOLO only). Restarts PTY when mode changes.
    pub fn set_use_tmux(&self, on: bool) {
        if self.is_gated_shell() {
            self.log("tmux toggle ignored — switch to YOLO mode first (Shell tab)");
            return;
        }
        if on && !which_cmd("tmux") {
            self.log("tmux not found on PATH — install tmux or stay on plain shell");
            return;
        }
        let prev = self.use_tmux.swap(on, Ordering::SeqCst);
        if prev == on {
            return;
        }
        let allowed = self.allowed_commands();
        let policy = self.shell_policy();
        let label = shell_label_for(on, policy, &allowed);
        self.patch(|s| {
            s.use_tmux = on;
            s.shell = label.clone();
        });
        self.log(if on {
            "Shell → tmux (restarting PTY…)"
        } else {
            "Shell → plain $SHELL (restarting PTY…)"
        });
        self.restart_pty.store(true, Ordering::SeqCst);
    }

    fn take_restart_pty(&self) -> bool {
        self.restart_pty.swap(false, Ordering::SeqCst)
    }

    fn register_hosted(&self, msg_id: u32) {
        let mut v = self.hosted_msgs.lock().unwrap();
        if !v.contains(&msg_id) {
            v.push(msg_id);
        }
    }

    fn hosted_ids(&self) -> Vec<u32> {
        self.hosted_msgs.lock().unwrap().clone()
    }

    fn log(&self, line: impl AsRef<str>) {
        let mut q = self.logs.lock().unwrap();
        if q.len() >= LOG_CAP {
            q.pop_front();
        }
        q.push_back(line.as_ref().to_string());
    }

    fn set_status(&self, s: impl AsRef<str>) {
        self.snap.lock().unwrap().status = s.as_ref().to_string();
    }

    fn patch<F: FnOnce(&mut HostSnapshot)>(&self, f: F) {
        f(&mut self.snap.lock().unwrap());
    }

    fn touch_live(&self, addr: &str, name: &str, writing: bool) {
        let mut map = self.live.lock().unwrap();
        let e = map.entry(addr.to_string()).or_insert_with(|| LivePeer {
            name: name.to_string(),
            addr: addr.to_string(),
            writing: false,
            last_seen: std::time::Instant::now(),
            writing_at: None,
        });
        if !name.is_empty() {
            e.name = name.to_string();
        }
        e.last_seen = std::time::Instant::now();
        if writing {
            e.writing = true;
            e.writing_at = Some(std::time::Instant::now());
            self.snap.lock().unwrap().last_writer =
                format!("{} <{}>", e.name, e.addr);
        }
    }

    fn drop_live(&self, addr: &str) {
        self.live.lock().unwrap().remove(addr);
    }

    fn snapshot(&self) -> HostSnapshot {
        // GC stale live peers
        {
            let mut map = self.live.lock().unwrap();
            map.retain(|_, p| p.last_seen.elapsed() < Duration::from_secs(30));
            for p in map.values_mut() {
                if p.writing
                    && p.writing_at
                        .map(|t| t.elapsed() > Duration::from_secs(4))
                        .unwrap_or(true)
                {
                    p.writing = false;
                }
            }
        }

        let mut s = self.snap.lock().unwrap().clone();
        s.peers_seen = self.peers_seen.load(Ordering::Relaxed);
        s.bytes_in = self.bytes_in.load(Ordering::Relaxed);
        s.bytes_out = self.bytes_out.load(Ordering::Relaxed);
        s.pings = self.pings.load(Ordering::Relaxed);
        s.use_tmux = self.use_tmux();
        s.tmux_available = which_cmd("tmux");
        s.allowed_commands = self.allowed_commands();
        s.shell_policy = self.shell_policy();
        s.pending_cmds = self.pending_cmds.lock().unwrap().clone();
        s.party_mode = self.party_mode();
        s.invite_url = self.invite_url();
        s.invite_busy = self.invite_busy();
        // share_targets kept as last loaded (refreshed by worker)
        s.logs = self.logs.lock().unwrap().iter().cloned().collect();
        let allow = self.write_allow_set();
        let party = s.party_mode;
        let host_l = s.identity.to_lowercase();
        let can = |addr: &str| -> bool {
            if party {
                return true;
            }
            let k = addr.to_lowercase();
            (!host_l.is_empty() && k == host_l) || allow.contains(&k)
        };

        let live = self.live.lock().unwrap();
        s.live_peers = live
            .values()
            .map(|p| AccessPeer {
                label: p.name.clone(),
                addr: p.addr.clone(),
                online: true,
                writing: p.writing,
                can_write: can(&p.addr),
            })
            .collect();
        s.live_peers
            .sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));

        // Merge live status into chat member list so the access panel is accurate.
        for m in &mut s.chat_members {
            let addr_l = m.addr.to_lowercase();
            m.can_write = can(&m.addr);
            if let Some(lp) = live.values().find(|p| p.addr.eq_ignore_ascii_case(&addr_l)) {
                m.online = true;
                m.writing = lp.writing;
                if !lp.name.is_empty() && lp.name != lp.addr {
                    m.label = lp.name.clone();
                }
            } else if m.addr.eq_ignore_ascii_case(&s.identity) {
                m.online = true; // host is always "online"
            } else {
                m.online = false;
                m.writing = false;
            }
        }
        s
    }
}

// ── PTY / realtime ──────────────────────────────────────────────────────────

struct PtySession {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    /// Kept so we can kill the shell/tmux client on restart or quit.
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
}

struct RealtimeOut {
    tx: mpsc::Sender<Vec<u8>>,
}

type Sessions = Arc<Mutex<HashMap<u32, Arc<PtySession>>>>;
type Outs = Arc<Mutex<HashMap<u32, RealtimeOut>>>;

fn bin_name() -> String {
    std::env::args()
        .next()
        .map(|p| {
            Path::new(&p)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("terminal_buddy")
                .to_string()
        })
        .unwrap_or_else(|| "terminal_buddy".into())
}

fn wants_help(args: &[String]) -> bool {
    args.iter().any(|a| matches!(a.as_str(), "-h" | "--help" | "help"))
}

fn wants_version(args: &[String]) -> bool {
    args.iter().any(|a| {
        matches!(
            a.as_str(),
            "-V" | "--version" | "--verison" | "version" // --verison: common typo
        )
    })
}

fn print_version() {
    let emb = if EMBEDDED_XDC.is_empty() {
        "no (rebuild with `make build`)".to_string()
    } else {
        format!("yes ({} bytes)", EMBEDDED_XDC.len())
    };
    println!("TerminalBuddy {APP_VERSION}");
    println!("  crate        {}", env!("CARGO_PKG_VERSION"));
    println!("  embedded xdc {emb}");
}

/// Resolve the mini app path: CLI path → DC_XDC → embedded bytes → DEFAULT_XDC.
fn resolve_xdc_path(cli: Option<&str>) -> Result<PathBuf> {
    if let Some(p) = cli.map(str::trim).filter(|s| !s.is_empty()) {
        let path = PathBuf::from(p);
        return path
            .canonicalize()
            .with_context(|| format!("webxdc not found: {}", path.display()));
    }

    if let Ok(p) = std::env::var("DC_XDC") {
        let p = p.trim();
        if !p.is_empty() {
            let path = PathBuf::from(p);
            if path.is_file() {
                return path
                    .canonicalize()
                    .with_context(|| format!("DC_XDC not readable: {}", path.display()));
            }
        }
    }

    if !EMBEDDED_XDC.is_empty() {
        let dir = std::env::temp_dir().join("terminalbuddy-embed");
        std::fs::create_dir_all(&dir).context("create temp dir for embedded xdc")?;
        let path = dir.join(format!("terminalbuddy-{APP_VERSION}.xdc"));
        // Rewrite when size changes (new embed after rebuild).
        let need_write = match std::fs::metadata(&path) {
            Ok(m) => m.len() as usize != EMBEDDED_XDC.len(),
            Err(_) => true,
        };
        if need_write {
            std::fs::write(&path, EMBEDDED_XDC).context("write embedded webxdc to temp")?;
        }
        return Ok(path);
    }

    // Dev fallback: loose file next to the crate / webxdc tree.
    for cand in [
        PathBuf::from(DEFAULT_XDC),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/terminalbuddy.xdc"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../webxdc/terminalbuddy.xdc"),
    ] {
        if cand.is_file() {
            return cand.canonicalize().with_context(|| {
                format!("webxdc present but unreadable: {}", cand.display())
            });
        }
    }

    bail!(
        "no webxdc available — pass a path, set DC_XDC, or run `make build` \
         to embed terminalbuddy.xdc into the binary"
    )
}

fn print_help() {
    let bin = bin_name();
    let emb = if EMBEDDED_XDC.is_empty() {
        "not embedded in this build"
    } else {
        "embedded in this binary"
    };
    println!(
        "\
TerminalBuddy {APP_VERSION} — shared shell over Delta Chat webxdc realtime

USAGE:
  {bin} <command> [options]
  {bin} --help
  {bin} --version

COMMANDS:
  serve [app.xdc] [options]   Host TUI + PTY; peers open the mini app
  join [invite-url]           Create identity (if needed) and SecureJoin invite
  send-file <path> [chat]     Send a one-off file into a chat
  help                        Show this help
  version                     Print version (+ embedded xdc info)

SERVE OPTIONS:
  --invite URL                Join this SecureJoin invite and send the mini app
  --force-join                Re-join even if a group already exists (default with --invite)
  --no-force-join             Reuse an existing group instead of re-joining
  --allowed-commands a,b,c    Whitelist mode (restricted shell — not YOLO)
  --allowed a,b,c             Alias for --allowed-commands
  --tmux                      YOLO + tmux (ignored when whitelist is set)
  --no-tmux                   Force plain shell
  --no-tui                    Headless host (no Ratatui); Ctrl-C to stop
  -h, --help                  Show help for serve

  [app.xdc] is optional when the mini app is {emb}.
  `make build` produces one binary with the .xdc inside.

  Prefer --allowed-commands over YOLO when hosting for others.

ENVIRONMENT:
  DC_AGENT_SERVER             Account server (default: {DEFAULT_SERVER})
  DC_AGENT_SHELL              Override PTY program path
  DC_USE_TMUX=1               Same as --tmux
  DC_TMUX_SESSION             tmux session name (default: terminalbuddy)
  DC_ALLOWED_COMMANDS         Same as --allowed-commands
  DC_BOOTSTRAP_INVITE=1       Auto join/send from DC_INVITE (make run-agent-inv)
  DC_INVITE                   Group/contact invite URL (quote it in .env!)
  DC_FORCE_JOIN=1             With bootstrap: SecureJoin even if a group exists
  DC_XDC                      Override path to .xdc (else use embedded)
  DC_NO_TUI=1                 Same as --no-tui (headless host)

MAKEFILE (repo root):
  make build                  Build webxdc + release binary with embed → dist/
  make run-agent              Dev host TUI (manual Share tab; no invite)
  make run-agent-inv          Dev host + join/send from DC_INVITE in .env

TUI (while hosting):
  1 Host · 2 Shell · 3 Access · 4 Share
  Tab / 1-4 switch tabs · q quit · ↑↓ scroll log
  Shell: y YOLO · w whitelist · a approve · pending y/n/w
  Share: paste invite or pick chat/contact to send the mini app

Default shell is plain $SHELL — tmux is OFF unless --tmux / DC_USE_TMUX=1
or you enable it on TUI tab 2 (Shell)."
    );
}

fn print_serve_help() {
    let bin = bin_name();
    let emb = if EMBEDDED_XDC.is_empty() {
        "none in this build — pass a path or `make build`"
    } else {
        "embedded package (default)"
    };
    println!(
        "\
TerminalBuddy serve — host the shared terminal

USAGE:
  {bin} serve [app.xdc] [options]

ARGUMENTS:
  [app.xdc]                   Optional mini app path (default: {emb})

OPTIONS:
  --invite URL                Join invite + send embedded mini app into that chat
  --force-join                Force SecureJoin (default when --invite is set)
  --no-force-join             Do not re-join if a group already exists
  --allowed-commands a,b,c    Whitelist only these commands (not YOLO)
  --allowed a,b,c             Alias for --allowed-commands
  --tmux                      YOLO + tmux (disabled under --allowed-commands)
  --no-tmux                   Force plain $SHELL / DC_AGENT_SHELL
  --no-tui                    Headless (logs to stdout; Ctrl-C to stop)
  -h, --help                  Show this help
  -V, --version               Show version

EXAMPLE (whitelist + invite — recommended over YOLO):
  {bin} serve \\
    --invite 'https://i.delta.chat/#…&v=3&…' \\
    --allowed-commands ls,cat,pwd,whoami,uname

  {bin} serve --no-tui --invite '…' --allowed-commands ls,pwd

Without --invite / DC_BOOTSTRAP_INVITE the host opens the TUI and you send
the mini app from tab 4 Share. With --no-tui you should pass --invite (no Share tab)."
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_help();
        return Ok(());
    }
    match args.first().map(|s| s.as_str()).unwrap_or("help") {
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "version" | "-V" | "--version" | "--verison" => {
            print_version();
            Ok(())
        }
        "restricted-shell" => {
            args.remove(0);
            if wants_help(&args) {
                println!(
                    "restricted-shell is an internal PTY helper (spawned by serve).\n\
                     Do not call it directly."
                );
                return Ok(());
            }
            restricted_shell::run(&args)
        }
        "join" => {
            args.remove(0);
            if wants_help(&args) {
                let bin = bin_name();
                println!(
                    "\
USAGE:
  {bin} join [invite-url]

Join a SecureJoin invite (https://i.delta.chat/#… or OPENPGP4FPR:…).
Creates a throwaway account on DC_AGENT_SERVER if none is configured yet.
Sends a short Hi after joining."
                );
                return Ok(());
            }
            cmd_join(args.first().cloned()).await
        }
        "serve" => {
            args.remove(0);
            if wants_help(&args) {
                print_serve_help();
                return Ok(());
            }
            if wants_version(&args) {
                print_version();
                return Ok(());
            }
            let opts = parse_serve_args(&args);
            let xdc = resolve_xdc_path(opts.xdc.as_deref())?;
            cmd_serve(
                xdc,
                opts.use_tmux,
                opts.allowed_commands,
                opts.invite,
                opts.force_join,
                opts.no_tui,
            )
            .await
        }
        "send-file" => {
            args.remove(0);
            if wants_help(&args) {
                let bin = bin_name();
                println!(
                    "\
USAGE:
  {bin} send-file <path> [chat-id]

Send a file once into a chat (debug helper)."
                );
                return Ok(());
            }
            let path = args
                .first()
                .context("send-file <path> [chat-id] — try --help")?
                .clone();
            cmd_send_file(PathBuf::from(path), args.get(1).cloned()).await
        }
        other if other.starts_with("http") || other.starts_with("OPENPGP4FPR") => {
            cmd_join(Some(other.into())).await
        }
        other => bail!("unknown {other:?} — try `{bin} --help`", bin = bin_name()),
    }
}

async fn open_ctx() -> Result<(Context, EventEmitter, PathBuf)> {
    let dir = std::env::current_dir()?.join(DATA_DIR);
    tokio::fs::create_dir_all(&dir).await?;
    let events = Events::new();
    let emitter = events.get_emitter();
    let ctx = ContextBuilder::new(dir.join(DB_NAME))
        .with_id(1)
        .with_events(events)
        .with_stock_strings(StockStrings::new())
        .open()
        .await?;
    Ok((ctx, emitter, dir))
}

/// Set display name + profile picture so peers see “TerminalBuddy” with the brand icon.
async fn apply_profile_branding(ctx: &Context) -> Result<()> {
    ctx.set_config(Config::Displayname, Some(PROFILE_NAME))
        .await
        .context("set display name")?;

    let avatar = Path::new(PROFILE_AVATAR);
    if avatar.is_file() {
        let path = avatar
            .canonicalize()
            .unwrap_or_else(|_| avatar.to_path_buf());
        let path_s = path.to_string_lossy();
        // Always re-apply so logo updates ship with new agent builds.
        if let Err(e) = ctx
            .set_config(Config::Selfavatar, Some(path_s.as_ref()))
            .await
        {
            say(&format!("  !  could not set profile avatar: {e:#}"));
        } else {
            say(&format!("  Profile  {PROFILE_NAME}  + avatar"));
        }
    } else {
        say(&format!(
            "  !  profile avatar missing at {} — name only",
            avatar.display()
        ));
        say(&format!("  Profile  {PROFILE_NAME}"));
    }
    Ok(())
}

async fn ensure_account(ctx: &Context, quiet: bool) -> Result<()> {
    if ctx.is_configured().await? {
        let addr = ctx
            .get_config(Config::ConfiguredAddr)
            .await?
            .unwrap_or_default();
        say_ok(&format!("Signed in as {addr}"));
        apply_profile_branding(ctx).await?;
        ctx.set_config_bool(Config::WebxdcRealtimeEnabled, true)
            .await?;
        ctx.start_io().await;
        ctx.maybe_network().await;
        return Ok(());
    }
    let server = std::env::var("DC_AGENT_SERVER")
        .ok()
        .map(|s| normalize_server(&s))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if quiet {
                DEFAULT_SERVER.to_string()
            } else {
                let a = prompt(&format!("server [default {DEFAULT_SERVER}]")).unwrap_or_default();
                let s = normalize_server(&a);
                if s.is_empty() {
                    DEFAULT_SERVER.to_string()
                } else {
                    s
                }
            }
        });
    say(&format!("Creating a new account on {server}…"));
    ctx.add_transport_from_qr(&format!("DCACCOUNT:{server}"))
        .await?;
    apply_profile_branding(ctx).await?;
    ctx.set_config_bool(Config::WebxdcRealtimeEnabled, true)
        .await?;
    let addr = ctx
        .get_config(Config::ConfiguredAddr)
        .await?
        .unwrap_or_default();
    say_ok(&format!("Account ready — {addr} ({PROFILE_NAME})"));
    Ok(())
}

async fn cmd_join(invite: Option<String>) -> Result<()> {
    say_banner("Join chat");
    let (ctx, emitter, dir) = open_ctx().await?;
    say(&format!("Profile folder: {}", dir.display()));
    ensure_account(&ctx, invite.is_some()).await?;
    let invite = match invite {
        Some(i) => i,
        None => prompt("Paste invite link")?,
    };
    let invite = invite.trim().to_string();
    if invite.is_empty() {
        bail!("Invite link is empty");
    }
    say("Joining via SecureJoin…");
    let chat_id = securejoin::join_securejoin(&ctx, &invite).await?;
    wait_securejoin(&emitter, Duration::from_secs(120)).await;
    chat::send_text_msg(&ctx, chat_id, "Hi".into()).await?;
    wait_smtp_any(&emitter, Duration::from_secs(60)).await;
    say_ok(&format!("Joined chat. Run `make run-agent` to host the terminal."));
    ctx.stop_io().await;
    Ok(())
}

async fn cmd_send_file(path: PathBuf, chat: Option<String>) -> Result<()> {
    say_banner("Send file");
    let (ctx, emitter, _) = open_ctx().await?;
    ensure_account(&ctx, true).await?;
    let chat_id = match chat {
        Some(s) => ChatId::new(s.parse()?),
        None => first_chat(&ctx).await?.context("No chat yet — join an invite first")?,
    };
    let id = send_xdc(&ctx, chat_id, &path).await?;
    wait_smtp_large(&emitter, Duration::from_secs(120)).await;
    say_ok(&format!("Sent ({id})"));
    ctx.stop_io().await;
    Ok(())
}

struct ServeOpts {
    /// Explicit path from CLI; `None` → embedded / DC_XDC / default search.
    xdc: Option<String>,
    use_tmux: bool,
    allowed_commands: Vec<String>,
    /// SecureJoin invite (CLI); enables bootstrap join+send.
    invite: Option<String>,
    /// When bootstrapping: force SecureJoin (default true if invite set).
    force_join: bool,
    /// Headless host — no Ratatui dashboard.
    no_tui: bool,
}

/// Parse `serve` CLI: optional path, invite, whitelist, tmux, no-tui.
/// Default: plain shell. Env `DC_USE_TMUX=1` / `DC_ALLOWED_COMMANDS` also apply.
fn parse_serve_args(args: &[String]) -> ServeOpts {
    let mut xdc: Option<String> = None;
    let mut use_tmux = std::env::var("DC_USE_TMUX")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    let mut allowed = std::env::var("DC_ALLOWED_COMMANDS")
        .map(|s| restricted_shell::split_allowed_list(&s))
        .unwrap_or_default();
    let mut invite: Option<String> = None;
    // None = not set on CLI → fall back to env when bootstrapping.
    let mut force_join_cli: Option<bool> = None;
    let mut no_tui = std::env::var("DC_NO_TUI")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--tmux" => {
                use_tmux = true;
                i += 1;
            }
            "--no-tmux" => {
                use_tmux = false;
                i += 1;
            }
            "--no-tui" | "--headless" => {
                no_tui = true;
                i += 1;
            }
            "--force-join" => {
                force_join_cli = Some(true);
                i += 1;
            }
            "--no-force-join" => {
                force_join_cli = Some(false);
                i += 1;
            }
            "--invite" => {
                if let Some(v) = args.get(i + 1) {
                    invite = Some(normalize_invite(v));
                    i += 2;
                } else {
                    i += 1;
                }
            }
            s if s.starts_with("--invite=") => {
                invite = Some(normalize_invite(s.trim_start_matches("--invite=")));
                i += 1;
            }
            "--allowed-commands" | "--allowed" => {
                if let Some(v) = args.get(i + 1) {
                    allowed.extend(restricted_shell::split_allowed_list(v));
                    i += 2;
                } else {
                    i += 1;
                }
            }
            s if s.starts_with("--allowed-commands=") => {
                allowed.extend(restricted_shell::split_allowed_list(
                    s.trim_start_matches("--allowed-commands="),
                ));
                i += 1;
            }
            s if s.starts_with("--allowed=") => {
                allowed.extend(restricted_shell::split_allowed_list(
                    s.trim_start_matches("--allowed="),
                ));
                i += 1;
            }
            s if !s.starts_with('-') => {
                xdc = Some(s.to_string());
                i += 1;
            }
            _ => i += 1,
        }
    }

    allowed.sort();
    allowed.dedup();

    if !allowed.is_empty() && use_tmux {
        eprintln!("  !  --allowed-commands implies restricted shell; ignoring --tmux");
        use_tmux = false;
    }
    if use_tmux && !which_cmd("tmux") {
        eprintln!("  !  --tmux requested but tmux not found; falling back to $SHELL");
        use_tmux = false;
    }

    // Prefer CLI invite; else keep env for make run-agent-inv.
    if invite.is_none() {
        if let Ok(v) = std::env::var("DC_INVITE") {
            let n = normalize_invite(&v);
            if !n.is_empty()
                && (n.contains("delta.chat") || n.to_ascii_lowercase().contains("openpgp4fpr"))
            {
                // Only use env invite when bootstrap is requested via env.
                if bootstrap_invite_from_env() {
                    invite = Some(n);
                }
            }
        }
    }

    let force_join = force_join_cli.unwrap_or_else(|| {
        std::env::var("DC_FORCE_JOIN")
            .map(|v| !matches!(v.as_str(), "0" | "false" | "no"))
            .unwrap_or(true)
    });

    ServeOpts {
        xdc,
        use_tmux,
        allowed_commands: allowed,
        invite,
        force_join,
        no_tui,
    }
}


fn normalize_invite(s: &str) -> String {
    s.trim()
        .trim_matches(|c| c == '\'' || c == '"' || c == '`' || c == ' ')
        .to_string()
}

async fn apply_share_job(
    ctx: &Context,
    sessions: &Sessions,
    outs: &Outs,
    state: &Arc<SharedState>,
    xdc: &Path,
    host_chat: &Arc<Mutex<Option<ChatId>>>,
    job: PendingShare,
) {
    match job {
        PendingShare::Invite(invite) => {
            apply_invite_and_send(ctx, sessions, outs, state, xdc, host_chat, invite).await;
        }
        PendingShare::Chat(id) => {
            let chat_id = ChatId::new(id);
            send_xdc_to_chat(ctx, sessions, outs, state, xdc, host_chat, chat_id).await;
        }
        PendingShare::Contact(id) => {
            let contact_id = ContactId::new(id);
            let chat_id = match ChatId::create_for_contact(ctx, contact_id).await {
                Ok(c) => c,
                Err(e) => {
                    state.finish_invite_job(false, format!("open chat failed: {e:#}"));
                    return;
                }
            };
            send_xdc_to_chat(ctx, sessions, outs, state, xdc, host_chat, chat_id).await;
        }
    }
    if let Ok(targets) = load_share_targets(ctx).await {
        state.patch(|s| s.share_targets = targets);
    }
}

/// Join invite from TUI, send webxdc, start hosting that instance.
async fn apply_invite_and_send(
    ctx: &Context,
    sessions: &Sessions,
    outs: &Outs,
    state: &Arc<SharedState>,
    xdc: &Path,
    host_chat: &Arc<Mutex<Option<ChatId>>>,
    invite: String,
) {
    let invite = normalize_invite(&invite);
    state.log(format!(
        "share: joining {}…",
        invite.chars().take(48).collect::<String>()
    ));
    ctx.maybe_network().await;

    let chat_id = match securejoin::join_securejoin(ctx, &invite).await {
        Ok(id) => id,
        Err(e) => {
            state.finish_invite_job(false, format!("join failed: {e:#}"));
            return;
        }
    };
    // Give SecureJoin a moment (event emitter is busy with the main loop).
    tokio::time::sleep(Duration::from_secs(3)).await;
    ctx.maybe_network().await;
    send_xdc_to_chat(ctx, sessions, outs, state, xdc, host_chat, chat_id).await;
}

async fn send_xdc_to_chat(
    ctx: &Context,
    sessions: &Sessions,
    outs: &Outs,
    state: &Arc<SharedState>,
    xdc: &Path,
    host_chat: &Arc<Mutex<Option<ChatId>>>,
    chat_id: ChatId,
) {
    let chat_title = match chat::Chat::load_from_db(ctx, chat_id).await {
        Ok(ch) => {
            let n = ch.get_name().to_string();
            if n.is_empty() {
                format!("{chat_id}")
            } else {
                n
            }
        }
        Err(_) => format!("{chat_id}"),
    };
    state.log(format!("share: target “{chat_title}” ({chat_id})"));
    *host_chat.lock().unwrap() = Some(chat_id);

    if let Ok(members) = load_chat_access_list(ctx, chat_id).await {
        state.patch(|s| {
            s.chat_id = chat_title.clone();
            s.chat_members = members;
        });
    } else {
        state.patch(|s| s.chat_id = chat_title.clone());
    }

    state.log("share: sending mini app…");
    let msg_id = match send_xdc(ctx, chat_id, xdc).await {
        Ok(id) => id,
        Err(e) => {
            state.finish_invite_job(false, format!("send webxdc failed: {e:#}"));
            return;
        }
    };
    if let Err(e) = wait_msg_sent(ctx, msg_id, Duration::from_secs(180)).await {
        state.log(format!("share: delivery wait: {e:#} (still hosting)"));
    }

    if let Err(e) = ensure_host(ctx, sessions.clone(), outs.clone(), state.clone(), msg_id).await {
        state.finish_invite_job(false, format!("host PTY failed: {e:#}"));
        return;
    }
    state.patch(|s| {
        s.primary_msg = format!("{msg_id}");
        s.hosting = sessions.lock().unwrap().len();
    });
    state.finish_invite_job(true, format!("sent to “{chat_title}” (msg {msg_id})"));
}

async fn load_share_targets(ctx: &Context) -> Result<Vec<ShareTarget>> {
    let mut out = Vec::new();
    let mut contact_in_chatlist = std::collections::HashSet::new();

    let list = Chatlist::try_load(ctx, 0, None, None).await?;
    for i in 0..list.len() {
        let id = list.get_chat_id(i)?;
        if id.is_special() {
            continue;
        }
        let ch = chat::Chat::load_from_db(ctx, id).await?;
        if ch.is_self_talk() || ch.is_device_talk() {
            continue;
        }
        let name = ch.get_name().to_string();
        let (detail, _is_group) = match ch.get_type() {
            deltachat::constants::Chattype::Group => ("group".to_string(), true),
            deltachat::constants::Chattype::Single => {
                // Remember 1:1 peer so we don't duplicate as a bare contact.
                if let Ok(contacts) = chat::get_chat_contacts(ctx, id).await {
                    for cid in contacts {
                        if cid != ContactId::SELF && cid.to_u32() >= 10 {
                            contact_in_chatlist.insert(cid.to_u32());
                        }
                    }
                }
                ("direct".to_string(), false)
            }
            other => (format!("{other:?}").to_lowercase(), false),
        };
        let label = if name.is_empty() {
            format!("{id}")
        } else {
            name
        };
        out.push(ShareTarget {
            label,
            detail,
            kind: ShareKind::Chat,
            chat_id: Some(id.to_u32()),
            contact_id: None,
        });
    }

    // Contacts without an existing 1:1 chat — send will create one.
    if let Ok(ids) = Contact::get_all(ctx, 0, None).await {
        for cid in ids {
            if cid == ContactId::SELF || cid.to_u32() < 10 {
                continue;
            }
            if contact_in_chatlist.contains(&cid.to_u32()) {
                continue;
            }
            let c = Contact::get_by_id(ctx, cid).await?;
            let addr = c.get_addr().to_string();
            if addr.is_empty() {
                continue;
            }
            let name = c.get_display_name().to_string();
            let label = if name.is_empty() || name == addr {
                addr.clone()
            } else {
                name
            };
            out.push(ShareTarget {
                label,
                detail: format!("contact · {addr}"),
                kind: ShareKind::Contact,
                chat_id: None,
                contact_id: Some(cid.to_u32()),
            });
        }
    }

    // Groups first-ish already from chatlist order; contacts at end.
    Ok(out)
}

fn shell_label_for(use_tmux: bool, policy: ShellPolicy, allowed: &[String]) -> String {
    match policy {
        ShellPolicy::Yolo => {
            if use_tmux {
                let session =
                    std::env::var("DC_TMUX_SESSION").unwrap_or_else(|_| "terminalbuddy".into());
                format!("YOLO tmux -A -s {session}")
            } else if let Ok(shell) = std::env::var("DC_AGENT_SHELL") {
                if !shell.trim().is_empty() {
                    format!("YOLO {shell}")
                } else {
                    format!(
                        "YOLO {}",
                        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
                    )
                }
            } else {
                format!(
                    "YOLO {}",
                    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
                )
            }
        }
        ShellPolicy::Whitelist => {
            let list = if allowed.len() <= 5 {
                allowed.join(", ")
            } else {
                format!("{}, …+{}", allowed[..4].join(", "), allowed.len() - 4)
            };
            format!("whitelist [{list}]")
        }
        ShellPolicy::Approve => "approve (host y/n each command)".into(),
    }
}

async fn cmd_serve(
    xdc: PathBuf,
    use_tmux: bool,
    allowed: Vec<String>,
    invite: Option<String>,
    force_join: bool,
    no_tui: bool,
) -> Result<()> {
    let policy = if allowed.is_empty() {
        ShellPolicy::Yolo
    } else {
        ShellPolicy::Whitelist
    };
    let shell_label = shell_label_for(use_tmux, policy, &allowed);

    // Pre-TUI bootstrap — keep this short and friendly.
    let xdc_size = xdc.metadata()?.len();
    let from_embed = !EMBEDDED_XDC.is_empty() && xdc_size as usize == EMBEDDED_XDC.len();
    say_banner(&format!("TerminalBuddy host  v{APP_VERSION}"));
    say_step(
        1,
        5,
        &format!(
            "App package  {}{}",
            xdc.file_name().unwrap_or_default().to_string_lossy(),
            if from_embed { "  (embedded)" } else { "" }
        ),
    );
    say(&format!("              {}", human_bytes(xdc_size)));
    let mode_note = match policy {
        ShellPolicy::Whitelist => format!("  (whitelist: {})", allowed.join(", ")),
        ShellPolicy::Approve => "  (approve mode)".into(),
        ShellPolicy::Yolo if use_tmux => "  (--tmux YOLO)".into(),
        ShellPolicy::Yolo => "  (YOLO plain shell — change on Shell tab)".into(),
    };
    say_step(1, 5, &format!("Terminal     {shell_label}{mode_note}"));
    if policy == ShellPolicy::Yolo {
        say("  !  YOLO mode: peers can run any command — prefer --allowed-commands");
    }

    let (ctx, emitter, _dir) = open_ctx().await?;
    ensure_account(&ctx, true).await?;

    let identity = ctx
        .get_config(Config::ConfiguredAddr)
        .await?
        .unwrap_or_default();

    say_step(2, 5, "Connecting to mail…");
    wait_imap(&emitter, Duration::from_secs(45)).await;

    // Bootstrap when --invite is set, or DC_BOOTSTRAP_INVITE=1 with DC_INVITE.
    let bootstrap = invite.is_some() || bootstrap_invite_from_env();
    let mut boot_chat: Option<ChatId> = None;
    let mut boot_msg: Option<MsgId> = None;
    let mut chat_title = "(none — use Share tab)".to_string();
    let mut members: Vec<AccessPeer> = Vec::new();
    let invite_for_ui = invite.clone().unwrap_or_default();

    if bootstrap {
        say_step(3, 5, "Bootstrap from invite…");
        let chat_id = resolve_host_chat(&ctx, &emitter, invite.as_deref(), force_join)
            .await
            .context("bootstrap invite failed")?;
        let chat = chat::Chat::load_from_db(&ctx, chat_id).await?;
        chat_title = {
            let n = chat.get_name();
            if n.is_empty() {
                format!("{chat_id}")
            } else {
                n.to_string()
            }
        };
        let chat_kind = match chat.get_type() {
            deltachat::constants::Chattype::Group => "group",
            deltachat::constants::Chattype::Single => "direct chat",
            other => &format!("{other:?}"),
        };
        say_ok(&format!("Chat {chat_title} ({chat_kind}, id={chat_id})"));

        members = load_chat_access_list(&ctx, chat_id).await?;
        say("Who can type in the terminal:");
        for m in &members {
            let who = if m.label == m.addr {
                m.addr.clone()
            } else {
                format!("{}  ({})", m.label, m.addr)
            };
            say(&format!("   ·  {who}"));
        }

        say_step(4, 5, "Sending mini app into this chat…");
        ctx.maybe_network().await;
        let msg_id = send_xdc(&ctx, chat_id, &xdc)
            .await
            .context("send webxdc into chat")?;
        let msg = Message::load_from_db(&ctx, msg_id).await?;
        if msg.get_viewtype() != Viewtype::Webxdc {
            bail!(
                "Sent message is not a webxdc (got {:?}). Check that the .xdc is a valid package.",
                msg.get_viewtype()
            );
        }
        say(&format!("Queued as message {msg_id} — waiting for SMTP…"));
        wait_msg_sent(&ctx, msg_id, Duration::from_secs(180))
            .await
            .context("webxdc was not delivered (SMTP failed or timed out)")?;
        say_ok(&format!(
            "Mini app delivered to “{chat_title}” (msg {msg_id})"
        ));
        boot_chat = Some(chat_id);
        boot_msg = Some(msg_id);
    } else {
        say_step(3, 5, "No auto-invite (manual Share tab)");
        say("  Tip: make run-agent-inv  → join/send from DC_INVITE in .env");
        say("  Or:  open TUI tab 4 Share, paste invite or pick a chat");
        say_step(4, 5, "Skipping automatic webxdc send");
    }

    if no_tui {
        say_step(5, 5, "Starting headless host (--no-tui)…");
        if !bootstrap {
            say("  !  --no-tui without --invite: nothing was auto-sent (no Share tab)");
            say("     pass --invite '…' so peers get the mini app");
        }
        if policy == ShellPolicy::Approve {
            say("  !  approve mode has no TUI keys here — prefer --allowed-commands");
        }
    } else {
        say_step(5, 5, "Opening host dashboard…");
    }
    println!();

    // Invite field prefilled when bootstrapping (CLI --invite or env).
    let invite_init = if !invite_for_ui.is_empty() {
        invite_for_ui
    } else if bootstrap {
        std::env::var("DC_INVITE")
            .ok()
            .map(|s| normalize_invite(&s))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let state = SharedState::new(use_tmux, allowed.clone(), invite_init.clone());
    state.patch(|s| {
        s.identity = identity.clone();
        s.chat_id = chat_title.clone();
        s.primary_msg = boot_msg
            .map(|m| format!("{m}"))
            .unwrap_or_else(|| "—".into());
        s.shell = shell_label.clone();
        s.xdc_path = xdc.display().to_string();
        s.status = if boot_msg.is_some() {
            "hosting".into()
        } else {
            "ready — Share tab to send".into()
        };
        s.chat_members = members;
        s.use_tmux = use_tmux && policy == ShellPolicy::Yolo;
        s.allowed_commands = allowed.clone();
        s.shell_policy = policy;
        s.invite_url = invite_init;
        s.invite_hint = "tab 4 Share: paste invite or pick chat/contact".into();
    });
    state.log(format!("Signed in as {identity}"));
    state.log(format!("Chat: {chat_title}"));
    if let Some(m) = boot_msg {
        state.log(format!("Webxdc {m}"));
    } else {
        state.log("Webxdc not sent yet — use Share tab (4)");
    }
    state.log(format!("PTY: {shell_label}"));
    state.log("Shell tab: y=YOLO  w=whitelist  a=approve · pending y/n");
    for m in &state.snap.lock().unwrap().chat_members {
        state.log(format!("Access: {} <{}>", m.label, m.addr));
    }

    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
    let outs: Outs = Arc::new(Mutex::new(HashMap::new()));
    // Track active host chat for member refresh (updated when Share is used).
    let host_chat: Arc<Mutex<Option<ChatId>>> = Arc::new(Mutex::new(boot_chat));

    if let Some(msg_id) = boot_msg {
        ensure_host(
            &ctx,
            sessions.clone(),
            outs.clone(),
            state.clone(),
            msg_id,
        )
        .await?;
        if let Some(chat_id) = boot_chat {
            for old in own_webxdc_msgs(&ctx, chat_id).await? {
                if old != msg_id {
                    state.log(format!("also host older webxdc {old}"));
                    let _ =
                        ensure_host(&ctx, sessions.clone(), outs.clone(), state.clone(), old).await;
                }
            }
        }
        state.patch(|s| s.hosting = sessions.lock().unwrap().len());
        if no_tui {
            state.log(String::from("headless ready — open newest webxdc on phone"));
        } else {
            state.log(String::from("TUI ready — open newest webxdc on phone"));
        }
        state.set_status("waiting for peer on realtime P2P");
    } else if no_tui {
        state.log(String::from("headless ready — no webxdc sent (use --invite)"));
        state.set_status("ready — waiting (no share UI)");
    } else {
        state.log(String::from("TUI ready — Share tab to send the mini app"));
        state.set_status("ready — Share tab to send");
    }

    // Keepalive + PTY restart + invite apply (Host tab field)
    let ctx_n = ctx.clone();
    let state_n = state.clone();
    let sessions_n = sessions.clone();
    let outs_n = outs.clone();
    let xdc_n = xdc.clone();
    let host_chat_n = host_chat.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if state_n.is_shutdown() {
                break;
            }
            if state_n.take_restart_pty() {
                restart_all_ptys(&ctx_n, &sessions_n, &outs_n, &state_n).await;
            }
            // Approve-mode gate: surface pending cmds + notify webxdc peers.
            let newly = state_n.poll_gate_requests();
            for p in &newly {
                state_n.log(format!("approve needed: {} ({})", p.line, p.id));
                let payload = serde_json::json!({
                    "t": "cmd_wait",
                    "i": p.id,
                    "cmd": p.cmd,
                    "line": p.line,
                });
                broadcast_json(&outs_n, &payload).await;
            }
            for (id, allow) in state_n.take_cmd_done_queue() {
                let payload = serde_json::json!({
                    "t": "cmd_done",
                    "i": id,
                    "ok": allow,
                });
                broadcast_json(&outs_n, &payload).await;
            }
            if let Some(job) = state_n.take_pending_share() {
                apply_share_job(
                    &ctx_n,
                    &sessions_n,
                    &outs_n,
                    &state_n,
                    &xdc_n,
                    &host_chat_n,
                    job,
                )
                .await;
            }
        }
    });
    // Initial share target list
    if let Ok(targets) = load_share_targets(&ctx).await {
        state.patch(|s| s.share_targets = targets);
    }
    let ctx_m = ctx.clone();
    let state_m = state.clone();
    let host_chat_m = host_chat.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            if state_m.is_shutdown() {
                break;
            }
            ctx_m.maybe_network().await;
            let chat_n = *host_chat_m.lock().unwrap();
            if let Some(chat_n) = chat_n {
                if let Ok(members) = load_chat_access_list(&ctx_m, chat_n).await {
                    state_m.patch(|s| s.chat_members = members);
                }
            }
            if let Ok(targets) = load_share_targets(&ctx_m).await {
                state_m.patch(|s| s.share_targets = targets);
            }
        }
    });

    // Event loop → updates SharedState
    let state_ev = state.clone();
    let sessions_ev = sessions.clone();
    let outs_ev = outs.clone();
    let ctx_ev = ctx.clone();
    let host_chat_ev = host_chat.clone();
    tokio::spawn(async move {
        loop {
            if state_ev.is_shutdown() {
                break;
            }
            let ev = tokio::select! {
                ev = emitter.recv() => ev,
                _ = tokio::time::sleep(Duration::from_millis(200)) => {
                    if state_ev.is_shutdown() {
                        break;
                    }
                    continue;
                }
            };
            let Some(ev) = ev else {
                state_ev.log("event stream ended");
                break;
            };
            match ev.typ {
                EventType::WebxdcRealtimeData { msg_id: mid, data } => {
                    let kind = data.first().copied().unwrap_or(0);
                    state_ev
                        .bytes_in
                        .fetch_add(data.len() as u64, Ordering::Relaxed);
                    // Skip spam: terminal output + high-rate cursor JSON.
                    if kind != KIND_OUTPUT && kind != b'{' {
                        state_ev.log(format!(
                            "← realtime {mid} len={} kind={}",
                            data.len(),
                            kind as char
                        ));
                    }
                    on_realtime(
                        &ctx_ev,
                        &sessions_ev,
                        &outs_ev,
                        &state_ev,
                        mid,
                        &data,
                    )
                    .await;
                }
                EventType::WebxdcRealtimeAdvertisementReceived { msg_id: mid } => {
                    state_ev.peers_seen.fetch_add(1, Ordering::Relaxed);
                    state_ev.log(format!("← peer advertisement {mid}"));
                    state_ev.set_status("peer on P2P");
                    if let Err(e) =
                        ensure_host(&ctx_ev, sessions_ev.clone(), outs_ev.clone(), state_ev.clone(), mid)
                            .await
                    {
                        state_ev.log(format!("ensure_host err: {e:#}"));
                    } else {
                        let _ = send_webxdc_realtime_advertisement(&ctx_ev, mid).await;
                        // Status is for host TUI / client header only — never inject into PTY stream.
                        write_pty(&sessions_ev, mid, b"\n");
                    }
                    state_ev.patch(|s| s.hosting = sessions_ev.lock().unwrap().len());
                }
                EventType::IncomingMsg {
                    chat_id: c,
                    msg_id: mid,
                } => {
                    let active = *host_chat_ev.lock().unwrap();
                    if active == Some(c) {
                        state_ev.log(format!("← chat msg {mid}"));
                    }
                }
                EventType::ImapConnected(m) => state_ev.log(format!("imap: {m}")),
                EventType::Error(m) => state_ev.log(format!("error: {m}")),
                EventType::Warning(m) if m.contains("IROH") || m.contains("realtime") => {
                    state_ev.log(format!("warn: {m}"));
                }
                EventType::Info(m) if m.contains("IROH_REALTIME") => {
                    // keep log quieter: only join/advertise
                    if m.contains("Joining") || m.contains("Sent realtime") || m.contains("Received")
                    {
                        // Received is very noisy when typing — skip bulk
                        if !m.contains("Received realtime data") {
                            state_ev.log(m);
                        }
                    }
                }
                EventType::SmtpMessageSent(m) => {
                    if let Some(n) = m
                        .split("len=")
                        .nth(1)
                        .and_then(|s| s.split_whitespace().next())
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        if n > 10_000 {
                            state_ev.log(format!("smtp: {m}"));
                        }
                    }
                }
                _ => {}
            }
        }
    });

    if no_tui {
        run_headless(state.clone()).await?;
    } else {
        // Ratatui on the main async task. `q` sets shutdown and returns.
        tui::run(state.clone()).await?;
    }

    state.request_shutdown();
    state.log("Stopping…");
    // Kill hosted shell/tmux client so we don't leave strays (except detached
    // tmux *server* session — only killed if we were in tmux mode).
    stop_all_ptys(&sessions, &outs, state.use_tmux());
    // Bound stop so we never hang on quit.
    let _ = tokio::time::timeout(Duration::from_secs(3), ctx.stop_io()).await;
    println!();
    say_ok("Host stopped. Bye.");
    println!();
    // Background tasks may still hold the runtime; force a clean process exit.
    std::process::exit(0);
}

/// Headless host: print logs to stdout until Ctrl-C (no Ratatui).
async fn run_headless(state: Arc<SharedState>) -> Result<()> {
    say("Headless mode (--no-tui) — logs below · Ctrl-C to stop");
    say(&format!("  status: {}", state.snapshot().status));
    println!();

    // Seed with whatever was already logged during bootstrap.
    let mut printed = {
        let snap = state.snapshot();
        for line in &snap.logs {
            println!("  ·  {line}");
        }
        snap.logs.len()
    };

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                say("Ctrl-C — shutting down…");
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if state.is_shutdown() {
                    break;
                }
                let snap = state.snapshot();
                if snap.logs.len() > printed {
                    for line in snap.logs.iter().skip(printed) {
                        println!("  ·  {line}");
                    }
                    printed = snap.logs.len();
                } else if snap.logs.len() < printed {
                    // Log buffer rotated (cap) — resync.
                    printed = snap.logs.len();
                }
            }
        }
    }
    Ok(())
}

async fn ensure_host(
    ctx: &Context,
    sessions: Sessions,
    outs: Outs,
    state: Arc<SharedState>,
    msg_id: MsgId,
) -> Result<()> {
    let id = msg_id.to_u32();
    if sessions.lock().unwrap().contains_key(&id) {
        let _ = send_webxdc_realtime_advertisement(ctx, msg_id).await?;
        return Ok(());
    }

    let msg = Message::load_from_db(ctx, msg_id).await?;
    if msg.get_viewtype() != Viewtype::Webxdc {
        bail!("{msg_id} is not webxdc");
    }

    let allowed = state.allowed_commands();
    let policy = state.shell_policy();
    state.log(format!(
        "start PTY + realtime for {msg_id} ({})",
        shell_label_for(state.use_tmux(), policy, &allowed)
    ));
    let _ = send_webxdc_realtime_advertisement(ctx, msg_id).await?;

    let (pty, mut reader) = spawn_pty(state.use_tmux(), policy, &allowed)?;
    sessions.lock().unwrap().insert(id, Arc::new(pty));
    state.register_hosted(id);

    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(64);
    outs.lock().unwrap().insert(id, RealtimeOut { tx: tx.clone() });

    let ctx_send = ctx.clone();
    let state_out = state.clone();
    tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            let n = frame.len() as u64;
            if let Err(e) = send_webxdc_realtime_data(&ctx_send, msg_id, frame).await {
                state_out.log(format!("realtime send failed: {e:#}"));
            } else {
                state_out.bytes_out.fetch_add(n, Ordering::Relaxed);
            }
        }
    });

    let outs_r = outs.clone();
    let sessions_r = sessions.clone();
    let state_r = state.clone();
    tokio::task::spawn_blocking(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let mut frame = Vec::with_capacity(1 + n);
                    frame.push(KIND_OUTPUT);
                    frame.extend_from_slice(&buf[..n]);
                    if tx.blocking_send(frame).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    state_r.log(format!("pty read: {e}"));
                    break;
                }
            }
        }
        let _ = tx.blocking_send(vec![KIND_EXIT]);
        sessions_r.lock().unwrap().remove(&id);
        outs_r.lock().unwrap().remove(&id);
        state_r.log(format!("PTY ended for {msg_id}"));
        state_r.patch(|s| s.hosting = sessions_r.lock().unwrap().len());
    });

    state.patch(|s| s.hosting = sessions.lock().unwrap().len());
    Ok(())
}

async fn on_realtime(
    ctx: &Context,
    sessions: &Sessions,
    outs: &Outs,
    state: &Arc<SharedState>,
    msg_id: MsgId,
    data: &[u8],
) {
    if data.is_empty() {
        return;
    }
    if !sessions.lock().unwrap().contains_key(&msg_id.to_u32()) {
        if let Err(e) =
            ensure_host(ctx, sessions.clone(), outs.clone(), state.clone(), msg_id).await
        {
            state.log(format!("lazy host: {e:#}"));
            return;
        }
    }

    // Presence JSON — peers propagate this among themselves. Host only
    // observes for the TUI (does not rebroadcast; cursors are natural P2P).
    if data[0] == b'{' {
        handle_presence_json(state, data);
        return;
    }

    match data[0] {
        // Peer wants keystrokes applied to the shared PTY.
        // Frame: I | 0x01 | u16le addr_len | addr utf8 | payload
        // (legacy bare payload: denied unless party mode)
        KIND_INPUT => {
            if data.len() < 2 {
                return;
            }
            let (addr, payload) = parse_input_frame(&data[1..]);
            if payload.is_empty() {
                return;
            }
            if !state.is_write_allowed(&addr) {
                // Soft-deny: log occasionally, never write.
                static DENY_LOG: AtomicU64 = AtomicU64::new(0);
                let n = DENY_LOG.fetch_add(1, Ordering::Relaxed);
                if n % 40 == 0 {
                    let who = if addr.is_empty() {
                        "(unknown peer)".into()
                    } else {
                        addr.clone()
                    };
                    state.log(format!("access denied for {who} (not on allow-list)"));
                }
                return;
            }
            write_pty(sessions, msg_id, payload);
        }
        // Peer is alive / joining the mesh — stay on the gossip topic.
        KIND_PING => {
            state.pings.fetch_add(1, Ordering::Relaxed);
            state.set_status("peer online (ping)");
            // Keep mesh warm; do not write pong into the terminal (header-only events).
            let _ = send_webxdc_realtime_advertisement(ctx, msg_id).await;
        }
        // Peer wants the PTY geometry to match their screen.
        KIND_RESIZE if data.len() >= 5 => {
            let cols = u16::from_le_bytes([data[1], data[2]]);
            let rows = u16::from_le_bytes([data[3], data[4]]);
            resize_pty(sessions, msg_id, cols, rows);
            state.log(format!("pty resize {cols}x{rows}"));
        }
        // Only the host may publish terminal output / exit.
        KIND_OUTPUT | KIND_EXIT => {}
        k => state.log(format!("unknown kind 0x{k:02x}")),
    }
}

fn handle_presence_json(state: &SharedState, data: &[u8]) {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(data) else {
        return;
    };
    let t = v.get("t").and_then(|x| x.as_str()).unwrap_or("");
    let addr = v.get("a").and_then(|x| x.as_str()).unwrap_or("");
    let name = v.get("n").and_then(|x| x.as_str()).unwrap_or(addr);
    if addr.is_empty() {
        return;
    }
    match t {
        "join" | "pos" => {
            state.touch_live(addr, name, false);
            if t == "join" {
                state.log(format!("live join: {name} <{addr}>"));
                state.set_status(format!("online: {name}"));
            }
        }
        "writer" | "in" => {
            state.touch_live(addr, name, true);
            state.log(format!("writer: {name} <{addr}>"));
            state.set_status(format!("writing: {name}"));
        }
        "rx" => {
            state.touch_live(addr, name, false);
            let emoji = v.get("e").and_then(|x| x.as_str()).unwrap_or("?");
            state.log(format!("reaction: {name} {emoji}"));
        }
        "leave" => {
            state.drop_live(addr);
            state.log(format!("live leave: {name} <{addr}>"));
        }
        _ => {}
    }
}

async fn load_chat_access_list(ctx: &Context, chat_id: ChatId) -> Result<Vec<AccessPeer>> {
    let host_addr = ctx
        .get_config(Config::ConfiguredAddr)
        .await?
        .unwrap_or_else(|| "self".into());

    let ids = chat::get_chat_contacts(ctx, chat_id).await?;
    let mut out = Vec::new();
    let mut saw_self = false;

    for id in ids {
        if id == ContactId::SELF {
            saw_self = true;
            out.push(AccessPeer {
                label: "host (you)".into(),
                addr: host_addr.clone(),
                online: true,
                writing: false,
                can_write: true,
            });
            continue;
        }
        // Skip special contacts (device/info).
        if id.to_u32() < 10 {
            continue;
        }
        let c = Contact::get_by_id(ctx, id).await?;
        let addr = c.get_addr().to_string();
        if addr.is_empty() {
            continue;
        }
        let name = c.get_display_name().to_string();
        out.push(AccessPeer {
            label: if name.is_empty() || name == addr {
                addr.clone()
            } else {
                name
            },
            addr,
            online: false,
            writing: false,
            can_write: true, // refreshed in snapshot from party/allow-list
        });
    }

    // 1:1 chats often omit SELF from get_chat_contacts — always list the host.
    if !saw_self {
        out.insert(
            0,
            AccessPeer {
                label: "host (you)".into(),
                addr: host_addr,
                online: true,
                writing: false,
                can_write: true,
            },
        );
    }

    // Stable order: host first, then by label.
    out.sort_by(|a, b| {
        let ah = a.label.starts_with("host");
        let bh = b.label.starts_with("host");
        match (ah, bh) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.label.to_lowercase().cmp(&b.label.to_lowercase()),
        }
    });
    Ok(out)
}

fn write_pty(sessions: &Sessions, msg_id: MsgId, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let Some(s) = sessions.lock().unwrap().get(&msg_id.to_u32()).cloned() else {
        return;
    };
    let mut w = s.writer.lock().unwrap();
    let _ = w.write_all(bytes);
    let _ = w.flush();
}

/// Parse input after the `I` kind byte.
///
/// Versioned frame (current clients):
///   `0x01 | u16le addr_len | addr UTF-8 | key payload`
///
/// Never treat raw keystrokes as an address: a previous u8-length scheme
/// collided when addr_len==64 (byte `@`) and hashed selfAddr had no `@`,
/// so the host wrote the address into the PTY on every key.
fn parse_input_frame(rest: &[u8]) -> (String, &[u8]) {
    // Version 1: 0x01 | u16 LE length | addr | payload
    if rest.first() == Some(&0x01) && rest.len() >= 3 {
        let n = u16::from_le_bytes([rest[1], rest[2]]) as usize;
        if n <= 512 && rest.len() >= 3 + n {
            let addr = String::from_utf8_lossy(&rest[3..3 + n]).into_owned();
            return (addr, &rest[3 + n..]);
        }
        // Truncated v1 frame — drop entirely (do not dump into PTY).
        return (String::new(), &[]);
    }
    // Legacy: bare key payload, empty addr (party mode only).
    (String::new(), rest)
}

fn resize_pty(sessions: &Sessions, msg_id: MsgId, cols: u16, rows: u16) {
    if cols == 0 || rows == 0 {
        return;
    }
    let Some(s) = sessions.lock().unwrap().get(&msg_id.to_u32()).cloned() else {
        return;
    };
    let _ = s.master.lock().unwrap().resize(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    });
}

fn spawn_pty(
    use_tmux: bool,
    policy: ShellPolicy,
    allowed: &[String],
) -> Result<(PtySession, Box<dyn Read + Send>)> {
    let sys = NativePtySystem::default();
    let pair = sys.openpty(PtySize {
        rows: 30,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let (program, args) = resolve_pty_command(use_tmux, policy, allowed)?;
    let mut cmd = CommandBuilder::new(&program);
    for a in &args {
        cmd.arg(a);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("REMOTE_XTERM", "1");
    // Gate env for restricted-shell child (absolute path).
    if matches!(policy, ShellPolicy::Whitelist | ShellPolicy::Approve) {
        let gate = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(GATE_DIR);
        let _ = std::fs::create_dir_all(gate.join("req"));
        let _ = std::fs::create_dir_all(gate.join("res"));
        cmd.env("REMOTE_XTERM_GATE_DIR", gate.display().to_string());
        cmd.env("REMOTE_XTERM_MODE", policy.short());
        if !allowed.is_empty() {
            cmd.env("REMOTE_XTERM_ALLOW", allowed.join(","));
        }
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .with_context(|| format!("spawn {program} {args:?}"))?;
    let writer = pair.master.take_writer()?;
    let reader = pair.master.try_clone_reader()?;
    Ok((
        PtySession {
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            child: Mutex::new(child),
        },
        reader,
    ))
}

/// YOLO → plain shell or tmux. Whitelist/Approve → this binary as gated shell.
fn resolve_pty_command(
    use_tmux: bool,
    policy: ShellPolicy,
    allowed: &[String],
) -> Result<(String, Vec<String>)> {
    match policy {
        ShellPolicy::Whitelist | ShellPolicy::Approve => {
            let exe = std::env::current_exe().context("current_exe for restricted-shell")?;
            let gate = std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(GATE_DIR);
            let mut args = vec![
                "restricted-shell".into(),
                "--mode".into(),
                policy.short().into(),
                "--gate-dir".into(),
                gate.display().to_string(),
            ];
            if policy == ShellPolicy::Whitelist {
                args.push("--allowed".into());
                args.push(allowed.join(","));
            }
            return Ok((exe.display().to_string(), args));
        }
        ShellPolicy::Yolo => {}
    }

    if use_tmux && which_cmd("tmux") {
        let conf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmux.conf");
        let session =
            std::env::var("DC_TMUX_SESSION").unwrap_or_else(|_| "terminalbuddy".into());
        let mut args = Vec::new();
        if conf.is_file() {
            args.push("-f".into());
            args.push(conf.display().to_string());
        }
        args.push("new-session".into());
        args.push("-A".into());
        args.push("-s".into());
        args.push(session);
        args.push("-x".into());
        args.push("80".into());
        args.push("-y".into());
        args.push("30".into());
        return Ok(("tmux".into(), args));
    }

    if let Ok(shell) = std::env::var("DC_AGENT_SHELL") {
        if !shell.trim().is_empty() {
            return Ok((shell, vec![]));
        }
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    Ok((shell, vec![]))
}

async fn broadcast_json(outs: &Outs, v: &serde_json::Value) {
    let Ok(bytes) = serde_json::to_vec(v) else {
        return;
    };
    let ids: Vec<u32> = outs.lock().unwrap().keys().copied().collect();
    for id in ids {
        let tx = outs.lock().unwrap().get(&id).map(|o| o.tx.clone());
        if let Some(tx) = tx {
            let _ = tx.send(bytes.clone()).await;
        }
    }
}

fn stop_all_ptys(sessions: &Sessions, outs: &Outs, was_tmux: bool) {
    let mut map = sessions.lock().unwrap();
    for (_id, sess) in map.drain() {
        if let Ok(mut child) = sess.child.lock() {
            let _ = child.kill();
        }
    }
    outs.lock().unwrap().clear();
    // If we had been using tmux, kill the named session so nothing is left on the system.
    if was_tmux && which_cmd("tmux") {
        let session =
            std::env::var("DC_TMUX_SESSION").unwrap_or_else(|_| "terminalbuddy".into());
        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", &session])
            .status();
    }
}

async fn restart_all_ptys(
    ctx: &Context,
    sessions: &Sessions,
    outs: &Outs,
    state: &Arc<SharedState>,
) {
    let ids = state.hosted_ids();
    // Mode already flipped in SharedState. Kill running PTY children always;
    // if we switched *to* plain shell, also drop the named tmux session.
    let kill_tmux_session = !state.use_tmux() && which_cmd("tmux");
    state.log(format!("restarting {} PTY session(s)…", ids.len()));
    {
        let mut map = sessions.lock().unwrap();
        for (_id, sess) in map.drain() {
            if let Ok(mut child) = sess.child.lock() {
                let _ = child.kill();
            }
        }
    }
    outs.lock().unwrap().clear();
    if kill_tmux_session {
        let session =
            std::env::var("DC_TMUX_SESSION").unwrap_or_else(|_| "terminalbuddy".into());
        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", &session])
            .status();
        state.log(format!("killed tmux session {session}"));
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    for id in ids {
        let mid = MsgId::new(id);
        if let Err(e) = ensure_host(ctx, sessions.clone(), outs.clone(), state.clone(), mid).await
        {
            state.log(format!("restart host {mid} failed: {e:#}"));
        }
    }
    state.patch(|s| s.hosting = sessions.lock().unwrap().len());
    let allowed = state.allowed_commands();
    state.set_status(format!(
        "PTY: {}",
        shell_label_for(state.use_tmux(), state.shell_policy(), &allowed)
    ));
}



fn which_cmd(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|p| {
                let candidate = p.join(name);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

async fn send_xdc(ctx: &Context, chat_id: ChatId, path: &Path) -> Result<MsgId> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("read {}", path.display()))?;
    if bytes.is_empty() {
        bail!("{} is empty", path.display());
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("terminalbuddy.xdc");
    if !name.ends_with(".xdc") {
        bail!("webxdc file must end with .xdc (got {name})");
    }
    say(&format!(
        "Attaching {name} ({}) → chat {chat_id}",
        human_bytes(bytes.len() as u64)
    ));
    // Prefer explicit Webxdc viewtype (core also accepts File + .xdc suffix).
    let mut msg = Message::new(Viewtype::Webxdc);
    msg.set_file_from_bytes(ctx, name, &bytes, Some("application/webxdc+zip"))
        .with_context(|| format!("attach {name}"))?;
    msg.set_text(format!("TerminalBuddy — {name}"));
    let id = chat::send_msg(ctx, chat_id, &mut msg)
        .await
        .with_context(|| format!("chat::send_msg to {chat_id}"))?;
    // Nudge SMTP immediately.
    ctx.maybe_network().await;
    Ok(id)
}

/// Wait until a specific outgoing message is delivered (or fail).
/// Polls DB state so we never miss SmtpMessageSent events.
async fn wait_msg_sent(ctx: &Context, msg_id: MsgId, timeout: Duration) -> Result<()> {
    let end = tokio::time::Instant::now() + timeout;
    let mut last_state = MessageState::Undefined;
    let mut ticks = 0u32;
    loop {
        let msg = Message::load_from_db(ctx, msg_id).await?;
        let state = msg.get_state();
        if state != last_state {
            last_state = state;
            say(&format!("  … msg {msg_id} state={state}"));
        }
        match state {
            MessageState::OutDelivered | MessageState::OutMdnRcvd => return Ok(()),
            MessageState::OutFailed => {
                let err = msg.error().unwrap_or_else(|| "unknown SMTP error".into());
                bail!("message {msg_id} failed: {err}");
            }
            _ => {}
        }
        if tokio::time::Instant::now() >= end {
            bail!(
                "timeout after {timeout:?} waiting for msg {msg_id} (last state={last_state})"
            );
        }
        ticks += 1;
        // Keep SMTP worker busy; every few seconds re-nudge the network.
        if ticks % 10 == 0 {
            ctx.maybe_network().await;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// True when `make run-agent-inv` / `DC_BOOTSTRAP_INVITE=1` should join+send from `.env`.
fn bootstrap_invite_from_env() -> bool {
    std::env::var("DC_BOOTSTRAP_INVITE")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// Join invite (CLI `--invite` or `DC_INVITE`) and optionally force re-join.
async fn resolve_host_chat(
    ctx: &Context,
    emitter: &EventEmitter,
    invite_cli: Option<&str>,
    force_join: bool,
) -> Result<ChatId> {
    let invite = invite_cli
        .map(normalize_invite)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("DC_INVITE")
                .ok()
                .map(|s| normalize_invite(&s))
                .filter(|s| !s.is_empty())
        })
        .filter(|s| s.contains("delta.chat") || s.to_ascii_lowercase().contains("openpgp4fpr"));

    let invite = invite.context(
        "bootstrap requires --invite URL or DC_INVITE (quote the URL — # is a shell comment)",
    )?;

    let need_join = force_join || !has_group_chat(ctx).await?;
    if need_join {
        say(&format!(
            "Invite: {}…",
            invite.chars().take(48).collect::<String>()
        ));
        let chat_id = securejoin::join_securejoin(ctx, &invite)
            .await
            .context("SecureJoin failed — check the invite URL is complete and quoted")?;
        wait_securejoin(emitter, Duration::from_secs(180)).await;
        if let Ok(ch) = chat::Chat::load_from_db(ctx, chat_id).await {
            if ch.is_contact_request() {
                say("Accepting chat…");
                chat_id.accept(ctx).await.ok();
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        ctx.maybe_network().await;
        say_ok(&format!("Joined target chat ({chat_id})"));
        return Ok(chat_id);
    }

    say("Invite present; already in a group — reusing first group (pass --force-join to rejoin)");
    first_chat(ctx)
        .await?
        .context("No group chat yet — pass --force-join to join from invite")
}

async fn has_group_chat(ctx: &Context) -> Result<bool> {
    let list = Chatlist::try_load(ctx, 0, None, None).await?;
    for i in 0..list.len() {
        let id = list.get_chat_id(i)?;
        if id.is_special() {
            continue;
        }
        let ch = chat::Chat::load_from_db(ctx, id).await?;
        if ch.is_self_talk() || ch.is_device_talk() {
            continue;
        }
        if ch.get_type() == deltachat::constants::Chattype::Group {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn first_chat(ctx: &Context) -> Result<Option<ChatId>> {
    let list = Chatlist::try_load(ctx, 0, None, None).await?;
    let mut fallback: Option<ChatId> = None;
    // Prefer encrypted groups (group invites) over 1:1 chats.
    for i in 0..list.len() {
        let id = list.get_chat_id(i)?;
        if id.is_special() {
            continue;
        }
        let ch = chat::Chat::load_from_db(ctx, id).await?;
        if ch.is_self_talk() || ch.is_device_talk() {
            continue;
        }
        if ch.get_type() == deltachat::constants::Chattype::Group {
            return Ok(Some(id));
        }
        if fallback.is_none() {
            fallback = Some(id);
        }
    }
    Ok(fallback)
}

async fn own_webxdc_msgs(ctx: &Context, chat_id: ChatId) -> Result<Vec<MsgId>> {
    let items = chat::get_chat_msgs(ctx, chat_id).await?;
    let mut out = Vec::new();
    for item in items {
        let ChatItem::Message { msg_id } = item else {
            continue;
        };
        let msg = Message::load_from_db(ctx, msg_id).await?;
        if msg.get_viewtype() == Viewtype::Webxdc && msg.get_from_id() == ContactId::SELF {
            out.push(msg_id);
        }
    }
    Ok(out)
}

fn normalize_server(s: &str) -> String {
    let s = s.trim();
    let s = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let s = s
        .strip_prefix("DCACCOUNT:")
        .or_else(|| s.strip_prefix("dcaccount:"))
        .unwrap_or(s)
        .trim_start_matches('/');
    s.split('/')
        .next()
        .unwrap_or(s)
        .trim()
        .trim_end_matches('.')
        .into()
}

fn prompt(msg: &str) -> Result<String> {
    println!("{msg}");
    print!("> ");
    io::stdout().flush()?;
    let mut l = String::new();
    io::stdin().read_line(&mut l)?;
    Ok(l.trim().into())
}

async fn wait_securejoin(emitter: &EventEmitter, timeout: Duration) {
    let end = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < end {
        let left = end.saturating_duration_since(tokio::time::Instant::now());
        let Some(ev) = tokio::time::timeout(left, emitter.recv())
            .await
            .ok()
            .flatten()
        else {
            break;
        };
        if let EventType::SecurejoinJoinerProgress { progress, .. } = ev.typ {
            if progress >= 1000 {
                say_ok("Secure join complete");
                return;
            }
            if progress == 400 {
                say("Verifying contact…");
            }
        }
    }
}

async fn wait_imap(emitter: &EventEmitter, timeout: Duration) {
    let end = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < end {
        let left = end.saturating_duration_since(tokio::time::Instant::now());
        let Some(ev) = tokio::time::timeout(left, emitter.recv())
            .await
            .ok()
            .flatten()
        else {
            break;
        };
        match ev.typ {
            EventType::ImapInboxIdle => {
                say_ok("Mail connection ready");
                return;
            }
            EventType::ImapConnected(_) => {
                // quiet — final “ready” is enough
            }
            EventType::Error(m) => eprintln!("  !  {m}"),
            _ => {}
        }
    }
    say("Mail is still warming up — continuing anyway");
}

async fn wait_smtp_any(emitter: &EventEmitter, timeout: Duration) {
    let end = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < end {
        let left = end.saturating_duration_since(tokio::time::Instant::now());
        let Some(ev) = tokio::time::timeout(left, emitter.recv())
            .await
            .ok()
            .flatten()
        else {
            break;
        };
        if let EventType::SmtpMessageSent(m) = ev.typ {
            if let Some(n) = smtp_size_hint(&m) {
                say_ok(&format!("Message sent ({})", human_bytes(n)));
            } else {
                say_ok("Message sent");
            }
            return;
        }
    }
}

async fn wait_smtp_large(emitter: &EventEmitter, timeout: Duration) {
    let end = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < end {
        let left = end.saturating_duration_since(tokio::time::Instant::now());
        let Some(ev) = tokio::time::timeout(left, emitter.recv())
            .await
            .ok()
            .flatten()
        else {
            break;
        };
        if let EventType::SmtpMessageSent(m) = ev.typ {
            if let Some(n) = smtp_size_hint(&m) {
                if n > 10_000 {
                    say_ok(&format!("Mini app sent ({})", human_bytes(n)));
                    return;
                }
                // small system/ad messages — keep waiting for the webxdc
                continue;
            }
            say_ok("Message sent");
            return;
        }
    }
}
