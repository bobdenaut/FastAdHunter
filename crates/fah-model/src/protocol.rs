//! The transport an upstream DNS server is reached over (ARCHITECTURE.md L1 —
//! pure data).
//!
//! A domain value, not a label: it is carried by `fah-dns`'s pool status,
//! `fah-metrics`' upstream snapshot, every persisted [`crate::PerfSample`] and
//! `GET /api/v1/telemetry`. It was a `&'static str` / `String` at each of those
//! hops, which made every consumer a `match` on spelling and every producer
//! free to invent a fourth one.
//!
//! ## Why this is not `fah_config::UpstreamProtocol`
//!
//! That enum has the same three variants and is genuinely a different thing:
//! it is the *configured* protocol, parsed and validated out of
//! `fastadhunter.toml`. This is the *observed* protocol on a live reading.
//! They cannot be unified anyway — both crates are L1, and the layering guard
//! (`crates/fastadhunter/tests/layering.rs`) forbids siblings importing each
//! other. `fah-dns` maps one to the other once, where it builds the pool.

use serde::{Deserialize, Deserializer, Serialize};

/// How an upstream is reached.
///
/// Serialized lowercase (`"udp"` / `"dot"` / `"doh"`) — the spelling already on
/// disk in `/data/history/perf/*.jsonl` and already published by the API, so
/// this type changes no byte of either.
///
/// **No `Default`.** An observed transport has no sensible default, and the
/// obvious one would silently label an encrypted upstream as plaintext.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Udp,
    /// DNS over TLS.
    Dot,
    /// DNS over HTTPS.
    Doh,
    /// A spelling this build does not know — only reachable by reading a perf
    /// row written by a newer one. Never produced by the pool.
    Unknown,
}

impl Protocol {
    /// The wire spelling, for a JSON field or a log line.
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Udp => "udp",
            Protocol::Dot => "dot",
            Protocol::Doh => "doh",
            Protocol::Unknown => "unknown",
        }
    }

    /// Whether reaching this upstream involves a TLS handshake — the two
    /// variants for which a `tls_handshakes` counter can be non-zero.
    pub fn is_encrypted(self) -> bool {
        matches!(self, Protocol::Dot | Protocol::Doh)
    }
}

/// Hand-written so an unknown spelling degrades to [`Protocol::Unknown`]
/// instead of failing. `#[serde(other)]` covers only tagged enums, and a
/// rejected value would take the whole persisted perf row with it —
/// `HistoryReader` skips a row it cannot parse, so one new transport written by
/// a newer binary would silently erase that row's RSS, latency and cache series.
impl<'de> Deserialize<'de> for Protocol {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // `deserialize_str` + `visit_str` rather than `<&str>::deserialize`:
        // the latter demands a borrowing deserializer and so would fail on
        // `serde_json::from_value`, which several callers use.
        deserializer.deserialize_str(ProtocolVisitor)
    }
}

struct ProtocolVisitor;

impl serde::de::Visitor<'_> for ProtocolVisitor {
    type Value = Protocol;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("an upstream protocol name")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Protocol, E> {
        Ok(match text {
            "udp" => Protocol::Udp,
            "dot" => Protocol::Dot,
            "doh" => Protocol::Doh,
            _ => Protocol::Unknown,
        })
    }
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 30 days of retained perf rows carry these as bare lowercase strings.
    /// Changing the Rust type must not change one byte of them, or the history
    /// series loses every row that names an upstream.
    #[test]
    fn the_wire_spelling_is_unchanged_lowercase() {
        for (protocol, spelling) in [
            (Protocol::Udp, "\"udp\""),
            (Protocol::Dot, "\"dot\""),
            (Protocol::Doh, "\"doh\""),
        ] {
            assert_eq!(serde_json::to_string(&protocol).unwrap(), spelling);
            assert_eq!(
                serde_json::from_str::<Protocol>(spelling).unwrap(),
                protocol
            );
            assert_eq!(format!("\"{protocol}\""), spelling);
        }
    }

    #[test]
    fn only_the_tls_transports_are_encrypted() {
        assert!(!Protocol::Udp.is_encrypted());
        assert!(Protocol::Dot.is_encrypted());
        assert!(Protocol::Doh.is_encrypted());
        assert!(!Protocol::Unknown.is_encrypted());
    }

    /// A row written by a build that knows a transport this one does not must
    /// still load. `HistoryReader` drops a row it cannot parse, so rejecting
    /// the value would erase that row's RSS, latency and cache series too —
    /// the perf file is already lenient about unknown *keys*, and this makes it
    /// equally lenient about this value.
    #[test]
    fn an_unrecognised_transport_degrades_instead_of_failing_the_row() {
        assert_eq!(
            serde_json::from_str::<Protocol>("\"doq\"").unwrap(),
            Protocol::Unknown
        );
        // Both entry points, because they take different serde paths and only
        // one of them can borrow.
        assert_eq!(
            serde_json::from_value::<Protocol>(serde_json::json!("doq")).unwrap(),
            Protocol::Unknown
        );
        assert_eq!(
            serde_json::to_string(&Protocol::Unknown).unwrap(),
            "\"unknown\""
        );
    }
}
