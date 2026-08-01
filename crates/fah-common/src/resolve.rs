//! Name resolution as a port (ARCHITECTURE.md §Dependency Layering → Ports).
//!
//! FastAdHunter is itself a fully configured DNS resolver, yet everything
//! inside it that needs a hostname resolved cannot simply call the DNS engine:
//! `fah-dns` is L3, and neither `fah-rules` (L2, list downloads) nor `fah-http`
//! (L3 sibling, proxy upstreams) may import it. So this trait states what those
//! consumers *need*; the L4 binary implements it over `fah-dns`'s upstream pool
//! and injects it. Same shape as `fah-api`'s `StatsSource`/`TelemetrySource`.
//!
//! It lives at L1 rather than in either consumer precisely so there is **one**
//! port and one implementation: `fah-rules` re-exports it, and `fah-http` uses
//! it directly, without either depending on the other.
//!
//! The alternative — the system resolver — is not available. On RouterOS the
//! container's `/etc/resolv.conf` is a 0-byte file, so `getaddrinfo` fails every
//! lookup while DNS forwarding keeps working perfectly (p1-11 defects 2 and 5):
//! the process looks healthy and silently downloads nothing.

use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;

/// The future a [`HostResolver`] returns. Owned and `'static` rather than
/// borrowing `self`, because the HTTP clients on both sides of this port
/// require exactly that shape — implementors clone an `Arc` into the future
/// instead.
pub type Resolving = Pin<Box<dyn Future<Output = io::Result<Vec<IpAddr>>> + Send>>;

/// Resolves a hostname to addresses over FastAdHunter's own configured
/// upstreams.
///
/// **Implementations must not route through the query pipeline** — no Rule
/// Engine, no cache. Two independent reasons, one per consumer:
///
/// - *List downloads*: a blocklist that happened to block the host serving its
///   own next copy would stop all future updates, unrecoverable short of
///   hand-editing the config.
/// - *Proxy upstreams*: HTTP filtering decides on host **and path** (p2-04), so
///   folding a verdict into name resolution would turn a blocked URL into a
///   connection failure instead of a synthesized block response — and would
///   block the whole host for a rule that targeted one path on it.
///
/// Returning every address found is deliberate: the caller applies its own
/// policy (see [`crate::egress`]) and may try them in order.
pub trait HostResolver: Send + Sync + 'static {
    fn resolve(&self, host: String) -> Resolving;
}
