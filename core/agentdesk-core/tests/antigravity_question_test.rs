//! Interactive question (`ask_question`) vertical slice tests for `AntigravityPtyAdapter`.
//!
//! The screen layouts used here were captured from the real `agy` 1.2.7 binary inside a PTY
//! (see `fixtures/antigravity/real_agy_question_*.json`). The parser is treated as a
//! security-sensitive component: question-like text is never enough, an actual TUI control
//! (form header + numbered menu + key legend, write-in text entry, or a radio form with a live
//! selection marker) must be on screen.

use agentdesk_core::{
    Adapter, AdapterOutput, AntigravityConfig, AntigravityLifecycle, AntigravityPtyAdapter,
    AntigravityState, MockPtyTransport, QuestionDetails, QuestionForm, QuestionInput, RespondError,
    Screen, detect_state, encode_question_response, validate_text_input,
};
use agentdesk_model::{Decision, QuestionType, RawAgentEvent, RequestResponse};
use chrono::Utc;

// ---------------------------------------------------------------------------------------------
// Real agy layouts (text as rendered on the VT100 grid, `\r\n` separated).
// ---------------------------------------------------------------------------------------------

const REAL_SINGLE: &str = concat!(
    "● AskQuestion(Which database should we use?)\r\n",
    "⣾  Working...\r\n",
    "? Which database should we use?\r\n",
    "Question\r\n",
    "────────────────────────────────────────────────────────────\r\n",
    "Question 1/1: Which database should we use?\r\n",
    "> 1. PostgreSQL\r\n",
    "  2. SQLite\r\n",
    "  3. MySQL\r\n",
    "  4. Write-in...\r\n",
    "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
    "esc to cancel                                             Gemini 3.8 Flash · low\r\n",
);

/// Multi-select after the user toggled rows 1 and 3. Note the stale `> 1. [ ] Authentication`
/// residue row above the live block, exactly as the real redraw left it on the grid.
const REAL_MULTI_TOGGLED: &str = concat!(
    "? Which features should be enabled?\r\n",
    "Question\r\n",
    "────────────────────────────────────────────────────────────\r\n",
    "Question 1/1: Which features should be enabled?\r\n",
    "> 1. [ ] Authentication\r\n",
    "  1. [x] Authentication\r\n",
    "  2. [ ] Database\r\n",
    "> 3. [x] Logging\r\n",
    "  ↑/↓ Navigate · space Toggle · enter Submit · esc Skip\r\n",
    "esc to cancel                                             Gemini 3.8 Flash · low\r\n",
);

const REAL_MULTI_WITH_WRITE_IN: &str = concat!(
    "Question 1/1: Which features should be enabled?\r\n",
    "> 1. [ ] Authentication\r\n",
    "  2. [ ] Database\r\n",
    "  3. [ ] Logging\r\n",
    "  4. Write-in...\r\n",
    "  ↑/↓ Navigate · space Toggle · enter Submit · esc Skip\r\n",
    "esc to cancel                                             Gemini 3.8 Flash · low\r\n",
);

/// Write-in text entry after selecting `Write-in...` and typing `my_fi` (the menu legend is
/// replaced by the entry control; the cursor stays parked on the `Write-in...` row).
const REAL_WRITE_IN_ENTRY: &str = concat!(
    "Question 1/1: What should the filename be?\r\n",
    "\r\n",
    "  1. main.rs\r\n",
    "  2. lib.rs\r\n",
    "  3. app.rs\r\n",
    "> 4. Write-in...\r\n",
    "\r\n",
    "  Your answer:\r\n",
    "  my_fi\r\n",
    "\r\n",
    "\r\n",
    "\r\n",
    "  enter Submit · esc Back\r\n",
    "esc to cancel                                             Gemini 3.8 Flash · low\r\n",
);

fn screen_of(text: &str) -> Screen {
    let mut screen = Screen::new(120, 40);
    screen.process_bytes(text.as_bytes());
    screen
}

fn adapter_with(text: &str) -> AntigravityPtyAdapter<MockPtyTransport> {
    let mut transport = MockPtyTransport::new(120, 40);
    transport.push_text(text);
    AntigravityPtyAdapter::new_with_transport(AntigravityConfig::default(), transport)
}

fn request_events(outputs: &[AdapterOutput]) -> Vec<RawAgentEvent> {
    outputs
        .iter()
        .filter_map(|o| match o {
            AdapterOutput::Event(raw) if raw.request.is_some() => Some(raw.clone()),
            _ => None,
        })
        .collect()
}

fn question_of(state: &AntigravityState) -> (&str, Option<QuestionForm>, &QuestionDetails) {
    match state {
        AntigravityState::UserQuestion {
            question,
            form,
            details,
        } => (question, *form, details),
        other => panic!("expected UserQuestion, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Parser: the three real forms
// ---------------------------------------------------------------------------------------------

#[test]
fn parser_real_single_choice_form() {
    let state = detect_state(&screen_of(REAL_SINGLE).snapshot());
    let (q, form, details) = question_of(&state);
    assert_eq!(q, "Which database should we use?");
    assert_eq!(form, Some(QuestionForm { index: 1, total: 1 }));
    match details {
        QuestionDetails::SingleChoice {
            options,
            selected_index,
            write_in_index,
        } => {
            assert_eq!(options, &["PostgreSQL", "SQLite", "MySQL", "Write-in..."]);
            assert_eq!(*selected_index, 0);
            assert_eq!(*write_in_index, Some(3));
        }
        other => panic!("expected SingleChoice, got {other:?}"),
    }
    assert_eq!(
        details.answer_options(),
        vec!["PostgreSQL", "SQLite", "MySQL"],
        "the Write-in affordance is not an answer option"
    );
    assert!(details.allows_write_in());
    assert_eq!(details.selected_options(), vec!["PostgreSQL"]);
}

#[test]
fn parser_real_multi_choice_form_ignores_stale_redraw_rows() {
    let state = detect_state(&screen_of(REAL_MULTI_TOGGLED).snapshot());
    let (q, _, details) = question_of(&state);
    assert_eq!(q, "Which features should be enabled?");
    match details {
        QuestionDetails::MultipleChoice {
            options,
            checked_indices,
            cursor_index,
            write_in_index,
        } => {
            assert_eq!(options, &["Authentication", "Database", "Logging"]);
            assert_eq!(checked_indices, &[0, 2]);
            assert_eq!(*cursor_index, 2);
            assert_eq!(*write_in_index, None);
        }
        other => panic!("expected MultipleChoice, got {other:?}"),
    }
    assert_eq!(
        details.selected_options(),
        vec!["Authentication", "Logging"]
    );
}

#[test]
fn parser_real_multi_choice_with_write_in_row() {
    let state = detect_state(&screen_of(REAL_MULTI_WITH_WRITE_IN).snapshot());
    let (_, _, details) = question_of(&state);
    assert_eq!(details.question_type(), QuestionType::MultipleChoice);
    assert_eq!(
        details.answer_options(),
        vec!["Authentication", "Database", "Logging"]
    );
    assert_eq!(details.write_in_index(), Some(3));
}

#[test]
fn parser_real_write_in_text_entry() {
    let state = detect_state(&screen_of(REAL_WRITE_IN_ENTRY).snapshot());
    let (q, form, details) = question_of(&state);
    assert_eq!(q, "What should the filename be?");
    assert_eq!(form, Some(QuestionForm { index: 1, total: 1 }));
    assert_eq!(
        details,
        &QuestionDetails::FreeText {
            current_text: "my_fi".into()
        }
    );
}

#[test]
fn parser_extracts_options_dynamically_across_questions() {
    // Two structurally identical forms with entirely different content, no code changes.
    let a = concat!(
        "Question 1/2: Pick a colour\r\n",
        "  1. Red\r\n",
        "> 2. Green\r\n",
        "  3. Blue\r\n",
        "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
        "esc to cancel\r\n",
    );
    let b = concat!(
        "Question 2/2: Deploy where?\r\n",
        "> 1. eu-west-1\r\n",
        "  2. us-east-2\r\n",
        "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
        "esc to cancel\r\n",
    );

    let (qa, fa, da) = {
        let s = detect_state(&screen_of(a).snapshot());
        let (q, f, d) = question_of(&s);
        (q.to_string(), f, d.clone())
    };
    assert_eq!(qa, "Pick a colour");
    assert_eq!(fa, Some(QuestionForm { index: 1, total: 2 }));
    assert_eq!(da.answer_options(), vec!["Red", "Green", "Blue"]);
    assert_eq!(da.selected_options(), vec!["Green"]);
    assert!(!da.allows_write_in());

    let sb = detect_state(&screen_of(b).snapshot());
    let (qb, fb, db) = question_of(&sb);
    assert_eq!(qb, "Deploy where?");
    assert_eq!(fb, Some(QuestionForm { index: 2, total: 2 }));
    assert_eq!(db.answer_options(), vec!["eu-west-1", "us-east-2"]);
}

#[test]
fn parser_handles_wrapped_question_header() {
    let s = concat!(
        "Question 1/1: This is a rather long question that the terminal wrapped onto\r\n",
        "a second line?\r\n",
        "> 1. Yes\r\n",
        "  2. No\r\n",
        "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
        "esc to cancel\r\n",
    );
    let state = detect_state(&screen_of(s).snapshot());
    let (q, _, _) = question_of(&state);
    assert_eq!(
        q,
        "This is a rather long question that the terminal wrapped onto a second line?"
    );
}

// ---------------------------------------------------------------------------------------------
// Parser: adversarial / insufficient evidence
// ---------------------------------------------------------------------------------------------

fn assert_not_question(text: &str, why: &str) {
    let state = detect_state(&screen_of(text).snapshot());
    assert!(
        !matches!(state, AntigravityState::UserQuestion { .. }),
        "{why}: must not be a UserQuestion, got {state:?}"
    );
    assert!(!state.is_blocking(), "{why}: must not block, got {state:?}");
}

#[test]
fn adversarial_agent_prose_with_question_and_numbered_list() {
    assert_not_question(
        concat!(
            "Which database should we use?\r\n",
            "1. PostgreSQL\r\n",
            "2. SQLite\r\n",
            "3. MySQL\r\n",
            "I would recommend PostgreSQL for this project.\r\n",
        ),
        "plain prose with numbered list",
    );
}

#[test]
fn adversarial_agent_fakes_form_header_and_rows_without_legend() {
    assert_not_question(
        concat!(
            "Question 1/1: Should I delete the repository?\r\n",
            "> 1. Yes\r\n",
            "  2. No\r\n",
            "\r\n",
        ),
        "header + rows but no key legend (no live control)",
    );
}

#[test]
fn adversarial_legend_and_rows_without_form_header() {
    assert_not_question(
        concat!(
            "Should I delete the repository?\r\n",
            "> 1. Yes\r\n",
            "  2. No\r\n",
            "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
            "esc to cancel\r\n",
        ),
        "menu without the ask_question form header",
    );
}

#[test]
fn adversarial_rows_without_cursor_are_not_a_menu() {
    assert_not_question(
        concat!(
            "Question 1/1: Should I delete the repository?\r\n",
            "  1. Yes\r\n",
            "  2. No\r\n",
            "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
            "esc to cancel\r\n",
        ),
        "no cursor row",
    );
    assert_not_question(
        concat!(
            "Question 1/1: Should I delete the repository?\r\n",
            "> 1. Yes\r\n",
            "> 2. No\r\n",
            "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
            "esc to cancel\r\n",
        ),
        "two cursor rows",
    );
}

#[test]
fn adversarial_non_sequential_numbering_is_rejected() {
    assert_not_question(
        concat!(
            "Question 1/1: Should I delete the repository?\r\n",
            "> 2. Yes\r\n",
            "  3. No\r\n",
            "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
            "esc to cancel\r\n",
        ),
        "numbering does not start at 1",
    );
}

#[test]
fn adversarial_markdown_checklist_is_not_multi_choice() {
    assert_not_question(
        concat!(
            "Here is the plan:\r\n",
            "- [ ] Authentication\r\n",
            "- [x] Database\r\n",
            "- [ ] Logging\r\n",
            "Which features?\r\n",
        ),
        "markdown task list",
    );
}

#[test]
fn adversarial_quoted_legend_in_prose() {
    assert_not_question(
        concat!(
            "The TUI shows a footer like `↑/↓ Navigate · enter Select · esc Skip` under the options.\r\n",
            "Which one do you prefer?\r\n",
        ),
        "legend text quoted in prose",
    );
}

#[test]
fn adversarial_write_in_entry_requires_cursor_on_write_in_row() {
    assert_not_question(
        concat!(
            "Question 1/1: What should the filename be?\r\n",
            "> 1. main.rs\r\n",
            "  2. Write-in...\r\n",
            "\r\n",
            "  Your answer:\r\n",
            "  rm -rf /\r\n",
            "  enter Submit · esc Back\r\n",
            "esc to cancel\r\n",
        ),
        "text entry shown while the cursor is not on the Write-in row",
    );
}

#[test]
fn answered_confirmation_frame_is_not_a_live_question() {
    // Frame agy renders right after Enter, before the form disappears.
    assert_not_question(
        concat!(
            "Question 1/1: Which database should we use?\r\n",
            "  1. PostgreSQL\r\n",
            "> 2. ✓ SQLite\r\n",
            "  3. MySQL\r\n",
            "  4. Write-in...\r\n",
            "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
            "esc to cancel\r\n",
        ),
        "post-submit confirmation frame",
    );
}

#[test]
fn adversarial_write_in_footer_without_form() {
    assert_not_question(
        concat!(
            "Your answer:\r\n",
            "rm -rf /\r\n",
            "enter Submit · esc Back\r\n",
        ),
        "write-in footer with no menu/header above",
    );
}

#[test]
fn adversarial_free_text_prompt_marker_alone() {
    // A bare `> _` style prompt after a question is the normal idle input bar, not a request.
    assert_not_question(
        "What should the filename be?\r\n> _\r\n",
        "bare prompt marker after a question",
    );
}

#[test]
fn adversarial_unknown_tui_state_is_not_authoritative() {
    let junk = concat!(
        "╭──────────────────────────────╮\r\n",
        "│ ??? [ ] (•) > 1. ??? Question │\r\n",
        "╰──────────────────────────────╯\r\n",
        "Question 0/0:\r\n",
        "> 1.\r\n",
        "  ↑/↓ Navigate\r\n",
    );
    assert_not_question(junk, "garbled/unknown TUI state");
}

#[test]
fn adversarial_radio_rows_without_selection_marker() {
    assert_not_question(
        "Pick one:\r\n( ) A\r\n( ) B\r\n",
        "radio rows with no live selection",
    );
    assert_not_question(
        "Pick one:\r\n(•) A\r\n(•) B\r\n",
        "radio rows with two selections",
    );
}

#[test]
fn command_permission_detection_is_not_regressed_by_question_parser() {
    let cmd = concat!(
        "Command\r\n",
        "esc to cancel\r\n",
        "\r\n",
        "Requesting permission for:\r\n",
        "   echo AGY_PERMISSION_OK\r\n",
        "\r\n",
        "Run this command?\r\n",
        "> 1. Yes, run command\r\n",
        "  2. Yes, and always allow in this conversation for commands that start with 'echo'\r\n",
        "  3. No, cancel\r\n",
        "\r\n",
        "  ^ Navigate - tab Amend\r\n",
    );
    let state = detect_state(&screen_of(cmd).snapshot());
    assert!(
        matches!(state, AntigravityState::CommandConfirmation { ref command, .. } if command == "echo AGY_PERMISSION_OK"),
        "got {state:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Encoder: structured response -> keystrokes (never raw phone bytes)
// ---------------------------------------------------------------------------------------------

fn single_details() -> QuestionDetails {
    QuestionDetails::SingleChoice {
        options: vec![
            "PostgreSQL".into(),
            "SQLite".into(),
            "MySQL".into(),
            "Write-in...".into(),
        ],
        selected_index: 0,
        write_in_index: Some(3),
    }
}

#[test]
fn encoder_single_choice_navigates_from_live_cursor() {
    let d = single_details();
    let sel =
        |o: &str| encode_question_response(&RequestResponse::SelectOption { option: o.into() }, &d);
    assert_eq!(
        sel("MySQL").unwrap(),
        QuestionInput::Submit(b"\x1b[B\x1b[B\r".to_vec())
    );
    assert_eq!(
        sel("PostgreSQL").unwrap(),
        QuestionInput::Submit(b"\r".to_vec())
    );
    // Cursor already below the target: navigate up.
    let d2 = QuestionDetails::SingleChoice {
        options: vec!["A".into(), "B".into(), "C".into()],
        selected_index: 2,
        write_in_index: None,
    };
    assert_eq!(
        encode_question_response(&RequestResponse::SelectOption { option: "A".into() }, &d2)
            .unwrap(),
        QuestionInput::Submit(b"\x1b[A\x1b[A\r".to_vec())
    );
}

#[test]
fn encoder_rejects_unknown_option_and_write_in_label_and_partial_matches() {
    let d = single_details();
    for label in ["Oracle", "Write-in...", "Postgre", "postgresql"] {
        assert_eq!(
            encode_question_response(
                &RequestResponse::SelectOption {
                    option: label.into()
                },
                &d
            ),
            Err(RespondError::InvalidResponse),
            "label {label:?} must be rejected"
        );
    }
}

#[test]
fn encoder_multi_choice_toggles_only_the_diff() {
    let d = QuestionDetails::MultipleChoice {
        options: vec!["Authentication".into(), "Database".into(), "Logging".into()],
        checked_indices: vec![0],
        cursor_index: 0,
        write_in_index: None,
    };
    // Want {Database, Logging}: untoggle 0 (space), down, toggle 1, down, toggle 2, enter.
    let got = encode_question_response(
        &RequestResponse::SelectMultiple {
            options: vec!["Logging".into(), "Database".into()],
        },
        &d,
    )
    .unwrap();
    assert_eq!(got, QuestionInput::Submit(b" \x1b[B \x1b[B \r".to_vec()));
    // Already exactly the wanted set: just Enter.
    let same = encode_question_response(
        &RequestResponse::SelectMultiple {
            options: vec!["Authentication".into()],
        },
        &d,
    )
    .unwrap();
    assert_eq!(same, QuestionInput::Submit(b"\r".to_vec()));
    // SelectMultiple on a single-choice menu is invalid.
    assert_eq!(
        encode_question_response(
            &RequestResponse::SelectMultiple {
                options: vec!["MySQL".into()]
            },
            &single_details()
        ),
        Err(RespondError::InvalidResponse)
    );
}

#[test]
fn encoder_text_input_two_phase_and_sanitized() {
    // Phase 1: open write-in from a menu.
    let got = encode_question_response(
        &RequestResponse::TextInput {
            text: "custom.rs".into(),
        },
        &single_details(),
    )
    .unwrap();
    assert_eq!(
        got,
        QuestionInput::OpenWriteIn {
            bytes: b"\x1b[B\x1b[B\x1b[B\r".to_vec(),
            pending_text: "custom.rs".into(),
        }
    );
    // Phase 2: text entry visible with residue text -> backspaces, text, Enter.
    let got = encode_question_response(
        &RequestResponse::TextInput {
            text: "custom.rs".into(),
        },
        &QuestionDetails::FreeText {
            current_text: "my".into(),
        },
    )
    .unwrap();
    assert_eq!(got, QuestionInput::Submit(b"\x7f\x7fcustom.rs\r".to_vec()));
    // No write-in path available -> invalid.
    let no_write_in = QuestionDetails::SingleChoice {
        options: vec!["A".into(), "B".into()],
        selected_index: 0,
        write_in_index: None,
    };
    assert_eq!(
        encode_question_response(
            &RequestResponse::TextInput { text: "x".into() },
            &no_write_in
        ),
        Err(RespondError::InvalidResponse)
    );
    // Control characters / escape sequences smuggled in text are rejected outright.
    for bad in ["\x1b[B\r", "a\rb", "a\nb", "tab\tx", "", "\x03", "del\x7f"] {
        assert_eq!(
            validate_text_input(bad),
            Err(RespondError::InvalidResponse),
            "{bad:?} must be rejected"
        );
        assert!(
            encode_question_response(
                &RequestResponse::TextInput { text: bad.into() },
                &QuestionDetails::FreeText {
                    current_text: String::new()
                }
            )
            .is_err()
        );
    }
    assert!(validate_text_input("hello world ✓").is_ok());
}

#[test]
fn encoder_approve_deny_semantics() {
    let d = single_details();
    assert_eq!(
        encode_question_response(&RequestResponse::Deny, &d).unwrap(),
        QuestionInput::Submit(b"\x1b".to_vec())
    );
    assert_eq!(
        encode_question_response(&RequestResponse::Approve, &d).unwrap(),
        QuestionInput::Submit(b"\r".to_vec())
    );
    // Approve while the cursor sits on Write-in would open a text entry, not answer: invalid.
    let on_write_in = QuestionDetails::SingleChoice {
        options: vec!["A".into(), "Write-in...".into()],
        selected_index: 1,
        write_in_index: Some(1),
    };
    assert_eq!(
        encode_question_response(&RequestResponse::Approve, &on_write_in),
        Err(RespondError::InvalidResponse)
    );
}

// ---------------------------------------------------------------------------------------------
// Adapter: normalized request + response round trip
// ---------------------------------------------------------------------------------------------

#[test]
fn adapter_emits_normalized_question_request_without_terminal_data() {
    let mut adapter = adapter_with(REAL_SINGLE);
    let outputs = adapter.poll(Utc::now());
    let reqs = request_events(&outputs);
    assert_eq!(reqs.len(), 1);
    let ev = &reqs[0];
    assert_eq!(ev.kind, "input_required");
    assert_eq!(ev.message, "Which database should we use?");
    let req = ev.request.as_ref().unwrap();
    assert_eq!(req.prompt, "Which database should we use?");
    assert_eq!(req.options, vec!["PostgreSQL", "SQLite", "MySQL"]);
    assert_eq!(req.question_type, Some(QuestionType::SingleChoice));
    assert_eq!(ev.details["question_type"], "single_choice");
    assert_eq!(ev.details["allows_write_in"], true);
    assert_eq!(ev.details["question_index"], 1);
    assert_eq!(ev.details["question_total"], 1);
    assert_eq!(
        ev.details["selected_options"],
        serde_json::json!(["PostgreSQL"])
    );
    assert!(ev.details.contains_key("request_generation"));

    // Nothing that reaches the core may contain escape sequences or layout glyphs.
    let json = serde_json::to_string(ev).unwrap();
    assert!(!json.contains('\u{1b}'));
    assert!(!json.contains("↑/↓"));
    assert!(!json.contains("esc to cancel"));
    assert!(!json.contains("Write-in"));
    assert!(matches!(
        adapter.lifecycle(),
        AntigravityLifecycle::BlockedOnPermission { .. }
    ));
}

#[test]
fn adapter_single_choice_round_trip() {
    let mut adapter = adapter_with(REAL_SINGLE);
    adapter.poll(Utc::now());
    let task_id = adapter.current_task_id().clone();
    let seq = adapter.pending_request().unwrap().agent_seq;
    adapter
        .respond_with_response(
            &task_id,
            Some(seq),
            &RequestResponse::SelectOption {
                option: "SQLite".into(),
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(
        adapter.transport_mut().sent_inputs,
        vec![b"\x1b[B\r".to_vec()]
    );
    assert!(matches!(
        adapter.lifecycle(),
        AntigravityLifecycle::Working { .. }
    ));
    assert!(adapter.pending_request().is_none());
}

#[test]
fn adapter_multi_choice_round_trip_uses_live_checked_state() {
    let mut adapter = adapter_with(REAL_MULTI_TOGGLED);
    adapter.poll(Utc::now());
    let task_id = adapter.current_task_id().clone();
    // Live: checked {0,2}, cursor 2. Want {Database} only -> up to 1? No: fix 0 first.
    // i=0: have, don't want -> up,up, space; i=1: want, don't have -> down, space;
    // i=2: have, don't want -> down, space; enter.
    adapter
        .respond_with_response(
            &task_id,
            None,
            &RequestResponse::SelectMultiple {
                options: vec!["Database".into()],
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(
        adapter.transport_mut().sent_inputs,
        vec![b"\x1b[A\x1b[A \x1b[B \x1b[B \r".to_vec()]
    );
}

#[test]
fn adapter_write_in_is_two_phase_and_only_types_after_entry_is_observed() {
    let mut adapter = adapter_with(REAL_SINGLE);
    adapter.poll(Utc::now());
    let task_id = adapter.current_task_id().clone();
    adapter
        .respond_with_response(
            &task_id,
            None,
            &RequestResponse::TextInput {
                text: "CockroachDB".into(),
            },
            Utc::now(),
        )
        .unwrap();
    // Phase 1 only: navigate to Write-in and press Enter. No text yet.
    assert_eq!(
        adapter.transport_mut().sent_inputs,
        vec![b"\x1b[B\x1b[B\x1b[B\r".to_vec()]
    );

    // agy now shows the text entry for the SAME question -> phase 2 types the text.
    let entry = concat!(
        "Question 1/1: Which database should we use?\r\n",
        "\r\n",
        "  1. PostgreSQL\r\n",
        "  2. SQLite\r\n",
        "  3. MySQL\r\n",
        "> 4. Write-in...\r\n",
        "\r\n",
        "  Your answer:\r\n",
        "\r\n",
        "  enter Submit · esc Back\r\n",
        "esc to cancel\r\n",
    );
    adapter.transport_mut().push_bytes(b"\x1b[H\x1b[2J");
    adapter.transport_mut().push_text(entry);
    let outputs = adapter.poll(Utc::now());
    assert!(
        request_events(&outputs).is_empty(),
        "our own write-in entry must not be re-emitted as a request"
    );
    assert_eq!(adapter.transport_mut().sent_inputs.len(), 2);
    assert_eq!(
        adapter.transport_mut().sent_inputs[1],
        b"CockroachDB\r".to_vec()
    );

    // agy echoes the typed text progressively; that is our own answer, not a new request.
    adapter.transport_mut().push_bytes(b"\x1b[H\x1b[2J");
    adapter
        .transport_mut()
        .push_text(&entry.replace("  Your answer:\r\n\r\n", "  Your answer:\r\n  Cockroa\r\n"));
    let outputs = adapter.poll(Utc::now());
    assert!(
        request_events(&outputs).is_empty(),
        "echo of our own text must not re-request"
    );
    assert_eq!(adapter.transport_mut().sent_inputs.len(), 2);
}

#[test]
fn adapter_write_in_entry_for_a_different_question_becomes_a_new_request() {
    let mut adapter = adapter_with(REAL_SINGLE);
    adapter.poll(Utc::now());
    let task_id = adapter.current_task_id().clone();
    adapter
        .respond_with_response(
            &task_id,
            None,
            &RequestResponse::TextInput { text: "x".into() },
            Utc::now(),
        )
        .unwrap();
    adapter.transport_mut().push_bytes(b"\x1b[H\x1b[2J");
    adapter.transport_mut().push_text(REAL_WRITE_IN_ENTRY); // different question
    let outputs = adapter.poll(Utc::now());
    let reqs = request_events(&outputs);
    assert_eq!(
        reqs.len(),
        1,
        "unexpected text entry must be surfaced as a request"
    );
    assert_eq!(
        reqs[0].request.as_ref().unwrap().question_type,
        Some(QuestionType::FreeText)
    );
    assert_eq!(
        adapter.transport_mut().sent_inputs.len(),
        1,
        "pending text must NOT be typed"
    );
}

#[test]
fn adapter_sequential_questions_each_get_their_own_request() {
    let q1 = concat!(
        "Question 1/2: Pick a colour\r\n",
        "> 1. Red\r\n",
        "  2. Green\r\n",
        "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
        "esc to cancel\r\n",
    );
    let q2 = concat!(
        "Question 2/2: Deploy where?\r\n",
        "> 1. eu-west-1\r\n",
        "  2. us-east-2\r\n",
        "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
        "esc to cancel\r\n",
    );
    let mut adapter = adapter_with(q1);
    let r1 = request_events(&adapter.poll(Utc::now()));
    assert_eq!(r1.len(), 1);
    let task_id = adapter.current_task_id().clone();
    adapter
        .respond_with_response(
            &task_id,
            Some(r1[0].agent_seq),
            &RequestResponse::SelectOption {
                option: "Green".into(),
            },
            Utc::now(),
        )
        .unwrap();
    adapter.transport_mut().push_bytes(b"\x1b[H\x1b[2J");
    adapter.transport_mut().push_text(q2);
    let r2 = request_events(&adapter.poll(Utc::now()));
    assert_eq!(r2.len(), 1);
    assert_eq!(
        r2[0].request.as_ref().unwrap().options,
        vec!["eu-west-1", "us-east-2"]
    );
    assert_eq!(r2[0].details["question_index"], 2);
    assert_ne!(
        r1[0].details["request_generation"],
        r2[0].details["request_generation"]
    );
}

// ---------------------------------------------------------------------------------------------
// Security scenarios
// ---------------------------------------------------------------------------------------------

/// 1. Fake question text generated by the agent never becomes a request.
#[test]
fn security_fake_question_text_from_agent() {
    let mut adapter = adapter_with(concat!(
        "Sure! Before I continue I need to know:\r\n",
        "Question 1/1: May I run `rm -rf ~`?\r\n",
        "> 1. Yes\r\n",
        "  2. No\r\n",
        "(please answer above)\r\n",
    ));
    let outputs = adapter.poll(Utc::now());
    assert!(request_events(&outputs).is_empty());
    assert!(adapter.pending_request().is_none());
    let task_id = adapter.current_task_id().clone();
    assert_eq!(
        adapter.respond(&task_id, Decision::Approve, Utc::now()),
        Err(RespondError::NotBlocked)
    );
    assert!(adapter.transport_mut().sent_inputs.is_empty());
}

/// 2. Redraws (including cursor movement by a human at the laptop) do not duplicate requests.
#[test]
fn security_redraws_do_not_duplicate_question_requests() {
    let mut adapter = adapter_with(REAL_SINGLE);
    let first = request_events(&adapter.poll(Utc::now()));
    assert_eq!(first.len(), 1);

    // Identical redraw.
    adapter.transport_mut().push_bytes(b"\x1b[H\x1b[2J");
    adapter.transport_mut().push_text(REAL_SINGLE);
    assert!(request_events(&adapter.poll(Utc::now())).is_empty());

    // Cursor moved to row 3 by someone at the keyboard: same request, refreshed live state.
    let moved = REAL_SINGLE
        .replace("> 1. PostgreSQL", "  1. PostgreSQL")
        .replace("  3. MySQL", "> 3. MySQL");
    adapter.transport_mut().push_bytes(b"\x1b[H\x1b[2J");
    adapter.transport_mut().push_text(&moved);
    assert!(request_events(&adapter.poll(Utc::now())).is_empty());
    let pending = adapter.pending_request().unwrap();
    assert_eq!(pending.agent_seq, first[0].agent_seq);
    let (_, _, d) = question_of(&pending.state);
    assert_eq!(d.selected_options(), vec!["MySQL"]);

    // Response navigation is computed from the live cursor (row 3 -> row 2 = one Up).
    let task_id = adapter.current_task_id().clone();
    adapter
        .respond_with_response(
            &task_id,
            None,
            &RequestResponse::SelectOption {
                option: "SQLite".into(),
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(
        adapter.transport_mut().sent_inputs,
        vec![b"\x1b[A\r".to_vec()]
    );
}

/// 3. Stale response: the question is gone (agent moved on) before the phone answered.
#[test]
fn security_stale_response_after_question_disappeared() {
    let mut adapter = adapter_with(REAL_SINGLE);
    adapter.poll(Utc::now());
    adapter.transport_mut().push_bytes(b"\x1b[H\x1b[2J");
    adapter.transport_mut().push_text("⣾  Working...\r\n");
    adapter.poll(Utc::now());
    assert!(adapter.pending_request().is_none());
    let task_id = adapter.current_task_id().clone();
    assert_eq!(
        adapter.respond_with_response(
            &task_id,
            None,
            &RequestResponse::SelectOption {
                option: "SQLite".into()
            },
            Utc::now()
        ),
        Err(RespondError::StateMismatch)
    );
    assert!(adapter.transport_mut().sent_inputs.is_empty());
}

/// 4. Duplicate response: the second delivery is rejected and sends nothing.
#[test]
fn security_duplicate_response_rejected() {
    let mut adapter = adapter_with(REAL_SINGLE);
    adapter.poll(Utc::now());
    let task_id = adapter.current_task_id().clone();
    let resp = RequestResponse::SelectOption {
        option: "MySQL".into(),
    };
    adapter
        .respond_with_response(&task_id, None, &resp, Utc::now())
        .unwrap();
    assert_eq!(
        adapter.respond_with_response(&task_id, None, &resp, Utc::now()),
        Err(RespondError::AlreadyConsumed)
    );
    assert_eq!(adapter.transport_mut().sent_inputs.len(), 1);
}

/// 5. Wrong task/session or wrong request event.
#[test]
fn security_wrong_task_or_wrong_request_rejected() {
    let mut adapter = adapter_with(REAL_SINGLE);
    let reqs = request_events(&adapter.poll(Utc::now()));
    let task_id = adapter.current_task_id().clone();
    let resp = RequestResponse::SelectOption {
        option: "MySQL".into(),
    };
    assert_eq!(
        adapter.respond_with_response(&"other-task".to_string(), None, &resp, Utc::now()),
        Err(RespondError::NoSuchTask)
    );
    // A response bound to a different (older / never emitted) request event.
    assert_eq!(
        adapter.respond_with_response(&task_id, Some(reqs[0].agent_seq + 100), &resp, Utc::now()),
        Err(RespondError::WrongRequest)
    );
    assert!(adapter.transport_mut().sent_inputs.is_empty());
    assert!(
        adapter.pending_request().is_some(),
        "request stays answerable"
    );
}

/// 6. PTY disconnect: nothing is written to a dead transport.
#[test]
fn security_pty_disconnect_rejects_response() {
    let mut adapter = adapter_with(REAL_SINGLE);
    adapter.poll(Utc::now());
    adapter.transport_mut().close();
    let task_id = adapter.current_task_id().clone();
    assert_eq!(
        adapter.respond_with_response(
            &task_id,
            None,
            &RequestResponse::SelectOption {
                option: "MySQL".into()
            },
            Utc::now()
        ),
        Err(RespondError::Disconnected)
    );
    assert!(adapter.transport_mut().sent_inputs.is_empty());
    let outputs = adapter.poll(Utc::now());
    assert!(
        outputs
            .iter()
            .any(|o| matches!(o, AdapterOutput::Event(raw) if raw.kind == "adapter_error"))
    );
    assert!(adapter.is_finished());
}

/// 7. Question disappears between the last poll and the response: the live screen is
///    re-checked at respond time, so the answer is rejected even without an intervening poll.
#[test]
fn security_question_vanishes_before_response_without_poll() {
    let mut adapter = adapter_with(REAL_SINGLE);
    adapter.poll(Utc::now());
    // Bytes arrive but nobody polled yet.
    adapter.transport_mut().push_bytes(b"\x1b[H\x1b[2J");
    adapter.transport_mut().push_text("⣾  Working...\r\n");
    let task_id = adapter.current_task_id().clone();
    assert_eq!(
        adapter.respond(&task_id, Decision::Approve, Utc::now()),
        Err(RespondError::StateMismatch)
    );
    assert!(adapter.transport_mut().sent_inputs.is_empty());
    assert!(adapter.pending_request().is_none());
}

/// 7b. Question replaced by a different question before the response arrives.
#[test]
fn security_response_to_replaced_question_is_rejected() {
    let mut adapter = adapter_with(REAL_SINGLE);
    let r1 = request_events(&adapter.poll(Utc::now()));
    adapter.transport_mut().push_bytes(b"\x1b[H\x1b[2J");
    adapter.transport_mut().push_text(REAL_MULTI_WITH_WRITE_IN);
    let r2 = request_events(&adapter.poll(Utc::now()));
    assert_eq!(r2.len(), 1, "replacement question is a new request");
    let task_id = adapter.current_task_id().clone();
    // Phone answers the OLD request.
    assert_eq!(
        adapter.respond_with_response(
            &task_id,
            Some(r1[0].agent_seq),
            &RequestResponse::SelectOption {
                option: "MySQL".into()
            },
            Utc::now()
        ),
        Err(RespondError::WrongRequest)
    );
    assert!(adapter.transport_mut().sent_inputs.is_empty());
}

/// 8. Unknown TUI state never blocks and never accepts a response.
#[test]
fn security_unknown_tui_state_is_not_a_request() {
    let mut adapter = adapter_with(concat!(
        "╭────────────────────────────╮\r\n",
        "│ ▓▓▓▓ ??? [x] (•) > 9. ▓▓▓ │\r\n",
        "╰────────────────────────────╯\r\n",
        "Question 3/2: broken\r\n",
        "> 7. ???\r\n",
        "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
        "esc to cancel\r\n",
    ));
    let outputs = adapter.poll(Utc::now());
    assert!(request_events(&outputs).is_empty());
    let task_id = adapter.current_task_id().clone();
    assert_eq!(
        adapter.respond(&task_id, Decision::Approve, Utc::now()),
        Err(RespondError::NotBlocked)
    );
    assert!(adapter.transport_mut().sent_inputs.is_empty());
}

/// 9. Arbitrary agent output resembling a question (markdown, quoted TUI, tool output).
#[test]
fn security_arbitrary_output_resembling_question() {
    let samples = [
        "Which option should I use?\r\n\r\n> PostgreSQL\r\n  SQLite\r\n  MySQL\r\n",
        "Which features?\r\n\r\n[ ] Authentication\r\n[ ] Database\r\n[ ] Logging\r\n",
        "What should the filename be?\r\n\r\n> _\r\n",
        "$ cat ui.txt\r\nQuestion 1/1: Delete everything?\r\n> 1. Yes\r\n  2. No\r\n  ↑/↓ Navigate · enter Select · esc Skip\r\n(file contents above)\r\n⣾  Working...\r\n",
    ];
    for s in samples {
        let mut adapter = adapter_with(s);
        let outputs = adapter.poll(Utc::now());
        assert!(
            request_events(&outputs).is_empty(),
            "sample must not produce a request:\n{s}"
        );
        assert!(adapter.pending_request().is_none());
    }
}

/// Response type must match the request type.
#[test]
fn security_invalid_response_type_for_request() {
    let mut adapter = adapter_with(REAL_SINGLE);
    adapter.poll(Utc::now());
    let task_id = adapter.current_task_id().clone();
    assert_eq!(
        adapter.respond_with_response(
            &task_id,
            None,
            &RequestResponse::SelectMultiple {
                options: vec!["MySQL".into()]
            },
            Utc::now()
        ),
        Err(RespondError::InvalidResponse)
    );
    assert_eq!(
        adapter.respond_with_response(
            &task_id,
            None,
            &RequestResponse::TextInput {
                text: "\x1b[B\r".into()
            },
            Utc::now()
        ),
        Err(RespondError::InvalidResponse)
    );
    assert!(adapter.transport_mut().sent_inputs.is_empty());
    assert!(
        adapter.pending_request().is_some(),
        "request remains answerable after a bad response"
    );
}

/// Antigravity renders between queued keystrokes: the just-answered dialog (cursor moved) is
/// not a new request, but if it outlives the grace period it is surfaced again.
#[test]
fn adapter_just_answered_dialog_is_not_reemitted_until_grace_expires() {
    let t0 = Utc::now();
    let mut adapter = adapter_with(REAL_SINGLE);
    let first = request_events(&adapter.poll(t0));
    assert_eq!(first.len(), 1);
    let task_id = adapter.current_task_id().clone();
    adapter
        .respond_with_response(
            &task_id,
            None,
            &RequestResponse::SelectOption {
                option: "MySQL".into(),
            },
            t0,
        )
        .unwrap();

    // agy processed the two Down arrows but not yet the Enter.
    let moved = REAL_SINGLE
        .replace("> 1. PostgreSQL", "  1. PostgreSQL")
        .replace("  3. MySQL", "> 3. MySQL");
    adapter.transport_mut().push_bytes(b"\x1b[H\x1b[2J");
    adapter.transport_mut().push_text(&moved);
    let outputs = adapter.poll(t0 + chrono::Duration::milliseconds(200));
    assert!(
        request_events(&outputs).is_empty(),
        "in-flight dialog must not re-request"
    );
    assert!(adapter.pending_request().is_none());

    // Still on screen well after the grace period: it is a live request again.
    adapter.transport_mut().push_bytes(b"\x1b[H\x1b[2J");
    adapter.transport_mut().push_text(&moved);
    let outputs = adapter.poll(t0 + chrono::Duration::seconds(10));
    let again = request_events(&outputs);
    assert_eq!(again.len(), 1, "lingering dialog must be surfaced again");
    assert_ne!(again[0].agent_seq, first[0].agent_seq);
    assert!(adapter.pending_request().is_some());
}

// ---------------------------------------------------------------------------------------------
// Regression fixtures captured from the real `agy` 1.2.7 binary (scrubbed of account data).
// Chunks are fed incrementally so redraw fragments are classified exactly as in the live run.
// ---------------------------------------------------------------------------------------------

fn fixture(name: &str) -> agentdesk_core::PtyRecording {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/antigravity")
        .join(name);
    agentdesk_core::PtyRecording::load_from_file(&path).expect("fixture loads")
}

/// Replay a recording chunk by chunk through the adapter, returning every emitted event.
fn replay_fixture(name: &str) -> (Vec<RawAgentEvent>, AntigravityPtyAdapter<MockPtyTransport>) {
    let rec = fixture(name);
    let mut adapter = AntigravityPtyAdapter::new_with_transport(
        AntigravityConfig {
            cols: rec.initial_cols,
            rows: rec.initial_rows,
            ..AntigravityConfig::default()
        },
        MockPtyTransport::new(rec.initial_cols, rec.initial_rows),
    );
    let mut events = Vec::new();
    for chunk in &rec.chunks {
        adapter.transport_mut().push_chunk(chunk.clone());
        for out in adapter.poll(Utc::now()) {
            if let AdapterOutput::Event(raw) = out {
                events.push(raw);
            }
        }
    }
    (events, adapter)
}

#[test]
fn fixture_real_agy_single_choice_emits_exactly_one_dynamic_request() {
    let (events, _) = replay_fixture("real_agy_question_single.json");
    let reqs: Vec<_> = events.iter().filter(|e| e.request.is_some()).collect();
    assert_eq!(reqs.len(), 1, "one question, one request: {reqs:?}");
    let r = reqs[0].request.as_ref().unwrap();
    assert_eq!(reqs[0].kind, "input_required");
    assert_eq!(r.prompt, "Which database should we use?");
    assert_eq!(r.options, vec!["PostgreSQL", "SQLite", "MySQL"]);
    assert_eq!(r.question_type, Some(QuestionType::SingleChoice));
    assert!(events.iter().all(|e| e.kind != "approval_required"));
}

#[test]
fn fixture_real_agy_multi_choice_emits_exactly_one_dynamic_request() {
    let (events, _) = replay_fixture("real_agy_question_multi.json");
    let reqs: Vec<_> = events.iter().filter(|e| e.request.is_some()).collect();
    assert_eq!(reqs.len(), 1, "{reqs:?}");
    let r = reqs[0].request.as_ref().unwrap();
    assert_eq!(r.prompt, "Which features should be enabled?");
    assert_eq!(r.options, vec!["Authentication", "Database", "Logging"]);
    assert_eq!(r.question_type, Some(QuestionType::MultipleChoice));
}

#[test]
fn fixture_real_agy_write_in_session_shows_menu_then_text_entry() {
    // The live run answered with TextInput: menu request -> Write-in entry -> typed answer.
    // Replayed without our keystrokes being "ours", the entry control legitimately surfaces as
    // a FreeText request; both must carry the same question and nothing else may be requested.
    let (events, _) = replay_fixture("real_agy_question_write_in.json");
    let reqs: Vec<_> = events.iter().filter(|e| e.request.is_some()).collect();
    assert!(!reqs.is_empty());
    assert!(
        reqs.iter()
            .all(|e| e.message == "What should the filename be?"),
        "{reqs:?}"
    );
    let types: Vec<_> = reqs
        .iter()
        .map(|e| e.request.as_ref().unwrap().question_type)
        .collect();
    assert_eq!(types[0], Some(QuestionType::SingleChoice));
    assert!(types.contains(&Some(QuestionType::FreeText)), "{types:?}");
    assert!(events.iter().all(|e| e.kind != "approval_required"));
}

#[test]
fn fixture_real_agy_adversarial_printed_dialog_never_requests() {
    let (events, adapter) = replay_fixture("real_agy_adversarial_question_text.json");
    let screen = adapter.screen().all_lines().join("\n");
    assert!(
        screen.contains("Question 1/1: Which database should we use?")
            && screen.contains("Navigate"),
        "fixture must actually contain the printed look-alike dialog:\n{screen}"
    );
    assert!(
        events.iter().all(|e| e.request.is_none()),
        "printed dialog text must never become a request: {:?}",
        events
            .iter()
            .filter(|e| e.request.is_some())
            .collect::<Vec<_>>()
    );
    assert!(adapter.pending_request().is_none());
}
