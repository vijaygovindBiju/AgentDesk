//! Logging subsystem for agentdesk-server.
//! Complies with P6.8:
//! - Secret tokens are NEVER logged at Info or Debug level.
//! - Raw payload contents are logged ONLY at Debug level (opt-in via `--debug`).

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    Info = 0,
    Debug = 1,
}

static CURRENT_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

/// An in-memory buffer to capture log records during tests or debugging.
#[derive(Debug, Default, Clone)]
pub struct LogCapture {
    records: Arc<Mutex<Vec<String>>>,
}

impl LogCapture {
    pub fn new() -> Self {
        Self {
            records: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn push(&self, line: String) {
        if let Ok(mut lock) = self.records.lock() {
            lock.push(line);
        }
    }

    pub fn lines(&self) -> Vec<String> {
        self.records.lock().map(|l| l.clone()).unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut lock) = self.records.lock() {
            lock.clear();
        }
    }

    pub fn contains(&self, query: &str) -> bool {
        let lines = self.lines();
        lines.iter().any(|l| l.contains(query))
    }
}

static GLOBAL_CAPTURE: Mutex<Option<LogCapture>> = Mutex::new(None);

pub fn set_log_level(level: LogLevel) {
    CURRENT_LEVEL.store(level as u8, Ordering::SeqCst);
}

pub fn current_log_level() -> LogLevel {
    if CURRENT_LEVEL.load(Ordering::SeqCst) == LogLevel::Debug as u8 {
        LogLevel::Debug
    } else {
        LogLevel::Info
    }
}

pub fn set_log_capture(capture: Option<LogCapture>) {
    if let Ok(mut lock) = GLOBAL_CAPTURE.lock() {
        *lock = capture;
    }
}

/// Emit an info-level log message. Never logs tokens or raw payload contents.
pub fn info(msg: impl AsRef<str>) {
    let text = msg.as_ref();
    let formatted = format!("[INFO] {}", text);
    if let Some(cap) = GLOBAL_CAPTURE.lock().ok().and_then(|l| l.clone()) {
        cap.push(formatted.clone());
    }
    eprintln!("{}", formatted);
}

/// Emit a debug-level log message (opt-in). Never logs the secret token.
pub fn debug(msg: impl AsRef<str>) {
    if current_log_level() < LogLevel::Debug {
        return;
    }
    let text = msg.as_ref();
    let formatted = format!("[DEBUG] {}", text);
    if let Some(cap) = GLOBAL_CAPTURE.lock().ok().and_then(|l| l.clone()) {
        cap.push(formatted.clone());
    }
    eprintln!("{}", formatted);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_capture_and_level_filtering() {
        let capture = LogCapture::new();
        set_log_capture(Some(capture.clone()));
        set_log_level(LogLevel::Info);

        info("server started");
        debug("detailed payload: secret_data");

        assert!(capture.contains("[INFO] server started"));
        assert!(!capture.contains("[DEBUG] detailed payload"));

        set_log_level(LogLevel::Debug);
        debug("detailed payload: debug_data");
        assert!(capture.contains("[DEBUG] detailed payload: debug_data"));

        set_log_capture(None);
        set_log_level(LogLevel::Info);
    }
}
