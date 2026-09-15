# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-09-15 (second pass — the p3-11 binary-level arms)

## Now

| | |
| --- | --- |
| Branch | **Phase 3 landed on `main`** on 2026-09-13, by fast-forward — `bc49e4e..78238b4`, 105 commits, no merge commit ([plan/plan-merge.md](../plan/plan-merge.md), all five steps closed). `main` is now **`99e4953`** and **`origin/main` and `backup/main` are both there** — nothing local, nothing unpushed. Twenty-three commits followed the Phase 3 tip `180beb8`; the last ten before this session are listed in §Session 2026-09-14/15, and the six from 2026-09-15 are `1055d1b`, `d93f4ad`, `5f91db5`, `5b346d7`, `dac5be0` and `99e4953` — tests and documentation only, no production code. `phase3-06` (`78238b4`) has served its purpose and sits well behind. Rollback tags: `main-pre-phase3-merge` = `ebc46f1`, `phase3-06-pre-main-merge` = `185139b`; `eb693e2` is the merge commit inside the branch, two parents. `pre-alloc-domain-2026-09-06` = `64be513` stays the rollback point before the allocation domains |
| Tree | **clean — nothing modified, nothing untracked.** One stash remains, `stash@{0}` ("phase3-06 project-state Next row"); it predates this work and is not ours. The last change to land was `99e4953`, the measurement trap about a ceiling that takes its own gauge down with it |
| Tests | **The gate ran on the tip, 2026-09-15, Windows dev box: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --all-features --workspace` 0 failed across 62 targets.** `shipped_path_e2e` now carries six tests in 21 s — four added on 2026-09-15 for the HTTPS listener's own edge cases, the last of them mutation-verified (`max_connections: 2` fails its negative control). The dashboard ran on the same tip and is green: `npm run typecheck` clean, **58 test files, 1 043 tests, 0 failures** (three fewer than the 1 046 of 2026-09-14 — the `fallback` mode's tests went with the mode in `5b5d3e8`), and `npm run build` is under budget at 136 921 B gzip against 153 600 B. `cargo bench -p fastadhunter --bench pipeline --no-run` builds for the first time since p5-04, with no profile override (`3a1b5b8`). The `e2e` `WSAEACCES` trap (§Known-good gate note) did not fire. Bench A/B against `main` over four alternating rounds: **no regression demonstrated** |
| Version | 0.3.4 (workspace, since `4e7a6de`), untagged. Newest tags `v0.3.2` (`89aac76`), `pre-alloc-domain-2026-09-06`, `soak-p2.6-11` |
| Deployed | **0.3.4, and not from today's merge.** Nothing was deployed on 2026-09-13 — landing Phase 3 on `main` is not a deployment, and deploying it is a separate decision that has not been made. Production runs image `kingston/fastadhunter-arm64-0.3.4.tar` as container `fastadhunter-0.3.4`, booted **2026-09-11T22:02:48Z**, HTTP allocation domains, N=2 (`FAH__RUNTIME__HTTP_RUNTIMES=2` on `fah-env`), `veth1` / `172.17.0.2`, mounts `fah-config,fah-data`. `GET /health` on 2026-09-13 18:24 local answered `0.3.4`, uptime 148 853 s. **The build commit is recorded nowhere** — neither `/health` nor the soak capture carries one; by timestamp the image matches `d307c36` with the version already at 0.3.4, the bump itself committed three minutes after the boot as `4e7a6de`. A **seven-day soak is running on this build**: t0 `2026-09-11T22:13:21Z`, ending ~2026-09-18T22:00Z, hourly scheduled task `FAH-soak-0.3.4`, artefacts under `docs/code-review/phase2.6/soak-0.3.4/` — untracked and gitignored, so they live outside the repository. The p3-06 probe (`fah-probe` on `veth3` / 172.17.0.4) was torn down 2026-09-11 evening; `veth3` remains, no test firewall rule or address list remains. Deploying Phase 3 is a separate decision and it has not been made |
| Build ≠ tip | the running 0.3.4 **has** the F10 stats flush (`32d7776`), the F1 TCP bound (`ed28395`), the F2 UDP ceiling (`b0b091e`), their close-out tests (`ad110d8`) and adaptive-only (`d307c36`) — every one of them committed before its 01:02 local boot. It **lacks** everything from the day after: the F11 supervisor (`0fb8dd0`), F3's query borrow and pre-sized `domain_of` (`07d4d68`, `c220956`), the H1-H3/D1 allocation removals (`3287418`), the idle upstream pool reaper (`8941770`), the HTTP refusal-counter split (`28c751d`) — and the whole of Phase 3: certificates, SNI filtering, HTTPS interception, the DoT and DoH listeners. The `strategy = "adaptive"` precondition is settled rather than pending: that boot already happened and the config loaded |
| Phase | **0–2.6 and 5 closed** (2.6 closed 2026-09-07, all 13 tasks `DONE`; `p2.6-12` reached `main` on 2026-09-11 as the cherry-pick of `fa9451a` — `adaptive` is the only strategy, the `fallback` walk is deleted, a config naming it fails at load). **3 is now in `plan/wip/phase3` on `main`** — the `open` → `wip` move landed with the merge, by the owner's decision (plan-merge.md §Step 3). `p3-01`…`p3-05` `DONE`, `p3-06` and `p3-06b` `PARKED` (2026-09-13, the interception decision), `p3-07`…`p3-09` `DONE` (ADR-0008: Interception Document, 525 classification, rejection view + editor). **`p3-10b` and `p3-10c` are `DONE`, both 2026-09-14** — the DoT connection gauge (`counters.dns_dot_connections`, counted at accept so a stalled handshake is in the figure that sizes the cap) and the HTTP, HTTPS and API acceptors reporting an unplanned end through `record_task_death`. Row 11's soak waits for neither. `p3-10` and `p3-11` stay `WAITING` — the only two the selector will pick. **N3 follow-up closed 2026-09-11 as a technical experiment, not promoted** — the client alert names the TLS stack, not the cause; HTTP/3 must be refused for intercepted clients ([p3-06-n3-alert-ab.md](code-review/phase3/p3-06-n3-alert-ab.md)) |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): §5.1–6 cleared; §5.7–14 gate Phase 3. S1-G2 tiers 1–3 met; S1-G4 and S1-G5 route 2 not validated and will not be |
| **Next** | **What can start today, needing no decision: the dev-box half of p3-11.** Part of it has landed — `crates/fastadhunter/tests/shipped_path_e2e.rs` (`095163b`) walks DNS over UDP and TCP, plain HTTP, an SNI block, an SNI splice through an allocation domain, DoT and DoH in one run with `clients` empty, every layer carrying an allowed control beside the blocked case; the security suite landed 2026-09-14 (`00698ee`); on 2026-09-15 four more binary-level arms closed two of the coverage table's three `no` rows (`d93f4ad`, `5f91db5`), leaving HTTPS lane saturation, which was deliberately left open. The deploy guide's HTTPS section is done too: §5c was audited against the live router on 2026-09-15 and three defects fixed (`dac5be0`) — rollback left the two QUIC rejects behind in `filter`, the intro counted two rules where there are up to seven, and the order was implicit, though the QUIC reject has to land before the dst-nat or a browser on HTTP/3 walks past the steer. **What is left on the dev box is Private DNS on the device, which waits for the deploy anyway. The SNI/DoT/DoH budget rows are not dev-box work** — every figure in PERFORMANCE.md's table is a column headed `Measured on the RB5009`, and its SNI and DoT/DoH rows say `TBD — set from the RB5009` because loopback and TLS legs do not convert (§Conversion, line 141). A dev-box bench is a diagnostic on the way to those rows, never the row itself. **Blocked on the deploy decision:** p3-10's B2 sweep on the RB5009 (it also owes p3-11 the dst-nat 443 rule) and p3-11's device half with its seven-day soak. **Their own go, and both waiting on more than a decision to write them:** the DoT/DoH load generator and a verified runner for splice RSS. Neither can produce an interpretable figure before p3-11 fixes the budgets on the device — PERFORMANCE.md's `DoT / DoH added latency vs UDP, p50` is still `TBD` and the TLS/HTTP legs do not convert from x86 ([p3-10-track-b1-x86.md](code-review/phase3/p3-10-track-b1-x86.md) §Remaining TODOs, `e028328`). In flight and needing nobody: the **0.3.4 soak ends ~2026-09-18**, and its counters settle the F1 `tcp_max_connections` default (§Risk inventory close-out). Interception ships **disabled** through an empty client scope, not through `engine.mode` — and since 2026-09-13 that is the owner's decision rather than a default awaiting a switch: `p3-06` and `p3-06b` are `PARKED`, their non-interception arms moved to p3-11, and the phase's definition of done no longer names interception. Off the critical path: dashboard settings metadata for `runtime.http_runtimes` — a deliberate omission, since `metadata.ts` is an acknowledged subset and the raw All-settings panel renders every unmodelled key — the TLS / lol_html capacity microbench, and alloc 11b (`Connection::graceful_shutdown` into `Proxy::serve_connection`; 11a already keeps DNS answering through the drain). **Before Phase 3 goes live:** the router refuses UDP 443 LAN→WAN, or HTTP/3 bypasses the steer. The rule's **placement was corrected 2026-09-14** — appended behind a final drop it does nothing — and the DNS escape routes are named beside it ([deploy-rb5009.md](deploy-rb5009.md), `73d79aa`) |

## Session 2026-09-14/15 — ten commits, all on both remotes

Newest first. Everything here is test, harness or documentation except
`2583c27`, which is the only production change of the session.

| Commit | What |
| --- | --- |
| `b495b05` | PERFORMANCE.md: the bench override is gone, the old absolutes are not. The recorded `full_pipeline` A/Bs used `CARGO_PROFILE_BENCH_DEBUG_ASSERTIONS=true` on both arms and stay fair; their absolutes stay ineligible as budget rows. Only runs from `3a1b5b8` onwards measure shipped codegen |
| `3a1b5b8` | `cargo bench -p fastadhunter` builds again, first time since p5-04. The dev-dependency no longer asks for `fah-api/test-harness` — cargo unified it into the bench profile, where `fah-api`'s `compile_error!` refuses it. `history_e2e.rs` is gated on the package's own feature instead. The security barrier in `fah-api` is untouched |
| `e028328` | p3-10 track B1: the two unrun DoT/DoH rows say what they wait on, not just that the tool is missing — the budget is `TBD` until p3-11 fixes it on the device, and x86 does not convert |
| `d4ab231` | ARCHITECTURE.md named `fallback` as the current upstream strategy. It has been rejected at load since 0.3.3 |
| `5b5d3e8` | The dashboard carried `fallback` as a live mode: a variant of `UpstreamMode`, three render branches, a prop gating half of every endpoint row, and a selectable settings value. All of it described a state no engine can report. `unknown` stays — it is also `GET /config` having failed |
| `2583c27` | **Production.** `fah-stats`' `append_line` wrote a record and its `\n` as two awaited calls, so an abort between them lost an hour of history; and it never flushed, so `tokio::fs`'s buffer could lose the line outright. The second defect was found by the test written to prove the first fix |
| `816dde7` | `adaptive_behaviour.rs` formatted. `d71ca0b` shipped unformatted because its gate used `cargo fmt \| tail`, and a pipe reports the wrong exit status |
| `7ab6645` | The DoT accept flake keeps its failure-only diagnostics, and its mechanism is now reproducible on demand in 0.48 s rather than once in ~810 loaded runs. Historical cause unassigned; see §Open, honestly |
| `b5355ca`, `533a180` | The `fah-dns --lib` flake that had been lost is named, and the handover replaced with the cause hunt |

### Open, honestly

- The **DoT accept flake**'s historical cause is **unassigned**, and stays that
  way until the instrumented assertion goes red again. The mechanism is
  reproducible and the production invariant is verified — `tls_handshakes == 2`
  passed in the recorded failure, so the pool opened exactly the connections it
  should. What is unknown is what opened the third one. Six hundred further
  runs under load found nothing;
  [allocation-oracles-that-only-hold-on-an-idle-machine.md](solutions/design-patterns/allocation-oracles-that-only-hold-on-an-idle-machine.md)
  §2 carries the verdict and the four readings its dial journal allows.
- `fah-api`'s `server.rs` warns that `slots` is never read when the crate is
  built without `test-harness`, which `3a1b5b8` made visible in an isolated
  bench build. Latent before, outside the workspace gate, untouched.

## Integration audit — status-pass 2026-09-14

[main-phase3-integration-audit.md](code-review/phase3/main-phase3-integration-audit.md)
was written 2026-09-08; p3-04…p3-09 and the merge have landed since, so every
finding was re-checked against the code rather than against the file (`a9e642d`).

Closed by the pass:

- **F1** — a documentation correction, no production code change.
  `allow_ip_literal_hosts` permits a direct IP-literal upstream connection on the
  HTTP proxy path **only**; an IP-literal SNI stays unsupported and is rejected
  at hostname resolution. CONFIGURATION.md claimed the switch governed both paths
  and now states the asymmetry. RFC 6066 forbids an IP literal in SNI and the
  switch is off by default, so the branch is deliberately not ported.
- **F2** and **F5** were already fixed by `8a5c809` on 2026-09-08, thirteen hours
  after the audit was written, and sat open for six days because nobody came back.
- **F6** closed by annotation: the stale section carries its own correction.

Still holding, none of them blocking — except F7, which closed on 2026-09-14:

- **F3** — recorded.
- **F7 — closed 2026-09-14** by `d71ca0b`. The `b5_recovery_and_flapping` p99
  guard was unrepairable, not merely weak: a capped penalty window is exactly
  one unit long, the flapping phase was 0.6 of one, so once the black-hole
  phase drove the backoff to its cap the window outlasted the phase on every
  arm and the comparison never exercised the property. It is replaced by a
  penalty-count oracle whose allowance is derived from `nominal_penalty_ms` at
  the 75 % jitter floor, all three arms run independently with their failures
  aggregated, and "insufficient samples" can no longer read as a pass.
  Mutation-verified: the oracle rejects the regression it is there to catch.
  The test keeps its `#[ignore]`; it is not a gate.
- **F8** — `tcp_max_connections` ships armed at 1024 while `udp_max_inflight`
  ships inert at 0 (confirmed on the live container, 2026-09-15). Inherited from
  `main`; no default was changed. **The inert bound also makes the gauge inert**,
  which F8 did not say: `admit()` returns before touching `active` or `peak`, so
  `dns_udp_inflight` reports `peak: 0` on a loaded build and that zero is not
  evidence. A ceiling that is off cannot be sized from its own telemetry, however
  long a soak runs ([measurement-traps.md](measurement-traps.md) §A metric can be
  present and not measuring, `99e4953`). No production task opened for it.
- **F9** — seven benches whose own-side spread on identical code is wider than
  the 10 % gate they are supposed to police, independently reconfirmed by the B1
  characterization on benches the audit did not cover.

`E:/fah-main-bench` is **kept**, detached at `ebc46f1`: the frozen pre-Phase-3
baseline p3-10 measures against. Rebuilt later it would be a different baseline,
not the same one. Do not switch its checkout or delete its `target/`.

## Risk inventory close-out — 2026-09-11

[project-risk-inventory.md](code-review/project-risk-inventory.md) surveyed
`main` at `baa2ecd`. Its three material findings — F1 (DNS-over-TCP had no
connection ceiling and allocated from the client's length prefix), F2 (UDP
in-flight queries unbounded) and F10 (no stats flush on a clean stop) — are
**closed**: fixed in `ed28395`, `b0b091e` and `32d7776`, verified by the
close-out audit, and moved to the inventory's §Closed with their evidence. No
material finding is open.

Follow-ups, neither a risk:

- **F1 soak — tuning only.** 1024 connections and 16 KiB per message are
  initial safety bounds. After 7 days on the RB5009 *with the F1 build*, read
  `counters.dns_tcp_connections.{peak,closed_oversize}` from
  `/api/v1/telemetry`, check the container fd budget, set the final
  `tcp_max_connections` default, and record corpus, workload and device under
  `docs/code-review/`. **It is running.** The 0.3.4 build carrying F1 booted
  2026-09-11T22:02:48Z and the seven-day soak reads it hourly to
  ~2026-09-18T22:00Z (§Now, Deployed). Read the counters at the end — and read
  them **before any restart**, since one zeroes them and costs the week.
  **Interim reading, pull `20260915T060002Z`, 82 pulls in:** `peak` **33**,
  `closed_oversize` **0**, `active` 0 in every sample, uptime continuous with t0
  so nothing restarted. The peak was reached in the second pull, sixteen minutes
  after t0, and has not moved in the 3.3 days since — most plausibly clients
  falling back to TCP while the resolver came up, which is the case the ceiling
  has to survive rather than one to discount. Expect a flat figure on the 18th,
  not a climbing one; 1024 is 31× it. This soak's build has no DoT listener, so
  the number is TCP alone and needs no separating.
- **F10 history-write residual.** `fah-stats` `history/mod.rs` `append_line`
  writes a rollup line and its `\n` as two `write_all`s; a stop landing between
  them leaves a partial line that the reader skips — one completed hour lost.
  Pre-existing, a microsecond window once per 300 s; the fix is one combined
  write. Its own go.

**F11 closed 2026-09-12** — report-only supervisor: the run loop checks every
long-lived task on the 10 s telemetry tick, logs a death once and counts it
in `counters.tasks_died`; no restart, no exit, `/health` unchanged (see
[project-risk-inventory.md](code-review/project-risk-inventory.md) §Closed).

**F3 closed 2026-09-12** — `Pipeline::handle` borrows `request.queries.first()`
instead of cloning it: one allocation per query fewer for names past hickory
`Name`'s 32 inline label bytes, measured A/B against `0fb8dd0` with the new
`warm_pipeline_handles_allocate_a_steady_amount` (inventory §Closed).
Committed as `07d4d68`. Its follow-up attributed every remaining per-query
allocation and pre-sized `domain_of`
([f3-name-alloc-attribution.md](code-review/phase2.6/f3-name-alloc-attribution.md)):
13 / 19 / 10 / 16 allocations per handle across blocked and cache-hit paths,
inline and heap names. F4–F9 and F12 are record-only. N5 (`Semaphore::new` panics above `MAX_PERMITS`; neither
`max_connections` key has an upper bound) is recorded and excluded by owner
decision.

## HTTP allocation domains — merged

What is deployed: each HTTP connection served end to end on one of N
`current_thread` runtimes on their own OS threads behind one acceptor, N=2, so
a connection's allocations are freed by the thread that made them. Decision
and cost: [ADR-0006](decisions/0006-http-allocation-domains.md); term:
CONTEXT.md "Allocation Domain"; config: CONFIGURATION.md `[runtime]` (boot
class); code review with findings 1–23:
[alloc-domains-http-review.md](code-review/phase2.6/alloc-domains-http-review.md).

Why N=2 — the RB5009 N sweep of 2026-09-07
([alloc-domains-n-sweep.md](code-review/phase2.6/alloc-domains-n-sweep.md)):

| | N=0 (old way) | N=2 | N=3 | N=4 |
| --- | --- | --- | --- | --- |
| new connections/s, keep-alive rps | 2341, 3297 | 1860, 2574 | 1963, 3237 | 1947, 3412 |
| HTTP p95 ms close / keep-alive | 15.9 / 46.9 | 27.8 / 45.3 | 49.1 / 53.5 | 50.7 / 53.3 |
| cores busy (close) | 3.63 | 2.21 | 2.58 | 2.65 |
| DNS p50 / p99 ms under HTTP load | 3.35 / 20.1 | 0.96 / 15.8 | 1.11 / 13.6 | 1.01 / 12.7 |
| held after 900 MiB WAN, +15 min | +56..+60 | +19 | +32 | +45 |

N=2 carries the tested connection-rate workload with a third less CPU per
request than N=0, better DNS latency under load, and a third of the old way's
held memory; N=3/4 buy keep-alive rate at the cost of p95 and memory. The LAN
transfer pass is **not** a 1 GbE test (router forwarding path caps it at
~67–70 MiB/s). Untested: TLS termination and HTML rewriting — the N decision
is re-measured when Phase 3/4 exist.

Rollback without a rebuild: `FAH__RUNTIME__HTTP_RUNTIMES=0` on `fah-env` +
restart (the 0.3.1 code path, same image). Rollback of the build:
`kingston/fastadhunter-arm64-0.3.1.tar`, or `main` at the tag.

Deferred, each its own go: 11b graceful shutdown of keep-alive connections
(finish the in-flight exchange instead of the whole transfer); dashboard
settings metadata for `runtime.http_runtimes` (review finding 3); the capacity
microbench (rustls AES-GCM and lol_html ms/MiB on the RB5009) that decides
whether N=2 clears 1 Gbit with TLS; IPv6 privacy-address rotation versus
address-exact client identity — reviewed 2026-09-07
([ipv6-privacy-rotation-review.md](code-review/phase2.6/ipv6-privacy-rotation-review.md)),
nothing built, direction is the owner's call.

## Deferrable (reconciled §6)

Type mirrors (`fah_config`/`fah_model`), `CacheStats` identity-DTO; compile
transient (peak 141.3 MiB on 0.3.1, monitored via `peak_rss`); policy
fail-open window and name-assignments-on-LRU; SWR no-EDNS truncation tax;
fah-common scope creep; `blocking_mode` inert; histogram 100 ms ceiling;
p2.5-10 n1 (test-helper readability in `fah-api`, test-only); 11b graceful
HTTP shutdown.

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (10013) when WinNAT reserves the ephemeral port block.
Environmental — do not attribute to a change.

p2.5-09 V3b (live healthcheck column) is a RouterOS 7.21.5 platform
limitation, not a build defect — documented as a trap in
[routeros-traps.md](routeros-traps.md) §Container configuration.
