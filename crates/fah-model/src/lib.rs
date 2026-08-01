//! Pure domain model and shared DTOs: Query, Verdict, Client, QueryEvent (ARCHITECTURE.md L1).

mod client;
mod history;
mod http;
mod memory;
mod operating_mode;
mod perf;
mod policy;
mod query;
mod query_event;
mod request_event;
mod verdict;

pub use client::Client;
pub use history::{
    ClientHits, DailyTopN, DomainHits, HistoryPoint, HistoryRange, HistoryResolution,
    HistorySeries, HourRollup, TopItems, TopKind,
};
pub use http::{HttpRequest, ResourceType};
pub use memory::{AllocatorStats, MemoryBreakdown, StatsHeap};
pub use operating_mode::{OperatingMode, ParseOperatingModeError};
pub use perf::{CacheStatsSample, LatencySummary, PerfSample, PerfSeries, UpstreamSample};
pub use policy::{Assignment, ClientSelector, Policy, PolicyId, Schedule};
pub use query::{Query, QueryType};
pub use query_event::QueryEvent;
pub use request_event::{Event, EventKind, Request, RequestEvent};
pub use verdict::{DecisiveRule, Verdict};
