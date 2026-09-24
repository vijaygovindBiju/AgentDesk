//! Real POSIX PTY shell session host and VT100 screen emulator integration.
//! Hosts the user's interactive shell ($SHELL) inside a real pseudo-terminal.

use std::io;
use std::path::{Path, PathBuf};

use agentdesk_core::{PtySession, Screen};

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

        Ok(Self {
            pty,
            screen,
            shell: shell.to_string(),
            cwd: cwd.to_path_buf(),
        })
    }

    /// Send raw keystrokes or byte sequences to the shell PTY.
    pub fn send_input(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.pty.send_input(bytes)
    }

    /// Read any available chunks from the PTY master and update the VT100 screen.
    pub fn pump_output(&mut self) -> bool {
        let mut received = false;
        while let Ok(Some(chunk)) = self.pty.try_recv() {
            received = true;
            self.screen.process_bytes(&chunk.bytes);
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
