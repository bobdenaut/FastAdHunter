//! Long-term history rollups (ARCHITECTURE.md L1 — pure data + serde). Written
//! by `fah-stats` to flat JSONL on `/data` (ADR-0002: no embedded DB) so a
//! future dashboard can chart 30/60/90 days without scanning the raw query
//! log. These are plain records: no logic, no I/O (root CLAUDE.md hard rule 2).

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// One completed clock-hour of aggregate counters, captured off the hot path
/// before the 24h ring ([`fah-stats` `HourlyBuckets`]) overwrites its slot.
/// Appended as one JSONL line to `/data/history/rollups/rollup-YYYY-MM-DD.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HourRollup {
    /// Hours since the Unix epoch — the completed hour this row summarizes.
    pub hour_epoch: u64,
    pub queries: u64,
    pub blocked: u64,
    pub cache_hits: u64,
    /// Canonical DNS-type label → count for the hour. The label set is fixed
    /// and bounded (`A`, `AAAA`, `HTTPS`, `MX`, `TXT`, `PTR`, `NS`, `SOA`,
    /// `SRV`, `CNAME`, `OTHER`); zero buckets are omitted. `OTHER` lumps every
    /// record type outside the named set — a fixed-size counter on the hot
    /// path can't hold an unbounded set of type strings (hard rule 4).
    pub per_type: BTreeMap<String, u64>,
}

/// Top-N domains and clients over the 24h ending at a day boundary, flushed
/// once per completed day to `/data/history/rollups/top-YYYY-MM-DD.json`. The
/// window is the rolling 24h at flush time (≈ the completed calendar day when
/// the flush fires shortly after midnight); top-N is a space-saving estimate,
/// so this is an approximation by design, not an exact daily ranking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyTopN {
    /// Days since the Unix epoch — the completed day this row summarizes.
    pub day_epoch: u64,
    pub top_blocked: Vec<DomainHits>,
    pub top_queried: Vec<DomainHits>,
    pub top_clients: Vec<ClientHits>,
}

/// One point of an aggregated history series, as
/// `GET /api/v1/history/summary` serves it. At `resolution=hour` it is one
/// [`HourRollup`] restated with a wall-clock start; at `resolution=day` it is
/// the sum of that day's hours. Counters only — `blocked_percent` is derived at
/// the wire boundary, like the cache view's `load_percent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPoint {
    /// Seconds since the Unix epoch at the **start** of the bucket this point
    /// summarizes (the hour or the UTC day).
    pub ts: u64,
    pub queries: u64,
    pub blocked: u64,
    pub cache_hits: u64,
    /// Merged per-type counts, same fixed label set as [`HourRollup::per_type`].
    pub per_type: BTreeMap<String, u64>,
}

/// A domain and its hit count within a rollup window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainHits {
    pub domain: String,
    pub count: u64,
}

/// A client (source IP, optional assigned name) and its query count within a
/// rollup window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientHits {
    pub ip: IpAddr,
    pub name: Option<String>,
    pub count: u64,
}

/// The half-open window `[from, to)` a history read is scoped to. Lives here
/// rather than in either L3 crate because `fah-stats` (which reads the files)
/// and `fah-api` (which parses the query string) have to agree on it
/// field-for-field — one definition, no translating adapter that could drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryRange {
    pub from: SystemTime,
    pub to: SystemTime,
}

/// Bucket width of a summary series: one point per completed hour, or one per
/// UTC day (that day's 24 hourly rows summed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryResolution {
    Hour,
    Day,
}

/// A summary series plus the decimation actually applied. `stride` is `1` when
/// every point in the range is present and `n` when only every `n`-th was kept
/// to hold the response under the caller's point budget — the client is told,
/// rather than being handed a silently sparse chart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistorySeries {
    pub points: Vec<HistoryPoint>,
    pub stride: u64,
}

/// Which ranking `GET /api/v1/history/top` merges out of the daily top-N files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopKind {
    Blocked,
    Queried,
    Clients,
}

/// A merged top-N. Two shapes because a client is identified by IP and an
/// optional name, a domain by its name alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopItems {
    Domains(Vec<DomainHits>),
    Clients(Vec<ClientHits>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hour_rollup_serde_roundtrip() {
        let rollup = HourRollup {
            hour_epoch: 471_000,
            queries: 1234,
            blocked: 456,
            cache_hits: 789,
            per_type: BTreeMap::from([("A".to_string(), 1000), ("AAAA".to_string(), 234)]),
        };
        let json = serde_json::to_string(&rollup).unwrap();
        let back: HourRollup = serde_json::from_str(&json).unwrap();
        assert_eq!(rollup, back);
    }

    #[test]
    fn history_point_serde_roundtrip() {
        let point = HistoryPoint {
            ts: 1_695_600_000,
            queries: 1234,
            blocked: 456,
            cache_hits: 789,
            per_type: BTreeMap::from([("A".to_string(), 1000)]),
        };
        let json = serde_json::to_string(&point).unwrap();
        let back: HistoryPoint = serde_json::from_str(&json).unwrap();
        assert_eq!(point, back);
    }

    #[test]
    fn daily_top_n_serde_roundtrip() {
        let top = DailyTopN {
            day_epoch: 20_291,
            top_blocked: vec![DomainHits {
                domain: "ads.example.com".to_string(),
                count: 42,
            }],
            top_queried: vec![DomainHits {
                domain: "example.com".to_string(),
                count: 99,
            }],
            top_clients: vec![ClientHits {
                ip: IpAddr::from([192, 168, 1, 10]),
                name: Some("liviu-phone".to_string()),
                count: 77,
            }],
        };
        let json = serde_json::to_string(&top).unwrap();
        let back: DailyTopN = serde_json::from_str(&json).unwrap();
        assert_eq!(top, back);
    }
}
