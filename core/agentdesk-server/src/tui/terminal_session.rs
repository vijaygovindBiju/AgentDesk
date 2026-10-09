//! Real POSIX PTY shell session host and VT100 screen emulator integration.
//! Hosts the user's interactive shell ($SHELL) inside a real pseudo-terminal.

use std::io;
use std::path::{Path, PathBuf};

use std::time::Duration;

use agentdesk_core::{PtySession, Screen};

/// Inspect bytes written by the child process to detect standard terminal attribute / status queries,
/// generating appropriate ANSI/VT100 terminal responses (e.g. Primary Device Attributes DA1 for fish).
fn handle_terminal_queries(bytes: &[u8], screen: &Screen) -> Vec<u8> {
    let mut replies = Vec::new();

    // Primary Device Attributes (DA1) query: ESC [ c or ESC [ 0 c
    if bytes.windows(3).any(|w| w == b"\x1b[c")
        || bytes.windows(4).any(|w| w == b"\x1b[0c")
    {
        replies.extend_from_slice(b"\x1b[?1;2c");
    }

    // Secondary Device Attributes (DA2) query: ESC [ > c or ESC [ > 0 c
    if bytes.windows(4).any(|w| w == b"\x1b[>c")
        || bytes.windows(5).any(|w| w == b"\x1b[>0c")
    {
        replies.extend_from_slice(b"\x1b[>0;10;0c");
    }

    // Device Status Report (DSR) query: ESC [ 5 n
    if bytes.windows(4).any(|w| w == b"\x1b[5n") {
        replies.extend_from_slice(b"\x1b[0n");
    }

    // Cursor Position Report (CPR) query: ESC [ 6 n
    if bytes.windows(4).any(|w| w == b"\x1b[6n") {
        let snap = screen.snapshot();
        let r = snap.cursor_row + 1;
        let c = snap.cursor_col + 1;
        replies.extend_from_slice(format!("\x1b[{};{}R", r, c).as_bytes());
    }

    replies
}

/// An active interactive terminal session backed by an OS pseudo-terminal.
pub struct TerminalSession {
    /// Low-level POSIX PTY session controlling the shell process.
    pub pty: PtySession,
    /// VT100 / ANSI screen emulator maintaining the 2D character grid and styling.
    pub screen: Screen,
    /// Shell program or command name.
    pub shell: String,
    /// Working directory the shell was spawned in.
    pub cwd: PathBuf,
}

impl TerminalSession {
    /// Spawn the user's default shell in the specified directory.
    pub fn spawn_shell(cwd: &Path, cols: u16, rows: u16) -> io::Result<Self> {
        let shell = std::env::var("AGENTDESK_SHELL")
            .or_else(|_| std::env::var("SHELL"))
            .unwrap_or_else(|_| {
                if Path::new("/bin/bash").exists() {
                    "/bin/bash".into()
                } else {
                    "/bin/sh".into()
                }
            });

        Self::spawn_with_shell(&shell, cwd, cols, rows)
    }

    /// Spawn a specific shell or command executable in the specified directory.
    pub fn spawn_with_shell(shell: &str, cwd: &Path, cols: u16, rows: u16) -> io::Result<Self> {
        let pty = PtySession::spawn(shell, &[], Some(&cwd.to_path_buf()), cols, rows)?;
        let screen = Screen::new(cols, rows);

        let mut session = Self {
            pty,
            screen,
            shell: shell.to_string(),
            cwd: cwd.to_path_buf(),
        };

        // Give shell brief startup window to emit initial DA queries and answer them
        std::thread::sleep(Duration::from_millis(60));
        let _ = session.pump_output();

        Ok(session)
    }

    /// Send raw keystrokes or byte sequences to the shell PTY.
    pub fn send_input(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.pty.send_input(bytes)
    }

    /// Read any available chunks from the PTY master, respond to terminal queries, and update the VT100 screen.
    pub fn pump_output(&mut self) -> bool {
        let mut received = false;
        let mut query_replies = Vec::new();

        while let Ok(Some(chunk)) = self.pty.try_recv() {
            received = true;
            let rep = handle_terminal_queries(&chunk.bytes, &self.screen);
            query_replies.extend(rep);
            self.screen.process_bytes(&chunk.bytes);
        }

        if !query_replies.is_empty() {
            let _ = self.pty.send_input(&query_replies);
        }

        received
    }

    /// Dynamically resize the terminal window geometry.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        if self.screen.cols != cols || self.screen.rows != rows {
            let _ = self.pty.resize(cols, rows);
            self.screen.resize(cols, rows);
        }
    }

    /// Check if the shell child process is still alive.
    pub fn is_alive(&self) -> bool {
        self.pty.is_alive()
    }

    /// Terminate the shell child process.
    pub fn terminate(&mut self) {
        self.pty.terminate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_spawn_shell_and_pump_output() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = TerminalSession::spawn_shell(temp.path(), 80, 24).unwrap();

        assert!(session.is_alive());

        // Wait brief instant for shell prompt to render
        std::thread::sleep(Duration::from_millis(50));
        let _ = session.pump_output();

        // Send a simple echo command
        session.send_input(b"echo 'AGENTDESK_TEST_ECHO_TOKEN'\r").unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut found = false;
        while std::time::Instant::now() < deadline {
            session.pump_output();
            let all = session.screen.all_lines().join("\n");
            if all.contains("AGENTDESK_TEST_ECHO_TOKEN") {
                found = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(found, "Shell did not echo test token in screen output");

        // Test resize
        session.resize(100, 30);
        assert_eq!(session.screen.cols, 100);
        assert_eq!(session.screen.rows, 30);

        session.terminate();
    }
}
