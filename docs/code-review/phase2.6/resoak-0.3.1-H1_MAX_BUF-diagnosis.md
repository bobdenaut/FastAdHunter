# resoak 0.3.1 — RSS step traced to the plain-HTTP pass-through path

Device: RB5009, production container `fastadhunter` 0.3.1, uptime 5 d 2 h.
Workload: live household traffic, not synthetic. Corpus: 753 219 compiled rules
from the 16 configured lists. Captured 2026-09-06 13:12–13:35 local (UTC+3) via
`GET /api/v1/telemetry` at 60 s and the `/api/v1/events` WebSocket.

## Summary

RSS on the live resolver reads between 73 MiB idle and 109 MiB under load. The
whole swing is `residual_bytes`; `ruleset_bytes`, `cache_estimated_bytes` and the
stats figures do not move. The driver is the plain-HTTP pass-through proxy, not
DNS and not the Rule Engine. `Proxy` builds both hyper HTTP/1 endpoints without
a buffer cap, so each falls back to hyper's 408 KiB default: the upstream leg's
read buffer grows adaptively under fast transfers, and the client-facing leg
queues up to that many response bytes ahead of the socket. `Interception`
already caps both of its endpoints at 128 KiB, so the two HTTP paths disagree by
3.2x. Grown buffers are then parked in the upstream pool, 8 idle per host for
60 s. Retention past that minute is allocator-side in the sample measured here,
which is why RSS can hold flat for minutes after traffic stops and then step
down with no activity at all. Nothing here is a leak: every wave was released,
one of them as a clean decommit with the fault counter unchanged, and the
post-wave idle level returned to within ~1 MiB of the pre-wave one.

The story, in order of how well each part is established:

| Claim | Standing |
| ----- | -------- |
| missing pass-through HTTP/1 cap sets the height of the HTTP-induced step | demonstrated: reproduced on demand, scales with connection count, direct lever |
| allocator retention keeps the step resident after traffic stops | demonstrated for this sample: clean decommit 18 min after the last transfer |
| cross-worker retention adds to the step at concurrency | plausible from the 1-vs-8 connection gap; not proven here |
| the long-run floor is stable | open; needs a multi-day quiet soak |
| the cap value | measured 2026-09-06 on the RB5009 (§RB5009 buffer A/B): no cap passes the 10 % CPU bar; 128 KiB is the only trade (half the step, +14..20 % CPU during bulk transfers). Owner keeps 408 KiB and takes retention to the HTTP allocation domain |

## Diagnostic

1. **Primary defect — the pass-through HTTP buffer cap is missing.** Excessive
   transient RSS. Findings 1 and 2 below. Sets the *height* of the step:
   0.72 MiB per connection measured at 1 and 1.86 MiB per connection at 8
   concurrent, against two nominal 408 KiB hyper defaults per connection. The
   nominal figures cap hyper's own accounting; they are not a ceiling on what
   the allocator hands out, because `BytesMut` grows by doubling while frames
   still reference the old block. No per-connection ceiling is derived from
   them.
2. **Allocator behaviour — freed pages stay resident for minutes.** Not a
   defect. Sets the *duration*, not the height. Measured: a clean 6.89 MiB
   decommit 18 min after the last transfer, fault counter unchanged either side
   of it. What is demonstrated: in this sample, retention beyond the 60 s pool
   lifetime is allocator-side, not idle-pool retention. mimalloc purge lag is
   the consistent reading of it; no test here excluded some other
   infrastructure cache also holding pages over that window.
3. **Open question — do repeated bursts ratchet the long-term floor on the
   RB5009?** Not answerable from this session, and an earlier ratchet reading in
   this file was retracted: the post-wave idle level returned to 73.84 MiB
   against 72.8 MiB pre-wave. That shows one wave was released, not that the
   long-run floor is stable. The waves also moved lifetime peak RSS to
   178.3 MiB, so these readings cannot serve as the baseline. Needs a clean
   multi-day soak.
4. **Secondary candidate — cross-worker retention at concurrency.** Eight
   concurrent connections cost 2.6x more per connection than one. The upstream
   pool does not explain that gap: it retains the single connection too, so
   pooling scales the step linearly at most. Frees landing on pages owned by
   other tokio workers — the mechanism the runtime-domain experiments named at
   4 workers ([resoak-0.3.1-memory-diagnosis.md](resoak-0.3.1-memory-diagnosis.md)
   F29, F36) — is a plausible contributor. Not measured here, not root cause.
   The A/B below bounds it: a step that shrinks by less than the cap ratio
   leaves a remainder that is not buffer.
5. **Would [allocation-domains-proposal.md](allocation-domains-proposal.md) help
   more?** It addresses a different axis first. The cap is the direct lever on
   item 1; the topology may act on item 4 indirectly, and its effect on step
   height and long-term floor is unproven.

| Axis | Buffer cap | Allocation domains |
| ---- | ---------- | ------------------ |
| step height | yes, primary lever | unproven; plausible via item 4 |
| return time after a burst | no | yes — one 30 s sample against ~9 min (its step 1b) |
| long-term floor | unknown, see item 3 | unproven — steps 1 and 1b showed no difference at +17 min, both inside the ±6 MB purge band |
| per-domain attribution | no | yes |
| cost | two builder calls plus the A/B | ~4 threads, +3..5 MiB baseline, N single-thread runtimes |

Ordering: cap first. It is two builder calls, and it shrinks the quantity the
topology would return faster, so a topology A/B run afterwards measures what
the cap leaves behind — the number that decides it. Running the topology first
would credit it with memory the cap removes on its own.

Its router benchmark row 3 and the buffer A/B below are the same workload
shape: one fixed payload through `:8080`, single stream plus 8 parallel, median
and p95, CPU alongside. Share one harness and one RB5009 night rather than
spending two.

## Decisions

- The 85 MiB reading that opened the investigation is a decaying high-water from
  an earlier port-80 transfer, not a level and not a budget breach. Steady-state
  budget is 128 MB = 122.1 MiB.
- `residual_bytes` is the only accounting bucket that moves with HTTP load. The
  HTTP-induced step is set by hyper's buffer policy; residual also includes
  allocator retention and possibly other holdings, and is therefore not itself
  a direct measure of buffer size.
- The interception path `H1_MAX_BUF` is the intended cap. The pass-through path
  omitting it is the defect, not the interception path being conservative. No
  value is chosen here: 408 KiB is hyper's default and the control, 128 KiB is
  the interception path's existing unmeasured setting. Neither is the product
  value before the RB5009 A/B.
- `http.max_connections` stays at 1024 and is out of scope.
- No production code changed. The A/B ran 2026-09-06 on a throwaway build
  (§RB5009 buffer A/B). Owner's decision the same day: keep 408 KiB, address
  retention through the HTTP allocation domain
  ([allocation-domains-proposal.md](allocation-domains-proposal.md), execution
  in [alloc-domains-http-task.md](alloc-domains-http-task.md)).

## Bugs found

| # | Severity | Site | Defect | Status |
| - | -------- | ---- | ------ | ------ |
| 1 | Medium | `crates/fah-http/src/proxy.rs:316` | Server `http1::Builder` omits `max_buf_size`; hyper defaults to 408 KiB. On plain TCP hyper writes with `WriteStrategy::Queue`, so for a download this bounds the response frames queued ahead of the client socket (and the upstream blocks those frames keep alive); for an upload it bounds the request read buffer. | accepted, owner decision 2026-09-06: every cap that cut memory cost > +10 % CPU (§RB5009 buffer A/B); 408 KiB stays on `main` and `phase3-06`, retention goes to the HTTP allocation domain |
| 2 | Medium | `crates/fah-http/src/proxy.rs:249` | Upstream `Client::builder` omits `http1_max_buf_size`; same 408 KiB default on the read buffer that receives the origin's response, which grows adaptively under fast transfers. | accepted, same decision as 1; 128 KiB recorded as the fallback trade |
| 3 | Low | `crates/fastadhunter/src/main.rs:729` | `MAX_IDLE_UPSTREAMS_PER_HOST = 8` at a 60 s `pool_idle_timeout` retains up to 8 grown buffers per origin, amplifying 2. | not tuned: a few MiB per host for ≤ 60 s, below the "not worth it" threshold |

Classification: none of these is a correctness defect. Requests succeed, bytes
are relayed intact, nothing leaks. They are bounded-memory defects under hard
rule 4. Findings 1 and 2 set the per-connection cost; finding 3 sets how long it
is held.

`http.max_connections` is out of scope and stays at 1024. The per-connection
cost is the lever, not the connection ceiling — that is what findings 1 and 2
address.

Practical severity is limited by bandwidth: hyper's read buffer only grows on
reads that fill it, and the write queue only fills when the origin outruns the
client, so idle and slow connections stay near the 8 KiB initial size. Sizing
from the measured 8-connection wave: the process idles at 72.8 MiB, so
(122.1 − 72.8) / 1.86 ≈ 26 concurrent fast port-80 downloads reach the
122.1 MiB steady-state budget. That is a sizing observation from one wave, not
a limit: the per-connection figure was measured at 8 connections and is not
shown to be linear in either direction. Lowering it raises the headroom without
touching the ceiling.

Reference constants:

| Constant | Value | Source |
| -------- | ----- | ------ |
| `DEFAULT_MAX_BUFFER_SIZE` | `8192 + 4096 * 100` = 417 792 B (408 KiB) | hyper 1.10.1 (the `Cargo.lock` version) `src/proto/h1/io.rs:23`; identical in the 1.11.0 source, checked as source verification only |
| `MAX_BUF_LIST_BUFFERS` | 16 | hyper 1.10.1 `src/proto/h1/io.rs:30`; second bound on the write queue |
| write strategy on plain TCP | `Queue` (`is_write_vectored()` is true) | hyper 1.10.1 `src/proto/h1/io.rs:60`; `Flatten` otherwise |
| `set_max_buf_size` | sets the read strategy max **and** `write_buf.max_buf_size` | hyper 1.10.1 `src/proto/h1/io.rs:86` |
| `http1_max_buf_size` | exists on the legacy client builder | hyper-util 0.1.20 `src/client/legacy/client.rs:1123` |
| `H1_MAX_BUF` | `128 * 1024` | `crates/fah-http/src/intercept.rs:41` |
| capped, interception server | yes | `crates/fah-http/src/intercept.rs:203` |
| capped, interception upstream | yes | `crates/fah-http/src/intercept.rs:366` |
| `HTTP_ORIGIN_PORT` | 80 | `crates/fastadhunter/src/main.rs:721` |
| `pool_idle_timeout` source | `http.idle_timeout_ms` = 60000 | `crates/fastadhunter/src/main.rs:913` |

Mechanism, per leg of a download through the pass-through:

- Upstream leg (finding 2): hyper's read strategy is `Adaptive` — the buffer
  doubles on every read that fills it, capped at `max`. Body frames are `Bytes`
  sliced from that `BytesMut`; while a frame is alive the block stays
  referenced and the next `reserve` doubles into a fresh one. The nominal
  408 KiB is therefore not a ceiling on allocation.
- Client-facing leg (finding 1): for a download its read buffer only sees
  request heads and does not grow. Response frames go to the HTTP/1 write
  queue. Before pulling each body frame hyper checks `can_buffer` — queued
  bytes below `max_buf_size` and fewer than 16 queued frames — and flushes to
  the socket first when either bound is hit (`dispatch.rs:377`), so the queue
  can exceed the cap by at most one frame. Those frames are the upstream leg's
  blocks, so the client cap bounds how many of them one slow client keeps
  alive.
- Bodies are never accumulated by FAH: the pass-through relays
  `Either::Left(Incoming)` and counts via `size_hint`.

The two knobs govern different things — read growth on one leg, queued frames
on the other. A symmetric value is a convenience, not a statement that the legs
are equivalent, and "408 + 408" is not a memory ceiling.

## Proposal

| Change | Site | Effect |
| ------ | ---- | ------ |
| `.max_buf_size(CLIENT_BUF)` on the server builder | `proxy.rs:316` | Caps the client-facing write queue and request read buffer. Value from the A/B. |
| `.http1_max_buf_size(UPSTREAM_BUF)` on the client builder | `proxy.rs:249` | Caps the upstream read buffer. Value from the A/B, not assumed equal to `CLIENT_BUF`. |
| Promote `H1_MAX_BUF` out of `intercept.rs` and set it to the A/B value | `fah-http` crate root, applied at `intercept.rs:203` and `intercept.rs:366` | One measured cap for both HTTP paths; removes the 3.2x disagreement and retires the unmeasured 128 KiB. |

No value is proposed here. `CLIENT_BUF` and `UPSTREAM_BUF` are outputs of the
A/B, and 128 KiB is not a candidate with any more standing than the others.

Effect direction only, not measured: if item 1 of the Diagnostic is the whole
story the step scales with the caps; if it shrinks by less than the cap ratio,
the remainder is item 4. Trade to measure before landing:

| Axis | Expected |
| ---- | -------- |
| Memory | down, in proportion to the chosen caps |
| Throughput, port 80 | down; smaller reads mean more syscalls per MiB |
| Hot path, DNS | none — different crate, different listener |
| Compile time | none |
| Build size | none |

Do not tune `MAX_IDLE_UPSTREAMS_PER_HOST` in the same change. With the cap in
place its worst case falls to 1 MiB per host, below the <1 MB not-worth-it
threshold.

### 128 KiB is inherited, not chosen

`H1_MAX_BUF` was set on the interception path; no measurement selects it. Before
adopting it for pass-through, measure.

The A/B therefore also re-opens the interception path. Once a value is chosen,
`H1_MAX_BUF` gets updated to it. The current 128 KiB is untested in both
directions: nothing shows it is low enough to matter, and nothing shows it is
high enough not to cost throughput. Both HTTP paths then read one measured
constant instead of one guessed and one absent.

Caveat before copying the number across: the A/B measures cleartext
pass-through. Interception adds a TLS record layer and an `auto::Builder` that
may negotiate HTTP/2, where `max_buf_size` applies only to the HTTP/1 arm. Its
TLS stream reports vectored writes (tokio-rustls 0.26.4), so hyper queues
frames there as well, but the record layer between the queue and the socket is
a further buffer this A/B does not measure. Adopt the pass-through value as
the default for both, then sanity-check the interception path at that value
rather than assuming it transfers.

Blocker: neither cap is reachable from a bench. The server builder at
`proxy.rs:316` sets nothing and the upstream `Client` is built inside
`Proxy::new`. The knobs stay **two independent fields** — `client_buf` and
`upstream_buf`, each an `Option<usize>` defaulting to `None` so behaviour is
unchanged until set — because the legs govern different things and the
implementation must not hard-code symmetry. `TlsProxy::with_splice_buffers` is
the precedent for the shape, and it already takes an up and a down argument
rather than one. Extract the client construction into one helper so `new` and
the setter do not duplicate it. The first experiment nonetheless sets both to
the same value on purpose; see the benchmark plan.

Analytical expectation, to be confirmed or refuted by the A/B: the cap bounds
`read()` size, so a 6x smaller buffer is at most 6x the syscall count. At 64 KiB
per read and 125 MB/s that is on the order of 2000 reads/s, a fraction of one
core. The read size is also bounded by what the kernel already has buffered, so
on the real WAN-fed port-80 path the large buffer is often not filled at all.
Figures are illustrative and name no preferred arm.

## Benchmark plan — RB5009 buffer A/B

**Executed 2026-09-06. Results, deviations and the verdict are in
§RB5009 buffer A/B below; this section is the plan as it stood before the run.**

Owner-requested. Throwaway build only; no production code change until the A/B
picks a value. Not before the day-7 soak read (2026-09-08 10:28 local) and not
on the soaking container. The throwaway is `db2f9b2` + `4eddc39` (the 0.3.2
base) plus the two knobs, so its baseline arm is production 0.3.2 and the
memory step it measures carries the per-listener `concurrent_connections`
high-water mark.

The earlier 8 + 4 + 3-arm matrix is withdrawn: every arm is an owner-run
container restart, so a dozen arms cannot share a sitting, and the analytical
expectation above says the throughput cost is small. A minimal causal A/B
answers the high-value question — does a low cap cost anything — for three
restarts.

### Arms

Two independently settable runtime variables, `client_buf` (finding 1 site)
and `upstream_buf` (finding 2 site). The implementation never hard-codes
symmetry; the first experiment sets both to the same value on purpose, because
one arm then answers the cheap question.

| Run | client / upstream (KiB) | Role |
| --- | ----------------------- | ---- |
| 1 | 408 / 408 | baseline: hyper default, current behaviour |
| 2 | 64 / 64 | aggressive low cap |
| 3 | 408 / 408 | control: must reproduce run 1 or the sitting is void |
| 4, conditional | 96 / 96 or 128 / 128 | only if run 2 shows a material throughput or CPU regression |

Asymmetric arms (`client_buf` ≠ `upstream_buf`) are added only if run 2 shows
the two legs behaving differently — for example the memory step shrinking on
the 8-parallel case but not the single stream, or vice versa. No asymmetric arm
is planned ahead of that evidence.

Neither 408 KiB nor 128 KiB is a candidate with standing: 408 is the default
and the control, 128 is the interception path's inherited setting.

### Metrics per arm

| Metric | How |
| ------ | --- |
| single-stream throughput, `time_total` | one request, fixed payload |
| 8-parallel aggregate throughput | 8 concurrent, same payload |
| per-stream p95, where available | needs N repeats per arm; N fixed across all arms |
| CPU during both, per MiB moved | RB5009, sampled across the run |
| RSS and `residual_bytes` before / peak / +15 min | `GET /api/v1/telemetry` |
| `concurrent_connections` high-water per sample | perf sample, from `4eddc39`; gives bytes per connection |

The slow-client case is dropped from the arms: it throttles the wrong leg (see
Verification) and a slow origin is not available in this setup.

### Protocol

- Sequential on the same RB5009, same payload, same origin, same sitting.
- Run order is fixed baseline / arm / control. With three runs that is the
  alternation, and the repeated baseline is the noise band.
- Each arm is one owner-run container restart with the knob values in the
  container's environment. The runtime knob saves a rebuild and an image
  transfer per arm, not the restart. A restart also resets the pool and the
  allocator, so every arm starts from the same cold state; the memory figure
  compared is the delta before / peak / +15 min, never the absolute level.
- Memory samples must clear the purge lag. The measured lag is `pool_idle_timeout`
  plus mimalloc delay, about 3 min; the +15 min sample is safely past it.
- Wall clock: three runs at a full +15 min tail plus restarts is about 1 h, one
  sitting. Run 4 adds ~20 min.

### Blockers, in order

1. **The knobs do not exist.** Both caps are unreachable. Needs the two
   `Option<usize>` fields, `client_buf` and `upstream_buf`, defaulting to
   `None`, fed from the container environment on the throwaway build. Throwaway
   branch, not committed.
2. **The router is off limits.** Deploying the throwaway image and each restart
   is a change the owner runs, not the agent. Read-only telemetry pulls need no
   ask.
3. **Origin choice, split by question.** For the memory step a WAN origin is
   the right instrument: it is the production shape (LAN → WAN over port 80),
   and the verification waves reached 1.86 MiB/conn through one. For throughput
   a WAN origin measures the uplink; a LAN origin on port 80 measures the
   proxy's own ceiling but needs an `egress.allow_destinations` exception — an
   owner-run config change. Recommended: WAN origin for the first sitting, with
   CPU per MiB alongside so a syscall-cost regression shows even where the
   uplink bounds throughput; LAN origin only if the owner wants the ceiling
   figure.
4. **CPU reading trap.** Do not calibrate from `cpu-frequency` or
   `scaling_cur_freq`; see the environment notes in the root `CLAUDE.md`.
5. **Optional dev-box dry run, memory only.** The `E:/fah-diag` rig at 4
   workers can show whether 64/64 shrinks the 8-parallel step at all before an
   RB5009 sitting is spent. Throughput does not transfer from it.

### Decision rule

64/64 becomes the leading candidate if its single-stream and 8-parallel
throughput are within the control's noise band, CPU shows no material
regression, and the HTTP RSS step is materially smaller than the baseline's. If
it regresses, run 4 at one intermediate value decides between that value and
the baseline. Where two arms tie on throughput, the lower memory step wins.
Record the noise band from the repeated control, not from an assumed
percentage.

Until the A/B runs, no value is documented as correct — including 128 KiB,
which this file treats only as the interception path's current unmeasured
setting.

## RB5009 buffer A/B — 2026-09-06

Device: RB5009, probe container `fah-h1buf` on `veth3` (172.17.0.4) beside the
live resolver; production never stopped, soak container untouched by the
harness. Build: `db2f9b2` + two `Option<usize>` knobs read from
`FAH_BENCH_H1_CLIENT_BUF_KIB` / `FAH_BENCH_H1_UPSTREAM_BUF_KIB` (server
`max_buf_size`, client `http1_max_buf_size`) + `cpu_user_ms` /
`cpu_system_ms` from `getrusage` on `/api/v1/debug/memory`. Worktree
`E:/FastAdHunter-var-h1buf031`, six files, not committed. Probe config = the
live 0.3.1 TOML plus a probe-only egress exception (`192.168.10.10/32`,
`allow_ip_literal_hosts = true`) and production's `apikey`. Harness
`E:/fah-diag/tools/h1buf-ab.sh`; raw series under `E:/fah-diag/out/h1buf/`.

Workload per arm: settle 180 s after a cold probe start, then 5 × 100 MiB
single stream and 5 × (8 × 10 MiB) parallel from `cachefly.cachefly.net` over
the WAN (uplink ~95 MiB/s, ~1 Gbit), 900 MiB in ~10 s; memory sampled every
1 s during transfers and at end / +3 min / +15 min. Every arm is an owner-run
envlist change plus restart, so floors are cache-start floors (~43 MiB). Run a
alone started from a raw-compile floor (63.65); its absolute loaded level
(99.3) is the figure comparable to the other 408 arms (103.3, 110.5, 105.5).

| arm | start UTC | floor | peak | +15 min | step peak | step +15 | CPU s | ms/MiB | single med (min–max) MiB/s | par8 med (min–max) MiB/s | p95 s | minflt |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 408/408 a | 13:22 | 63.65 | 99.28 | 100.95 | +35.6 | +37.3 | 11.26 | 12.5 | 95.5 (90.4–97.7) | 87.9 (76.2–92.2) | 0.814 | 5334 |
| 64/64 | 13:42 | 43.71 | 57.28 | 51.26 | +13.6 | +7.6 | 14.63 | 16.3 | 86.5 (75.1–89.5) | 89.1 (76.1–90.6) | 0.828 | 1855 |
| 408/408 b | 14:03 | 43.88 | 103.28 | 99.65 | +59.4 | +55.8 | 11.95 | 13.3 | 92.9 (85.4–96.2) | 86.2 (64.7–91.5) | 0.829 | 7576 |
| 128/128 | 14:31 | 43.59 | 75.49 | 71.15 | +31.9 | +27.6 | 12.97 | 14.4 | 83.8 (68.3–98.3) | 87.5 (86.7–90.8) | 0.800 | 2140 |
| 96/96 | 14:51 | 43.75 | 74.02 | 73.89 | +30.3 | +30.1 | 14.17 | 15.7 | 85.7 (68.4–94.0) | 88.3 (87.4–90.8) | 0.820 | 1029 |
| 408/408 c | 15:11 | 42.90 | 110.47 | 111.54 | +67.6 | +68.6 | 11.90 | 13.2 | 89.0 (72.6–96.2) | 89.7 (88.7–92.9) | 0.803 | 5177 |
| 128/128 b | 16:34 | 42.78 | 79.21 | 79.86 | +36.4 | +37.1 | 13.66 | 15.2 | 79.6 (69.5–96.6) | 91.3 (89.5–91.9) | 0.806 | 1645 |
| 256/256 | 16:54 | 43.31 | 104.40 | 105.26 | +61.1 | +62.0 | 12.72 | 14.1 | 88.6 (71.7–92.0) | 90.2 (88.6–91.5) | 0.799 | 3079 |
| 408/408 d | 17:15 | 43.27 | 105.51 | 100.18 | +62.2 | +56.9 | 10.55 | 11.7 | 95.1 (70.6–95.8) | 89.5 (82.8–90.1) | 0.806 | 6382 |

Baseline band from the four 408 arms: CPU 10.55–11.95 s (mean 11.4); step held
+56..+69 at cache-start floors — a 13 MiB spread, above the ±2 MiB confidence
threshold, so arm effects are read against the whole band, not one baseline;
8-parallel 86–90 MiB/s; p95 0.80–0.83 s. Single-stream medians swing 80–95
with minima near 70 in most arms: WAN noise; the 8-parallel figure is the
stable throughput reading. The evening control (d) is the lowest CPU reading of
the day, which refutes the afternoon-vs-evening drift hypothesis raised after
128 b.

Selection rule, fixed by the owner during the sitting: the smallest cap that
materially reduces RSS with no more than +10 % CPU over the control band and no
relevant throughput / p95 regression.

| cap | memory | CPU vs band mean | throughput / p95 | verdict |
| --- | --- | --- | --- | --- |
| 256 | none (+62 vs +56..+69) | +11 % | in band | no gain |
| 128 | halves (+28 / +37) | +14 / +20 % | in band | fails the bar; the only trade |
| 96 | same as 128 (+30) | +24 % | in band | dominated by 128 |
| 64 | +8 | +28 % | single −7 %, rest in band | fails |

**Result: no cap passes.** Below 256 KiB every reduction buys memory with CPU,
roughly linearly (11.7 → 16.3 ms/MiB). 128 KiB is the only trade worth naming:
half the step for +14..20 % CPU while a bulk port-80 transfer is in flight.
Owner's decision: keep 408 KiB (hyper's default, current behaviour) and address
retention through the HTTP allocation domain
([alloc-domains-http-task.md](alloc-domains-http-task.md)).

Other findings from the sitting:

| Finding | Evidence |
| --- | --- |
| No retrace on an idle process at any cap | +15 min ≈ peak in all nine arms, `MIMALLOC_PURGE_DELAY=0` in the envlist. The probe serves no DNS, so nothing cycles the owners (F29). Production's 45 min retrace (§Verification) is DNS-churn driven, not allocator-driven |
| The CPU cost of a smaller cap is real | the lowest 128 reading (12.97) sits 8.5 % above the highest 408 reading (11.95); the evening 408 is the lowest of the day |
| Fewer minor faults with smaller caps | 5177–7576 at 408, 1029–2140 at ≤ 128: less fresh memory touched per transfer |
| LAN origin from the dev box is slower than WAN | 57–63 MiB/s through the probe with a python `sendfile` server and with nginx-in-Docker alike; 1 Gbps full-duplex NIC serving and receiving the same stream. "LAN = final buffer-selection benchmark" withdrawn for this setup; the WAN control reproduced within 3 % throughput / 6 % CPU in the afternoon and was used for selection |
| 0.3.1 resolves an IP-literal `Host` through the DNS upstreams | `approved_address` always calls the resolver; `allow_ip_literal_hosts` only skips the claim check, so `Host: 192.168.10.10` ends in `resolve_failures` and a 502 "request refused". Workaround for a LAN origin: a public name that resolves to the LAN IP (`192-168-10-10.nip.io`) plus the `allow_destinations` exception |
| Dev-box dry runs hit production | the dev box is a LAN client, so its Docker containers' port-80 fetches went through production's pass-through (NAT rule 10, `src-address=!172.17.0.0/24`): ~700 MiB at 8 parallel, 12:44–12:52Z, one more production spike. The probe path avoids it: the harness targets 172.17.0.4:8080 and the probe fetches from 172.17.0.0/24 |
| RouterOS facts used | free memory 658 MiB of 1024 before the sitting; global container `memory-high` unlimited; `veth3` 172.17.0.4/24 on bridge `CONTAINERS`; `fah-env` = `MIMALLOC_ARENA_EAGER_COMMIT=0`, `MIMALLOC_PURGE_DECOMMITS=1`, `MIMALLOC_PURGE_DELAY=0`; `/container/envs` keys by `list=` |

State at the end of the sitting: probe container `fah-h1buf` **stopped, not
removed**; mounts `h1buf-config` / `h1buf-data` and envlist `h1buf-env` kept for
the allocation-domain A/B; tar `fastadhunter-h1buf-db2f9b2-rosready.tar` still
on `kingston/` (counts as used until deleted, F34).

## Measurements

RSS against proxied bytes, 60 s samples. `d_MiB` is the delta of
`counters.http.response_bytes`, port 80 only.

| time | rss MiB | resid MiB | d_http req | d_MiB | d_dns |
| ---- | ------- | --------- | ---------- | ----- | ----- |
| 13:13:35 | 73.4 | 43.7 | 0 | 0.0 | 101 |
| 13:15:35 | 73.4 | 43.7 | 0 | 0.0 | 109 |
| 13:17:35 | 72.8 | 43.0 | 0 | 0.0 | 72 |
| 13:18:35 | 105.7 | 76.0 | 264 | 283.4 | 111 |
| 13:19:35 | 105.7 | 76.0 | 13 | 0.1 | 81 |
| 13:20:35 | 103.9 | 74.1 | 110 | 94.6 | 120 |
| 13:21:35 | 104.6 | 74.9 | 610 | 603.6 | 91 |
| 13:22:35 | 107.4 | 77.6 | 64 | 56.2 | 92 |
| 13:23:36 | 109.3 | 79.6 | 65 | 60.5 | 114 |
| 13:24:36 | 109.3 | 79.6 | 1 | 0.0 | 78 |
| 13:25:36 | 109.3 | 79.6 | 0 | 0.0 | 81 |
| 13:26:36 | 109.3 | 79.6 | 0 | 0.0 | 85 |
| 13:27:36 | 104.7 | 74.9 | 0 | 0.0 | 72 |
| 13:29:36 | 98.4 | 68.7 | 77 | 71.0 | 107 |
| 13:30:36 | 94.6 | 64.8 | 197 | 185.0 | 98 |
| 13:31:36 | 93.9 | 64.2 | 0 | 0.0 | 118 |

Totals over the window: 1 354 MiB proxied, idle floor 72.8 MiB, loaded peak
109.3 MiB, step 36.5 MiB.

Key readings:

| Reading | Value |
| ------- | ----- |
| RSS holds flat after last traffic | 3 min (13:23:36 to 13:27:36) |
| `pool_idle_timeout` | 60 s |
| RSS independent of throughput | 283 MiB/min and 604 MiB/min both give ~105 MiB |
| DNS rate across every row | 72–120 queries/min |
| Composition at 73.4 MiB RSS | ruleset 24.06, cache 4.20, stats 1.46, binary 8.78, residual 43.69 |
| Lifetime peak RSS | 153.2 MiB, startup compile, 2.76 s for 1.2 M raw rules |

Purge signature — one 30 s interval showed RSS −2.6 MiB with minor faults
+19 532 against a baseline of ~100 per interval. Consistent with reclamation
and refault activity inside one interval; the sequence is not established.
19 532 faults is ~76 MiB of pages touched, far more than the net 2.6 MiB, so
the two numbers are not one event.

Ruled out by measurement:

| Hypothesis | Disproof |
| ---------- | -------- |
| API request handling | 40 back-to-back `/api/v1/stats` in <2 s: RSS fell 78.7 to 72.5, minor faults unchanged |
| Large API responses | largest payload is `/api/v1/clients` at 112 KB; telemetry 2.9 KB, cache 195 B |
| DNS burst | query rate flat 72–120/min across both RSS jumps |
| Ruleset reload | last list refresh 2026-09-05T22:50Z, ~11 h before the step; `rules` and `compile_duration_seconds` unchanged |
| Body buffering | pass-through relays `Either::Left(Incoming)`; `response_bytes` from `size_hint` |

### Pre-change bench baseline

Dev box, x86, loopback origin, `cargo bench -p fah-http --bench proxy --
http_opaque_body --warm-up-time 1 --measurement-time 4`. Hyper buffer at its
uncapped default. Recorded so the A/B has a same-checkout reference; criterion
stored baselines are not used for the comparison.

| Arm | direct | through proxy |
| --- | ------ | ------------- |
| 8 KiB | 34.71 µs / 225.1 MiB/s | 66.77 µs / 117.0 MiB/s |
| 1 MiB | 805.9 µs / 1.212 GiB/s | 935.1 µs / 1.044 GiB/s |
| 8 MiB | 5.175 ms / 1.510 GiB/s | 5.914 ms / 1.321 GiB/s |

Variance is wide on the two large arms (1 MiB spans 878–990 µs proxied), so the
A/B needs longer measurement time before small differences mean anything. RB5009
figures are not derivable from these by the ~9x factor: that factor is for
CPU-bound work, and these arms are loopback-IO bound.

## Verification

Reproduced on the live 0.3.1 container, 2026-09-06 13:49–13:53 local. Requests
sent to `172.17.0.2:8080` in origin form with a `Host` header, which is the
shape the transparent redirect produces. Origin `cachefly.cachefly.net`, client
`192.168.10.10` (dev box). 176 MiB total pulled over the household uplink.

| Wave | Conns | Bytes | RSS before | RSS after | Step | Per conn |
| ---- | ----- | ----- | ---------- | --------- | ---- | -------- |
| fast | 1 | 10 MiB | 82.59 | 83.31 | +0.72 MiB | 0.72 MiB |
| fast | 8 | 80 MiB | 83.68 | 98.56 | +14.88 MiB | 1.86 MiB |
| slow client | 8 | 16 MiB | 92.69 | 98.45 | +5.76 MiB | 0.72 MiB |

The single-connection wave is the tightest datapoint. Its +0.72 MiB is of the
same order as two nominal 408 KiB hyper defaults, and that is all it shows: the
measurement includes per-connection and task state, the nominal figure is not
an allocation ceiling (see the mechanism note under Bugs found), and nothing
here attributes the 0.72 MiB to either buffer. Read it as an order-of-magnitude
consistency check with the buffer hypothesis, not as a buffer attribution.

The step scales with connection count, not bytes: one connection moving 10 MiB
costs 0.72 MiB, eight moving 80 MiB cost 14.88 MiB. Per connection the
eight-connection figure is 2.6x the single one, and the upstream pool does not
explain that: it retains the single connection too, so pooling scales the step
linearly at most. What does explain the excess is not measured here. Frees
landing on pages owned by other tokio workers — the mechanism the
runtime-domain experiments named at 4 workers — is the candidate, not a
finding. The A/B's 8-parallel arm bounds it: if the step shrinks by less than
the cap ratio, the remainder is not buffer.

Decay after the 8-connection fast wave, matching `pool_idle_timeout` plus
mimalloc purge lag:

| time | rss MiB |
| ---- | ------- |
| 13:50:05 | 99.71 |
| 13:50:46 | 97.71 |
| 13:51:26 | 97.87 |
| 13:52:06 | 92.71 |
| 13:52:46 | 92.69 |

The third wave is a negative control only, not a causal finding.
`curl --limit-rate` throttles the client leg; hyper's `Incoming` is
pull-driven, so the proxy reads the origin only as fast as the client drains
it, and whatever the origin sent ahead sits in the kernel's socket receive
buffer, which is not process RSS. The wave is therefore not a controlled
measurement of adaptive buffer growth in either direction. What it shows: 8
throttled connections cost 0.72 MiB each against 1.86 MiB for 8 fast ones, and
RSS kept rising to 100.03 MiB after the wave completed. Both are consistent
with the buffers staying small under paced reads; neither is attributed.
Isolating adaptive growth needs a slow *origin*, not a slow client.

The `/api/v1/events` stream attributed both waves to `192.168.10.10` with byte
counts matching the transfers, confirming per-client HTTP attribution works in
0.3.1 without the missing concurrency gauge.

### Purge lag, not idle-pool retention

Sampled 14:10–14:13, about 18 min after the last transfer, with the request
counter frozen:

| time | rss MiB | resid MiB | minor faults | http req |
| ---- | ------- | --------- | ------------ | -------- |
| 14:10:57 | 83.38 | 53.62 | 3 152 807 | 8078 |
| 14:11:27 | 83.38 | 53.62 | 3 152 807 | 8078 |
| 14:11:57 | 76.49 | 46.72 | 3 152 807 | 8078 |
| 14:12:27 | 76.49 | 46.72 | 3 152 807 | 8078 |
| 14:12:57 | 76.49 | 46.72 | 3 152 807 | 8078 |
| 14:13:27 | 76.49 | 46.72 | 3 152 807 | 8078 |

A single 6.89 MiB decommit with the fault counter identical on both sides of it
and no request activity. The idle pool is excluded on all three grounds:
`pool_idle_timeout` is 60 s and this is 18 min later; nothing opened or closed;
a teardown would show allocation activity around it. This is the strong
allocator evidence in the file: in this sample, retention beyond the 60 s pool
lifetime is allocator-side. It does not exclude some other infrastructure cache
holding pages over the same window; no test here targets that.

Two distinct signatures were observed, and they should not be conflated:

| Signature | RSS | Minor faults | Reading |
| --------- | --- | ------------ | ------- |
| reclamation plus refault | −2.6 MiB | +19 532 | reclamation and refault activity in one interval; sequence not established |
| clean decommit | −6.89 MiB | unchanged | dead pages returned, never touched again |

Correction to the decay table above: it is labelled as `pool_idle_timeout` plus
purge lag. The pool contribution is bounded at 60 s, so retention past that
minute is allocator-side rather than idle-pool. That is what the samples show;
they do not separately rule out another infrastructure cache holding pages over
the same window.

### The floor returns — an earlier ratchet reading was premature

| Reading | rss MiB | resid MiB | When |
| ------- | ------- | --------- | ---- |
| 24 h minimum | 62.6 | — | before this session |
| idle | 72.8 | 43.0 | 13:17, after natural traffic decayed |
| idle | 76.49 | 46.72 | 14:13, 18 min after the verification waves |
| idle | 73.84 | 43.97 | 14:39, 45 min after the verification waves |

Retracted: the 62.6 → 72.8 → 76.49 sequence was written up as a ratcheting
floor. It was not. The 76.49 reading had not settled — it held flat across six
samples, then purged further. At 14:39 the idle level is within ~1 MiB of the
72.8 measured before any synthetic load, with 25 small proxy requests in
between and no change in bytes forwarded.

What survives: the post-wave level returns to the pre-wave level. What is not
established: whether either sits above a true long-run floor. The 62.6 MiB
figure is a 24 h minimum that predates both the household's own traffic today
and the verification waves, so it is not a like-for-like comparison and no
growth claim rests on it.

Accounted structures stayed flat throughout: ruleset 24.06–24.07 MiB, cache
4.20–4.31 MiB, stats 1.46–1.49 MiB.

Method note this cost: a flat reading is not a settled reading when mimalloc is
purging. Six identical samples over three minutes still preceded a further
2.65 MiB release. Floor claims need a quiet window measured in hours.

Also raised by the verification waves: lifetime `process_peak_rss` moved from
153.2 to 178.3 MiB. That high-water is benchmark-contaminated, not production
traffic, and is not a production baseline.

### The verification wave and the running soak

The wave ran 2026-09-06 10:49–10:53Z (13:49 local), inside soak day W6 and
before the day-7 read (2026-09-08T07:27:49Z, 10:27 local) that the hand-off in
[resoak-0.3.1-memory-diagnosis.md](resoak-0.3.1-memory-diagnosis.md) had
reserved as no-burst. Declared here as a confound, not hidden:

- G2 gates `floor(W7) − floor(W4) < 2 MiB` on day-window RSS minima
  ([resoak-0.3.1-predeclaration.md](resoak-0.3.1-predeclaration.md) §G2). A
  45 min excursion cannot lower a minimum; it raises W6's or W7's only if
  residue persists, and residue is what the gate measures. In-file, the
  post-wave idle returned to within ~1 MiB of pre-wave inside 45 min. Expected:
  the gate stays computable and unaffected. If G2 fails, the wave is a declared
  alternative explanation for a W7 residue that the day-7 read cannot separate
  from a natural ratchet — the case the no-burst rule existed to prevent.
- G3 peak-step attribution: the day-7 pull will carry a `process_peak_rss`
  step to 178.3 MiB at 10:49–10:53Z on 09-06. Attribute it to this wave.
- The hourly-minima slope series has two affected hours, 10:00Z and 11:00Z on
  09-06. In-file readings put them at 72.8 and ≤ 73.84 MiB, within ~1 MiB of
  the pre-wave idle. No local production series covers them (last local pull
  06:45Z 09-06) and production was not queried again for this revision, so the
  check is done on the day-7 pull: if those two hourly minima stand out from
  their neighbours by more than the ~1 MiB seen here, report the slope with and
  without them.
- Second confound, same day: the dev-box dry runs of the A/B harness went
  through production's port-80 redirect 12:44–12:52Z (§RB5009 buffer A/B).
  The owner treats the 0.3.1 soak as invalidated for G2 from this point; the
  probe container took every later run.

## Client attribution

Unconfirmed. The port-80 path carries no HTTPS, so 1 354 MiB of cleartext in
19 min points at update delivery.

| Client | Evidence | Status |
| ------ | -------- | ------ |
| `2a02:2f04:5000:a400:1845:594:c99e:9e6e` | queried `msedge.b.tlu.dl.delivery.mp.microsoft.com` in-window; same interface id as `fd6c:7f32:8e91:0:1845:594:c99e:9e6e`, which queries `login.live.com`, `aps.prod.windows.com`, `arc.msn.com` | candidate, not proven by byte attribution |
| `2a02:2f04:5000:a400:dd99:85f7:9beb:9bc9` | only proxy requests captured in the quiet window, `connectivitycheck.gstatic.com`, 0 bytes | excluded as heavy talker |

The device identity does not change the finding. Any LAN client can drive the
73 to 109 MiB step by downloading over port 80. Attribution stops at the
candidate device: 0.3.1 exposes no per-listener concurrency gauge, so
bytes-per-connection cannot be derived from the household window; the
verification waves supplied it instead.

## Files changed

No production code. Dev-box artefacts only: throwaway worktree
`E:/FastAdHunter-var-h1buf031` (`db2f9b2` + the knob patch, six files,
uncommitted), image `fah:db2f9b2-h1buf` (amd64) and the arm64 tars in that
worktree, harness `E:/fah-diag/tools/h1buf-ab.sh`, origin server
`E:/fah-diag/tools/origin-server.py`, probe config dir `E:/fah-diag/probe/`.

## Remaining TODOs

- At the day-7 pull (2026-09-08 after 10:28 local): compute G2 as declared;
  check the 10:00Z and 11:00Z 09-06 hourly minima against their neighbours;
  attribute the 178.3 MiB `process_peak_rss` step to the 09-06 verification
  wave under G3.
- Allocation-domain branch per
  [alloc-domains-http-task.md](alloc-domains-http-task.md): `alloc-domains/http`
  from `64be513`, cherry-pick `4eddc39`, HTTP domain (acceptor + channel
  hand-off, N single-thread runtimes, shared `max_connections`), A/B on the
  probe against a `main` + `4eddc39` build. Decisive metric: +3 / +15 min
  against peak, where every 408 arm above held ~100 % for 15 min.
- 128 KiB stays on record as the fallback if the domain does not deliver the
  return time: half the step for +14..20 % CPU during transfers. Re-decide with
  the domain numbers, not before.
- Decide whether `cpu_user_ms` / `cpu_system_ms` on `/api/v1/debug/memory`
  (bench build only) becomes a product field; it separated the probe's CPU
  from production's, which `/tool/profile` cannot.
- The pass-through stays uncapped by decision while the interception path keeps
  `H1_MAX_BUF = 128 KiB`; the 3.2x disagreement is now a recorded decision, not
  an omission. Revisit only with the domain result.
- Delete `fastadhunter-h1buf-db2f9b2-rosready.tar` from `kingston/` when the
  domain image replaces it.
- Soak the idle floor for several days with no synthetic load, sampling only
  after hours of proxy quiet, to establish whether a long-run floor exists and
  where. This session showed the post-wave level returning to the pre-wave one,
  which is not the same claim. Do not reuse these numbers as a baseline: the
  verification waves moved `process_peak_rss` to 178.3 MiB, and a flat reading
  here twice preceded a further purge.
- Catch a burst on the live `/api/v1/events` watcher to close the client
  attribution.
- Decide whether PERFORMANCE.md should carry a loaded-HTTP RSS figure separate
  from the idle figure. Needs owner approval before any doc edit.
