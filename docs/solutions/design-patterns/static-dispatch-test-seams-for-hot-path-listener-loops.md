---
title: Static-dispatch test seams for hot-path listener loops
date: 2026-08-22
category: design-patterns
module: fah-dns
problem_type: design_pattern
component: testing_framework
severity: medium
applies_when:
  - testing retry/backoff/escalation logic in a hot-path network listener loop
  - hot path forbids dyn dispatch, boxed futures, and per-call allocations
  - need to force recv_from/accept-style socket calls to fail without touching production types
  - loop function must stay generic so production still monomorphizes to static dispatch
symptoms:
  - no way to make recv_from or accept return an error on demand in a test
  - "mocking would require dyn Trait or Box<dyn Future>, violating hot-path/no-alloc rules"
  - backoff and fatal-escalation branches are untested because real OS sockets rarely error on command
  - waiting out real backoff sleep durations makes the test suite slow
root_cause: missing_tooling
resolution_type: test_fix
related_components:
  - fah-dns/udp
  - fah-dns/tcp
  - fah-dns/backoff
  - fah-dns/testkit
tags:
  - rpitit
  - test-seams
  - hot-path
  - static-dispatch
  - tokio
  - dns-listener
  - backoff
  - monomorphization
---

# Static-dispatch test seams for hot-path listener loops

## Context

`fah-dns`'s UDP and TCP listener loops (`crates/fah-dns/src/udp.rs`,
`crates/fah-dns/src/tcp.rs`) got retry/backoff/escalation on socket errors in
`p2.5-01-listener-resilience` — a task seeded by the Global Architecture
Review's "silent DNS listener death" finding (session history). The first
review pass (`docs/code-review/phase2.5/p2.5-01-listener-resilience-review.md`
F1) found the acceptance criterion untested: no test produced a real
`recv_from`/`accept` `io::Error`, so the Sleep→continue wiring, the
on-success reset, and the Fatal→`ListenerDied` exit chain were unexercised
end to end.

The loops were hard-coded to concrete `tokio::net::UdpSocket` /
`TcpListener` types, and the project forbids the obvious fixes:

- `dyn Trait` boxing → vtable dispatch + allocation on the hottest loop in
  the binary (CLAUDE.md hard rule 3: no locks, no allocations on the hot
  path).
- `#[cfg(test)]`-swapping the socket type → production wiring itself goes
  untested, only a parallel test-only code path.
- A real-socket error-injection test (force a kernel `recv_from`/`accept`
  failure) → not portable across platforms, no reliable trigger.

## Guidance

Seam the loop with a trait whose async methods are declared via
return-position `impl Trait` in trait (RPITIT), implemented for the real
socket type by 1:1 delegation, with the loop itself made generic over the
trait (`crates/fah-dns/src/udp.rs:17-50`):

```rust
pub trait Datagrams: Send + Sync + 'static {
    fn recv_from(
        &self,
        buf: &mut [u8],
    ) -> impl Future<Output = io::Result<(usize, SocketAddr)>> + Send;

    fn send_to(
        &self,
        reply: &[u8],
        client: SocketAddr,
    ) -> impl Future<Output = io::Result<usize>> + Send;
}

impl Datagrams for UdpSocket {
    fn recv_from(
        &self,
        buf: &mut [u8],
    ) -> impl Future<Output = io::Result<(usize, SocketAddr)>> + Send {
        UdpSocket::recv_from(self, buf)
    }

    fn send_to(
        &self,
        reply: &[u8],
        client: SocketAddr,
    ) -> impl Future<Output = io::Result<usize>> + Send {
        UdpSocket::send_to(self, reply, client)
    }
}

pub async fn run<S: Datagrams, F: Forwarder>(
    socket: S,
    pipeline: Arc<Pipeline<F>>,
) -> ListenerDied {
```

TCP needs an associated type instead of a plain future, because `accept`
returns a stream the connection handler must also be generic over
(`crates/fah-dns/src/tcp.rs:32-44`):

```rust
pub trait Accept: Send + Sync + 'static {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;

    fn accept(&self) -> impl Future<Output = io::Result<(Self::Stream, SocketAddr)>> + Send;
}

impl Accept for TcpListener {
    type Stream = TcpStream;

    fn accept(&self) -> impl Future<Output = io::Result<(TcpStream, SocketAddr)>> + Send {
        TcpListener::accept(self)
    }
}
```

`run<L: Accept, F: Forwarder>` (`tcp.rs:46`) hands the accepted stream to a
separately generic `handle_connection<S: AsyncRead + AsyncWrite + Unpin, F:
Forwarder>` (`tcp.rs:91`), so the per-connection I/O is also seamed without
needing `Accept::Stream` to be `TcpStream` specifically — `FlakyListener`'s
`type Stream = DuplexStream` (`tcp.rs:157`, from `tokio::io::duplex`) plugs
straight in.

The fake injects N errors, one success, then errors until escalation,
tracked with plain `Arc<AtomicU32>` counters
(`crates/fah-dns/src/udp.rs:107-129`):

```rust
struct FlakySocket {
    errors_before_first_success: u32,
    errors: Arc<AtomicU32>,
    delivered: Arc<AtomicU32>,
}

impl Datagrams for FlakySocket {
    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        if self.errors.load(Ordering::Relaxed) == self.errors_before_first_success
            && self.delivered.load(Ordering::Relaxed) == 0
        {
            self.delivered.fetch_add(1, Ordering::Relaxed);
            buf[..2].copy_from_slice(&[0x2a, 0x2a]);
            return Ok((2, SocketAddr::from((Ipv4Addr::LOCALHOST, 5353))));
        }
        self.errors.fetch_add(1, Ordering::Relaxed);
        Err(io::Error::other("induced recv failure"))
    }

    async fn send_to(&self, reply: &[u8], _client: SocketAddr) -> io::Result<usize> {
        Ok(reply.len())
    }
}
```

Driven under virtual time so the real-world backoff duration costs nothing
in wall-clock test time (`crates/fah-dns/src/udp.rs:131-155`):

```rust
#[tokio::test(start_paused = true)]
async fn a_transient_recv_error_retries_and_a_receive_resets_the_escalation() {
    let (pipeline, _data_dir) = testkit::pipeline();
    let errors = Arc::new(AtomicU32::new(0));
    let delivered = Arc::new(AtomicU32::new(0));
    let socket = FlakySocket {
        errors_before_first_success: FATAL_CONSECUTIVE_ERRORS - 1,
        errors: Arc::clone(&errors),
        delivered: Arc::clone(&delivered),
    };

    let died = run(socket, pipeline).await;

    assert!(
        died.last_error.to_string().contains("induced recv failure"),
        "got: {}",
        died.last_error
    );
    assert_eq!(delivered.load(Ordering::Relaxed), 1);
    assert_eq!(
        errors.load(Ordering::Relaxed),
        2 * FATAL_CONSECUTIVE_ERRORS - 1,
        "the successful receive must reset the consecutive-error count"
    );
}
```

`crates/fah-dns/src/testkit.rs` (declared `#[cfg(test)] mod testkit;` in
`crates/fah-dns/src/lib.rs:15-16`) factors the one piece both loop tests
need — a `Pipeline` wired to a `NullForwarder` — so neither test file
duplicates rule-manager/cache/channel setup. `crates/fah-dns/src/backoff.rs`
(`RetryPolicy`, `FATAL_CONSECUTIVE_ERRORS = 40`, `backoff.rs:5`) is the pure,
clock-free policy both loops call into and both tests pin the arithmetic of.

## Why This Matters

- **Zero-cost, verified, not assumed**: `udp`/`tcp` are private modules
  (`crates/fah-dns/src/lib.rs:14` and `crates/fah-dns/src/lib.rs:17`) and
  `Datagrams`/`Accept` are not
  exported — the only production caller is `Server`, which passes a
  concrete `UdpSocket`/`TcpListener`. The generic `run<S: Datagrams, ...>`
  monomorphizes to exactly the pre-seam code at that call site: no vtable,
  no dynamic dispatch, no allocation added to the hot receive/accept path.
  The review's performance pass confirmed by inspection that the happy path
  differs from the pre-seam version by exactly one non-atomic `u32` store
  (`RetryPolicy::on_success`), and that store predates the seam
  (`docs/code-review/phase2.5/p2.5-01-listener-resilience-review.md`,
  §Categories checked → Performance, and §Status re-verification).
- **Virtual time makes a ~66 s test fast and non-flaky**:
  `FATAL_CONSECUTIVE_ERRORS = 40` means the reset test runs 39 errors → one
  success (resets the policy) → 40 more errors → escalation: 79 total
  induced errors (`errors.load == 2 * FATAL_CONSECUTIVE_ERRORS - 1`,
  `udp.rs:150-154`, `tcp.rs:194-198`), with backoff doubling 10 ms → capped
  at 1 s per phase — roughly two runs of ~33 s each in real time.
  `#[tokio::test(start_paused = true)]` auto-advances every
  `tokio::time::sleep` the instant nothing else is runnable, so the whole
  test completes in milliseconds of wall time with no sleep-vs-assertion
  race.

## When to Apply

- An async I/O loop has error-path branching (retry, backoff, escalate)
  that needs proving, and the loop sits on a path CLAUDE.md hard rule 3
  forbids adding dispatch/allocation cost to.
- The only realistic way to exercise the error branch is fault injection,
  and a real OS-level fault is not portably inducible (kernel socket
  errors, filesystem races, etc.).
- A backoff/retry sequence is too long to run at real wall-clock speed —
  reach for `#[tokio::test(start_paused = true)]` instead of shrinking the
  real constants just to make the test fast.
- **Declared reuse**: this loop+seam shape is the template Phase 3's
  DoT/DoH and SNI listeners inherit
  (`plan/wip/phase2.5-hardening/p2.5-01-listener-resilience.md` §Scope).
  New Phase 3 listener loops should be seamed the same way from the start
  rather than retrofitted after their own F1-shaped review finding.

## Examples

Before (untestable — review finding F1): `udp::run` took a concrete
`UdpSocket` directly. The only way to hit the error arm
(`RetryDecision::Sleep`/`RetryDecision::Fatal`) was a real `recv_from`
failure — not reliably producible cross-platform — so the escalation and
reset logic shipped covered only by `backoff.rs`'s pure policy-math unit
tests (`backoff.rs:47-83`), never by the loop that actually calls
`policy.on_error()`/`policy.on_success()` against a socket.

After (seamed, generic, provable): `run<S: Datagrams, F: Forwarder>`
accepts anything implementing `Datagrams`. Production passes `UdpSocket`
(zero-cost by monomorphization); tests pass `FlakySocket`. The
reset-on-success assertion is the concrete proof the seam exists to
deliver — it pins that a single successful `recv_from` between two error
runs really does zero the consecutive-error counter, not just that the
pure `RetryPolicy` object would do so in isolation.

The TCP side is the same shape with one extra generic parameter
(`Accept::Stream`) so the per-connection handler can run over
`tokio::io::duplex`'s `DuplexStream` in tests and `TcpStream` in
production without any behavior fork between them (`tcp.rs:150-199`).

Committed on `main`, `8ffbcac`.

## Related

- `docs/code-review/phase2.5/p2.5-01-listener-resilience-review.md` —
  review whose F1 finding produced this pattern; records the fix and its
  re-verification.
- `plan/wip/phase2.5-hardening/p2.5-01-listener-resilience.md` — task spec;
  declares this loop shape the template Phase 3 listeners inherit.
- `plan/open/phase3/p3-05-dot-doh-listeners.md` — next consumer of the
  pattern (DoT/DoH listeners into the same pipeline).
- `ARCHITECTURE.md` §Runtime Model, "Ingest socket topology" bullet —
  background on the listener loop structure this seams.
