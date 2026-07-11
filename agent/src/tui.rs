//! Ratatui dashboard for TerminalBuddy (`terminal_buddy serve`).
//!
//! Tabs:
//!   1 Host   — status, members, log
//!   2 Shell  — plain $SHELL vs tmux / restricted commands
//!   3 Access — party mode or per-person write allow-list
//!   4 Share  — invite link or pick chat/contact to send webxdc

use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Tabs, Wrap};

use crate::{HostSnapshot, ShareKind, ShellPolicy, SharedState};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Host = 0,
    Shell = 1,
    Access = 2,
    Share = 3,
}

impl Tab {
    fn next(self) -> Self {
        match self {
            Tab::Host => Tab::Shell,
            Tab::Shell => Tab::Access,
            Tab::Access => Tab::Share,
            Tab::Share => Tab::Host,
        }
    }

    fn prev(self) -> Self {
        match self {
            Tab::Host => Tab::Share,
            Tab::Shell => Tab::Host,
            Tab::Access => Tab::Shell,
            Tab::Share => Tab::Access,
        }
    }
}

struct App {
    state: Arc<SharedState>,
    log_scroll: usize,
    should_quit: bool,
    tab: Tab,
    access_sel: usize,
    share_sel: usize,
    invite_edit: bool,
    invite_buf: String,
    /// Adding a whitelist program name on Shell tab.
    whitelist_edit: bool,
    whitelist_buf: String,
    whitelist_sel: usize,
}

pub async fn run(state: Arc<SharedState>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App {
        invite_buf: state.invite_url(),
        state,
        log_scroll: 0,
        should_quit: false,
        tab: Tab::Host,
        access_sel: 0,
        share_sel: 0,
        invite_edit: false,
        whitelist_edit: false,
        whitelist_buf: String::new(),
        whitelist_sel: 0,
    };

    let res = run_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        let snap = app.state.snapshot();
        if !snap.chat_members.is_empty() {
            app.access_sel = app.access_sel.min(snap.chat_members.len() - 1);
        } else {
            app.access_sel = 0;
        }
        if !snap.share_targets.is_empty() {
            app.share_sel = app.share_sel.min(snap.share_targets.len() - 1);
        } else {
            app.share_sel = 0;
        }

        terminal.draw(|f| ui(f, &snap, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Text fields take priority
                    if app.invite_edit && app.tab == Tab::Share {
                        match key.code {
                            KeyCode::Esc => {
                                app.invite_edit = false;
                                app.invite_buf = app.state.invite_url();
                            }
                            KeyCode::Enter => {
                                app.invite_edit = false;
                                app.state.set_invite_url(app.invite_buf.clone());
                                app.state.submit_invite(app.invite_buf.clone());
                            }
                            KeyCode::Backspace => {
                                app.invite_buf.pop();
                            }
                            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.invite_buf.clear();
                            }
                            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.invite_buf.push(c);
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if app.whitelist_edit && app.tab == Tab::Shell {
                        match key.code {
                            KeyCode::Esc => {
                                app.whitelist_edit = false;
                                app.whitelist_buf.clear();
                            }
                            KeyCode::Enter => {
                                let name = app.whitelist_buf.trim().to_string();
                                app.whitelist_edit = false;
                                app.whitelist_buf.clear();
                                if !name.is_empty() {
                                    app.state.add_whitelist_cmd(&name);
                                }
                            }
                            KeyCode::Backspace => {
                                app.whitelist_buf.pop();
                            }
                            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                if !c.is_whitespace() {
                                    app.whitelist_buf.push(c);
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }

                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            app.state.request_shutdown();
                            app.should_quit = true;
                        }
                        KeyCode::Esc
                            if app.tab != Tab::Access && app.tab != Tab::Share =>
                        {
                            app.state.request_shutdown();
                            app.should_quit = true;
                        }
                        KeyCode::Tab => app.tab = app.tab.next(),
                        KeyCode::BackTab => app.tab = app.tab.prev(),
                        KeyCode::Char('1') => app.tab = Tab::Host,
                        KeyCode::Char('2') => app.tab = Tab::Shell,
                        KeyCode::Char('3') => app.tab = Tab::Access,
                        KeyCode::Char('4') => app.tab = Tab::Share,

                        // Host log
                        KeyCode::Up | KeyCode::Char('k') if app.tab == Tab::Host => {
                            app.log_scroll = app.log_scroll.saturating_add(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') if app.tab == Tab::Host => {
                            app.log_scroll = app.log_scroll.saturating_sub(1);
                        }
                        KeyCode::PageUp if app.tab == Tab::Host => {
                            app.log_scroll = app.log_scroll.saturating_add(10);
                        }
                        KeyCode::PageDown if app.tab == Tab::Host => {
                            app.log_scroll = app.log_scroll.saturating_sub(10);
                        }
                        KeyCode::Home if app.tab == Tab::Host => app.log_scroll = 0,

                        // Shell tab — pending approve takes priority over mode keys
                        // Y = run once · w = run + add to whitelist · n = deny
                        KeyCode::Char('Y')
                            if app.tab == Tab::Shell && !snap.pending_cmds.is_empty() =>
                        {
                            if let Some(p) = snap.pending_cmds.first() {
                                app.state.resolve_pending_cmd(&p.id, true, false);
                            }
                        }
                        KeyCode::Char('y')
                            if app.tab == Tab::Shell && !snap.pending_cmds.is_empty() =>
                        {
                            // once only
                            if let Some(p) = snap.pending_cmds.first() {
                                app.state.resolve_pending_cmd(&p.id, true, false);
                            }
                        }
                        KeyCode::Char('w')
                            if app.tab == Tab::Shell && !snap.pending_cmds.is_empty() =>
                        {
                            // approve + whitelist so next time auto-runs
                            if let Some(p) = snap.pending_cmds.first() {
                                app.state.resolve_pending_cmd(&p.id, true, true);
                            }
                        }
                        KeyCode::Char('n') | KeyCode::Char('N')
                            if app.tab == Tab::Shell && !snap.pending_cmds.is_empty() =>
                        {
                            if let Some(p) = snap.pending_cmds.first() {
                                app.state.resolve_pending_cmd(&p.id, false, false);
                            }
                        }
                        // Mode switch (only reached when no pending matched above)
                        KeyCode::Char('y') | KeyCode::Char('Y') if app.tab == Tab::Shell => {
                            app.state.set_shell_policy(ShellPolicy::Yolo);
                        }
                        KeyCode::Char('w') | KeyCode::Char('W') if app.tab == Tab::Shell => {
                            app.state.set_shell_policy(ShellPolicy::Whitelist);
                        }
                        KeyCode::Char('a') | KeyCode::Char('A') if app.tab == Tab::Shell => {
                            app.state.set_shell_policy(ShellPolicy::Approve);
                        }
                        KeyCode::Char('t') | KeyCode::Char('T') if app.tab == Tab::Shell => {
                            app.state.set_use_tmux(!app.state.use_tmux());
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') if app.tab == Tab::Shell => {
                            app.whitelist_edit = true;
                            app.whitelist_buf.clear();
                        }
                        KeyCode::Char('-') | KeyCode::Char('d') | KeyCode::Char('D')
                            if app.tab == Tab::Shell =>
                        {
                            if let Some(name) = snap.allowed_commands.get(app.whitelist_sel) {
                                app.state.remove_whitelist_cmd(name);
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') if app.tab == Tab::Shell => {
                            app.whitelist_sel = app.whitelist_sel.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') if app.tab == Tab::Shell => {
                            let max = snap.allowed_commands.len().saturating_sub(1);
                            if app.whitelist_sel < max {
                                app.whitelist_sel += 1;
                            }
                        }

                        // Access
                        KeyCode::Char('p') | KeyCode::Char('P') if app.tab == Tab::Access => {
                            app.state.set_party_mode(!app.state.party_mode());
                        }
                        KeyCode::Up | KeyCode::Char('k') if app.tab == Tab::Access => {
                            app.access_sel = app.access_sel.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') if app.tab == Tab::Access => {
                            let max = snap.chat_members.len().saturating_sub(1);
                            if app.access_sel < max {
                                app.access_sel += 1;
                            }
                        }
                        KeyCode::Char(' ') | KeyCode::Enter if app.tab == Tab::Access => {
                            if let Some(p) = snap.chat_members.get(app.access_sel) {
                                if app.state.party_mode() {
                                    app.state.set_party_mode(false);
                                }
                                app.state.toggle_write_allow(&p.addr);
                            }
                        }
                        KeyCode::Char('a') | KeyCode::Char('A') if app.tab == Tab::Access => {
                            app.state.set_party_mode(false);
                            for p in &snap.chat_members {
                                if !p.label.starts_with("host") && !app.state.is_write_allowed(&p.addr)
                                {
                                    app.state.toggle_write_allow(&p.addr);
                                }
                            }
                            app.state.log("access: granted write to all chat members");
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') if app.tab == Tab::Access => {
                            app.state.set_party_mode(false);
                            let allow: Vec<String> = snap
                                .chat_members
                                .iter()
                                .filter(|p| p.can_write && !p.label.starts_with("host"))
                                .map(|p| p.addr.clone())
                                .collect();
                            for addr in allow {
                                if app.state.is_write_allowed(&addr) {
                                    app.state.toggle_write_allow(&addr);
                                }
                            }
                            app.state.log("access: only host may write");
                        }

                        // Share tab
                        KeyCode::Char('i') | KeyCode::Char('I') | KeyCode::Char('e')
                            if app.tab == Tab::Share && !snap.invite_busy =>
                        {
                            app.invite_edit = true;
                            app.invite_buf = app.state.invite_url();
                        }
                        KeyCode::Up | KeyCode::Char('k') if app.tab == Tab::Share => {
                            app.share_sel = app.share_sel.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') if app.tab == Tab::Share => {
                            let max = snap.share_targets.len().saturating_sub(1);
                            if app.share_sel < max {
                                app.share_sel += 1;
                            }
                        }
                        KeyCode::Enter if app.tab == Tab::Share && !snap.invite_busy => {
                            // Prefer invite if field has a delta.chat URL, else selected target
                            let inv = if !app.invite_buf.trim().is_empty() {
                                app.invite_buf.clone()
                            } else {
                                app.state.invite_url()
                            };
                            let inv = inv.trim();
                            if inv.contains("delta.chat")
                                || inv.to_ascii_lowercase().contains("openpgp4fpr")
                            {
                                app.state.set_invite_url(inv.to_string());
                                app.state.submit_invite(inv.to_string());
                            } else if let Some(t) = snap.share_targets.get(app.share_sel) {
                                match t.kind {
                                    ShareKind::Chat => {
                                        if let Some(id) = t.chat_id {
                                            app.state.submit_send_chat(id, &t.label);
                                        }
                                    }
                                    ShareKind::Contact => {
                                        if let Some(id) = t.contact_id {
                                            app.state.submit_send_contact(id, &t.label);
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Char('s') | KeyCode::Char('S')
                            if app.tab == Tab::Share && !snap.invite_busy =>
                        {
                            // Force send to selected chat/contact (ignore invite field)
                            if let Some(t) = snap.share_targets.get(app.share_sel) {
                                match t.kind {
                                    ShareKind::Chat => {
                                        if let Some(id) = t.chat_id {
                                            app.state.submit_send_chat(id, &t.label);
                                        }
                                    }
                                    ShareKind::Contact => {
                                        if let Some(id) = t.contact_id {
                                            app.state.submit_send_contact(id, &t.label);
                                        }
                                    }
                                }
                            }
                        }

                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
        tokio::task::yield_now().await;
    }
    Ok(())
}

fn ui(f: &mut ratatui::Frame, snap: &HostSnapshot, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(f.area());

    let access_label = if snap.party_mode { "party" } else { "allow-list" };
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            " TerminalBuddy ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            snap.status.clone(),
            Style::default()
                .fg(status_color(&snap.status))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  access: "),
        Span::styled(
            access_label,
            Style::default().fg(if snap.party_mode {
                Color::Green
            } else {
                Color::Yellow
            }),
        ),
        Span::raw("  chat: "),
        Span::styled(snap.chat_id.clone(), Style::default().fg(Color::Cyan)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" TerminalBuddy ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(title, root[0]);

    let titles = vec!["1 Host", "2 Shell", "3 Access", "4 Share"];
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(" Tabs "))
        .select(app.tab as usize)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .divider(" │ ");
    f.render_widget(tabs, root[1]);

    match app.tab {
        Tab::Host => draw_host_tab(f, root[2], snap, app),
        Tab::Shell => draw_shell_tab(f, root[2], snap, app),
        Tab::Access => draw_access_tab(f, root[2], snap, app.access_sel),
        Tab::Share => draw_share_tab(f, root[2], snap, app),
    }

    let help = if app.invite_edit {
        " editing invite  ·  Enter = join+send  ·  Esc = cancel  ·  Ctrl-u = clear "
    } else {
        match app.tab {
            Tab::Host => " Tab/1–4  ·  ↑↓ log  ·  q quit ",
            Tab::Shell => {
                if app.whitelist_edit {
                    " type program name  ·  Enter add  ·  Esc cancel "
                } else if !snap.pending_cmds.is_empty() {
                    " pending:  y = run once  ·  w = run + whitelist  ·  n = deny  ·  q "
                } else {
                    " y=YOLO  w=whitelist  a=approve  ·  +/− list  ·  t=tmux  ·  q "
                }
            }
            Tab::Access => " Tab  ·  p party  ·  ↑↓  ·  Space grant  ·  a all  ·  n none  ·  q ",
            Tab::Share => {
                " i = edit invite  ·  ↑↓ pick chat/contact  ·  Enter = send  ·  s = send to selection  ·  q "
            }
        }
    };
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(Color::DarkGray)),
        root[3],
    );
}

fn draw_host_tab(f: &mut ratatui::Frame, area: Rect, snap: &HostSnapshot, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(3),
            Constraint::Min(4),
        ])
        .split(area);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(root[0]);

    let xdc_short = shorten(&snap.xdc_path, 48);
    let left = Paragraph::new(vec![
        line_kv("identity", snap.identity.as_str()),
        line_kv("chat", snap.chat_id.as_str()),
        line_kv("webxdc", snap.primary_msg.as_str()),
        line_kv("shell", snap.shell.as_str()),
        line_kv("xdc", xdc_short.as_str()),
        line_kv("hosting", &format!("{} instance(s)", snap.hosting)),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Account ")
            .border_style(Style::default().fg(Color::Blue)),
    )
    .wrap(Wrap { trim: true });
    f.render_widget(left, top[0]);

    let policy = if snap.party_mode {
        "Party mode — everyone may type"
    } else {
        "Allow-list — only granted people + host"
    };
    let right = Paragraph::new(vec![
        line_kv("ads seen", &snap.peers_seen.to_string()),
        line_kv("pings", &snap.pings.to_string()),
        line_kv("bytes in", &format_bytes(snap.bytes_in)),
        line_kv("bytes out", &format_bytes(snap.bytes_out)),
        Line::from(""),
        Line::from(Span::styled(policy, Style::default().fg(Color::Yellow))),
        Line::from(Span::styled(
            "Share webxdc → tab 4",
            Style::default().fg(Color::Cyan),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Realtime P2P ")
            .border_style(Style::default().fg(Color::Magenta)),
    );
    f.render_widget(right, top[1]);

    let access = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(root[1]);
    draw_people_list(
        f,
        access[0],
        &format!(" Chat members ({}) ", snap.chat_members.len()),
        &snap.chat_members,
        Color::Blue,
    );
    draw_people_list(
        f,
        access[1],
        &format!(" Live in app now ({}) ", snap.live_peers.len()),
        &snap.live_peers,
        Color::Green,
    );

    let activity = ((snap.bytes_in + snap.bytes_out) % 1000) as u16 / 10;
    f.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Traffic (relative) "),
            )
            .gauge_style(
                Style::default()
                    .fg(Color::Green)
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .percent(activity.min(100)),
        root[2],
    );
    draw_log(f, root[3], &snap.logs, app.log_scroll);
}

fn draw_share_tab(f: &mut ratatui::Frame, area: Rect, snap: &HostSnapshot, app: &App) {
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(6),
            Constraint::Length(4),
        ])
        .split(area);

    // Invite field
    let display = if app.invite_edit {
        format!("{}▌", app.invite_buf)
    } else if snap.invite_url.is_empty() {
        "(empty — press i and paste invite URL)".into()
    } else {
        shorten(&snap.invite_url, 100)
    };
    let field_border = if app.invite_edit {
        Color::Yellow
    } else if snap.invite_busy {
        Color::Magenta
    } else {
        Color::Cyan
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                display,
                Style::default().fg(if app.invite_edit {
                    Color::White
                } else {
                    Color::Cyan
                }),
            )),
            Line::from(Span::styled(
                snap.invite_hint.clone(),
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(if app.invite_edit {
                    " Invite link (editing) — Enter join+send · Esc cancel "
                } else {
                    " Invite link — i edit · Enter join chat + send webxdc "
                })
                .border_style(Style::default().fg(field_border)),
        )
        .wrap(Wrap { trim: true }),
        body[0],
    );

    // Chat / contact list
    let items: Vec<ListItem> = if snap.share_targets.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "(no chats or contacts yet — join a group or wait for contacts)",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        snap.share_targets
            .iter()
            .map(|t| {
                let tag = match t.kind {
                    ShareKind::Chat => {
                        if t.detail == "group" {
                            "group"
                        } else {
                            "chat"
                        }
                    }
                    ShareKind::Contact => "contact",
                };
                let style = match t.kind {
                    ShareKind::Chat if t.detail == "group" => Style::default().fg(Color::Magenta),
                    ShareKind::Chat => Style::default().fg(Color::Cyan),
                    ShareKind::Contact => Style::default().fg(Color::Green),
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{tag:8} "), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{:20}", t.label), style.add_modifier(Modifier::BOLD)),
                    Span::styled(
                        if t.detail == "group" || t.detail == "direct" {
                            String::new()
                        } else {
                            format!("  {}", t.detail)
                        },
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect()
    };

    let mut state = ListState::default();
    if !snap.share_targets.is_empty() {
        state.select(Some(app.share_sel.min(snap.share_targets.len() - 1)));
    }
    f.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Chats & contacts — ↑↓ select · s / Enter send webxdc here ")
                    .border_style(Style::default().fg(Color::Green)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› "),
        body[1],
        &mut state,
    );

    f.render_widget(
        Paragraph::new(vec![
            Line::from("Invite: joins the group (SecureJoin), sends the mini app, hosts it."),
            Line::from("Chat/contact: sends the mini app into that conversation and hosts it."),
            Line::from("Everyone who opens the newest mini app in that chat shares the terminal."),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Notes ")
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: true }),
        body[2],
    );
}

fn draw_access_tab(f: &mut ratatui::Frame, area: Rect, snap: &HostSnapshot, selected: usize) {
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(6),
            Constraint::Length(5),
        ])
        .split(area);

    let mode = if snap.party_mode {
        "PARTY MODE — everyone in the chat may type"
    } else {
        "ALLOW-LIST — only checked people (+ host) may type"
    };
    let mode_color = if snap.party_mode {
        Color::Green
    } else {
        Color::Yellow
    };
    let writers = snap.chat_members.iter().filter(|p| p.can_write).count();

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                mode,
                Style::default()
                    .fg(mode_color)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "p",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " to toggle party mode.",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
            Line::from(if snap.party_mode {
                "With party off, Space grants/revokes write for the selected person.".into()
            } else {
                format!("Currently {writers} people may write (host always included).")
            }),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Access policy ")
                .border_style(Style::default().fg(mode_color)),
        )
        .wrap(Wrap { trim: true }),
        body[0],
    );

    let items: Vec<ListItem> = if snap.chat_members.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "(no members yet)",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        snap.chat_members
            .iter()
            .map(|p| {
                let level = access_level_label(p, snap.party_mode);
                let style = if p.can_write || snap.party_mode {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Gray)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{:18}", p.label), style.add_modifier(Modifier::BOLD)),
                    Span::styled(level, style),
                ]))
            })
            .collect()
    };

    let mut state = ListState::default();
    if !snap.chat_members.is_empty() {
        state.select(Some(selected.min(snap.chat_members.len() - 1)));
    }
    f.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" People — Space grant/revoke  ·  p party  ·  a all  ·  n none ")
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› "),
        body[1],
        &mut state,
    );

    f.render_widget(
        Paragraph::new(vec![
            Line::from("Party mode: anyone who opens the mini app can type."),
            Line::from("Allow-list: only granted people may type; others can watch."),
            Line::from("Host (you) always has write access."),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Notes ")
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: true }),
        body[2],
    );
}

fn draw_shell_tab(f: &mut ratatui::Frame, area: Rect, snap: &HostSnapshot, app: &App) {
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),
            Constraint::Length(6),
            Constraint::Min(5),
            Constraint::Length(5),
        ])
        .split(area);

    let mode_color = match snap.shell_policy {
        ShellPolicy::Yolo => Color::Green,
        ShellPolicy::Whitelist => Color::Yellow,
        ShellPolicy::Approve => Color::Magenta,
    };

    let yolo_mark = if snap.shell_policy == ShellPolicy::Yolo {
        "(*)"
    } else {
        "( )"
    };
    let wl_mark = if snap.shell_policy == ShellPolicy::Whitelist {
        "(*)"
    } else {
        "( )"
    };
    let ap_mark = if snap.shell_policy == ShellPolicy::Approve {
        "(*)"
    } else {
        "( )"
    };

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Command policy for binaries the shell will run",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("{yolo_mark} "), Style::default().fg(Color::Green)),
                Span::styled(
                    "y  YOLO",
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  — full shell, no filtering",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
            Line::from(vec![
                Span::styled(format!("{wl_mark} "), Style::default().fg(Color::Yellow)),
                Span::styled(
                    "w  whitelist",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  — only listed programs",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
            Line::from(vec![
                Span::styled(format!("{ap_mark} "), Style::default().fg(Color::Magenta)),
                Span::styled(
                    "a  approve",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  — ask each time; w on pending = add to whitelist",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
            Line::from(""),
            line_kv("active", snap.shell.as_str()),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Mode · {} ", snap.shell_policy.label()))
                .border_style(Style::default().fg(mode_color)),
        )
        .wrap(Wrap { trim: true }),
        body[0],
    );

    // Pending approvals
    let pending_lines: Vec<Line> = if snap.pending_cmds.is_empty() {
        vec![Line::from(Span::styled(
            if snap.shell_policy == ShellPolicy::Approve {
                "(none — waiting for someone to run a command)"
            } else {
                "(approve mode off)"
            },
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        snap.pending_cmds
            .iter()
            .take(4)
            .map(|p| {
                Line::from(vec![
                    Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        p.line.clone(),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "  → y once · w +whitelist · n deny",
                        Style::default().fg(Color::Cyan),
                    ),
                ])
            })
            .collect()
    };
    f.render_widget(
        Paragraph::new(pending_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Pending · y = once  ·  w = allow forever (whitelist)  ·  n = deny ")
                .border_style(Style::default().fg(Color::Magenta)),
        ),
        body[1],
    );

    // Whitelist editor
    let wl_title = if app.whitelist_edit {
        format!(" Whitelist (+ editing: {}▌) · Enter save · Esc cancel ", app.whitelist_buf)
    } else {
        " Whitelist · + add  −/d remove  ↑↓ select ".into()
    };
    let wl_items: Vec<ListItem> = if snap.allowed_commands.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "(empty — press + to add, e.g. ls)",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        snap.allowed_commands
            .iter()
            .map(|c| ListItem::new(Line::from(Span::styled(c.clone(), Style::default().fg(Color::Yellow)))))
            .collect()
    };
    let mut wl_state = ListState::default();
    if !snap.allowed_commands.is_empty() {
        wl_state.select(Some(
            app.whitelist_sel
                .min(snap.allowed_commands.len().saturating_sub(1)),
        ));
    }
    f.render_stateful_widget(
        List::new(wl_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(wl_title)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› "),
        body[2],
        &mut wl_state,
    );

    let tmux_line = if snap.shell_policy != ShellPolicy::Yolo {
        "tmux: only available in YOLO mode (press y first, then t)"
    } else if snap.use_tmux {
        "[x] tmux  (t to disable)"
    } else {
        "[ ] tmux  (t to enable)"
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(tmux_line, Style::default().fg(Color::Cyan))),
            Line::from(Span::styled(
                "YOLO = full $SHELL. Whitelist/Approve = gated shell (no free bash).",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "Approve: peers see “waiting…” until you press Y or n here.",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Notes ")
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        body[3],
    );
}

fn draw_people_list(
    f: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    peers: &[crate::AccessPeer],
    border: Color,
) {
    let items: Vec<ListItem> = if peers.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "(none yet)",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        peers
            .iter()
            .map(|p| {
                let level = if p.label.starts_with("host") {
                    "host"
                } else if p.can_write {
                    "write"
                } else {
                    "view"
                };
                let style = if p.can_write {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{:18}", p.label), style.add_modifier(Modifier::BOLD)),
                    Span::styled(level, style),
                ]))
            })
            .collect()
    };
    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(border)),
        ),
        area,
    );
}

fn access_level_label(p: &crate::AccessPeer, party_mode: bool) -> &'static str {
    if p.label.starts_with("host") {
        "host"
    } else if party_mode || p.can_write {
        "write"
    } else {
        "view"
    }
}

fn draw_log(f: &mut ratatui::Frame, area: Rect, logs: &[String], scroll: usize) {
    let h = area.height.saturating_sub(2) as usize;
    let total = logs.len();
    let end = total.saturating_sub(scroll);
    let start = end.saturating_sub(h);
    let items: Vec<ListItem> = logs[start..end]
        .iter()
        .map(|l| {
            let style = if l.contains("error") || l.contains("failed") || l.contains("denied") {
                Style::default().fg(Color::Red)
            } else if l.contains("share:") || l.contains("invite:") {
                Style::default().fg(Color::Cyan)
            } else if l.contains("access:") {
                Style::default().fg(Color::Yellow)
            } else if l.contains("join") || l.contains("peer") || l.contains("online") {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(Line::from(Span::styled(l.clone(), style)))
        })
        .collect();
    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Event log ({total}) "))
                .border_style(Style::default().fg(Color::White)),
        ),
        area,
    );
}

fn line_kv(k: &str, v: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k:12}"), Style::default().fg(Color::DarkGray)),
        Span::styled(v.to_string(), Style::default().fg(Color::White)),
    ])
}

fn status_color(s: &str) -> Color {
    let s = s.to_lowercase();
    if s.contains("error") || s.contains("fail") {
        Color::Red
    } else if s.contains("sending") || s.contains("joining") || s.contains("allow-list") {
        Color::Yellow
    } else if s.contains("party") || s.contains("hosting") || s.contains("connected") {
        Color::Green
    } else {
        Color::Cyan
    }
}

fn format_bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if n >= MB {
        format!("{:.1} MiB", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.1} KiB", n as f64 / KB as f64)
    } else {
        format!("{n} B")
    }
}

fn shorten(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("…{}", &s[s.len() - max + 1..])
    }
}
