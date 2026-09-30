//! Replay of a REAL Antigravity (`agy` 1.2.12) PTY recording captured during the physical
//! remote E2E test (`fixtures/antigravity/real_agy_free_text_reply.json`, account e-mail
//! redacted). The session shows:
//!
//! 1. The workspace-trust dialog (`> Yes, I trust this folder` / `  No, exit`).
//! 2. The user's prompt asking agy to ask which programming language to use.
//! 3. agy choosing NOT to use an `ask_question` menu and asking in plain chat, then focusing
//!    the chat input: a free-form interaction.
//!
//! Each state is verified through the real pipeline: PTY bytes -> VT screen -> semantic
//! state -> structured request -> response encoding against the live state.

use std::path::PathBuf;

use agentdesk_core::{
    Adapter, AdapterOutput, AntigravityConfig, AntigravityPtyAdapter, AntigravityState,
    MockPtyTransport, PtyRecording, RespondError, Screen, detect_state,
};
use agentdesk_model::{QuestionType, RawAgentEvent, RequestResponse};
use chrono::Utc;

fn recording() -> PtyRecording {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/antigravity/real_agy_free_text_reply.json");
    PtyRecording::load_from_file(&path).expect("load real agy recording")
}

/// Replay chunks into a VT screen, returning every distinct detected state in order.
fn replay_states(rec: &PtyRecording) -> Vec<AntigravityState> {
    let mut screen = Screen::new(rec.initial_cols, rec.initial_rows);
    let mut states: Vec<AntigravityState> = Vec::new();
    for chunk in &rec.chunks {
        screen.process_bytes(&chunk.bytes);
        let s = detect_state(&screen.snapshot());
        if states.last() != Some(&s) {
            states.push(s);
        }
    }
    states
}

fn requests(outputs: &[AdapterOutput]) -> Vec<RawAgentEvent> {
    outputs
        .iter()
        .filter_map(|o| match o {
            AdapterOutput::Event(raw) if raw.request.is_some() => Some(raw.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn real_workspace_trust_dialog_mirrors_exact_prompt_and_options() {
    let states = replay_states(&recording());
    let trust = states
        .iter()
        .find_map(|s| match s {
            AntigravityState::WorkspaceTrust {
                directory,
                question,
                options,
                selected_index,
            } => Some((directory.clone(), question.clone(), options.clone(), *selected_index)),
            _ => None,
        })
        .expect("real recording shows the workspace trust dialog");
    assert_eq!(trust.0, "/tmp/agentdesk-e2e-ws4");
    assert_eq!(trust.1, "Do you trust the contents of this project?");
    // Cursor marker stripped from the focused row.
    assert_eq!(trust.2, vec!["Yes, I trust this folder", "No, exit"]);
    assert_eq!(trust.3, 0);
}

#[test]
fn real_plain_chat_question_becomes_free_text_request_with_exact_prompt() {
    let states = replay_states(&recording());
    assert_eq!(
        states.last(),
        Some(&AntigravityState::AwaitingReply {
            message: "Which programming language would you like me to use?".into()
        }),
        "final real screen: agy asked in chat and focused its input; thoughts and the user's \
         prompt must not leak into the request. States: {states:#?}"
    );
    // Spinner animation frames must not each become a distinct state.
    let working = states
        .iter()
        .filter(|s| matches!(s, AntigravityState::Working { .. }))
        .count();
    assert!(working <= 6, "spinner redraws produced {working} working states");
}

#[test]
fn real_session_through_adapter_emits_structured_requests_not_approve_deny() {
    let rec = recording();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(
        AntigravityConfig { cols: rec.initial_cols, rows: rec.initial_rows, ..Default::default() },
        MockPtyTransport::new(rec.initial_cols, rec.initial_rows),
    );
    let task = adapter.current_task_id().clone();
    let mut all = Vec::new();
    let mut answered_trust = false;
    for chunk in &rec.chunks {
        adapter.transport_mut().push_bytes(&chunk.bytes);
        let out = adapter.poll(Utc::now());
        for req in requests(&out) {
            if req.details.get("interaction").and_then(|v| v.as_str()) == Some("workspace_trust")
                && !answered_trust
            {
                // Phone selects the exact option label; encoded against the live cursor.
                adapter
                    .respond_with_response(
                        &task,
                        Some(req.agent_seq),
                        &RequestResponse::SelectOption { option: "Yes, I trust this folder".into() },
                        Utc::now(),
                    )
                    .expect("trust selection delivered");
                answered_trust = true;
            }
        }
        all.extend(out);
    }

    let reqs = requests(&all);
    let trust = reqs
        .iter()
        .find(|r| r.details.get("interaction").and_then(|v| v.as_str()) == Some("workspace_trust"))
        .expect("trust request");
    let info = trust.request.as_ref().unwrap();
    assert_eq!(info.prompt, "Do you trust the contents of this project?");
    assert_eq!(info.options, vec!["Yes, I trust this folder", "No, exit"]);
    assert_eq!(info.question_type, Some(QuestionType::SingleChoice));
    assert!(!info.options.iter().any(|o| o == "approve" || o == "deny"));
    assert_eq!(adapter.transport_mut().sent_inputs[0], b"\r", "cursor already on the chosen row: Enter only");

    let reply = reqs.last().expect("free text request");
    let info = reply.request.as_ref().unwrap();
    assert_eq!(info.question_type, Some(QuestionType::FreeText));
    assert_eq!(info.prompt, "Which programming language would you like me to use?");
    assert!(info.options.is_empty());

    // Stale / wrong-shaped responses are rejected without keystrokes.
    let before = adapter.transport_mut().sent_inputs.len();
    assert_eq!(
        adapter.respond_with_response(
            &task,
            Some(reply.agent_seq),
            &RequestResponse::Approve,
            Utc::now()
        ),
        Err(RespondError::InvalidResponse)
    );
    assert_eq!(
        adapter.respond_with_response(
            &task,
            Some(reply.agent_seq),
            &RequestResponse::SelectOption { option: "".into() },
            Utc::now()
        ),
        Err(RespondError::InvalidResponse)
    );
    assert_eq!(
        adapter.respond_with_response(
            &task,
            Some(trust.agent_seq),
            &RequestResponse::TextInput { text: "Rust".into() },
            Utc::now()
        ),
        Err(RespondError::WrongRequest)
    );
    assert!(
        adapter
            .respond_with_response(
                &task,
                Some(reply.agent_seq),
                &RequestResponse::TextInput { text: "Rust\u{1b}[A".into() },
                Utc::now()
            )
            .is_err(),
        "control bytes must never reach the PTY"
    );
    assert!(
        adapter
            .respond_with_response(
                &task,
                Some(reply.agent_seq),
                &RequestResponse::SelectOption { option: "Rust\u{1b}[A".into() },
                Utc::now()
            )
            .is_err(),
        "control bytes in SelectOption must never reach the PTY"
    );
    assert_eq!(adapter.transport_mut().sent_inputs.len(), before);

    // Selected option is treated as text input and delivered with Enter.
    adapter
        .respond_with_response(
            &task,
            Some(reply.agent_seq),
            &RequestResponse::SelectOption { option: "Rust".into() },
            Utc::now(),
        )
        .expect("selected option delivered");
    assert_eq!(adapter.transport_mut().sent_inputs.last().unwrap(), b"Rust\r");
}
