use std::thread;
use std::time::{Duration, Instant};

use agentdesk_model::Decision;
use antigravity_pty_lab::{
    AntigravityState, AntigravityStateMachine, PtySession, Screen, encode_decision,
};

#[test]
fn test_live_pty_reverse_control_approval_and_denial() {
    // Spawn interactive bash script simulating Bubbletea confirmation menu
    let script = r#"
printf "\033[?1049h\033[H\033[2JCommand: git status\r\n\033[7mYes, run command\033[0m\r\nNo, deny\r\n"
read -rsn1 key1
if [ "$key1" = "" ]; then
    echo "RESULT:APPROVED"
elif [ "$key1" = $'\x1b' ]; then
    read -rsn2 -t 1 key2
    if [ "$key2" = "[B" ]; then
        read -rsn1 key3
        if [ "$key3" = "" ]; then
            echo "RESULT:DENIED"
        else
            echo "RESULT:UNKNOWN"
        fi
    else
        echo "RESULT:CANCELLED"
    fi
else
    echo "RESULT:UNKNOWN"
fi
"#;

    // Test 1: Test Approve (sends \r)
    let mut session_approve = PtySession::spawn(
        "/bin/bash",
        &["-c".to_string(), script.to_string()],
        None,
        80,
        24,
    )
    .expect("Failed to spawn PTY session");

    let mut screen = Screen::new(80, 24);
    let mut sm = AntigravityStateMachine::new("test-agent");
    let mut detected_state = None;

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        while let Ok(Some(chunk)) = session_approve.try_recv() {
            screen.process_bytes(&chunk.bytes);
            let snapshot = screen.snapshot();
            let _ = sm.update(&snapshot);
            if matches!(
                sm.current_state(),
                AntigravityState::CommandConfirmation { .. }
            ) {
                detected_state = Some(sm.current_state().clone());
                break;
            }
        }
        if detected_state.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    let state = detected_state.expect("Must detect CommandConfirmation in live PTY");
    let input = encode_decision(Decision::Approve, &state);
    session_approve
        .send_input(&input)
        .expect("Failed to send approval input");

    let exit_code = session_approve.wait_for_exit(Duration::from_secs(3));
    assert_eq!(exit_code, Some(0));

    // Verify script received approval
    while let Ok(Some(chunk)) = session_approve.try_recv() {
        screen.process_bytes(&chunk.bytes);
    }
    let full_text = screen.all_lines().join("\n");
    assert!(
        full_text.contains("RESULT:APPROVED"),
        "PTY child must have received approve keystroke: screen was\n{full_text}"
    );

    // Test 2: Test Deny (sends Down Arrow + \r)
    let mut session_deny = PtySession::spawn(
        "/bin/bash",
        &["-c".to_string(), script.to_string()],
        None,
        80,
        24,
    )
    .expect("Failed to spawn PTY session");

    let mut screen_deny = Screen::new(80, 24);
    let mut sm_deny = AntigravityStateMachine::new("test-agent");
    let mut detected_deny_state = None;

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        while let Ok(Some(chunk)) = session_deny.try_recv() {
            screen_deny.process_bytes(&chunk.bytes);
            let snapshot = screen_deny.snapshot();
            let _ = sm_deny.update(&snapshot);
            if matches!(
                sm_deny.current_state(),
                AntigravityState::CommandConfirmation { .. }
            ) {
                detected_deny_state = Some(sm_deny.current_state().clone());
                break;
            }
        }
        if detected_deny_state.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    let state_deny = detected_deny_state.expect("Must detect CommandConfirmation in live PTY");
    let deny_input = encode_decision(Decision::Deny, &state_deny);
    session_deny
        .send_input(&deny_input)
        .expect("Failed to send deny input");

    let exit_code_deny = session_deny.wait_for_exit(Duration::from_secs(3));
    assert_eq!(exit_code_deny, Some(0));

    while let Ok(Some(chunk)) = session_deny.try_recv() {
        screen_deny.process_bytes(&chunk.bytes);
    }
    let deny_text = screen_deny.all_lines().join("\n");
    assert!(
        deny_text.contains("RESULT:DENIED"),
        "PTY child must have received deny navigation keystroke: screen was\n{deny_text}"
    );
}
