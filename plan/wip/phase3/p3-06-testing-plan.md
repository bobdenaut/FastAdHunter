# p3-06 — campaign 2: what must be measured, and the scripts that measure it

Declaration for the probe-side arms of `p3-06-phase3-verification-plan.md`
Step 4 items 2 and 7 — the review file's Runbook 5 and 7 — on the
**post-merge tip** of `phase3-06`.

**Campaign 1 is superseded in full — nothing carries.** Its figures were taken
on images built at `a2d0802`, which predates merge `e0c6071`: `fah-http`'s
`domain.rs` does not exist there and `runtime.http_runtimes` is not a config
key. Every HTTPS figure it produced measured an execution model that is no
longer shipped. Campaign 1's record stays where it is and is not edited:
`docs/code-review/phase3/p3-06-testing-results.md`, its `results-*/`
directories, and the frozen §Pre-declaration plus deltas 1–15 in
`p3-06-phase3-verification-review.md`. Campaign 2 restarts delta numbering at 1
and writes its figures to a new file,
`docs/code-review/phase3/p3-06-testing-results-2.md`.

The binding pre-declaration is this file plus the review file's campaign-2
§Pre-declaration. Every departure is a numbered §Declaration delta, recorded —
owner approval, `.md` edit — **before** the arm runs. A figure taken under an
unrecorded declaration is diagnostic only.

Gate thresholds — P2's 2 ×, P3's 50 MiB/s, P5's 1 ms, P6's 100 / 50 ms, the
≥ 90 % hit rate, RAM ≤ 128 MB, the 32 MiB buffer budget — are PERFORMANCE.md
**targets**, kept as targets. Nothing *measured* carries; the one campaign-1
number that was not a target, the P3 RSS ceiling, is re-derived in §The P3
ceiling.

## What changed under the campaign, and why nothing carries

| Change | Commit / source | Consequence for measurement |
| --- | --- | --- |
| HTTPS sessions moved onto allocation domains | merge `e0c6071`; `fah-http/src/{server,domain}.rs` | the acceptor hands each accepted socket to one of N `current_thread` runtimes on their own threads (`fah-http-<i>`). Hello peek, SNI verdict, splice and the whole intercepted session run there, not on the base runtime. Handshake latency, splice throughput, per-session RSS and prewarm cost are all N-dependent now |
| `runtime.http_runtimes` | `fah-config/src/schema/runtime.rs` | default `max(1, cores / 2)` — **2** on the RB5009, **16** on bobdenaut. `0` = the old shared-runtime path. Max 64. An unpinned N makes a figure unreadable |
| `HANDOFF_QUEUE = 32` per domain, shared by both lanes | `fah-http/src/server.rs:30` | one bounded channel per domain carries HTTP and HTTPS hand-offs together; a stalled lane can head-of-line-block the other acceptor (audit F3). Newly measurable |
| adaptive is the only upstream strategy | `fa9451a` (p2.6-12) | `strategy = "fallback"` — what the campaign-1 probe config carried — is rejected at load with `REMOVED_FALLBACK`. **The probe will not boot on the tip build until its config is fixed** |
| production moved to 0.3.3, N=2 | project-state 2026-09-07 | the soak comparator is 0.3.3, not 0.3.1. P8 / P9 proper re-baseline |
| `main` at `857865d` carries the domains | merge parents | the pre-phase-3 A/B checkout is now on the same execution model, so a dev-box A/B isolates Phase 3 rather than Phase 3 + the runtime change |

## Scope

Probe container `fah-probe` on `veth3` (`172.17.0.4`), running the `phase3-06`
tip build (hash recorded per run), driven from bobdenaut (`192.168.10.10`,
Windows 11) over the LAN. The **Mac** is the second wired LAN endpoint (§The
Mac endpoint). No router steering, no device tied to the probe, production on
`veth1` untouched. Every router write this plan needs — a probe restart after
boot keys, an env-list change, a bench container add / remove — is proposed to
the owner with the exact command, never run.

**Hard rule — no test process is named `fastadhunter`.** `/tool/profile
cpu=all` keys on process name and sums two containers running the same binary
(`docs/routeros-traps.md`); the live resolver and a test build must be
separable in every read. Every image uploaded for this task renames its binary
(§Scripts, naming table) — the FAH probe instance included. The binary never
reads its own name (no `current_exe` / `argv[0]` use), so the rename changes
nothing else.

## The N axis

`runtime.http_runtimes` is a boot key and an axis on every HTTPS figure. Two
rules:

1. **Every HTTPS figure records the N it ran under.** A figure with no N is
   diagnostic, whatever else it satisfies.
2. **Unless an arm declares otherwise, it runs at N = 2** — the RB5009's
   compiled-in default and production's pinned value. P10 is the only arm that
   sweeps N.

**Precondition, blocking, owner-run.** Config precedence is
`defaults < file < FAH__ env` (`fah-config/src/lib.rs:24`), and the probe is
attached to `envlists fah-env` — the same list that pins production's
`FAH__RUNTIME__HTTP_RUNTIMES=2`. While that list is attached, the probe's TOML
`runtime.http_runtimes` and any `POST /api/v1/config` value are dead. Editing
`fah-env` is not an option: it would change production's N at its next restart,
mid-0.3.3-soak. The probe therefore needs **its own env list** —
`fahprobe-env`, carrying the same `MIMALLOC_*` keys as `fah-env` (the allocator
band is P3's control) plus `FAH__RUNTIME__HTTP_RUNTIMES` — and
`/container/set` on `fah-probe` to attach it. Proposed to the owner before any
HTTPS arm runs; until it lands, every probe HTTPS figure is at N = 2 by
inheritance and P10 cannot run at all.

The pattern is not new: the phase-2.6 sweep ran its probe on envlist
**`h1buf-env`** — mimalloc keys plus `FAH__RUNTIME__HTTP_RUNTIMES` set to the
arm — for exactly this reason
([alloc-domains-n-sweep.md](../../../docs/code-review/phase2.6/alloc-domains-n-sweep.md)
§Rig). `fahprobe-env` is that list rebuilt for `fah-probe`, or `h1buf-env`
re-attached if its mimalloc keys still match `fah-env`'s. Owner's choice; the
value is read back from `/config` after every restart either way.

## Measurements

| # | Question | Row / budget (PERFORMANCE.md §Budgets) | Runs on | Needs |
| --- | --- | --- | --- | --- |
| SNI | a blocked domain closes at SNI, before any certificate, on the domain lane | gate — the everyone path of the definition of done | probe, LAN client, N = 2 | a domain the probe's lists block |
| P1-LAN | splice throughput on the deployment path — the shipped 16/16 build, then the sweep's pick | **relative only: median ≥ 0.9 × P1-control's median.** The absolute "≥ 100 MiB/s" row campaign 1 declared is **withdrawn as unmeasurable on this topology** — see §The 100 MiB/s row is withdrawn. Delta 1 | probe; bobdenaut as client, **Mac as origin**; 64 MiB, one connection, 5 runs, median + range; 8-connection aggregate arm (§Choosing `SPLICE_BUF`). **`p1-lan.mjs` sets the row, not `oha`** — the declared quantity is steady-state MiB/s excluding connect, ClientHello and teardown, while `oha`'s `sizePerSec` includes them. An `oha` pass runs beside it as a cross-check and is labelled as one | origin under a **publicly resolvable** name; `egress.allow_destinations`; the Mac endpoint |
| P1-control | what the same LAN path carries without the probe | **the ceiling P1-LAN is read against, and the only one there is.** No budget of its own | bobdenaut → Mac, direct, same origin process, same 5 runs, interleaved with P1-LAN | the Mac endpoint |
| P1-loopback | CPU per relayed byte vs buffer size — the `SPLICE_BUF` sweep | diagnostic; picks the buffer (§Choosing `SPLICE_BUF`). Reads the buffer sensitivity of the whole in-device loop — client, proxy and origin in one process — and stands for CPU per byte **only if the run is shown CPU-bound** (`/tool/profile` during it); the LAN aggregate arm's `/tool/profile` share is the confirmatory CPU reading | bench container, the `splicebench` example (`crates/fah-http/examples/splicebench.rs`, shipped `TlsServer` / `TlsProxy` relay via `TlsProxy::with_splice_buffers`) on in-device loopback, up / down sizes as runtime parameters, interleaved | **one** image; stdout to the container log, no mount |
| P2 | handshake cost: direct vs spliced vs intercepted | **two rows**: "SNI verdict + splice added latency" = spliced p50 − direct p50 (handshake and first-byte columns both recorded); "intercepted p50 ≤ 2 × spliced p50" on the handshake column | probe at N = 2, one fixed public origin (name, TLS version, ALPN recorded), 200 rounds, three arms interleaved per round, min / p50 / p99 | CA PEM on the client; **all three arms from one host** — the **Mac** (§The Mac endpoint item 1), carrying **two verified same-family LAN IPv4 addresses**, one listed and one not (§The Mac endpoint); never v4 vs v6, never one arm per machine, **no source-IP workaround on bobdenaut**. **Execution location: `p2-handshake.mjs` runs on the Mac**; bobdenaut only launches it over ssh and copies the results back — every socket of all three arms binds a Mac address, none originates on bobdenaut. Precondition the script proves before any round: both addresses on the Mac's wired interface, each observed by the probe (`/api/v1/clients` after one DNS query bound to each source), exactly one of them in `https.interception.clients` — else `INVALID`. Path proof per row is the served issuer (§Invalidity rules); `/telemetry` + `/certificates` before and after each arm are **supporting evidence only**: `minted_total` delta recorded, **not** required to be +1 — the origin's leaf may already be cached and no API reads per-host state; `blocked = 0` |
| P3 | intercepted h2 relay: throughput **and** per-session RSS under a 64-stream stall | ≥ 50 MiB/s (throughput arm). RSS: the reported quantity is the **session RSS delta** = max `process_rss` over the stall window − `process_rss` before the session, per run; gate statistic = the **max delta over 3 runs**, read against the ≈ 8 MiB download-stall ceiling (§The P3 ceiling). Raw `process_rss` is never called "per-session RSS". P3 is the **sole authority** for that ceiling — a passing soak says nothing about it | probe at N = 2, h2 origin on the Mac, 8 MiB per stream, 3 runs. **Warm-up first:** one small request to the same origin through the terminate leg, so the leaf mint and the first upstream connection are cached before "before" is read — otherwise they land in the delta. **Stall barrier:** the window opens only after all 64 streams are open **simultaneously**, each has received `:status 200` **and** a first DATA chunk, and the client has then stopped reading; `process_rss` sampled every second for the settle time, max taken; "after" follows the last close. **No-stall control — a separate matched run**, never the same session: its own warm-up, "before", window and "after"; same origin, same 64 streams **fully drained**, same sampling; 3 control runs interleaved with the 3 stall runs (S/C/S/C/S/C). The control delta is the allocator's band (mimalloc purge band alone is ± 6 MB — a single delta cannot resolve an 8 MiB question). **Report the raw stall delta and the control delta separately, never combined.** **Attribution:** `RESOLVED` when the smallest stall delta exceeds the largest control delta (disjoint ranges); otherwise **`UNRESOLVED`** — the stall delta sits inside the control / allocator band, is reported as such and is **not** read against the ceiling as a session cost. Throughput is a separate arm: one unstalled 8 MiB stream, 3 runs, median MiB/s, recorded with its ratio to P1-LAN's median from the same session (§The 100 MiB/s row is withdrawn) | listed client; **a publicly trusted name on the Mac origin** — the release probe verifies upstreams against `webpki-roots` only, so a local CA is refused with `UnknownIssuer`. The Mac endpoint does **not** remove this requirement; the barrier met, else no RSS figure; **nothing else enters the probe during the window** — `listeners.https.connections` delta = warm-up + 1 and the DNS query counters flat across it, read from `/telemetry` before / after |
| P4 | DoT / DoH added latency vs UDP, p50 — in-engine, reused connection | TBD — this arm sets the row | **in-device**: the `encrypted_latency` harness cross-compiled beside the release binary, 3 interleaved rounds × 2 000 per transport, handshakes excluded | a second image (test binary + release binary); owner container add / remove. DoH over **h2** (`reqwest` dev-dependency feature `http2`, every response asserted `HTTP/2.0`) |
| P4-LAN | the same quantity as the LAN sees it | diagnostic only — the LAN hop and one handshake per batch are in the figure, which is why it is not the row | probe, LAN client, blocked domain, 3 × 2 000, one connection per transport per batch | a domain the probe's lists block; the DoT hostname |
| P5 | cold `prewarm` per first-sight host (whole path incl. eviction scan) | < 1 ms | probe at N = 2, first-sight hosts **over DoT**, handshake only, no query. DoT runs on the **base** runtime (`dns::dot::run`), not on a domain — record that asymmetry with the figure: P5 measures the DoT listener's prewarm, p3-04's terminate-leg prewarm runs on a domain's blocking pool and is a different quantity. **State the script configures, not assumes:** CA generated on the probe; client trust irrelevant; host count **below** `LEAF_CACHE_CAPACITY` (512) minus what is already cached — 256 — so the repeat pass evicts nothing. Per host one first-sight and one repeat handshake; figure = median(first-sight) − median(repeat), an **incremental p50 estimate** of mint + insert, not a mint duration | host list (syntactically valid names, need not resolve); CA on the probe; `minted_total`, `unwarmed_misses`, `evictions` before / after; served issuer per row |
| D11 | `certs_mint` on the device — the cold mint path including the eviction scan | diagnostic beside P5 | bench container, criterion at **default** warm-up and measurement time, else the ARM figures stop being comparable with the recorded x86 ones | the bench image |
| P6 | CA generate / API-pair import | < 100 ms / < 50 ms on **`time_starttransfer − time_appconnect`** — client-observed request-processing time excluding the TLS handshake; `time_total` beside it as the handshake-inclusive diagnostic | probe API, 5 each, min / median | a real pair on disk for the import arm; archive headroom (§Environment) |
| P8-probe | CPU the probe burns under a stage's load | diagnostic, **undeclared until recorded**. P8 proper is the soak deploy against the 0.3.3 `dns+http` container, not the probe | **primary: the probe's own `cpu_user_ms + cpu_system_ms` from `/api/v1/debug/memory`, sampled every 2 s** — cores = delta ÷ run seconds (§P10 rig). `/tool/profile cpu=all` is the cross-check, not the source | for the cross-check only: the naming rule (§Scope), an idle baseline before every read, and a load sized to outlast the profile window |
| P9-probe | boot-to-serving with the phase-3 listeners and N domain threads | diagnostic. P9 proper is the soak deploy's container log against the 0.3.3 container; the probe's list set is its own, not production's — record its parsed count and its N beside the reading | `/log print where topics~"container"` | — |
| P10 | **does the N chosen for `dns+http` still hold with TLS loaded** — the ADR-0006 revisit trigger, discharged here | no budget; the arm **sets the N proposed for the `phase3-06` deploy**, under the owner's phase-2.6 criterion: *the smallest N that carries the tested workload, keeps HTTP p95 in bounds, does not degrade DNS, and keeps the locality win*. Owner decision on the measured trade, never a pre-emptive edit | probe, `http_runtimes ∈ {0, 1, 2, 4}`, container restarted between arms, runs inside an arm sharing one process. **The rig is the phase-2.6 one re-used, not a new one** (§P10 rig) | `fahprobe-env` attached (§The N axis); the phase-2.6 table as the like-for-like comparator |
| Runbook 7 | certificate-store checks | pass / fail, all mandatory | split below | — |

**Runbook 7 split.** Script side: no key material over the API — `ca/export` in
both formats and `/config` searched for the CA key's base64 payload, plus the
security suite's traversal list with `curl -sk` against `:8443`. Owner side,
propose only: import-then-restart (`/container/stop` + `start` on `fah-probe`
is a router write) with `openssl s_client` and `source == "imported"` after;
`0600` on every private key via `sftp` `ls -l` after first boot, import and
`ca/generate` ×2; the archive cap — the **ninth** generate answers `409`
`archive_full` with the live fingerprint unchanged.

**Device-only, no script can stand in:** Android CA trust and Private DNS
(Runbook 2, 3), the pinned-app check (Runbook 4), the ECH browser retry.

**Out of this plan, still required before the phase row flips:** Runbook 1
(dst-nat 443 v4 + the v6 decision), P7 — the 24 h full-mode soak on the
production container (RAM ≤ 128 MB steady; the minted-leaf hit-rate row ≥ 90 %
from the `leaf_cache` counters; watch items a–f), and P8 / P9 proper on the
soak deploy. **The full-mode soak cannot start before the 0.3.3 soak ends
2026-09-14** (project-state §Now) — it is the same container.

## The Mac endpoint

The Mac is the second wired LAN endpoint. It serves the P1 origin and the P3
h2 origin, and it is the single host that carries P2's two source addresses.
bobdenaut stays the driving host and the P1 client, so the Mac's TCP stack is
the origin side of **both** P1-LAN and P1-control and cancels between them.

Preconditions, verified and recorded in `run.log` before any arm that uses it:

| # | Precondition | Why |
| --- | --- | --- |
| 1 | **Wired Ethernet**, not Wi-Fi (a USB-C / Thunderbolt adapter counts). `ifconfig <iface>` shows `media: … baseT` and the link is up | Protects **P1-LAN / P1-control** — a relative gate needs a control whose spread is under the 10 % it decides; wired spread was ~2 % across the phase-2.6 sweep's arms, Wi-Fi's varies with the air — and **P3 throughput**, an absolute 50 MiB/s the air may not carry. P2's ratio gate and P3's RSS arm have no network term of that size: if wired is impossible they may run over Wi-Fi with the link type recorded as a declaration delta (an IP alias keeps one MAC, so campaign 1's bridged-VM trap does not apply). P2's added-latency row (spliced p50 − direct p50, sub-ms to a few ms) sits at Wi-Fi jitter's scale and may come back `UNRESOLVED` there |
| 2 | Node installed; `node --version` recorded. **`oha` 1.16.0 installed** (`cargo install oha --version 1.16.0`); `oha --version` recorded | every script is Node, no dependencies; the intercepted TLS connection-rate arm is `oha` run from this host (§Load generators) |
| 3 | Remote Login enabled, key-based ssh from bobdenaut | bobdenaut launches `p2-handshake.mjs` there and collects its results |
| 4 | On AC power, `caffeinate -dimsu` for the run's duration | a sleeping or thermally throttled laptop mid-throughput-arm produces a number with no control to cancel it |
| 5 | For P2: a **second IPv4 alias** on the wired interface — `sudo ifconfig <iface> alias <addr> 255.255.255.255`, outside the router's DHCP pool, recorded. Removed after the campaign | P2 needs two same-family addresses on one host. An alias on a Mac costs nothing and, unlike `New-NetIPAddress` on bobdenaut, risks no DHCP lease |
| 6 | For P1 / P3: the origin binds `:443`, which needs `sudo` on macOS | unlike Windows, macOS reserves ports < 1024. `sudo node p1-origin.mjs …`; the alternative — a `pf` redirect from 443 to a high port — adds a NAT hop to the measured path and is rejected |
| 7 | macOS Application Firewall: an explicit allow for the `node` binary, **or** the firewall off for the session, recorded either way | otherwise inbound is dropped silently and the arm fails with no local error — the same failure shape a `Public` network profile produces on bobdenaut |
| 8 | Inbound scoping uses **`192.168.10.1`**, never `172.17.0.4` | `/ip/firewall/nat` srcnat rule 1 is `action=masquerade src-address=172.17.0.0/24` with **no** `out-interface` restriction, so probe → LAN traffic arrives from the router's LAN address (review file §Pre-declaration delta 15, 2026-09-05). The reverse direction is not matched, so the probe still sees real client addresses and P2's identity precondition stands |

Darwin costs the scripts must absorb: `ip -4 addr` does not exist — the P2
identity precondition reads `ifconfig` on Darwin, `ip` on Linux, and prints
`INVALID` on Windows as before. Nothing else in the script set is platform-
specific.

**Still required and not solved by the Mac:** P3's origin needs a **publicly
trusted certificate under a public name** (Let's Encrypt DNS-01), because the
release probe verifies upstreams against `webpki-roots` only. A self-signed or
local-CA certificate on the Mac is refused with `UnknownIssuer`. Until that
name exists, P3 does not run.

## The P3 ceiling is ≈ 8 MiB, not 5.5

Campaign 1 read P3's stall delta against "≈ 5.5 MiB per stalled session,
CONFIGURATION.md". That figure was the flow-control ceiling of the **pre-fix**
relay — a 256 KiB h2 connection window — and the window was the bug: with 64
streams and a 64 KiB stream window, four stalled streams filled the connection
window and the origin could send nothing for the streams the client *was*
reading. That is the "P3 BLOCKED" state (control 64/64, stall 5/64), filed and
fixed on 2026-09-03 as **p3-04 S2**: `H2_CONNECTION_WINDOW = H2_MAX_STREAMS ×
H2_STREAM_WINDOW` (4 MiB), both legs, pinned by `fah-http/tests/interception.rs`
`sixty_four_stalled_h2_streams_all_receive_status_and_first_data_and_stay_open`
and reviewed in the review file §Post-review work E. Nothing is left to file.
Campaign 2's stall arm is that fix's on-device confirmation: a barrier not met
at any N is a **regression finding**, not a reproduction.

The ceiling CONFIGURATION.md `[https] max_connections` states now, per leg: a
4 MiB receive window (64 × 64 KiB) plus 64 × 64 KiB send buffers, h1 buffers
128 KiB. What a download stall — the client stops reading — can hold is the
upstream leg's receive window plus the client leg's send buffers: **≈ 8 MiB**.
With uploads stalled too, ≈ 12 MiB (review E1). P3's stall arm is a download
stall, so its gate reads against ≈ 8 MiB; `5.5` appears nowhere in campaign 2.

## The 100 MiB/s row is withdrawn

Campaign 1 declared P1-LAN's gate as "≥ 100 MiB/s steady state" and justified
it with "gigabit is 119 MiB/s". That is a link-speed argument, not a measured
ceiling, and the phase-2.6 sweep has since measured the actual one on this
device: **single stream 58–60 MiB/s, 8-parallel 67–70 MiB/s, at N = 2, 3
and 4** (the sweep's LAN arms; N = 0 had none —
[alloc-domains-n-sweep.md](../../../docs/code-review/phase2.6/alloc-domains-n-sweep.md)
§Rig, §Measurements). The cause is topology, not code — the router forwards
both flows, origin → probe and probe → client, over the same LAN port. The
origin in that run served 380 MB/s locally, so it was not the limit. The same
probe reached ~89 MiB/s from a WAN origin in that sweep: the ceiling belongs to
a LAN-side origin, and a WAN origin is already the far-side host that lifts
it — still short of 100 MiB/s.

The Mac does not change this: it sits on the same LAN side. A path that tops
out near 60 MiB/s cannot answer a 100 MiB/s gate, and a build that fails it
would be failing the router, not the splice loop. The phase-2.6 §Remaining
TODOs say the same thing — a line-rate origin needs a second LAN port or a
host on the far side of the router.

So: the absolute row is withdrawn, and P1-LAN is gated **relative to
P1-control** — the same client, the same origin, the same router path, without
the probe. That is the only ceiling this rig can produce, and it is the one
that actually isolates the splice loop. The absolute MiB/s is still recorded
as a diagnostic. Reinstating an absolute row needs a topology that can carry
it, and that is a separate owner decision.

Consequence for P3: its ≥ 50 MiB/s throughput gate stays as the PERFORMANCE.md
target — it sits under the 58–60 MiB/s single-stream ceiling with roughly 15 %
of headroom, and that ceiling moved by ~2 % across the sweep's arms. But 15 %
is thin for a relay the dev box measured at ~0.53 × the splice, so the gate
carries a second, co-equal column: **P3 median ÷ P1-LAN median**, same
session. A miss on the absolute with P1-LAN ≥ 0.9 × control is an interception
cost; a miss with P1-LAN under that is attributed to the path first.

## P10 rig

**Re-use, do not re-invent.** The phase-2.6 N sweep
([alloc-domains-n-sweep.md](../../../docs/code-review/phase2.6/alloc-domains-n-sweep.md))
already answered this question for `dns+http` on this device, with a rig that
took three attempts to get right. P10 is that rig with TLS arms added, so the
two tables are read side by side.

Inherited unchanged:

- **Arms.** One container restart per N; the runs inside an arm share one
  process. N = 0 is the control (shared runtime, the pre-domain path).
- **Statistics.** Requests/s; p50 / p95 / p99; **cores** = probe
  `cpu_user_ms + cpu_system_ms` delta over the run ÷ run seconds; ms per
  request; 502 count; **ΔRSS against the arm-local floor** (the arm's first
  `before` sample) — levels are never compared across arms.
- **Sampling.** `/api/v1/debug/memory` every 2 s (`process_rss`,
  `cpu_user_ms`, `cpu_system_ms`), plus a +3 min sample. CPU comes from the
  container's own counters, **not** `/tool/profile` — no process-name summing,
  no idle-baseline dance, and it survives the naming rule by construction.
- **Client discipline.** The client reads to the server's FIN before closing,
  so TIME_WAIT sits on the probe and not on bobdenaut (§Traps).
- **Mixed arm.** DNS 300 qps beside the HTTP load — 50 % cached, 20 % blocked,
  30 % uncached — so "does N degrade DNS" is answered, not assumed.
- **Shutdown arm.** `/container/stop` with transfers in flight; record the
  drain timeout, exit status and elapsed time.

Added for Phase 3, run at every N:

| Arm | Workload | Reads |
| --- | --- | --- |
| TLS connection rate, spliced | new TLS connection per request against a blocked-at-SNI name and an allowed name, 48 concurrent, 300 s | handshakes/s, p50 / p95 / p99, cores, ms/handshake, `listeners.https` counters |
| TLS connection rate, intercepted | the same from the **listed** client, terminate leg | the same, plus `leaf_cache` deltas |
| TLS keep-alive | 20 requests per intercepted session, last with `Connection: close` | requests/s, connections/s, cores, ms/req |
| Mixed with TLS | DNS 300 qps + spliced and intercepted connections together | DNS p50 / p95 / p99 under TLS load — the question ADR-0006 left open |
| Transfers | one spliced 900 MiB pass (5 × 100 MiB single, 5 × 8 × 10 MiB parallel), P1-control read beside it | MiB/s, cores per 900 MiB, ΔRSS step and what is still held at +15 min |

N = 1 is new to this campaign (phase 2.6 swept 0 / 2 / 3 / 4). It is included
because a TLS session costs far more CPU per connection than a plaintext one,
so the domain count that plateaus may sit lower, not higher. N = 3 is dropped
in exchange; if the {0, 1, 2, 4} curve puts the knee between 2 and 4, N = 3 is
run as a follow-up rather than pre-emptively.

Two client-side constraints, both from the discarded phase-2.6 rigs: the
origin must not be a toy server (a listen backlog of 5 produced 212 × 502 and
put the origin's own latency in the numbers), and the load client must not be
the one closing first (Windows port exhaustion caps a client-closes-first loop
near 130 connections/s, which would be reported as an N result).

## Load generators — `oha`, and what it cannot do

`oha` **1.16.0** drives the arms it can drive exactly; the specialised clients
stay where it cannot reproduce the declared quantity. Nothing below re-defines
a measurement to suit the tool — where `oha` would change the quantity, it is
not used, or it is used beside the row and labelled a cross-check.

The tool is already validated on this rig: phase 2.6 ran `oha` against
`connrate.py` in close mode and got 2 480 vs 2 405 rps with the probe at 3.6
cores (N not recorded for that check) — ~3 % apart, both far above the ~130 conn/s a client-closes-first loop
would have capped at, so `oha` satisfies the client discipline on this box by
evidence, not by assumption
([alloc-domains-n-sweep.md](../../../docs/code-review/phase2.6/alloc-domains-n-sweep.md)
§Rig).

**Pinned.** `oha 1.16.0`, installed on bobdenaut and on the Mac. The version is
recorded in `run.log` for every arm it drives; a different version is a
declaration delta, not a silent substitution. `--worker-threads` is **set
explicitly per arm and recorded** — the default is the physical core count (24
on bobdenaut), which would let the load client compete with the reading for the
box.

Arms `oha` drives, with the flags that make them the declared quantity:

| Arm | Invocation | Declared quantity preserved because |
| --- | --- | --- |
| close-mode HTTP | `--disable-keepalive -c 48 -z 300s -H "Host: <origin>" --rand-regex-url 'http://<probe>:8080/(1k\|10k\|50k).bin'` | one request per connection, body fully read, the size mix reproduced by the regex. `statusCodeDistribution` / `errorDistribution` carry what `connrate.py` reported as `status` / `errkind` |
| TLS connection rate, spliced | `--disable-keepalive --connect-to <name>:443:<probe>:8444 --cacert <ca.pem>`, from **bobdenaut** | `oha` cannot bind a source address and does not need to: bobdenaut is unlisted, so every connection of the run takes the splice leg |
| TLS connection rate, intercepted | the same, from **the Mac** | the Mac is listed, so every connection takes the terminate leg. The arm split is by host, never by flag |
| transfers | `-c 1 -n 5` single, `-c 8 -n 40` parallel | replaces `h1buf-ab.sh`'s curl loop at the same shape |

Statistics read from `--output-format json`: `rps` (with percentiles),
`latencyPercentiles`, `firstBytePercentiles`, `summary.sizePerSec`,
`statusCodeDistribution`, `errorDistribution`, `details.DNSDialup`.

Arms `oha` does **not** drive, and why — each of these keeps its own client:

| Arm | Blocker |
| --- | --- |
| keep-alive, 20 requests per connection | `oha` has no requests-per-connection control; it reuses one connection for the whole run, so `connections/s` stops existing. Phase 2.6 reports 165 conn/s at N=0, and that comparison is what P10 exists for. `p10-connrate.mjs` keeps this arm |
| the DNS half of the mixed arm; P4; P4-LAN | `oha` is HTTP-only. The cached / blocked / uncached mix and the 16-bit transaction-ID matching have no equivalent. `p10-dnsload.mjs` keeps these |
| P2 | needs three arms interleaved per round from one host on two **bound source addresses**, and a handshake **p50**. `oha` binds no source address, and reports `details.DNSDialup` as average / fastest / slowest only — no percentiles |
| P3 throughput arm | `summary.sizePerSec` includes connect, TLS handshake and teardown — the same exclusion that keeps `oha` off P1's row. `p3-h2stall.mjs --arm throughput` sets the row; an `oha --http2 -c 1 -p 1 -n 3` pass runs beside it, labelled a cross-check |
| P3 RSS stall arm | `oha` has no option to stop reading a response (`--help`, 1.16.0) — inferred, not documented, that it drains every body; the barrier requires 64 streams open, each with a first DATA chunk, and the client then not reading |
| SNI gate, P5 | raw TLS work — handshake-only, served issuer per row, records inspected before any certificate is served |
| P6, P7-store | five timed API calls on `time_starttransfer − time_appconnect`, and traversal probes with a needle search. Not load |

Two conditions on every `oha`-driven TLS arm:

1. **Path proof stays certificate-based.** `oha` reports no served issuer, and
   §Invalidity rules make provenance authoritative. Take one `openssl s_client`
   issuer sample immediately before and immediately after each arm, into
   `run.log`; the `listeners.https` and `leaf_cache` counters remain supporting
   evidence, as declared.
2. **`--connect-to` must preserve SNI.** The arm depends on the ClientHello
   carrying the public origin name while the socket goes to the probe's 8444.
   That behaviour is confirmed once in the smoke plan (Layer 0) before any
   `oha` arm carries a figure.
3. **No `--http2` on the rate arms.** `--disable-keepalive` is h1-only
   (`oha --help`), and `oha` is assumed to offer h1 alone without `--http2` —
   an assumption, not read from the binary. If a rate arm's `https` events
   show h2 on the terminate leg, the arm is a declaration delta, not a figure.

## Choosing `SPLICE_BUF` — 16 KiB or 64 KiB

Re-run from zero on the tip build: the splice loop now runs on a
`current_thread` domain runtime, so the buffer's CPU trade is measured on a
different scheduler than campaign 1's.

"16 or 64" is the wrong question. Campaign 1's device-loopback figures
(`a2d0802`, pre-domain — history, not a figure) sat 3–4 × above the 119 MiB/s
link and ~7 × above the measured 58–60 MiB/s LAN ceiling, so on this LAN any
size ties on the wire. What the buffer buys is **CPU per relayed byte** —
fewer read / write syscalls per TLS record — and what it costs is **memory per
session**. Two points cannot find the knee, and the two directions are not
symmetric: the splice is
`copy_bidirectional_with_sizes(client, upstream, SPLICE_BUF, SPLICE_BUF)`
(`fah-http/src/https.rs`), downloads fill upstream → client, requests barely
fill client → upstream. The sizes are runtime arguments to tokio already, so
the bench can sweep them; production keeps a const (or two).

Prediction to falsify: 32 KiB down captures most of the 16 → 64 gain (TLS
records are ≤ 16 KiB; a LAN receive queue holds 2–4 of them at line rate),
128 ≈ 64, and `up` does not matter for a download.

Procedure:

1. **Sweep on device loopback, one image** (P1-loopback). The bench takes `up`
   / `down` as parameters. Matrix: `up = 16 KiB`, `down ∈ {16, 32, 64, 128}
   KiB`, plus `64 / 64` as the symmetry control; steady state, 64 MiB, 5 reps
   each, arms interleaved A/B/A/B within one run. No container swaps, no
   rebuild per point. `/tool/profile cpu=all` is read during the run and the
   `fah-splicebench` share recorded: the arm counts as CPU-bound, and its MiB/s
   as a CPU-per-byte proxy, only if that share shows it; otherwise the sweep
   reports "throughput of the loop" and the CPU axis comes from step 4 alone.
2. **Pick the knee — among candidates inside the step 3 budget only.** Per
   candidate the statistic is the **median of its 5 repetitions**; **best** is
   the highest candidate median; the pick is the **smallest in-budget candidate
   whose median ≥ 0.9 × best**. Min–max per candidate is reported beside the
   median: where the pick's range overlaps best's, the difference is not
   established, which only strengthens the smaller pick. `up` stays 16 unless
   `64 / 64`'s median beats `16 / 64`'s by more than 10 %. Candidates outside
   the budget are still measured (the evidence for step 3's separate decision),
   never picked. Five repetitions per candidate.
3. **Memory budget, `max_connections = 1024` fixed.** `max_connections` is a
   product capacity property; it does not move during the sweep or the
   selection, and it is never lowered to make room for a buffer. The budget is
   `(up + down) × 1024`, worst case, declared by the owner **before** the sweep
   runs; until another figure is declared it is today's 32 MiB (`16 + 16`).
   Worst case per candidate at 1024:

   | `up + down` (KiB) | worst case |
   | --- | --- |
   | 16 + 16 | 32 MiB |
   | 16 + 32 | 48 MiB |
   | 16 + 64 | 80 MiB |
   | 64 + 64 | 128 MiB |
   | 16 + 128 | 144 MiB |

   A candidate above the budget is **rejected**, whatever its CPU gain. At the
   32 MiB budget only `16 / 16` fits — the sweep then yields evidence for a
   **separate owner decision** (raise the buffer budget, or change
   `max_connections` with its own justification), recorded as a decision item
   in the review file, never taken inside this procedure.
4. **Confirm on the LAN with the real binary** (P1-LAN): the shipped `16 / 16`
   first, then the pick if one passed step 3 — one extra image at most,
   container replace by the owner, P1-control re-read before and after so drift
   shows. Single connection must land in P1-control's range for both. The
   8-connection aggregate arm reads `/tool/profile` share for each build at the
   same aggregate rate (P8-probe, idle baseline first, naming rule §Scope):
   that CPU delta is the number that justifies a change, because wire speed
   will tie. A build that misses P1-control's range is a **finding** (veth hop,
   conntrack, domain hand-off, splice loop), attributed before any tuning.
5. The const changes only through its own gates plus a dev-box D6 / D7 rerun;
   each build gets one row in the review §Measurements with all three axes.

Rejected: an adaptive buffer (start at 16 KiB, grow on a full read). Hot-path
state and a branch per read for a gain the sweep prices first — principle 16.

## Scripts

**One script per measurement**, all under `docs/code-review/phase3/p3-06-probe/`,
Node, no dependencies. Shared code lives in `lib.mjs` only: config and flag
parsing (`--probe`, `--key`, `--out` on every script), the bearer API call,
percentiles, the `valid` / `INVALID` / `degraded` reporting and the results
directory. Each script owns its preconditions and its §Invalidity rule; none
knows about another, so a change to one measurement touches one file.

The campaign-1 script set is the starting point and is **re-verified against
the tip by the smoke plan before any device run** — it is not assumed correct
merely because it ran once. Changes campaign 2 requires are listed in the last
column.

| Script | Covers | Emits | Change for campaign 2 |
| --- | --- | --- | --- |
| `lib.mjs` | shared — nothing measured here | — | landed 2026-09-08: `Run.init` logs `runtime.http_runtimes`, refuses an HTTPS run when it cannot read it, and every result envelope carries `http_runtimes` |
| `p0-sni.mjs` | SNI gate | `sni.json` | none |
| `p1-lan.mjs` | P1-LAN; `--direct` runs P1-control against the same origin; `--connections 8` the aggregate arm | `p1.json` | none |
| `p1-origin.mjs` | the P1 origin as its own process, on the Mac | serves N MiB over TLS on `:443` | none — runs under `sudo` on Darwin |
| `p2-handshake.mjs` | P2, three arms interleaved — **runs on the Mac**, launched from bobdenaut over ssh | `p2.json`, copied back | identity precondition reads `ifconfig` on Darwin |
| `p3-h2stall.mjs` | P3, throughput and RSS | `p3.json` | none |
| `Dockerfile.p4` | P4, in-device | harness output in the container log | rebuild at the tip; DoH arm over h2 |
| `p4-lan.mjs` | P4-LAN | `p4-lan.json` | none |
| `p5-mint.mjs` | P5, first-sight and repeat arms over DoT | `p5.json` | none |
| `p6-certs-time.mjs` | P6, generate and import arms, both columns | `p6.json` | none |
| `p7-store.mjs` | Runbook 7 script side | `certs.json` | none |
| `p10-domains.mjs` | **new** — P10, one N: sequences the arms, invokes `oha` where §Load generators says so, samples `/api/v1/debug/memory` every 2 s, computes cores / ms-per-request / ΔRSS-against-arm-floor, records the `oha` version and `--worker-threads` | `p10-N<n>.json`, one `oha-<arm>.json` per `oha` run beside it | written 2026-09-08. `--n` is checked against `/config`; arms `close`, `keepalive`, `mixed`, `tls-spliced` (allowed then blocked name), `tls-intercepted` (this host, or the Mac via `--intercepted-ssh`), `mixed-tls`, `transfers`; served-issuer sample before and after every TLS arm, `--skip-issuer-sample` makes it a diagnostic |
| `oha` **1.16.0** | close-mode HTTP, both TLS connection-rate arms, the transfer arms; labelled cross-check passes beside P1-LAN and P3 throughput (§Load generators) | its `--output-format json`, captured into the run directory | **not a repo script — a pinned external dependency.** Installed on bobdenaut and the Mac; version and `--worker-threads` recorded per arm |
| `p10-connrate.mjs` | **new** — the keep-alive arm only (20 requests per connection, last with `Connection: close`), plaintext and TLS | `p10-connrate.json` standalone; `runConnrate` when imported by `p10-domains.mjs` | written 2026-09-08, ported from `E:/fah-diag/tools/connrate.py`. `worker_threads` share the connection loops; the `--tls` half records the served issuer as `leg` (spliced / intercepted). Narrower than campaign 2's first draft: `oha` took close mode and the TLS rate arms, and this is the one HTTP shape it cannot express |
| `p10-dnsload.mjs` | **new** — the 300 qps DNS mix (50 % cached, 20 % blocked, 30 % uncached), transaction-ID matched | `p10-dnsload.json` standalone; `DnsLoad` when imported by `p10-domains.mjs`, which warms it and starts it inside the arm's window | written 2026-09-08, ported from `E:/fah-diag/tools/dnsload.py` in full. No `oha` equivalent exists |
| `Dockerfile.splicebench` | P1-loopback, the whole sweep | per-candidate rows and the pick line in the container log | rebuild at the tip |
| `Dockerfile.certs` | D11-on-device, `certs_mint` at criterion defaults | criterion output in the container log | rebuild at the tip |
| `Dockerfile.fahprobe` | the probe FAH instance every LAN arm drives; the `SPLICE_BUF` pick is a second build of it | image whose process is `fah-probe` | rebuild at the tip |
| `smoke/h2-origin.mjs`, `smoke/h2-preflight.mjs` | the P3 origin and its preflight, on the Mac | — | none — run under `sudo` on Darwin |
| `p5-clock-load.mjs`, `p5-conc-diag.mjs`, `p5-paired-diag.mjs`, `Dockerfile.bench` | campaign-1 P5 diagnostics and the D5–D9 criterion image — **history**, not in campaign 2's arm set | — | not run, not smoked; kept with the campaign-1 record |

Every script writes `<name>.json` with `valid`, appends `raw.jsonl` and
`run.log`, and snapshots the probe's `/config` once per run directory.

**Binary names on the router** (the hard rule in §Scope). Linux `comm`
truncates at 15 characters, so every name is ≤ 15; the rename is a `COPY …
/<name>` plus `ENTRYPOINT` in the Dockerfile, nothing in the code.

| Image | Process name | Never |
| --- | --- | --- |
| `Dockerfile.fahprobe` (probe instance, both buffer builds) | `fah-probe` | `fastadhunter` |
| `Dockerfile.splicebench` | `fah-splicebench` | the criterion hash name |
| `Dockerfile.certs` | `fah-certs` | the criterion hash name |
| `Dockerfile.p4` — harness plus the binary it spawns | `fah-p4` spawning `/fah-probe` (`FAH_E2E_BINARY=/fah-probe`) | `fastadhunter` |

Verification before the first `/tool/profile` read of a session:
`/container/print detail` shows the test container's `cmd` / entrypoint under
its own name, and `/tool/profile cpu=all` lists `fastadhunter` **and** the test
name as separate rows while both run. The live resolver's row is the idle
baseline for every attribution.

Every in-device image follows the `Dockerfile.probe` pattern — single layer,
legacy docker-archive, never OCI layout (`docs/routeros-traps.md`).

Results: raw output stays in the `results-<ts>/` directory under
`p3-06-probe/`, committed with the run. **Figures are recorded in
`docs/code-review/phase3/p3-06-testing-results-2.md`** (root CLAUDE.md rule 19:
measurements live under `docs/code-review/`). That file has **one section per
measurement ID, two tables each**:

- **Runs** — one row per run: date, tip hash, **N**, device / workload /
  corpus, idle check, validity / attribution, the declaration delta number if
  the run followed one, raw directory.
- **Figures** — shaped for the measurement (arms × min / p50 / p99, candidates
  × median / min / max, runs × stall delta / control delta, N × the P10
  readings), the **gate line as its last row**. Diagnostics go in this table,
  never in prose.

An `INVALID` or `degraded` run gets a Runs row and no Figures row. The review
file §Measurements links to the section. **This plan carries no results and is
not edited after a run.** A figure that is in neither place is not a result.

No script may print a figure it did not verify it could produce — see
§Invalidity rules.

## Invalidity rules

Every script checks its own preconditions and prints `INVALID` with the reason
instead of a number when they fail. Each rule exists because its absence
produced a wrong figure.

| Measurement | Its script must verify before reporting |
| --- | --- |
| all HTTPS arms | `runtime.http_runtimes` was read from the probe and recorded; a figure with no N is not printed as a figure |
| SNI | (a) the allowed name reaches ServerHello through the probe, else `INVALID` — a listener that closes everything proves nothing about blocked rows; (b) `listeners.https.blocked` moves by at least the blocked-attempt count (attempts × 2 when `https.sni.no_sni = "block"`, × 1 when `"pass"`), else `INVALID` — a close caused by a resolve failure is not an SNI verdict; (c) the gate boolean covers the blocked and the no-SNI attempts: every one `closed_silent` or `alert`, none reaching ServerHello |
| P1 | a spliced sample completed; the P1-control (`--direct`) median exists in the same interleaved session, else `INVALID: no P1-control median in this session`; a client-and-origin-on-one-host arm, if run, is labelled `loopback_origin` and is excluded from the gate line |
| P1-loopback | the `/tool/profile` share during the run is recorded; without it, or with a share that does not show the loop CPU-bound, the figure is labelled "throughput of the loop", never CPU per byte |
| P2 | the identity precondition: two same-family IPv4 addresses on one interface of the Mac (wired by §The Mac endpoint item 1, or Wi-Fi under a recorded delta), both observed by the probe, exactly one listed — else `INVALID` before the first round. Then **both**, per row: every spliced row's served issuer ≠ FastAdHunter CA **and** every intercepted row's served issuer = FastAdHunter CA. Either condition alone proves nothing about the other arm. Certificate provenance is authoritative; counters are supporting — `minted_total` is recorded, never asserted. `requests > connections` is **not** a rule — with one request per fresh connection it is false by construction on both legs |
| P3 | the warm-up ran before "before"; the stall barrier was met — all 64 open at once, each with `:status 200` and a first DATA chunk — before the window opened; the window was exclusive (`listeners.https.connections` delta = warm-up + 1, DNS counters flat); the 3 matched no-stall control runs ran, interleaved with the stall runs, each with its own warm-up / before / window / after. Any of these missing ⇒ no RSS figure. Stall and control deltas printed separately; the attribution label (`RESOLVED` / `UNRESOLVED`) printed with them |
| P4 / P4-LAN | the figure is labelled by which of the two it is; the DoH arm asserted `HTTP/2.0` on every response; replies matched by 16-bit transaction ID; DoT reassembled by its 2-byte length prefix; unanswered and unmatched counts reported |
| P5 | `minted_total` moved by exactly the host count; `evictions` delta 0 across the repeat pass; every row's served issuer = FastAdHunter CA (the fallback certificate on any row ⇒ no CA in the store ⇒ `INVALID`); the repeat arm ran; `unwarmed_misses` delta reported |
| P6 | every generate and import returned `200`; the archive count read first |
| D11 | criterion ran at default warm-up and measurement time (the ARM / x86 comparison needs it); `workdir` writable; the bench container ran alone on `veth3` |
| P8-probe / P9-probe | the naming rule held — `/tool/profile` lists `fastadhunter` and the test name as separate rows; an idle baseline was read before every profile; P9 records the probe's parsed list count and its N beside the reading |
| Runbook 7 | the CA key's base64 payload was read from the store before the search; every `ca/export` format and `/config` answered without it; every traversal row answered without the needle — a `200` on a sensitive path is logged, never counted as a pass by status |
| P10 | the probe's `runtime.http_runtimes` read back after the restart **equals the intended N** — the env-list precondition silently pins 2 otherwise, and four identical rows would be reported as a plateau; the idle baseline was read before each `/tool/profile`; the workload outlasted the profile window |
| any `oha`-driven arm | `oha --version` is **1.16.0** and is recorded, `--worker-threads` was passed explicitly and is recorded, and — on a TLS arm — the `openssl s_client` issuer sample was taken before and after and matches the leg the arm claims. A missing version, an unset `--worker-threads` or a missing issuer sample ⇒ the arm is a diagnostic, not a figure |
| all | origin failure rate within budget — **2 %** of a stage's samples (`--max-fail-pct` default 2); zero completed samples ⇒ `INVALID`; at or under 2 % ⇒ `valid`, figures from the completed samples; above ⇒ `degraded`, not a gate. P2's issuer rules stay `INVALID` conditions, outside this budget. Tip hash, N and idle baseline recorded |

## Gate statistic vs diagnostics

One statistic per measurement decides; everything else is reported beside it
and decides nothing. A script prints the gate line from the gate statistic
only.

| Measurement | Gate statistic (authoritative) | Diagnostics (reported, never gate) |
| --- | --- | --- |
| SNI | every attempt closed before any certificate — boolean | close latency |
| P1-LAN | median of 5 runs ≥ **0.9 × P1-control's median**, both measured in the same interleaved session | absolute MiB/s, min / max, aggregate-arm MiB/s, CPU share |
| P1-control | none — it is the ceiling | min / p50 / max |
| P1-loopback | none — selection statistic is the per-candidate median (§Choosing step 2) | min / max, `/tool/profile` share |
| P2 | handshake column: intercepted **p50** ≤ 2 × spliced **p50**; row value: spliced p50 − direct p50 (handshake and first-byte); p50 = median over 200 rounds | min, p99, per-round raw, first-byte ratio, counters |
| P3 throughput | median of 3 runs ≥ 50 MiB/s (PERFORMANCE.md target), **read with** P3 median ÷ P1-LAN median from the same session — a miss with P1-LAN ≥ 0.9 × control is an interception cost, a miss with P1-LAN under it is attributed to the path first | per-run MiB/s, the `oha` cross-check |
| P3 RSS | **max over 3 runs** of the stall delta, read against the ≈ 8 MiB download-stall ceiling (§The P3 ceiling) **only when attribution is `RESOLVED`**; above it is a finding, not a fail. `UNRESOLVED` ⇒ both deltas reported, no reading against the ceiling | per-run stall delta, per-run control delta, the per-second series |
| P4 | per transport p50 − UDP p50 (sets the row) | p99, per-round, handshake time |
| P4-LAN | none — diagnostic | p50 / p99, batch wall-clock mean |
| P5 | median(first-sight) − median(repeat) < 1 ms | per-host raw, D11-on-device, `unwarmed_misses` / `evictions` deltas |
| D11 | none — diagnostic beside P5 | criterion point estimate and interval, ARM/x86 factor |
| P6 | median of 5 on `time_starttransfer − time_appconnect`: generate < 100 ms, import < 50 ms | min, `time_total` |
| P8-probe, P9-probe | none — diagnostic | as declared |
| P10 | none — the arm proposes an N, the owner decides | per N: handshakes/s, p50 / p99, CPU share, `process_rss`, counters |
| Runbook 7 | each check pass / fail | — |

## Environment the scripts assume

Probe config, set once via `POST /api/v1/config`. All are **boot** keys
(`restart_required: true`): set them, then one restart — `/container/stop` +
`start` on `fah-probe`, a router write the owner runs.

| Key | Value | Why |
| --- | --- | --- |
| `engine.mode` | `dns+http+https` | default is `dns`; no HTTPS or DoT listener otherwise |
| `egress.allow_destinations` | the Mac's LAN address | empty refuses every private destination, judged on the **resolved** address |
| `https.interception.clients` | the listed client only | anything not listed is spliced |
| `dns.upstreams.strategy` | **absent, or `adaptive`** | `"fallback"` is rejected at load since `fa9451a`. The campaign-1 probe config carries it — **the probe will not boot until it is removed** |
| `runtime.http_runtimes` | pinned per arm, 2 unless the arm sweeps it | see §The N axis — this key is dead while `fah-env` is attached |

Plus: a CA generated on the probe and exported to the driving host — and
**re-exported after every P6 run**, since each generate replaces it; a publicly
resolvable name for the P1 origin on the Mac (`<dashed-ip>.nip.io` works);
a publicly **trusted** name for the P3 origin; `ca-archive/` **empty** before
P6, so that P6's five generates plus four more reach the ninth for the
archive-cap check (`fah_certs::MAX_ARCHIVES` = 8; `api-archive/` likewise
before the import arm).

## Running the campaign — the agent drives

Every script except `p2-handshake.mjs` runs on bobdenaut from the repo
checkout, invoked by the agent, with `--out
docs/code-review/phase3/p3-06-probe/results-<ts>`. **`p2-handshake.mjs` runs on
the Mac**: bobdenaut launches it over ssh, and its results directory is copied
back into the run's results directory afterwards (`scp` — root CLAUDE.md rule
18, ask first). The origin processes (`p1-origin.mjs`, `smoke/h2-origin.mjs`)
run on the Mac under `sudo`.

| Action | Who |
| --- | --- |
| run a script, read probe API / router read-only prints, write results under `p3-06-probe/` | agent |
| launch a script on the Mac over ssh; copy its results back | agent (the copy after a yes — `scp`) |
| start / stop an origin on the Mac under `sudo`; add the IPv4 alias; allow `node` in the Application Firewall | owner, or the agent after an explicit yes per command |
| add / remove a local firewall rule on bobdenaut (elevated PowerShell) | agent **after an explicit yes** per rule, or owner |
| restart the probe, attach `fahprobe-env`, add / remove a bench container, `scp` an image | owner — agent proposes the exact command (root CLAUDE.md: router off limits; rule 18 for `scp`) |
| commit results | owner's yes per changeset |

**Before every script**, recorded in `run.log`:

1. `GET /health` on the probe answers `200`; `engine.mode` from `/config`
   contains `https` for the HTTPS and DoT arms; `runtime.http_runtimes` read
   and recorded.
2. `Get-NetConnectionProfile` — bobdenaut's LAN adapter is `Private`. On
   `Public`, Windows drops unsolicited inbound silently and the first-run
   `node.exe` "Windows Security Alert" is suppressed, so an origin arm fails
   with no local error.
3. `tasklist` shows no browser or video player on bobdenaut
   (measurement-traps: a tip-only figure on a non-idle box has no control to
   cancel the load). Same check on the Mac before an arm that runs there.
4. Before an origin starts on the Mac: `lsof -nP -iTCP:443 -sTCP:LISTEN` is
   empty — an existing listener makes the arm `INVALID` with `EADDRINUSE`, not
   a wrong number.
5. Before P2, on the Mac: `ifconfig` shows both LAN addresses on the wired
   interface; nothing else runs on the Mac.
6. Before any `oha`-driven arm, on the host that runs it: `oha --version`
   reads **1.16.0**, and the `--worker-threads` value the arm will pass is
   written to `run.log` before the arm starts, not after.

**Local firewall.** Inbound only where a host *accepts* a connection; every
other arm is outbound and needs no rule.

| Measurement | Host accepts from | Rule |
| --- | --- | --- |
| P1-LAN, P3 — origin on the Mac | the probe, arriving as **`192.168.10.1`** (masquerade, §The Mac endpoint item 8), TCP `443` | Mac Application Firewall allow for `node`, scoped where the tool allows it |
| P1-control — origin on the Mac | bobdenaut, TCP `443` | the same allow covers it |
| SNI, P2, P4-LAN, P5, P6, Runbook 7 | nothing | none |

On bobdenaut, never `New-NetIPAddress` / `netsh … add address` (§Traps), and
never answer the "Windows Security Alert" dialog with *Allow*: it writes a
permanent, unscoped `node.exe` rule for every port.

## Traps

Campaign 1 paid for every row here except the last three.

| Trap | Effect | Avoidance |
| --- | --- | --- |
| The probe inherits `fah-env`, which pins `FAH__RUNTIME__HTTP_RUNTIMES=2` | env beats file and API, so a P10 sweep silently measures N = 2 four times and reports a plateau that is an artefact | `fahprobe-env` attached first (§The N axis); P10's invalidity rule reads N back after every restart |
| Editing `fah-env` to move the probe's N | changes production's N at its next restart, mid-soak | never; the probe gets its own list |
| A probe config carrying `strategy = "fallback"` | the tip build refuses to load it; the container restarts into a boot error, not a working probe | §Environment; the smoke plan exercises the refusal deliberately |
| Probe and production both run a binary named `fastadhunter` | `/tool/profile` sums them; a 84.5 % core reading was attributed to the probe and was not | hard rule in §Scope; idle baseline before every attribution |
| A listed client has no spliced path | the "spliced" arm silently measures the terminate leg | separate the arms by client address and check the issuer |
| Spliced and intercepted arms from two different machines | the ratio compares two TLS stacks, not two legs | all three arms from the Mac, two verified same-family IPv4 addresses. Never v4 vs v6: the probe listens on v4 only and interception matches the client address, so the "arm" would change the path |
| A Wi-Fi link on the second endpoint | no stable band to read the P1 gate against, and P3's absolute gate may sit above what the air carries; a bridged VM additionally loses frames carrying a second MAC | wired Ethernet for P1 and P3 throughput, verified in `run.log`; P2 and the P3 RSS arm over Wi-Fi only under a recorded delta (§The Mac endpoint item 1) |
| `New-NetIPAddress` / `netsh … add address` for a second source IP | drops bobdenaut's DHCP lease and default route | do not; the alias goes on the Mac |
| The proxy resolves upstream names through the container's stub resolver, not its own engine | a `$dnsrewrite` rule never applies to the splice path; `.test` names give `resolve_failures` | use a publicly resolvable name |
| `dnsrewrite` to an IPv4 answers AAAA with `::` | upstream connect fails instantly | v6 rewrite as well, or a name that resolves for real |
| `/tool/fetch` as a throughput control | 20 s to `disk1`, 1 s to `kingston`; measures the write target, 1 s resolution | not a control |
| A figure printed before its stage checked preconditions | two P3 "results" (4.0 MiB, then 0.0 MiB for 64 "stalled" streams) were quoted before any stream was shown to carry a byte | §Invalidity rules; a stage prints `INVALID`, never a number it cannot stand behind |
| `chrome.exe` running on the driving host | every stage prints `INVALID` at the idle precondition; `--allow-busy` only yields `degraded`, which answers no gate. Cost four aborted runs in campaign 1 | close it before starting a stage |
| Git Bash path conversion on bobdenaut | any command with a bare `/path`, a `docker -v` mount or an `openssl -subj` is mangled | `MSYS2_ARG_CONV_EXCL='*'` |
| A client calling `sock.destroy()` on `secureConnect` | the RST arrives before the server finishes `into_stream`, so the DoT listener takes its handshake-failed arm and never reaches code after it | close with `sock.end()` |
| Results left in a `results-*` directory on the laptop | campaign 1's figures exist nowhere the review can cite | commit the directory with the run; §Measurements cites it |
| Criterion needs a writable cwd | bench aborts before printing | `workdir=/data` with a mount |
| Stage runs longer than the profile window | `/tool/profile` samples an idle box | size the load to outlast the profile |
| A load client that closes the connection first | bobdenaut's 16 384 dynamic ports at 120 s TIME_WAIT cap the loop near **130 connections/s**; the cap is then reported as an N or a throughput result. Cost the phase-2.6 sweep its first rig — 250 k failed connects | the client reads to the server's FIN before closing, so TIME_WAIT sits on the probe (Linux) |
| A toy origin | phase-2.6 rig 2: `ThreadingHTTPServer`'s listen backlog of 5 refused the probe's upstream connects (212 × 502) and put the origin's own per-request latency in the numbers | a real server; the origin's local throughput measured and recorded before it is used |
| `oha` at its default `--worker-threads` | the default is the physical core count (24 on bobdenaut); the load client then competes with whatever else the box is doing, and the arm reads the client, not the probe | pass it explicitly per arm and record it (§Load generators) |
| `oha`'s keep-alive read as connrate's | `oha` reuses one connection for the whole run; connrate's keep-alive arm closes every 20 requests and reports `connections/s`. Swapping them silently would break the comparison against the phase-2.6 table | the keep-alive arm keeps `p10-connrate.mjs`; §Load generators lists what `oha` may and may not drive |
| `oha`'s `sizePerSec` read as steady-state throughput | it includes connect, ClientHello and teardown; P1's and P3's declared quantities exclude them | `p1-lan.mjs` and `p3-h2stall.mjs` set those rows; the `oha` pass is labelled a cross-check |
| An `oha` TLS arm with no issuer sample | `oha` reports no served issuer, so a listed-vs-unlisted mix-up would go unnoticed and the "spliced" arm could be the terminate leg | `openssl s_client` before and after each arm, into `run.log` (§Load generators) |
| Reading a LAN throughput figure as a link-capacity figure | the router forwards origin → probe and probe → client over the same LAN port; **par8 caps at 67–70 MiB/s, single stream at 58–60**, at N = 2 / 3 / 4 (the sweep's LAN arms). Campaign 1's 100 MiB/s gate was set against 119 MiB/s of link speed and is unreachable here | §The 100 MiB/s row is withdrawn: gate relative to P1-control, absolute recorded as diagnostic |
| A Mac asleep or thermally throttled mid-arm | a throughput or latency figure with no control to cancel it | AC power, `caffeinate -dimsu`, recorded |
| macOS reserves ports < 1024 | the origin fails to bind `:443` and the arm reports a connect error, not a bind error | `sudo`; a `pf` redirect is rejected — it adds a NAT hop to the measured path |

## Declaration deltas

Each delta is recorded in the review file's campaign-2 §Pre-declaration
"Declaration changes" **before** the arm runs; the original block stays
unedited. Two are already owed, both from evidence that arrived after campaign
1 declared its numbers.

1. **P1-LAN's absolute gate withdrawn (owner decision needed).** Declared in
   campaign 1: "≥ 100 MiB/s steady state", justified by gigabit link speed.
   Now: gated **relative to P1-control** (median ≥ 0.9 × control median), with
   the absolute MiB/s recorded as a diagnostic. Reason: the phase-2.6 sweep
   measured this device's actual LAN-through-router ceiling at 58–60 MiB/s
   single stream and 67–70 par8 at N = 2 / 3 / 4, with a 380 MB/s origin (and
   ~89 MiB/s from a WAN origin on the same probe) — so the
   declared gate is unreachable by topology and a build that missed it would
   be failing the router. §The 100 MiB/s row is withdrawn. Reinstating an
   absolute row needs a host on the far side of the router; separate decision.
2. **P10 inherits the phase-2.6 rig rather than declaring a new one.**
   Campaign 1 had no N arm at all. P10's arms, statistics (cores from the
   container's own CPU counters, ΔRSS against the arm-local floor), sampling
   cadence and client discipline are
   [alloc-domains-n-sweep.md](../../../docs/code-review/phase2.6/alloc-domains-n-sweep.md)'s,
   so the two tables are comparable; only the TLS arms and N = 1 are new, and
   N = 3 is deferred rather than dropped. §P10 rig.
3. **`oha` 1.16.0 adopted for the arms it reproduces exactly.** Campaign 2's
   first draft ported both phase-2.6 Python tools to Node. Now: `oha` drives
   close-mode HTTP, both TLS connection-rate arms and the transfer arms, and
   runs a labelled cross-check beside P1-LAN and P3 throughput;
   `p10-connrate.mjs` shrinks to the keep-alive arm alone and
   `p10-dnsload.mjs` is ported in full, because `oha` cannot express
   requests-per-connection or DNS. **No declared quantity changes** — where
   `oha` would change one (P1's and P3's steady-state exclusion of setup, P2's bound
   source addresses and handshake p50, P3's stall barrier, P5/SNI's raw TLS
   work, P6's curl phase timings) it is not used, or it runs beside the row
   labelled a cross-check. Version and `--worker-threads` are pinned and
   recorded; `--connect-to`'s SNI behaviour is a smoke prerequisite.
   §Load generators.

## Campaign 1 — carried as history, not as figures

Not results for the tip build. Listed only so a reader knows what exists and
where.

| Item | Campaign 1 state | Why it does not carry |
| --- | --- | --- |
| SNI, P1-loopback, P4, P4-LAN, P5, P5-conc, P5-diag, P6, P7-store, P8-probe, P9-probe, D11 | run at `a2d0802` | pre-merge execution model; P6 / D11 / P7-store do not touch the HTTPS listener, but the owner chose a full re-run from zero rather than a survival argument per arm |
| P1-LAN, P1-control, P2, P3 | parked | no second LAN endpoint, no two-address host, no publicly trusted h2 origin. The Mac solves the first two; the third is still outstanding |
| `SPLICE_BUF` | provisional 16 / 16, budget 32 MiB | the sweep is re-run on the domain-lane scheduler |
| P3 BLOCKED | one h2 stream of 64 answered through the terminate leg; dev-box reproduction (smoke F7: control 64/64, stall 5/64) | **filed and fixed as p3-04 S2** on 2026-09-03 (256 KiB → 4 MiB h2 connection window, `interception.rs` stall tests, review §Post-review work E). Not owed. Campaign 2's stall arm confirms the fix on the device — §The P3 ceiling |
