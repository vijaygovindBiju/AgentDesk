//! Terminal application runner, event loop, and keyboard interaction.

use std::io::{self, stdout};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, EventStream, KeyCode,
    KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use agentdesk_model::{Decision, QuestionType};

use crate::device::DeviceRegistry;
use crate::discovery::AgentDefinition;
use crate::pairing::load_session_file;
use crate::tui::client::{TuiBridge, TuiEvent};
use crate::tui::key_encoder::encode_key_event;
use crate::tui::preferences::{WorkspacePreferences, save_preferences};
use crate::tui::state::{
    DashboardSection, DisclosureLevel, PairingMode, Screen, TextSelection,
    TuiState, WorkspaceOverlay,
};
use crate::tui::terminal_session::TerminalSession;
use crate::tui::views::render_ui;

pub struct TuiApp {
    pub state: TuiState,
    pub bridge: TuiBridge,
    pub rx_events: mpsc::Receiver<TuiEvent>,
    pub config_dir: PathBuf,
    pub state_machine: agentdesk_core::AntigravityStateMachine,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
    pub click_count: usize,
}

impl TuiApp {
    pub fn new(
        state: TuiState,
        bridge: TuiBridge,
        rx_events: mpsc::Receiver<TuiEvent>,
        config_dir: PathBuf,
    ) -> Self {
        Self {
            state,
            bridge,
            rx_events,
            config_dir,
            state_machine: agentdesk_core::AntigravityStateMachine::new("antigravity-tui"),
            last_click: None,
            click_count: 0,
        }
    }

    pub async fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        crate::logging::set_suppress_stderr(true);
        // Safe terminal setup
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            crossterm::event::PushKeyboardEnhancementFlags(
                crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            )
        )?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Ensure panic restores terminal
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crate::logging::set_suppress_stderr(false);
            let _ = execute!(
                io::stdout(),
                crossterm::event::PopKeyboardEnhancementFlags,
                LeaveAlternateScreen,
                DisableMouseCapture
            );
            let _ = disable_raw_mode();
            default_hook(info);
        }));

        let mut event_stream = EventStream::new();
        let mut tick_interval = tokio::time::interval(Duration::from_millis(250));

        // Initial device & runtime status load
        self.state.refresh_devices(&self.config_dir);
        self.state.refresh_runtime_status(&self.config_dir);

        let res = self.main_loop(&mut terminal, &mut event_stream, &mut tick_interval).await;

        // Restore terminal
        crate::logging::set_suppress_stderr(false);
        let _ = execute!(
            terminal.backend_mut(),
            crossterm::event::PopKeyboardEnhancementFlags,
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        disable_raw_mode()?;
        terminal.show_cursor()?;

        res
    }

    async fn main_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        event_stream: &mut EventStream,
        tick_interval: &mut tokio::time::Interval,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            if let Ok(size) = terminal.size() {
                let term_cols = size.width;
                let term_rows = if self.state.terminal_only_mode {
                    size.height
                } else {
                    size.height.saturating_sub(2)
                };
                if let Some(ref mut session) = self.state.shell_session {
                    session.resize(term_cols, term_rows);
                    session.pump_output();
                }
            }

            terminal.draw(|f| render_ui(f, &self.state))?;

            tokio::select! {
                // Inbound daemon events
                Some(event) = self.rx_events.recv() => {
                    self.handle_daemon_event(event).await;
                }

                // Terminal Keyboard, Mouse, and Resize inputs
                Some(Ok(evt)) = event_stream.next() => {
                    match evt {
                        CrosstermEvent::Key(key) => {
                            if self.handle_key(key).await? {
                                break; // User requested quit
                            }
                        }
                        CrosstermEvent::Mouse(mouse) => {
                            self.handle_mouse(mouse);
                        }
                        CrosstermEvent::Resize(cols, rows) => {
                            let term_rows = if self.state.terminal_only_mode {
                                rows
                            } else {
                                rows.saturating_sub(2)
                            };
                            if let Some(ref mut session) = self.state.shell_session {
                                session.resize(cols, term_rows);
                                session.pump_output();
                            }
                        }
                        CrosstermEvent::Paste(text) => {
                            if let Some(ref mut session) = self.state.shell_session {
                                if session.screen.bracketed_paste {
                                    let _ = session.send_input(b"\x1b[200~");
                                    let _ = session.send_input(text.as_bytes());
                                    let _ = session.send_input(b"\x1b[201~");
                                } else {
                                    let _ = session.send_input(text.as_bytes());
                                }
                                let _ = session.pump_output();
                            }
                        }
                        _ => {}
                    }
                }

                // Periodic tick for duration updates and status checks
                _ = tick_interval.tick() => {
                    self.handle_tick().await;
                }
            }
        }
        Ok(())
    }

    async fn inspect_shell_screen(&mut self) {
        if let Some(ref mut session) = self.state.shell_session {
            let snapshot = session.screen.snapshot();
            if let Some(raw_event) = self.state_machine.update(&snapshot) {
                let _ = self.bridge.send_adapter_event(raw_event).await;
            }
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.state.current_screen == Screen::Workspace
            && self.state.workspace_overlay == WorkspaceOverlay::None
        {
            let term_y = if self.state.terminal_only_mode { 0 } else { 1 };
            let term_height = self
                .state
                .shell_session
                .as_ref()
                .map(|s| s.screen.rows)
                .unwrap_or(24);
            let term_width = self
                .state
                .shell_session
                .as_ref()
                .map(|s| s.screen.cols)
                .unwrap_or(80);

            let is_inside_terminal = mouse.row >= term_y
                && mouse.row < term_y + term_height
                && mouse.column < term_width;

            let grid_row = mouse.row.saturating_sub(term_y);
            let grid_col = mouse.column;

            // Forward mouse event to child process if mouse tracking is active
            if is_inside_terminal && self.state.shell_session.as_ref().map(|s| s.mouse_tracking).unwrap_or(false) {
                let col_1based = grid_col + 1;
                let row_1based = grid_row + 1;
                let (cb, is_release) = match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => (0, false),
                    MouseEventKind::Down(MouseButton::Middle) => (1, false),
                    MouseEventKind::Down(MouseButton::Right) => (2, false),
                    MouseEventKind::Up(MouseButton::Left) => (0, true),
                    MouseEventKind::Up(MouseButton::Middle) => (1, true),
                    MouseEventKind::Up(MouseButton::Right) => (2, true),
                    MouseEventKind::Drag(MouseButton::Left) => (32, false),
                    MouseEventKind::ScrollUp => (64, false),
                    MouseEventKind::ScrollDown => (65, false),
                    _ => (0, false),
                };
                let suffix = if is_release { 'm' } else { 'M' };
                let sgr = format!("\x1b[<{};{};{}{}", cb, col_1based, row_1based, suffix);
                if let Some(ref mut session) = self.state.shell_session {
                    let _ = session.send_input(sgr.as_bytes());
                    let _ = session.pump_output();
                }
                return;
            }

            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    let max_scroll = self
                        .state
                        .shell_session
                        .as_ref()
                        .map(|s| s.screen.scrollback().len())
                        .unwrap_or(0);
                    self.state.terminal_scroll =
                        std::cmp::min(self.state.terminal_scroll + 3, max_scroll);
                    self.state.terminal_selection = None;
                    self.state.is_mouse_selecting = false;
                }
                MouseEventKind::ScrollDown => {
                    self.state.terminal_scroll = self.state.terminal_scroll.saturating_sub(3);
                    self.state.terminal_selection = None;
                    self.state.is_mouse_selecting = false;
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if is_inside_terminal {
                        let now = std::time::Instant::now();
                        let is_same_cell = self
                            .last_click
                            .map(|(inst, r, c)| r == grid_row && c == grid_col && inst.elapsed() < Duration::from_millis(500))
                            .unwrap_or(false);
                        if is_same_cell {
                            self.click_count = (self.click_count % 3) + 1;
                        } else {
                            self.click_count = 1;
                        }
                        self.last_click = Some((now, grid_row, grid_col));

                        if self.click_count == 2 {
                            // Double-click word selection
                            if let Some(ref session) = self.state.shell_session {
                                let line_text = if self.state.terminal_scroll == 0 {
                                    session.screen.row_text(grid_row)
                                } else {
                                    let sb = session.screen.scrollback();
                                    let source = (grid_row as isize) - (self.state.terminal_scroll as isize);
                                    if source < 0 {
                                        let idx = (sb.len() as isize) + source;
                                        if idx >= 0 && (idx as usize) < sb.len() {
                                            sb[idx as usize].clone()
                                        } else {
                                            String::new()
                                        }
                                    } else {
                                        session.screen.row_text(source as u16)
                                    }
                                };
                                let (word_start, word_end) = find_word_boundaries(&line_text, grid_col as usize);
                                let sel = TextSelection {
                                    start_row: grid_row,
                                    start_col: word_start,
                                    end_row: grid_row,
                                    end_col: word_end,
                                };
                                self.state.terminal_selection = Some(sel);
                                self.state.is_mouse_selecting = false;
                                let selected_text = crate::tui::views::workspace::extract_selected_text(
                                    &session.screen,
                                    self.state.terminal_scroll,
                                    session.screen.rows,
                                    session.screen.cols,
                                    sel,
                                );
                                if !selected_text.trim().is_empty() {
                                    crate::tui::clipboard::copy_to_clipboard(&selected_text);
                                    self.state.clipboard_notice = Some((
                                        format!("Copied: {}", selected_text),
                                        std::time::Instant::now(),
                                    ));
                                }
                            }
                        } else if self.click_count == 3 {
                            // Triple-click line selection
                            if let Some(ref session) = self.state.shell_session {
                                let sel = TextSelection {
                                    start_row: grid_row,
                                    start_col: 0,
                                    end_row: grid_row,
                                    end_col: term_width.saturating_sub(1),
                                };
                                self.state.terminal_selection = Some(sel);
                                self.state.is_mouse_selecting = false;
                                let selected_text = crate::tui::views::workspace::extract_selected_text(
                                    &session.screen,
                                    self.state.terminal_scroll,
                                    session.screen.rows,
                                    session.screen.cols,
                                    sel,
                                );
                                if !selected_text.trim().is_empty() {
                                    crate::tui::clipboard::copy_to_clipboard(&selected_text);
                                    self.state.clipboard_notice = Some((
                                        format!("Copied line ({} chars)", selected_text.chars().count()),
                                        std::time::Instant::now(),
                                    ));
                                }
                            }
                        } else {
                            self.state.terminal_selection = Some(TextSelection {
                                start_col: grid_col,
                                start_row: grid_row,
                                end_col: grid_col,
                                end_row: grid_row,
                            });
                            self.state.is_mouse_selecting = true;
                        }
                    } else {
                        self.state.terminal_selection = None;
                        self.state.is_mouse_selecting = false;
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if self.state.is_mouse_selecting {
                        if mouse.row < term_y {
                            let max_scroll = self
                                .state
                                .shell_session
                                .as_ref()
                                .map(|s| s.screen.scrollback().len())
                                .unwrap_or(0);
                            self.state.terminal_scroll =
                                std::cmp::min(self.state.terminal_scroll + 1, max_scroll);
                        } else if mouse.row >= term_y + term_height {
                            self.state.terminal_scroll = self.state.terminal_scroll.saturating_sub(1);
                        }

                        let clamped_row = std::cmp::min(grid_row, term_height.saturating_sub(1));
                        let clamped_col = std::cmp::min(grid_col, term_width.saturating_sub(1));
                        if let Some(ref mut sel) = self.state.terminal_selection {
                            sel.end_col = clamped_col;
                            sel.end_row = clamped_row;
                        }
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    if self.state.is_mouse_selecting {
                        self.state.is_mouse_selecting = false;
                        if let Some(sel) = self.state.terminal_selection {
                            if sel.start_col != sel.end_col || sel.start_row != sel.end_row {
                                if let Some(ref session) = self.state.shell_session {
                                    let selected_text = crate::tui::views::workspace::extract_selected_text(
                                        &session.screen,
                                        self.state.terminal_scroll,
                                        session.screen.rows,
                                        session.screen.cols,
                                        sel,
                                    );
                                    if !selected_text.trim().is_empty() {
                                        crate::tui::clipboard::copy_to_clipboard(&selected_text);
                                        self.state.clipboard_notice = Some((
                                            format!("Copied {} chars", selected_text.chars().count()),
                                            std::time::Instant::now(),
                                        ));
                                    }
                                }
                            } else {
                                self.state.terminal_selection = None;
                            }
                        }
                    }
                }
                MouseEventKind::Down(MouseButton::Right) => {
                    self.state.terminal_selection = None;
                    self.state.is_mouse_selecting = false;
                }
                _ => {}
            }
        }
    }
}

fn find_word_boundaries(line: &str, col: usize) -> (u16, u16) {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let col = col.min(chars.len().saturating_sub(1));
    let is_word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == ':';
    let mut start = col;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end + 1 < chars.len() && is_word_char(chars[end + 1]) {
        end += 1;
    }
    (start as u16, end as u16)
}

impl TuiApp {

    async fn handle_daemon_event(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::Snapshot(s) => self.state.apply_snapshot(s),
            TuiEvent::Event(e) => self.state.apply_event(e),
            TuiEvent::ScoreUpdate(s) => self.state.apply_score_update(s),
            TuiEvent::StateUpdate(s) => self.state.apply_state_update(s),
            TuiEvent::Logs(l) => self.state.apply_logs(l),
            TuiEvent::PhoneConnected(conn, transport, name) => {
                self.state.phone_connected = conn;
                self.state.phone_transport = transport;
                self.state.phone_device_name = name;
            }
            TuiEvent::PairingSessionCreated {
                session_id,
                uri,
                qr,
                instance_id,
                expires_at,
            } => {
                self.state.pairing_session_id = Some(session_id);
                self.state.pairing_uri = Some(uri);
                self.state.pairing_qr = Some(qr);
                self.state.pairing_instance_id = Some(instance_id);
                self.state.pairing_expires_at = Some(expires_at);
                self.state.pairing_paired_device = None;
                self.state.pairing_error = None;
            }
            TuiEvent::PairingComplete { device_name, device_id } => {
                self.state.pairing_paired_device = Some((device_name, device_id));
                self.state.refresh_devices(&self.config_dir);
            }
            TuiEvent::PairingFailed(err) => {
                self.state.pairing_error = Some(err);
            }
            TuiEvent::PromptReceived { prompt, workspace, agent_id: _, mode } => {
                if self.state.is_attached_mode {
                    // In attached mode, the background daemon service already runs the agent adapter
                    // and processes this prompt. Do not spawn a second duplicate agent session in the TUI shell.
                    return;
                }
                if self.state.is_duplicate_prompt(&prompt) {
                    return;
                }
                if let Some(ref ws) = workspace {
                    let p = PathBuf::from(ws);
                    if p.is_dir() {
                        self.state.workspace_dir = p.clone();
                        self.state.add_recent_workspace(p);
                        self.save_current_preferences();
                    }
                }
                if let Some(ref m) = mode {
                    self.state.dangerously_skip_permissions = m == "dangerously-skip-permissions"
                        || m == "--dangerously-skip-permissions";
                    self.save_current_preferences();
                }

                let need_spawn = match &self.state.shell_session {
                    None => true,
                    Some(s) => !s.is_alive(),
                };

                if need_spawn {
                    if let Ok(mut session) = TerminalSession::spawn_shell(&self.state.workspace_dir, 120, 40) {
                        let _ = session.pump_output();
                        self.state.shell_session = Some(session);
                    }
                }

                let exe = self.state.selected_agent().map(|a| a.executable.clone()).unwrap_or_else(|| "agy".into());

                if let Some(ev) = self.state_machine.start_prompt(&prompt) {
                    let _ = self.bridge.send_adapter_event(ev).await;
                }

                if let Some(ref mut session) = self.state.shell_session {
                    let _ = session.pump_output();
                    let snapshot = session.screen.snapshot();
                    let live = agentdesk_core::detect_state(&snapshot);

                    if live.accepts_chat_input() {
                        let payload = if prompt.contains('\n') {
                            format!("\x1b[200~{prompt}\x1b[201~\r")
                        } else {
                            format!("{prompt}\r")
                        };
                        let _ = session.send_input(payload.as_bytes());
                        let _ = session.pump_output();
                    } else {
                        let escaped = prompt.replace('\'', "'\\''");
                        let skip_perm = mode.as_deref() == Some("dangerously-skip-permissions")
                            || mode.as_deref() == Some("--dangerously-skip-permissions");
                        let cmd = if exe == "agy" || exe.ends_with("/agy") {
                            if skip_perm {
                                format!("agy --dangerously-skip-permissions -i '{}'\r", escaped)
                            } else {
                                format!("agy -i '{}'\r", escaped)
                            }
                        } else {
                            format!("{} '{}'\r", exe, escaped)
                        };
                        let _ = session.send_input(cmd.as_bytes());
                        let _ = session.pump_output();
                    }
                }

                self.state.current_screen = Screen::Workspace;
                self.state.workspace_overlay = WorkspaceOverlay::None;
                self.state.agent_session_running = true;
            }
            TuiEvent::AdapterRespond { event_id, decision, response } => {
                let outcome = if let Some(ref mut session) = self.state.shell_session {
                    let snapshot = session.screen.snapshot();
                    let live = agentdesk_core::detect_state(&snapshot);
                    let input_result = if let Some(ref resp) = response {
                        agentdesk_core::encode_response_for_state(&live, resp)
                    } else {
                        match &live {
                            agentdesk_core::AntigravityState::AwaitingReply { .. } => {
                                Err(agentdesk_core::RespondError::InvalidResponse)
                            }
                            _ => Ok(agentdesk_core::QuestionInput::Submit(
                                agentdesk_core::encode_decision(decision, &live),
                            )),
                        }
                    };

                    match input_result {
                        Ok(agentdesk_core::QuestionInput::Submit(bytes)) => {
                            let _ = session.send_input(&bytes);
                            let _ = session.pump_output();
                            agentdesk_core::AdapterOutcome::Delivered { event_id, decision }
                        }
                        Ok(agentdesk_core::QuestionInput::OpenWriteIn { bytes, .. }) => {
                            let _ = session.send_input(&bytes);
                            let _ = session.pump_output();
                            agentdesk_core::AdapterOutcome::Delivered { event_id, decision }
                        }
                        Err(err) => agentdesk_core::AdapterOutcome::DeliveryFailed {
                            event_id,
                            reason: format!("Failed encoding decision: {err}"),
                        },
                    }
                } else {
                    agentdesk_core::AdapterOutcome::DeliveryFailed {
                        event_id,
                        reason: "No active shell session".into(),
                    }
                };

                let _ = self.bridge.send_adapter_response(outcome).await;
                self.inspect_shell_screen().await;
            }
        }
    }

    async fn handle_tick(&mut self) {
        self.state.refresh_runtime_status(&self.config_dir);
        self.state.refresh_devices(&self.config_dir);

        if let Some(ref mut session) = self.state.shell_session {
            session.pump_output();
        }
        self.inspect_shell_screen().await;

        // Check if pairing completed in session file
        if self.state.current_screen == Screen::Pairing {
            if let Some(ref sess_id) = self.state.pairing_session_id {
                if let Some(sess) = load_session_file(&self.config_dir) {
                    if sess.session_id == *sess_id && sess.used {
                        let name = sess.paired_device_name.unwrap_or_else(|| "Android phone".into());
                        let id = sess.paired_device_id.unwrap_or_else(|| "unknown".into());
                        self.state.pairing_paired_device = Some((name, id));
                        self.state.refresh_devices(&self.config_dir);
                    }
                }
            }
        }
    }

    /// Handles keyboard input. Returns true if application should exit.
    async fn handle_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        // Handle AgentSelection screen
        if self.state.current_screen == Screen::AgentSelection {
            if self.state.workspace_overlay == WorkspaceOverlay::ProjectSelect {
                match key.code {
                    KeyCode::Up => {
                        if !self.state.recent_workspaces.is_empty() {
                            if self.state.project_select_index == 0 {
                                self.state.project_select_index = self.state.recent_workspaces.len().saturating_sub(1);
                            } else {
                                self.state.project_select_index -= 1;
                            }
                            if let Some(p) = self.state.recent_workspaces.get(self.state.project_select_index) {
                                self.state.project_input_text = p.to_string_lossy().to_string();
                            }
                        }
                    }
                    KeyCode::Down => {
                        if !self.state.recent_workspaces.is_empty() {
                            self.state.project_select_index = (self.state.project_select_index + 1) % self.state.recent_workspaces.len();
                            if let Some(p) = self.state.recent_workspaces.get(self.state.project_select_index) {
                                self.state.project_input_text = p.to_string_lossy().to_string();
                            }
                        }
                    }
                    KeyCode::Tab => {
                        self.state.tab_complete_project_input();
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.state.project_completions.clear();
                        self.state.project_completion_index = 0;
                        self.state.project_input_text.push(c);
                    }
                    KeyCode::Backspace => {
                        self.state.project_completions.clear();
                        self.state.project_completion_index = 0;
                        self.state.project_input_text.pop();
                    }
                    KeyCode::Enter => {
                        let path_str = self.state.project_input_text.trim();
                        let path = if path_str.starts_with("~/") || path_str == "~" {
                            if let Some(home) = std::env::var_os("HOME") {
                                let mut p = PathBuf::from(home);
                                if path_str.len() > 1 {
                                    p.push(&path_str[2..]);
                                }
                                p
                            } else {
                                PathBuf::from(path_str)
                            }
                        } else {
                            PathBuf::from(path_str)
                        };
                        if path.is_dir() {
                            self.state.workspace_dir = path.clone();
                            self.state.add_recent_workspace(path);
                            self.save_current_preferences();
                        }
                        self.state.workspace_overlay = WorkspaceOverlay::None;
                    }
                    KeyCode::Esc => {
                        self.state.workspace_overlay = WorkspaceOverlay::None;
                    }
                    _ => {}
                }
                return Ok(false);
            }

            match key.code {
                KeyCode::Up => {
                    self.state.select_previous_agent();
                    return Ok(false);
                }
                KeyCode::Down => {
                    self.state.select_next_agent();
                    return Ok(false);
                }
                KeyCode::Char('m') | KeyCode::Tab | KeyCode::F(8) => {
                    self.state.toggle_execution_mode();
                    self.save_current_preferences();
                    return Ok(false);
                }
                KeyCode::Char('d') | KeyCode::Char('p') | KeyCode::F(2) => {
                    self.state.open_project_select();
                    return Ok(false);
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.state.terminal_only_mode = true;
                    self.launch_selected_agent().await;
                    return Ok(false);
                }
                KeyCode::Enter => {
                    self.launch_selected_agent().await;
                    return Ok(false);
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    return Ok(true);
                }
                _ => return Ok(false),
            }
        }

        // Handle Workspace screen (where the REAL TERMINAL lives!)
        if self.state.current_screen == Screen::Workspace {
            if self.state.workspace_overlay != WorkspaceOverlay::None {
                match self.state.workspace_overlay {
                    WorkspaceOverlay::AgentSwitch => match key.code {
                        KeyCode::Up => self.state.select_previous_switch_agent(),
                        KeyCode::Down => self.state.select_next_switch_agent(),
                        KeyCode::Enter => {
                            if let Some(agent) = self.state.detected_agents.get(self.state.agent_switch_index).cloned() {
                                let is_active = self.state.active_agent.as_ref().map(|a| a.id == agent.id).unwrap_or(false);
                                if !is_active && self.state.agent_session_running {
                                    self.state.agent_switch_pending_index = Some(self.state.agent_switch_index);
                                    self.state.workspace_overlay = WorkspaceOverlay::SwitchConfirm;
                                } else {
                                    self.switch_to_agent(agent).await;
                                    self.state.workspace_overlay = WorkspaceOverlay::None;
                                }
                            }
                        }
                        KeyCode::Esc => {
                            self.state.workspace_overlay = WorkspaceOverlay::None;
                        }
                        _ => {}
                    },
                    WorkspaceOverlay::SwitchConfirm => match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            if let Some(idx) = self.state.agent_switch_pending_index.take() {
                                if let Some(agent) = self.state.detected_agents.get(idx).cloned() {
                                    self.switch_to_agent(agent).await;
                                }
                            }
                            self.state.workspace_overlay = WorkspaceOverlay::None;
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            self.state.agent_switch_pending_index = None;
                            self.state.workspace_overlay = WorkspaceOverlay::None;
                        }
                        _ => {}
                    },
                    WorkspaceOverlay::ProjectSelect => match key.code {
                        KeyCode::Tab => {
                            self.state.tab_complete_project_input();
                        }
                        KeyCode::Up => {
                            if !self.state.recent_workspaces.is_empty() {
                                if self.state.project_select_index == 0 {
                                    self.state.project_select_index = self.state.recent_workspaces.len().saturating_sub(1);
                                } else {
                                    self.state.project_select_index -= 1;
                                }
                                if let Some(p) = self.state.recent_workspaces.get(self.state.project_select_index) {
                                    self.state.project_input_text = p.to_string_lossy().to_string();
                                }
                            }
                        }
                        KeyCode::Down => {
                            if !self.state.recent_workspaces.is_empty() {
                                self.state.project_select_index = (self.state.project_select_index + 1) % self.state.recent_workspaces.len();
                                if let Some(p) = self.state.recent_workspaces.get(self.state.project_select_index) {
                                    self.state.project_input_text = p.to_string_lossy().to_string();
                                }
                            }
                        }
                        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.state.project_completions.clear();
                            self.state.project_completion_index = 0;
                            self.state.project_input_text.push(c);
                        }
                        KeyCode::Backspace => {
                            self.state.project_completions.clear();
                            self.state.project_completion_index = 0;
                            self.state.project_input_text.pop();
                        }
                        KeyCode::Enter => {
                            let path_str = self.state.project_input_text.trim();
                            let path = if path_str.starts_with("~/") || path_str == "~" {
                                if let Some(home) = std::env::var_os("HOME") {
                                    let mut p = PathBuf::from(home);
                                    if path_str.len() > 1 {
                                        p.push(&path_str[2..]);
                                    }
                                    p
                                } else {
                                    PathBuf::from(path_str)
                                }
                            } else {
                                PathBuf::from(path_str)
                            };
                            if path.is_dir() {
                                self.state.workspace_dir = path.clone();
                                self.state.add_recent_workspace(path);
                                self.save_current_preferences();
                                let _ = self.state.shell_session.take();
                                if let Ok(mut session) = TerminalSession::spawn_shell(&self.state.workspace_dir, 120, 40) {
                                    let _ = session.pump_output();
                                    self.state.shell_session = Some(session);
                                }
                            }
                            self.state.workspace_overlay = WorkspaceOverlay::None;
                        }
                        KeyCode::Esc => {
                            self.state.workspace_overlay = WorkspaceOverlay::None;
                        }
                        _ => {}
                    },
                    WorkspaceOverlay::Attention => match key.code {
                        KeyCode::Char('d') | KeyCode::Char('c') => {
                            let event_ids: Vec<agentdesk_model::EventId> = self.state.requests().iter().map(|(entry, _)| entry.event_id).collect();
                            for id in event_ids {
                                let _ = self.bridge.dismiss(id).await;
                                self.state.entries.retain(|e| e.event_id != id);
                            }
                        }
                        KeyCode::Char('a') | KeyCode::Enter => {
                            if let Some((first_entry, _)) = self.state.requests().first() {
                                    let event_id = first_entry.event_id;
                                let req = agentdesk_model::RespondRequest {
                                    event_id,
                                    decision: Decision::Approve,
                                    selected_options: None,
                                    text_input: None,
                                };
                                let _ = self.bridge.send_response(req).await;
                                self.state.entries.retain(|e| e.event_id != event_id);
                            }
                        }
                        KeyCode::Esc => {
                            self.state.workspace_overlay = WorkspaceOverlay::None;
                        }
                        _ => {}
                    },
                    WorkspaceOverlay::Menu => match key.code {
                        KeyCode::Up => {
                            if self.state.config_menu_index == 0 {
                                self.state.config_menu_index = 7;
                            } else {
                                self.state.config_menu_index -= 1;
                            }
                        }
                        KeyCode::Down => {
                            self.state.config_menu_index = (self.state.config_menu_index + 1) % 8;
                        }
                        KeyCode::Char('1') => {
                            self.initiate_pairing(PairingMode::Relay);
                            self.state.workspace_overlay = WorkspaceOverlay::Pairing;
                        }
                        KeyCode::Char('2') => {
                            self.state.navigate_to(Screen::Devices);
                            self.state.workspace_overlay = WorkspaceOverlay::None;
                        }
                        KeyCode::Char('3') => {
                            self.state.workspace_overlay = WorkspaceOverlay::Attention;
                        }
                        KeyCode::Char('4') => {
                            self.state.navigate_to(Screen::Connection);
                            self.state.workspace_overlay = WorkspaceOverlay::None;
                        }
                        KeyCode::Char('5') => {
                            self.state.agent_switch_index = self.state.selected_agent_index;
                            self.state.workspace_overlay = WorkspaceOverlay::AgentSwitch;
                        }
                        KeyCode::Char('6') => {
                            self.state.open_project_select();
                        }
                        KeyCode::Char('7') => {
                            self.state.toggle_execution_mode();
                            self.save_current_preferences();
                        }
                        KeyCode::Char('8') => {
                            return Ok(true);
                        }
                        KeyCode::Enter => {
                            match self.state.config_menu_index {
                                0 => {
                                    self.initiate_pairing(PairingMode::Relay);
                                    self.state.workspace_overlay = WorkspaceOverlay::Pairing;
                                }
                                1 => {
                                    self.state.navigate_to(Screen::Devices);
                                    self.state.workspace_overlay = WorkspaceOverlay::None;
                                }
                                2 => {
                                    self.state.workspace_overlay = WorkspaceOverlay::Attention;
                                }
                                3 => {
                                    self.state.navigate_to(Screen::Connection);
                                    self.state.workspace_overlay = WorkspaceOverlay::None;
                                }
                                4 => {
                                    self.state.agent_switch_index = self.state.selected_agent_index;
                                    self.state.workspace_overlay = WorkspaceOverlay::AgentSwitch;
                                }
                                5 => {
                                    self.state.open_project_select();
                                }
                                6 => {
                                    self.state.toggle_execution_mode();
                                    self.save_current_preferences();
                                }
                                7 => {
                                    return Ok(true);
                                }
                                _ => {}
                            }
                        }
                        KeyCode::Esc => {
                            self.state.workspace_overlay = WorkspaceOverlay::None;
                        }
                        _ => {}
                    },
                    WorkspaceOverlay::Pairing => match key.code {
                        KeyCode::Esc => {
                            self.state.workspace_overlay = WorkspaceOverlay::None;
                        }
                        _ => {}
                    },
                    WorkspaceOverlay::PromptInput => {
                        match key.code {
                            KeyCode::Esc => {
                                self.state.workspace_overlay = WorkspaceOverlay::None;
                                self.state.prompt_input.clear();
                                self.state.prompt_history.reset_navigation();
                            }
                            KeyCode::Tab | KeyCode::F(8) => {
                                self.state.toggle_execution_mode();
                                self.save_current_preferences();
                            }
                            KeyCode::F(2) | KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::ALT) => {
                                self.state.open_project_select();
                            }
                            KeyCode::Enter => {
                                let prompt = self.state.prompt_input.trim().to_string();
                                if !prompt.is_empty() {
                                    self.state.prompt_history.push(&prompt);
                                    self.state.prompt_input.clear();
                                    self.state.workspace_overlay = WorkspaceOverlay::None;
                                    self.handle_local_prompt(prompt).await;
                                }
                            }
                            KeyCode::Backspace => {
                                self.state.prompt_input.pop();
                            }
                            KeyCode::Up => {
                                let current = self.state.prompt_input.clone();
                                if let Some(text) = self.state.prompt_history.up(&current) {
                                    self.state.prompt_input = text.to_string();
                                }
                            }
                            KeyCode::Down => {
                                if let Some(text) = self.state.prompt_history.down() {
                                    self.state.prompt_input = text.to_string();
                                }
                            }
                            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                self.state.prompt_input.push(c);
                            }
                            KeyCode::PageUp | KeyCode::PageDown => {
                                // Overlay is open — ignore PgUp/PgDn
                            }
                            _ => {}
                        }
                    }
                    WorkspaceOverlay::Search => {
                        match key.code {
                            KeyCode::Esc | KeyCode::Enter => {
                                self.state.workspace_overlay = WorkspaceOverlay::None;
                            }
                            KeyCode::Backspace => {
                                self.state.search_query.pop();
                                self.state.perform_regex_search();
                            }
                            KeyCode::Char('n') => {
                                self.state.select_next_search_match();
                            }
                            KeyCode::Char('N') => {
                                self.state.select_prev_search_match();
                            }
                            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                self.state.search_query.push(c);
                                self.state.perform_regex_search();
                            }
                            _ => {}
                        }
                    }
                    WorkspaceOverlay::UrlPicker => {
                        match key.code {
                            KeyCode::Esc => {
                                self.state.workspace_overlay = WorkspaceOverlay::None;
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                if self.state.url_picker_index > 0 {
                                    self.state.url_picker_index -= 1;
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if !self.state.detected_urls.is_empty()
                                    && self.state.url_picker_index + 1 < self.state.detected_urls.len()
                                {
                                    self.state.url_picker_index += 1;
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(url) = self.state.detected_urls.get(self.state.url_picker_index).cloned() {
                                    crate::tui::clipboard::open_url_in_browser(&url);
                                    self.state.clipboard_notice = Some((format!("Opened {}", url), std::time::Instant::now()));
                                }
                                self.state.workspace_overlay = WorkspaceOverlay::None;
                            }
                            KeyCode::Char('c') | KeyCode::Char('y') => {
                                if let Some(url) = self.state.detected_urls.get(self.state.url_picker_index).cloned() {
                                    crate::tui::clipboard::copy_to_clipboard(&url);
                                    self.state.clipboard_notice = Some(("Copied URL to clipboard".into(), std::time::Instant::now()));
                                }
                                self.state.workspace_overlay = WorkspaceOverlay::None;
                            }
                            _ => {}
                        }
                    }
                    _ => {
                        self.state.workspace_overlay = WorkspaceOverlay::None;
                    }
                }
                return Ok(false);
            }

            // In Workspace without overlay:
            // Check visual mode navigation
            if self.state.is_visual_mode {
                let max_cols = self.state.shell_session.as_ref().map(|s| s.screen.cols).unwrap_or(80);
                let max_rows = self.state.shell_session.as_ref().map(|s| s.screen.rows).unwrap_or(24);
                match key.code {
                    KeyCode::Esc => {
                        self.state.is_visual_mode = false;
                        self.state.terminal_selection = None;
                        return Ok(false);
                    }
                    KeyCode::Char('y') => {
                        if let Some(ref session) = self.state.shell_session {
                            if let Some(sel) = self.state.terminal_selection {
                                let text = crate::tui::views::workspace::extract_selected_text(
                                    &session.screen,
                                    self.state.terminal_scroll,
                                    session.screen.rows,
                                    session.screen.cols,
                                    sel,
                                );
                                if !text.is_empty() {
                                    crate::tui::clipboard::copy_to_clipboard(&text);
                                    self.state.clipboard_notice = Some((
                                        format!("Yanked {} chars", text.chars().count()),
                                        std::time::Instant::now(),
                                    ));
                                }
                            }
                        }
                        self.state.is_visual_mode = false;
                        self.state.terminal_selection = None;
                        return Ok(false);
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        self.state.move_visual_cursor(0, -1, max_rows, max_cols);
                        return Ok(false);
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        self.state.move_visual_cursor(0, 1, max_rows, max_cols);
                        return Ok(false);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.state.move_visual_cursor(-1, 0, max_rows, max_cols);
                        return Ok(false);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.state.move_visual_cursor(1, 0, max_rows, max_cols);
                        return Ok(false);
                    }
                    _ => {}
                }
            }

            // Ctrl+F in terminal view: Toggles terminal-only mode on and off
            if key.code == KeyCode::Char('f') && key.modifiers.contains(KeyModifiers::CONTROL) {
                self.state.terminal_only_mode = !self.state.terminal_only_mode;
                if let Some(ref mut session) = self.state.shell_session {
                    let rows = if self.state.terminal_only_mode {
                        session.screen.rows.saturating_add(2)
                    } else {
                        session.screen.rows.saturating_sub(2)
                    };
                    session.resize(session.screen.cols, rows);
                    session.pump_output();
                }
                return Ok(false);
            }

            // Ctrl+] or Ctrl+Q in terminal-only mode: Detaches from active terminal and returns to supervisor view
            if self.state.terminal_only_mode
                && ((key.code == KeyCode::Char(']') && key.modifiers.contains(KeyModifiers::CONTROL))
                    || (key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL)))
            {
                self.state.terminal_only_mode = false;
                self.state.current_screen = self.state.previous_screen.unwrap_or(Screen::Dashboard);
                if let Some(ref mut session) = self.state.shell_session {
                    let rows = session.screen.rows.saturating_sub(2);
                    session.resize(session.screen.cols, rows);
                    session.pump_output();
                }
                return Ok(false);
            }

            // Interactive URL Picker (o / Ctrl+O)
            if (key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL))
                || ((self.state.terminal_only_mode || self.state.terminal_scroll > 0)
                    && key.code == KeyCode::Char('o')
                    && !key.modifiers.contains(KeyModifiers::CONTROL))
            {
                self.state.scan_terminal_urls();
                self.state.workspace_overlay = WorkspaceOverlay::UrlPicker;
                return Ok(false);
            }

            // Scrollback Regex Search (/)
            if (self.state.terminal_only_mode || self.state.terminal_scroll > 0)
                && key.code == KeyCode::Char('/')
                && !key.modifiers.contains(KeyModifiers::CONTROL)
            {
                self.state.search_query.clear();
                self.state.search_matches.clear();
                self.state.search_match_index = 0;
                self.state.workspace_overlay = WorkspaceOverlay::Search;
                return Ok(false);
            }

            // Navigate search matches with n / N
            if (self.state.terminal_only_mode || self.state.terminal_scroll > 0)
                && !self.state.search_matches.is_empty()
            {
                if key.code == KeyCode::Char('n') && !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.state.select_next_search_match();
                    return Ok(false);
                }
                if key.code == KeyCode::Char('N')
                    || (key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::SHIFT))
                {
                    self.state.select_prev_search_match();
                    return Ok(false);
                }
            }

            // Visual Keyboard Selection (v)
            if (self.state.terminal_only_mode || self.state.terminal_scroll > 0)
                && key.code == KeyCode::Char('v')
                && !key.modifiers.contains(KeyModifiers::CONTROL)
            {
                let (cr, cc) = if let Some(ref session) = self.state.shell_session {
                    (session.screen.cursor_row, session.screen.cursor_col)
                } else {
                    (0, 0)
                };
                self.state.start_visual_mode(cr, cc);
                return Ok(false);
            }

            // Check hotkeys
            if key.code == KeyCode::F(1) || (key.code == KeyCode::Char('a') && key.modifiers.contains(KeyModifiers::ALT)) {
                self.state.agent_switch_index = self.state.selected_agent_index;
                self.state.workspace_overlay = WorkspaceOverlay::AgentSwitch;
                return Ok(false);
            }
            if key.code == KeyCode::F(2) || (key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::ALT)) {
                self.state.open_project_select();
                return Ok(false);
            }
            if key.code == KeyCode::F(3) || (key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::ALT)) {
                self.state.workspace_overlay = WorkspaceOverlay::Attention;
                return Ok(false);
            }
            if key.code == KeyCode::F(4)
                || key.code == KeyCode::F(5)
                || (key.code == KeyCode::Char('m') && key.modifiers.contains(KeyModifiers::ALT))
                || (key.code == KeyCode::Char('m') && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                self.state.workspace_overlay = WorkspaceOverlay::Menu;
                return Ok(false);
            }
            if key.code == KeyCode::F(6) || (key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::ALT)) {
                // Cycle split pane layout: Single -> Horizontal -> Vertical -> Single
                self.state.split_layout = match self.state.split_layout {
                    crate::tui::state::SplitPaneLayout::Single => crate::tui::state::SplitPaneLayout::Horizontal,
                    crate::tui::state::SplitPaneLayout::Horizontal => crate::tui::state::SplitPaneLayout::Vertical,
                    crate::tui::state::SplitPaneLayout::Vertical => crate::tui::state::SplitPaneLayout::Single,
                };
                self.state.pane_zoomed = false;
                return Ok(false);
            }
            // Switch pane focus: Ctrl+W or Tab in split mode
            if key.code == KeyCode::Char('w') && key.modifiers.contains(KeyModifiers::CONTROL) {
                self.state.pane_focus = match self.state.pane_focus {
                    crate::tui::state::PaneFocus::Primary => crate::tui::state::PaneFocus::Secondary,
                    crate::tui::state::PaneFocus::Secondary => crate::tui::state::PaneFocus::Primary,
                };
                return Ok(false);
            }
            // Zoom / Fullscreen toggle pane: Alt+Z
            if key.code == KeyCode::Char('z') && key.modifiers.contains(KeyModifiers::ALT) {
                self.state.pane_zoomed = !self.state.pane_zoomed;
                return Ok(false);
            }
            if key.code == KeyCode::F(8) {
                self.state.toggle_execution_mode();
                self.save_current_preferences();
                return Ok(false);
            }
            if key.code == KeyCode::F(10) || (key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::ALT)) {
                return Ok(true);
            }

            // Ctrl+I opens the TUI prompt input bar
            if key.code == KeyCode::Char('i') && key.modifiers.contains(KeyModifiers::CONTROL) {
                self.state.workspace_overlay = WorkspaceOverlay::PromptInput;
                return Ok(false);
            }

            // PageUp / PageDown in Workspace (without overlay) scroll terminal scrollback
            if key.code == KeyCode::PageUp {
                let max_scroll = self
                    .state
                    .shell_session
                    .as_ref()
                    .map(|s| s.screen.scrollback().len())
                    .unwrap_or(0);
                self.state.terminal_scroll =
                    std::cmp::min(self.state.terminal_scroll + 15, max_scroll);
                return Ok(false);
            }
            if key.code == KeyCode::PageDown {
                self.state.terminal_scroll = self.state.terminal_scroll.saturating_sub(15);
                return Ok(false);
            }

            // Clear selection on key press (Esc clears selection without forwarding Esc to shell)
            if self.state.terminal_selection.is_some() {
                self.state.terminal_selection = None;
                if key.code == KeyCode::Esc {
                    return Ok(false);
                }
            }

            // If terminal is scrolled up into history:
            // Esc resets scrollback to 0 without sending Esc to shell
            if self.state.terminal_scroll > 0 && key.code == KeyCode::Esc {
                self.state.terminal_scroll = 0;
                return Ok(false);
            }
            // Any other typing or Enter resets scrollback back to 0 so prompt is visible
            if self.state.terminal_scroll > 0 {
                self.state.terminal_scroll = 0;
            }

            // All other keystrokes go straight to the active PTY!
            if let Some(bytes) = encode_key_event(key) {
                if let Some(ref mut session) = self.state.shell_session {
                    let _ = session.send_input(&bytes);
                    let _ = session.pump_output();
                }
                self.inspect_shell_screen().await;
            }
            return Ok(false);
        }

        // Ctrl+F from sessions list directly opens selected session into terminal-only full-screen mode
        if key.code == KeyCode::Char('f') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.state.terminal_only_mode = true;
            self.state.previous_screen = Some(self.state.current_screen);
            if self.state.shell_session.is_none() {
                let ws = self.state.workspace_dir.clone();
                if let Ok(session) = TerminalSession::spawn_shell(&ws, 80, 24) {
                    self.state.shell_session = Some(session);
                }
            }
            self.state.current_screen = Screen::Workspace;
            return Ok(false);
        }

        // Global quit for legacy screens
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(true);
        }

        // If in text typing mode inside Requests, handle characters
        if self.state.current_screen == Screen::Requests {
            if self.is_typing_active() {
                match key.code {
                    KeyCode::Esc => {
                        // Cancel typing focus
                        self.state.request_cursor_pos = 0;
                        return Ok(false);
                    }
                    KeyCode::Enter => {
                        self.submit_active_request().await;
                        return Ok(false);
                    }
                    KeyCode::Backspace => {
                        if !self.state.request_text_input.is_empty() {
                            self.state.request_text_input.pop();
                        }
                        return Ok(false);
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.state.request_text_input.push(c);
                        return Ok(false);
                    }
                    _ => {}
                }
            }
        }

        // Global Navigation Shortcuts
        match key.code {
            KeyCode::Char('q') if self.state.current_screen != Screen::Requests || !self.is_typing_active() => {
                return Ok(true);
            }
            KeyCode::Char('?') => {
                if self.state.current_screen == Screen::Help {
                    self.state.go_back();
                } else {
                    self.state.navigate_to(Screen::Help);
                }
                return Ok(false);
            }
            KeyCode::Char('a') => {
                self.state.navigate_to(Screen::Agents);
                return Ok(false);
            }
            KeyCode::Char('r') => {
                self.state.navigate_to(Screen::Requests);
                return Ok(false);
            }
            KeyCode::Char('p') => {
                self.state.navigate_to(Screen::Projects);
                return Ok(false);
            }
            KeyCode::Char('d') => {
                self.state.navigate_to(Screen::Devices);
                return Ok(false);
            }
            KeyCode::Char('h') => {
                self.state.navigate_to(Screen::History);
                return Ok(false);
            }
            KeyCode::Char('w') | KeyCode::Char('t') => {
                self.state.navigate_to(Screen::Working);
                return Ok(false);
            }
            KeyCode::Char('e') => {
                self.state.navigate_to(Screen::Errors);
                return Ok(false);
            }
            KeyCode::Char('c') => {
                self.state.navigate_to(Screen::Connection);
                return Ok(false);
            }
            KeyCode::Char('l') => {
                self.state.navigate_to(Screen::Logs);
                return Ok(false);
            }
            KeyCode::Char('P') => {
                self.initiate_pairing(PairingMode::Relay);
                self.state.navigate_to(Screen::Pairing);
                return Ok(false);
            }
            KeyCode::Esc => {
                self.state.go_back();
                return Ok(false);
            }
            _ => {}
        }

        // Screen-specific navigation and interaction
        match self.state.current_screen {
            Screen::Dashboard => self.handle_dashboard_key(key).await,
            Screen::Requests => self.handle_requests_key(key).await,
            Screen::Agents => self.handle_agents_key(key),
            Screen::AgentDetail => self.handle_agent_detail_key(key),
            Screen::Errors => self.handle_errors_key(key),
            Screen::ErrorDetail => self.handle_error_detail_key(key).await,
            Screen::Working => self.handle_working_key(key),
            Screen::Completed => self.handle_completed_key(key),
            Screen::Projects => self.handle_projects_key(key),
            Screen::Devices => self.handle_devices_key(key),
            Screen::History => self.handle_history_key(key),
            Screen::Logs => self.handle_logs_key(key),
            Screen::Pairing => self.handle_pairing_key(key),
            _ => {}
        }

        Ok(false)
    }

    fn is_typing_active(&self) -> bool {
        if let Some((_, event)) = self.state.active_request() {
            if let Some(ref req) = event.request {
                if req.question_type == Some(QuestionType::FreeText) {
                    return true;
                }
                if let Some(idx) = self.state.request_selected_single {
                    if idx < req.options.len() && req.options[idx].eq_ignore_ascii_case("other") {
                        return true;
                    }
                }
            }
        }
        false
    }

    async fn handle_dashboard_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab => {
                self.state.dashboard_section = match self.state.dashboard_section {
                    DashboardSection::Attention => DashboardSection::Errors,
                    DashboardSection::Errors => DashboardSection::Working,
                    DashboardSection::Working => DashboardSection::Completed,
                    DashboardSection::Completed => DashboardSection::Attention,
                };
            }
            // Quick answer on dashboard if request is present
            KeyCode::Char('1') => {
                self.quick_respond_option(0).await;
            }
            KeyCode::Char('2') => {
                self.quick_respond_option(1).await;
            }
            KeyCode::Char('3') => {
                self.quick_respond_option(2).await;
            }
            KeyCode::Char('4') => {
                self.quick_respond_option(3).await;
            }
            KeyCode::Char('y') => {
                self.quick_respond_decision(Decision::Approve).await;
            }
            KeyCode::Char('n') => {
                self.quick_respond_decision(Decision::Deny).await;
            }
            KeyCode::Enter => {
                match self.state.dashboard_section {
                    DashboardSection::Attention => self.state.navigate_to(Screen::Requests),
                    DashboardSection::Errors => self.state.navigate_to(Screen::Errors),
                    DashboardSection::Working => self.state.navigate_to(Screen::Working),
                    DashboardSection::Completed => self.state.navigate_to(Screen::Completed),
                }
            }
            _ => {}
        }
    }

    async fn quick_respond_option(&mut self, opt_idx: usize) {
        if let Some((_, event)) = self.state.active_request() {
            if let Some(ref req) = event.request {
                if opt_idx < req.options.len() {
                    let opt = req.options[opt_idx].clone();
                    let resp = agentdesk_model::RespondRequest {
                        event_id: event.event_id,
                        decision: Decision::Approve,
                        selected_options: Some(vec![opt]),
                        text_input: None,
                    };
                    let _ = self.bridge.send_response(resp).await;
                    self.state.request_feedback = Some((format!("✓ Responded to {}", event.agent_name), false));
                }
            }
        }
    }

    async fn quick_respond_decision(&mut self, decision: Decision) {
        if let Some((_, event)) = self.state.active_request() {
            let resp = agentdesk_model::RespondRequest {
                event_id: event.event_id,
                decision,
                selected_options: None,
                text_input: None,
            };
            let _ = self.bridge.send_response(resp).await;
            let action = if decision == Decision::Approve { "Approved" } else { "Denied" };
            self.state.request_feedback = Some((format!("✓ {action} {}", event.agent_name), false));
        }
    }

    async fn handle_requests_key(&mut self, key: KeyEvent) {
        let (_entry, event) = match self.state.active_request() {
            Some(r) => r,
            None => return,
        };
        let req_info = match event.request {
            Some(ref r) => r,
            None => return,
        };

        let num_opts = req_info.options.len();

        match key.code {
            KeyCode::Up => {
                if self.state.request_option_cursor > 0 {
                    self.state.request_option_cursor -= 1;
                }
            }
            KeyCode::Down => {
                if num_opts > 0 && self.state.request_option_cursor + 1 < num_opts {
                    self.state.request_option_cursor += 1;
                }
            }
            KeyCode::Left => {
                self.state.request_approval_decision = Decision::Approve;
            }
            KeyCode::Right => {
                self.state.request_approval_decision = Decision::Deny;
            }
            KeyCode::Char('y') => {
                self.state.request_approval_decision = Decision::Approve;
            }
            KeyCode::Char('n') => {
                self.state.request_approval_decision = Decision::Deny;
            }
            KeyCode::Char(' ') => {
                // Toggle checkbox or radio
                let cur = self.state.request_option_cursor;
                if req_info.question_type == Some(QuestionType::MultipleChoice) {
                    if self.state.request_selected_multi.contains(&cur) {
                        self.state.request_selected_multi.remove(&cur);
                    } else {
                        self.state.request_selected_multi.insert(cur);
                    }
                } else {
                    self.state.request_selected_single = Some(cur);
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                let digit_idx = (c as usize) - ('1' as usize);
                if digit_idx < num_opts {
                    self.state.request_option_cursor = digit_idx;
                    self.state.request_selected_single = Some(digit_idx);
                }
            }
            KeyCode::Enter => {
                self.submit_active_request().await;
            }
            _ => {}
        }
    }

    async fn submit_active_request(&mut self) {
        if let Some(req) = self.state.build_respond_request() {
            let agent_name = self.state.active_request().map(|(_, e)| e.agent_name.clone()).unwrap_or_default();
            match self.bridge.send_response(req).await {
                Ok(()) => {
                    self.state.request_feedback = Some((format!("✓ Response delivered to {agent_name}"), false));
                    self.state.reset_request_interaction();
                }
                Err(e) => {
                    self.state.request_feedback = Some((format!("✗ Error: {e}"), true));
                }
            }
        }
    }

    fn handle_agents_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.state.agents_index > 0 {
                    self.state.agents_index -= 1;
                }
            }
            KeyCode::Down => {
                if !self.state.agents.is_empty() && self.state.agents_index + 1 < self.state.agents.len() {
                    self.state.agents_index += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(a) = self.state.agents.get(self.state.agents_index) {
                    self.state.selected_agent_id = Some(a.agent_id.clone());
                    self.state.navigate_to(Screen::AgentDetail);
                }
            }
            _ => {}
        }
    }

    fn handle_agent_detail_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.state.navigate_to(Screen::Agents);
        }
    }

    fn handle_errors_key(&mut self, key: KeyEvent) {
        let errs = self.state.errors();
        match key.code {
            KeyCode::Up => {
                if self.state.errors_index > 0 {
                    self.state.errors_index -= 1;
                }
            }
            KeyCode::Down => {
                if !errs.is_empty() && self.state.errors_index + 1 < errs.len() {
                    self.state.errors_index += 1;
                }
            }
            KeyCode::Enter => {
                if let Some((_, ev)) = self.state.active_error() {
                    self.state.selected_error_id = Some(ev.event_id);
                    self.state.error_disclosure_level = DisclosureLevel::Summary;
                    self.state.navigate_to(Screen::ErrorDetail);
                }
            }
            _ => {}
        }
    }

    async fn handle_error_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab => {
                self.state.error_disclosure_level = match self.state.error_disclosure_level {
                    DisclosureLevel::Summary => DisclosureLevel::Details,
                    DisclosureLevel::Details => DisclosureLevel::Logs,
                    DisclosureLevel::Logs => DisclosureLevel::Summary,
                };
                if self.state.error_disclosure_level == DisclosureLevel::Logs {
                    if let Some((_, ev)) = self.state.active_error() {
                        let _ = self.bridge.get_logs(ev.event_id, ev.log_range.start as i64, 50).await;
                    }
                }
            }
            KeyCode::Char('1') => self.state.error_disclosure_level = DisclosureLevel::Summary,
            KeyCode::Char('2') => self.state.error_disclosure_level = DisclosureLevel::Details,
            KeyCode::Char('3') => {
                self.state.error_disclosure_level = DisclosureLevel::Logs;
                if let Some((_, ev)) = self.state.active_error() {
                    let _ = self.bridge.get_logs(ev.event_id, ev.log_range.start as i64, 50).await;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.state.error_log_scroll += 1;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.state.error_log_scroll > 0 {
                    self.state.error_log_scroll -= 1;
                }
            }
            KeyCode::Char('d') => {
                if let Some((_, ev)) = self.state.active_error() {
                    let _ = self.bridge.dismiss(ev.event_id).await;
                    self.state.navigate_to(Screen::Errors);
                }
            }
            KeyCode::Esc => {
                self.state.navigate_to(Screen::Errors);
            }
            _ => {}
        }
    }

    fn handle_working_key(&mut self, key: KeyEvent) {
        let work = self.state.working();
        match key.code {
            KeyCode::Up => {
                if self.state.working_index > 0 {
                    self.state.working_index -= 1;
                }
            }
            KeyCode::Down => {
                if !work.is_empty() && self.state.working_index + 1 < work.len() {
                    self.state.working_index += 1;
                }
            }
            _ => {}
        }
    }

    fn handle_completed_key(&mut self, key: KeyEvent) {
        let comp = self.state.completed();
        match key.code {
            KeyCode::Up => {
                if self.state.completed_index > 0 {
                    self.state.completed_index -= 1;
                }
            }
            KeyCode::Down => {
                if !comp.is_empty() && self.state.completed_index + 1 < comp.len() {
                    self.state.completed_index += 1;
                }
            }
            _ => {}
        }
    }

    fn handle_projects_key(&mut self, key: KeyEvent) {
        let count = self.state.projects().len();
        match key.code {
            KeyCode::Up => {
                if self.state.projects_index > 0 {
                    self.state.projects_index -= 1;
                }
            }
            KeyCode::Down => {
                if count > 0 && self.state.projects_index + 1 < count {
                    self.state.projects_index += 1;
                }
            }
            _ => {}
        }
    }

    fn handle_devices_key(&mut self, key: KeyEvent) {
        let count = self.state.paired_devices.len();
        match key.code {
            KeyCode::Up => {
                if self.state.devices_index > 0 {
                    self.state.devices_index -= 1;
                }
            }
            KeyCode::Down => {
                if count > 0 && self.state.devices_index + 1 < count {
                    self.state.devices_index += 1;
                }
            }
            KeyCode::Char('r') => {
                if self.state.devices_index < self.state.paired_devices.len() {
                    let dev_id = self.state.paired_devices[self.state.devices_index].device_id.clone();
                    if let Ok(mut reg) = DeviceRegistry::load_or_create(&self.config_dir) {
                        if reg.revoke_device(&dev_id) {
                            let _ = reg.save(&self.config_dir);
                            self.state.refresh_devices(&self.config_dir);
                        }
                    }
                }
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                if self.state.devices_index < self.state.paired_devices.len() {
                    let dev_id = self.state.paired_devices[self.state.devices_index].device_id.clone();
                    if let Ok(mut reg) = DeviceRegistry::load_or_create(&self.config_dir) {
                        if reg.remove_device(&dev_id) {
                            let _ = reg.save(&self.config_dir);
                            self.state.refresh_devices(&self.config_dir);
                            if self.state.devices_index > 0 && self.state.devices_index >= self.state.paired_devices.len() {
                                self.state.devices_index = self.state.paired_devices.len().saturating_sub(1);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_history_key(&mut self, key: KeyEvent) {
        let count = self.state.history.len();
        match key.code {
            KeyCode::Up => {
                if self.state.history_index > 0 {
                    self.state.history_index -= 1;
                }
            }
            KeyCode::Down => {
                if count > 0 && self.state.history_index + 1 < count {
                    self.state.history_index += 1;
                }
            }
            _ => {}
        }
    }

    fn handle_logs_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.state.logs_scroll += 1;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.state.logs_scroll > 0 {
                    self.state.logs_scroll -= 1;
                }
            }
            _ => {}
        }
    }

    fn handle_pairing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('g') => {
                self.initiate_pairing(self.state.pairing_mode);
            }
            KeyCode::Esc => {
                self.state.go_back();
            }
            _ => {}
        }
    }

    fn initiate_pairing(&mut self, mode: PairingMode) {
        match self.bridge.generate_pairing_session(
            mode,
            &self.config_dir,
            self.state.daemon_port,
            self.state.relay_url.clone(),
            self.state.fingerprint.clone(),
        ) {
            Ok((sess_id, uri, qr, inst, exp, resolved_relay)) => {
                self.state.pairing_session_id = Some(sess_id);
                self.state.pairing_uri = Some(uri);
                self.state.pairing_qr = Some(qr);
                self.state.pairing_instance_id = Some(inst);
                self.state.pairing_expires_at = Some(exp);
                self.state.relay_url = Some(resolved_relay);
                self.state.pairing_paired_device = None;
                self.state.pairing_error = None;
            }
            Err(e) => {
                self.state.pairing_error = Some(e);
            }
        }
    }

    async fn launch_selected_agent(&mut self) {
        let Some(agent) = self.state.selected_agent().cloned() else {
            return;
        };

        self.state.active_agent = Some(agent.clone());
        self.state.agent_session_running = agent.is_ready();
        self.save_current_preferences();

        if self.state.shell_session.is_none() {
            if let Ok(mut session) = TerminalSession::spawn_shell(&self.state.workspace_dir, 120, 40) {
                if agent.is_ready() {
                    let cmd = if agent.id == "antigravity" || agent.executable == "agy" || agent.executable.ends_with("/agy") {
                        if self.state.dangerously_skip_permissions {
                            format!("{} --dangerously-skip-permissions\r", agent.executable)
                        } else {
                            format!("{}\r", agent.executable)
                        }
                    } else {
                        format!("{}\r", agent.executable)
                    };
                    let _ = session.send_input(cmd.as_bytes());
                }
                let _ = session.pump_output();
                self.state.shell_session = Some(session);
            }
        } else if agent.is_ready() {
            if let Some(ref mut session) = self.state.shell_session {
                let cmd = if agent.id == "antigravity" || agent.executable == "agy" || agent.executable.ends_with("/agy") {
                    if self.state.dangerously_skip_permissions {
                        format!("{} --dangerously-skip-permissions\r", agent.executable)
                    } else {
                        format!("{}\r", agent.executable)
                    }
                } else {
                    format!("{}\r", agent.executable)
                };
                let _ = session.send_input(cmd.as_bytes());
                let _ = session.pump_output();
            }
        }

        self.state.current_screen = Screen::Workspace;
    }

    fn save_current_preferences(&self) {
        let prefs = WorkspacePreferences {
            last_agent_id: self.state.active_agent.as_ref().map(|a| a.id.clone()),
            last_workspace: Some(self.state.workspace_dir.clone()),
            dangerously_skip_permissions: self.state.dangerously_skip_permissions,
            recent_workspaces: self.state.recent_workspaces.clone(),
        };
        let _ = save_preferences(&self.config_dir, &prefs);
    }

    async fn switch_to_agent(&mut self, agent: AgentDefinition) {
        self.state.active_agent = Some(agent.clone());
        self.state.agent_session_running = agent.is_ready();
        self.save_current_preferences();

        if agent.is_ready() {
            if let Some(ref mut session) = self.state.shell_session {
                let cmd = if agent.id == "antigravity" || agent.executable == "agy" || agent.executable.ends_with("/agy") {
                    if self.state.dangerously_skip_permissions {
                        format!("{} --dangerously-skip-permissions\r", agent.executable)
                    } else {
                        format!("{}\r", agent.executable)
                    }
                } else {
                    format!("{}\r", agent.executable)
                };
                let _ = session.send_input(cmd.as_bytes());
                let _ = session.pump_output();
            }
        }
    }

    /// Handle a prompt typed locally in the TUI prompt input bar.
    /// Mirrors the TuiEvent::PromptReceived logic without the workspace-dir update or screen switch.
    async fn handle_local_prompt(&mut self, prompt: String) {
        if self.state.is_duplicate_prompt(&prompt) {
            return;
        }
        // Ensure a shell session exists
        let need_spawn = match &self.state.shell_session {
            None => true,
            Some(s) => !s.is_alive(),
        };
        if need_spawn {
            if let Ok(mut session) = TerminalSession::spawn_shell(&self.state.workspace_dir, 120, 40) {
                let _ = session.pump_output();
                self.state.shell_session = Some(session);
            }
        }

        if let Some(ev) = self.state_machine.start_prompt(&prompt) {
            let _ = self.bridge.send_adapter_event(ev).await;
        }

        // Extract exe before the mutable shell_session borrow to satisfy the borrow checker
        let exe = self.state.selected_agent().map(|a| a.executable.clone()).unwrap_or_else(|| "agy".into());

        if let Some(ref mut session) = self.state.shell_session {
            let _ = session.pump_output();
            let snapshot = session.screen.snapshot();
            let live = agentdesk_core::detect_state(&snapshot);

            if live.accepts_chat_input() {
                let payload = if prompt.contains('\n') {
                    format!("\x1b[200~{prompt}\x1b[201~\r")
                } else {
                    format!("{prompt}\r")
                };
                let _ = session.send_input(payload.as_bytes());
                let _ = session.pump_output();
            } else {
                let escaped = prompt.replace('\'', "'\\''");
                let cmd = if exe == "agy" || exe.ends_with("/agy") {
                    if self.state.dangerously_skip_permissions {
                        format!("agy --dangerously-skip-permissions -i '{}'\r", escaped)
                    } else {
                        format!("agy -i '{}'\r", escaped)
                    }
                } else {
                    format!("{} '{}'\r", exe, escaped)
                };
                let _ = session.send_input(cmd.as_bytes());
                let _ = session.pump_output();
            }
        }

        self.state.agent_session_running = true;
    }
}
