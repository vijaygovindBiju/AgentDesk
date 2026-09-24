pub mod agent_selection;
pub mod agents;
pub mod completed;
pub mod connection;
pub mod dashboard;
pub mod devices;
pub mod errors;
pub mod footer;
pub mod header;
pub mod help;
pub mod history;
pub mod logs;
pub mod pairing;
pub mod projects;
pub mod requests;
pub mod working;
pub mod workspace;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

use crate::tui::state::{Screen, TuiState};

pub fn render_ui(f: &mut Frame, state: &TuiState) {
    let size = f.area();

    // Small terminal safeguard: minimum 20x6 to render anything
    if size.width < 20 || size.height < 6 {
        return;
    }

    match state.current_screen {
        Screen::AgentSelection => {
            agent_selection::render_agent_selection(f, size, state);
            return;
        }
        Screen::Workspace => {
            workspace::render_workspace(f, size, state);
            return;
        }
        _ => {}
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Header Bar
            Constraint::Min(8),    // Body Area
            Constraint::Length(1), // Bottom Shortcuts / Footer
        ])
        .split(size);

    // 1. Header
    header::render_header(f, chunks[0], state);

    // 2. Active Screen Body
    let body_area = chunks[1];
    match state.current_screen {
        Screen::Dashboard => dashboard::render_dashboard(f, body_area, state),
        Screen::Agents => agents::render_agents(f, body_area, state),
        Screen::AgentDetail => agents::render_agent_detail(f, body_area, state),
        Screen::Requests => requests::render_requests(f, body_area, state),
        Screen::Errors => errors::render_errors(f, body_area, state),
        Screen::ErrorDetail => errors::render_error_detail(f, body_area, state),
        Screen::Working => working::render_working(f, body_area, state),
        Screen::Completed => completed::render_completed(f, body_area, state),
        Screen::Projects => projects::render_projects(f, body_area, state),
        Screen::ProjectDetail => projects::render_projects(f, body_area, state),
        Screen::Devices => devices::render_devices(f, body_area, state),
        Screen::Connection => connection::render_connection(f, body_area, state),
        Screen::History => history::render_history(f, body_area, state),
        Screen::Logs => logs::render_logs(f, body_area, state),
        Screen::Pairing => {
            // Render dashboard behind the modal popup
            dashboard::render_dashboard(f, body_area, state);
            pairing::render_pairing(f, body_area, state);
        }
        Screen::Help => {
            // Render dashboard behind help overlay
            dashboard::render_dashboard(f, body_area, state);
            help::render_help(f, body_area, state);
        }
        Screen::AgentSelection | Screen::Workspace => unreachable!(),
    }

    // 3. Footer
    footer::render_footer(f, chunks[2], state);
}
