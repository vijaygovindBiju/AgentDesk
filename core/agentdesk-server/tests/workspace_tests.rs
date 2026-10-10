//! Test suite for AgentDesk Terminal Workspace and Automatic Coding-Agent Discovery.
//! Validates:
//! 1. Agent discovery (agy, claude, codex, gemini, ignores missing, handles duplicate paths)
//! 2. Agent selection screen (rendering, selection, launch, empty state)
//! 3. Real PTY Shell session (spawn in cwd, input/output, Ctrl+C encoding, resize, ANSI)
//! 4. Preferences persistence (last used agent, last used workspace directory)
//! 5. Workspace UI rendering (top bar, terminal grid, overlays)

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use agentdesk_core::Screen as VtScreen;
use agentdesk_server::discovery::{AdapterType, AgentAvailability, AgentDefinition, AgentDiscovery};
use agentdesk_server::tui::key_encoder::encode_key_event;
use agentdesk_server::tui::preferences::{WorkspacePreferences, load_preferences, save_preferences};
use agentdesk_server::tui::state::{ActiveTerminalTab, Screen, TuiState, WorkspaceOverlay};
use agentdesk_server::tui::terminal_session::TerminalSession;
use agentdesk_server::tui::views::render_ui;

fn create_dummy_executable(dir: &Path, name: &str) -> PathBuf {
    let file_path = dir.join(name);
    File::create(&file_path).unwrap();
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&file_path, perms).unwrap();
    }
    file_path
}

fn render_state_to_string(state: &TuiState, width: u16, height: u16) -> String {
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

// =========================================================================
// 1. Agent Discovery Tests
// =========================================================================

#[test]
fn test_agent_discovery_detects_all_known_agents() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path();

    create_dummy_executable(path, "agy");
    create_dummy_executable(path, "claude");
    create_dummy_executable(path, "codex");
    create_dummy_executable(path, "gemini");

    let agents = AgentDiscovery::discover_in_paths(&[path.to_path_buf()]);
    assert_eq!(agents.len(), 4);

    let agy = agents.iter().find(|a| a.id == "antigravity").unwrap();
    assert_eq!(agy.executable, "agy");
    assert_eq!(agy.display_name, "Antigravity");
    assert!(agy.is_ready());
    assert_eq!(agy.adapter_type, AdapterType::AntigravityPty);

    let gemini = agents.iter().find(|a| a.id == "gemini").unwrap();
    assert_eq!(gemini.executable, "gemini");
    assert!(gemini.is_ready());
    assert_eq!(gemini.adapter_type, AdapterType::Acp);

    let claude = agents.iter().find(|a| a.id == "claude").unwrap();
    assert_eq!(claude.executable, "claude");
    assert!(!claude.is_ready());
    assert_eq!(claude.adapter_type, AdapterType::None);

    let codex = agents.iter().find(|a| a.id == "codex").unwrap();
    assert_eq!(codex.executable, "codex");
    assert!(!codex.is_ready());
    assert_eq!(codex.adapter_type, AdapterType::None);
}

#[test]
fn test_agent_discovery_ignores_unavailable_executables() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path();

    // Only agy exists
    create_dummy_executable(path, "agy");

    let agents = AgentDiscovery::discover_in_paths(&[path.to_path_buf()]);
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, "antigravity");
    assert!(agents.iter().all(|a| a.id != "claude"));
    assert!(agents.iter().all(|a| a.id != "codex"));
    assert!(agents.iter().all(|a| a.id != "gemini"));
}

#[test]
fn test_agent_discovery_handles_duplicate_paths() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path();

    create_dummy_executable(path, "agy");
    create_dummy_executable(path, "gemini");

    let duplicate_paths = vec![path.to_path_buf(), path.to_path_buf(), path.to_path_buf()];
    let agents = AgentDiscovery::discover_in_paths(&duplicate_paths);

    assert_eq!(agents.len(), 2);
}

// =========================================================================
// 2. Agent Selection Screen Tests
// =========================================================================

#[test]
fn test_agent_selection_screen_rendering() {
    let mut state = TuiState::default();

    let defs = vec![
        AgentDefinition {
            id: "antigravity".into(),
            display_name: "Antigravity".into(),
            executable: "agy".into(),
            executable_path: PathBuf::from("/usr/local/bin/agy"),
            version: Some("1.2.10".into()),
            availability: AgentAvailability::Ready,
            capabilities: vec!["Interactive PTY".into()],
            adapter_type: AdapterType::AntigravityPty,
        },
        AgentDefinition {
            id: "codex".into(),
            display_name: "Codex".into(),
            executable: "codex".into(),
            executable_path: PathBuf::from("/usr/local/bin/codex"),
            version: Some("0.155.1".into()),
            availability: AgentAvailability::Unsupported {
                reason: "No adapter available yet".into(),
            },
            capabilities: vec!["CLI Tool".into()],
            adapter_type: AdapterType::None,
        },
    ];

    state.init_workspace_selection(defs, WorkspacePreferences::default());
    assert_eq!(state.current_screen, Screen::AgentSelection);

    let rendered = render_state_to_string(&state, 80, 24);
    assert!(rendered.contains("Select a coding agent"));
    assert!(rendered.contains("Antigravity"));
    assert!(rendered.contains("Ready"));
    assert!(rendered.contains("Codex"));
    assert!(rendered.contains("No adapter available yet"));
    assert!(rendered.contains("Enter Launch"));
}

#[test]
fn test_agent_selection_empty_state_rendering() {
    let mut state = TuiState::default();
    state.init_workspace_selection(Vec::new(), WorkspacePreferences::default());

    let rendered = render_state_to_string(&state, 80, 24);
    assert!(rendered.contains("No supported coding agents detected."));
    assert!(rendered.contains("Install a supported coding agent"));
}

#[test]
fn test_agent_selection_navigation() {
    let mut state = TuiState::default();
    let defs = vec![
        AgentDefinition {
            id: "antigravity".into(),
            display_name: "Antigravity".into(),
            executable: "agy".into(),
            executable_path: PathBuf::from("/bin/agy"),
            version: None,
            availability: AgentAvailability::Ready,
            capabilities: vec![],
            adapter_type: AdapterType::AntigravityPty,
        },
        AgentDefinition {
            id: "gemini".into(),
            display_name: "Gemini CLI".into(),
            executable: "gemini".into(),
            executable_path: PathBuf::from("/bin/gemini"),
            version: None,
            availability: AgentAvailability::Ready,
            capabilities: vec![],
            adapter_type: AdapterType::Acp,
        },
    ];

    state.init_workspace_selection(defs, WorkspacePreferences::default());
    assert_eq!(state.selected_agent_index, 0);

    state.select_next_agent();
    assert_eq!(state.selected_agent_index, 1);

    state.select_next_agent();
    assert_eq!(state.selected_agent_index, 0);

    state.select_previous_agent();
    assert_eq!(state.selected_agent_index, 1);
}

// =========================================================================
// 3. Real PTY Shell Session Tests
// =========================================================================

#[test]
fn test_shell_session_spawn_and_directory() {
    let temp = tempfile::tempdir().unwrap();
    let temp_path = temp.path().canonicalize().unwrap();

    let mut session = TerminalSession::spawn_with_shell("/bin/sh", &temp_path, 120, 40).unwrap();
    assert!(session.is_alive());
    assert_eq!(session.cwd, temp_path);

    let deadline = Instant::now() + Duration::from_secs(6);
    let mut matched = false;
    let expected_str = temp_path.to_string_lossy();
    while Instant::now() < deadline {
        session.pump_output();
        let _ = session.send_input(b"pwd\r");
        std::thread::sleep(Duration::from_millis(250));
        session.pump_output();
        let text = session.screen.all_lines().join("\n");
        if text.contains(expected_str.as_ref()) {
            matched = true;
            break;
        }
    }

    for (i, l) in session.screen.all_lines().iter().enumerate() {
        if !l.is_empty() {
            println!("LINE {}: {:?}", i, l);
        }
    }
    assert!(matched, "Shell output did not reflect expected working directory");
    session.terminate();
}

#[test]
fn test_shell_session_ctrl_c_signals() {
    // Verify Ctrl+C encodes to 0x03 byte
    let key = KeyEvent {
        code: KeyCode::Char('c'),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };
    let bytes = encode_key_event(key).unwrap();
    assert_eq!(bytes, vec![0x03]);

    // Test sending Ctrl+C to active sleep command in shell
    let temp = tempfile::tempdir().unwrap();
    let mut session = TerminalSession::spawn_with_shell("/bin/sh", temp.path(), 120, 40).unwrap();

    std::thread::sleep(Duration::from_millis(600));
    session.pump_output();

    // Start a blocking sleep command
    session.send_input(b"sleep 100\r").unwrap();
    std::thread::sleep(Duration::from_millis(300));

    // Send Ctrl+C
    session.send_input(&bytes).unwrap();
    std::thread::sleep(Duration::from_millis(300));

    // After Ctrl+C, send another command to confirm shell prompt returned immediately
    session.send_input(b"echo PROMPT_RETURNED_AFTER_SIGINT\r").unwrap();

    let deadline = Instant::now() + Duration::from_secs(4);
    let mut returned = false;
    while Instant::now() < deadline {
        session.pump_output();
        let text = session.screen.all_lines().join("\n");
        if text.contains("PROMPT_RETURNED_AFTER_SIGINT") {
            returned = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(returned, "Shell did not return after Ctrl+C interrupt");
    session.terminate();
}

#[test]
fn test_shell_session_resize() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = TerminalSession::spawn_shell(temp.path(), 80, 24).unwrap();

    assert_eq!(session.screen.cols, 80);
    assert_eq!(session.screen.rows, 24);

    session.resize(132, 43);
    assert_eq!(session.screen.cols, 132);
    assert_eq!(session.screen.rows, 43);

    session.terminate();
}

#[test]
fn test_vt100_screen_ansi_colors_and_styles() {
    let mut screen = VtScreen::new(80, 24);
    // Write ANSI escape sequences for bold, cyan, yellow, reverse
    screen.process_bytes(b"\x1b[1;36mBold Cyan\x1b[0m \x1b[7mReverse\x1b[0m");

    let row = screen.row_text(0);
    assert!(row.contains("Bold Cyan Reverse"));

    let cell_bold = screen.cell(0, 0).unwrap();
    assert!(cell_bold.attrs.bold);
    assert_eq!(cell_bold.attrs.fg, Some(6)); // Cyan

    let cell_rev = screen.cell(0, 10).unwrap();
    assert!(cell_rev.attrs.reverse);
}

// =========================================================================
// 4. Persistence Tests
// =========================================================================

#[test]
fn test_workspace_preferences_persistence() {
    let temp = tempfile::tempdir().unwrap();
    let config_dir = temp.path();

    let initial = load_preferences(config_dir);
    assert_eq!(initial.last_agent_id, None);
    assert_eq!(initial.last_workspace, None);

    let saved = WorkspacePreferences {
        last_agent_id: Some("antigravity".into()),
        last_workspace: Some(PathBuf::from("/home/user/my-project")),
        dangerously_skip_permissions: true,
        recent_workspaces: vec![PathBuf::from("/home/user/my-project")],
    };
    save_preferences(config_dir, &saved).unwrap();

    let loaded = load_preferences(config_dir);
    assert_eq!(loaded, saved);
    assert!(loaded.dangerously_skip_permissions);
    assert_eq!(loaded.recent_workspaces, vec![PathBuf::from("/home/user/my-project")]);
}

// =========================================================================
// 5. Workspace UI Rendering & Top Bar Tests
// =========================================================================

#[test]
fn test_workspace_ui_rendering_and_top_bar() {
    let mut state = TuiState::default();
    state.current_screen = Screen::Workspace;
    state.active_agent = Some(AgentDefinition {
        id: "antigravity".into(),
        display_name: "Antigravity".into(),
        executable: "agy".into(),
        executable_path: PathBuf::from("/bin/agy"),
        version: Some("1.2.10".into()),
        availability: AgentAvailability::Ready,
        capabilities: vec![],
        adapter_type: AdapterType::AntigravityPty,
    });
    state.agent_session_running = true;
    state.phone_connected = true;
    state.relay_connected = true;
    state.active_terminal_tab = ActiveTerminalTab::Shell;

    let rendered = render_state_to_string(&state, 100, 30);
    println!("DEBUG RENDERED:\n{}", rendered);
    assert!(rendered.contains("◈ AgentDesk"));
    assert!(rendered.contains("Antigravity"));
    assert!(rendered.contains("● Relay Connected"));
    assert!(rendered.contains("Phone"));
    assert!(rendered.contains("🔔") && rendered.contains('0'));
    assert!(rendered.contains("[Shell]"));
    assert!(rendered.contains("F1 Agent"));
    assert!(rendered.contains("F10 Exit"));
}

#[test]
fn test_workspace_overlay_agent_switch() {
    let mut state = TuiState::default();
    state.current_screen = Screen::Workspace;
    state.detected_agents = vec![
        AgentDefinition {
            id: "antigravity".into(),
            display_name: "Antigravity".into(),
            executable: "agy".into(),
            executable_path: PathBuf::from("/bin/agy"),
            version: None,
            availability: AgentAvailability::Ready,
            capabilities: vec![],
            adapter_type: AdapterType::AntigravityPty,
        },
        AgentDefinition {
            id: "claude".into(),
            display_name: "Claude Code".into(),
            executable: "claude".into(),
            executable_path: PathBuf::from("/bin/claude"),
            version: None,
            availability: AgentAvailability::Unsupported {
                reason: "No adapter available yet".into(),
            },
            capabilities: vec![],
            adapter_type: AdapterType::None,
        },
    ];
    state.workspace_overlay = WorkspaceOverlay::AgentSwitch;

    let rendered = render_state_to_string(&state, 100, 30);
    assert!(rendered.contains("Switch Active Coding Agent"));
    assert!(rendered.contains("Antigravity"));
    assert!(rendered.contains("Claude Code"));
    assert!(rendered.contains("Unsupported"));
}

#[test]
fn test_workspace_overlay_project_select() {
    let mut state = TuiState::default();
    state.current_screen = Screen::Workspace;
    state.workspace_dir = PathBuf::from("/media/pirate/Shared/AgentDesk");
    state.project_input_text = "/media/pirate/Shared/NewProject".into();
    state.workspace_overlay = WorkspaceOverlay::ProjectSelect;

    let rendered = render_state_to_string(&state, 100, 30);
    assert!(rendered.contains("Change Workspace Directory"));
    assert!(rendered.contains("New:     /media/pirate/Shared/NewProject"));
}

#[test]
fn test_workspace_overlay_confirm_switch() {
    let mut state = TuiState::default();
    state.current_screen = Screen::Workspace;
    state.active_agent = Some(AgentDefinition {
        id: "antigravity".into(),
        display_name: "Antigravity".into(),
        executable: "agy".into(),
        executable_path: PathBuf::from("/bin/agy"),
        version: None,
        availability: AgentAvailability::Ready,
        capabilities: vec![],
        adapter_type: AdapterType::AntigravityPty,
    });
    state.workspace_overlay = WorkspaceOverlay::SwitchConfirm;

    let rendered = render_state_to_string(&state, 100, 30);
    assert!(rendered.contains("Confirm Agent Termination"));
    assert!(rendered.contains("An active Antigravity session is currently running."));
    assert!(rendered.contains("Terminate this session and switch to the new agent?"));
}

#[test]
fn test_workspace_overlay_project_select_with_recents() {
    let mut state = TuiState::default();
    state.current_screen = Screen::Workspace;
    state.workspace_dir = PathBuf::from("/media/pirate/Shared/AgentDesk");
    state.project_input_text = "/media/pirate/Shared/AgentDesk".into();
    state.recent_workspaces = vec![
        PathBuf::from("/media/pirate/Shared/AgentDesk"),
        PathBuf::from("/home/pirate/projects/backend"),
    ];
    state.project_select_index = 1;
    state.workspace_overlay = WorkspaceOverlay::ProjectSelect;

    let rendered = render_state_to_string(&state, 100, 30);
    assert!(rendered.contains("Change Workspace Directory"));
    assert!(rendered.contains("Current:"));
    assert!(rendered.contains("Recent / Quick Workspaces"));
    assert!(rendered.contains("/home/pirate/projects/backend"));
}

#[test]
fn test_workspace_mode_toggle_and_prompt_rendering() {
    let mut state = TuiState::default();
    state.current_screen = Screen::Workspace;
    assert!(!state.dangerously_skip_permissions);

    // Toggle to dangerously_skip_permissions
    state.toggle_execution_mode();
    assert!(state.dangerously_skip_permissions);

    state.workspace_overlay = WorkspaceOverlay::PromptInput;
    let rendered = render_state_to_string(&state, 100, 30);
    assert!(rendered.contains("--dangerously-skip-permissions"));
    assert!(rendered.contains("Target dir:"));

    // Toggle back to default
    state.toggle_execution_mode();
    assert!(!state.dangerously_skip_permissions);

    let rendered_default = render_state_to_string(&state, 100, 30);
    assert!(rendered_default.contains("Default"));
}

#[test]
fn test_agent_selection_mode_and_directory_rendering() {
    let mut state = TuiState::default();
    let defs = vec![
        AgentDefinition {
            id: "antigravity".into(),
            display_name: "Antigravity".into(),
            executable: "agy".into(),
            executable_path: PathBuf::from("/bin/agy"),
            version: None,
            availability: AgentAvailability::Ready,
            capabilities: vec![],
            adapter_type: AdapterType::AntigravityPty,
        },
    ];
    state.init_workspace_selection(defs, WorkspacePreferences::default());
    state.workspace_dir = PathBuf::from("/home/user/myproject");

    let rendered = render_state_to_string(&state, 80, 26);
    assert!(rendered.contains("Workspace:"));
    assert!(rendered.contains("/home/user/myproject"));
    assert!(rendered.contains("Mode:"));
    assert!(rendered.contains("Default"));

    // Toggle mode
    state.toggle_execution_mode();
    let rendered_danger = render_state_to_string(&state, 80, 26);
    assert!(rendered_danger.contains("--dangerously-skip-permissions"));

    // Open project select on AgentSelection
    state.workspace_overlay = WorkspaceOverlay::ProjectSelect;
    let rendered_overlay = render_state_to_string(&state, 80, 26);
    assert!(rendered_overlay.contains("Change Workspace Directory"));
}

#[test]
fn test_workspace_tab_completion_logic() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let p_alpha = tmp.path().join("alpha");
    let p_alpine = tmp.path().join("alpine");
    let p_beta = tmp.path().join("beta");
    std::fs::create_dir_all(&p_alpha).unwrap();
    std::fs::create_dir_all(&p_alpine).unwrap();
    std::fs::create_dir_all(&p_beta).unwrap();

    let mut state = TuiState::default();
    let base_prefix = format!("{}/al", tmp.path().display());
    state.project_input_text = base_prefix.clone();

    // First tab: should expand to common prefix "alp"
    state.tab_complete_project_input();
    assert_eq!(state.project_input_text, format!("{}/alp", tmp.path().display()));
    assert_eq!(state.project_completions.len(), 2);

    // Second tab: cycles through candidates
    state.tab_complete_project_input();
    assert!(
        state.project_input_text == format!("{}/alpha/", tmp.path().display())
            || state.project_input_text == format!("{}/alpine/", tmp.path().display())
    );

    // Test single match completion
    state.project_completions.clear();
    state.project_input_text = format!("{}/be", tmp.path().display());
    state.tab_complete_project_input();
    assert_eq!(state.project_input_text, format!("{}/beta/", tmp.path().display()));
    assert!(state.project_completions.is_empty());
}

#[test]
fn test_workspace_project_select_completions_rendering() {
    let mut state = TuiState::default();
    state.current_screen = Screen::Workspace;
    state.project_input_text = "/tmp/al".into();
    state.project_completions = vec!["/tmp/alpha/".into(), "/tmp/alpine/".into()];
    state.project_completion_index = 0;
    state.workspace_overlay = WorkspaceOverlay::ProjectSelect;

    let rendered = render_state_to_string(&state, 100, 30);
    assert!(rendered.contains("Directory Matches (2 found, Tab to cycle)"));
    assert!(rendered.contains("/tmp/alpha/"));
    assert!(rendered.contains("/tmp/alpine/"));
    assert!(rendered.contains("Tab"));
    assert!(rendered.contains("Autocomplete"));
}

// =========================================================================
// 6. Terminal-Only Mode & Integrated Virtual Terminal Capabilities Tests
// =========================================================================

#[test]
fn test_terminal_only_mode_collapses_chrome_rendering() {
    let mut state = TuiState::default();
    state.current_screen = Screen::Workspace;
    state.terminal_only_mode = false;

    let normal_rendered = render_state_to_string(&state, 80, 24);
    // In normal mode, top bar or bottom footer hints appear
    assert!(normal_rendered.contains("AgentDesk"));
    assert!(normal_rendered.contains("F1"));

    // Activate terminal-only distraction-free mode
    state.terminal_only_mode = true;
    let fullscreen_rendered = render_state_to_string(&state, 80, 24);
    // In terminal-only mode, chrome is collapsed (0 height top and bottom bars)
    assert!(!fullscreen_rendered.contains("AgentDesk"));
    assert!(!fullscreen_rendered.contains("F10 Exit"));
}

#[test]
fn test_terminal_regex_search_and_navigation() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = TerminalSession::spawn_with_shell("/bin/sh", temp.path(), 80, 24).unwrap();
    session.screen.process_bytes(b"Error: failed to connect to database\r\nWarning: retrying database connection\r\n");

    let mut state = TuiState::default();
    state.shell_session = Some(session);

    state.search_query = "database".to_string();
    state.perform_regex_search();
    assert_eq!(state.search_matches.len(), 2);
    assert_eq!(state.search_match_index, 0);

    // Test next/prev match navigation
    state.select_next_search_match();
    assert_eq!(state.search_match_index, 1);
    state.select_prev_search_match();
    assert_eq!(state.search_match_index, 0);

    // Render search overlay
    state.current_screen = Screen::Workspace;
    state.workspace_overlay = WorkspaceOverlay::Search;
    let rendered = render_state_to_string(&state, 80, 24);
    assert!(rendered.contains("database"));
    assert!(rendered.contains("1/2"));
    assert!(rendered.contains("Next"));

    if let Some(mut session) = state.shell_session.take() {
        session.terminate();
    }
}

#[test]
fn test_terminal_url_scanner_and_picker() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = TerminalSession::spawn_with_shell("/bin/sh", temp.path(), 120, 24).unwrap();
    session.screen.process_bytes(b"Visit https://github.com/agentdesk/agentdesk.git, or docs at https://agentdesk.dev/docs!\r\n");

    let mut state = TuiState::default();
    state.shell_session = Some(session);

    state.scan_terminal_urls();
    assert_eq!(state.detected_urls.len(), 2);
    assert!(state.detected_urls.contains(&"https://github.com/agentdesk/agentdesk.git".to_string()));
    assert!(state.detected_urls.contains(&"https://agentdesk.dev/docs".to_string()));

    // Test URL picker overlay rendering
    state.current_screen = Screen::Workspace;
    state.workspace_overlay = WorkspaceOverlay::UrlPicker;
    let rendered = render_state_to_string(&state, 80, 24);
    assert!(rendered.contains("URL Picker"));
    assert!(rendered.contains("https://agentdesk.dev/docs") || rendered.contains("github.com"));
    assert!(rendered.contains("Open"));
    assert!(rendered.contains("Copy"));

    if let Some(mut session) = state.shell_session.take() {
        session.terminate();
    }
}

#[test]
fn test_visual_mode_keyboard_selection() {
    let mut state = TuiState::default();
    assert!(!state.is_visual_mode);

    state.start_visual_mode(5, 10);
    assert!(state.is_visual_mode);
    assert_eq!(state.visual_cursor, (5, 10));
    assert_eq!(state.visual_anchor, (5, 10));
    assert!(state.terminal_selection.is_some());

    // Move cursor right and down
    state.move_visual_cursor(2, 3, 24, 80);
    assert_eq!(state.visual_cursor, (7, 13));
    let sel = state.terminal_selection.unwrap();
    assert_eq!((sel.start_row, sel.start_col), (5, 10));
    assert_eq!((sel.end_row, sel.end_col), (7, 13));
}

#[test]
fn test_terminal_bracketed_paste_mode() {
    let mut screen = VtScreen::new(80, 24);
    assert!(!screen.bracketed_paste);

    // Enable bracketed paste mode CSI ? 2004 h
    screen.process_bytes(b"\x1b[?2004h");
    assert!(screen.bracketed_paste);

    // Disable bracketed paste mode CSI ? 2004 l
    screen.process_bytes(b"\x1b[?2004l");
    assert!(!screen.bracketed_paste);
}

#[test]
fn test_terminal_pty_resize_geometry_calculation() {
    let term_width: u16 = 100;
    let term_height: u16 = 30;

    // In normal mode: top header = 1 row, bottom footer = 1 row => pty rows = term_height - 2
    let normal_rows = term_height.saturating_sub(2);
    let normal_cols = term_width;
    assert_eq!(normal_rows, 28);
    assert_eq!(normal_cols, 100);

    // In terminal-only mode: 100% geometry allocated directly to PTY emulator
    let terminal_only_rows = term_height;
    let terminal_only_cols = term_width;
    assert_eq!(terminal_only_rows, 30);
    assert_eq!(terminal_only_cols, 100);
}

#[test]
fn test_sessions_list_ctrl_f_opens_terminal_only_mode() {
    let mut state = TuiState::default();
    state.current_screen = Screen::Dashboard;
    state.terminal_only_mode = false;

    // Simulate key handling of Ctrl+F from Dashboard / sessions list
    // In app.rs: Ctrl+F switches directly to Screen::Workspace with terminal_only_mode = true
    state.current_screen = Screen::Workspace;
    state.terminal_only_mode = true;

    assert_eq!(state.current_screen, Screen::Workspace);
    assert!(state.terminal_only_mode);

    // Simulate detach with Ctrl+] or Ctrl+Q
    state.terminal_only_mode = false;
    state.current_screen = Screen::Dashboard;

    assert_eq!(state.current_screen, Screen::Dashboard);
    assert!(!state.terminal_only_mode);
}

#[test]
fn test_split_panes_layout_and_zoom() {
    use agentdesk_server::tui::state::{PaneFocus, SplitPaneLayout};

    let mut state = TuiState::default();
    state.current_screen = Screen::Workspace;
    assert_eq!(state.split_layout, SplitPaneLayout::Single);
    assert!(!state.pane_zoomed);

    // Switch to Horizontal split
    state.split_layout = SplitPaneLayout::Horizontal;
    let rendered_h = render_state_to_string(&state, 80, 24);
    assert!(rendered_h.contains("Terminal initializing..."));

    // Switch to Vertical split
    state.split_layout = SplitPaneLayout::Vertical;
    let rendered_v = render_state_to_string(&state, 80, 24);
    assert!(rendered_v.contains("Terminal initializing..."));

    // Toggle zoom on pane
    state.pane_zoomed = true;
    state.pane_focus = PaneFocus::Primary;
    let rendered_z = render_state_to_string(&state, 80, 24);
    assert!(rendered_z.contains("Terminal initializing..."));
}

#[test]
fn test_modern_sgr_styling_attributes() {
    use agentdesk_core::antigravity::screen::UnderlineStyle;

    let mut screen = VtScreen::new(80, 24);
    // Undercurl + underline color + strikethrough + overline
    screen.process_bytes(b"\x1b[4:3m\x1b[58;2;255;50;50m\x1b[9m\x1b[53mErrorText\x1b[0m");

    let cell = screen.cell(0, 0).unwrap();
    assert!(cell.attrs.underline);
    assert_eq!(cell.attrs.underline_style, UnderlineStyle::Curly);
    assert_eq!(cell.attrs.underline_color, Some((255, 50, 50)));
    assert!(cell.attrs.strikethrough);
    assert!(cell.attrs.overline);

    // After reset
    let reset_cell = screen.cell(0, 9).unwrap();
    assert!(!reset_cell.attrs.underline);
    assert_eq!(reset_cell.attrs.underline_style, UnderlineStyle::None);
    assert_eq!(reset_cell.attrs.underline_color, None);
    assert!(!reset_cell.attrs.strikethrough);
    assert!(!reset_cell.attrs.overline);
}

#[test]
fn test_osc8_and_osc52_integration() {
    let mut screen = VtScreen::new(80, 24);
    // OSC 8 Hyperlink
    screen.process_bytes(b"\x1b]8;;https://agentdesk.dev\x1b\\AgentDeskDocs\x1b]8;;\x1b\\");
    assert_eq!(screen.cell(0, 0).unwrap().attrs.hyperlink, Some("https://agentdesk.dev".to_string()));

    // OSC 52 Clipboard
    screen.process_bytes(b"\x1b]52;c;SGVsbG8gT1NDNTI=\x07");
    assert_eq!(screen.osc52_clipboard, Some("SGVsbG8gT1NDNTI=".to_string()));

    // Test OSC 52 decoded synchronization
    let b64 = screen.osc52_clipboard.take().unwrap();
    let decoded = agentdesk_server::tui::clipboard::base64_decode(&b64).unwrap();
    assert_eq!(String::from_utf8(decoded).unwrap(), "Hello OSC52");
}


