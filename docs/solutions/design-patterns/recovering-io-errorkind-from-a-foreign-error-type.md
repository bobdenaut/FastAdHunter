---
title: Recovering io::ErrorKind from a foreign error type that erases it
date: 2026-08-22
category: design-patterns
module: fah-dns
problem_type: design_pattern
component: infrastructure
severity: medium
applies_when:
  - a third-party transport returns its own error enum and the caller's contract is io::Error
  - callers need to branch on the kind (retry, invalidate a pooled connection, tier a failure) rather than log a string
  - the wrapping error type hides the io::Error in a tuple variant, behind a sibling accessor, or inside a stringified message
  - a crypto or protocol layer failure must be distinguishable from a plain socket failure
symptoms:
  - every upstream failure arrives as io::ErrorKind::Other, so the caller cannot tell refused from unreachable from a rejected certificate
  - "map_err(io::Error::other) preserves the message and destroys the kind"
  - a chain walk over source() finds nothing because the variant holds its payload as a plain tuple field
related_components:
  - fah-dns/upstream/encrypted
  - fah-dns/upstream/pool
  - fah-common/error_chain
tags:
  - error-handling
  - io-errorkind
  - downcast
  - source-chain
  - hickory
  - rustls
  - h2
  - dot
  - doh
---

# Recovering io::ErrorKind from a foreign error type that erases it

## Context

`fah-dns` exposes one upstream contract: `io::Result<Message>`. DoT and DoH go
through hickory-net, whose `NetError` is a wide enum. The three `map_err` sites
used `io::Error::other`, which keeps the text and collapses every kind to
`Other`. Callers that must *decide* — the pool's invalidate-on-failure path, and
the failure tiering Adaptive DNS Stage 1 needs — had nothing to branch on, and
the only alternative was substring-matching a message.

The obvious fix, "walk `source()` and downcast to `io::Error`", is not enough on
its own: it finds nothing for the most common case.

## Guidance

Classify in four ordered steps, cheapest and most reliable first
([encrypted.rs:217](../../../crates/fah-dns/src/upstream/encrypted.rs#L217)):

```rust
fn transport_error(err: NetError) -> io::Error {
    io::Error::new(transport_error_kind(&err), fah_common::error_chain(&err))
}

fn transport_error_kind(err: &NetError) -> io::ErrorKind {
    match err {
        NetError::Io(io_err) => return io_err.kind(),
        NetError::H2(h2_err) => {
            if let Some(io_err) = h2_err.get_io() {
                return io_err.kind();
            }
        }
        _ => {}
    }
    let mut tls = false;
    let mut next = std::error::Error::source(err);
    while let Some(cause) = next {
        if let Some(io_err) = cause.downcast_ref::<io::Error>() {
            return io_err.kind();
        }
        tls |= cause.is::<rustls::Error>();
        next = cause.source();
    }
    if tls {
        io::ErrorKind::InvalidData
    } else {
        io::ErrorKind::Other
    }
}
```

1. **Variant-first, before any chain walk.** `NetError::Io` carries the
   `io::Error` as a plain tuple field with no `#[source]` attribute, so
   `source()` returns `None` and a chain-only classifier misses the single most
   common case. Read the enum's derive attributes; do not assume a payload is
   reachable just because it is stored.
2. **Sibling accessors for types that are not in the chain.** `NetError::H2`
   holds an `h2::Error`, also without `#[source]`. `h2::Error::get_io()` is the
   public way in. One arm per such type, and the arm falls through rather than
   returning when the accessor yields `None`.
3. **Chain walk with `downcast_ref::<io::Error>()`** for the layered cases the
   variants do not cover.
4. **Type-marker fallback.** While walking, note whether any cause
   `is::<rustls::Error>()`. A TLS failure that never surfaced an `io::Error` is
   still not opaque — return `InvalidData`, which is what tokio-rustls itself
   stamps when `process_new_packets` fails. Everything genuinely unknown stays
   `Other`.

**Audit the library before promising pass-through.** Classification quality is a
property of the transport, not of the crate. In hickory-net 0.26.1:

- `impl From<io::Error> for NetError` rewrites any error of kind `TimedOut` into
  `NetError::Timeout`, so a kernel `ETIMEDOUT` cannot be recovered at all.
- The DoH exchange path formats failures into `NetError::Message`, a string.
  Nothing is recoverable from it short of parsing text — which is worse than
  `Other`.

The honest claim for this change is therefore *DoT connect and exchange, plus
the DoH handshake* — not "DoT and DoH". Name the covered paths and record the
rest as limitations; a scope line that overstates coverage is a defect a
reviewer will find (p2.5-04 F1, F2).

**The typed source is the deliberate cost.** `io::Error::new(kind, String)`
means `source()` and `downcast` on the returned error no longer reach the
`NetError`. `io::Error::other(err)` keeps the typed chain but destroys the kind;
one `io::Error` cannot carry both. Kind wins because callers branch on it, and
[`fah_common::error_chain`](../../../crates/fah-common/src/lib.rs#L33) flattens
the whole chain into the message, so no diagnostic text is lost.

## Why This Matters

- A uniform `Other` forces every caller into string matching, or into treating
  all failures identically. `ConnectionRefused` (server down, move on now),
  `NetworkUnreachable` (link problem, back off) and `InvalidData` (certificate
  rejected — an encrypted upstream never silently downgrades) demand different
  responses.
- The classifier is off the hot path: it runs only on a failed exchange, and
  allocates one `String` per failure via `error_chain`. The success path
  allocates nothing extra.
- It is a pure function of a borrowed error, so its behaviour is unit-testable
  without a socket.

## When to Apply

Use it when a dependency's error type stands between you and an `io::Error`
contract *and* callers branch on the kind. Skip it when callers only log — then
`io::Error::other` is correct and cheaper. If the error type being classified is
your own, fix it at the source with `#[source]` attributes instead of bolting a
classifier onto the far end.

## Examples

Synthesize the errors you can construct, and refuse to fake the ones you cannot:

```rust
#[test]
fn transport_error_donates_the_wrapped_io_kind() {
    for kind in [
        io::ErrorKind::NetworkUnreachable,
        io::ErrorKind::HostUnreachable,
        io::ErrorKind::ConnectionReset,
    ] {
        let err = transport_error(NetError::from(io::Error::new(kind, "synthetic")));
        assert_eq!(err.kind(), kind);
        assert!(err.to_string().contains("synthetic"));
    }
}

#[test]
fn transport_error_classifies_a_rustls_failure_as_invalid_data() {
    assert_eq!(
        transport_error(NetError::from(rustls::Error::DecryptError)).kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn transport_error_leaves_a_wire_decode_failure_opaque() {
    let err = transport_error(NetError::Message("malformed answer"));
    assert_eq!(err.kind(), io::ErrorKind::Other);
    assert_eq!(err.to_string(), "malformed answer");
}
```

The `Other` test is the one that keeps the classifier honest: it pins that an
unrecognised failure is *not* guessed at.

End to end, one real socket case earns its cost — bind a TCP listener, take its
address, drop the listener, point the pool at the dead port:

```rust
async fn closed_tcp_addr() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr
}
```

Give that test a generous `timeout_ms` (10 s, not the 4 s first used). A
connect-refused round trip is microseconds on Linux, but the attempt-timeout
wrapper mints `TimedOut` if it fires first, and on Windows the margin was thin
enough to risk a flake (p2.5-04 F3).

The `h2` handshake arm has **no** unit test: `h2 0.4.15` exposes no public
constructor for an io-backed `h2::Error` (`from_io` is `pub(crate)`). Record
that as a limitation rather than reaching for a mock — the arm is three lines
and compiles against a pinned version.

## Related

- [docs/code-review/phase2.5/p2.5-04-transport-error-kinds-review.md](../../code-review/phase2.5/p2.5-04-transport-error-kinds-review.md)
  — F1 (DoH exchange-path erasure), F2 (`TimedOut` rewritten by
  `From<io::Error>`), F3 (test headroom), F5 (typed source dropped as designed),
  and the pinned-API verification that preceded all of it.
- [generation-stamped-slot-for-invalidating-a-pooled-connection.md](generation-stamped-slot-for-invalidating-a-pooled-connection.md)
  — the pooled-connection invalidation path that consumes these kinds. That doc
  fixed `TimedOut` as the trigger; widening it to other kinds is a Stage 1
  decision, not a side effect of classifying better.
