//! DNS engine (ARCHITECTURE.md L3): UDP/TCP listeners and the full pipeline
//! — decode -> Rule Engine verdict -> block synthesis, or cache -> upstream
//! ([`upstream::UpstreamPool`]: plain UDP with TCP truncation retry, DoT,
//! DoH, ordered fallback).

mod backoff;
mod cache;
mod dot;
mod pipeline;
mod qtype;
mod response;
mod rewrite;
mod server;
mod swr;
mod tcp;
#[cfg(test)]
mod testkit;
mod udp;
mod upstream;

pub use cache::{CacheClean, CacheCleanupStats, CacheStats, DEFAULT_REFRESH_CLAIM_LEASE};
pub use dot::{DotTls, DOT_MAX_CONNECTIONS};
pub use pipeline::{Pipeline, Transport};
pub use server::{ListenerDied, Server};
pub use swr::SwrStats;
pub use tcp::{TcpConnectionGauge, MAX_MESSAGE_LEN};
pub use udp::UdpInflightGauge;
pub use upstream::health::{
    classify, next_word, pack, penalty, record, select, unpack, Candidate, Health, HealthMode,
    Outcome, PackedWord, Policy, Selected, State, Transition, TransportKind, Word,
};
pub use upstream::{
    worst_case_walk, ForwardOutcome, Forwarder, UpstreamPool, UpstreamStatus, ATTEMPT_LEGS,
};
