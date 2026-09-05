//! HTTP engine (ARCHITECTURE.md L3): transparent proxy for unencrypted
//! traffic, with a streaming pass-through fast path.
//!
//! [`Server`] binds `[http.listen]` and accepts; [`Proxy`] serves each
//! connection — parse the head, judge the destination, **take a verdict**,
//! then stream the body. The pass-through path was benched against
//! PERFORMANCE.md in p2-02, before any rule touched it, so the cost of
//! filtering is a delta against a known number rather than a first
//! measurement.
//!
//! Two properties are load-bearing beyond this phase:
//!
//! - [`Proxy::serve_connection`] is **generic over the stream**, so Phase 3
//!   reuses this pipeline unchanged by handing it a TLS-terminated stream.
//! - The destination decision lives at L1 in [`fah_common::egress`], not here,
//!   because HTTPS judges SNI with exactly the same rules. Only the *claim
//!   parsing* is HTTP-shaped, and that is what stays in this crate.
//!
//! This crate does not depend on `fah-dns`: name resolution arrives as the
//! injected [`fah_common::resolve::HostResolver`] port (siblings never import
//! each other, hard rule 1).

mod block;
mod claim;
mod connections;
mod exclusions;
mod https;
mod intercept;
mod proxy;
mod request;
mod server;
mod sni;
mod tls;
mod tls_server;

pub use block::BlockStyle;
pub use claim::{ClaimError, Destination};
pub use connections::ConnectionGauge;
pub use exclusions::{ExclusionSet, InvalidExclusion, BASELINE_EXCLUSIONS};
pub use https::TlsProxy;
pub use intercept::Interception;
pub use proxy::{Proxy, ProxyCounters, ProxyStats, Ruleset};
pub use server::Server;
pub use sni::{scan_client_hello, HelloScan, MAX_HELLO_BYTES};
pub use tls::{client_config, client_config_with_roots, server_config};
pub use tls_server::TlsServer;
