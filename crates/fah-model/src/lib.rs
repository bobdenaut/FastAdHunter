//! Pure domain model and shared DTOs: Query, Verdict, Client, QueryEvent (ARCHITECTURE.md L1).

mod client;
mod engine;
mod history;
mod http;
mod memory;
mod operating_mode;
mod perf;
mod policy;
mod protocol;
mod query;
mod query_event;
mod request_event;
mod verdict;

pub use client::Client;
pub use engine::{
    AnswerCounters, CacheCleanupCounters, DnsCounters, DnsLatency, EngineCounters, EngineTelemetry,
    HttpCounters, HttpLatency, LatencyTotals, RulesetInfo, StageTotals, SwrCounters,
};
pub use history::{
    ClientHits, DailyTopN, DomainHits, HistoryPoint, HistoryRange, HistoryResolution,
    HistorySeries, HourRollup, TopItems, TopKind,
};
pub use http::{HttpRequest, ResourceType};
pub use memory::{AllocatorStats, MemoryBreakdown, MemoryComponents, ProcessStats, StatsHeap};
pub use operating_mode::{OperatingMode, ParseOperatingModeError};
pub use perf::{CacheStatsSample, LatencySummary, PerfSample, PerfSeries, UpstreamSample};
pub use policy::{Assignment, ClientSelector, Policy, PolicyId, Schedule};
pub use protocol::Protocol;
pub use query::{Query, QueryType};
pub use query_event::{AnswerOutcome, QueryEvent, StaleServe};
pub use request_event::{Event, EventKind, Request, RequestEvent};
pub use verdict::{DecisiveRule, Verdict};
