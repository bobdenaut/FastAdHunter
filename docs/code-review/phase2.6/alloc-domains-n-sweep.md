# HTTP allocation domains — N sweep on the RB5009 (connection rate, mixed DNS+HTTP, transfers, shutdown)

Run 2026-09-07 06:33–10:28Z. Image `fastadhunter:alloc-6d591ad`
(`alloc-domains/http` at `6d591ad` = `52232f3` + the IP-literal fix
`0a716ec`), probe container `fah-alloc3` on `veth3` (172.17.0.4), mount lists
`h1buf-config` / `h1buf-data`, envlist `h1buf-env` (mimalloc keys +
`FAH__RUNTIME__HTTP_RUNTIMES` = the arm). Container restarted between arms;
the five runs inside an arm share one process. Production untouched.
Complements [alloc-domains-http-task.md](alloc-domains-http-task.md) §Results
(WAN transfers, N=0 vs N=2) with the two questions that A/B could not answer:
connection rate and DNS under HTTP load.

## Summary

- Four arms: N=0 (0.3.1 shared runtime, control), N=2, N=3, N=4. Five runs
  per arm (three for N=0): new-connection rate, keep-alive rate, mixed
  (DNS 300 qps + new connections), WAN 900 MiB, LAN 900 MiB.
- N=2 carries the tested connection-rate workload at ~1.9k new connections/s
  and ~2.6k keep-alive requests/s, with materially lower CPU (2.1–2.2 cores
  against 3.6 for N=0, 23–26 % less CPU per request) and better DNS latency
  under load (p50 0.96 ms against 3.35 ms). Zero 502s. It holds +19 MiB after
  the WAN burst against +56..+60 for the old way.
- The LAN test is not a 1 GbE capacity test: the router's forwarding path
  capped every arm at ~67–70 MiB/s par8, below the WAN's ~89. Arm-to-arm it
  is fair; as a ceiling it says nothing about N.
- N=3 and N=4 add keep-alive rate (+26 %, +33 %) at the price of HTTP p95
  (49–53 ms against 28 ms) and memory held after the burst (+32, +45 MiB).
  New-connection rate stops growing after N=3. DNS does not degrade at any N.
- Shutdown at N=4 with three transfers in flight: drains time out at +5.0 s,
  exit 0 at +5.6 s, no SIGKILL.
- Verdict by the owner's criterion (smallest N that carries the tested
  workload, keeps HTTP p95 in bounds, does not degrade DNS, keeps the locality
  win): **N=2**. It is the compiled-in default (`max(1, cores/2)`). Untested
  here: TLS termination and HTML rewriting (Phase 3/4); the N decision is
  re-measured when those exist (ADR-0006 revisit criteria).

## Decisions

- Instrument: the numbers here come from the third rig. The first two are
  discarded (§Rig), so this file supersedes nothing in the task file and the
  task file's WAN figures stand.
- ΔRSS is reported against the arm-local floor (the arm's first `before`
  sample); levels are never compared across arms.
- N=4's DNS result (best p99 of the four) corrects the pre-run expectation
  that four HTTP threads would starve the DNS workers; the domains yield to
  them better than the shared runtime does.

## Rig

- Dev box 192.168.10.10, Windows. Origin: `static-web-server` 2.44 native on
  port 80 serving `E:/fah-diag/origin` (1k/10k/50k/10mb/100mb `.bin`);
  380 MB/s on loopback. The probe reaches it through the egress exception
  `192.168.10.10/32` with `allow_ip_literal_hosts = true`, which needed the
  IP-literal fix (`0a716ec`; before it every such request was a 502).
- HTTP client `E:/fah-diag/tools/connrate.py`: 6 processes × 8 threads
  (48 concurrent), raw sockets, files chosen uniformly from 1k/10k/50k.
  Close mode = one request per connection with `Connection: close`; keep-alive
  = 20 requests per connection, the last with `Connection: close`. In both
  modes the client reads to the server's FIN before closing, so TIME_WAIT sits
  on the probe (Linux) and not on the Windows client, whose 16 384 dynamic
  ports at 120 s TIME_WAIT cap a client-closes-first loop at ~130
  connections/s.
- DNS client `dnsload.py`: 300 qps UDP to the probe, 50 % cached (20 names,
  warmed), 20 % blocked (ad hosts), 30 % uncached (random label under
  `example.com`, forwarded every time).
- Driver `alloc-run.sh` samples `/api/v1/debug/memory` every 2 s
  (`process_rss`, `cpu_user_ms`, `cpu_system_ms`) and takes a +3 min sample.
  Transfers: `h1buf-ab.sh` N=5, WAN `cachefly.cachefly.net` (+3/+15 min
  tail), LAN the same origin as above (`/100mb.bin`, `/10mb.bin`).
- Discarded rigs: (1) Python `http.client` closing first — Windows port
  exhaustion, 250k failed connects; (2) Python `ThreadingHTTPServer` origin —
  listen backlog 5, the probe's upstream connects refused (212 × 502) and
  Python's per-request latency in the numbers. Rig 3 through the probe:
  close-mode rate equal to `oha`'s (2 405 vs 2 480 rps) with the probe at
  3.6 cores, so the probe is the bottleneck, not the client.
- LAN ceiling: single stream 58–60 MiB/s, par8 67–70 MiB/s at every N, below
  the WAN's 89 MiB/s. The origin does 380 MB/s locally, so the limit is the
  router forwarding both flows (origin → probe, probe → client) over the same
  LAN port. Fair arm to arm; not a 1 GbE line-rate test.

## Measurements

Rate runs: 300 s each, 48 concurrent client connections. CPU = probe
`cpu_user + cpu_system` delta over the run ÷ 300 s. ΔRSS = `process_rss` at
the +3 min sample minus the arm-local floor.

| | N=0 | N=2 | N=3 | N=4 |
| --- | --- | --- | --- | --- |
| close: rps | 2341 | 1860 | 1963 | 1947 |
| close: p50 / p95 / p99 ms | 8.8 / 15.9 / 23.2 | 12.8 / 27.8 / 58.0 | 9.1 / 49.1 / 59.2 | 8.4 / 50.7 / 59.2 |
| close: cores, ms/req | 3.63, 1.55 | 2.21, 1.19 | 2.58, 1.31 | 2.65, 1.36 |
| close: 502s | 99 | 0 | 0 | 0 |
| keep-alive: rps (conn/s) | 3297 (165) | 2574 (129) | 3237 (162) | 3412 (171) |
| keep-alive: p50 / p95 / p99 ms | 11.2 / 46.9 / 60.3 | 17.2 / 45.3 / 63.8 | 9.6 / 53.5 / 60.8 | 8.4 / 53.3 / 60.2 |
| keep-alive: cores, ms/req | 3.67, 1.11 | 2.12, 0.83 | 2.99, 0.92 | 3.34, 0.98 |
| keep-alive: 502s | 23 | 0 | 0 | 1 |
| mixed: HTTP rps, p95 ms | 2246, 17.1 | 1833, 27.9 | 1965, 48.9 | 1938, 50.6 |
| mixed: cores | 3.64 | 2.33 | 2.71 | 2.75 |
| mixed: DNS p50 / p95 / p99 ms | 3.35 / 16.3 / 20.1 | 0.96 / 10.3 / 15.8 | 1.11 / 10.4 / 13.6 | 1.01 / 10.3 / 12.7 |
| mixed: DNS cached p99, uncached p50 ms | 8.0, 12.8 | 8.9, 8.1 | 5.5, 8.5 | 4.4, 8.5 |
| mixed: DNS timeouts of 90 000 | 1 | 0 | 0 | 0 |
| ΔRSS MiB after close / keep-alive / mixed | −2 / +4 / +17 | +1 / +5 / +28 | +4 / +12 / +32 | +11 / +14 / +36 |

Transfers, 900 MiB per pass (5 × 100 MiB single, 5 × 8 × 10 MiB parallel).
LAN rows are capped by the router's forwarding path at ~67–70 MiB/s par8 (§Rig);
they compare arms, they do not measure 1 GbE capacity.

| | N=0 | N=2 | N=3 | N=4 |
| --- | --- | --- | --- | --- |
| WAN single median MiB/s | 92.8 (task file) | 95.3 | 92.0 | 91.4 |
| WAN par8 median MiB/s (min–max) | 89.7 (task file) | 89.3 (64–92) | 76.6 (72–91) | 88.6 (69–91) |
| WAN CPU s per 900 MiB | 11.7–12.2 (task file) | 9 | 10 | 11 |
| WAN ΔRSS step, held at +15 min | +56..+60 (task file) | +21, +19 | +30, +32 | +45, +45 |
| LAN single median MiB/s | — | 58.5 | 58.9 | 59.5 |
| LAN par8 median MiB/s | — | 68.0 | 67.4 | 69.6 |

Shutdown, N=4, three 100 MiB fetches at 500 KiB/s in flight:
`/container/stop` 10:27:29Z → three domains log `drain timed out; aborting
the remaining connections open=1` at +5.0 s → `shutting down` → `exited with
status 0` at +5.6 s. Clients: curl exit 18 (partial, ~39 MB of 100). Same
outcome as the task file's N=2 test (5.5 s).

Raw series: `E:/fah-diag/out/alloc/n{0,2,3,4}-*` (`samples.jsonl`,
`http.json`, `dns.json`, `runs.txt`), `shutdown-n4/`.

## Files changed

None in the tree. Dev-box tools: `E:/fah-diag/tools/{connrate.py,dnsload.py,alloc-run.sh,arm.sh,arm-redo.sh,nginx-origin.conf}`.

## Remaining TODOs

- Production swap to `fastadhunter-alloc-6d591ad-rosready.tar` at the default
  N=2; 7-day soak; merge on the verdict.
- 11b (graceful shutdown of keep-alive connections) if the 5 s cut at stop
  ever matters.
- A 1 GbE line-rate origin needs a second LAN port or a host on the other
  side of the router; not needed for the N decision.
