//! Central TUI state and data models.
//! Follows the AgentDesk priority hierarchy:
//! 1. REQUESTS / APPROVALS
//! 2. ERRORS
//! 3. COMPLETED
//! 4. WORKING

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};

use agentdesk_core::ClientId;
use agentdesk_model::{
    AgentId, AgentInfo, Category, Decision, Event, EventId, EventLogs,
    EventPush, LogLine, QuestionType, QueueEntry,
    Resolution, RespondRequest, ScoreUpdate, Snapshot, StateUpdate,
};

use crate::device::{DeviceRegistry, PairedDevice};
use crate::discovery::AgentDefinition;
use crate::status::{RuntimeStatus, load_runtime_status};
use crate::tui::preferences::WorkspacePreferences;
use crate::tui::terminal_session::TerminalSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    AgentSelection,
    Workspace,
    Dashboard,
    Agents,
    AgentDetail,
    Requests,
    Errors,
    ErrorDetail,
    Working,
    Completed,
    Projects,
    ProjectDetail,
    Devices,
    Connection,
    History,
    Logs,
    Pairing,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceOverlay {
    None,
    AgentSwitch,
    ProjectSelect,
    Attention,
    SwitchConfirm,
    Menu,
    Pairing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTerminalTab {
    Shell,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardSection {
    Attention,
    Errors,
    Working,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisclosureLevel {
    Summary = 1,
    Details = 2,
    Logs = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingMode {
    Relay,
}

pub struct TuiState {
    pub current_screen: Screen,
    pub previous_screen: Option<Screen>,
    pub dashboard_section: DashboardSection,
    pub dashboard_index: usize,

    // Live AgentDesk models
    pub entries: Vec<QueueEntry>,
    pub events: HashMap<EventId, Event>,
    pub history: Vec<Event>,
    pub agents: Vec<AgentInfo>,
    pub paired_devices: Vec<PairedDevice>,

    // Runtime & Connectivity
    pub runtime_status: Option<RuntimeStatus>,
    pub phone_connected: bool,
    pub phone_transport: Option<String>,
    pub phone_device_name: Option<String>,
    pub phone_last_seen: Option<DateTime<Utc>>,
    pub daemon_port: u16,
    pub daemon_bind: String,
    pub relay_url: Option<String>,
    pub fingerprint: Option<String>,
    pub client_id: ClientId,

    // Screen-specific navigation states
    pub agents_index: usize,
    pub selected_agent_id: Option<AgentId>,

    pub requests_index: usize,
    pub request_option_cursor: usize,
    pub request_selected_single: Option<usize>,
    pub request_selected_multi: HashSet<usize>,
    pub request_approval_decision: Decision,
    pub request_text_input: String,
    pub request_cursor_pos: usize,
    pub request_feedback: Option<(String, bool)>, // (message, is_error)

    pub errors_index: usize,
    pub selected_error_id: Option<EventId>,
    pub error_disclosure_level: DisclosureLevel,
    pub error_log_scroll: usize,

    pub working_index: usize,
    pub completed_index: usize,

    pub projects_index: usize,
    pub selected_project: Option<String>,

    pub devices_index: usize,
    pub history_index: usize,
    pub history_filter: Option<Category>,

    pub logs_index: usize,
    pub logs_lines: Vec<LogLine>,
    pub logs_scroll: usize,

    // Pairing flow state
    pub pairing_mode: PairingMode,
    pub pairing_session_id: Option<String>,
    pub pairing_qr: Option<String>,
    pub pairing_uri: Option<String>,
    pub pairing_instance_id: Option<String>,
    pub pairing_expires_at: Option<DateTime<Utc>>,
    pub pairing_paired_device: Option<(String, String)>,
    pub pairing_error: Option<String>,

    // Workspace & Coding-Agent selection state
    pub detected_agents: Vec<AgentDefinition>,
    pub selected_agent_index: usize,
    pub active_agent: Option<AgentDefinition>,
    pub workspace_dir: PathBuf,
    pub last_used_agent_id: Option<String>,
    pub active_terminal_tab: ActiveTerminalTab,
    pub workspace_overlay: WorkspaceOverlay,
    pub agent_switch_index: usize,
    pub project_input_text: String,
    pub project_input_cursor: usize,
    pub shell_session: Option<TerminalSession>,
    pub agent_session_running: bool,
    pub agent_switch_pending_index: Option<usize>,
    pub config_menu_index: usize,
}

impl Default for TuiState {
    fn default() -> Self {
        Self::new(8765, "127.0.0.1".into(), None, None)
    }
}

impl TuiState {
    pub fn new(
        port: u16,
        bind: String,
        relay_url: Option<String>,
        fingerprint: Option<String>,
    ) -> Self {
        Self {
            current_screen: Screen::Dashboard,
            previous_screen: None,
            dashboard_section: DashboardSection::Attention,
            dashboard_index: 0,

            entries: Vec::new(),
            events: HashMap::new(),
            history: Vec::new(),
            agents: Vec::new(),
            paired_devices: Vec::new(),

            runtime_status: None,
            phone_connected: false,
            phone_transport: None,
            phone_device_name: None,
            phone_last_seen: None,
            daemon_port: port,
            daemon_bind: bind,
            relay_url,
            fingerprint,
            client_id: 0,

            agents_index: 0,
            selected_agent_id: None,

            requests_index: 0,
            request_option_cursor: 0,
            request_selected_single: None,
            request_selected_multi: HashSet::new(),
            request_approval_decision: Decision::Approve,
            request_text_input: String::new(),
            request_cursor_pos: 0,
            request_feedback: None,

            errors_index: 0,
            selected_error_id: None,
            error_disclosure_level: DisclosureLevel::Summary,
            error_log_scroll: 0,

            working_index: 0,
            completed_index: 0,

            projects_index: 0,
            selected_project: None,

            devices_index: 0,
            history_index: 0,
            history_filter: None,

            logs_index: 0,
            logs_lines: Vec::new(),
            logs_scroll: 0,

            pairing_mode: PairingMode::Relay,
            pairing_session_id: None,
            pairing_qr: None,
            pairing_uri: None,
            pairing_instance_id: None,
            pairing_expires_at: None,
            pairing_paired_device: None,
            pairing_error: None,

            detected_agents: Vec::new(),
            selected_agent_index: 0,
            active_agent: None,
            workspace_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            last_used_agent_id: None,
            active_terminal_tab: ActiveTerminalTab::Shell,
            workspace_overlay: WorkspaceOverlay::None,
            agent_switch_index: 0,
            project_input_text: String::new(),
            project_input_cursor: 0,
            shell_session: None,
            agent_session_running: false,
            agent_switch_pending_index: None,
            config_menu_index: 0,
        }
    }

    /// Initialize state with discovered agents and preferences
    pub fn init_workspace_selection(&mut self, agents: Vec<AgentDefinition>, prefs: WorkspacePreferences) {
        self.current_screen = Screen::AgentSelection;
        self.detected_agents = agents;
        self.last_used_agent_id = prefs.last_agent_id.clone();
        if let Some(dir) = prefs.last_workspace {
            if dir.is_dir() {
                self.workspace_dir = dir;
            }
        }
        if let Some(last_id) = &self.last_used_agent_id {
            if let Some(idx) = self.detected_agents.iter().position(|a| &a.id == last_id) {
                self.selected_agent_index = idx;
            }
        }
    }

    pub fn select_next_agent(&mut self) {
        if !self.detected_agents.is_empty() {
            self.selected_agent_index = (self.selected_agent_index + 1) % self.detected_agents.len();
        }
    }

    pub fn select_previous_agent(&mut self) {
        if !self.detected_agents.is_empty() {
            if self.selected_agent_index == 0 {
                self.selected_agent_index = self.detected_agents.len() - 1;
            } else {
                self.selected_agent_index -= 1;
            }
        }
    }

    pub fn selected_agent(&self) -> Option<&AgentDefinition> {
        self.detected_agents.get(self.selected_agent_index)
    }

    pub fn select_next_switch_agent(&mut self) {
        if !self.detected_agents.is_empty() {
            self.agent_switch_index = (self.agent_switch_index + 1) % self.detected_agents.len();
        }
    }

    pub fn select_previous_switch_agent(&mut self) {
        if !self.detected_agents.is_empty() {
            if self.agent_switch_index == 0 {
                self.agent_switch_index = self.detected_agents.len() - 1;
            } else {
                self.agent_switch_index -= 1;
            }
        }
    }

    /// Reset structured interaction inputs when switching active request
    pub fn reset_request_interaction(&mut self) {
        self.request_option_cursor = 0;
        self.request_selected_single = None;
        self.request_selected_multi.clear();
        self.request_approval_decision = Decision::Approve;
        self.request_text_input.clear();
        self.request_cursor_pos = 0;
        self.request_feedback = None;

        // Pre-select first option if available
        if let Some((_, event)) = self.active_request() {
            if let Some(ref req) = event.request {
                if !req.options.is_empty() {
                    self.request_selected_single = Some(0);
                }
            }
        }
    }

    pub fn navigate_to(&mut self, screen: Screen) {
        if self.current_screen != screen {
            self.previous_screen = Some(self.current_screen);
            self.current_screen = screen;
            if screen == Screen::Requests {
                self.reset_request_interaction();
            }
        }
    }

    pub fn go_back(&mut self) {
        if let Some(prev) = self.previous_screen.take() {
            self.current_screen = prev;
        } else {
            self.current_screen = Screen::Dashboard;
        }
    }

    // --- State update receivers ---

    pub fn apply_snapshot(&mut self, snapshot: Snapshot) {
        self.entries = snapshot.entries;
        self.sort_entries();
        for event in snapshot.events {
            self.history.retain(|e| e.event_id != event.event_id);
            self.history.insert(0, event.clone());
            self.events.insert(event.event_id, event);
        }
        self.auto_discover_agents();
        self.reset_request_interaction();
    }

    pub fn apply_event(&mut self, push: EventPush) {
        // Update or insert entry
        if let Some(pos) = self.entries.iter().position(|e| e.event_id == push.entry.event_id) {
            self.entries[pos] = push.entry;
        } else {
            self.entries.push(push.entry);
        }
        self.sort_entries();

        // Update events map and history
        self.history.retain(|e| e.event_id != push.event.event_id);
        self.history.insert(0, push.event.clone());
        self.events.insert(push.event.event_id, push.event);

        self.auto_discover_agents();
    }

    pub fn apply_score_update(&mut self, update: ScoreUpdate) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.event_id == update.event_id) {
            entry.score = update.score;
            entry.escalation_level = update.escalation_level;
        }
        self.sort_entries();
    }

    pub fn apply_state_update(&mut self, update: StateUpdate) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.event_id == update.event_id) {
            entry.state = update.state;
            if let Some(res) = update.resolution {
                entry.resolution = Some(res);
            }
            if update.superseded {
                entry.superseded = true;
            }
        }
        self.sort_entries();
    }

    pub fn apply_logs(&mut self, logs: EventLogs) {
        self.logs_lines = logs.lines;
        self.logs_scroll = 0;
    }

    pub fn refresh_devices(&mut self, config_dir: &Path) {
        if let Ok(reg) = DeviceRegistry::load_or_create(config_dir) {
            self.paired_devices = reg.list_devices().into_iter().cloned().collect();
        }
    }

    pub fn refresh_runtime_status(&mut self, config_dir: &Path) {
        if let Some(status) = load_runtime_status(config_dir) {
            self.phone_connected = status.phone_connected;
            self.phone_transport = status.phone_transport.clone();
            self.phone_device_name = status.phone_device_name.clone();
            self.phone_last_seen = status.phone_last_seen;
            self.runtime_status = Some(status);
        }
    }

    fn sort_entries(&mut self) {
        self.entries.sort_by_key(|e| e.order_key());
    }

    fn auto_discover_agents(&mut self) {
        for event in self.events.values() {
            if !self.agents.iter().any(|a| a.agent_id == event.agent_id) {
                self.agents.push(AgentInfo {
                    agent_id: event.agent_id.clone(),
                    name: event.agent_name.clone(),
                    project: event.project.clone(),
                    adapter_kind: agentdesk_model::AdapterKind::Acp,
                });
            }
        }
        self.agents.sort_by(|a, b| a.name.cmp(&b.name));
    }

    // --- Query Helpers (Preserving AgentDesk Hierarchy) ---

    /// Priority 1: Requests needing attention
    pub fn requests(&self) -> Vec<(&QueueEntry, &Event)> {
        self.entries
            .iter()
            .filter(|e| e.tier == 0 && e.is_live() && e.resolution == Some(Resolution::Unresolved))
            .filter_map(|e| self.events.get(&e.event_id).map(|ev| (e, ev)))
            .collect()
    }

    /// Priority 2: Unresolved errors
    pub fn errors(&self) -> Vec<(&QueueEntry, &Event)> {
        self.entries
            .iter()
            .filter(|e| e.tier == 1 && e.is_live())
            .filter_map(|e| self.events.get(&e.event_id).map(|ev| (e, ev)))
            .collect()
    }

    /// Priority 3: Completed work
    pub fn completed(&self) -> Vec<(&QueueEntry, &Event)> {
        self.entries
            .iter()
            .filter(|e| e.tier == 2 && e.is_live())
            .filter_map(|e| self.events.get(&e.event_id).map(|ev| (e, ev)))
            .collect()
    }

    /// Priority 4: Working tasks (quiet, aggregated)
    pub fn working(&self) -> Vec<(&QueueEntry, &Event)> {
        self.entries
            .iter()
            .filter(|e| e.tier == 3 && e.is_live())
            .filter_map(|e| self.events.get(&e.event_id).map(|ev| (e, ev)))
            .collect()
    }

    pub fn active_request(&self) -> Option<(&QueueEntry, &Event)> {
        let reqs = self.requests();
        if reqs.is_empty() {
            None
        } else {
            let idx = self.requests_index.min(reqs.len() - 1);
            Some(reqs[idx])
        }
    }

    pub fn active_error(&self) -> Option<(&QueueEntry, &Event)> {
        let errs = self.errors();
        if errs.is_empty() {
            None
        } else {
            let idx = self.errors_index.min(errs.len() - 1);
            Some(errs[idx])
        }
    }

    pub fn projects(&self) -> BTreeMap<String, Vec<(&QueueEntry, &Event)>> {
        let mut map: BTreeMap<String, Vec<(&QueueEntry, &Event)>> = BTreeMap::new();
        for entry in &self.entries {
            if !entry.is_live() {
                continue;
            }
            if let Some(event) = self.events.get(&entry.event_id) {
                map.entry(event.project.clone())
                    .or_default()
                    .push((entry, event));
            }
        }
        map
    }

    pub fn active_agents_count(&self) -> usize {
        let working_agents: HashSet<&str> = self.working().iter().map(|(_, e)| e.agent_id.as_str()).collect();
        let request_agents: HashSet<&str> = self.requests().iter().map(|(_, e)| e.agent_id.as_str()).collect();
        working_agents.union(&request_agents).count().max(self.agents.len())
    }

    pub fn get_agent_status(&self, agent_id: &str) -> (Category, &'static str, Option<&Event>) {
        if let Some((_, ev)) = self.requests().into_iter().find(|(_, e)| e.agent_id == agent_id) {
            return (Category::Request, "Needs attention", Some(ev));
        }
        if let Some((_, ev)) = self.errors().into_iter().find(|(_, e)| e.agent_id == agent_id) {
            return (Category::Error, "Error", Some(ev));
        }
        if let Some((_, ev)) = self.working().into_iter().find(|(_, e)| e.agent_id == agent_id) {
            return (Category::Working, "Working", Some(ev));
        }
        if let Some((_, ev)) = self.completed().into_iter().find(|(_, e)| e.agent_id == agent_id) {
            return (Category::Completed, "Completed", Some(ev));
        }
        (Category::Working, "Idle", None)
    }

    pub fn get_agent_recent_events(&self, agent_id: &str) -> Vec<&Event> {
        self.history
            .iter()
            .filter(|e| e.agent_id == agent_id)
            .take(15)
            .collect()
    }

    /// Construct a RespondRequest from current user interaction state
    pub fn build_respond_request(&self) -> Option<RespondRequest> {
        let (entry, event) = self.active_request()?;
        let req_info = event.request.as_ref()?;

        let qtype = req_info.question_type.unwrap_or_else(|| {
            if req_info.options.is_empty() {
                QuestionType::YesNo
            } else if req_info.options.len() == 2
                && req_info.options[0].eq_ignore_ascii_case("yes")
                && req_info.options[1].eq_ignore_ascii_case("no")
            {
                QuestionType::YesNo
            } else {
                QuestionType::SingleChoice
            }
        });

        match qtype {
            QuestionType::YesNo => Some(RespondRequest {
                event_id: entry.event_id,
                decision: self.request_approval_decision,
                selected_options: None,
                text_input: None,
            }),
            QuestionType::SingleChoice => {
                if self.request_approval_decision == Decision::Deny {
                    Some(RespondRequest {
                        event_id: entry.event_id,
                        decision: Decision::Deny,
                        selected_options: None,
                        text_input: None,
                    })
                } else if let Some(idx) = self.request_selected_single {
                    if idx < req_info.options.len() {
                        let opt = req_info.options[idx].clone();
                        // Check if choice + write-in is used
                        let text = if opt.eq_ignore_ascii_case("other") && !self.request_text_input.trim().is_empty() {
                            Some(self.request_text_input.trim().to_string())
                        } else {
                            None
                        };
                        Some(RespondRequest {
                            event_id: entry.event_id,
                            decision: Decision::Approve,
                            selected_options: Some(vec![opt]),
                            text_input: text,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            QuestionType::MultipleChoice => {
                if self.request_approval_decision == Decision::Deny {
                    Some(RespondRequest {
                        event_id: entry.event_id,
                        decision: Decision::Deny,
                        selected_options: None,
                        text_input: None,
                    })
                } else {
                    let mut chosen = Vec::new();
                    for idx in &self.request_selected_multi {
                        if *idx < req_info.options.len() {
                            chosen.push(req_info.options[*idx].clone());
                        }
                    }
                    Some(RespondRequest {
                        event_id: entry.event_id,
                        decision: Decision::Approve,
                        selected_options: Some(chosen),
                        text_input: None,
                    })
                }
            }
            QuestionType::FreeText => {
                if self.request_approval_decision == Decision::Deny {
                    Some(RespondRequest {
                        event_id: entry.event_id,
                        decision: Decision::Deny,
                        selected_options: None,
                        text_input: None,
                    })
                } else {
                    Some(RespondRequest {
                        event_id: entry.event_id,
                        decision: Decision::Approve,
                        selected_options: None,
                        text_input: Some(self.request_text_input.clone()),
                    })
                }
            }
        }
    }
}
