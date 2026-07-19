//! Output shapes matching API.md's `GET /api/v1/stats` (§Statistics & query
//! log) — the wire JSON itself is fah-api's concern (p1-09); these are the
//! Rust values it will serialize.

use std::net::IpAddr;

use serde::Serialize;

use crate::bucket::BucketView;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DomainCount {
    pub domain: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClientCount {
    pub ip: IpAddr,
    pub name: Option<String>,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsSnapshot {
    pub window: &'static str,
    pub queries_total: u64,
    pub blocked_total: u64,
    pub blocked_percent: f64,
    pub cache_hit_percent: f64,
    pub top_blocked_domains: Vec<DomainCount>,
    pub top_queried_domains: Vec<DomainCount>,
    pub top_clients: Vec<ClientCount>,
    pub buckets: Vec<BucketView>,
}
