---
title: Scripted UDP mock for per-endpoint failure-sequence tests
date: 2026-08-22
category: design-patterns
module: fah-dns
problem_type: design_pattern
component: testing_framework
severity: medium
applies_when:
  - a counter or histogram is defined by a *sequence* of failures and successes on one endpoint
  - the assertion needs the real transport, not a stubbed Forwarder — the counter lives in the pool
  - failures must land on the same endpoint identity that later succeeds
  - the suite runs on the Windows dev box as well as musl/aarch64
symptoms:
  - "how do I make one upstream fail twice and then answer, without a second address?"
  - a test binds an ephemeral port, drops the socket, and rebinds it to start answering
  - each simulated failure costs a full attempt timeout, so the test is seconds long
  - a "make failures fast" trick behaves differently on Windows than the reasoning predicted
root_cause: missing_tooling
resolution_type: test_fix
related_components:
  - fah-dns/upstream/pool
  - fah-dns/upstream/plain
tags:
  - test-mocks
  - udp
  - tokio
  - determinism
  - port-reuse
  - windows
  - timeouts
---

# Scripted UDP mock for per-endpoint failure-sequence tests

## Context

p2.5-06 added per-endpoint failure **run-length** buckets: consecutive transport
failures on one upstream, closed by that upstream's next success. Every
assertion is a sequence — `F,F,S → [0,1,0,0]` — and all of it has to happen on
**one endpoint**, because the run is closed by the same server that failed.

That rules out the cheap options. A stub `Forwarder` never reaches the counters
(they live in `UpstreamPool`). A second address is a different endpoint. So the
test drives the real UDP transport and the mock has to decide, per request,
whether this one fails.

Related but different: [static-dispatch test seams for hot-path listener
loops](static-dispatch-test-seams-for-hot-path-listener-loops.md) fakes the
socket call *inside* the loop; this pattern keeps the real socket and scripts
what comes back.

## Guidance

### 1. One socket, bound for the whole test, scripted per request

`Vec<bool>` — one flag per incoming request, `false` = stay silent (the client
times out and counts a transport failure), exhausted = answer
([mod.rs:522](../../../crates/fah-dns/src/upstream/mod.rs#L522)):

```rust
async fn scripted_udp_server(script: Vec<bool>) -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        let mut script = script.into_iter();
        loop {
            let (len, client) = socket.recv_from(&mut buf).await.unwrap();
            if !script.next().unwrap_or(true) {
                continue;
            }
            let request = Message::from_vec(&buf[..len]).unwrap();
            let reply = answer_for(&request).to_vec().unwrap();
            socket.send_to(&reply, client).await.unwrap();
        }
    });
    addr
}
```

A flag **vector**, not an "ignore the first *k*" counter: any assertion about a
*second* run (`F,F,S,F,S` — buckets accumulate rather than reset) needs a
failure after a success, which a leading-count mock cannot express.

### 2. Never release a port to rebind it

The obvious shape — bind, take the address, `drop` the socket to make it dead,
then rebind later to start answering — has a real race: between the drop and the
rebind, any parallel test binding `127.0.0.1:0` can take that port and the
rebind panics. Holding one socket for the whole test removes the window
entirely. (Pre-existing `dead_addr` helpers still use bind-and-drop; that is
fine as long as nothing rebinds.)

### 3. Do not try to make the failures fast — measure first

The tempting trick: reply **truncated**, which sends the client into the RFC 1035
TCP retry against the same address, where a port with no TCP listener refuses
instantly. Zero timeouts.

Measured on the Windows dev box: it does not refuse. The connect runs into the
full attempt timeout — **22 s** for the same test set that costs ~1 s on
timeouts. Scoped claim: Windows 11 dev box, loopback, `TcpStream::connect` to a
port with no listener; not retested on musl/aarch64. The lesson generalizes past
the platform: a "fast failure" that depends on an OS refusing a connection is an
assumption to time before building a test suite on it.

### 4. Buy the wall clock back with concurrency, not with a shorter timeout

Each silent request costs one attempt timeout, so the temptation is to shrink
the timeout — which eats the headroom the *answering* leg needs on a saturated
box, where a spawned task can stall past a too-tight window and turn a success
into a phantom failure.

Instead keep the timeout the rest of the module already answers inside (here
`RUN_TIMEOUT_MS = 200`, [mod.rs:520](../../../crates/fah-dns/src/upstream/mod.rs#L520))
and run the independent sequences **concurrently** — each with its own mock and
its own pool, so nothing is shared ([mod.rs:580](../../../crates/fah-dns/src/upstream/mod.rs#L580)):

```rust
tokio::join!(
    closed_run_lands_in(0, [0, 0, 0, 0]),
    closed_run_lands_in(1, [1, 0, 0, 0]),
    closed_run_lands_in(2, [0, 1, 0, 0]),
    closed_run_lands_in(3, [0, 0, 1, 0]),
    closed_run_lands_in(5, [0, 0, 0, 1]),
);
```

Wall clock becomes the longest single sequence instead of their sum: 11
timeouts' worth of waiting collapses to 5. `#[tokio::test]`'s single-threaded
runtime is enough — the cost is timer waits, not CPU.

### 5. Seed the counter directly when the precondition is state, not history

The RCODE test asks "does a SERVFAIL close an open run?" — the run's *history*
is irrelevant, only that a run is open. `consecutive_failures.store(2, Relaxed)`
is one line and costs no timeout. Drive real traffic only when the sequence
itself is the thing under test.

## When to apply

| Situation | Use |
| --- | --- |
| Assertion is about a sequence on one endpoint | scripted mock (this pattern) |
| Assertion is about one branch given a state | seed the atomic, one request |
| Assertion is about the loop around the socket call | [test seam](static-dispatch-test-seams-for-hot-path-listener-loops.md), not a real socket |
| Two endpoints must diverge | one scripted mock per endpoint, one pool |

## Measurements

| Variant | `upstream::tests` wall clock | Source |
| --- | --- | --- |
| Before this task (no run-length tests) | 2.04 s | measured |
| Sequential sequences, 100 ms timeout | 1.21 s | measured |
| Concurrent sequences, 200 ms timeout (shipped) | 1.03 s | measured |
| Sequential sequences, 200 ms timeout | ~2.2 s | arithmetic, not run |
| Truncated-reply → TCP-refusal trick | 22 s | measured |

Corpus: `cargo test -p fah-dns --all-features upstream::tests`, Windows 11 x86
dev box. Not measured on the RB5009.

## Files

- [crates/fah-dns/src/upstream/mod.rs](../../../crates/fah-dns/src/upstream/mod.rs)
  — `scripted_udp_server`, `failing_then_answering_udp_server`,
  `closed_run_lands_in`, `RUN_TIMEOUT_MS`
- [docs/code-review/phase2.5/p2.5-06-failure-runlength-review.md](../../code-review/phase2.5/p2.5-06-failure-runlength-review.md)
  — findings N2, N3, N6 are the trail this pattern came out of
