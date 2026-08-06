# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-08-06

## Now

| | |
| --- | --- |
| Branch | `feat/phase2-http-pipeline` at `10d9470`, pushed to `origin` **and** `backup` |
| Tree | **dirty** — `crates/fah-stats/src/{stats.rs,query_log/ring.rs}`, `crates/fastadhunter/src/main.rs`, `Cargo.lock`, `requests/{history,stats}.http`, docs |
| Tests | workspace green as of 2026-08-06 (89 in `fah-stats`); `fmt`/`clippy` fail **only** in `tui-monitor/` |
| Phase | 2 (`plan/wip/phase2`) |
| Next task | **none selectable** — p2-08 needs a load run, p2-09 is blocked by decision (§Blocker) |

`tui-monitor/` and `fah-top.py` are on this branch and are **off-plan** — a
local monitoring utility, not a phase task. They are the uncommitted `main.rs`
change's context.

## Phase 2 status

| Task | State |
| --- | --- |
| p2-00 … p2-07, p2-10, p2-11 | DONE |
| **p2-08** | **`AWAITING SOAK`** — needs a generated-load run for the on-device HTTP numbers |
| p2-09 | **BLOCKED by decision 2026-08-06** — `query_log` stays off in production (§Blocker) |

**p2-07 closed 2026-08-06** on the soak below —
[`docs/code-review/p2-07-review.md`](code-review/p2-07-review.md) §12.
**p2-11 closed the same day** with no code — see below. The phase does not close
while p2-08 exists.

## The soak — finished, and it closed p2-07

**Do not start a new one to satisfy p2-08.** What p2-08 still needs is a short
**generated-load** run, which a soak cannot produce (≈8 intercepted connections
per 6 minutes, no control over body size or origin RTT).

Two windows, one request from the router's own `/history/perf`:

| Window | Span | Sampling | Residual slope (whole / final third) |
| --- | --- | --- | --- |
| T0 process | 2026-08-02T10:18:25Z → 2026-08-04T23:35Z (61.3 h) | 60 s | +0.082 / +0.174 MiB/h |
| Current | 2026-08-04T23:59:09Z → present (36.2 h at read) | 360 s | +0.027 / −0.057 MiB/h |

No leak: signs disagree, band is ~37 MiB wide, means agree to 0.4 %. Residual is
**57–58 % of RSS** — the mimalloc-era baseline. Full numbers in the review §12;
baseline and pass criteria in
[`docs/code-review/0.2.10-soak-baseline.md`](code-review/0.2.10-soak-baseline.md).

**The T0 window was cut short by an ISP IPv6 outage** — the repo owner rebooted
and reconfigured the router while diagnosing it, giving five container starts
between 23:36 and 23:59 on 2026-08-04. Cause is external to FastAdHunter
(`auto-restart-interval=none`). Both windows stand on their own.

## Deployed

**0.2.10** on the RB5009, `mode=dns+http`. Current process started
**2026-08-06T13:14:59Z**. Steady RSS **46.6 MiB**, compile peak **181.4 MiB**,
16 lists `ok`, 798,250 compiled rules, `events_dropped_total` 0.

**Three config values are live and differ from the 2026-08-02 baseline:**

- `MIMALLOC_PURGE_DELAY` **100 → 0** — `p2-11`, see above. Unproven under load.
- `history.sample_interval_seconds` **60 → 360** — the perf series is 6× coarser.
- `query_log` persistence **off** (`enabled:false`, `retention_days:0`) —
  RouterOS was reporting ~700 MB of disk in use.

Rollback is available: the last four version tarballs are on `kingston/`, so
reverting is `/container/add` from the previous tar rather than a rebuild.
Disable the `fah-liveness` scheduler around the swap.

## Blocker for p2-09

`p2-09` is a reader over the **persisted** query-log segments. Persistence is
currently **off** on the device (above), so its on-device acceptance check has
nothing to read — and the fallback once recorded here, "scope the check to the
in-memory ring", **does not exist**: the same flag empties the ring (Open
items). There is no on-device data source at all.

**Decision 2026-08-06: leave it blocked.** `query_log` stays off in production,
so the task is picked up only if persistence itself needs validating — and then
it needs the flag re-enabled with a bounded `retention_max_mb` first.

## `p2-11` compile peak RSS — DONE 2026-08-06, zero code

Report: [`docs/code-review/p2-11-compile-transient.md`](code-review/p2-11-compile-transient.md).

**The peak is a ratchet across compiles, not one compile's cost** — 131.5 →
209.9 → 228.7 MiB over boot plus two refreshes, saturating ~230 MiB, driven by
mimalloc's deferred purge. Boot alone is 122–131 MiB.

`MIMALLOC_PURGE_DELAY` 100 → **0** on the device:

| | before | after |
| --- | ---: | ---: |
| Steady RSS | 58.13 MiB | **46.6 MiB** |
| Peak after repeated compiles | 230.7 MiB | **181.4 MiB** |
| Post-swap plateau of dead memory | 7.4–9.9 s | **none** |

**Three claims retracted:** the peak does not come from boot; there is no 0.2.8
regression (158 MiB was the same ratchet read after fewer compiles); the plateau
was the deferred purge, not tokio's blocking-thread keep-alive. PERFORMANCE.md's
streaming-parse lever is retracted with them — raw list text was never the
dominant term.

**Two things left open**, both in the Open items list below: `delay=0` is
unmeasured above ~0.5 qps, and ~59 MiB of ratchet survives it.

## Open items

- **HTTP interception is IPv4-only.** A host reached over IPv6 bypasses the proxy
  entirely. Mirror `/ipv6/firewall/nat` rules are written and were deferred until
  after the soak — **that deferral has now expired**;
  `docs/code-review/0.2.10-soak-baseline.md` §Known gap. Two open points there:
  `to-ports` support on IPv6 dstnat is unverified, and IPv6-only origins are
  unreachable from a ULA-only container.
- `HTTP_ORIGIN_PORT` is hardcoded to 80, and the egress guard allows no other
  port. Correct in production; it means `http_e2e` needs `127.0.0.1:80` and skips
  when it cannot bind it.
- **`MIMALLOC_PURGE_DELAY=0` is unproven above ~0.5 qps.** 0.2.7 moved it 0 → 100
  on a syscall-churn concern and said its own stress test did not exercise the
  allocation-heavy forward path; `p2-11` did not either. Watch
  `rate(fastadhunter_process_minor_page_faults_total)` at flat RSS over the
  coming days — flat means 0.2.7's concern was theoretical, climbing means revert
  to 100. Idle cost measured at 6.3 faults/s.
- **~59 MiB of compile ratchet survives `delay=0`** (122 → 181.4 MiB),
  decelerating hard. Suspect is `arena_reserve` = 1 GiB (0.2.7 §5.3). A separate
  `MIMALLOC_ARENA_RESERVE` experiment, worth running only if ≤128 MiB becomes the
  target or Phase 3's footprint makes the remaining 75 MB tight.
- **`[query_log] enabled` controls two things**, not one: the disk segments
  *and* the in-memory ring (`Stats::log` returns before `ring.push`). With it
  off on the device, `GET /api/v1/queries` returns an empty page;
  `WS /api/v1/events` is unaffected — it is fed from the binary's event
  fan-out. Splitting the flag was considered and rejected; the WS feed is what
  is actually used.
- **`Ring::heap_bytes` was over- *and* under-reporting** — fixed 2026-08-06,
  [`docs/code-review/query-log-disabled-and-ring-accounting.md`](code-review/query-log-disabled-and-ring-accounting.md).
  Re-read the p2-07 residual once `query_log` is re-enabled: the `stats`
  component shifts by the ring delta.
- **`heap::string_bytes` documents "Capacity, not length"** but takes `&str`
  and returns `len()`. Same under-report class as the ring bug. Unfixed.
- **No regression guard on the compile peak.** Invisible to every automated gate,
  and the dev box cannot help — `process_rss` returns `None` on Windows. Any
  guard has to be on-device.
- The mimalloc-vs-`System` A/B to isolate the −17 % CPU from the hit-ratio
  confound.
- Forward rule 4 on the router, "Postgres ACCEPT - FAH", says
  `src-address=172.177.0.2` — a typo for `172.17.0.2`. Harmless (FAH does not use
  postgres); delete rather than fix.
- `shrink_to_fit` on the cache map is **still undecided**; a soak cannot settle
  it (tables grow from `map.capacity()`, and the cache peaked at 2.2 % of cap).
  Needs a fill-then-drain test.
- The cache byte cap's **eviction path has never fired on-device** — the entry
  cap binds first every time, so p1.5-05's headline criterion is unit-test-only.
- `ListStatus::last_refreshed` reports `null` after a restart alongside
  `last_result: Ok`. Confirmed still true 2026-08-06. Deliberately not fixed —
  reversing it needs RULE_ENGINE.md/API.md review.
- `main` is 25 commits behind this branch; merge is a fast-forward when the
  phase closes.

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (os error 10013) when WinNAT has reserved the ephemeral port block
being drawn. **Environmental** — it fails identically on a stashed clean tree.
Do not attribute it to a change.

The retry loop in `tests/common/mod.rs` survives it, but only while each boot
draws the minimum number of ports: a third draw per attempt made it lose the
race noticeably more often, which is why `Ports::http()` is lazy.
