//! Pure domain model and shared DTOs: Query, Verdict, Client, QueryEvent (ARCHITECTURE.md L1).

mod client;
mod client_transport;
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
pub use client_transport::ClientTransport;
pub use engine::{
    AnswerCounters, CacheCleanupCounters, DnsCounters, DnsLatency, EngineCounters, EngineTelemetry,
    HttpCounters, HttpLatency, LatencyTotals, ListFetchCounters, ListenerCounters,
    ListenerTelemetry, RulesetInfo, StageTotals, SwrCounters,
};
pub use history::{
    ClientHits, DailyTopN, DomainHits, HistoryPoint, HistoryRange, HistoryResolution,
    HistorySeries, HourRollup, TopItems, TopKind,
};
pub use http::{HttpRequest, ResourceType};
pub use memory::{AllocatorStats, MemoryBreakdown, MemoryComponents, ProcessStats, StatsHeap};
pub use operating_mode::{OperatingMode, ParseOperatingModeError};
pub use perf::{
    AddressFamily, CacheStatsSample, ConcurrentConnections, LatencySummary, PerfSample, PerfSeries,
    UpstreamRtt, UpstreamSample, UpstreamState, UPSTREAM_RTT_BUCKETS_SECONDS,
};
pub use policy::{Assignment, ClientSelector, Policy, PolicyId, Schedule};
pub use protocol::Protocol;
pub use query::{Query, QueryType};
pub use query_event::{AnswerOutcome, QueryEvent, StaleServe};
pub use request_event::{Event, EventKind, Request, RequestEvent};
pub use verdict::{DecisiveRule, Verdict};
