# Allocation domains — isolated single-thread runtimes, one heap domain per thread

**Status: PROPOSAL, design level. Nothing implemented. Arena-per-domain is
unvalidated.** Written 2026-09-05 during the 0.3.1 floor-climb diagnosis
([resoak-0.3.1-memory-diagnosis.md](resoak-0.3.1-memory-diagnosis.md)) so the
idea survives until the two A/B tests below decide it. No ADR yet; an ADR
follows only if both tests pass.

## Summary

- Today one tokio runtime (4 workers on the RB5009) serves DNS, the HTTP proxy,
  the API and the list lifecycle. Every worker heap holds pages that mix a
  burst's short-lived connection state with long-lived cache entries and stats
  keys. After the burst the connection state is freed but one long-lived block
  per page keeps the page resident (F28, F29). Each "first contact" burst leaves
  +1..2 MiB at 4 workers (F24, F30); production shows three such steps in the
  first 48 h (F33).
- The allocator swap (musl) removes the residue but costs +4..12 % per cached or
  blocked query (F27). Rejected: performance is a primary objective.
- Proposal: DNS stays the multi-thread base runtime; HTTP, API and Rules each
  get isolated single-thread runtimes (HTTP: N of them, own `SO_REUSEPORT`
  listener each), so a subsystem's allocations live and die on their own thread
  and therefore in their own mimalloc heaps. Pages then hold one lifetime
  profile each and empty out after a transient. Optionally, bind each
  runtime's heaps to an exclusive mimalloc arena so the domain is one address
  range and the kernel can attribute RSS to it.
- Two validation steps, cheapest first: HTTP A/B, then API A/B. No design
  decision before the numbers.

## Current layout vs proposed

```text
 TODAY (0.3.1)                                  PROPOSED
 one runtime, 4 workers, shared heaps           four runtimes, one heap domain each

 ┌─ tokio runtime · 4 workers ────────────┐     ┌─ DNS · cores (4) ─────────────────┐
 │ worker heap = mixed pages:             │     │ pipeline · cache · stats · poll   │
 │  [cache entry][conn buf][stats key]    │     │ long-lived + tiny transients only │
 │  [conn buf][conn buf][cache entry]...  │     └───────────────┬───────────────────┘
 │                                        │                     │ Arc<Matcher>, read-only
 │ DNS query, proxy connection, API read, │     ┌─ HTTP · N single-thread runtimes ─┐
 │ list download all land on whichever    │     │ rt1 → 1 worker · own listener     │
 │ worker is free                         │     │ rtN → 1 worker · SO_REUSEPORT     │
 │                                        │     │ connection lives and dies on one  │
 │ after a burst: conn bufs freed, but    │     │ thread; N = max(1,cores/2), cfg   │
 │ one cache entry / stats key per page   │     └───────────────────────────────────┘
 │ keeps the page alive  → floor step     │     ┌─ API · 1 ─────────────────────────┐
 └────────────────────────────────────────┘     │ API · dashboard · history reads   │
 ┌─ blocking pool (exit after 10 s idle) ─┐     │ rare, large, own pages            │
 │ compile · fetch parse · snapshots      │     └───────────────────────────────────┘
 │ heap dies with the thread  (good)      │     ┌─ Rules · 1 + blocking pool ───────┐
 └────────────────────────────────────────┘     │ list fetch · compile              │
                                                │ compile threads still EXIT → heap │
 residue per burst @4 workers: +1..2 MiB        │ abandoned, freed by the dropper   │
 (three "first contact" steps in prod)          └───────────────────────────────────┘

 RULE  an object is dropped on the runtime that allocated it,
       or it came from a thread that has already exited.
       Cross-runtime: Arc (shared, read-only) or channel, receiver owns the drop.
       Retention case (F29, open until the mechanism is closed): allocated on a
       thread that then parks for good, freed by another one.
```

## Proposed topology

| Runtime | Hosts | Lifetime profile | Workers (derived) |
| --- | --- | --- | --- |
| DNS | pipeline, cache, stats, telemetry poll | long-lived + tiny transients | `cores` (or `cores − 1` when HTTP is enabled, config choice) |
| HTTP | proxy, connections, splice buffers | bursty, short-lived | **`1` by mechanism** (F36: 2 workers retain +12.3 MiB, 1 worker returns the burst at once). More capacity = N single-thread runtimes with `SO_REUSEPORT` listeners, never one N-worker runtime |
| API | API, dashboard, history reads | rare, large | `1` |
| Rules | list fetch, compile (on its blocking pool) | periodic, large, dies with the thread | `1` |

```text
Allocation domains — isolated runtimes with single-thread ownership

DNS      base runtime, multi-thread, work-stealing
         └── cores workers (4)        cache · stats · pipeline   (owners cycle on traffic)

HTTP     N single-thread runtimes, own SO_REUSEPORT listener each
         ├── runtime 1 → 1 worker     connection lives and dies here
         ├── runtime 2 → 1 worker
         └── runtime N → 1 worker     N = max(1, cores/2), configurable; N=1 until splicebench says otherwise

API      └── runtime → 1 worker       dashboard · history reads

Rules    └── runtime → 1 worker       list fetch; compile on its blocking pool, threads exit on idle

RULE     a block is freed on the thread that allocated it, or that thread has exited
```

Each HTTP runtime is its own allocation domain (own thread heap); "HTTP" in the
telemetry is the sum of N. Tokio's multi-thread runtime is work-stealing by
design and cannot pin a task to a worker, so "no migration" means one runtime
per thread. What is lost is work stealing inside HTTP: a thread with one heavy
splice cannot hand its other connections to an idle neighbour. Accepted for a
household proxy; the classic thread-per-core trade.

Only the DNS base runtime scales with `available_parallelism`; every isolated
runtime is single-threaded. Counts are overridable in config, never hard-coded
in the binary. The original `max(1, cores / 2)` for HTTP was the pre-test
proposal and is withdrawn by step 1 of the validation.

Compile keeps today's property: it runs on blocking threads that exit after
10 s idle, so its heap is abandoned and its pages are freed by whoever drops
the old ruleset. A persistent compile thread would break this (its pages would
become remote frees waiting for a thread that wakes once an hour). The Rules
runtime exists to move the *downloads* (28 MB of list bodies plus HTTP client
state) off DNS worker heaps, not to change how compile is threaded.

Telemetry writers (snapshot, perf row, rollups) stay where they are: a few
hundred KB per 300 s on blocking threads, too small to own a domain.

## Ownership and hand-off rule

An object is dropped on the runtime that allocated it, or it came from a
thread that has already exited. The retention case seen so far (F29) is:
allocated by a thread that then parks for good, freed by another. Stated as
the observed mechanism, not as a closed rule — the arena/domain design below
is unvalidated and step 1 of the validation showed the mechanism biting the
design itself (an isolated runtime *creates* owners that park for good).

Cross-runtime traffic therefore uses `Arc` (shared, read-only, last dropper
frees into abandoned or still-cycling pages) or a channel where the receiver
owns the drop. No `Bytes` / `Vec` allocated in one runtime and dropped in
another. The ruleset swap is the good case: built on a thread that exits,
dropped later by a DNS worker.

## Allocation domain = arena (PROPOSED, UNVALIDATED)

mimalloc heaps are per thread; a "domain heap" is the aggregate of a runtime's
thread heaps. Two ways to make that aggregate measurable:

| | A — exclusive arena per domain (preferred) | B — heap tag + self-visit on park (fallback) |
| --- | --- | --- |
| Mechanism | reserve one exclusive arena per runtime; on each worker start (`on_thread_start`, blocking pool included) create a heap in that arena and set it as the thread default | tag each runtime's heaps; each worker walks its own heap when it parks (rate-limited), writes committed/used to an atomic slot |
| Attribution | kernel: `/proc/self/smaps` `Rss` per arena address range, read by the existing 10 s poll | mimalloc's own view, summed per tag |
| Isolation | physical: a domain's pages never mix, even on cross-thread frees; a rule violation shows as growth in the origin domain | per-thread heaps only |
| Hot path | zero: heap chosen once per thread, same `mi_malloc` fast path | zero on the request path; one page walk per park |
| Open questions | exhaustion policy of an exclusive arena (fallback to OS vs `NULL`); whether huge blocks (24 MB matcher arena, 28 MB list text) bypass the arena; `libmimalloc-sys` `extended` feature exposing `mi_reserve_os_memory_ex`, `mi_heap_new_in_arena`, `mi_heap_set_default`, `mi_arena_area`; an "other" domain for main/reporter threads so the sum matches process RSS | none known; weaker attribution |

Reservation is virtual, so 256 MiB–1 GiB per domain costs nothing resident.
Nothing above has been tried; A needs a one-day dev-box prototype before it is
believed.

## Per-domain memory telemetry

Per domain, in `/debug/memory` and the perf sample: `rss`, `committed`,
`accounted` (ruleset in Rules; cache and stats in DNS; 0 elsewhere) and the
derived `residual = rss − accounted`. Eight to twelve `u64` per row. The
question it must answer, from `/history/perf` alone:

```text
RSS +8 MiB
  ↓
DNS   +0.2
HTTP  +7.4   ← culprit / floor climbed
API   +0.1
Rules +0.3
```

Complements the per-listener `concurrent_connections` high-water mark
(`4eddc39`): that names the *event*, this names the *owner*.

## Validation plan

| Step | Arms | Measure | Pass |
| --- | --- | --- | --- |
| 1. HTTP A/B, 2 workers | `httprt4-b` (`db2f9b2` + HTTP on its own 2-worker runtime, throwaway worktree `E:/FastAdHunter-var-httprt`) vs `ctl4-b` (`db2f9b2`), both `TOKIO_WORKER_THREADS=4`, 50 QPS DNS, F14 burst (3 × 500 keep-alive) at 15 min | residual at +15 min minus pre-burst, same-time control | **FAIL** (2026-09-05 18:28Z): pre 11.8 / 11.0, peak 28.6 / 27.9, done+9 **+11.6 / +0.2**, done+17 **+12.3 flat / +1.7**. Isolated runtime retains 6× the shared pool. Reading: after the burst the 2 HTTP workers park for good; frees that landed on the other HTTP worker (task migration) wait for an owner that never runs again (F29). The shared pool heals in 9 min because 50 QPS DNS keeps all 4 owners cycling. Series `E:/fah-diag/out/httprt/*-2w.jsonl` |
| 1b. HTTP A/B, 1 worker | same, `FAH_HTTP_WORKER_THREADS=1` | same | **Mechanism confirmed, floor effect not shown** (2026-09-05 19:11Z): pre 11.2 / 11.0, peak 25.3 / 29.3, done+0 **+1.8 / +15.2**, done+1 +1.4 / +9.6, done+9 +2.0 / +1.5, done+17 +3.7 / +1.9. The single-worker runtime returns the burst within one 30 s sample (every free is local, pages empty, purge takes them); the shared pool needs ~9 min of DNS-driven owner cycling. Final residues are inside the ±6 MB purge band (measurement-traps §Memory): no floor difference demonstrated at +17 min. Series `E:/fah-diag/out/httprt/*-1w.jsonl` |
| 2. API A/B | same shape; API on its own 1-worker runtime; load = repeated full-row `/history/perf` reads over a 4 000-row history | residual at +15 min | below control; note hist4 already showed only +2.0 for six reads on `db2f9b2` at 4 workers, so the effect may be small |
| 3. Arena prototype | one arm with A applied to HTTP only; burst; read `smaps` per arena range | HTTP range moves, DNS range flat | attribution matches the per-arm residual |

Step 1 (2 workers) failed and inverted the expectation: a multi-worker isolated
runtime is worse than the shared pool at production idleness, because
isolation creates owners that park for good. Step 1b (1 worker) confirmed the
mechanism: with no intra-runtime migration the burst is returned within one
sample, while the shared pool heals in ~9 min through DNS-driven owner cycling.
Neither run showed a floor difference at +17 min (both inside the purge band).

Consequences for the design:

- **Isolated runtimes must be single-threaded.** A multi-worker isolated runtime
  recreates F29 inside itself. If a domain ever needs more than one thread, it
  is N single-thread runtimes, each with its own listener, not one N-worker
  runtime. The "derived worker count" principle therefore reduces to `1` per
  isolated domain; only the DNS base runtime scales with cores.
- **The topology is not a floor fix on current evidence.** The shared 4-worker
  pool already heals at production traffic (F26, F32); the first-contact steps
  (+2..+5, F33) are small and were not separated from noise here. What the
  topology buys is instant return after a burst and, with step 3, attribution.
  Whether that is worth 3 threads is the owner's call, stated as such.
- Step 2 (API) and step 3 (arena) remain open; step 3 is the one that decides
  whether the topology pays.

## Router benchmark (arm64, before any ADR)

Dev-box results are x86 and say nothing about the RB5009. Every row is an A/B:
**A** = 0.3.2 as deployed, **B** = proposal build; same router, same night hour,
same LAN client, same script; sequential (one container on the router), the
`concurrent_connections` row confirming each burst reached 500. Owner deploys
and runs the client side; everything else read-only.

| # | Measure | Method | Pass |
| --- | --- | --- | --- |
| 1 | Burst residue | F14 burst from a LAN machine to `router:8080`; residual from `/debug/memory` before, peak, +15 min | B ≤ A |
| 2 | DNS latency under burst | `telemetry.latency.dns` means and perf-row `cache_hit_p99` / `block_p99` for the burst interval vs the hour before (F27 method) | B ≤ A within noise |
| 3 | HTTP throughput | one fixed payload (LAN HTTP server or pinned URL) through `:8080`; `curl -o /dev/null -w '%{speed_download} %{time_total}'`; **5 runs, median and p95**; two shapes: (a) 1 download, (b) 8 parallel downloads, aggregate and per-stream p95 | B within 10 % of A on both shapes, or the loss stated and accepted in the ADR. PERFORMANCE.md rows are a sanity ceiling, not the comparator |
| 4 | CPU during 1 and 3 | `/system/resource/print` `cpu-load`, `/container/print detail` `cpu-usage` | reported |
| 5 | Shutdown | `/container/stop` with open connections, inside `stop-time=10s`, no SIGKILL in `/log` | pass/fail |
| 6 | 7-day soak on B | standing gates G1–G5, plus per-domain telemetry if step 3 shipped | all pass |

0.3.2's own soak supplies the A half of rows 1, 2, 4 and 6 for free.

## Scope

For the first time the question "is the redesign worth it?" gets an objective
answer, from the six rows above:

```text
MEMORY    → B better, or at least equal?
DNS       → B without a relevant regression?
HTTP      → B fast enough?
CPU       → reasonable cost?
SHUTDOWN  → safe?
7 DAYS    → stable?
```

If B passes, the redesign is no longer an interesting Rust/mimalloc idea. It is
an architecture measured on the RB5009 and justified by results. This block is
the verdict rule of the ADR, verbatim.

If B does not pass, nothing is lost: the measurement shows that the simplicity
of the current runtime deserves to be kept, and it says so with a number.

One precision, so the outcome cannot be misread later. On MEMORY the honest
expectation is a **tie on the floor** (F36 showed no difference at +17 min) and
a **win on return time** (within one sample vs ~10 min). "B passes" will most
likely read: floor equal, return faster, attribution per domain, cost N threads.
The redesign is then justified by observability and isolation, not by a lower
RSS number. Anyone expecting 64 → 50 MiB from it is expecting something the
evidence does not promise.

## Costs

~4 extra worker threads plus per-runtime blocking pools: +3..5 MiB baseline for
heaps and stacks, judged acceptable if it buys attribution. Cross-runtime
hand-offs must follow the rule above or they recreate F29 between domains.

## Execution prompt (hand-off)

Self-contained prompt for the session that executes the plan. Written
2026-09-06; every "go" below is a separate owner decision.

```text
Role: build and bench 0.3.2 (A) and the allocation-domains build (B) for
FastAdHunter on the RB5009. Read-only against production and the router;
experiments on the dev box only. Owner rules, absolute: NO router change (read
-only queries fine); NO commit, tag or push without a go for that changeset; NO
.md edit without a go per file; scp needs permission; production only via GET
with the key in E:/FastAdHunter/.vscode/production.key read into a shell
variable, never printed. Answer in English; clock times as UTC with Z, then
Bucharest (UTC+3).

Read first: this file (allocation-domains-proposal.md), then
resoak-0.3.1-memory-diagnosis.md §Summary + F27, F29, F32–F36, then
docs/routeros-traps.md §Build and deploy pipeline, docs/measurement-traps.md
§Memory.

Established, do not re-derive:
- Deployed production = 0.3.1 = commit db2f9b2 (on main), mode dns+http, one
  tokio runtime, 4 workers, mimalloc. Soak T0 2026-09-01T07:27:49Z; day 7
  closes 2026-09-08T07:27:49Z (10:27:49 local). Nothing is deployed before the
  owner's day-7 read.
- HEAD of phase3-06 = db2f9b2 + 52 commits: all of Phase 3 (fah-certs, SNI
  filtering, interception, DoT/DoH) + fixes + 4eddc39. Not soaked. 0.3.2 is
  NOT HEAD.
- 4eddc39 = per-listener concurrent_connections high-water mark in the perf
  sample (fah-http ConnectionGauge; fah-model ConcurrentConnections {http,
  https}; fah-stats reader; fah-api ?fields=; API.md). Cherry-pick onto
  db2f9b2 conflicts in crates/fah-http/src/lib.rs and server.rs (Phase 3
  reshaped them) and crates/fah-http/src/tls_server.rs (absent at db2f9b2);
  the other 11 files apply clean.
- musl allocator rejected by the owner (F27: +4..12 % per cached/blocked
  query). mimalloc stays. Isolated runtimes must be single-threaded (F36).
- The RouterOS "used" graph counts page cache; an uploaded image tar shows as
  used until deleted (F34). Delete the tar after the container starts.

Part A — release 0.3.2 = db2f9b2 + the counter, nothing else.
 A1 (go) git branch release/0.3.2 db2f9b2; git cherry-pick 4eddc39; resolve:
    apply the gauge to the 0.3.1 accept loop in server.rs (field, bind,
    serve, accessor, accept_loop param, enter() before spawn, guard held with
    the permit), lib.rs mod + pub use, drop tls_server.rs (https stays 0).
    No comments in Rust code (hook rejects them).
 A2 cargo fmt --all -- --check; cargo clippy --workspace --all-targets
    --message-format=short -- -D warnings; cargo test --all-features
    --workspace. All green before A3.
 A3 (go) bump workspace version to 0.3.2 in Cargo.toml (+ Cargo.lock);
    commit "chore(release): 0.3.2 — 0.3.1 plus the concurrent_connections
    high-water mark"; tag v0.3.2. Push only on a further go, to origin AND
    backup.
 A4 arm64 image per routeros-traps §Build and deploy pipeline: re-register
    QEMU after any Docker Desktop restart, buildx --platform linux/arm64 to
    an OCI tar, skopeo to docker-archive fastadhunter-arm64-0.3.2.tar.
 A5 (permission) scp the tar to kingston/. Then propose the exact /container
    commands for the swap (add reusing interface=veth1, root-dir, mountlists
    fah-config,fah-data, envlists fah-env, workdir, start-on-boot, logging,
    comment) and STOP; the owner runs them, after the day-7 read. Delete the
    tar from kingston after the container is up.
 A6 soak 0.3.2 with a pre-declaration file (go per .md): standing gates
    G1–G5, plus: every evening spike ≥ 80 MiB is read against the
    concurrent_connections row of the same interval. This soak is the A half
    of router-benchmark rows 1, 2, 4, 6. Rows 3 and 5 need one dedicated
    night on A (owner runs curl / the burst script from a LAN machine).

Part B — the allocation-domains build, on release/0.3.2, topology only.
 B1 (go) branch alloc-domains/0.3.2 from v0.3.2. Implement, in this order,
    each with tests: HTTP server group (one acceptor, N current_thread
    runtimes on own threads, hand-off as std::net::TcpStream re-registered
    with TcpStream::from_std inside the target runtime; N from config,
    default 1; ConnectionGauge shared across N); shutdown (stop acceptors →
    drain with timeout → shutdown_background per runtime after the main
    block_on returns; must finish inside RouterOS stop-time=10s); API on a
    1-worker runtime; Rules on a 1-worker runtime (scheduler + fetch client
    created inside it; compile on its blocking pool, threads exit on idle;
    POST /lists/refresh must spawn there, not on the caller); [runtime]
    config section (dns_workers, http_runtimes; api/rules fixed 1; defaults
    derived from available_parallelism). No arena, no per-domain telemetry
    in this build: one variable.
 B2 gates as A2, plus: crates/fah-http/examples/splicebench.rs on 1 thread;
    F14 burst at 4 DNS workers on the dev box vs v0.3.2 (residual pre/peak/
    +15 min); SIGTERM drain test with open connections.
 B3 (go) commit; version 0.3.2-domains.1 (pre-release, not a release);
    arm64 image as A4; (permission) scp; propose the swap; owner deploys at
    the same night hour as the A rows were taken; delete the tar.
 B4 router benchmark rows 1–6 of this file on B, same script, same LAN
    client, same hour. Compare with A. Fill the table in this file (go).
 B5 verdict per §Scope. If B passes: ADR (topology, single-thread rule,
    derived counts, ownership rule, telemetry plan), CONTEXT.md entry
    "allocation domain", CONFIGURATION.md [runtime] keys, forward-port to
    main after phase3-06 merges — each .md its own go. If B fails: record
    the numbers here, keep the shared runtime, close the proposal.

Part C — only after B passes, separate increment: arena-per-domain
 telemetry (this file §Allocation domain = arena, option A, fallback B),
 prototyped on the dev box first (arena exhaustion policy, huge blocks,
 libmimalloc-sys extended API), then its own A/B on the router.

Stop points: after A2 (before the release commit), after A4 (before scp),
after B2 (before the commit), after B3 (before scp), and before every .md.
```

## Remaining TODOs

- Step 2 and step 3 on the dev box, one day, if the owner wants the topology for
  attribution rather than for the floor.
- If all three pass: ADR (topology, derived counts, ownership rule, telemetry),
  CONTEXT.md entry for *allocation domain*, CONFIGURATION.md keys for the
  worker counts, API.md fields for the per-domain block. Each needs its own go.
