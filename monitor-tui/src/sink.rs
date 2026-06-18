//! The ratatui skin's output sink.
//!
//! Agent output (from the Monty mind, a later phase) is delivered to a
//! [`RatatuiSink`] inside the session fan-out, forwarded over an mpsc channel,
//! and drained by the run loop into the transcript. `deliver` must never block
//! or touch ratatui state (it may run on any thread once the actor wrapper
//! lands) — it only forwards. This is the same discipline the egui skin's sink
//! will use.

use monitor_presence::{OutputChunk, OutputSink};
use tokio::sync::mpsc;

/// An [`OutputSink`] that forwards delivered chunks over a channel for the run
/// loop to fold into the transcript.
pub struct RatatuiSink {
    tx: mpsc::UnboundedSender<OutputChunk>,
}

impl RatatuiSink {
    /// Create the sink and the receiver the run loop drains.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<OutputChunk>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }
}

impl OutputSink for RatatuiSink {
    fn deliver(&mut self, chunk: &OutputChunk) {
        // Forward only; never block, never touch ratatui here.
        let _ = self.tx.send(chunk.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_presence::OutputStream;

    #[test]
    fn deliver_forwards_chunk_to_receiver() {
        let (mut sink, mut rx) = RatatuiSink::new();
        let chunk = OutputChunk {
            turn: 1,
            stream: OutputStream::AgentThought,
            seq: 0,
            data: "hi".into(),
            last: true,
        };
        sink.deliver(&chunk);
        assert_eq!(rx.try_recv().unwrap().data, "hi");
    }
}
