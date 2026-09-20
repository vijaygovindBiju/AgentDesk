//! Keystroke and terminal input encoder for Antigravity PTY control.
//!
//! Translates high-level AgentDesk decisions (`Decision::Approve`, `Decision::Deny`, text input)
//! into exact terminal byte sequences (VT100 arrow navigation, Enter, Esc, Ctrl+C).

use agentdesk_model::{Decision, RequestResponse};

use crate::adapter::RespondError;
use crate::antigravity::state_machine::{AntigravityState, QuestionDetails};

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
    pub const BACKSPACE: &[u8] = b"\x7f";
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
            // Enter accepts the current selection / typed text; Esc skips the question.
            Decision::Approve => keys::ENTER.to_vec(),
            Decision::Deny => keys::ESC.to_vec(),
        },

        _ => match decision {
            Decision::Approve => keys::ENTER.to_vec(),
            Decision::Deny => keys::ESC.to_vec(),
        },
    }
}

/// Maximum accepted length (in characters) for a write-in answer.
pub const MAX_TEXT_INPUT_CHARS: usize = 4096;

/// Terminal input derived from a validated question response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionInput {
    /// Keystrokes that fully answer the question.
    Submit(Vec<u8>),
    /// Keystrokes that open the TUI's write-in text entry. `pending_text` must only be typed
    /// once the text-entry control has actually been observed on screen.
    OpenWriteIn {
        bytes: Vec<u8>,
        pending_text: String,
    },
}

/// Validate free text destined for the terminal: printable characters only. Control bytes
/// (including ESC, CR, LF, TAB, DEL) are rejected so phone-provided text can never encode
/// keystrokes or escape sequences.
pub fn validate_text_input(text: &str) -> Result<(), RespondError> {
    if text.is_empty() || text.chars().count() > MAX_TEXT_INPUT_CHARS {
        return Err(RespondError::InvalidResponse);
    }
    if text.chars().any(|c| c.is_control()) {
        return Err(RespondError::InvalidResponse);
    }
    Ok(())
}

/// Resolve an answer label to its raw menu row index (exact match, write-in row excluded).
fn resolve_option_index(label: &str, details: &QuestionDetails) -> Result<usize, RespondError> {
    let write_in = details.write_in_index();
    details
        .menu_rows()
        .iter()
        .enumerate()
        .find(|(i, row)| Some(*i) != write_in && row.trim() == label.trim())
        .map(|(i, _)| i)
        .ok_or(RespondError::InvalidResponse)
}

/// Encode a structured `RequestResponse` for an interactive question into deterministic PTY
/// keystrokes, navigating relative to the *live* cursor/toggle state in `details`.
pub fn encode_question_response(
    response: &RequestResponse,
    details: &QuestionDetails,
) -> Result<QuestionInput, RespondError> {
    match response {
        // Esc skips the question (or backs out of the write-in entry).
        RequestResponse::Deny => Ok(QuestionInput::Submit(keys::ESC.to_vec())),

        // Enter accepts the current selection / checked set / typed text.
        RequestResponse::Approve => match details {
            QuestionDetails::SingleChoice {
                selected_index,
                write_in_index,
                ..
            } if Some(*selected_index) == *write_in_index => Err(RespondError::InvalidResponse),
            _ => Ok(QuestionInput::Submit(keys::ENTER.to_vec())),
        },

        RequestResponse::SelectOption { option } => match details {
            QuestionDetails::SingleChoice { selected_index, .. } => {
                let target = resolve_option_index(option, details)?;
                Ok(QuestionInput::Submit(navigate_and_select(
                    *selected_index,
                    target,
                )))
            }
            QuestionDetails::MultipleChoice { .. } => {
                encode_multi_choice_selection(std::slice::from_ref(option), details)
            }
            QuestionDetails::FreeText { .. } => Err(RespondError::InvalidResponse),
        },

        RequestResponse::SelectMultiple { options } => match details {
            QuestionDetails::MultipleChoice { .. } => {
                encode_multi_choice_selection(options, details)
            }
            _ => Err(RespondError::InvalidResponse),
        },

        RequestResponse::TextInput { text } => {
            validate_text_input(text)?;
            match details {
                QuestionDetails::FreeText { current_text } => {
                    let mut bytes = Vec::new();
                    for _ in 0..current_text.chars().count() {
                        bytes.extend_from_slice(keys::BACKSPACE);
                    }
                    bytes.extend_from_slice(text.as_bytes());
                    bytes.extend_from_slice(keys::ENTER);
                    Ok(QuestionInput::Submit(bytes))
                }
                QuestionDetails::SingleChoice {
                    selected_index,
                    write_in_index: Some(w),
                    ..
                } => Ok(QuestionInput::OpenWriteIn {
                    bytes: navigate_and_select(*selected_index, *w),
                    pending_text: text.clone(),
                }),
                QuestionDetails::MultipleChoice {
                    cursor_index,
                    write_in_index: Some(w),
                    ..
                } => Ok(QuestionInput::OpenWriteIn {
                    bytes: navigate_and_select(*cursor_index, *w),
                    pending_text: text.clone(),
                }),
                _ => Err(RespondError::InvalidResponse),
            }
        }
    }
}

fn encode_multi_choice_selection(
    wanted: &[String],
    details: &QuestionDetails,
) -> Result<QuestionInput, RespondError> {
    let QuestionDetails::MultipleChoice {
        options,
        checked_indices,
        cursor_index,
        ..
    } = details
    else {
        return Err(RespondError::InvalidResponse);
    };
    if wanted.is_empty() {
        return Err(RespondError::InvalidResponse);
    }
    let mut targets = Vec::new();
    for label in wanted {
        let idx = resolve_option_index(label, details)?;
        if !targets.contains(&idx) {
            targets.push(idx);
        }
    }

    let mut bytes = Vec::new();
    let mut cursor = *cursor_index;
    for i in 0..options.len() {
        let want = targets.contains(&i);
        let have = checked_indices.contains(&i);
        if want != have {
            bytes.extend_from_slice(&navigate(cursor, i));
            cursor = i;
            bytes.extend_from_slice(keys::SPACE);
        }
    }
    bytes.extend_from_slice(keys::ENTER);
    Ok(QuestionInput::Submit(bytes))
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
            // Find "No, deny", "No, reject", or "No, cancel" option index if known
            let deny_index = options
                .iter()
                .position(|opt| {
                    opt.contains("No, deny")
                        || opt.contains("No, reject")
                        || opt.contains("No, cancel")
                        || opt.contains("cancel")
                })
                .unwrap_or(if options.len() > 1 {
                    options.len() - 1
                } else {
                    0
                });

            navigate_and_select(current_index, deny_index)
        }
    }
}

/// Generate arrow keys to move the menu cursor from `from_index` to `to_index`.
pub fn navigate(from_index: usize, to_index: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    if to_index > from_index {
        for _ in 0..(to_index - from_index) {
            bytes.extend_from_slice(keys::DOWN_ARROW);
        }
    } else {
        for _ in 0..(from_index - to_index) {
            bytes.extend_from_slice(keys::UP_ARROW);
        }
    }
    bytes
}

/// Generate arrow keys to move from `from_index` to `to_index`, then press Enter.
pub fn navigate_and_select(from_index: usize, to_index: usize) -> Vec<u8> {
    let mut bytes = navigate(from_index, to_index);
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
