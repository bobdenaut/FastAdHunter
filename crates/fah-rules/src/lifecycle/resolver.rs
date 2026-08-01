//! Name resolution for the list fetcher — the `reqwest` half of the port.
//!
//! The port itself lives at L1 in [`fah_common::resolve`], because `fah-http`
//! needs the same thing for proxy upstreams and the two consumers must not
//! depend on each other to share it. This module only bridges it into the shape
//! `reqwest` wants; the trait is re-exported here so this crate's public
//! surface is unchanged.

use std::net::SocketAddr;
use std::sync::Arc;

pub use fah_common::resolve::{HostResolver, Resolving};

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
