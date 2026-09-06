# HTTP connections are served on their own single-thread runtimes

Until now every proxied HTTP connection ran on the shared multi-thread Tokio
runtime: the server-side connection task and the upstream connection task were
two work-stealing tasks, so every body chunk and every 408 KiB read-buffer
reallocation was allocated on one worker and freed on another. mimalloc only
reclaims a block on the thread that owns its page; a worker that woke for a
burst, allocated, then parked for good never reclaims. On the RB5009 a 900 MiB
transfer left +56..+60 MiB resident that no later idle period returned
([resoak-0.3.1-memory-diagnosis](../code-review/phase2.6/resoak-0.3.1-memory-diagnosis.md)
F29; [alloc-domains-http-task](../code-review/phase2.6/alloc-domains-http-task.md)
§Results).

Now the HTTP Engine runs N **allocation domains** (CONTEXT.md): one
`current_thread` runtime per OS thread, each serving whole connections end to
end with its own hyper client and pool. One acceptor on the shared runtime
takes the `max_connections` permit, accepts, and hands the socket to the next
domain over a bounded channel. `[runtime] http_runtimes` sets N; `0` keeps the
shared-runtime path.

## Decision

- Hand-off is one acceptor plus channels, not `SO_REUSEPORT`: the permit and the
  connection gauge stay one object each, and the gates run on Windows.
- `max_connections` is one semaphore across all domains.
- A domain is a `current_thread` runtime on its own std thread, never one
  runtime with N workers: intra-runtime migration recreates the retention.
- Each domain builds its own `Proxy` (hyper-util client and pool) on its own
  thread; resolver, ruleset, policies and one `ProxyCounters` are shared `Arc`s,
  the events `Sender` is cloned.
- `fah-http` owns the threads and runtimes — a deliberate deviation from
  engineering principle 6 — because hand-off and drain are one mechanism with
  the accept loop. The binary decides N and the drain timeout.
- Shutdown: HTTP drains first (≤ 5 s, then abort), DNS is aborted after, so the
  resolver answers through the drain.
- Cross-runtime traffic is limited to what the ownership rule allows
  ([allocation-domains-proposal](../code-review/phase2.6/allocation-domains-proposal.md)
  §Ownership): the accepted socket state (no heap), one boxed event per
  request, atomics, shared read-only `Arc`s. The DoT/DoH exchange is pinned to
  the runtime that built the upstream pool, whichever task first connects it.

## What this buys

Measured on the RB5009, N=2 against N=0, same image, same 900 MiB workload
(task file §Results): held residue +14..+17 MiB against +56..+60, CPU
−17..−24 %, throughput and p95 inside the control spread, container stop with
three transfers in flight in 5.5 s. Three further waves neither ratchet the
residue nor return it while idle.

## The cost, stated plainly

- N threads with 2 MiB virtual stacks, N runtimes, N hyper pools (8 idle
  connections per host each, 60 s). Bounded by config; N ≤ 64.
- Per accepted connection: one `into_std`/`from_std` pair, one channel send.
  Nothing per byte.
- A stalled domain blocks the acceptor after 32 queued hand-offs
  (head-of-line); a dead domain is dropped from the rotation with one `error`
  line, the rest keep serving.
- The predeclared memory criterion — floor ±6 MB by +3 min — was **not met**.
  The residue is a quarter of the control's, bounded, and consistent with
  allocator page retention rather than live objects
  ([alloc-domains-http-review](../code-review/phase2.6/alloc-domains-http-review.md)
  third pass). Adopting on that evidence is the owner's decision, recorded
  here; the 7-day soak at N=2 is the verdict on the plateau.

## Considered options

- Tune mimalloc (purge delay, page reclaim, collect on park): ablated in the
  0.3.1 diagnosis without effect; a collect on park runs before the remote frees
  arrive.
- Shrink hyper's buffers: the 408 KiB A/B moved throughput, not the residue.
- One N-worker runtime for HTTP: same cross-worker frees, rejected before
  measuring.
- `SO_REUSEPORT` per domain: N acceptors, N semaphores, unix-only.

## Revisit criteria

- The 7-day soak at N=2 shows the plateau climbing: add an idle-time collect on
  the domain threads (couples `fah-http` to the allocator; only then).
- A workload where head-of-line blocking at the acceptor is observed: raise the
  hand-off depth or shed to the next domain instead of waiting.
- Phase 3 TLS termination: the domain serves the TLS stream unchanged; if the
  handshake cost moves the CPU figure, remeasure N.
