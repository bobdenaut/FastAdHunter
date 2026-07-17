//! Pure domain model and shared DTOs: Query, Verdict, Client, QueryEvent (ARCHITECTURE.md L1).

mod client;
mod operating_mode;
mod query;
mod query_event;
mod verdict;

pub use client::Client;
pub use operating_mode::{OperatingMode, ParseOperatingModeError};
pub use query::{Query, QueryType};
pub use query_event::QueryEvent;
pub use verdict::{DecisiveRule, Verdict};
