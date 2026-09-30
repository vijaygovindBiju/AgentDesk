//! Command approval with MORE than two choices, from a REAL `agy` 1.2.12 recording captured in
//! the physical remote E2E test (`fixtures/antigravity/real_agy_command_approval.json`):
//!
//! ```text
//! Run this command?
//! > 1. Yes, run command
//!   2. Yes, and always allow in this conversation for commands that start with 'ls'
//!   3. Yes, and always allow for commands that start with 'ls' (Persist to settings.json)
//!   4. No, cancel
//! ```
//!
//! The request must carry agy's exact four labels (never synthetic approve/deny), and a phone
//! selection must become the minimal arrow navigation from the LIVE cursor row plus Enter.

use std::path::PathBuf;

use agentdesk_core::{
    Adapter, AdapterOutput, AntigravityConfig, AntigravityPtyAdapter, MockPtyTransport,
    PtyRecording, RespondError,
};
use agentdesk_model::{QuestionType, RawAgentEvent, RequestResponse};
use chrono::Utc;

const OPT1: &str = "Yes, run command";
const OPT2: &str = "Yes, and always allow in this conversation for commands that start with 'ls'";
const OPT3: &str =
    "Yes, and always allow for commands that start with 'ls' (Persist to settings.json)";
const OPT4: &str = "No, cancel";
const CMD: &str = "ls -la && which cargo rustc && cargo --version";

const DOWN: &[u8] = b"\x1b[B";
const UP: &[u8] = b"\x1b[A";

fn recording() -> PtyRecording {
    PtyRecording::load_from_file(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/antigravity/real_agy_command_approval.json"),
    )
    .expect("load real agy command approval recording")
}

/// Replay the real session until the command-approval request is emitted.
fn adapter_at_approval() -> (AntigravityPtyAdapter<MockPtyTransport>, RawAgentEvent) {
    let rec = recording();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(
        AntigravityConfig { cols: rec.initial_cols, rows: rec.initial_rows, ..Default::default() },
        MockPtyTransport::new(rec.initial_cols, rec.initial_rows),
    );
    let mut approval = None;
    for chunk in &rec.chunks {
        adapter.transport_mut().push_bytes(&chunk.bytes);
        for out in adapter.poll(Utc::now()) {
            if let AdapterOutput::Event(raw) = out
                && raw.details.get("interaction").and_then(|v| v.as_str()) == Some("command_approval")
            {
                approval = Some(raw);
            }
        }
    }
    (adapter, approval.expect("real recording raises the command approval"))
}

fn keys(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn respond(
    adapter: &mut AntigravityPtyAdapter<MockPtyTransport>,
    req: &RawAgentEvent,
    response: RequestResponse,
) -> Result<(), RespondError> {
    let task = adapter.current_task_id().clone();
    adapter.respond_with_response(&task, Some(req.agent_seq), &response, Utc::now())
}

#[test]
fn real_command_approval_request_preserves_all_four_exact_options() {
    let (_, req) = adapter_at_approval();
    let info = req.request.as_ref().unwrap();
    assert_eq!(req.kind, "approval_required");
    assert_eq!(info.question_type, Some(QuestionType::SingleChoice));
    assert_eq!(info.prompt, format!("Run this command?\n{CMD}"));
    assert_eq!(info.options, vec![OPT1, OPT2, OPT3, OPT4]);
    assert!(!info.options.iter().any(|o| o == "approve" || o == "deny"));
    assert_eq!(req.details["command"], CMD);
    assert_eq!(req.details["selected_option"], 0);
}

/// Each phone option maps to the corresponding agy row: live cursor is on row 1, so option N
/// needs exactly N-1 Down presses then Enter.
#[test]
fn each_of_the_four_options_selects_the_matching_agy_row() {
    let cases: [(&str, Vec<u8>); 4] = [
        (OPT1, keys(&[b"\r"])),
        (OPT2, keys(&[DOWN, b"\r"])),
        (OPT3, keys(&[DOWN, DOWN, b"\r"])),
        (OPT4, keys(&[DOWN, DOWN, DOWN, b"\r"])),
    ];
    for (option, expected) in cases {
        let (mut adapter, req) = adapter_at_approval();
        respond(&mut adapter, &req, RequestResponse::SelectOption { option: option.into() })
            .unwrap_or_else(|e| panic!("{option}: {e:?}"));
        assert_eq!(adapter.transport_mut().sent_inputs, vec![expected], "option {option}");
    }
}

/// Navigation is computed from the LIVE cursor, not assumed to start at row 1.
#[test]
fn navigation_is_relative_to_the_live_cursor_row() {
    let (mut adapter, req) = adapter_at_approval();
    // A user at the laptop moves agy's cursor to row 4 before the phone answers.
    let screen = adapter.screen().all_lines();
    let menu_top = screen.iter().position(|l| l.contains(OPT1)).unwrap();
    let mut redraw = String::new();
    for (i, label) in [OPT1, OPT2, OPT3, OPT4].iter().enumerate() {
        let marker = if i == 3 { ">" } else { " " };
        redraw.push_str(&format!("\x1b[{};1H\x1b[2K{marker} {}. {label}", menu_top + i + 1, i + 1));
    }
    adapter.transport_mut().push_text(&redraw);
    let _ = adapter.poll(Utc::now());

    respond(&mut adapter, &req, RequestResponse::SelectOption { option: OPT2.into() }).unwrap();
    assert_eq!(adapter.transport_mut().sent_inputs, vec![keys(&[UP, UP, b"\r"])]);
}

#[test]
fn invalid_and_stale_selections_send_no_keystrokes() {
    let (mut adapter, req) = adapter_at_approval();
    for bad in [
        RequestResponse::SelectOption { option: "approve".into() },
        RequestResponse::SelectOption { option: "Yes".into() },
        RequestResponse::SelectOption { option: "Rust".into() },
        RequestResponse::SelectMultiple { options: vec![OPT1.into(), OPT4.into()] },
        RequestResponse::TextInput { text: "1".into() },
    ] {
        assert_eq!(respond(&mut adapter, &req, bad.clone()), Err(RespondError::InvalidResponse), "{bad:?}");
    }
    // Wrong request id.
    let mut other = req.clone();
    other.agent_seq += 100;
    assert_eq!(
        respond(&mut adapter, &other, RequestResponse::SelectOption { option: OPT1.into() }),
        Err(RespondError::WrongRequest)
    );
    assert!(adapter.transport_mut().sent_inputs.is_empty());

    // agy moves on to a different dialog: the old selection is stale and rejected.
    adapter.transport_mut().push_text(
        "\x1b[2J\x1b[H> Create a small command-line program.\r\n\r\n\
         Run this command?\r\n  $ rm -rf target\r\n> 1. Yes, run command\r\n  2. No, cancel\r\n",
    );
    let _ = adapter.poll(Utc::now());
    assert!(respond(&mut adapter, &req, RequestResponse::SelectOption { option: OPT3.into() }).is_err());
    assert!(adapter.transport_mut().sent_inputs.is_empty());
}

#[test]
fn legacy_approve_deny_map_to_real_rows_by_label() {
    let (mut adapter, req) = adapter_at_approval();
    respond(&mut adapter, &req, RequestResponse::Deny).unwrap();
    assert_eq!(adapter.transport_mut().sent_inputs, vec![keys(&[DOWN, DOWN, DOWN, b"\r"])]);
}

/// Regression: a two-choice approval still shows exactly its two real rows.
#[test]
fn two_option_approval_keeps_its_two_real_rows() {
    let mut adapter = AntigravityPtyAdapter::new_with_transport(
        AntigravityConfig::default(),
        MockPtyTransport::new(120, 40),
    );
    adapter.transport_mut().push_text(
        "Requesting permission for:\r\n   cargo test\r\n\r\nRun this command?\r\n\
         > 1. Yes, run command\r\n  2. No, cancel\r\n",
    );
    let req = adapter
        .poll(Utc::now())
        .into_iter()
        .find_map(|o| match o {
            AdapterOutput::Event(raw) if raw.request.is_some() => Some(raw),
            _ => None,
        })
        .expect("two-option approval request");
    let info = req.request.as_ref().unwrap();
    assert_eq!(info.options, vec!["Yes, run command", "No, cancel"]);
    respond(&mut adapter, &req, RequestResponse::SelectOption { option: "No, cancel".into() }).unwrap();
    assert_eq!(adapter.transport_mut().sent_inputs, vec![keys(&[DOWN, b"\r"])]);
}

#[test]
fn historical_bash_commands_in_scrollback_do_not_override_permission_command() {
    let mut adapter = AntigravityPtyAdapter::new_with_transport(
        AntigravityConfig::default(),
        MockPtyTransport::new(120, 40),
    );
    // Simulate terminal scrollback with older Bash commands preceding the current permission request
    adapter.transport_mut().push_text(
        "● Bash(ls -la)\r\n\
         ● Read(/media/pirate/Shared/currently working/AgentDesk/core/Cargo.toml)\r\n\
         ● Bash(git status) (ctrl+o to expand)\r\n\r\n\
         Command\r\n\
         ────────────────────────────────────────────────────────────────────────────\r\n\r\n\
         Requesting permission for:\r\n\
            rustc --version\r\n\r\n\
         Run this command?\r\n\
         > 1. Yes, run command\r\n\
           2. Yes, and always allow in this conversation for commands that start with 'rustc'\r\n\
           3. Yes, and always allow for commands that start with 'rustc' (Persist to settings.json)\r\n\
           4. No, cancel\r\n"
    );
    let req = adapter
        .poll(Utc::now())
        .into_iter()
        .find_map(|o| match o {
            AdapterOutput::Event(raw) if raw.request.is_some() => Some(raw),
            _ => None,
        })
        .expect("permission request should be emitted");
    assert_eq!(req.details["command"], "rustc --version");
    let info = req.request.as_ref().unwrap();
    assert_eq!(info.prompt, "Run this command?\nrustc --version");
    assert_eq!(info.options.len(), 4);
    assert_eq!(info.options[0], "Yes, run command");
    assert_eq!(info.options[3], "No, cancel");
}

