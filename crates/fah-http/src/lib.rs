//! HTTP engine (ARCHITECTURE.md L3): transparent proxy for unencrypted
//! traffic, with a streaming pass-through fast path.
//!
//! **Scaffold only (p2-01).** [`Server`] binds `[http.listen]` and accepts
//! connections, then closes each one immediately. Proxying arrives in p2-02
//! and filtering in p2-04 — deliberately in that order, so the pass-through
//! path is benched against PERFORMANCE.md before any verdict touches it.
//!
//! The listener exists at all this early for one reason: binding is the part
//! that can fail in deployment (privileged ports, address families, a port
//! already held), and `engine.mode` gating is the part that must be provably
//! off in `dns` mode. Both are cheaper to get right against an empty engine.

mod server;

pub use server::Server;
