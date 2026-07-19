//! Per-upstream-server snapshot — this crate's own DTO, not `fah_dns`'s
//! `UpstreamStatus` (siblings never import each other, ARCHITECTURE.md
//! §Dependency Layering). The binary polls `UpstreamPool::status()` and
//! translates each entry into one of these via [`Metrics::set_upstreams`]
//! (`fastadhunter` is the only crate allowed to know both shapes).

/// One upstream server's counters at the moment of the last poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamSnapshot {
    /// The stable identity for metrics labels — exactly as configured.
    pub address: String,
    /// `"udp"` | `"dot"` | `"doh"`.
    pub protocol: &'static str,
    pub attempts: u64,
    pub failures: u64,
    /// Failures since the last success — non-zero means currently unhealthy.
    pub consecutive_failures: u64,
    pub tls_handshakes: u64,
}
