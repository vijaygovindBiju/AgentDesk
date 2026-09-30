//! Deterministic tests for Antigravity adapter handling of `AntigravityState::AwaitingReply`.
//!
//! Covers the 6 required scenarios:
//! 1. AwaitingReply + TextInput("Rust") -> PTY receives Rust + Enter
//! 2. AwaitingReply + SelectOption("Rust") -> PTY receives Rust + Enter
//! 3. AwaitingReply + SelectOption("Python") -> PTY receives Python + Enter
//! 4. AwaitingReply + invalid SelectOption -> InvalidResponse -> zero PTY bytes
//! 5. AwaitingReply + unsupported response -> InvalidResponse -> zero PTY bytes
//! 6. Verify the selected option is never converted into an implicit Approve/deny decision.

use std::path::PathBuf;

use agentdesk_core::{
    Adapter, AdapterOutput, AntigravityConfig, AntigravityPtyAdapter, AntigravityState,
    MockPtyTransport, PtyRecording, QuestionInput, RespondError, encode_decision,
    encode_response_for_state, keys,
};
use agentdesk_model::{Decision, RequestResponse};
use chrono::Utc;

fn recording() -> PtyRecording {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/antigravity/real_agy_free_text_reply.json");
    PtyRecording::load_from_file(&path).expect("load real agy recording")
}

/// Helper to set up an adapter replayed up to the AwaitingReply request.
/// Returns (adapter, task_id, reply_agent_seq).
fn setup_awaiting_reply_adapter() -> (AntigravityPtyAdapter<MockPtyTransport>, String, u64) {
    let rec = recording();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(
        AntigravityConfig {
            cols: rec.initial_cols,
            rows: rec.initial_rows,
            ..Default::default()
        },
        MockPtyTransport::new(rec.initial_cols, rec.initial_rows),
    );
    let task = adapter.current_task_id().clone();
    let mut answered_trust = false;
    let mut reply_seq = None;

    for chunk in &rec.chunks {
        adapter.transport_mut().push_bytes(&chunk.bytes);
        let out = adapter.poll(Utc::now());
        for ev in out {
            if let AdapterOutput::Event(raw) = ev {
                if raw.kind == "input_required" {
                    if raw.details.get("interaction").and_then(|v| v.as_str())
                        == Some("workspace_trust")
                        && !answered_trust
                    {
                        adapter
                            .respond_with_response(
                                &task,
                                Some(raw.agent_seq),
                                &RequestResponse::SelectOption {
                                    option: "Yes, I trust this folder".into(),
                                },
                                Utc::now(),
                            )
                            .expect("trust selection delivered");
                        answered_trust = true;
                    } else if raw.details.get("interaction").and_then(|v| v.as_str())
                        == Some("agent_reply")
                    {
                        reply_seq = Some(raw.agent_seq);
                    }
                }
            }
        }
    }

    let seq = reply_seq.expect("AwaitingReply request event must be emitted");
    (adapter, task, seq)
}

#[test]
fn test_1_awaiting_reply_text_input_rust() {
    let (mut adapter, task, seq) = setup_awaiting_reply_adapter();
    let initial_inputs = adapter.transport_mut().sent_inputs.len();

    let res = adapter.respond_with_response(
        &task,
        Some(seq),
        &RequestResponse::TextInput {
            text: "Rust".into(),
        },
        Utc::now(),
    );
    assert!(res.is_ok(), "TextInput('Rust') should succeed: {res:?}");

    let inputs = &adapter.transport_mut().sent_inputs;
    assert_eq!(inputs.len(), initial_inputs + 1);
    assert_eq!(inputs.last().unwrap(), b"Rust\r");
}

#[test]
fn test_2_awaiting_reply_select_option_rust() {
    let (mut adapter, task, seq) = setup_awaiting_reply_adapter();
    let initial_inputs = adapter.transport_mut().sent_inputs.len();

    let res = adapter.respond_with_response(
        &task,
        Some(seq),
        &RequestResponse::SelectOption {
            option: "Rust".into(),
        },
        Utc::now(),
    );
    assert!(res.is_ok(), "SelectOption('Rust') should succeed: {res:?}");

    let inputs = &adapter.transport_mut().sent_inputs;
    assert_eq!(inputs.len(), initial_inputs + 1);
    assert_eq!(inputs.last().unwrap(), b"Rust\r");
}

#[test]
fn test_3_awaiting_reply_select_option_python() {
    let (mut adapter, task, seq) = setup_awaiting_reply_adapter();
    let initial_inputs = adapter.transport_mut().sent_inputs.len();

    let res = adapter.respond_with_response(
        &task,
        Some(seq),
        &RequestResponse::SelectOption {
            option: "Python".into(),
        },
        Utc::now(),
    );
    assert!(
        res.is_ok(),
        "SelectOption('Python') should succeed: {res:?}"
    );

    let inputs = &adapter.transport_mut().sent_inputs;
    assert_eq!(inputs.len(), initial_inputs + 1);
    assert_eq!(inputs.last().unwrap(), b"Python\r");
}

#[test]
fn test_4_awaiting_reply_invalid_select_option() {
    let invalid_options = [
        "",                     // empty
        "   ",                  // whitespace only
        "Rust\x1b[A",           // ANSI escape sequence
        "Rust\n",               // newline
        "Rust\r",               // carriage return
        "Rust\0",               // null byte
        "Rust\tcode",           // tab control character
        "Rust\x7f",             // delete character
    ];

    for bad in invalid_options {
        let (mut adapter, task, seq) = setup_awaiting_reply_adapter();
        let initial_inputs = adapter.transport_mut().sent_inputs.len();

        let res = adapter.respond_with_response(
            &task,
            Some(seq),
            &RequestResponse::SelectOption {
                option: bad.into(),
            },
            Utc::now(),
        );

        assert_eq!(
            res,
            Err(RespondError::InvalidResponse),
            "invalid option {bad:?} must be rejected with InvalidResponse"
        );
        assert_eq!(
            adapter.transport_mut().sent_inputs.len(),
            initial_inputs,
            "zero PTY bytes must be sent for invalid option {bad:?}"
        );
    }
}

#[test]
fn test_5_awaiting_reply_unsupported_response() {
    let unsupported = [
        RequestResponse::Approve,
        RequestResponse::Deny,
        RequestResponse::SelectMultiple {
            options: vec!["Rust".into(), "Python".into()],
        },
        RequestResponse::SelectMultiple {
            options: vec!["Rust".into()],
        },
    ];

    for resp in unsupported {
        let (mut adapter, task, seq) = setup_awaiting_reply_adapter();
        let initial_inputs = adapter.transport_mut().sent_inputs.len();

        let res = adapter.respond_with_response(&task, Some(seq), &resp, Utc::now());

        assert_eq!(
            res,
            Err(RespondError::InvalidResponse),
            "unsupported response {resp:?} must be rejected with InvalidResponse"
        );
        assert_eq!(
            adapter.transport_mut().sent_inputs.len(),
            initial_inputs,
            "zero PTY bytes must be sent for unsupported response {resp:?}"
        );
    }
}

#[test]
fn test_6_select_option_never_converts_to_implicit_approve_or_deny() {
    let (mut adapter, task, seq) = setup_awaiting_reply_adapter();
    let initial_inputs = adapter.transport_mut().sent_inputs.len();

    // 6a. Direct adapter delivery: SelectOption("Rust") sends Rust + Enter, NOT just Enter (\r)
    adapter
        .respond_with_response(
            &task,
            Some(seq),
            &RequestResponse::SelectOption {
                option: "Rust".into(),
            },
            Utc::now(),
        )
        .expect("SelectOption delivers");

    assert_eq!(adapter.transport_mut().sent_inputs.len(), initial_inputs + 1);
    let sent = adapter.transport_mut().sent_inputs.last().unwrap();
    assert_eq!(sent, b"Rust\r");
    assert_ne!(
        sent,
        keys::ENTER,
        "SelectOption('Rust') must never be sent as bare Enter (Approve)"
    );
    assert_ne!(
        sent,
        keys::ESC,
        "SelectOption('Rust') must never be sent as bare Esc (Deny)"
    );

    // 6b. encode_decision on AwaitingReply returns empty bytes (never bare Enter or Esc)
    let state = AntigravityState::AwaitingReply {
        message: "Which programming language would you like me to use?".into(),
    };
    assert_eq!(
        encode_decision(Decision::Approve, &state),
        Vec::<u8>::new(),
        "encode_decision(Approve) on AwaitingReply must never emit bare Enter"
    );
    assert_eq!(
        encode_decision(Decision::Deny, &state),
        Vec::<u8>::new(),
        "encode_decision(Deny) on AwaitingReply must never emit bare Esc"
    );

    // 6c. encode_response_for_state produces explicit text submission, not bare Enter
    let encoded = encode_response_for_state(
        &state,
        &RequestResponse::SelectOption {
            option: "Rust".into(),
        },
    )
    .expect("encoded");
    assert_eq!(encoded, QuestionInput::Submit(b"Rust\r".to_vec()));
    assert_ne!(encoded, QuestionInput::Submit(keys::ENTER.to_vec()));
}
