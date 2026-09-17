# Review — audit closures of 2026-09-17 (`b0c7709`, `067427a`)

## Summary

- Scope: the two commits on `main` ahead of `origin/main`. `b0c7709` closes
  SP2, N2 and R2 from [post-merge-audit-2026-09-15.md](post-merge-audit-2026-09-15.md);
  `067427a` closes S1, S3, S7, TODO row 9 and two doc-drift rows from
  [hot-path-audit-dns-http.md](hot-path-audit-dns-http.md).
- Every unit the commit messages claim exists where they say and does what the
  finding's "smallest fix" asked. No undocumented deviation.
- No hot-path change: `clean` runs on the blocking pool, `qtype_name` serves
  the event feed, the DoT pre-warm is once per handshake.
- No defect found in the code. One gate figure recorded in both audit files is
  wrong (F1). Two notes on what the SP2 deadline bounds (F2, F3). Comment
  drift in three places the batch touched (F4). One impossible fixture (F5).
- Gates re-run on this tree: §Measurements.
- Verdict: **PASS WITH DEFERRED FINDINGS**.

## Findings

Severity-ranked.

### F1 — LOW · both audits record `vitest run` as 625 passed; the suite reports 1056

**CLOSED 2026-09-17** — both lines now read 1056, on the owner's approval.
The row below is the state that was found.

- [post-merge-audit-2026-09-15.md:721](post-merge-audit-2026-09-15.md),
  [hot-path-audit-dns-http.md:1343](hot-path-audit-dns-http.md).
- `npx vitest run` on this tree: 58 files, 1056 tests passed. The sources hold
  1034 `it(` blocks plus one `it.each`. 625 matches nothing in the suite and
  appears nowhere else under `docs/`.
- Why it matters: the paragraph exists so the next gate run can be compared
  against it. A wrong baseline hides a lost test file, or sends someone
  hunting for a regression that is not there.
- Action: replace both with the figure `vitest run` prints. `.md` edit — needs
  the owner's yes.

### F2 — LOW · the SP2 deadline bounds the slot, not the mint

- `crates/fah-dns/src/dot.rs:143-150`, `:193`.
- On expiry the connection closes and the slot is freed, but the
  `spawn_blocking` mint keeps its blocking-pool thread, or its queue place,
  until `store.prewarm` returns; dropping the `JoinHandle` cancels nothing.
  Before the change outstanding mints were bounded by the 64 slots. Now they
  are bounded by 64 per `HANDSHAKE_TIMEOUT` window (10 s) for as long as the
  pool is stalled, each holding an `Arc<CertStore>` and the host `String`.
- Steady state is unchanged: a mint is ~450 µs on the RB5009 (PERFORMANCE.md),
  so nothing accumulates unless the blocking pool is starved, which is a
  process-wide failure on its own. Unique hosts were unbounded before this
  change too; the leaf cache's single-flight already serialises a same-host
  burst.
- The §SP2 closure text says the mint "runs to completion" but not what
  bounds how many are running.
- Action: none required. One sentence on the bound in §SP2 if the owner wants
  it recorded.

### F3 — INFORMATIONAL · a pre-warm timeout is counted nowhere

- `dot.rs:147`. The `JoinError` arm goes through `tcp::report_prewarm_failure`
  (`tcp.rs:159-171`), which counts into `prewarm_failures` and warns under
  throttle. The new timeout arm logs at `debug!` and counts nothing.
- Defensible: the deadline is shared with the ClientHello read, so a pre-warm
  timeout can be a slow client rather than a slow mint, and counting it as a
  mint failure would mislead. Recorded so a soak that shows the debug line
  knows there is no counter to read.
- Action: none.

### F4 — INFORMATIONAL · comment drift in three places the batch touched

- `cache.rs:648`: "`saturating_sub` matches `remove`/`clean`" — `clean` now
  assigns; only `remove` and the eviction loop subtract.
- `response.rs:154`: "TCP callers pass `u16::MAX` so truncation never
  triggers" — the new test at `:292` proves hickory's encoder truncates at
  65535 with `TC` set; only the budget comparison never triggers.
- `wire.rs:253`: "whatever the resolver called it for everything else" —
  predates `Other(code)`; the spelling is `TYPE<n>`.
- S7 was closed by rewriting two `.ts`/`.tsx` comments in the commit right
  after rule 7 declared the TypeScript gate. Deleting the two comments would
  have satisfied S7 and rule 7 at once.
- Action: rule 7 forbids adding comment lines; deleting the stale clauses is
  allowed. Fold into the next touch of each file.

### F5 — INFORMATIONAL · the Health fixture pins a state the UDP gauge cannot produce

- `diagnostics-health.test.tsx:91`:
  `dns_udp_inflight: { active: 0, peak: 0, shed: 4 }`. `UdpInflightGauge::admit`
  (`udp.rs:43-65`) sheds only under a limit, and the first query admitted
  under a limit sets `peak ≥ 1`. Harmless — the test pins the rendering of
  four rows, not gauge semantics.
- Action: none; `peak: 1` makes the fixture true if the file is touched again.

## Checked and found acceptable

| Area | What was checked | Verdict |
| --- | --- | --- |
| SP2 correctness | `timeout_at(deadline, prewarm(..))` shares the deadline with the ClientHello read and `into_stream`; the timeout arm returns `Ok(())` like its two neighbours, so `report_connection_end` and the `OpenConnection` drop run unchanged and the permit leaves with the task | correct |
| SP2 store claim | `CertStore::prewarm` inserts inside the blocking closure (`store.rs:431`); no fs I/O in `leaf.rs`; an abandoned mint still lands, and the test's `TempDir` drop cannot race it | correct |
| `diag-timing` path | `&mut diag` is borrowed by the future and released on its drop; clippy with `--all-features` clean | correct |
| SP2 test | one worker, one blocking thread held by `occupied.recv()`; `TcpStream::connect(SocketAddr)` and rustls need no blocking thread; elapsed ≥ 250 ms is a deterministic lower bound (deadline 300 ms, `timeout_at` never fires early); `await_gauge` carries its own 2 s guard; the queued mint runs after `release.send` and runtime drop waits for it | not timing-flaky |
| S1 correctness | `retain` visits every entry once; `entry_heap_bytes` depends only on immutable fields (key length, record counts); `Shard::bytes` has no contributor but entries (`:664`, `:348`, `:356`), so the assigned total is the invariant's definition | correct |
| S1 cost | a few adds and multiplies per kept entry inside a walk that already calls `entry.state`; `clean` runs on the blocking pool (`pipeline.rs:240`), one shard lock at a time — not the query path | none on the hot path |
| S1 "heals at the next sweep" | a panicking sweep surfaces as `JoinError` at `pipeline.rs:240-247`; the scheduler `continue`s, so the next tick runs; `lock_shard` recovers a poisoned shard | claim holds |
| S1 test | skews all 16 shards by 2^40, sweeps with `purge_stale = false`, expects the pre-skew total and `entries_after == 2`; on `saturating_sub(freed)` with `freed == 0` the skew stays | proves the fix |
| TODO 9 test | 1200 A records under distinct 60-byte labels encode to ~94 KB; assertions pin `failure.is_none()`, `len ≤ 65535`, `TC` set, fewer answers, and the framed prefix equal to the payload; `frame_reply`'s `unwrap_or(u16::MAX)` stays unreachable | proves the invariant |
| S3 | output unchanged for all 16 arms (`wire.rs:1296`, `:1304`); `qtype_label` had no other caller; serves the event feed and the rule tester, not the DNS path | correct, no scope creep |
| R2 | rows `dot–dns` and `dot–http` added; blamed key `dns.listen.dot_port` follows the table order R4 describes (`lib.rs:541` is last) | every pair asserted |
| N2 shape | field names match `fah_model::DnsTcpConnections` / `DnsUdpInflight` (`engine.rs:99-112`); the dashboard ships inside the binary, so producer and consumer cannot skew; `Counters` has one fixture, updated; `tsc --noEmit` clean | correct |
| N2 footnote | "stay 0 while `udp_max_inflight` is 0": `admit` returns `true` without touching `active`/`peak` and `release` is a no-op when `limit` is `None` (`udp.rs:43-71`); "compiled-in cap of 64 with no config key": `DOT_MAX_CONNECTIONS = 64`, no `dot_max*` key in `fah-config`; `tcp_max_connections` and `udp_max_inflight` sit under `[dns]` (CONFIGURATION.md:92, :101) | accurate |
| CLAUDE.md rule 7 | the hook's case arm is `*.rs\|*.ts\|*.tsx` (`no-rust-comments.sh:25`) | accurate |
| API.md | `TYPE<n>` under §Events and the `/rules/test` row; `parse_qtype` accepts the spelling (`wire.rs:302`); S2's silent `TYPE0` coercion stays open and undocumented, as the audit leaves it | consistent |
| Commit hashes named in the doc edits | `2eb5018`, `3e97d16`, `52eec09`, `af5cf61`, `b67ecec`, `f32f214` all resolve | accurate |
| Hard rules | layering untouched; no new Rust comment; no new allocation, lock or atomic on the query path; nothing grows with traffic beyond F2's bound | hold |

## Plan compliance

| Finding | Asked for | Delivered | Match |
| --- | --- | --- | --- |
| SP2 | owner decision; verification with `max_blocking_threads = 1` | pre-warm inside `timeout_at`; test with a one-thread pool held by a stalled task | yes |
| N2 | a Health row for the three gauges, or an explicit line | four rows and a footnote on the Backpressure card; test pins the rows | yes |
| R2 | add the two cases, or rename the test | two cases added | yes |
| S1 | sum the kept entries in the walk; assign `guard.bytes` | exactly that; the eviction loop keeps its subtraction, as the audit says | yes |
| S3 | delete the arm or route `qtype_name` through it | folded; the `"OTHER"` literal is gone | yes |
| S7 | correct two comments | corrected | yes (see F4) |
| TODO 9 | a >64 KiB TCP reply test | added in `response.rs`, framing included | yes |
| Doc-drift rows | name `af5cf61`; `TYPE<n>` in API.md | done | yes |
| Undocumented deviations | — | none; the perf-audit corrections (A3, A6, three counts) are named in `b0c7709`'s message | — |
| `.md` permission | every `.md` edit needs the owner's yes | both messages say "on the owner's approval"; not verifiable from the tree | assumed |

## Measurements — gates on this tree (Windows dev box, 2026-09-17)

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test -p fah-dns -p fah-config -p fah-api` | 228 + 98 + 149 unit tests and every integration target green, 0 failed |
| `cargo test --all-features --workspace` | 1659 passed, 0 failed, 12 ignored across 62 targets — matches the 1659 the hot-path audit records |
| `npx tsc --noEmit` (dashboard) | clean |
| `npx vitest run` (dashboard) | 58 files, 1056 passed — the audits say 625 (F1) |

## Files reviewed

Both commits in full: `dot.rs`, `cache.rs`, `response.rs`, `wire.rs`,
`fah-config/src/lib.rs`, the four dashboard files, `API.md`, `CLAUDE.md`, the
three audit files. Read for dependency evidence only: `dot.rs:60-215` and its
test helpers, `cache.rs:250-260`, `:330-362`, `:640-668`, `:760-840`,
`pipeline.rs:232-256`, `tcp.rs:159-171`, `:221-224`, `udp.rs:40-82`,
`store.rs:431-456`, `engine.rs:99-112`, `no-rust-comments.sh:25`.

## Remaining TODOs

- F2: one sentence on the mint bound in §SP2, if wanted.
- F4: delete the three stale comment clauses on the next touch of each file.
- S2, S4–S6, SP8, R3–R7: open before this batch, untouched by it.

**PASS WITH DEFERRED FINDINGS** — no defect in the code of either commit; F1
was a wrong number in two docs, closed the same day; F2–F5 are notes.
