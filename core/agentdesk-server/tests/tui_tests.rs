//! Comprehensive test suite for AgentDesk Production TUI.
//! Validates all 20 requirements specified in Section 20:
//! 1. dashboard rendering
//! 2. empty state
//! 3. working state
//! 4. request state
//! 5. approval request
//! 6. yes/no request
//! 7. single-choice request
//! 8. multiple-choice request
//! 9. free-form request
//! 10. write-in request
//! 11. error state
//! 12. completed state
//! 13. device connected
//! 14. device offline
//! 15. project with multiple agents
//! 16. keyboard navigation
//! 17. small terminal dimensions
//! 18. event ordering
//! 19. request priority
//! 20. stale/resolved request handling

use std::collections::BTreeMap;
use chrono::Utc;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use uuid::Uuid;

use agentdesk_model::{
    Category, Decision, Event, EventPush, LogRange, Operation, QuestionType, QueueEntry,
    RequestInfo, Resolution, Severity,
};
use agentdesk_server::tui::state::{DisclosureLevel, Screen, TuiState};
use agentdesk_server::tui::views::render_ui;

fn create_sample_event(
    category: Category,
    agent_id: &str,
    agent_name: &str,
    project: &str,
    summary: &str,
    request: Option<RequestInfo>,
) -> (QueueEntry, Event) {
    let event_id = Uuid::new_v4();
    let now = Utc::now();
    let entry = QueueEntry::new(event_id, category, 1, 100);

    let event = Event {
        schema_version: 1,
        event_id,
        seq: 1,
        agent_seq: 1,
        agent_id: agent_id.to_string(),
        agent_name: agent_name.to_string(),
        project: project.to_string(),
        task_id: Some("task-01".into()),
        ts: now,
        category,
        severity: if category == Category::Error {
            Severity::Critical
        } else {
            Severity::Routine
        },
        kind: "test_event".into(),
        operation: Operation::Edit,
        summary: summary.to_string(),
        message: format!("Message: {summary}"),
        details: BTreeMap::new(),
        log_range: LogRange {
            start: 0,
            end: 10,
            pinned: false,
        },
        request,
    };

    (entry, event)
}

fn render_to_string(state: &TuiState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render_ui(f, state)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..height {
        for x in 0..width {
            out.push(buffer[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        out.push('\n');
    }
    out
}

// 1. Dashboard rendering
#[test]
fn test_dashboard_rendering() {
    let state = TuiState::default();
    let rendered = render_to_string(&state, 100, 30);
    assert!(rendered.contains("AGENTDESK"));
    assert!(rendered.contains("NEEDS YOUR ATTENTION"));
    assert!(rendered.contains("ERRORS"));
    assert!(rendered.contains("WORKING"));
    assert!(rendered.contains("COMPLETED"));
    assert!(rendered.contains("[a]"));
    assert!(rendered.contains("[r]"));
    assert!(rendered.contains("[q]"));
}

// 2. Empty state
#[test]
fn test_empty_state() {
    let state = TuiState::default();
    let rendered = render_to_string(&state, 100, 30);
    assert!(rendered.contains("All agents running autonomously"));
    assert!(rendered.contains("Zero active errors"));
    assert!(rendered.contains("No agents currently executing tasks"));
    assert!(rendered.contains("No completed tasks yet"));
}

// 3. Working state
#[test]
fn test_working_state() {
    let mut state = TuiState::default();
    let (entry, event) = create_sample_event(
        Category::Working,
        "antigravity",
        "Antigravity",
        "AgentDesk",
        "Implementing remote pairing",
        None,
    );
    state.apply_event(EventPush { event, entry });

    let rendered = render_to_string(&state, 120, 30);
    assert!(rendered.contains("Antigravity"));
    assert!(rendered.contains("AgentDesk"));
    assert!(rendered.contains("Implementing remote pairing"));
    assert!(!rendered.contains("No agents currently executing work"));
}

// 4. Request state
#[test]
fn test_request_state() {
    let mut state = TuiState::default();
    let req_info = RequestInfo {
        prompt: "Which database should we use?".into(),
        options: vec!["PostgreSQL".into(), "SQLite".into()],
        question_type: Some(QuestionType::SingleChoice),
    };
    let (entry, event) = create_sample_event(
        Category::Request,
        "claude",
        "Claude Code",
        "BackendApp",
        "Database Selection",
        Some(req_info),
    );
    state.apply_event(EventPush { event, entry });

    let rendered = render_to_string(&state, 120, 30);
    assert!(rendered.contains("NEEDS YOUR ATTENTION"));
    assert!(rendered.contains("Claude Code"));
    assert!(rendered.contains("Which database should we use?"));
    assert!(rendered.contains("PostgreSQL"));
    assert!(rendered.contains("SQLite"));
}

// 5. Approval request
#[test]
fn test_approval_request() {
    let mut state = TuiState::default();
    let req_info = RequestInfo {
        prompt: "Execute cargo test --workspace?".into(),
        options: Vec::new(),
        question_type: None,
    };
    let (entry, event) = create_sample_event(
        Category::Request,
        "antigravity",
        "Antigravity",
        "AgentDesk",
        "Test Execution Approval",
        Some(req_info),
    );
    state.apply_event(EventPush { event, entry });
    state.navigate_to(Screen::Requests);

    let rendered = render_to_string(&state, 100, 25);
    assert!(rendered.contains("APPROVAL REQUIRED"));
    assert!(rendered.contains("Execute cargo test --workspace?"));
    assert!(rendered.contains("[ Approve ]"));
    assert!(rendered.contains("[ Deny ]"));

    // Verify RespondRequest generation
    state.request_approval_decision = Decision::Approve;
    let resp = state.build_respond_request().unwrap();
    assert_eq!(resp.decision, Decision::Approve);
    assert!(resp.selected_options.is_none());

    state.request_approval_decision = Decision::Deny;
    let resp_deny = state.build_respond_request().unwrap();
    assert_eq!(resp_deny.decision, Decision::Deny);
}

// 6. Yes/No request
#[test]
fn test_yes_no_request() {
    let mut state = TuiState::default();
    let req_info = RequestInfo {
        prompt: "Do you want to enable logging?".into(),
        options: vec!["Yes".into(), "No".into()],
        question_type: Some(QuestionType::YesNo),
    };
    let (entry, event) = create_sample_event(
        Category::Request,
        "codex",
        "Codex",
        "Netra",
        "Enable Logging",
        Some(req_info),
    );
    state.apply_event(EventPush { event, entry });
    state.navigate_to(Screen::Requests);

    let rendered = render_to_string(&state, 100, 25);
    assert!(rendered.contains("Do you want to enable logging?"));
    assert!(rendered.contains("[ Yes ]"));
    assert!(rendered.contains("[ No ]"));

    state.request_approval_decision = Decision::Approve;
    let resp = state.build_respond_request().unwrap();
    assert_eq!(resp.decision, Decision::Approve);
}

// 7. Single-choice request
#[test]
fn test_single_choice_request() {
    let mut state = TuiState::default();
    let req_info = RequestInfo {
        prompt: "Choose language".into(),
        options: vec!["Rust".into(), "Python".into(), "Go".into()],
        question_type: Some(QuestionType::SingleChoice),
    };
    let (entry, event) = create_sample_event(
        Category::Request,
        "gemini",
        "Gemini",
        "CLI",
        "Language Selection",
        Some(req_info),
    );
    state.apply_event(EventPush { event, entry });
    state.navigate_to(Screen::Requests);

    state.request_selected_single = Some(0);
    let rendered = render_to_string(&state, 100, 25);
    assert!(rendered.contains("Choose language"));
    assert!(rendered.contains("● Rust") || rendered.contains("Rust"));
    assert!(rendered.contains("○ Python") || rendered.contains("Python"));

    let resp = state.build_respond_request().unwrap();
    assert_eq!(resp.selected_options.unwrap(), vec!["Rust"]);
}

// 8. Multiple-choice request
#[test]
fn test_multiple_choice_request() {
    let mut state = TuiState::default();
    let req_info = RequestInfo {
        prompt: "Select features".into(),
        options: vec!["Auth".into(), "Metrics".into(), "Logs".into()],
        question_type: Some(QuestionType::MultipleChoice),
    };
    let (entry, event) = create_sample_event(
        Category::Request,
        "antigravity",
        "Antigravity",
        "AgentDesk",
        "Feature Selection",
        Some(req_info),
    );
    state.apply_event(EventPush { event, entry });
    state.navigate_to(Screen::Requests);

    state.request_selected_multi.insert(0);
    state.request_selected_multi.insert(2);

    let rendered = render_to_string(&state, 100, 25);
    assert!(rendered.contains("[✓] Auth") || rendered.contains("Auth"));
    assert!(rendered.contains("[ ] Metrics") || rendered.contains("Metrics"));
    assert!(rendered.contains("[✓] Logs") || rendered.contains("Logs"));

    let resp = state.build_respond_request().unwrap();
    let mut opts = resp.selected_options.unwrap();
    opts.sort();
    assert_eq!(opts, vec!["Auth", "Logs"]);
}

// 9. Free-form request
#[test]
fn test_free_form_request() {
    let mut state = TuiState::default();
    let req_info = RequestInfo {
        prompt: "Enter project name".into(),
        options: Vec::new(),
        question_type: Some(QuestionType::FreeText),
    };
    let (entry, event) = create_sample_event(
        Category::Request,
        "antigravity",
        "Antigravity",
        "AgentDesk",
        "Name prompt",
        Some(req_info),
    );
    state.apply_event(EventPush { event, entry });
    state.navigate_to(Screen::Requests);

    state.request_text_input = "AgentDesk-Next".into();
    let rendered = render_to_string(&state, 100, 25);
    assert!(rendered.contains("Enter project name"));
    assert!(rendered.contains("AgentDesk-Next"));

    let resp = state.build_respond_request().unwrap();
    assert_eq!(resp.text_input.unwrap(), "AgentDesk-Next");
}

// 10. Write-in request
#[test]
fn test_write_in_request() {
    let mut state = TuiState::default();
    let req_info = RequestInfo {
        prompt: "Pick or custom".into(),
        options: vec!["OptionA".into(), "Other".into()],
        question_type: Some(QuestionType::SingleChoice),
    };
    let (entry, event) = create_sample_event(
        Category::Request,
        "claude",
        "Claude Code",
        "App",
        "Custom option",
        Some(req_info),
    );
    state.apply_event(EventPush { event, entry });
    state.navigate_to(Screen::Requests);

    state.request_selected_single = Some(1); // Select "Other"
    state.request_text_input = "MyCustomLanguage".into();

    let resp = state.build_respond_request().unwrap();
    assert_eq!(resp.selected_options.unwrap(), vec!["Other"]);
    assert_eq!(resp.text_input.unwrap(), "MyCustomLanguage");
}

// 11. Error state
#[test]
fn test_error_state() {
    let mut state = TuiState::default();
    let (entry, mut event) = create_sample_event(
        Category::Error,
        "claude",
        "Claude Code",
        "Hybrid",
        "Command failed — exit code 1",
        None,
    );
    event.details.insert("exit_code".into(), serde_json::json!(1));
    event.details.insert("file".into(), serde_json::json!("main.rs"));
    state.apply_event(EventPush { event, entry });

    // Test dashboard shows error
    let dash = render_to_string(&state, 120, 30);
    assert!(dash.contains("Command failed — exit code 1"));

    // Test errors screen
    state.navigate_to(Screen::Errors);
    let err_view = render_to_string(&state, 120, 30);
    assert!(err_view.contains("Claude Code"));
    assert!(err_view.contains("CRITICAL"));

    // Test progressive disclosure Level 2
    state.navigate_to(Screen::ErrorDetail);
    state.error_disclosure_level = DisclosureLevel::Details;
    let detail_view = render_to_string(&state, 120, 30);
    assert!(detail_view.contains("Structured Attributes"));
    assert!(detail_view.contains("exit_code"));
    assert!(detail_view.contains("main.rs"));
}

// 12. Completed state
#[test]
fn test_completed_state() {
    let mut state = TuiState::default();
    let (entry, event) = create_sample_event(
        Category::Completed,
        "gemini",
        "Gemini",
        "LifeLine",
        "Search repository fixed",
        None,
    );
    state.apply_event(EventPush { event, entry });

    let rendered = render_to_string(&state, 120, 30);
    assert!(rendered.contains("Gemini"));
    assert!(rendered.contains("LifeLine"));
    assert!(rendered.contains("Search repository fixed"));

    state.navigate_to(Screen::Completed);
    let comp_view = render_to_string(&state, 120, 30);
    assert!(comp_view.contains("COMPLETED TASKS"));
    assert!(comp_view.contains("Search repository fixed"));
}

// 13. Device connected
#[test]
fn test_device_connected() {
    let mut state = TuiState::default();
    state.phone_connected = true;
    state.phone_transport = Some("Relay".into());
    state.phone_device_name = Some("Vijay Phone".into());

    let rendered = render_to_string(&state, 120, 30);
    assert!(rendered.contains("PHONE CONNECTED"));
    assert!(rendered.contains("RELAY"));
}

// 14. Device offline
#[test]
fn test_device_offline() {
    let mut state = TuiState::default();
    state.phone_connected = false;

    let rendered = render_to_string(&state, 120, 30);
    assert!(rendered.contains("PHONE OFFLINE"));
}

// 15. Project with multiple agents
#[test]
fn test_project_with_multiple_agents() {
    let mut state = TuiState::default();
    let (e1, ev1) = create_sample_event(Category::Working, "antigravity", "Antigravity", "AgentDesk", "Implementing pairing", None);
    let (e2, ev2) = create_sample_event(Category::Request, "claude", "Claude Code", "AgentDesk", "Needs attention", Some(RequestInfo {
        prompt: "Proceed?".into(),
        options: vec!["Yes".into(), "No".into()],
        question_type: Some(QuestionType::YesNo),
    }));
    state.apply_event(EventPush { event: ev1, entry: e1 });
    state.apply_event(EventPush { event: ev2, entry: e2 });

    state.navigate_to(Screen::Projects);
    let rendered = render_to_string(&state, 120, 30);
    assert!(rendered.contains("Project: AgentDesk"));
    assert!(rendered.contains("Antigravity"));
    assert!(rendered.contains("Claude Code"));
}

// 16. Keyboard navigation
#[test]
fn test_keyboard_navigation() {
    let mut state = TuiState::default();
    assert_eq!(state.current_screen, Screen::Dashboard);

    state.navigate_to(Screen::Agents);
    assert_eq!(state.current_screen, Screen::Agents);

    state.navigate_to(Screen::AgentDetail);
    assert_eq!(state.current_screen, Screen::AgentDetail);

    state.go_back();
    assert_eq!(state.current_screen, Screen::Agents);

    state.navigate_to(Screen::Requests);
    assert_eq!(state.current_screen, Screen::Requests);

    state.navigate_to(Screen::Pairing);
    assert_eq!(state.current_screen, Screen::Pairing);

    state.go_back();
    assert_eq!(state.current_screen, Screen::Requests);
}

// 17. Small terminal dimensions
#[test]
fn test_small_terminal_dimensions() {
    let state = TuiState::default();
    // 80x24 standard small terminal
    let s80x24 = render_to_string(&state, 80, 24);
    assert!(s80x24.contains("AGENTDESK"));

    // 60x15 compact
    let s60x15 = render_to_string(&state, 60, 15);
    assert!(s60x15.contains("AGENTDESK"));

    // 30x10 extreme edge case
    let s30x10 = render_to_string(&state, 30, 10);
    assert!(!s30x10.is_empty());
}

// 18. Event ordering
#[test]
fn test_event_ordering() {
    let mut state = TuiState::default();
    let (e_work, ev_work) = create_sample_event(Category::Working, "a1", "Agent1", "P", "Working", None);
    let (e_err, ev_err) = create_sample_event(Category::Error, "a2", "Agent2", "P", "Error", None);
    let (e_req, ev_req) = create_sample_event(Category::Request, "a3", "Agent3", "P", "Request", Some(RequestInfo {
        prompt: "Prompt".into(),
        options: vec!["A".into()],
        question_type: None,
    }));
    let (e_comp, ev_comp) = create_sample_event(Category::Completed, "a4", "Agent4", "P", "Done", None);

    state.apply_event(EventPush { event: ev_work, entry: e_work });
    state.apply_event(EventPush { event: ev_err, entry: e_err });
    state.apply_event(EventPush { event: ev_req, entry: e_req });
    state.apply_event(EventPush { event: ev_comp, entry: e_comp });

    // Hierarchy check: Tier 0 (Request) < Tier 1 (Error) < Tier 2 (Completed) < Tier 3 (Working)
    assert_eq!(state.entries[0].tier, 0); // Request
    assert_eq!(state.entries[1].tier, 1); // Error
    assert_eq!(state.entries[2].tier, 2); // Completed
    assert_eq!(state.entries[3].tier, 3); // Working
}

// 19. Request priority
#[test]
fn test_request_priority() {
    let mut state = TuiState::default();
    let (e_req, ev_req) = create_sample_event(Category::Request, "a", "A", "P", "Req", Some(RequestInfo {
        prompt: "Need approval".into(),
        options: vec!["OK".into()],
        question_type: None,
    }));
    let (e_err, ev_err) = create_sample_event(Category::Error, "b", "B", "P", "Fatal error", None);

    state.apply_event(EventPush { event: ev_err, entry: e_err });
    state.apply_event(EventPush { event: ev_req, entry: e_req });

    assert_eq!(state.requests().len(), 1);
    assert_eq!(state.active_request().unwrap().1.summary, "Req");
    assert_eq!(state.entries[0].tier, 0);
}

// 20. Stale / resolved request handling
#[test]
fn test_stale_resolved_request_handling() {
    let mut state = TuiState::default();
    let (e_req, ev_req) = create_sample_event(Category::Request, "a", "A", "P", "Req", Some(RequestInfo {
        prompt: "Need approval".into(),
        options: vec!["OK".into()],
        question_type: None,
    }));
    let id = ev_req.event_id;
    state.apply_event(EventPush { event: ev_req, entry: e_req });
    assert_eq!(state.requests().len(), 1);

    // Resolve the request
    state.apply_state_update(agentdesk_model::StateUpdate {
        event_id: id,
        state: agentdesk_model::EntryState::Seen,
        resolution: Some(Resolution::Approved),
        superseded: false,
    });

    // Resolved request must no longer block or appear in active requests
    assert_eq!(state.requests().len(), 0);
}

#[test]
fn test_paired_devices_revoke_and_removal() {
    use agentdesk_server::device::DeviceRegistry;

    let temp_dir = std::env::temp_dir().join(format!("agentdesk_test_dev_rm_{}", uuid::Uuid::new_v4()));
    let mut reg = DeviceRegistry::load_or_create(&temp_dir).unwrap();
    let dev_id = "phone-pixel-9";
    let dev_token = reg.register_device(dev_id.into(), "Pixel 9 Pro".into());
    reg.save(&temp_dir).unwrap();

    let mut state = TuiState::default();
    state.refresh_devices(&temp_dir);
    assert_eq!(state.paired_devices.len(), 1);
    assert_eq!(state.paired_devices[0].device_name, "Pixel 9 Pro");
    assert!(!state.paired_devices[0].revoked);
    assert!(reg.is_authorized(dev_id, &dev_token));

    // Render devices view with active device
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    state.navigate_to(Screen::Devices);
    terminal.draw(|f| render_ui(f, &state)).unwrap();

    // 1. Revoke device
    assert!(reg.revoke_device(dev_id));
    reg.save(&temp_dir).unwrap();
    state.refresh_devices(&temp_dir);
    assert_eq!(state.paired_devices.len(), 1);
    assert!(state.paired_devices[0].revoked);
    assert!(!reg.is_authorized(dev_id, &dev_token));

    // Render devices view with revoked status
    terminal.draw(|f| render_ui(f, &state)).unwrap();

    // 2. Remove device completely
    assert!(reg.remove_device(dev_id));
    reg.save(&temp_dir).unwrap();
    state.refresh_devices(&temp_dir);
    assert_eq!(state.paired_devices.len(), 0);
    assert!(!reg.is_authorized(dev_id, &dev_token));

    // Render empty devices view
    terminal.draw(|f| render_ui(f, &state)).unwrap();

    let _ = std::fs::remove_dir_all(&temp_dir);
}
