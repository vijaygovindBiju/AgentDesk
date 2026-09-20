//! POSIX PTY session manager for Antigravity experimentation.
//!
//! Spawns processes inside a true pseudo-terminal, provides controlling terminal semantics,
//! deterministic window sizing, timed chunk capture, and thread-safe input injection.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// A single timestamped chunk captured from the PTY master.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtyChunk {
    /// Milliseconds elapsed since the PTY session started.
    pub timestamp_ms: u64,
    /// Raw bytes emitted by the child process to the terminal.
    pub bytes: Vec<u8>,
}

/// Complete recording of a PTY session for offline replay and regression testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyRecording {
    pub session_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub initial_cols: u16,
    pub initial_rows: u16,
    pub chunks: Vec<PtyChunk>,
    pub exit_code: Option<i32>,
    pub total_duration_ms: u64,
}

impl PtyRecording {
    pub fn new(
        session_id: impl Into<String>,
        command: impl Into<String>,
        args: Vec<String>,
        cwd: impl Into<String>,
        initial_cols: u16,
        initial_rows: u16,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            command: command.into(),
            args,
            cwd: cwd.into(),
            initial_cols,
            initial_rows,
            chunks: Vec::new(),
            exit_code: None,
            total_duration_ms: 0,
        }
    }

    pub fn save_to_file(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    pub fn load_from_file(path: &Path) -> io::Result<Self> {
        let data = std::fs::read_to_string(path)?;
        serde_json::from_str(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

/// Active PTY session controlling a child process.
pub struct PtySession {
    master_fd: RawFd,
    writer: Arc<Mutex<File>>,
    child: Arc<Mutex<Child>>,
    rx: Receiver<PtyChunk>,
    alive: Arc<AtomicBool>,
    start_time: Instant,
    recording: PtyRecording,
}

impl PtySession {
    /// Open a new PTY pair and spawn the specified command inside it.
    pub fn spawn(
        command: &str,
        args: &[String],
        cwd: Option<&PathBuf>,
        cols: u16,
        rows: u16,
    ) -> io::Result<Self> {
        let mut master: libc::c_int = 0;
        let mut slave: libc::c_int = 0;
        let win = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // Open pseudo-terminal pair with deterministic window geometry
        let res = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &win,
            )
        };
        if res != 0 {
            return Err(io::Error::last_os_error());
        }

        let slave_file_stdin = unsafe { File::from_raw_fd(slave) };
        let slave_file_stdout = slave_file_stdin.try_clone()?;
        let slave_file_stderr = slave_file_stdin.try_clone()?;

        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::from(slave_file_stdin))
            .stdout(Stdio::from(slave_file_stdout))
            .stderr(Stdio::from(slave_file_stderr));

        // Standard terminal environment
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLUMNS", cols.to_string());
        cmd.env("LINES", rows.to_string());

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        // Establish slave as controlling terminal in child process
        unsafe {
            cmd.pre_exec(move || {
                libc::setsid();
                libc::ioctl(0, libc::TIOCSCTTY, 0);
                Ok(())
            });
        }

        let child = cmd.spawn()?;
        // Slave is now held by child and safely closed when child exits

        let master_reader = unsafe { File::from_raw_fd(master) };
        let master_writer = master_reader.try_clone()?;

        let (tx, rx) = channel();
        let alive = Arc::new(AtomicBool::new(true));
        let alive_thread = alive.clone();
        let start_time = Instant::now();

        // Background reader thread capturing timestamped chunks from PTY master
        thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut reader = master_reader;
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let elapsed = start_time.elapsed().as_millis() as u64;
                            let chunk = PtyChunk {
                                timestamp_ms: elapsed,
                                bytes: buf[..n].to_vec(),
                            };
                            if tx.send(chunk).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            // EIO is expected on Linux when slave closes
                            if e.raw_os_error() == Some(libc::EIO) {
                                break;
                            }
                            break;
                        }
                    }
                }
                alive_thread.store(false, Ordering::SeqCst);
            })?;

        let recording = PtyRecording::new(
            format!("session-{}", start_time.elapsed().as_micros()),
            command,
            args.to_vec(),
            cwd.map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".into()),
            cols,
            rows,
        );

        Ok(Self {
            master_fd: master,
            writer: Arc::new(Mutex::new(master_writer)),
            child: Arc::new(Mutex::new(child)),
            rx,
            alive,
            start_time,
            recording,
        })
    }

    /// Read any new chunks received from the PTY master without blocking.
    pub fn try_recv(&mut self) -> Result<Option<PtyChunk>, io::Error> {
        match self.rx.try_recv() {
            Ok(chunk) => {
                self.recording.chunks.push(chunk.clone());
                Ok(Some(chunk))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                self.alive.store(false, Ordering::SeqCst);
                Ok(None)
            }
        }
    }

    /// Send raw keystrokes / bytes to the child process via the PTY master.
    pub fn send_input(&mut self, bytes: &[u8]) -> io::Result<()> {
        if !self.is_alive() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Child process is dead",
            ));
        }
        let mut w = self.writer.lock().unwrap();
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    /// Dynamically resize the PTY window geometry (triggers SIGWINCH in child).
    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        let win = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let res = unsafe { libc::ioctl(self.master_fd, libc::TIOCSWINSZ, &win) };
        if res == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Check if child process is still alive.
    pub fn is_alive(&self) -> bool {
        if !self.alive.load(Ordering::SeqCst) {
            return false;
        }
        if let Ok(mut child) = self.child.lock() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.alive.store(false, Ordering::SeqCst);
                    false
                }
                Ok(None) => true,
                Err(_) => {
                    self.alive.store(false, Ordering::SeqCst);
                    false
                }
            }
        } else {
            false
        }
    }

    /// Wait up to `timeout` for child process termination and return exit code.
    pub fn wait_for_exit(&mut self, timeout: Duration) -> Option<i32> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let status = self
                .child
                .lock()
                .ok()
                .and_then(|mut child| child.try_wait().ok().flatten());
            if let Some(status) = status {
                let code = status.code().unwrap_or(-1);
                self.recording.exit_code = Some(code);
                self.recording.total_duration_ms = self.start_time.elapsed().as_millis() as u64;
                self.alive.store(false, Ordering::SeqCst);
                return Some(code);
            }
            thread::sleep(Duration::from_millis(50));
        }
        None
    }

    /// Terminate child process (SIGTERM, then SIGKILL if uncooperative).
    pub fn terminate(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Export recording of all captured chunks and metadata.
    pub fn recording(&self) -> &PtyRecording {
        &self.recording
    }

    pub fn recording_mut(&mut self) -> &mut PtyRecording {
        &mut self.recording
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.terminate();
    }
}
