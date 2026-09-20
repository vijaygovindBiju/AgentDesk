//! Keystroke and terminal input encoder for Antigravity PTY control.
//!
//! Translates high-level AgentDesk decisions (`Decision::Approve`, `Decision::Deny`, text input)
//! into exact terminal byte sequences (VT100 arrow navigation, Enter, Esc, Ctrl+C).

use agentdesk_model::Decision;

use crate::state_machine::AntigravityState;

/// VT100 keystroke sequences.
pub mod keys {
    pub const ENTER: &[u8] = b"\r";
    pub const ESC: &[u8] = b"\x1b";
    pub const CTRL_C: &[u8] = b"\x03";
    pub const UP_ARROW: &[u8] = b"\x1b[A";
    pub const DOWN_ARROW: &[u8] = b"\x1b[B";
    pub const RIGHT_ARROW: &[u8] = b"\x1b[C";
    pub const LEFT_ARROW: &[u8] = b"\x1b[D";
    pub const TAB: &[u8] = b"\t";
    pub const SPACE: &[u8] = b" ";
}

/// Encode an AgentDesk `Decision` for the current Antigravity interactive state.
pub fn encode_decision(decision: Decision, state: &AntigravityState) -> Vec<u8> {
    match state {
        AntigravityState::CommandConfirmation {
            selected_option_index,
            options,
            ..
        } => encode_confirmation_decision(decision, *selected_option_index, options),

        AntigravityState::FileEditConfirmation {
            selected_option_index,
            options,
            ..
        } => encode_confirmation_decision(decision, *selected_option_index, options),

        AntigravityState::WorkspaceTrust { .. } => match decision {
            Decision::Approve => {
                // First option is "Yes, I trust this folder"
                keys::ENTER.to_vec()
            }
            Decision::Deny => {
                // Down arrow to "No, exit", then Enter
                let mut bytes = Vec::new();
                bytes.extend_from_slice(keys::DOWN_ARROW);
                bytes.extend_from_slice(keys::ENTER);
                bytes
            }
        },

        AntigravityState::UserQuestion { .. } => match decision {
            Decision::Approve => keys::ENTER.to_vec(),
            Decision::Deny => keys::ESC.to_vec(),
        },

        _ => match decision {
            Decision::Approve => keys::ENTER.to_vec(),
            Decision::Deny => keys::ESC.to_vec(),
        },
    }
}

fn encode_confirmation_decision(
    decision: Decision,
    current_index: usize,
    options: &[String],
) -> Vec<u8> {
    match decision {
        Decision::Approve => {
            // Target option 0 ("Yes, run command" / "Yes, accept this change")
            navigate_and_select(current_index, 0)
        }
        Decision::Deny => {
            // Find "No, deny" or "No, reject" option index if known
            let deny_index = options
                .iter()
                .position(|opt| opt.contains("No, deny") || opt.contains("No, reject"))
                .unwrap_or(if options.len() > 1 { 1 } else { 0 });

            navigate_and_select(current_index, deny_index)
        }
    }
}

/// Generate arrow keys to move from `from_index` to `to_index`, then press Enter.
pub fn navigate_and_select(from_index: usize, to_index: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    if to_index > from_index {
        let diff = to_index - from_index;
        for _ in 0..diff {
            bytes.extend_from_slice(keys::DOWN_ARROW);
        }
    } else if from_index > to_index {
        let diff = from_index - to_index;
        for _ in 0..diff {
            bytes.extend_from_slice(keys::UP_ARROW);
        }
    }
    bytes.extend_from_slice(keys::ENTER);
    bytes
}

/// Encode free text entry (e.g. answering a prompt or question) followed by Enter.
pub fn encode_text_submission(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.extend_from_slice(keys::ENTER);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approve_when_already_at_top_option() {
        let state = AntigravityState::CommandConfirmation {
            command: "ls".into(),
            selected_option_index: 0,
            options: vec!["Yes, run command".into(), "No, deny".into()],
        };
        let input = encode_decision(Decision::Approve, &state);
        assert_eq!(input, keys::ENTER);
    }

    #[test]
    fn test_deny_navigation() {
        let state = AntigravityState::CommandConfirmation {
            command: "rm -rf /".into(),
            selected_option_index: 0,
            options: vec![
                "Yes, run command".into(),
                "Yes, always allow".into(),
                "No, deny".into(),
            ],
        };
        let input = encode_decision(Decision::Deny, &state);
        // Option 2 is "No, deny", so down arrow twice, then Enter
        let mut expected = Vec::new();
        expected.extend_from_slice(keys::DOWN_ARROW);
        expected.extend_from_slice(keys::DOWN_ARROW);
        expected.extend_from_slice(keys::ENTER);
        assert_eq!(input, expected);
    }

    #[test]
    fn test_workspace_trust_approval() {
        let state = AntigravityState::WorkspaceTrust {
            directory: "/repo".into(),
        };
        let input = encode_decision(Decision::Approve, &state);
        assert_eq!(input, keys::ENTER);
    }
}
