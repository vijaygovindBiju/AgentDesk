use std::path::PathBuf;

use agentdesk_model::Operation;
use antigravity_pty_lab::{AntigravityState, PtyReplayer};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures/antigravity")
}

#[test]
fn test_fixture_command_confirmation() {
    let path = fixtures_dir().join("command_confirmation.json");
    let mut replayer = PtyReplayer::from_file(&path).expect("Failed to load fixture");
    let result = replayer.replay();

    let conf_state = result
        .states
        .iter()
        .find(|s| matches!(s, AntigravityState::CommandConfirmation { .. }));
    assert!(
        conf_state.is_some(),
        "Must detect CommandConfirmation state"
    );

    let event = result
        .events
        .iter()
        .find(|e| e.kind == "command_confirmation")
        .expect("Must emit command_confirmation event");

    assert_eq!(event.operation, Operation::Test);
    assert!(event.request.is_some());
    let req = event.request.as_ref().unwrap();
    assert_eq!(req.prompt, "cargo test --workspace");
    assert_eq!(req.options, vec!["approve", "deny"]);
}

#[test]
fn test_fixture_file_edit_confirmation() {
    let path = fixtures_dir().join("file_edit_confirmation.json");
    let mut replayer = PtyReplayer::from_file(&path).expect("Failed to load fixture");
    let result = replayer.replay();

    let conf_state = result
        .states
        .iter()
        .find(|s| matches!(s, AntigravityState::FileEditConfirmation { .. }));
    assert!(
        conf_state.is_some(),
        "Must detect FileEditConfirmation state"
    );

    let event = result
        .events
        .iter()
        .find(|e| e.kind == "file_edit_confirmation")
        .expect("Must emit file_edit_confirmation event");

    assert_eq!(event.operation, Operation::Edit);
    assert!(event.request.is_some());
}

#[test]
fn test_fixture_adversarial_chat_never_requests_approval() {
    let path = fixtures_dir().join("adversarial_chat.json");
    let mut replayer = PtyReplayer::from_file(&path).expect("Failed to load fixture");
    let result = replayer.replay();

    for event in &result.events {
        assert!(
            event.request.is_none(),
            "Adversarial chat fixture must NEVER emit a request: found {:?}",
            event
        );
        assert_ne!(
            event.kind, "command_confirmation",
            "Adversarial chat must not emit command_confirmation"
        );
        assert_ne!(
            event.kind, "file_edit_confirmation",
            "Adversarial chat must not emit file_edit_confirmation"
        );
    }
}

#[test]
fn test_fixture_user_question() {
    let path = fixtures_dir().join("user_question.json");
    let mut replayer = PtyReplayer::from_file(&path).expect("Failed to load fixture");
    let result = replayer.replay();

    let event = result
        .events
        .iter()
        .find(|e| e.kind == "question")
        .expect("Must emit question event");

    assert!(event.request.is_some());
    assert!(
        event
            .message
            .contains("Which authentication method would you like to configure?")
    );
}

#[test]
fn test_fixture_workspace_trust() {
    let path = fixtures_dir().join("workspace_trust.json");
    let mut replayer = PtyReplayer::from_file(&path).expect("Failed to load fixture");
    let result = replayer.replay();

    let event = result
        .events
        .iter()
        .find(|e| e.kind == "workspace_trust")
        .expect("Must emit workspace_trust event");

    assert!(event.request.is_some());
}
