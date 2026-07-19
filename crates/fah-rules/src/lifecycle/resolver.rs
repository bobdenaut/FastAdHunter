//! Name resolution for the list fetcher, as a port.
//!
//! FastAdHunter is a fully configured DNS resolver, yet its own list downloads
//! went through the *system* resolver — which on RouterOS means a 0-byte
//! `/etc/resolv.conf` and every fetch failing while DNS forwarding works fine
//! (p1-11 defects 2 and 5). The fix is to resolve list hosts over the
//! configured upstreams instead.
//!
//! `fah-rules` is L2 and the DNS engine is L3, so this crate cannot reach it
//! (ARCHITECTURE.md §Dependency Layering — siblings never import each other,
//! and L2 never imports upward). Instead this module declares what the fetcher
//! *needs*; the L4 binary implements it over `fah-dns` and hands it in. Same
//! shape as the `StatsSource`/`TelemetrySource` ports in `fah-api`.

use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;

/// The future a [`HostResolver`] returns. Owned and `'static` rather than
/// borrowing `self`, because the HTTP client requires exactly that shape —
/// implementors clone an `Arc` into the future instead.
pub type Resolving = Pin<Box<dyn Future<Output = io::Result<Vec<IpAddr>>> + Send>>;

/// Resolves a hostname for list downloads.
///
/// Implementations must not route through the query pipeline: a blocklist that
/// happened to block the host serving its own next copy would otherwise stop
/// all future updates, unrecoverably short of hand-editing the config.
pub trait HostResolver: Send + Sync + 'static {
    fn resolve(&self, host: String) -> Resolving;
}

/// Bridges a [`HostResolver`] into the shape `reqwest` wants. Kept private:
/// `reqwest` is this crate's implementation detail and must not leak into the
/// binary's side of the port.
pub(super) struct ReqwestResolver(pub(super) Arc<dyn HostResolver>);

impl reqwest::dns::Resolve for ReqwestResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let resolver = Arc::clone(&self.0);
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs = resolver.resolve(host).await?;
            // Port 0: reqwest substitutes the scheme's conventional port, and
            // an explicit port in the URL overrides it either way.
            let addrs: reqwest::dns::Addrs =
                Box::new(addrs.into_iter().map(|ip| SocketAddr::new(ip, 0)));
            Ok(addrs)
        })
    }
}
