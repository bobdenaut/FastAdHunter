//! DNS engine (ARCHITECTURE.md L3): UDP/TCP listeners and the full pipeline
//! — decode -> Rule Engine verdict -> block synthesis, or cache -> upstream
//! ([`upstream::UpstreamPool`]: plain UDP with TCP truncation retry, DoT,
//! DoH, ordered fallback).

mod cache;
mod pipeline;
mod qtype;
mod response;
mod rewrite;
mod server;
mod swr;
mod tcp;
mod udp;
mod upstream;

pub use cache::{CacheClean, CacheStats};
pub use pipeline::{Pipeline, Transport};
pub use server::Server;
pub use swr::SwrStats;
pub use upstream::{Forwarder, UpstreamPool, UpstreamStatus};
