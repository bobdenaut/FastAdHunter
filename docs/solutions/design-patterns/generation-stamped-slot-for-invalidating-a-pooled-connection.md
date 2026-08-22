---
title: Generation-stamped slot for invalidating a pooled handle that has no identity
date: 2026-08-22
category: design-patterns
module: fah-dns
problem_type: design_pattern
component: infrastructure
severity: medium
applies_when:
  - a shared slot holds one reusable connection or client that many concurrent callers clone out of
  - an error path must discard that connection, but only if it is still the one the caller used
  - the handle is a cheap clone with no identity that survives cloning, so Arc::ptr_eq is unavailable
  - a caller may reconnect while another caller is still holding the previous handle
symptoms:
  - a stale error path clears the slot and drops a live connection a concurrent caller just installed
  - two callers failing on the same dead connection each connect, so handshakes scale with concurrency
  - "no way to answer \"is the pooled handle still the one I used?\" — every clone compares equal"
root_cause: concurrency
resolution_type: code_fix
related_components:
  - fah-dns/upstream
tags:
  - connection-pool
  - generation-counter
  - aba
  - invalidation
  - hickory
  - dot-doh
---

# Generation-stamped slot for invalidating a pooled handle that has no identity

## Context

`ExchangeConn` keeps one DoT/DoH connection in `Mutex<Slot>` and hands every
query a clone ([encrypted.rs:44](../../../crates/fah-dns/src/upstream/encrypted.rs#L44)).
A timed-out exchange must be thrown away so the next query reconnects — but
the discard runs *after* the failure, by which time another query may have
already installed a newer connection. The
[liveness-check pattern](liveness-check-under-the-commit-lock-for-in-flight-background-work.md)
solves the same shape with `Arc` pointer identity; that is not available here,
because hickory's `DnsExchange` is a cheap clone whose inner `Arc`s are not
exposed, so every clone compares equal to every other.

## Guidance

Stamp the slot with a monotone `u64` that **only a store advances**, hand each
caller the generation it read, and make every mutation conditional on it.

```rust
struct Slot { generation: u64, exchange: Option<DnsExchange<TokioRuntimeProvider>> }

impl Slot {
    fn store(&mut self, exchange: DnsExchange<TokioRuntimeProvider>) {
        self.generation += 1;
        self.exchange = Some(exchange);
    }
}

async fn invalidate_if_current(&self, generation: u64) {
    let mut slot = self.slot.lock().await;
    if slot.generation == generation {
        slot.exchange = None;
    }
}
```

Two rules make it work:

| Rule | Consequence |
| --- | --- |
| Increment on **store** only, never on clear | The counter is monotone and a clear is idempotent, so there is no ABA: a stale generation can match only the exact exchange it named, or a slot already `None` (clearing `None` is a no-op) |
| Mutate only under the slot's own lock | No atomics, no separate lock ordering; the counter and the value are one critical section |

Then express "first attempt" and "post-error reconnect" as **one** entry point
taking the generation the caller wants to avoid
([encrypted.rs:121](../../../crates/fah-dns/src/upstream/encrypted.rs#L121)):

```rust
async fn acquire(&self, attempt_timeout: Duration, stale: Option<u64>) -> io::Result<Held> {
    let mut slot = self.slot.lock().await;
    if stale != Some(slot.generation) {
        if let Some(exchange) = slot.exchange.as_ref() { /* adopt */ }
    }
    // connect + slot.store(...)
}
```

`stale = None` is the first attempt (adopt anything pooled); `stale = Some(g)`
is the reconnect (adopt anything *except* `g`). A reconnect therefore adopts a
connection a concurrent query installed meanwhile instead of dropping it.

## Why This Matters

- The naive `reconnect()` — connect unconditionally — makes N concurrent
  queries on one dead connection perform N handshakes and drop N-1 live
  connections. The stale-generation check collapses that to one.
- The invariant "an older caller never drops a newer value" then holds on
  **both** mutation paths, not just the clear. Splitting the two into separate
  `connected()` / `reconnect()` functions is what let them diverge; folding them
  into one `acquire` was a net line reduction.
- Cost is one `u64` inside the existing `Mutex` and one `u64` in a returned
  struct: no allocation, no new lock, nothing on the success path.

## When to Apply

- The handle cannot be compared by identity (cheap clone, opaque inner, FFI
  handle, index into a slab that gets reused).
- Invalidation is decided asynchronously, after the value was cloned out.
- Prefer `Arc` pointer identity when the handle *does* have it — it needs no
  extra state. The generation counter is the fallback, not the default.

## Examples

The discriminating test is not "does it reconnect" but "does it reconnect
**only** for the generation that failed":

```
store (gen G) → reconnect (gen G+1)
invalidate_if_current(G)   → slot still full
invalidate_if_current(G+1) → slot empty
```

Plus the concurrency case: two `acquire(_, Some(G))` calls after one reconnect
must yield `fresh == false` and the same generation on the second, with
`handshakes() == 2` — proof the second call adopted rather than reconnected.
For the end-to-end shape, a test server needs a **blackhole switch** (read the
query, never answer, never close) — a close is the idle-close path, which is
already handled and would not exercise the timeout path at all.

## Related

- [docs/code-review/phase2.5/p2.5-03-encrypted-reconnect-review.md](../../code-review/phase2.5/p2.5-03-encrypted-reconnect-review.md)
  — F1 (the dropped-newer-connection defect), the invariant table, and why F2
  (slot lock held across `connect()`) is an accepted constraint owned by
  Adaptive DNS Stage 1.
- [liveness-check-under-the-commit-lock-for-in-flight-background-work.md](liveness-check-under-the-commit-lock-for-in-flight-background-work.md)
  — same "stale worker must not clobber newer state" problem where `Arc`
  identity *is* available; that doc's "not a generation counter" note is this
  doc's opposite case.
