# F2 — UDP in-flight cost under an upstream outage, and the admission guard

Follow-up to [project-risk-inventory.md](../project-risk-inventory.md) F2.
Measured 2026-09-11 on the dev box; the device inputs are read-only GETs
against the RB5009's API the same day.

## Summary

- A full black hole (every upstream silent) costs **3.23 s per query** at
  four UDP upstreams and `timeout_ms = 800`, **1.62 s** at two. UDP pays only
  the read leg per server, so the 3-leg bound (9.6 s at four) is not reached.
- **Adaptive does not lower the peak.** Penalties need two failures per
  endpoint and the first lands 800 ms in; every query dispatched before then
  walks the full 3.2 s. Adaptive shortens the sustained tail only.
- Peak in-flight = arrival rate × walk, exactly, linear to 5,000.
- **~8.1 KiB heap and ~7.5 KiB RSS per in-flight query**, flat across rates:
  26 KiB per qps of arrival at the four-upstream walk.
- The household's 30-day peak arrival is 10.5 qps (6-minute mean): 0.3 MiB.
  Filling the 256 MB ceiling above today's 59 MiB RSS needs ~7,300 qps
  sustained through the walk; a RouterOS OOM ~35,000 qps, above the device's
  measured throughput.
- Decision (owner): ship `[dns] udp_max_inflight` as a deployment-level
  guardrail, **default 0 = no cap**, not enabled for the household deployment.

## Decisions

- Shed = drop the datagram unanswered and count it (`shed`); no SERVFAIL
  reply. Cheapest path, and a stub resolver already retries a lost packet.
- Admission before the datagram copy: a shed datagram allocates nothing.
- `0` counts nothing: two branches per datagram, no atomics. Enabled: one
  CAS loop on an `AtomicUsize` `active` (its result is the exact new count;
  `peak` is its `fetch_max`) and one `fetch_sub` from the task's drop guard,
  which rides the socket `Arc` the task already holds. Tokio's `Semaphore`
  was rejected: permit release locks the waiter mutex on every query.
- Telemetry `counters.dns_udp_inflight { active, peak, shed }` is one
  `ArcSwap` snapshot in the registry (the F1 pattern); the gauge read reports
  `max(peak, active)` for the same reason F1 does.
- No non-zero default is derived from this device: the measured household
  workload does not justify a cap, and a cap under a LAN flood sheds
  legitimate queries while the CPU cost stays.

## Measurements

Corpus: unique names `q<n>.<arm>.f2.example.`, A queries, ~40 B each. Workload:
8 s flood per arm at a fixed rate, 10 ms ticks, one client socket, replies
drained and counted; 6 s + walk settle after each arm. Device: dev box
(Windows 11, x86-64), `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`,
mimalloc, real `Server` on `127.0.0.1:0`, real `UpstreamPool`, black holes are
bound UDP sockets nobody reads. In-flight = `RuntimeMetrics::num_alive_tasks`
sampled every 20 ms minus the arm's baseline; heap = live-bytes high-water
from a counting `GlobalAlloc`; RSS = working set from `tasklist` every
250 ms. Allocation counts are process-wide: the harness client's own per-query
work (name `format!`, query encode, reply parse) is inside the figure, so the
`Allocs / query` column is an upper bound on the server's share, not the
server's count. Two runs agreed within 1 %. Harness:
`crates/fah-dns/tests/udp_inflight_cost.rs`, run with
`cargo test -p fah-dns --test udp_inflight_cost -- --ignored --nocapture --test-threads=1`.

| Arm | Rate (qps) | Walk | Peak in-flight | Rate × walk | Heap / in-flight | RSS growth | Allocs / query (process-wide) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| fallback, 2 upstreams | 100 | 1.61 s | 164 | 161 | 8.1 KiB | 1 MiB | 51 |
| fallback, 2 upstreams | 300 | 1.62 s | 490 | 486 | 8.1 KiB | 3 MiB | 50 |
| fallback, 2 upstreams | 1000 | 1.62 s | 1641 | 1620 | 8.1 KiB | 12 MiB | 49 |
| fallback, 2 upstreams | 3000 | 1.61 s | 4945 | 4827 | 8.1 KiB | 36 MiB | 49 |
| fallback, 4 upstreams | 1000 | 3.24 s | 3291 | 3235 | 8.1 KiB | ‡ | 77 |
| adaptive, 4 upstreams | 1000 | 3.23 s | 3253 | 3234 | 8.2 KiB | ‡ | 69 |
| control, answering upstream | 1000 | 2 ms | 10 | 2 | — | 0 | 29 |

‡ 1–3 MiB only because the 3000 qps arm had already grown the working set and
mimalloc reused it; RSS per in-flight query from the ramp arms is ~7.5 KiB.
The first control arm of a run carries ~2.8 MiB of one-time warm-up and is
not a per-query figure.

Device inputs (RB5009, `GET /api/v1/config`, `/telemetry`, `/history/perf`):

| Input | Value |
| --- | --- |
| Upstreams | `adaptive`, 4 × UDP, `timeout_ms = 800`, `penalty_failures = 2` |
| RSS now / lifetime peak | 59 MiB / 141 MiB (uptime 2.3 days) |
| QPS, 30 days, 360 s means | mean 0.97 · p50 0.73 · p99 4.6 · max 10.5 |

Sizing table at 3.23 s × 8.1 KiB = 26 KiB per qps:

| Arrival during a full outage | In-flight | Heap |
| --- | --- | --- |
| 10.5 qps (observed 6-min peak) | 34 | 0.3 MiB |
| 100 qps (10× peak, retry storm) | 323 | 2.6 MiB |
| 1,000 qps | 3,230 | 26 MiB |
| 7,300 qps | 23,600 | 190 MiB — fills the 256 MB ceiling above today's RSS |
| ~35,000 qps | 113,000 | ~900 MiB — RouterOS OOM, above the device's ~20k QPS capacity |

The walk is timer-bound, not CPU-bound, so it transfers to the RB5009
unconverted; the ~9× factor does not apply. Per-query heap is pointer-size
bound and transfers as-is. The 360 s sampling hides bursts inside an interval,
so the observed peak is a floor on the instantaneous one.

## Files changed

- `crates/fah-config/src/schema/dns/mod.rs` — `udp_max_inflight`, default 0.
- `crates/fah-api/src/config_store.rs` — boot key.
- `crates/fah-model/src/engine.rs` — `DnsUdpInflight`, `EngineCounters.dns_udp_inflight`.
- `crates/fah-dns/src/udp.rs` — `UdpInflightGauge`, admission in `run`, tests.
- `crates/fah-dns/src/server.rs` — `Server::bind(&DnsConfig)`, `udp_inflight()`.
- `crates/fah-dns/src/testkit.rs` — `pipeline_with(forwarder)`.
- `crates/fah-metrics/src/registry.rs` — `ArcSwap` snapshot, setter, test.
- `crates/fastadhunter/src/main.rs` — wiring into the 10 s poll.
- `crates/fah-dns/tests/udp_inflight_cost.rs` — the harness (`#[ignore]`).
- `CONFIGURATION.md`, `API.md`, `ARCHITECTURE.md`, `requests/settings.http`,
  `requests/telemetry.http`, `project-risk-inventory.md` F2.

## Remaining TODOs

- None for F2. If a deployment sets the key, read `counters.dns_udp_inflight`
  `peak` and `shed` over a week before trusting the value.
