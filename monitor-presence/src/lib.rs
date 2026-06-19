//! `monitor-presence` — the frontend-agnostic core that every monitor-agent
//! skin renders and acts through.
//!
//! This crate holds the canonical [`PresenceModel`] (pure `monitor-core`
//! types, swarm-free), the data reducer [`PresenceModel::apply`], the
//! frontend-neutral [`Intent`] input contract, and [`MontyPresence`] — the
//! object a ratatui or egui skin attaches to. One mind, one state, two skins.
//!
//! See `docs/design/inhabit-both-surfaces.md`.

mod event;
mod intent;
mod model;
mod presence;
pub mod session;

pub use event::DataEvent;
pub use intent::Intent;
pub use model::{ChatMessage, PresenceModel, Tab};
pub use presence::MontyPresence;
pub use session::{AttachId, AttachRole, OutputChunk, OutputSink, OutputStream, SessionState};
