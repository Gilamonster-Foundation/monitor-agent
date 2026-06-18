//! A lean, in-process output **fan-out** — one session, many attachments.
//!
//! The same [`OutputChunk`] stream fans out to every attachment, so a ratatui
//! skin and an egui skin (and, later, a mesh peer) all see the same agent
//! turn. Two structural invariants:
//!
//! * only a [`AttachRole::Driver`] may submit input — an [`AttachRole::Observer`]
//!   can never mutate, enforced by the *role*, not by anyone's caveats;
//! * turns are serialized — a turn in flight must complete or cancel before the
//!   next input is accepted.
//!
//! The API shape deliberately mirrors `newt_core::session::SessionState`
//! (docs/design/inhabit-both-surfaces.md §2.4) so the standalone-repo split can
//! swap this lean, dependency-free version for newt-core's verbatim. What is
//! deferred to that swap (and to wiring the Monty mind as the Driver): the
//! `Caveats` authority each attachment carries and `effective_caveats()`. This
//! phase needs only the role invariant, which is caveat-free.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

/// Which logical stream an [`OutputChunk`] belongs to (a skin styles each
/// distinctly — stdout vs. the agent's thoughts vs. a tool call vs. a diff).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
    AgentThought,
    ToolCall,
    Diff,
}

/// One streamed unit of session output, fanned out to every attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputChunk {
    /// Which turn produced this chunk (monotonic within a session).
    pub turn: u64,
    pub stream: OutputStream,
    /// Ordering within the turn; a reconnecting attachment asks for
    /// [`SessionState::replay_from`] by this.
    pub seq: u64,
    pub data: String,
    /// Final chunk of this turn's stream.
    pub last: bool,
}

/// How an attachment relates to its session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachRole {
    /// May submit input (drive turns).
    Driver,
    /// Read-only: receives the output stream, can never submit input.
    Observer,
}

/// Identifies one attachment within a session (assigned at attach time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct AttachId(u64);

impl fmt::Display for AttachId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "attach#{}", self.0)
    }
}

/// The sink an attachment receives [`OutputChunk`]s on. A ratatui skin, an egui
/// skin, or a test collector all implement this, so the session fans out to
/// `dyn OutputSink` and never knows the surface.
pub trait OutputSink: Send {
    fn deliver(&mut self, chunk: &OutputChunk);
}

/// Why [`SessionState::submit_input`] refused an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRefused {
    /// The attachment is an `Observer` — only a `Driver` may submit.
    NotADriver,
    /// A turn is already in flight; it must complete or cancel first.
    TurnInFlight,
    /// No such attachment in this session.
    NoSuchAttachment,
}

impl fmt::Display for InputRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADriver => {
                write!(
                    f,
                    "attachment is an observer; only a driver may submit input"
                )
            }
            Self::TurnInFlight => {
                write!(
                    f,
                    "a turn is already in flight; complete or cancel it first"
                )
            }
            Self::NoSuchAttachment => write!(f, "no such attachment in this session"),
        }
    }
}

impl std::error::Error for InputRefused {}

struct Attachment {
    role: AttachRole,
    sink: Box<dyn OutputSink>,
}

/// A turn accepted for execution: the harness runs it and streams output back
/// via [`SessionState::emit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedTurn {
    pub turn: u64,
    pub driver: AttachId,
    pub text: String,
}

struct InFlight {
    turn: u64,
    driver: AttachId,
    next_seq: u64,
}

/// One session: a set of attachments fanned the same output, with serialized
/// input.
pub struct SessionState {
    attachments: BTreeMap<AttachId, Attachment>,
    next_attach: u64,
    turn: u64,
    in_flight: Option<InFlight>,
    ring: VecDeque<OutputChunk>,
    ring_cap: usize,
}

impl SessionState {
    /// `ring_cap` bounds the resume buffer (recent chunks replayed to an
    /// attachment that reconnects). Clamped to at least 1.
    pub fn new(ring_cap: usize) -> Self {
        Self {
            attachments: BTreeMap::new(),
            next_attach: 0,
            turn: 0,
            in_flight: None,
            ring: VecDeque::new(),
            ring_cap: ring_cap.max(1),
        }
    }

    /// Attach a skin/peer/observer; returns its handle.
    pub fn attach(&mut self, role: AttachRole, sink: Box<dyn OutputSink>) -> AttachId {
        let id = AttachId(self.next_attach);
        self.next_attach += 1;
        self.attachments.insert(id, Attachment { role, sink });
        id
    }

    /// Detach an attachment; future output no longer fans out to it. Returns
    /// whether it was present.
    pub fn detach(&mut self, id: AttachId) -> bool {
        self.attachments.remove(&id).is_some()
    }

    pub fn attachment_count(&self) -> usize {
        self.attachments.len()
    }

    pub fn driver_count(&self) -> usize {
        self.attachments
            .values()
            .filter(|a| a.role == AttachRole::Driver)
            .count()
    }

    pub fn turn_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    /// The driver of the in-flight turn, or `None` when idle. (At P6 this is
    /// the seam where the active driver's `Caveats` are looked up for
    /// `effective_caveats`.)
    pub fn active_driver(&self) -> Option<AttachId> {
        self.in_flight.as_ref().map(|f| f.driver)
    }

    /// Submit a human/peer input. Enforces the structural invariants:
    /// driver-only, and one turn at a time. On accept, opens a new turn.
    pub fn submit_input(
        &mut self,
        from: AttachId,
        text: impl Into<String>,
    ) -> Result<AcceptedTurn, InputRefused> {
        let att = self
            .attachments
            .get(&from)
            .ok_or(InputRefused::NoSuchAttachment)?;
        if att.role != AttachRole::Driver {
            return Err(InputRefused::NotADriver);
        }
        if self.in_flight.is_some() {
            return Err(InputRefused::TurnInFlight);
        }
        self.turn += 1;
        self.in_flight = Some(InFlight {
            turn: self.turn,
            driver: from,
            next_seq: 0,
        });
        Ok(AcceptedTurn {
            turn: self.turn,
            driver: from,
            text: text.into(),
        })
    }

    /// Emit one output chunk for the in-flight turn: buffer it (bounded by
    /// `ring_cap`) and fan it out to every attachment. No-op if idle.
    pub fn emit(&mut self, stream: OutputStream, data: impl Into<String>, last: bool) {
        let Some(f) = self.in_flight.as_mut() else {
            return;
        };
        let chunk = OutputChunk {
            turn: f.turn,
            stream,
            seq: f.next_seq,
            data: data.into(),
            last,
        };
        f.next_seq += 1;
        if self.ring.len() == self.ring_cap {
            self.ring.pop_front();
        }
        self.ring.push_back(chunk.clone());
        for att in self.attachments.values_mut() {
            att.sink.deliver(&chunk);
        }
    }

    /// End the in-flight turn (success). Returns whether one was in flight.
    pub fn complete_turn(&mut self) -> bool {
        self.in_flight.take().is_some()
    }

    /// Cancel the in-flight turn. Returns whether one was in flight.
    pub fn cancel_turn(&mut self) -> bool {
        self.in_flight.take().is_some()
    }

    /// Replay buffered chunks with `seq >= from_seq` to a reconnecting
    /// attachment. Bounded by `ring_cap`, so a long-gone attachment gets the
    /// retained tail.
    pub fn replay_from(&self, from_seq: u64) -> Vec<OutputChunk> {
        self.ring
            .iter()
            .filter(|c| c.seq >= from_seq)
            .cloned()
            .collect()
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A test sink that records every delivered chunk.
    #[derive(Clone, Default)]
    struct Collector(Arc<Mutex<Vec<OutputChunk>>>);
    impl OutputSink for Collector {
        fn deliver(&mut self, chunk: &OutputChunk) {
            self.0.lock().unwrap().push(chunk.clone());
        }
    }
    impl Collector {
        fn count(&self) -> usize {
            self.0.lock().unwrap().len()
        }
        fn last_data(&self) -> Option<String> {
            self.0.lock().unwrap().last().map(|c| c.data.clone())
        }
    }

    #[test]
    fn observer_cannot_submit_input() {
        let mut s = SessionState::new(16);
        let obs = s.attach(AttachRole::Observer, Box::new(Collector::default()));
        assert_eq!(s.submit_input(obs, "hi"), Err(InputRefused::NotADriver));
    }

    #[test]
    fn unknown_attachment_is_refused() {
        let mut s = SessionState::new(16);
        // AttachId is opaque; a detached id is unknown.
        let id = s.attach(AttachRole::Driver, Box::new(Collector::default()));
        s.detach(id);
        assert_eq!(
            s.submit_input(id, "hi"),
            Err(InputRefused::NoSuchAttachment)
        );
    }

    #[test]
    fn driver_emit_fans_out_to_every_attachment() {
        let mut s = SessionState::new(16);
        let a = Collector::default();
        let b = Collector::default();
        let driver = s.attach(AttachRole::Driver, Box::new(a.clone()));
        s.attach(AttachRole::Observer, Box::new(b.clone()));

        s.submit_input(driver, "go").unwrap();
        s.emit(OutputStream::AgentThought, "thinking", false);
        s.emit(OutputStream::Stdout, "done", true);
        s.complete_turn();

        assert_eq!(a.count(), 2, "driver sink receives both chunks");
        assert_eq!(b.count(), 2, "observer sink receives the same two chunks");
        assert_eq!(b.last_data().as_deref(), Some("done"));
    }

    #[test]
    fn emit_while_idle_is_a_no_op() {
        let mut s = SessionState::new(16);
        let c = Collector::default();
        s.attach(AttachRole::Observer, Box::new(c.clone()));
        s.emit(OutputStream::Stdout, "nope", true); // no turn in flight
        assert_eq!(c.count(), 0);
    }

    #[test]
    fn one_turn_at_a_time() {
        let mut s = SessionState::new(16);
        let d = s.attach(AttachRole::Driver, Box::new(Collector::default()));
        s.submit_input(d, "first").unwrap();
        assert_eq!(s.submit_input(d, "second"), Err(InputRefused::TurnInFlight));
        s.complete_turn();
        assert!(s.submit_input(d, "third").is_ok());
    }

    #[test]
    fn detach_stops_fanout() {
        let mut s = SessionState::new(16);
        let d = s.attach(AttachRole::Driver, Box::new(Collector::default()));
        let c = Collector::default();
        let obs = s.attach(AttachRole::Observer, Box::new(c.clone()));
        s.submit_input(d, "go").unwrap();
        s.emit(OutputStream::Stdout, "one", false);
        assert_eq!(c.count(), 1);
        s.detach(obs);
        s.emit(OutputStream::Stdout, "two", true);
        assert_eq!(c.count(), 1, "detached observer receives nothing further");
    }

    #[test]
    fn replay_returns_the_buffered_tail() {
        let mut s = SessionState::new(2); // tiny ring
        let d = s.attach(AttachRole::Driver, Box::new(Collector::default()));
        s.submit_input(d, "go").unwrap();
        s.emit(OutputStream::Stdout, "a", false); // seq 0 (evicted)
        s.emit(OutputStream::Stdout, "b", false); // seq 1
        s.emit(OutputStream::Stdout, "c", true); // seq 2
        let tail = s.replay_from(0);
        assert_eq!(tail.len(), 2, "ring cap 2 retains only the last two");
        assert_eq!(tail[0].data, "b");
        assert_eq!(tail[1].data, "c");
    }

    #[test]
    fn counts_and_in_flight_track_state() {
        let mut s = SessionState::new(16);
        assert_eq!(s.attachment_count(), 0);
        let d = s.attach(AttachRole::Driver, Box::new(Collector::default()));
        s.attach(AttachRole::Observer, Box::new(Collector::default()));
        assert_eq!(s.attachment_count(), 2);
        assert_eq!(s.driver_count(), 1);
        assert!(!s.turn_in_flight());
        assert_eq!(s.active_driver(), None);
        s.submit_input(d, "go").unwrap();
        assert!(s.turn_in_flight());
        assert_eq!(s.active_driver(), Some(d));
        assert!(s.cancel_turn());
        assert!(!s.turn_in_flight());
        assert_eq!(s.active_driver(), None);
    }
}
