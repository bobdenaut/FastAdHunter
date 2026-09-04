# p3-06 — testing results

Figures for the arms declared in
[plan/wip/phase3/p3-06-testing-plan.md](../../../plan/wip/phase3/p3-06-testing-plan.md)
— the plan is the binding definition; the legend below is a copy for reading
this file on its own. One section per ID, two tables each: **Runs**, one row
per run; **Figures**, shaped for the measurement, gate line last. An `INVALID`
or `degraded` run gets a Runs row and no Figures row. Raw output in the cited
`p3-06-probe/results-<ts>/` directory. A figure not in this file is not a
result.

**Campaign window.** The campaign runs on the RB5009 while the 0.3.1 soak is
still collecting, so its load lands in the soak's `/history/perf` series and
must be attributed rather than read as a regression. Windows observed so far:

| Window (UTC) | What ran | Effect seen on the router |
| --- | --- | --- |
| 2026-09-04 18:56–18:59 | naming-verification load, ~47.7 k qps of a list-blocked domain at the probe | all four cores 93–100 %, `fah-probe` 62.5–78.5 % per core |
| 2026-09-04 19:00 | SNI stage (`p0-sni.mjs`, 15 connections) | none measurable |
| 2026-09-04 19:13:05–19:13:10 | `SPLICE_BUF` sweep, container `fah-splicebench` | 30 × 64 MiB over in-device loopback in 5 s; `fah-probe` stopped throughout |
| 2026-09-04 19:18:29–19:18:38 | P4 in-device, container `fah-p4` | 18 000 loopback queries in 8.75 s; `fah-probe` stopped throughout |
| 2026-09-04 19:25:31–19:25:47 | P4-LAN (`p4-lan.mjs`) | 18 000 queries over the LAN in 15 s |
| 2026-09-04 19:28:26–19:28:29 | P5 (`p5-mint.mjs`) | 512 DoT handshakes and 256 leaf mints in 2 s |
| 2026-09-04 19:36:06 | P6 (`p6-certs-time.mjs`) | 5 CA generates + 5 API-pair imports in under 1 s |
| 2026-09-04 20:14:19–20:15:16 | D11-on-device, container `fah-certs` | criterion at defaults, ~57 s; `fah-probe` stopped throughout |
| 2026-09-04 20:28:47–20:31:07 | P5-conc A / B / A′ (`p5-conc-diag.mjs`) | 1 536 DoT handshakes and 768 leaf mints, plus three CA regenerates to purge the cache between arms |
| 2026-09-04 21:18:48–21:23:02 | P5-diag, container `fah-probediag` (`diag-timing` build) | 128 DoT handshakes across two attempts; `fah-probe` stopped throughout |
| 2026-09-05 01:22:31–01:22:41 | P4 rerun 1, container `fah-p4`, with `/tool/profile cpu=all duration=10s` over it | 18 000 loopback queries in 9.36 s; `fah-probe` stopped throughout |
| 2026-09-05 01:23:39–01:23:48 | P4 rerun 2, same container and profile | 18 000 loopback queries in 9.26 s; `fah-probe` stopped throughout |

`veth3` carries one container at a time, so `fah-probe` was stopped for both
in-device stages and restarted at 19:19:58. `fastadhunter` on `veth1` was never
stopped, reconfigured or profiled.

## Campaign status after the 2026-09-04 session

| ID | State | Gate |
| --- | --- | --- |
| SNI | run | **pass** |
| P1-loopback | run, CPU axis missing | no pick — every candidate that clears 0.9 × best is over the 32 MiB budget. Buffer decision taken 2026-09-05: **16/16 stays, budget stays 32 MiB** (§P1-loopback) — the shipped configuration, not a P1 pass |
| P1-LAN, P1-control | **parked** | no second LAN endpoint; the origin would sit on the driving host, which the plan excludes from the gate |
| P2 | **parked** | needs one host with two same-family LAN IPv4 addresses; bridged WSL was assessed and rejected — it would unblock P2 but leaves P1-control measuring a Hyper-V switch, and it risks this laptop's static DHCP lease, which two probe boot keys name |
| P3 (throughput, RSS) | **parked** | needs an h2 origin under a public name with a publicly trusted certificate on a second LAN endpoint (delta 14) |
| P4 | run ×3, all valid | **row-setter withdrawn 2026-09-05 — DoT and DoH rows return to `TBD`.** The declared statistic is not robust across sessions (§P4-reruns): the UDP control moved 101 / 175 / 148 µs, carrying DoT added from +61 to +17 / +16 µs. Transport paths are healthy — both beat their ×9 prediction |
| P4-LAN | run | diagnostic: DoT +71 µs, DoH +427 µs. The attribution against P4 was attempted 2026-09-05 and is **unresolved** (§P4-reruns) |
| P5 | run | **fail at 1.389 ms against < 1 ms** under the frozen statistic — not attributable to minting (see D11-on-device); the increment tracks the CPU speed regime, like-for-like 0.728 ms (C2, §P5-regime). Closed as a diagnostic, no code change. **Owner disposition 2026-09-05: a recorded budget miss, not a demonstrated defect — it does not block Phase 3 closure** |
| D11-on-device | run | diagnostic: `certs_mint` **450.88 µs**, inside the < 1 ms row; ARM/x86 8.4× |
| P5-conc | run | diagnostic: incremental reproducible at ~1.29 ms (conc 1); 8× concurrency removes ~0.26 ms. The remainder is not an integration cost — §P5-regime attributes it to arm order and clock regime (C2 like-for-like 0.728 ms) |
| P5-diag | run | diagnostic: server-side split reconciles P5 to 3 % — `dispatch_wait` +4.5 µs, `prewarm` +940.5 µs, `handshake_after_prewarm` +647 µs |
| P6 | run | **pass** — 4.937 ms / 6.015 ms |
| P7-store | run, script side | **pass** |
| P8-probe | run | diagnostic — naming rule verified |
| P9-probe | run | diagnostic — ~0.18 s boot-to-serving, lower bound |

Not attempted here, all owner-side on the device: Runbook 1 (dst-nat 443 v4 and
the v6 decision), Runbook 2–4 (Android CA trust, Private DNS, the pinned-app
check), Runbook 7's import-then-restart and archive-cap halves, the 24 h soak on
the production container, and P8 / P9 proper on the soak deploy.

**The two certificate results must not be conflated.** Mint performance on
target hardware **passes** (D11-on-device, 450.88 µs against < 1 ms). P5's
LAN-observed incremental cost **fails** (1.389 ms against < 1 ms) under the
plan's frozen statistic, median(first-sight pass) − median(repeat pass). P5-diag
segmented the difference from inside the probe and reconciled it to 3 %
(`prewarm` +940.5 µs, `handshake_after_prewarm` +647 µs); §P5-regime then
showed both terms are the two passes running in different clock regimes, not a
cost the first-sight path pays: paired per host on the same device the
incremental is **0.728 ms** median (C2), and at equal clock the post-pre-warm
handshake is the same in both arms. The diagnosis points at arm order and
clock regime, inferred from timings without a per-connection clock trace, and
**not** at a demonstrated extra cost in `certs_mint` or in the listener →
certificate store → TLS handshake integration. P5 is closed as a diagnostic:
no further experiment, no code change, `fah-certs` unchanged.

The P4 vs P4-LAN DoH gap was probed on 2026-09-05 with `/tool/profile cpu=all`
over two in-device P4 reruns. **The instrument cannot separate the harness
client from the server** — RouterOS reports one aggregate `container` task —
so the attribution stays **unresolved**, and **no code change follows from
it** (§P4-reruns).

## Session state and environment (end of the 2026-09-04 session)

What a later run needs and cannot derive from the figures.

| Container | Where | State |
| --- | --- | --- |
| `fastadhunter-0.3.1`, comment `fastadhunter` | `veth1`, `172.17.0.2` | production, soaking to 2026-09-08. **Never stopped, reconfigured or profiled** during the campaign |
| comment `fah-probe` | `veth3`, `172.17.0.4` | the campaign's probe. `root-dir /kingston/fahprobe/root`, `mountlists fahprobe-config,fahprobe-data`, `envlists fah-env`, `logging=yes`, `start-on-boot=no`, no `cpu-list`, no `memory-high`. Running |

`veth3` carries one container at a time and every `comment=` must be unique —
[routeros-traps.md](../../routeros-traps.md) §On-device measurement.

| Image | Where | Notes |
| --- | --- | --- |
| `fah-probe-a2d0802-rosready.tar` | on `kingston` | the probe instance |
| `fah-p4-a2d0802-rosready.tar` | on `kingston` | P4, `/fah-p4` spawning `/fah-probe`, uid 65532 (delta 10) |
| `fah-splicebench-a2d0802-rosready.tar` | on `kingston` | P1-loopback sweep |
| `fah-certs-a2d0802-rosready.tar` | on `kingston` | D11-on-device |
| `fah-probediag-a2d0802-rosready.tar` | on `kingston` | the `diag-timing` probe build used by P5-diag |
| `fah-bench-a2d0802-rosready.tar` | **repo root only, not uploaded** | 43.3 MB, five bench binaries in one image, run one at a time via `entrypoint=`: `/fah-proxy` (D6/D7), `/fah-intercept` (D8/D9), `/fah-matcher` (D5), `/fah-urlm` (URL tier, real corpus baked in at `/corpus`), `/fah-certs` (D11). Criterion must stay at **default** warm-up and measurement time or the ARM figures stop being comparable with the recorded x86 ones |

Probe configuration, already correct — the three boot keys need no further
`POST /api/v1/config` and no restart:

| Key | Value |
| --- | --- |
| `engine.mode` | `dns+http+https` |
| `egress.allow_destinations` | `["192.168.10.10"]` |
| `https.interception.clients` | `["192.168.10.10"]` — bobdenaut is the **listed** client, so its traffic takes the terminate leg |
| upstreams | `1.1.1.1`, `9.9.9.9`, UDP, `strategy = "fallback"` |
| list | `oisd-basic`, 63 109 rules, `compiled_rules` 63 110 |
| blocked domain used by SNI and P4-LAN | `analytics.google.com` (`\|\|analytics.google.com^`) |
| allowed name used by SNI | `example.com` |

Certificate store, as the campaign left it:

- CA fingerprint `61:F5:44:BE…` plus two further regenerates during P5-conc.
  Every earlier CA export is invalid.
- **`ca-archive` is at 7 of 8.** The next `ca/generate` is the ninth, which
  Runbook 7 owns as its `409 archive_full` check — do not spend it. To clear the
  leaf cache without a generate, restart the probe.
- `api_certificate.source = "imported"`: P6 imported a throwaway self-signed
  pair, so `curl -k` everywhere.
- The probe's bearer key is at `.vscode/probe.key` (gitignored); every script
  takes `--key <file>`. **Do not read `.vscode/settings.json`** — it holds
  secrets in plaintext.

Three traps that cost runs in this session:

| Trap | Effect | Avoidance |
| --- | --- | --- |
| `chrome.exe` running on the driving host | every stage prints `INVALID` at the idle precondition (plan §Running item 3); `--allow-busy` only yields `degraded`, which answers no gate. Cost four aborted runs | close it before starting a stage. `--allow-busy` is legitimate only when the figures are timestamped inside the probe, as in P5-diag |
| Git Bash path conversion | any command with a bare `/path`, a `docker -v` mount or an `openssl -subj` is mangled | `MSYS2_ARG_CONV_EXCL='*'` |
| A client calling `sock.destroy()` on `secureConnect` | the RST arrives before the server finishes `into_stream`, so the DoT listener takes its handshake-failed arm and never reaches code after it. P5-diag's first attempt emitted nothing for this reason | close with `sock.end()`. `p5-conc-diag.mjs` does; `p5-mint.mjs` is a frozen stage script and still destroys, which is consistent across its own arms |

## Legend — what each ID means

| ID | Question | Method (script, arms, count) | Gate statistic |
| --- | --- | --- | --- |
| SNI | does a blocked domain close at SNI, before any certificate | `p0-sni.mjs`, LAN client → probe `:8444`, one blocked and one allowed name, plus a no-SNI hello | every attempt closed before any certificate — boolean |
| P1-LAN | splice throughput on the deployment path | `p1-lan.mjs`, bobdenaut → probe → LAN origin (`p1-origin.mjs` on the second endpoint), 64 MiB, one connection, 5 runs; `--connections 8` aggregate arm | median of 5 ≥ 100 MiB/s **and** inside P1-control's min–max |
| P1-control | what the same LAN path carries without the probe | `p1-lan.mjs --direct`, bobdenaut → LAN origin, 5 runs | none — its min–max is the noise band |
| P1-loopback | buffer sensitivity of the splice loop (`SPLICE_BUF` sweep) | `Dockerfile.splicebench` on the device, the `splicebench` example, up 16 KiB × down {16, 32, 64, 128} KiB + 64/64 control, 5 reps each, interleaved; `/tool/profile` share during | none — per-candidate median picks (plan §Choosing `SPLICE_BUF`) |
| P2 | handshake cost: direct vs spliced vs intercepted | `p2-handshake.mjs` **on the wired bridged VM**, one public origin, 200 rounds, three arms interleaved; issuer per row proves the leg | handshake p50: intercepted ≤ 2 × spliced; row value spliced p50 − direct p50 |
| P3 throughput | intercepted h2 relay rate | `p3-h2stall.mjs`, listed client, public h2 origin, one unstalled 8 MiB stream, 3 runs | median of 3 ≥ 50 MiB/s |
| P3 RSS | process RSS growth under a 64-stream stall | `p3-h2stall.mjs`, warm-up, 64 streams to `:status 200` + first DATA then stalled, `process_rss` per second; 3 stall runs interleaved with 3 matched no-stall control runs | max stall delta over 3 runs vs ≈ 5.5 MiB, read only when attribution `RESOLVED` (min stall > max control) |
| P4 | DoT / DoH added latency vs UDP, in-engine, reused connection | `Dockerfile.p4` on the device, `encrypted_latency` harness, 3 × 2 000 per transport, interleaved | per transport p50 − UDP p50 — sets the PERFORMANCE.md row |
| P4-LAN | the same as the LAN sees it | `p4-lan.mjs`, bobdenaut → probe, blocked domain, 3 × 2 000, one connection per transport per batch | none — diagnostic |
| P5 | cold leaf mint per first-sight host | `p5-mint.mjs`, 256 hosts over DoT `:853`, CA on the probe, one first-sight and one repeat handshake per host | median(first-sight) − median(repeat) < 1 ms — incremental p50 estimate |
| P6 | CA generate / API-pair import time | `p6-certs-time.mjs`, 5 generates + 5 imports, `time_starttransfer − time_appconnect` | median of 5: generate < 100 ms, import < 50 ms |
| P7-store | key material never leaves the probe over the API | `p7-store.mjs`, `ca/export` both formats + `/config` searched for the CA key payload, traversal list against `:8443` | each check pass / fail |
| P8-probe | CPU the probe burns under a stage's load | `/tool/profile cpu=all`, read-only, idle baseline first, test binary named apart | none — diagnostic |
| P9-probe | probe boot-to-serving with the phase-3 listeners | container log timestamps | none — diagnostic |

## SNI

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 19:00 | `a2d0802ddc55-dirty` | RB5009, probe `fah-probe` 0.3.1 on `veth3`; driven from bobdenaut over the LAN; corpus `oisd-basic`, 63 109 rules, `compiled_rules` 63 110; 5 attempts per kind | PASS (adapter reported `Public`, irrelevant — every SNI attempt is outbound) | valid | 13 | `results-20260904T1900Z/` |

### Figures

| Name | Kind (allowed / blocked / no-SNI) | Closed before certificate | Close latency (ms) |
| --- | --- | --- | --- |
| `analytics.google.com` (`\|\|analytics.google.com^`, `oisd-basic`) | blocked, n=5 | yes — 5/5 `closed_silent`, 0 bytes | min 0.890 / p50 1.443 / max 1.740 |
| — (hello with no SNI extension) | no-SNI, n=5, `https.sni.no_sni = "pass"` | yes — 5/5 `closed_silent`, 0 bytes | min 0.955 / p50 1.646 / max 2.266 |
| `example.com` | allowed, n=5 | no — 5/5 `server_hello` (the precondition: the listener is not closing everything) | min 18.805 / p50 24.009 / max 38.366 |
| counters | `listeners.https` delta over the run | `connections` 15, `requests` 15, `blocked` 5 = attempts × 1 under `no_sni = "pass"` (delta 13 rule b); `resolve_failures` 0 | — |
| gate | every blocked and no-SNI attempt closed before a certificate | **pass** — 10/10 closed before any certificate, allowed name reached ServerHello | — |

## P1-LAN

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |

### Figures

| Build (up / down KiB) | Arm (single / aggregate 8) | n | Median MiB/s | Min | Max | P1-control min–max | `/tool/profile` share |
| --- | --- | --- | --- | --- | --- | --- | --- |
| gate | single-connection median ≥ 100 MiB/s and inside P1-control's min–max | | | | | | |

## P1-control

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |

### Figures

| Run | MiB/s |
| --- | --- |
| band (min / p50 / max) | |

## P1-loopback

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 19:13 | `a2d0802` | RB5009, in-device loopback, container `fah-splicebench` on `veth3` (`fah-probe` stopped for the duration); `splicebench` example, 64 MiB per connection, one connection, 5 reps, candidates interleaved per rep; mimalloc | router idle apart from this container; `fastadhunter` serving household traffic on `veth1` | valid as throughput; **CPU axis absent** — no `/tool/profile` share (run was 5 s, shorter than the window) | 1 | not kept — figures read off the container log, which is not tracked |

### Figures

| Candidate (up / down KiB) | n | Median MiB/s | Min | Max | Worst case at 1024 | In budget | `/tool/profile` share |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `loopback_origin` (not a control) | 5 | 925.3 | 819.3 | 1045.7 | — | — | not captured |
| 16 / 16 (shipped) | 5 | 341.4 | 267.8 | 364.4 | 32 MiB | yes | not captured |
| 16 / 32 | 5 | 460.5 | 454.9 | 469.8 | 48 MiB | no | not captured |
| 16 / 64 | 5 | 480.1 | 369.6 | 496.5 | 80 MiB | no | not captured |
| 16 / 128 | 5 | 479.5 | 437.3 | 538.6 | 144 MiB | no | not captured |
| 64 / 64 (symmetry control) | 5 | 488.8 | 429.5 | 517.3 | 128 MiB | no | not captured |
| `up` sensitivity | — | 64/64 over 16/64 = **+1.8 %**, under the +10 % rule — `up` stays 16 | | | | | |
| pick | smallest in-budget candidate with median ≥ 0.9 × best | **none** — 0.9 × best = 439.9 MiB/s and the only in-budget candidate (16/16) medians 341.4 | | | | | |

Every candidate sits 3–4 × above the 119 MiB/s NIC, so on the wire they tie;
the axis that separates them is CPU per relayed byte, and **this run has no CPU
figure** — the sweep finished in 5 s, shorter than the profile window, so the
`/tool/profile` share was never captured and §Invalidity rules label these
"throughput of the loop". Step 4's confirmatory CPU reading is the P1-LAN
aggregate arm, which is parked. Nothing here justifies changing `SPLICE_BUF`.

**Owner decision, taken 2026-09-05** (plan §Choosing step 3, outside the
procedure as the plan requires): `SPLICE_BUF` stays **16 KiB per direction**
(32 KiB per session) and the budget stays **32 MiB** at `max_connections =
1024`. Grounds: the sweep establishes no CPU-per-relayed-byte advantage for a
larger buffer — it carries no `/tool/profile` share, so its figures are
throughput of the loop (§Invalidity rules), never CPU per byte — and every
candidate already sits 3–4 × above the 119 MiB/s NIC, where the wire ties them.
`max_connections` does not move. **This is the shipped configuration, not a P1
result: P1-LAN remains the required confirmation of the ≥ 100 MiB/s row and is
still parked.** One consequence for step 4 — with no pick, P1-LAN carries one
build rather than two.

What the decision does **not** claim: 16/16's and 16/32's ranges are disjoint
over 5 interleaved repetitions, so a real effect exists between 16 and 32 KiB
down. It is simply not attributed to the relay (the loopback loop contains the
client and origin halves too) and not observable on a gigabit wire. 16/16 is
also the weakest candidate by that margin, so a future P1-LAN that misses the
≥ 100 MiB/s row at 16/16 reopens this decision against the same budget.

The plan's three predictions all held: 32 KiB down captures 86 % of the
16 → 64 gain, 128 ≈ 64 (479.5 vs 480.1), and `up` is irrelevant for a download.

## P2

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity / identities | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |

### Figures

| Arm | n | Handshake min | p50 | p99 | First-byte min | p50 | p99 | Served issuer (all rows) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| direct | | | | | | | | |
| spliced | | | | | | | | |
| intercepted | | | | | | | | |
| row: spliced p50 − direct p50 (handshake / first-byte) | | | | | | | | |
| gate: intercepted p50 / spliced p50 ≤ 2 (handshake) | | | | | | | | |

Counters (supporting): `minted_total`, `blocked`, `connections` before / after
each arm — one line per run in the Runs row's raw directory, summarised here.

## P3 throughput

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |

### Figures

| Run | MiB/s |
| --- | --- |
| gate: median of 3 ≥ 50 MiB/s | |

## P3 RSS

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity / attribution | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |

### Figures

| Run | Kind (stall / control) | Before (MiB) | Max during | After | Delta (MiB) | Barrier met | Window exclusive |
| --- | --- | --- | --- | --- | --- | --- | --- |
| attribution: min stall delta > max control delta ⇒ `RESOLVED` | | | | | | | |
| gate: max stall delta vs ≈ 5.5 MiB (read only when `RESOLVED`) | | | | | | | |

## P4

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 19:18 | `a2d0802` | RB5009, in-device, container `fah-p4` on `veth3` (`fah-probe` stopped); `encrypted_latency` harness spawning `/fah-probe` on loopback with ephemeral ports and an in-process mock upstream; blocked domain answered in-engine (no cache, no upstream); 3 interleaved rounds × 2 000 per transport, handshakes excluded | router idle apart from this container | valid | 10, 12 | not kept — figures read off the container log, which is not tracked |

### Figures

| Transport | n | Min | p50 | p99 | p50 − UDP p50 |
| --- | --- | --- | --- | --- | --- |
| udp | 6 000 | 97 µs | 101 µs | 282 µs | — |
| dot | 6 000 | 116 µs | 162 µs | 396 µs | **+61 µs** |
| doh (POST, `HTTP/2.0`) | 6 000 | 579 µs | 1 016 µs | 1 980 µs | **+915 µs** |
| row: was to set PERFORMANCE.md "DoT / DoH added latency vs UDP, p50" | | | | | ~~DoT +61 µs, DoH +915 µs~~ — **withdrawn 2026-09-05**, see §P4-reruns |

Diagnostics: p90 udp 190 µs, dot 279 µs, doh 1 341 µs; max udp 677 µs, dot
3 304 µs, doh 6 984 µs. DoT handshakes, excluded from the per-query figures,
were 2.089 / 1.987 / 1.953 ms. Per-round p50s were udp 140 / 101 / 101, dot
226 / 162 / 131, doh 1 033 / 980 / 1 022 µs — round 1 sits high on every
transport and rounds 2–3 settle; the pooled p50 is the declared statistic, so
this is recorded, not corrected. Delta 12 is satisfied: the harness asserted
`Some(HTTP/2.0)` on the DoH arm, so this column and P4-LAN's describe one
protocol.

DoH costs ~6 × DoT and ~10 × UDP on this device. There is no budget to fail —
P4 is the arm that was to set the row. **Two reruns on 2026-09-05 withdrew
these added-latency figures as row-setters** (§P4-reruns); the absolute
columns above stand as measured.

## P4-LAN

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 19:24 | `a2d0802ddc55-dirty` | bobdenaut → probe `172.17.0.4`; run aborted at the precondition, nothing measured | FAIL — `chrome` running | INVALID (plan §Running item 3) | — | not kept — the run produced no figure |
| 2026-09-04 19:25 | `a2d0802ddc55-dirty` | RB5009 probe `fah-probe` 0.3.1 on `veth3`, driven from bobdenaut over the LAN; blocked domain `analytics.google.com` (`oisd-basic`, answered in-engine, `0.0.0.0` under `null_ip`); 3 rounds × 2 000 per transport, one connection per transport per batch | PASS | valid | — | `results-20260904T1925Z/` |

### Figures

| Transport | n | p50 | p99 | Batch wall-clock mean | Unanswered | Unmatched |
| --- | --- | --- | --- | --- | --- | --- |
| udp | 6 000 | 0.334 ms | 0.637 ms | 734.4 ms | 0 | 0 |
| dot (TLS 1.3, issuer `FastAdHunter CA`) | 6 000 | 0.405 ms | 0.728 ms | 903.1 ms | 0 | 0 |
| doh (h2 POST `/dns-query`) | 6 000 | 0.761 ms | 1.337 ms | 1 702.9 ms | 0 | 0 |
| diagnostic — no gate | added vs UDP p50: **DoT +71 µs, DoH +427 µs** | | | | | |

Handshakes, excluded from the per-query figures and paid once per batch: DoT
12.077 / 7.457 / 7.478 ms, DoH 13.386 / 6.933 / 6.067 ms. The DoT arm's served
issuer is `FastAdHunter CA`, so the listener minted a leaf for the SNI the
client sent (`dns.fah.test`).

**Discrepancy against P4, owed an attribution.** The in-device P4 run 7 minutes
earlier gave DoT +61 µs and DoH +915 µs. DoT agrees across the two (+61 vs
+71 µs — the LAN hop costs about 10 µs), but DoH is **less than half** over the
LAN despite carrying that same extra hop, which no ordering of the two can
explain on its own. The difference between the arms is where the h2 client
runs: in-device it competes for the same four ARM cores as the server, over the
LAN it sits on bobdenaut. DoT shows no such gap, consistent with its 2-byte
length framing against h2 framing plus HPACK. If that holds, the in-device
+915 µs contains client cost a real DoH client would not impose on the device.
**Neither figure is adjusted here** — both stand as measured. The
`/tool/profile cpu=all` read this paragraph proposed was taken on 2026-09-05
and **could not separate the two processes**; see §P4-reruns.

## P4-reruns — diagnostic, session-to-session stability of the P4 statistic

Not a stage. Two further runs of the **unchanged** `fah-p4` container and the
unchanged declared workload (3 rounds × 2 000 per transport, interleaved,
handshakes excluded), taken to answer the attribution §P4-LAN left open and,
incidentally, to see whether P4's figures reproduce. The container was re-added
from the image already on the store; `fah-probe` was stopped for both, and
`fastadhunter` on `veth1` was not touched.

The attribution rule was declared before the runs: with `S_client` the
`fah-p4` share and `S_server` the `fah-probe` share summed over cores,
`S_client < 0.2 × S_server` would leave P4's DoH figure standing as in-engine,
and anything else would make it an upper bound.

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-05 01:22 | `a2d0802` | RB5009, in-device, container `fah-p4` on `veth3` (`fah-probe` stopped); as §P4 — `encrypted_latency` spawning `/fah-probe` on loopback, blocked domain answered in-engine; run 9.36 s | `/tool/profile` idle baseline first: cpu0–3 at 1.5 / 1.5 / 0 / 1 %, no `container` row | valid (diagnostic) | 10, 12 | not kept — figures read off the container log, which is not tracked |
| 2026-09-05 01:23 | `a2d0802` | as above, second start of the same container; run 9.26 s | same baseline | valid (diagnostic) | 10, 12 | not kept — as above |

### Figures

| Session | UDP p50 | DoT p50 | DoH p50 | DoT − UDP | DoH − UDP |
| --- | --- | --- | --- | --- | --- |
| 2026-09-04 19:18 (§P4) | 101 µs | 162 µs | 1 016 µs | **+61 µs** | **+915 µs** |
| 2026-09-05 01:22 | 175 µs | 192 µs | 1 044 µs | **+17 µs** | **+869 µs** |
| 2026-09-05 01:23 | 148 µs | 164 µs | 1 041 µs | **+16 µs** | **+893 µs** |
| spread across the three | **73 %** | 18 % | **3 %** | 61 → 16 | 915 → 869 |
| diagnostic — no gate | n = 6 000 per transport per session, `Some(HTTP/2.0)` asserted on every DoH arm (delta 12) | | | | |

| `/tool/profile cpu=all`, 10 s over each run | cpu0 | cpu1 | cpu2 | cpu3 | summed |
| --- | --- | --- | --- | --- | --- |
| read 1 — `container` | 21.5 % | 32.5 % | 35.5 % | 33 % | **122.5 %** |
| read 2 — `container` | 34 % | 27.5 % | 32.5 % | 28 % | **122 %** |
| `fah-p4` | 0 % (read 1, cpu0 only) | — | — | — | — |
| `fah-probe` | absent from both reads | | | | |

**The attribution instrument does not work here.** RouterOS charged all
container work to one aggregate `container` task — ~1.22 cores, stable across
both reads — and never named the two processes separately. `fah-p4` appeared
once at 0 % and the spawned `/fah-probe` never appeared at all. The 18:58 read
in §P8-probe separated `fah-probe` because it was the container's PID 1 under
external load; here PID 1 is the harness and the server is its child. Neither
`S_client` nor `S_server` is obtainable, so the pre-declared rule cannot be
evaluated and **client/server CPU attribution stays unresolved**. The
observation that in-device DoH absolute p50 (1 041 µs) exceeds P4-LAN's
(761 µs) despite the LAN's extra hop is **suggestive of harness cost and does
not establish it**.

**What the reruns do settle: the declared statistic is not robust.** The gate
quantity is a difference of two p50s where the subtrahend moved 73 % between
sessions while the difference is roughly a sixth of either operand. DoT added
latency therefore reads +61, +17 and +16 µs across three valid sessions of the
same workload on the same build. **`p50(DoT) − p50(UDP) = +61 µs` is withdrawn
as a row-setter**, and no replacement is chosen from the other two — picking
one of three would repeat the error.

**The transport paths are healthy, and no code change follows.** Both beat the
project's ×9 x86 → RB5009 conversion: D13's x86 DoT 45 µs and DoH 158 µs
predict ≈ 405 µs and ≈ 1 422 µs on this device, against 162–192 µs and
1 016–1 044 µs measured. DoH is the *most* stable quantity in the set at ~3 %
across three sessions, and the transport ratios match x86's shape (DoH ≈ 6 ×
DoT in both). The unstable element is the UDP control, which is not the path
under evaluation. **Nothing here justifies touching `dot.rs`, the DoH path or
opening an implementation investigation** — this is benchmark metrology, not a
defect.

**Consequence for the row.** Both the DoT and the DoH PERFORMANCE.md rows
return to `TBD`, pending a more robust declared statistic. The methodology fix
this points at: pool the UDP control across sessions rather than reading it
once per session, so the control's own variance cannot dominate a
transport − UDP difference. Every future in-device latency arm inherits that
jitter — the baseline moved 101 → 175 µs on an otherwise idle router.

## P5

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 19:28 | `a2d0802ddc55-dirty` | RB5009 probe `fah-probe` 0.3.1 on `veth3`, driven from bobdenaut over the LAN; 256 synthetic first-sight hosts over DoT `:853`, handshake only, no query; seed `20260904T192825Z`; leaf cache 1 → 257 against capacity 512 | PASS | valid — gate **fails** | 3, 6 | `results-20260904T1928Z/` |

### Figures

| Arm | n | Min | p50 | p99 | Served issuer (all rows) |
| --- | --- | --- | --- | --- | --- |
| first-sight | 256 | 1.861 ms | 3.322 ms | 5.753 ms | `FastAdHunter CA` |
| repeat | 256 | 1.204 ms | 1.933 ms | 3.779 ms | `FastAdHunter CA` |
| counters: `minted_total` / `unwarmed_misses` / `evictions` delta | — | 256 / 0 / 0 — mints equal the host count exactly, the repeat pass minted 0 and evicted nothing, `prewarm_hits` 256 | | | |
| gate: p50(first-sight) − p50(repeat) < 1 ms | — | **fail at 1.389 ms against < 1 ms** | | | |

Every precondition held and the run took the shipped path, so this is a gate
failure — a performance miss against the budget — not a defect.

**Attribution, settled below: the failure is not the mint.** `certs_mint`
measured **450.88 µs** in-process on the same device (§D11-on-device), inside
the < 1 ms budget, and at **8.4×** the dev box's 53.64 µs — the project's ~9×
x86 → RB5009 factor, holding.

**Conclusion.** P5's increment depends on the CPU speed regime the device is
in, not on the mint. Under the frozen pass-vs-pass statistic it is reproducibly
over budget (1.25–1.39 ms, four sessions); paired per host the median is
0.728 ms with 74 of 240 pairs still over 1 ms. The mint itself is inside budget
(450.88 µs, D11-on-device). Mechanism inferred from the 1 : 2 : 4 interval
levels, including on the crypto-free `dispatch_wait` path; no per-connection
clock trace exists. Evidence in §P5-regime, dispositions at the end of
§P5-diag. **Nothing in `fah-certs` changes on this evidence.**

**Phase-3 disposition (owner, 2026-09-05).** P5 is a recorded
performance-budget miss, **not a demonstrated implementation defect**, and
**does not block Phase 3 closure**. `certs_mint` meets the D11-on-device
budget; the P5 end-to-end increment is not reliably < 1 ms on this device.

Two hypotheses this section carried are refuted, and are recorded rather than
deleted: that a `spawn_blocking` dispatch-and-wake asymmetry explains the gap —
§P5-diag measured `dispatch_wait` at **+4.5 µs**, so the plan's "paid by both
arms and cancels" was right; and, in an earlier revision, that the ~9× factor
fails for crypto workloads — §D11-on-device measured **8.4×**.

## D11-on-device — `certs` criterion suite on the RB5009

The diagnostic the plan names beside P5 ("On-device `certs_mint` (criterion) is
the diagnostic beside it"). In-process, no network, no `spawn_blocking`, no LAN
— an independent instrument for the same `CertStore::prewarm` call P5 measured
across the wire.

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 20:14 | `a2d0802` | RB5009, container `fah-certs` on `veth3` (`fah-probe` stopped for the duration), `CRITERION_HOME=/tmp/criterion`; criterion defaults — 3 s warm-up, 100 samples — the same settings as the x86 D11 run; `certs_mint` rotates 4 096 hosts against a 512-entry cache, so every iteration mints and the eviction scan is inside the sample | router idle apart from this container | valid (diagnostic) | — | not kept — figures read off the container log, which is not tracked |

### Figures

| Bench | RB5009 (mean [lo hi]) | Dev box, 2026-09-02 | ARM / x86 |
| --- | --- | --- | --- |
| `certs_mint` | **450.88 µs** [445.52 457.11] | 53.64 µs | **8.4×** |
| `certs_cache_hit` | 378.71 ns [378.47 378.92] | not recorded here | — |
| `certs_prewarm_warm` | 563.12 ns [562.90 563.40] | not recorded here | — |
| `certs_replay_zipf` | 145.45 µs [143.53 147.40] | not recorded here | — |
| diagnostic — no gate of its own | reads against PERFORMANCE.md's `< 1 ms` cold-`prewarm` row: **met**, at 45 % of it | | |

**What this settles.** The ~9× x86 → RB5009 factor **holds** for this workload —
8.4× measured, against a factor derived from the DNS/HTTP pipeline. An earlier
revision of the P5 section claimed the opposite from the LAN figure alone; that
claim is withdrawn.

**What it opens.** P5's 1 389 µs incremental minus this 451 µs leaves ~940 µs
that the mint does not explain. `certs_prewarm_warm` at 563 ns says the repeat
arm's pre-warm is essentially free, so the gap sits in what the first-sight arm
does *around* the mint — the leading candidate is `spawn_blocking` dispatch and
worker wake-up, which the plan assumed cancels between the arms. Unproven here.

`certs_replay_zipf` reports `hit_rate = 0.6732` over 100 000 synthetic Zipf
handshakes. **MA-7 struck the synthetic Zipf hit rate as evidence for the
minted-leaf hit-rate row**; only the soak's `leaf_cache` counters count for that.
It is recorded as a shape, not as a hit-rate figure.

## P5-conc — diagnostic, P5 path segmentation by concurrency

Not a stage and not in the frozen plan. `p5-mint.mjs` was left untouched; the
arms run from `p5-conc-diag.mjs`, the same instrument (TCP connect →
`secureConnect` to `:853`, host as SNI, 256 fresh hosts, first-sight pass then
repeat pass) with one variable changed: how many handshakes are in flight.

The leaf cache holds 512 entries and each arm inserts 256, so the CA was
regenerated between arms to purge it. That spends `ca-archive` slots: the run
left **7 of 8** used, and the next generate is the ninth, which Runbook 7 owns
as its `409 archive_full` check.

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 20:28 (A) | `a2d0802ddc55-dirty` | RB5009 probe `fah-probe` 0.3.1 over the LAN, `--conc 1`, 256 hosts, leaf cache empty at start (probe restarted 31 s earlier) | PASS | valid (diagnostic) | — | `results-20260904T2030Z-conc/` |
| 2026-09-04 20:29 (B) | `a2d0802ddc55-dirty` | as A, `--conc 8`, cache purged by a CA regenerate | PASS | valid (diagnostic) | — | `results-20260904T2030Z-conc8/` |
| 2026-09-04 20:30 | `a2d0802ddc55-dirty` | ordering control, aborted at the precondition; the CA regenerate before it had already purged the cache | FAIL — `chrome` running | INVALID | — | `results-20260904T2030Z-conc1b/` (overwritten by the rerun) |
| 2026-09-04 20:31 (A′) | `a2d0802ddc55-dirty` | ordering control, `--conc 1`, same cache purge, probe uptime 146 s | PASS | valid (diagnostic) | — | `results-20260904T2030Z-conc1b/` |

### Figures

| Arm | conc | Incremental p50 (first-sight − repeat) | first-sight p50 | repeat p50 | first-sight handshakes/s | repeat handshakes/s |
| --- | --- | --- | --- | --- | --- | --- |
| A | 1 | 1.321 ms | 3.699 ms | 2.378 ms | 169.3 | 212.6 |
| B | 8 | **1.031 ms** | 5.794 ms | 4.763 ms | 752.3 | 862.5 |
| A′ | 1 | 1.252 ms | 3.117 ms | 1.865 ms | 215.8 | 331.6 |
| diagnostic — no gate | A/A′ bracket at 1.321 / 1.252 ms (5.4 % apart); B falls outside that band | | | | | |

Every arm: 256/256 handshakes, 0 errors, `minted_total` +256 exactly, repeat
pass minted 0, `evictions` 0, served issuer `FastAdHunter CA` on every row.

**What is demonstrated.** The incremental is reproducible at **~1.29 ms** at
conc 1 across two sessions (P5 itself read 1.389 ms). Raising concurrency 8×
**reduces the measured incremental median by ~0.26 ms**. That figure bounds the
observable concurrency-sensitive component of the gap; it does **not** identify
that component as `spawn_blocking` dispatch or wake-up latency. The effect is
partial — about a fifth of the difference between the incremental and
`certs_mint`'s 451 µs — so concurrency sensitivity is not shown to be the main
cause.

**Roughly 0.6 ms remained unattributed at the time of this run**, and was not to
be split between scheduling and CPU without instrumentation. It is no longer
open: §P5-regime attributes it to the execution-regime difference between two
sequential passes.

**Withdrawn:** an estimate of ~0.68 ms of CPU per first-sight handshake, derived
from the wall-clock difference between the two passes at conc 8. A′ refutes the
method — the same wall-clock difference reads 1.202 ms in A and 1.618 ms in A′,
a 35 % spread — and the raw throughputs moved 27–56 % between A and A′ (probe
uptime 31 s against 146 s). Differencing the medians cancels that drift; the
wall-clock comparison does not. No conclusion rests on it.

The next experiment named here — instrumenting the listener path with
timestamps at request, dispatch, worker start, mint completion and response —
was built as the `diag-timing` feature and run as §P5-diag. It **refuted** the
dispatch-and-wake hypothesis (`dispatch_wait` +4.5 µs), and §P5-regime then
attributed the remainder to arm order and CPU speed regime. No further
experiment follows.

## P5-diag — diagnostic, listener-side segmentation of the P5 gap

Not a stage. Timestamps taken **inside the probe** by an off-by-default
`diag-timing` Cargo feature on `fah-dns` (`dot.rs`): SNI parsed → before
`spawn_blocking` → first statement inside the closure → `store.prewarm()`
returned → `into_stream()` completed. The shipped binary carries none of it;
the feature exists only for this run. Driven by `p5-conc-diag.mjs --conc 1
--hosts 32` — 32 hosts rather than 256 because each SNI connection emits one
log line and the container log is the only channel off the device.

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 21:19 | `a2d0802` + `diag-timing` | first attempt; the client called `sock.destroy()` on `secureConnect`, resetting the connection before the server finished `into_stream`, so the listener took its handshake-failed arm and emitted no timing line | FAIL — `chrome` | degraded, **no output** | — | not kept — the run produced no figure |
| 2026-09-04 21:23 | `a2d0802` + `diag-timing` | RB5009, container `fah-probediag` on `veth3` (`fah-probe` stopped), mountlists shared with the normal probe so the existing CA is in the store; client closes gracefully with `sock.end()`; 32 first-sight then 32 repeat DoT handshakes, `--conc 1` | FAIL — `chrome`; client-side figures discarded, server-side intervals unaffected | degraded (diagnostic) | — | `results-20260905T0025Z-diag/`, transcribed rows in `dot-timing.tsv` |

### Figures

| Interval | first-sight p50 | repeat p50 | delta |
| --- | --- | --- | --- |
| `sni_to_dispatch_us` | 0 µs | 0 µs | 0 |
| `dispatch_wait_us` | 51.5 µs | 47 µs | **+4.5 µs** |
| `prewarm_us` | 944.5 µs | 4 µs | **+940.5 µs** |
| `handshake_after_prewarm_us` | 2 391.5 µs | 1 744.5 µs | **+647 µs** |
| server-side sum | 3 387.5 µs | 1 795.5 µs | **+1 592 µs** |
| diagnostic — no gate | n = 32 per arm; ranges in `dot-timing.tsv` | | |

**The instrumentation reconciles P5.** The client measured an incremental of
1 545 µs on this same run; the server-side deltas sum to 1 592 µs, 3 % apart.
No part of the gap is outside the four intervals.

Read strictly as measured:

- **`dispatch_wait`: +4.5 µs.** The two arms are practically identical, so
  `spawn_blocking` does not explain the gap. The hypothesis carried from
  §P5-conc — dispatch and worker wake-up — is refuted by this run.
- **`prewarm`: +940.5 µs.** The largest observed difference between the arms,
  and the interval that contains the mint.
- **`handshake_after_prewarm`: +647 µs.** A real difference between arms that
  both serve an already-cached leaf by that point. The cause is **not
  identified**.

Two facts sit beside each other and are not reconciled **by this run**: the same
`store.prewarm()` call measures **944.5 µs** inside the listener and **451 µs**
under criterion (§D11-on-device). Why the two contexts differ, and why the
post-pre-warm handshake differs between arms, were open questions when this run
was recorded. Neither is answered here and neither is inferred from it.

**Both were closed afterwards by §P5-regime**, which is the update to read
against this section. The 944.5 µs is the mixed-regime median of a mint whose
fastest listener rows read 513–522 µs against criterion's 450.88 µs — +14 %,
~60 µs of real context cost. The +647 µs is the **repeat arm's advantage** from
running as its own dense pass after the first-sight pass: at the fastest regime
level the two arms' post-pre-warm handshakes are equal.

**Verdict unchanged.** P5 remains **FAIL** at 1.389 ms against < 1 ms, and the
failure is **not** a `certs_mint` performance failure — D11-on-device passes
independently at 450.88 µs. **No change to `fah-certs` follows from this.**

This section originally pointed the next step at the integration of listener →
certificate store → TLS handshake. **§P5-regime rejected that direction**: at
equal regime the post-pre-warm handshake is the same in both arms, which bounds
cache eviction, allocator cross-thread frees, lazy key setup, a cold `Arc` and
core migration together to under ~100 µs. The direction is recorded as
superseded, not deleted.

## P5-regime — diagnostic, arm order and clock regime (C0 / C1 / C2)

Not a stage. Takes the two open questions from §P5-diag — the +647 µs
`handshake_after_prewarm` difference between arms that both serve a cached
leaf, and `prewarm` at 944.5 µs in the listener against 450.88 µs under
criterion — first by re-reading the rows already in this file, then with three
client-side runs on 2026-09-04/05. No Rust change, no `diag-timing`; the
regular probe image `a2d0802` (`fah-probe-a2d0802-rosready` on `veth3`), the
client scripts from tree `3bb12c0-dirty`. The production container was not
touched.

**Re-reading §P5-diag's rows (`dot-timing.tsv`, n = 32 per arm).** Every
interval sits on discrete levels at ratios ≈ 1 : 2 : 4, including the pure
kernel wake path that carries no crypto: `dispatch_wait` 34–49 / 69–76 /
120–147 µs; `prewarm` 513–522 / 940–1060 / 2 227 µs; `handshake_after_prewarm`
1 479–1 669 / ~2 350 / 3 374–4 142 µs. Consecutive rows share a level in blocks
of 4–10. The **repeat arm shows the same levels with no mint in it** (rows
7–10: `dispatch_wait` 120–147, handshake 3 374–4 142). At the fastest level the
first-sight handshake (1 479–1 669 µs, n = 7) equals the repeat handshake
(1 531–1 815 µs, n = 17): there is no cost paid after the mint at equal clock,
which bounds cache eviction, mimalloc cross-thread frees, lazy key setup, a
cold `Arc` and core migration together to under ~100 µs. §P5-conc's client
rows agree: floor-to-floor the arms differ by 0.5–0.8 ms (A: min 1.85 vs 1.34
ms, p10 2.27 vs 1.50; A′: min 1.85 vs 1.23, p10 2.02 vs 1.33) while the medians
differ by 1.25–1.32 ms. The median incremental is a regime-mix difference.

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 23:08 (C0) | `a2d0802` / `3bb12c0-dirty` | RB5009 `fah-probe` over the LAN, `p5-conc-diag.mjs --conc 1 --hosts 240`, cache empty at start, alone | PASS | valid (diagnostic) | — | `results-20260904T2308Z-c0/` |
| 2026-09-04 23:08 | `a2d0802` / `3bb12c0-dirty` | `p5-clock-load.mjs --hosts 16 --conc 2 --duration 90`: 16 hosts minted, then closed-loop repeat handshakes, 597–598/s at p50 2.06 ms for the first 20 s, 0 errors in that window; after ~12 000 connections the client ran out of ephemeral ports and the load collapsed (82 663 client-side errors, all after C1 finished) | PASS | degraded (client port exhaustion, after C1) | — | `results-20260904T2308Z-c1-load/` |
| 2026-09-04 23:09 (C1) | `a2d0802` / `3bb12c0-dirty` | as C0, run inside the load's second clean window (598/s, 0 errors), nothing changed on the probe between C0 and C1, cache 240 + 16 → 496 | PASS | valid (diagnostic) | — | `results-20260904T2308Z-c1/` |
| 2026-09-04 23:15 | — | owner restarted `fah-probe` to purge the cache (496 of 512; no CA generate, archive stays 7 of 8) | — | — | — | — |
| 2026-09-04 23:16 (C2) | `a2d0802` / `3bb12c0-dirty` | `p5-paired-diag.mjs --hosts 240`: per host one first-sight handshake then immediately its repeat, cache empty at start, alone | PASS | valid (diagnostic) | — | `results-20260904T2316Z-c2/` |

Every arm: 240/240 handshakes, 0 errors, `minted_total` +240 exactly,
`evictions` 0, served issuer `FastAdHunter CA` on every row. `cpu-load` peaked
at 53 % during C1. A read-only 5 Hz `/system/resource/print` poll over ssh ran
beside C0, C1 and C2 symmetrically; its logs sit outside the repo and are not
part of the record (see the retraction below).

### Figures

| Arm | design | first-sight p50 | repeat p50 | incremental |
| --- | --- | --- | --- | --- |
| C0 | sequential passes, alone | 2.883 ms | 1.490 ms | **1.393 ms** |
| C1 | sequential passes, beside 598/s background load | 2.799 ms | 1.982 ms | **0.817 ms** |
| C2 | paired per host | 2.930 ms | 2.049 ms | **0.728 ms** paired median (p25 0.49, p75 1.06); 0.891 ms median − median; 166/240 pairs under 1 ms |
| diagnostic — no gate | C0 reproduces P5 (1.389) and §P5-conc A / A′ (1.321 / 1.252): fourth session, same figure | | | |

Read strictly as measured:

- **C0 → C1.** With the clock kept busy the incremental falls under 1 ms. The
  load adds contention to both arms (repeat +0.49 ms at p50), so the absolute
  figures are confounded; the incremental is not, both arms carry the same
  contention. Underneath it the first-sight arm sped up by ~0.57 ms.
- **C0 → C2.** The first-sight arm is unchanged (2.883 → 2.930 ms). The repeat
  arm slows by 0.56 ms once it is interleaved with the mints (1.490 → 2.049 ms).
  The +647 µs of §P5-diag was the **repeat arm's advantage** from running as
  its own dense pass after the first-sight pass, not a cost the first-sight
  handshake pays. Regime blocks persist in both arms of C2 and switch together
  (within-pair correlation 0.44, diff sd 1.10 ms, 32 pairs above 1.5 ms, 9
  negative): pairing cancels most of the regime variation, not all of it.
- **Q1, the listener-vs-criterion `prewarm` gap.** 944.5 µs is the
  mixed-regime median of a mint whose fastest listener rows read 513–522 µs
  against 450.88 µs under criterion: +14 %, ~60 µs of real context cost. Not
  worth chasing. The x86 1.49× (85 vs 57 µs, 2026-09-05, one degraded session)
  is the same class.

### x86 comparison — supporting diagnostic, no gate, no row-setting

The same instrument on the dev box, to separate the mint from the rest of the
end-to-end increment on a platform whose clock does not step the same way.
**Raw output is outside the repo, at `E:\tmp\p5x86\`** — this subsection is the
only record of it, and it is supporting evidence, not a repo-complete artefact.
It sets no row and changes no verdict.

Dev box x86_64 Windows 11, 2026-09-05, tree `3bb12c0-dirty`. Release binary
built with `cargo build --release -p fastadhunter --features
fah-dns/diag-timing`, run as a local full-mode instance on loopback;
`p5-conc-diag.mjs --conc 1 --hosts 32` against it. Both arms **degraded** — the
box was not idle and its editor cannot be closed. n = 32 per arm, 32/32
handshakes, `minted_total` +32 exactly, `evictions` 0, served issuer
`FastAdHunter CA` on every row. Control `certs_mint` **re-run in the same
session**: 57.055 µs [56.342 57.942] unpinned, against the stored pinned
53.64 µs.

| Interval, p50 | x86 first-sight | x86 repeat | x86 delta | RB5009 delta |
| --- | --- | --- | --- | --- |
| `sni_to_dispatch` | 0 µs | 0 µs | 0 | 0 |
| `dispatch_wait` | 9 µs | 8 µs | +1 µs | +4.5 µs |
| `prewarm` | 85 µs | 1 µs | **+84 µs** | +940.5 µs |
| `handshake_after_prewarm` | 747 µs | 760 µs | **−13 µs** | +647 µs |
| server-side sum | 841 µs | 769 µs | **+72 µs** | +1 592 µs |
| supporting diagnostic — no gate | client incremental 65 µs against the 72 µs server sum, 10 % apart | | | |

- **The mint and the rest separate cleanly.** Listener `prewarm` 85 µs against
  criterion 57.055 µs is **1.49×** — the same class as the device's +14 % over
  its fastest listener rows, and the only real context cost either platform
  shows.
- **No post-mint cost on either platform once regime is controlled.** On x86 the
  two arms' post-pre-warm handshakes are equal, with the repeat marginally
  *slower* (747 vs 760 µs). That is what the device shows at its fastest regime
  level, and it is why the device's mixed-regime +647 µs is not a first-sight
  cost.
- **The whole x86 increment converts to 648 µs** by the ~9× factor — inside the
  < 1 ms row. The device's pass-vs-pass 1.389 ms exceeds that prediction; its
  paired 0.728 ms does not.
- Absolute handshakes do **not** take the factor: 747 µs here against 1 479–1 815
  µs at the device's fastest level, ~2.2×. Consistent with PERFORMANCE.md
  §Converting — TLS legs do not convert.

Caveats: one session, n = 32, both arms degraded, Windows heap against musl +
mimalloc on the device, and a different governor. Scope is this box and this
session; it corroborates the device diagnosis and cannot stand in for it.

**Retraction.** The 5 Hz `cpu-frequency` poll read 350 MHz through C0's
first-sight pass and 700/1400 through C1; that was reported during the session
as corroboration. During C2 it read 350 for 9 of 13 samples while the paired
differences say the mint ran near full clock. A 5 Hz read of an instantaneous
value cannot see inside a ~2 ms connection, so the C0 reading is consistent
with the diagnosis and is **not evidence** for it. `measurement-traps.md`'s
rule stands unchanged: the reported frequency never scales, and after this run
it does not corroborate either. The diagnosis rests on timings only: C0 1.393,
C1 0.817, C2 0.728 ms.

**What is inferred and what is measured.** Measured: the arms sample different
speed regimes when run as sequential passes, and the difference disappears when
they are paired or when the device is kept busy. Inferred, not measured: that
the regimes are the CPU clock stepping under a light bursty workload, chosen by
the governor from the arms' order and duty cycle. There is no per-connection
clock trace, so the governor's mechanism stays an inference. Scope: RB5009 with
the live resolver co-resident, probe `a2d0802`, 240 synthetic hosts, one LAN
client, `--conc 1`; superseded only by a run with the clock held, which the
client cannot arrange.

**Dispositions.**

- **P5 remains FAIL at 1.389 ms** under the plan's frozen statistic,
  median(first-sight pass) − median(repeat pass) < 1 ms. The plan is not edited
  after a run and the statistic is not reinterpreted.
- **C2's 0.728 ms paired median is the like-for-like figure.** It shows the P5
  FAIL is dominated by the execution-regime difference between the two passes,
  not by `certs_mint`.
- **D11-on-device stays the row-setter** for PERFORMANCE.md's "cold `prewarm`
  per first-sight host" row at 450.88 µs; P5 is recorded beside it as a gate
  failure of the end-to-end measurement. **No code change follows.**
- **No further experiment after C2.** P5 is closed as a diagnostic.

Instruments, both diagnostic, beside `p5-conc-diag.mjs`: `p5-clock-load.mjs`
(known defects before any reuse: no error-kind logging, no stop on an error
burst, and its "load phase minted N leaves" line is misleading when a diag run
mints beside it by design) and `p5-paired-diag.mjs`.

## P6

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 19:35 | `a2d0802ddc55-dirty` | bobdenaut → probe `172.17.0.4`; aborted at the precondition, no certificate touched | FAIL — `chrome` running | INVALID (plan §Running item 3) | — | not kept — the run produced no figure |
| 2026-09-04 19:36 | `a2d0802ddc55-dirty` | RB5009 probe `fah-probe` 0.3.1, API `:8443` over the LAN; 5 × `ca/generate` then 5 × `import` of a self-signed RSA-2048 pair (CN `fah-probe-api`), one fresh TLS connection per call; `ca-archive` and `api-archive` both empty before the run | PASS | valid | 7 | `results-20260904T1936Z/` |

### Figures

| Op | n | Min | Median (`starttransfer − appconnect`) | Median `time_total` | Archive count before |
| --- | --- | --- | --- | --- | --- |
| ca/generate | 5 | 3.865 ms | **4.937 ms** | 8.366 ms | 0 |
| import | 5 | 4.048 ms | **6.015 ms** | 9.913 ms | 0 |
| gate: generate < 100 ms, import < 50 ms | — | **pass** — 4.937 ms and 6.015 ms; every call answered `200` | | | |

Per-call gate column: generate 7.476 / 3.912 / 3.865 / 4.937 / 4.951 ms, import
6.666 / 9.738 / 6.015 / 5.788 / 4.048 ms. Both clear by a wide margin — generate
sits at ~5 % of its allowance.

State the run changed, by design: the CA was replaced five times
(`BC:C9:D8:24…` → `61:F5:44:BE…`) and re-exported to
`results-20260904T1936Z/ca-after-p6.pem`, invalidating every earlier export and
purging the leaf cache; `api_certificate.source` became `imported` with
`restart_required`, so the acceptor keeps the old pair until the probe restarts
(Runbook 7's import-then-restart check, owner side). Archives ended at 5 and 5
against `MAX_ARCHIVES = 8`.

**Reads against P5.** A whole `ca/generate` — key pair, self-signature, archive
copy, staging, two renames and the JSON response — costs 4.937 ms on this
device. That makes P5's 1.389 ms incremental leaf mint consistent with it rather
than anomalous, and it says there is no general crypto or TLS problem here: the
budget miss is specific to per-leaf generation.

## P7-store

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 19:56 | `a2d0802ddc55-dirty` | RB5009 probe `fah-probe` 0.3.1, API `:8443` over the LAN; the CA in the store is the one P6 generated (`61:F5:44:BE…`), its private key copied off the router for the comparison and deleted afterwards; key is `ec prime256v1`, payload 184 base64 chars / 138 DER bytes | PASS | valid | — | `results-20260904T1956Z/` |

### Figures

| Check | Result |
| --- | --- |
| `ca/export?format=pem` — certificate blocks, key payload absent | **pass** — `200`, `application/x-pem-file`, blocks `["CERTIFICATE"]` only, `leaks: []` |
| `ca/export?format=der` — key payload absent | **pass** — `200`, `application/pkix-cert`, 383 bytes, first byte `0x30`, `leaks: []` |
| `/config` — key payload absent | **pass** — `200`, `leaks: []` (marker, base64 and DER forms all searched) |
| traversal list against `:8443` | **pass** — 20 paths × 2 (with and without bearer) = 40 requests: 24 served the SPA shell, **0 other 200s**, 16 rejected, `failing: []` |
| gate: every check pass | **pass** |

Script side only. The owner-side half of Runbook 7 — import-then-restart with
`openssl s_client` and `source == "imported"` after, `0600` on every private key
via an `sftp` listing, and the ninth generate answering `409 archive_full` with
the live fingerprint unchanged — has not been run.

## P8-probe

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 18:56 | `a2d0802` | RB5009, `/tool/profile cpu=all duration=5s`, both containers running and idle | — | valid (idle baseline) | 4 | pasted in this file only |
| 2026-09-04 18:58 | `a2d0802` | RB5009, `/tool/profile cpu=all duration=30s` during a throwaway UDP flood at the probe — `analytics.google.com`, blocked in-engine, ~47.7 k qps sustained, 64 queries in flight from bobdenaut | — | valid (naming verification) | 4 | pasted in this file only |

### Figures

| Read | Stage under load | `fastadhunter` share | Test process (name) share | Idle baseline |
| --- | --- | --- | --- | --- |
| 18:56, 5 s | none — both containers idle | absent (below the sampler's floor) | absent | this row *is* the baseline |
| 18:58, 30 s | ~47.7 k qps at the probe | absent | `fah-probe` **78.5 / 62.5 / 63 %** on cpu0 / cpu1 / cpu2 | the 18:56 read |
| diagnostic — no gate | naming rule satisfied: the renamed binary appears under its own name and is never summed with the live resolver | | | |

The whole box read 100 / 93 / 95 % on cpu0–2 under that flood, with `bridging`
6–17 %, `ethernet` 3–6 % and `firewall` 3–4 % beside `fah-probe`. This read
exists to prove the rename works before any CPU attribution — it is not a
throughput or capacity figure, and the load was a throwaway generator, not a
declared arm.

The stage that needed a `/tool/profile` share for its own sake — P1-loopback —
did not get one: that run lasted 5 s.

## P9-probe

### Runs

| Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Validity | Delta | Raw directory |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-04 19:19 | `a2d0802` | RB5009, container `fah-probe` on `veth3`, `mode=DnsHttpHttps`; boot from a warm `/data/lists` cache (no list download in the window); the probe's own list set, not production's | router idle | valid (diagnostic) | — | not kept — figures read off the container log, which is not tracked |

### Figures

| Run | `/container/start` → `API listening` (s) | Parsed rules |
| --- | --- | --- |
| 2026-09-04 19:19, boot from cache | 0.18 s from the first binary log line (19:19:58.729 → 19:19:58.912) | 63 110 compiled (`oisd-basic` 63 109 + 1 user rule) |
| diagnostic — no gate | against the `< 3 s hard` row; P9 proper is the soak deploy's container log on the 0.3.1 container (MA-6) | |

The container-level `*** start` line carries no sub-second stamp, so the
measured interval starts at the binary's first log line and excludes RouterOS's
own container start-up. That makes 0.18 s a lower bound on boot-to-serving, not
the whole of it.
