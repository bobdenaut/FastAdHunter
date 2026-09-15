# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-09-15 (ninth pass — the hot-path audit's two medium
findings shipped as `26ffe1c`, and the no-comments hook now gates the dashboard
as `3e97d16`; tree clean, both remotes current)

## Now

| | |
| --- | --- |
| Branch | **Phase 3 landed on `main`** on 2026-09-13, by fast-forward — `bc49e4e..78238b4`, 105 commits, no merge commit ([plan/plan-merge.md](../plan/plan-merge.md), all five steps closed). `main` is at **`3e97d16`** — the no-comments hook extended to the dashboard — and **both remotes carry it**: `git ls-remote` puts `origin/main` and `backup/main` on `3e97d16` too, so nothing is unpushed. Read the remotes that way, not from the tracking refs, and never from this row. Read the tip from `git rev-parse main`, not from this row either: it named `99e4953` for two commits after that stopped being true, and `9d9d792` for six more. Eight commits followed `9d9d792`: `e8e7cf8` the enumeration sweep's record, `2b03a30` `bd6b1f0` `0662df4` `8abb9c1` the hot-path audit and its rewrites, `26ffe1c` the A1/A2 listener fix and the only production one of the set, `ad12103` this file, and `3e97d16` the hook — §Hot-path audit below. Everything before them is in `git log`; the ten from 2026-09-14/15 are listed in §Session 2026-09-14/15. `phase3-06` (`78238b4`) has served its purpose and sits well behind. Rollback tags: `main-pre-phase3-merge` = `ebc46f1`, `phase3-06-pre-main-merge` = `185139b`; `eb693e2` is the merge commit inside the branch, two parents. `pre-alloc-domain-2026-09-06` = `64be513` stays the rollback point before the allocation domains |
| Tree | **clean — nothing modified, nothing untracked, nothing unpushed.** One stash remains, `stash@{0}` ("phase3-06 project-state Next row"); it predates all of this work and is not ours |
| Tests | **The gate ran at `26ffe1c`, the last commit to touch compilable code — `ad12103` and `3e97d16` are this file, a shell hook and its own test, so no gate was owed.** 2026-09-15, Windows dev box: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --all-features --workspace` **1 648 passed / 0 failed** (1 635 at `9d9d792`). The A1/A2 fix added thirteen: `fah-common --lib` is at 58 (44 before — `retry` carried its four moved tests plus two for `never_fatal`, `throttle` brought nine), `fah-dns --lib` 220 (218, two classification tests) and `fah-http --lib` 92 (91, the falsifiable accept-loop test). The dashboard figures below were measured at `9d9d792` and were not re-run for a change that touches no frontend code. From the earlier passes and still current: the listener fix added twelve tests, R1 one and F14 two, so `fah-config --lib` is at 98 (97 after the listener fix, 90 before it) and `fah-api --test api` 138 (133). Both of the last two fixes are mutation-verified: for R1, deleting the two new arms fails its test on `UnknownEnvKey` and pointing one arm at the wrong field fails it on the value; for F14, reverting the atomic fails the behavioural test while deleting the `post_config` call fails the wiring test and leaves the behavioural one passing — which is the proof the two cover different halves. `shipped_path_e2e` now carries six tests in 21 s — four added on 2026-09-15 for the HTTPS listener's own edge cases, the last of them mutation-verified (`max_connections: 2` fails its negative control). The dashboard ran on the same tip and is green: `npm run typecheck` clean, **58 test files, 1 043 tests, 0 failures** (three fewer than the 1 046 of 2026-09-14 — the `fallback` mode's tests went with the mode in `5b5d3e8`), and `npm run build` is under budget at 136 921 B gzip against 153 600 B. `cargo bench -p fastadhunter --bench pipeline --no-run` builds for the first time since p5-04, with no profile override (`3a1b5b8`). The `e2e` `WSAEACCES` trap (§Known-good gate note) did not fire. Bench A/B against `main` over four alternating rounds: **no regression demonstrated** |
| Version | 0.3.4 (workspace, since `4e7a6de`), untagged. Newest tags `v0.3.2` (`89aac76`), `pre-alloc-domain-2026-09-06`, `soak-p2.6-11` |
| Deployed | **0.3.4, and not from today's merge.** Nothing was deployed on 2026-09-13 — landing Phase 3 on `main` is not a deployment, and deploying it is a separate decision that has not been made. Production runs image `kingston/fastadhunter-arm64-0.3.4.tar` as container `fastadhunter-0.3.4`, booted **2026-09-11T22:02:48Z**, HTTP allocation domains, N=2 (`FAH__RUNTIME__HTTP_RUNTIMES=2` on `fah-env`), `veth1` / `172.17.0.2`, mounts `fah-config,fah-data`. `GET /health` on 2026-09-13 18:24 local answered `0.3.4`, uptime 148 853 s. **The build commit is recorded nowhere** — neither `/health` nor the soak capture carries one; by timestamp the image matches `d307c36` with the version already at 0.3.4, the bump itself committed three minutes after the boot as `4e7a6de`. A **seven-day soak is running on this build**: t0 `2026-09-11T22:13:21Z`, ending ~2026-09-18T22:00Z, hourly scheduled task `FAH-soak-0.3.4`, artefacts under `docs/code-review/phase2.6/soak-0.3.4/` — untracked and gitignored, so they live outside the repository. The p3-06 probe (`fah-probe` on `veth3` / 172.17.0.4) was torn down 2026-09-11 evening; `veth3` remains, no test firewall rule or address list remains. Deploying Phase 3 is a separate decision and it has not been made |
| Build ≠ tip | the running 0.3.4 **has** the F10 stats flush (`32d7776`), the F1 TCP bound (`ed28395`), the F2 UDP ceiling (`b0b091e`), their close-out tests (`ad110d8`) and adaptive-only (`d307c36`) — every one of them committed before its 01:02 local boot. It **lacks** everything from the day after: the F11 supervisor (`0fb8dd0`), F3's query borrow and pre-sized `domain_of` (`07d4d68`, `c220956`), the H1-H3/D1 allocation removals (`3287418`), the idle upstream pool reaper (`8941770`), the HTTP refusal-counter split (`28c751d`) — and the whole of Phase 3: certificates, SNI filtering, HTTPS interception, the DoT and DoH listeners. The `strategy = "adaptive"` precondition is settled rather than pending: that boot already happened and the config loaded |
| Phase | **4 is `PARKED`, 2026-09-15** — all five tasks, owner decision with its evidence in [ADR-0009](decisions/0009-phase-4-parked.md). HTML rewriting needs the page body, the body needs interception, and interception is off; plain HTTP, the one path left, carried 2 900 requests and zero blocks in 3.3 days, and the deployed ruleset is 720 URL rules against 1 182 029 DNS ones. The folder stays in `plan/open/` — when Phase 3 closes, the selector finds no `WAITING` task and moves it to `closed` by itself. **Phase 3 is therefore the last one that ships.** **0–2.6 and 5 closed** (2.6 closed 2026-09-07, all 13 tasks `DONE`; `p2.6-12` reached `main` on 2026-09-11 as the cherry-pick of `fa9451a` — `adaptive` is the only strategy, the `fallback` walk is deleted, a config naming it fails at load). **3 is now in `plan/wip/phase3` on `main`** — the `open` → `wip` move landed with the merge, by the owner's decision (plan-merge.md §Step 3). `p3-01`…`p3-05` `DONE`, `p3-06` and `p3-06b` `PARKED` (2026-09-13, the interception decision), `p3-07`…`p3-09` `DONE` (ADR-0008: Interception Document, 525 classification, rejection view + editor). **`p3-10b` and `p3-10c` are `DONE`, both 2026-09-14** — the DoT connection gauge (`counters.dns_dot_connections`, counted at accept so a stalled handshake is in the figure that sizes the cap) and the HTTP, HTTPS and API acceptors reporting an unplanned end through `record_task_death`. Row 11's soak waits for neither. `p3-10` and `p3-11` stay `WAITING` — the only two the selector will pick. **N3 follow-up closed 2026-09-11 as a technical experiment, not promoted** — the client alert names the TLS stack, not the cause; HTTP/3 must be refused for intercepted clients ([p3-06-n3-alert-ab.md](code-review/phase3/p3-06-n3-alert-ab.md)) |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): §5.1–6 cleared; §5.7–14 gate Phase 3. S1-G2 tiers 1–3 met; S1-G4 and S1-G5 route 2 not validated and will not be |
| **Next** | **What can start today, needing no decision: the dev-box half of p3-11.** Part of it has landed — `crates/fastadhunter/tests/shipped_path_e2e.rs` (`095163b`) walks DNS over UDP and TCP, plain HTTP, an SNI block, an SNI splice through an allocation domain, DoT and DoH in one run with `clients` empty, every layer carrying an allowed control beside the blocked case; the security suite landed 2026-09-14 (`00698ee`); on 2026-09-15 four more binary-level arms closed two of the coverage table's three `no` rows (`d93f4ad`, `5f91db5`), leaving HTTPS lane saturation, which was deliberately left open. The deploy guide's HTTPS section is done too: §5c was audited against the live router on 2026-09-15 and three defects fixed (`dac5be0`) — rollback left the two QUIC rejects behind in `filter`, the intro counted two rules where there are up to seven, and the order was implicit, though the QUIC reject has to land before the dst-nat or a browser on HTTP/3 walks past the steer. **What is left on the dev box is Private DNS on the device, which waits for the deploy anyway. The SNI/DoT/DoH budget rows are not dev-box work** — every figure in PERFORMANCE.md's table is a column headed `Measured on the RB5009`, and its SNI and DoT/DoH rows say `TBD — set from the RB5009` because loopback and TLS legs do not convert (§Conversion, line 141). A dev-box bench is a diagnostic on the way to those rows, never the row itself. **Blocked on the deploy decision:** p3-10's B2 sweep on the RB5009 (it also owes p3-11 the dst-nat 443 rule) and p3-11's device half with its seven-day soak. **Their own go, and both waiting on more than a decision to write them:** the DoT/DoH load generator and a verified runner for splice RSS. Neither can produce an interpretable figure before p3-11 fixes the budgets on the device — PERFORMANCE.md's `DoT / DoH added latency vs UDP, p50` is still `TBD` and the TLS/HTTP legs do not convert from x86 ([p3-10-track-b1-x86.md](code-review/phase3/p3-10-track-b1-x86.md) §Remaining TODOs, `e028328`). In flight and needing nobody: the **0.3.4 soak ends ~2026-09-18**, and its counters settle the F1 `tcp_max_connections` default (§Risk inventory close-out). Interception ships **disabled** through an empty client scope, not through `engine.mode` — and since 2026-09-13 that is the owner's decision rather than a default awaiting a switch: `p3-06` and `p3-06b` are `PARKED`, their non-interception arms moved to p3-11, and the phase's definition of done no longer names interception. Off the critical path: dashboard settings metadata for `runtime.http_runtimes` — a deliberate omission, since `metadata.ts` is an acknowledged subset and the raw All-settings panel renders every unmodelled key — and alloc 11b (`Connection::graceful_shutdown` into `Proxy::serve_connection`; 11a already keeps DNS answering through the drain). **Before Phase 3 goes live:** the router refuses UDP 443 LAN→WAN, or HTTP/3 bypasses the steer. The rule's **placement was corrected 2026-09-14** — appended behind a final drop it does nothing — and the DNS escape routes are named beside it ([deploy-rb5009.md](deploy-rb5009.md), `73d79aa`) |

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

### Hot-path audit and the A1/A2 fix — 2026-09-15

[post-merge-performance-audit-2026-09-15.md](code-review/phase3/post-merge-performance-audit-2026-09-15.md).
**Not the same file as the integration audit below**, whose name differs by one
word (`post-merge-audit-…`); the two are easy to confuse and cover different
things. This one is hot-path performance, memory and Rust quality, read-only,
SNAPSHOT mode — there was no code diff to review.

Its first pass was wrong and was rewritten over four commits. It had reported
"0 locks" and "0 panics" for files its own scope listed, because cutting each
file at the first `#[cfg(test)]` drops 187 lines of `cache.rs` production code —
that attribute sits on two individual methods long before the test module.
Anchor such a cut at column 0. It had also read passing allocation oracles as
headroom when the ceilings equal the measurements: 8 of 12 ceiling checks clear
by exactly the 4-allocation jitter allowance, so they are tight regression
detectors and nothing more.

Findings are labelled `A1`–`A7`, local to that file. Bare `F` numbers were not
available: the review registry already uses them for whole files
(`phase2.6/f2-udp-inflight.md`, `f3-name-alloc-attribution.md`,
`phase3/f7-flapping-oracle-redesign.md`).

- **A1, A2 — fixed 2026-09-15 in `26ffe1c`**, the only production commit of the
  set. The HTTP/HTTPS accept loop had no backoff, so descriptor exhaustion spun a
  core; and four `warn!` sites a client could drive had nothing limiting their
  rate. `RetryPolicy` moved from `fah-dns` to `fah-common` (hard rule 1 forbids
  `fah-http` importing `fah-dns`), and `LogThrottle` is new there.
- **The acceptor recovers rather than dying** — owner's decision. `Fatal` after
  40 consecutive errors is ~33 s and descriptor exhaustion outlasts that, so
  `accept_loop` uses `RetryPolicy::never_fatal()`. The three DNS listeners keep
  the old escalation. The signal is a throttled `warn` with a cumulative count,
  not a task death through `record_task_death`.
- **A throttle must not be per connection.** `DotTls` is `Clone` and is cloned
  once per connection; a throttle field there would have been the defect itself.
  Instances live on the connection gauges and on `Pipeline`.
- **A6 — fixed 2026-09-15 in `3e97d16`, and two claims under it withdrawn.** The
  hook cited "hard rule 20" for a rule CLAUDE.md numbers 7, left over from
  `plan/CLAUDE.md`'s old copy of the principles; it references the rule by name
  now. Withdrawn: that 8369 comment lines mean rule 7 "does not describe the
  tree" — a prohibition is not a description, and the existing comments predate
  it — and that the hook is too strict for rejecting an edit that carries a
  pre-existing comment through unchanged. **Hard rule 7 is not open for
  discussion.** A comment is an input cost paid on every read of the file, by
  every agent, in every session, against a one-time benefit. A rule that needs
  the model's judgement to apply ("2–3 lines where needed") was tried and
  eroded; a binary, machine-checkable one holds.
- **A3, A4, A5 and A7 open, all low.** A3 is the TCP/DoT length-prefix realloc,
  held deliberately: encoding at a two-byte offset is invalid, because hickory
  emits name-compression pointers as absolute buffer indices. A4 and A7 are
  method findings. A5 is the only one waiting on an owner decision — the text of
  hard rule 3, which forbids hot-path locks the cache legitimately takes.

### The no-comments hook now covers the dashboard — `3e97d16`

`.claude/hooks/no-rust-comments.sh` gates `.rs`, `.ts` and `.tsx`. The dashboard
had never had a gate and sits at **15.7 % comment lines** (6342 of 40469)
against **9.7 %** in `crates` (8405 of 86829) — and the `crates` figure is
mostly pre-hook code, since the hook blocks new edits rather than cleaning old
ones. Single-line template literals are stripped before the scan: `socket.ts`
builds a websocket URL with a literal `//` inside backticks, which the old
string-stripping read as a comment. No new exemption was needed — the dashboard
has zero functional pragmas, so all 6342 lines are prose.

`.claude/hooks/no-rust-comments.test.sh` covers it: 14 cases, blocked / allowed /
out of scope, and falsified rather than trusted — dropping `.ts` from the
extension filter fails three, dropping the backtick stripping fails exactly the
template-literal case. The `MultiEdit` path had no coverage before.

**The existing comments are left alone, deliberately.** Each is either a
duplicate of a fact that already has a home — `api/types.ts` cites API.md for
the two traps it repeats, and both are there at `API.md:289` and `:784` — or the
only copy, and deleting it loses the fact. Per file, not a `sed`.

### Post-merge audit — 2026-09-15

[post-merge-audit-2026-09-15.md](code-review/phase3/post-merge-audit-2026-09-15.md)
covers what the 2026-09-08 audit could not: it was written against the first
in-branch merge, and the second one (`eb693e2`, the landing) was reviewed only by
plan-merge's §Step 2 and §Step 4 checklists. Scope was agreed before the pass and
kept small — the `ConnectionGauge` move to `fah-common`, the `eb693e2` hunks no
earlier review names, instrumentation validity, and merge-window tests that could
pass with the wiring they prove broken. Two later passes the same day widened it
— a defect hunt and a listener-configuration sweep, findings SP1–SP8, below.
**PASS WITH DEFERRED FINDINGS.**

Two facts worth carrying out of it, neither obvious from the code:

- The true pre-integration base is **`bc49e4e`**, not the tag. `main-pre-phase3-merge`
  (`ebc46f1`) is its parent, one plan-doc commit behind, and is the frozen bench
  checkout — a rollback point, never a diff base.
- `git show --cc --stat` on a merge prints files taken whole from one side too.
  The real hand-decision surface of `eb693e2` is **36 files**, not 67; it is the
  intersection of `git diff --name-only <parent> eb693e2` over both parents.

Findings:

- **N1 — closed 2026-09-15, the only defect the first pass found.** `CONFIGURATION.md` documented
  `FAH__DNS__TCP_MAX_CONNECTIONS` and `FAH__DNS__UDP_MAX_INFLIGHT`, and
  `env::apply_one` had no arm for either, so its `_ =>` arm returned
  `UnknownEnvKey` and **the process refused to start** with a documented variable
  set. Independently re-verified and reproduced on the built binary before any
  fix. The two arms landed with three `fah-config` tests, one child-process test
  through `--healthcheck` (no `std::env::set_var`), and an anti-drift test that
  walks every `Env: FAH__…` name in CONFIGURATION.md through
  `apply_env_overrides` — the doc and the allowlist can no longer diverge with
  the suite green. Gates green: `fmt` and `clippy -D warnings` clean,
  `cargo test --all-features --workspace` 0 failed, `fah-config` 90 passed
  (87 before), `healthcheck` 6 (5 before). **This is the same `udp_max_inflight`
  F8 leaves inert at 0** — arming it from the container's `fah-env` was the one
  route that looked available and did not exist.
- **N2 — open, low.** `counters.dns_tcp_connections`, `dns_dot_connections` and
  `dns_udp_inflight` reach `/api/v1/telemetry` correctly and have no dashboard
  consumer; `tasks_died` does. These are the figures CONFIGURATION.md tells the
  operator to retune the ceilings from. Owner's call: a Health row, or a line
  saying they are telemetry-API-only.
- **N3 — info, nothing owed.** `strategy_ab.rs` loops over one strategy since
  `fallback` was removed; the disposition is already recorded and the harness is
  not claimed to discriminate.

Not reopened, by instruction: F1–F9, p3-10 Track A, p3-11, and the dashboard as a
review surface. Untouched and still true: an unknown `FAH__` variable on a fresh
`/config` volume still leaves a defaults-only TOML before the load fails, because
`Config::load` writes before it applies the environment. Outside N1's scope.

#### Second pass and listener sweep — 2026-09-15, findings SP1–SP8

The same file carries two later passes: a defect hunt for what a green suite can
miss (SP1–SP3), then a sweep on one question — which invalid or mutually
incompatible **listener** configurations the product accepts as valid, and what
happens afterwards. Every conclusion came from the source and from reproduction
on the built binary; no existing review document was taken as evidence.

**Four confirmed defects, all fixed the same day, all on the configuration and
startup surface and all fail-closed:**

- **SP1** — of the six listener port pairs, the three not involving `https`
  (`dns–api`, `dns–http`, `api–http`) were compared by nothing. A colliding pair
  validated clean and could sit latent: under the shipped `mode = "dns"`,
  `api.port = 8080` beside the HTTP default was accepted and persisted, and the
  patch that later enabled `dns+http` also passed, answered `restart_required`,
  and the restart did not come back.
- **SP4** — `[api] address` accepted any IPv6 literal, including the `::`
  CONFIGURATION.md offers, then failed to bind it: `ApiServer::bind` assembled
  the address with `format!("{address}:{port}")`, the exact trap
  `fah_common::listen::listen_addr` exists to avoid. It also meant the API, the
  dashboard and DoH could not be reached over IPv6 at all.
- **SP5** — the API was the one listener whose bind failure never went through
  `bind_error`, so a port conflict printed a bare OS errno and a privileged
  `[api] port` would have lost the `CAP_NET_BIND_SERVICE` hint.
- **SP6** — `bind_error`'s `AddrInUse` arm dropped the config key its own doc
  comment promised, and told the operator another process held a port this
  process was holding itself.

**The rule SP1 now implements**, agreed before any code: *shape is validated
always, relationships only when both ends are live.* Sockets that actually bind
are compared, not config keys; same port plus overlapping addresses is a
conflict; `::` overlaps both families because `bind_tcp` clears `IPV6_V6ONLY` by
our own decision, `0.0.0.0` overlaps IPv4, and two concrete addresses never
overlap. A listener joins only when it would bind. Address **syntax** stays
unconditional. This is safe because the flip is always revalidated — the API
validates the whole merged candidate and boot revalidates the whole file — so a
parked collision is refused the moment it is enabled, by validation, with the key
named. It closes **SP7** (the `https` loop refusing configurations that could not
collide) without a change of its own.

Two consequences worth carrying, neither cleanup: `https.listen.port` is now
checked **less** often than before, since it was the only listener compared
unconditionally; and the e2e harness had to stop drawing duplicate ports, because
with the matrix complete a duplicate stops being a retryable `EADDRINUSE` at bind
and becomes a hard validation refusal. The harness was fixed at the source
(`free_tcp_port_excluding`), and the `is_port_conflict` needle list was
deliberately **not** widened — that would let a genuine wrong refusal hide behind
a retry.

Still open from these passes, both owner decisions, neither blocking: **SP2**
(the DoT leaf pre-warm awaited outside the handshake deadline while holding an
accept permit — Low, structural, and the measured 450.88 µs mint argues against
it being live) and **SP8** (DoH has no status surface when `[api] tls = false`
silences it, where DoT has `DotListener::Closed { reason }` on
`GET /api/v1/certificates`). **SP3** is info: `wiring.rs` asserts on `main.rs`
source text, and `acceptor_death.rs` is the test that discriminates.

One thing the fix could not carry: deleting `main.rs`'s `http_enabled` /
`https_enabled` removed the six-line comment explaining why the mode match is
exhaustive. The match moved to `schema/engine.rs`; the rationale could not,
because hard rule 7 forbids an agent writing Rust comments. Behaviour is
self-enforcing without it — the match really is exhaustive — but the reasoning
now lives only in the audit file and in git history.

#### Independent review of the two fix commits — 2026-09-15, findings R1–R7

`f32f214` and `2eb5018` were then reviewed by a reviewer who did not write them:
plan compliance against the resolutions above, correctness, architecture,
performance, memory, Rust quality, tests, regression. **One should-fix, six
notes, no blocker, and no defect in the DNS or HTTP data path.**

- **R1 — closed 2026-09-15.** `fah-http`'s two `PORT_SETTING` constants
  advertised `FAH__HTTP__LISTEN__PORT` and `FAH__HTTPS__LISTEN__PORT`;
  `env::apply_one` had no arm for either, so following the hint stopped the
  process from booting. N1's class in a **second population**: N1's anti-drift
  test walks the names *CONFIGURATION.md* advertises, and nothing walked the
  names the *code* advertises. The two arms landed, the five constants moved to
  `fah-config/src/port_setting.rs` — `fah-common` owns `bind_error` but is an L1
  sibling and may not import the allowlist — and one test parses the variable out
  of each constant, so a renamed constant carries its own check. CONFIGURATION.md
  gained the two `Env:` lines, which makes the doc-walking test cover five names
  instead of three.
- **R2–R7 — open, notes, nothing owed.** The pair-matrix test drives 5 of the 10
  pairs (`dot–dns` and `dot–http` asserted by nothing, though the validator is one
  uniform loop); `addresses_overlap` misses IPv4-mapped IPv6; the blamed key is
  the later entry in the socket table, not the edited one; the `Env:` scan's
  `>= 3` floor equals the count it guards; one API test binds `[::]:0` on every
  interface and needs host IPv6; `validate_listen_sockets` takes both the config
  and the addresses derived from it.

**Neither R1 nor N1 was created by the Phase 3 merge** — worth recording, because
the audit that found them was triggered by it. `git merge-base --is-ancestor`
against `bc49e4e`, the pre-merge tip of `main`: N1's two keys landed on `main`
itself on 2026-09-11 (`ed28395`, `b0b091e`), and R1's **HTTP half** landed on
2026-07-26 with p2-01 (`4ef6d4a`). Only R1's **HTTPS half** arrived with the merge
(`40ca0cc`, p3-03) — `tls_server.rs` did not exist on `main` before it. On
pre-merge `main` the wrong HTTP variable was near-invisible: `bind_error`'s
`AddrInUse` arm dropped `port_setting` entirely, so it printed only on
`PermissionDenied`, which needs `[http.listen] port` below 1024 against a default
of 8080. **SP6's fix is what made a two-month-old defect visible.** The version
number does not discriminate any of this: `bc49e4e` already reads `0.3.4`.

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

**F14 opened and closed 2026-09-15** — `rules.refresh_hours_default` was
classified runtime, and the only component it governs never saw a change:
`ListManager` copied it into a plain `u32` at construction, so `POST
/api/v1/config` answered `applied, no restart` while the refresh scheduler kept
the boot value. Its **two readers** are what hid it — the `/lists` handlers read
the config store live, so `GET /config` and `GET /lists` both reported the new
number and the only witness was the timing of the next fetch. It reached the
shipped default, since `oisd-basic` carries no per-list `refresh_hours`. Fixed
with an `AtomicU32` and `set_default_refresh_hours`, called from `post_config`
beside the applies it already ran; mutation-verified on two tests that fail
separately. **Predated the Phase 3 merge** (`7920415`, `e056190` — p1.5-07).
The durable lesson is about enumeration tests, not this key: the family *was*
enumerated at `config_store.rs`, but the `consumer` column named a reader rather
than an applier and the assertion only checked the classification, so the
enumeration certified the defect instead of catching it
([project-risk-inventory.md](code-review/project-risk-inventory.md) §Closed).

**The enumeration sweep is finished, 2026-09-15 — four families counted, two
clean, two findings.** F14 was the first, and the method it proved is the thing
to carry: write down both sides of a set and compare the counts. A diff shows
changed lines; it cannot show an absent member, which is what every finding of
this sweep turned out to be.

- `BOOT_KEYS` against the keys with a live consumer → **F14**, the only defect.
- Schema fields against CONFIGURATION.md, in the schema → doc direction the
  `Env:` walk does not cover → **clean**, 25 structs and 55 leaf keys, all 55
  documented.
- `/api/v1/telemetry` fields against dashboard consumers → **F15**, minor. The
  whole `listeners` block is unmodelled by `interface Telemetry`, and an
  unmodelled JSON field is silently ignored, so adding it produced no signal
  anywhere. N2 was not the only instance of that family; this one is 34 leaves
  against N2's three. **The only finding of the sweep the merge created** — at
  `bc49e4e` there was no `listeners` field to model. Owner has not decided
  whether the dashboard consumes it at all.
- Per-listener runtime dispositions → **clean**. A disposition earns its keep
  only where the real state can differ from the config, which is true of DoT
  alone — and DoT is the one that has `DotListener::Closed { reason }`. This
  refines SP8 rather than overturning it: DoH's state is two config reads away,
  not hidden. **F16** fell out beside the sweep: when an acceptor dies the
  identity of the dead task exists only in one log line, while `/config`,
  `/health` and `/telemetry` all keep reporting the lane as healthy. Not a
  defect — F11's design working as decided; F11 settled whether to restart, not
  whether the identity should be queryable.

All four are recorded with their numbers in
[project-risk-inventory.md](code-review/project-risk-inventory.md), the clean
ones in §Checked and clean so a later pass skips them.

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
microbench that decides whether N=2 clears 1 Gbit with TLS — **its AES-GCM half
ran 2026-09-15** by probe, no deploy, ~1.0 ms/MiB per core and so ~12 % of one
core at line rate, which removes encryption as the explanation but answers
nothing about the NIC, the forwarding path or the scheduler
([p3-10-track-b2-rb5009.md](code-review/phase3/p3-10-track-b2-rb5009.md)
§Measurements — bulk AEAD cost); **the lol_html half is cancelled, not deferred — Phase 4 is parked 2026-09-15 ([ADR-0009](decisions/0009-phase-4-parked.md))**; IPv6 privacy-address rotation versus
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
