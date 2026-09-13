# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-09-13

## Now

| | |
| --- | --- |
| Branch | **Phase 3 landed on `main`** on 2026-09-13, by fast-forward — `bc49e4e..78238b4`, 105 commits, no merge commit ([plan/plan-merge.md](../plan/plan-merge.md), all five steps closed). `main` is now `a9a964e` and is **one commit ahead of both remotes; that commit is not pushed** — `origin/main` and `backup/main` are both at `78238b4`. `phase3-06` (`78238b4`) has served its purpose and sits one behind. Rollback tags: `main-pre-phase3-merge` = `ebc46f1`, `phase3-06-pre-main-merge` = `185139b`; `eb693e2` is the merge commit inside the branch, two parents. `pre-alloc-domain-2026-09-06` = `64be513` stays the rollback point before the allocation domains |
| Tree | no longer mid-merge. Clean apart from `.gitignore`, modified and uncommitted by the owner's decision. Its stash (`stash@{0}`, "owner gitignore") is **kept on purpose** — the pop collided with the p3-06 smoke rules the merge brought in, was resolved by hand keeping both blocks, and while `.gitignore` is uncommitted that stash is its only backup. `stash@{1}` predates this work and is not ours. The `CLAUDE.md` stash is gone: applied and committed as `a9a964e`. `plan/wip/phase3/p3-10-post-merge-performance.md` is an approved draft and still untracked |
| Tests | Step 4 ran on the merged tree, 2026-09-13, Windows dev box: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --all-features --workspace` 58 suites and 0 failures. The targeted checks pass — F1's three TCP tests, F3's allocation ceilings, F10's shutdown flush, H1-H3/D1's `proxy_alloc`, the idle-pool reaper — and F11's wiring was read in `main.rs:769` and `:775-776`. Phase 3's surface passes: `interception` 45/45, `security_phase3` 7/7, `sni` 11/11, `e2e_https` 3/3, `interception_migration` 6/6. Dashboard: `npm run typecheck` clean, 1 048 tests. Bench A/B against `main` over four alternating rounds: **no regression demonstrated** |
| Version | 0.3.4 (workspace, since `4e7a6de`), untagged. Newest tags `v0.3.2` (`89aac76`), `pre-alloc-domain-2026-09-06`, `soak-p2.6-11` |
| Deployed | **0.3.4, and not from today's merge.** Nothing was deployed on 2026-09-13 — landing Phase 3 on `main` is not a deployment, and deploying it is a separate decision that has not been made. Production runs image `kingston/fastadhunter-arm64-0.3.4.tar` as container `fastadhunter-0.3.4`, booted **2026-09-11T22:02:48Z**, HTTP allocation domains, N=2 (`FAH__RUNTIME__HTTP_RUNTIMES=2` on `fah-env`), `veth1` / `172.17.0.2`, mounts `fah-config,fah-data`. `GET /health` on 2026-09-13 18:24 local answered `0.3.4`, uptime 148 853 s. **The build commit is recorded nowhere** — neither `/health` nor the soak capture carries one; by timestamp the image matches `d307c36` with the version already at 0.3.4, the bump itself committed three minutes after the boot as `4e7a6de`. A **seven-day soak is running on this build**: t0 `2026-09-11T22:13:21Z`, ending ~2026-09-18T22:00Z, hourly scheduled task `FAH-soak-0.3.4`, artefacts under `docs/code-review/phase2.6/soak-0.3.4/` — untracked and gitignored, so they live outside the repository. The p3-06 probe (`fah-probe` on `veth3` / 172.17.0.4) was torn down 2026-09-11 evening; `veth3` remains, no test firewall rule or address list remains. Deploying Phase 3 is a separate decision and it has not been made |
| Build ≠ tip | the running 0.3.4 **has** the F10 stats flush (`32d7776`), the F1 TCP bound (`ed28395`), the F2 UDP ceiling (`b0b091e`), their close-out tests (`ad110d8`) and adaptive-only (`d307c36`) — every one of them committed before its 01:02 local boot. It **lacks** everything from the day after: the F11 supervisor (`0fb8dd0`), F3's query borrow and pre-sized `domain_of` (`07d4d68`, `c220956`), the H1-H3/D1 allocation removals (`3287418`), the idle upstream pool reaper (`8941770`), the HTTP refusal-counter split (`28c751d`) — and the whole of Phase 3: certificates, SNI filtering, HTTPS interception, the DoT and DoH listeners. The `strategy = "adaptive"` precondition is settled rather than pending: that boot already happened and the config loaded |
| Phase | **0–2.6 and 5 closed** (2.6 closed 2026-09-07, all 13 tasks `DONE`; `p2.6-12` reached `main` on 2026-09-11 as the cherry-pick of `fa9451a` — `adaptive` is the only strategy, the `fallback` walk is deleted, a config naming it fails at load). **3 is now in `plan/wip/phase3` on `main`** — the `open` → `wip` move landed with the merge, by the owner's decision (plan-merge.md §Step 3). `p3-01`…`p3-05` `DONE`, `p3-06` `AWAITING SOAK`, `p3-06b` `WAITING`, `p3-07`…`p3-09` `DONE` (ADR-0008: Interception Document, 525 classification, rejection view + editor). **N3 follow-up closed 2026-09-11 as a technical experiment, not promoted** — the client alert names the TLS stack, not the cause; HTTP/3 must be refused for intercepted clients ([p3-06-n3-alert-ab.md](code-review/phase3/p3-06-n3-alert-ab.md)) |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): §5.1–6 cleared; §5.7–14 gate Phase 3. S1-G2 tiers 1–3 met; S1-G4 and S1-G5 route 2 not validated and will not be |
| **Next** | the merge is finished; `plan-merge.md` has no open row. Three things follow, none of them started: **p3-10** (`plan/wip/phase3/p3-10-post-merge-performance.md`, approved draft, not yet committed), the **p3-06b re-scope**, and the **deploy decision**, which is separate and unmade. One thing is already in flight and needs nobody: the **0.3.4 soak ends ~2026-09-18**, and its counters settle the F1 `tcp_max_connections` default (§Risk inventory close-out). Interception ships **disabled** through an empty client scope, not through `engine.mode`. Off the critical path: the dashboard's dead `fallback` mode branch (`derive.ts`, `degraded-banner.tsx`), dashboard settings metadata for `runtime.http_runtimes`, the TLS / lol_html capacity microbench. **Before Phase 3 goes live:** the router refuses UDP 443 LAN→WAN (deploy-rb5009.md §5c), HTTP/3 bypasses the steer otherwise |

## From the merge — 2026-09-13

Three findings came out of the measuring rather than the code, and nothing else
on `main` records them —
[main-phase3-integration-audit.md](code-review/phase3/main-phase3-integration-audit.md):

- **F7** — `adaptive_behaviour.rs`'s `b5_recovery_and_flapping` p99 guard cannot
  decide anything: one arm can never reach its sample threshold, one breaks when
  its *reference* phase gets faster, and a panic in one arm skips the next.
- **F8** — `tcp_max_connections` ships armed at 1024 while `udp_max_inflight`
  ships inert at 0. Inherited from `main`; no default was changed.
- **F9** — seven benches whose own-side spread on identical code is wider than
  the 10 % gate they are supposed to police.

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
  ~2026-09-18T22:00Z (§Now, Deployed). Read the counters at the end.
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
