//! Terminal application runner, event loop, and keyboard interaction.

use std::io::{self, stdout};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{
    Event as CrosstermEvent, EventStream, KeyCode, KeyEvent, KeyModifiers,
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
use crate::tui::state::{ActiveTerminalTab, DashboardSection, DisclosureLevel, PairingMode, Screen, TuiState, WorkspaceOverlay};
use crate::tui::terminal_session::TerminalSession;
use crate::tui::views::render_ui;

pub struct TuiApp {
    pub state: TuiState,
    pub bridge: TuiBridge,
    pub rx_events: mpsc::Receiver<TuiEvent>,
    pub config_dir: PathBuf,
    pub state_machine: agentdesk_core::AntigravityStateMachine,
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
            state_machine: agentdesk_core::AntigravityStateMachine::new("antigravity"),
        }
    }

    pub async fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        crate::logging::set_suppress_stderr(true);
        // Safe terminal setup
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Ensure panic restores terminal
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crate::logging::set_suppress_stderr(false);
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
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
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
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
                let term_rows = size.height.saturating_sub(2);
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
                        CrosstermEvent::Resize(cols, rows) => {
                            if let Some(ref mut session) = self.state.shell_session {
                                session.resize(cols, rows.saturating_sub(2));
                                session.pump_output();
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
            TuiEvent::PromptReceived(prompt) => {
                if self.state.shell_session.is_none() {
                    self.launch_selected_agent().await;
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
                if let Some(ref mut session) = self.state.shell_session {
                    let input = format!("{prompt}\r");
                    let _ = session.send_input(input.as_bytes());
                    let _ = session.pump_output();
                }
                self.inspect_shell_screen().await;
                self.state.current_screen = Screen::Workspace;
                self.state.workspace_overlay = WorkspaceOverlay::None;
            }
            TuiEvent::AdapterRespond { event_id, decision, response } => {
                let outcome = if let Some(ref mut session) = self.state.shell_session {
                    let snapshot = session.screen.snapshot();
                    let live = agentdesk_core::detect_state(&snapshot);
                    let input_result = if let Some(ref resp) = response {
                        match &live {
                            agentdesk_core::AntigravityState::UserQuestion { details, .. } => {
                                agentdesk_core::encode_question_response(resp, details)
                            }
                            _ => {
                                let dec = match resp {
                                    agentdesk_model::RequestResponse::Deny => agentdesk_model::Decision::Deny,
                                    _ => agentdesk_model::Decision::Approve,
                                };
                                Ok(agentdesk_core::QuestionInput::Submit(agentdesk_core::encode_decision(dec, &live)))
                            }
                        }
                    } else {
                        Ok(agentdesk_core::QuestionInput::Submit(agentdesk_core::encode_decision(decision, &live)))
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
            match key.code {
                KeyCode::Up => {
                    self.state.select_previous_agent();
                    return Ok(false);
                }
                KeyCode::Down => {
                    self.state.select_next_agent();
                    return Ok(false);
                }
                KeyCode::Enter => {
                    self.launch_selected_agent().await;
                    return Ok(false);
                }
                KeyCode::Char('p') | KeyCode::Tab => {
                    self.state.project_input_text = self.state.workspace_dir.to_string_lossy().to_string();
                    self.state.workspace_overlay = WorkspaceOverlay::ProjectSelect;
                    self.state.current_screen = Screen::Workspace;
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
                        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.state.project_input_text.push(c);
                        }
                        KeyCode::Backspace => {
                            self.state.project_input_text.pop();
                        }
                        KeyCode::Enter => {
                            let path = PathBuf::from(&self.state.project_input_text);
                            if path.is_dir() {
                                self.state.workspace_dir = path;
                                let prefs = WorkspacePreferences {
                                    last_agent_id: self.state.active_agent.as_ref().map(|a| a.id.clone()),
                                    last_workspace: Some(self.state.workspace_dir.clone()),
                                };
                                let _ = save_preferences(&self.config_dir, &prefs);
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
                                self.state.config_menu_index = 5;
                            } else {
                                self.state.config_menu_index -= 1;
                            }
                        }
                        KeyCode::Down => {
                            self.state.config_menu_index = (self.state.config_menu_index + 1) % 6;
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
                    _ => {
                        self.state.workspace_overlay = WorkspaceOverlay::None;
                    }
                }
                return Ok(false);
            }

            // In Workspace without overlay:
            // Check hotkeys
            if key.code == KeyCode::F(1) || (key.code == KeyCode::Char('a') && key.modifiers.contains(KeyModifiers::ALT)) {
                self.state.agent_switch_index = self.state.selected_agent_index;
                self.state.workspace_overlay = WorkspaceOverlay::AgentSwitch;
                return Ok(false);
            }
            if key.code == KeyCode::F(2) || (key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::ALT)) {
                self.state.project_input_text = self.state.workspace_dir.to_string_lossy().to_string();
                self.state.workspace_overlay = WorkspaceOverlay::ProjectSelect;
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
                self.state.active_terminal_tab = match self.state.active_terminal_tab {
                    ActiveTerminalTab::Shell => ActiveTerminalTab::Agent,
                    ActiveTerminalTab::Agent => ActiveTerminalTab::Shell,
                };
                return Ok(false);
            }
            if key.code == KeyCode::F(10) || (key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::ALT)) {
                return Ok(true);
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

        let prefs = WorkspacePreferences {
            last_agent_id: Some(agent.id.clone()),
            last_workspace: Some(self.state.workspace_dir.clone()),
        };
        let _ = save_preferences(&self.config_dir, &prefs);

        self.state.active_agent = Some(agent.clone());
        self.state.agent_session_running = agent.is_ready();

        if self.state.shell_session.is_none() {
            if let Ok(mut session) = TerminalSession::spawn_shell(&self.state.workspace_dir, 120, 40) {
                if agent.is_ready() {
                    let cmd = format!("{}\r", agent.executable);
                    let _ = session.send_input(cmd.as_bytes());
                }
                let _ = session.pump_output();
                self.state.shell_session = Some(session);
            }
        } else if agent.is_ready() {
            if let Some(ref mut session) = self.state.shell_session {
                let cmd = format!("{}\r", agent.executable);
                let _ = session.send_input(cmd.as_bytes());
                let _ = session.pump_output();
            }
        }

        self.state.current_screen = Screen::Workspace;
    }

    async fn switch_to_agent(&mut self, agent: AgentDefinition) {
        self.state.active_agent = Some(agent.clone());
        self.state.agent_session_running = agent.is_ready();
        let prefs = WorkspacePreferences {
            last_agent_id: Some(agent.id.clone()),
            last_workspace: Some(self.state.workspace_dir.clone()),
        };
        let _ = save_preferences(&self.config_dir, &prefs);

        if agent.is_ready() {
            if let Some(ref mut session) = self.state.shell_session {
                let cmd = format!("{}\r", agent.executable);
                let _ = session.send_input(cmd.as_bytes());
                let _ = session.pump_output();
            }
        }
    }
}
