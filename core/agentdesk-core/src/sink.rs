//! TransportSink trait and test implementations.
//! All outbound wire messages pass through a `TransportSink`.
//! See docs/ARCHITECTURE.md and docs/SYSTEM_DESIGN.md.

use agentdesk_model::{Body, Message};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinkError {
    ChannelFull,
    Closed,
    Io(String),
}

impl std::fmt::Display for SinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SinkError::ChannelFull => write!(f, "outbound channel full"),
            SinkError::Closed => write!(f, "sink closed"),
            SinkError::Io(e) => write!(f, "sink io error: {e}"),
        }
    }
}

impl std::error::Error for SinkError {}

pub trait TransportSink: Send {
    /// Send a message to the sink. Returns the number of UTF-8 JSON payload bytes written.
    fn send(&mut self, message: &Message) -> Result<usize, SinkError>;
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// A sink that records all sent messages and counts bytes. Used in tests.
#[derive(Debug, Default, Clone)]
pub struct VecSink {
    pub messages: Vec<Message>,
    pub bytes_written: usize,
    pub events_count: usize,
}

impl VecSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }
}

impl TransportSink for VecSink {
    fn send(&mut self, message: &Message) -> Result<usize, SinkError> {
        let json_bytes = serde_json::to_vec(message).map_err(|e| SinkError::Io(e.to_string()))?;
        let len = json_bytes.len();
        self.bytes_written += len;
        if matches!(message.body, Body::Event(_) | Body::RawEvent(_)) {
            self.events_count += 1;
        }
        self.messages.push(message.clone());
        Ok(len)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A sink that only counts messages and bytes without retaining them in memory.
#[derive(Debug, Default, Clone)]
pub struct CountingSink {
    pub messages_count: usize,
    pub bytes_written: usize,
    pub events_count: usize,
}

impl CountingSink {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TransportSink for CountingSink {
    fn send(&mut self, message: &Message) -> Result<usize, SinkError> {
        let json_bytes = serde_json::to_vec(message).map_err(|e| SinkError::Io(e.to_string()))?;
        let len = json_bytes.len();
        self.messages_count += 1;
        self.bytes_written += len;
        if matches!(message.body, Body::Event(_) | Body::RawEvent(_)) {
            self.events_count += 1;
        }
        Ok(len)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// An async channel sink for forwarding to a per-connection task.
pub struct ChannelSink {
    sender: tokio::sync::mpsc::Sender<Message>,
}

impl ChannelSink {
    pub fn new(sender: tokio::sync::mpsc::Sender<Message>) -> Self {
        ChannelSink { sender }
    }
}

impl TransportSink for ChannelSink {
    fn send(&mut self, message: &Message) -> Result<usize, SinkError> {
        let json_bytes = serde_json::to_vec(message).map_err(|e| SinkError::Io(e.to_string()))?;
        let len = json_bytes.len();
        self.sender.try_send(message.clone()).map_err(|e| match e {
            tokio::sync::mpsc::error::TrySendError::Full(_) => SinkError::ChannelFull,
            tokio::sync::mpsc::error::TrySendError::Closed(_) => SinkError::Closed,
        })?;
        Ok(len)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentdesk_model::{Body, Empty, Message};

    #[test]
    fn vec_sink_records_messages_and_bytes() {
        let mut sink = VecSink::new();
        let msg = Message::push(Body::GetMetrics(Empty {}));
        let bytes = sink.send(&msg).unwrap();
        assert!(bytes > 0);
        assert_eq!(sink.len(), 1);
        assert_eq!(sink.bytes_written, bytes);
        assert_eq!(sink.events_count, 0);
    }

    #[test]
    fn counting_sink_accumulates_counts() {
        let mut sink = CountingSink::new();
        let msg = Message::push(Body::GetMetrics(Empty {}));
        let b1 = sink.send(&msg).unwrap();
        let b2 = sink.send(&msg).unwrap();
        assert_eq!(sink.messages_count, 2);
        assert_eq!(sink.bytes_written, b1 + b2);
    }
}
