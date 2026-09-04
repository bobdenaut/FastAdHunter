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

`veth3` carries one container at a time, so `fah-probe` was stopped for both
in-device stages and restarted at 19:19:58. `fastadhunter` on `veth1` was never
stopped, reconfigured or profiled.

## Campaign status after the 2026-09-04 session

| ID | State | Gate |
| --- | --- | --- |
| SNI | run | **pass** |
| P1-loopback | run, CPU axis missing | no pick — every candidate that clears 0.9 × best is over the 32 MiB budget; owner decision owed |
| P1-LAN, P1-control | **parked** | no second LAN endpoint; the origin would sit on the driving host, which the plan excludes from the gate |
| P2 | **parked** | needs one host with two same-family LAN IPv4 addresses; bridged WSL was assessed and rejected — it would unblock P2 but leaves P1-control measuring a Hyper-V switch, and it risks this laptop's static DHCP lease, which two probe boot keys name |
| P3 (throughput, RSS) | **parked** | needs an h2 origin under a public name with a publicly trusted certificate on a second LAN endpoint (delta 14) |
| P4 | run | sets the row: **DoT +61 µs, DoH +915 µs** |
| P4-LAN | run | diagnostic: DoT +71 µs, DoH +427 µs — the DoH gap against P4 is owed an attribution |
| P5 | run | **fail at 1.389 ms against < 1 ms** — not attributable to minting (see D11-on-device); ~0.94 ms unexplained |
| D11-on-device | run | diagnostic: `certs_mint` **450.88 µs**, inside the < 1 ms row; ARM/x86 8.4× |
| P5-conc | run | diagnostic: incremental reproducible at ~1.29 ms (conc 1); 8× concurrency removes ~0.26 ms; ~0.6 ms unattributed |
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
LAN-observed incremental cost **fails** (1.389 ms against < 1 ms). P5-diag
segmented the difference from inside the probe and reconciled it to 3 %:
`dispatch_wait` +4.5 µs, `prewarm` +940.5 µs, `handshake_after_prewarm`
+647 µs. So the failure is located in the integration of listener → certificate
store → TLS handshake, **not** in the minting algorithm. `fah-certs` is not to
be changed on this evidence.

One measurement still owes an attribution: the P4 vs P4-LAN DoH gap, testable
with `/tool/profile cpu=all` during an in-device P4 run. Until it is run, **no
code change should follow from it.**

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

**Owner decision owed (plan §Choosing step 3, never taken inside the
procedure):** every candidate that clears the 0.9 × best bar exceeds the
declared 32 MiB budget at `max_connections = 1024`. Either raise the buffer
budget or change `max_connections` with its own justification — or keep 16/16.
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
| row: sets PERFORMANCE.md "DoT / DoH added latency vs UDP, p50" | | | | | **DoT +61 µs, DoH +915 µs** |

Diagnostics: p90 udp 190 µs, dot 279 µs, doh 1 341 µs; max udp 677 µs, dot
3 304 µs, doh 6 984 µs. DoT handshakes, excluded from the per-query figures,
were 2.089 / 1.987 / 1.953 ms. Per-round p50s were udp 140 / 101 / 101, dot
226 / 162 / 131, doh 1 033 / 980 / 1 022 µs — round 1 sits high on every
transport and rounds 2–3 settle; the pooled p50 is the declared statistic, so
this is recorded, not corrected. Delta 12 is satisfied: the harness asserted
`Some(HTTP/2.0)` on the DoH arm, so this column and P4-LAN's describe one
protocol.

DoH costs ~6 × DoT and ~10 × UDP on this device. There is no budget to fail —
P4 is the arm that sets the row — but +915 µs is the figure that row carries.

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
+915 µs that sets the PERFORMANCE.md row contains client cost a real DoH client
would not impose on the device. **Neither figure is adjusted here** — P4 is the
declared row-setter and both stand as measured. Testable by a `/tool/profile
cpu=all` read during an in-device P4 run, which would show the `fah-p4` share.

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

**Attribution, settled by the D11-on-device run below: the failure is not the
mint.** `certs_mint` measured **450.88 µs** in-process on the same device,
inside the < 1 ms budget, and at **8.4×** the dev box's 53.64 µs — the project's
~9× x86 → RB5009 factor, holding. So roughly **940 µs of P5's 1 389 µs is not
certificate minting** and remains unexplained by the mint benchmark.

The plan assumed the `spawn_blocking` hop "is paid by both arms and cancels".
The two figures argue against that: `certs_prewarm_warm` is **563 ns**, so the
repeat arm's pre-warm is effectively free, while the first-sight arm dispatches
a 451 µs blocking task and waits for a worker thread to pick it up. A
dispatch-and-wake asymmetry of that size sits in the listener's path, not in the
crypto. That is a hypothesis this run does not prove.

**Next step is segmentation of the P5 path, not optimisation of the mint.**
Nothing in `fah-certs` should change on this evidence — the component it points
at measures inside its budget. The first segmentation attempt is P5-conc below:
it bounds the concurrency-sensitive part at ~0.26 ms and leaves ~0.6 ms
unattributed.

An earlier revision of this section attributed the miss to the ~9× factor
failing for crypto workloads. The D11-on-device run refutes that; the paragraph
was replaced rather than kept, and this note records that it existed.

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

**Roughly 0.6 ms remains unattributed** and must not be split between scheduling
and CPU without instrumentation.

**Withdrawn:** an estimate of ~0.68 ms of CPU per first-sight handshake, derived
from the wall-clock difference between the two passes at conc 8. A′ refutes the
method — the same wall-clock difference reads 1.202 ms in A and 1.618 ms in A′,
a 35 % spread — and the raw throughputs moved 27–56 % between A and A′ (probe
uptime 31 s against 146 s). Differencing the medians cancels that drift; the
wall-clock comparison does not. No conclusion rests on it.

The next experiment is instrumenting the listener path — timestamps at request,
dispatch, worker start, mint completion, response. That is a code change and is
not made on this evidence.

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

Two facts sit beside each other and are not reconciled here: the same
`store.prewarm()` call measures **944.5 µs** inside the listener and **451 µs**
under criterion (§D11-on-device). Why the two contexts differ, and why the
post-pre-warm handshake differs between arms, are open questions. Neither is
answered by this run and neither is inferred from it.

**Verdict unchanged.** P5 remains **FAIL** at 1.389 ms against < 1 ms, and the
failure is **not** a `certs_mint` performance failure — D11-on-device passes
independently at 450.88 µs. What this run changes is where to look: the
integration of listener → certificate store → TLS handshake, not the minting
algorithm in isolation. **No change to `fah-certs` follows from this.**

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
