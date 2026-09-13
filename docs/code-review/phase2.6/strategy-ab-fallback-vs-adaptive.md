# `fallback` vs `adaptive` — controlled A/B before removing `fallback` from `main`

Measured 2026-09-11 on the dev box against `main` at `ad110d8`, the last
commit on which both strategies exist. The decision it supports: `fallback`
is removed from `main` by cherry-picking `fa9451a` (the `p2.6-12` default
flip that already lived on `phase3-06`). After that commit the harness runs
`adaptive` only; the `fallback` rows below are the baseline it is compared
against and cannot be re-measured on `main`.

## Summary

- **1 dead of 4, dead endpoint first in the list** — the case adaptive exists
  for: p50 **831 ms → 31 ms**, taxed answers (≥ `timeout_ms`) **100 % → 4.1 %**.
  The 41 taxed queries are exactly the ones in flight when the second failure
  landed (50 qps × 0.8 s); the tax scales with arrival rate × timeout, not
  with outage length.
- **2 dead of 4**: p50 **1640 ms → 24 ms**; every fallback answer is above 1 s,
  adaptive p95 stays at 33 ms.
- **0 dead, dead-last, all dead**: identical within noise (≤ 0.5 ms). Same
  SERVFAIL rate everywhere: 0 % with any healthy endpoint, 100 % with none,
  same 3.23 s walk.
- **Recovery**: fallback reuses a revived endpoint instantly because it never
  stopped paying for it (750 taxed answers over a 15 s outage); adaptive
  probed it back **7.0 s** after revival (round-1 penalty, 24 s ± 25 %) with
  no client-visible cost meanwhile — the next endpoint served at 18–31 ms.
- No measured axis favours `fallback`.

## Corpus, workload, device

| | |
| --- | --- |
| Build | `main` at `ad110d8`, real `Server` + real `UpstreamPool::from_config` |
| Upstreams | 4 × UDP in-process mocks (`tests/support/mock_upstream.rs`), configured order U1…U4 |
| Config | `timeout_ms = 800`, `penalty_failures = 2` — production values; penalty ladder 24 s, 48 s, … ± 25 % jitter |
| Healthy mock | lognormal latency, median 15 ms, σ 0.3 |
| Dead mock | black hole (no answer; the failure mode of a silent upstream) |
| Traffic | 50 qps, one unique A name per query (cache bypassed), 20 s per arm (1000 queries); recovery arms 60 s (3000) |
| Client | one UDP socket, replies matched by message id, 4.5 s drain after the last send |
| Device | dev box, Windows 11 x86-64, `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]` |
| Order | every scenario runs `fallback` then `adaptive` back to back, fresh mocks and pool per arm |

The walk and the penalty ladder are timer-bound, so the latency figures
transfer to the RB5009 unconverted; the ~9× CPU factor does not apply.

Harness: `crates/fah-dns/tests/strategy_ab.rs`, `#[ignore]`:

```sh
cargo test -p fah-dns --test strategy_ab -- --ignored --nocapture --test-threads=1
```

## Constant failures

Latency in ms over answered queries. "taxed" = answers ≥ 800 ms. Every arm:
1000 sent, 1000 answered, 0 unanswered.

| Scenario | Strategy | p50 | p95 | p99 | max | mean | taxed | ≥ 1 s | SERVFAIL | datagram share U1/U2/U3/U4 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 0/4 dead | fallback | 30.4 | 32.5 | 32.9 | 47.8 | 24.1 | 0 | 0 | 0 | 100 / 0 / 0 / 0 |
| 0/4 dead | adaptive | 30.5 | 32.1 | 32.7 | 47.7 | 24.1 | 0 | 0 | 0 | 100 / 0 / 0 / 0 |
| **1/4 dead, first** | fallback | **830.9** | 846.2 | 850.6 | 863.1 | 833.9 | **1000** | 0 | 0 | 50 / 50 / 0 / 0 |
| **1/4 dead, first** | adaptive | **30.9** | 33.2 | 834.2 | 847.4 | 57.3 | **41** | 0 | 0 | 3.9 / 96.1 / 0 / 0 |
| 1/4 dead, last | fallback | 29.0 | 32.3 | 33.3 | 47.6 | 23.9 | 0 | 0 | 0 | 100 / 0 / 0 / 0 |
| 1/4 dead, last | adaptive | 30.5 | 32.3 | 32.9 | 47.9 | 24.1 | 0 | 0 | 0 | 100 / 0 / 0 / 0 |
| 2/4 dead | fallback | 1640.2 | 1657.3 | 1661.9 | 1670.2 | 1641.3 | 1000 | 1000 | 0 | 33.3 / 33.3 / 33.3 / 0 |
| 2/4 dead | adaptive | 24.1 | 842.7 | 1652.6 | 1674.4 | 123.7 | 82 | 41 | 0 | 3.7 / 7.3 / 89.0 / 0 |
| 4/4 dead | fallback | 3229.4 | 3246.0 | 3255.4 | 3263.7 | 3230.1 | 1000 | 1000 | 1000 | 25 / 25 / 25 / 25 |
| 4/4 dead | adaptive | 3228.9 | 3245.9 | 3251.2 | 3256.9 | 3031.5 | 1000 | 959 | 1000 | 23.4 / 24.5 / 25.5 / 26.6 |

Upstream success rate (mock answered / mock datagrams): dead endpoints 0 %
under both strategies, healthy endpoints 100 % under both. Pool state at the
end of each adaptive arm: every dead endpoint `Penalized`, `penalty_round = 1`,
exactly one penalty, zero probes inside the 20 s (the round-1 deadline is
18–30 s out). In the 4/4 arm adaptive still sends every query to every
endpoint once all are penalized — "every endpoint penalized still sends a
query" — so the walk, the SERVFAIL rate and the latency are the same as
fallback's; the lower mean is the 82 queries answered before the last
penalty landed.

## Recovery

U1 black-holed for 15 s from mock start, then answering; U2–U4 healthy; 60 s
at 50 qps. Samples of the mocks' counters every 100 ms.

| | fallback | adaptive |
| --- | --- | --- |
| overall p50 / p95 / p99 ms | 31.2 / 841.3 / 845.9 | 19.9 / 33.1 / 826.7 |
| taxed answers | 750 — every query during the outage | 41 — onset only |
| U1 datagrams / answered / success | 3000 / 2250 / 75 % | 1943 / 1902 / 97.9 % |
| pool: U1 penalties / probes / probe successes | 0 / 0 / 0 | 1 / 1 / 1 |
| first packet to U1 after revival | +0.0 s (never stopped trying) | +7.0 s (round-1 probe at ≈ 22 s) |
| first answer from U1 after revival | +0.1 s | +7.0 s |
| U1 back at 100 % of traffic | 20–25 s window | 25–30 s window |

Per 5 s window (n = answers, U1 share of upstream datagrams):

| window | fallback p50 / p95 | fallback U1 share | adaptive p50 / p95 | adaptive U1 share |
| --- | --- | ---: | --- | ---: |
| 0–5 s | 829.9 / 844.8 | 50 % ¹ | 31.7 / 842.2 | 14 % ¹ |
| 5–10 s | 830.3 / 846.1 | 50.0 % | 18.7 / 33.0 | 0.0 % |
| 10–15 s | 829.4 / 845.5 | 49.9 % | 17.6 / 33.0 | 0.0 % |
| 15–20 s | 31.1 / 839.6 | 84.7 % | 31.5 / 33.0 | 0.0 % |
| 20–25 s | 21.2 / 33.0 | 100 % | 20.0 / 33.1 | 58.8 % |
| 25–30 s | 19.6 / 33.0 | 100 % | 27.8 / 33.0 | 100 % |
| 30–60 s | 17–31 / 33 | 100 % | 17–31 / 33.5 | 100 % |

¹ The run printed 0 % for the first window (no counter sample precedes
t = 0); the harness was corrected afterwards to use a zero baseline, and the
figures shown are the run's own counts (fallback 1000 datagrams split 50/50;
adaptive 41 of ≈ 290).

Adaptive's recovery delay is bounded by 1.25 × the current penalty round:
≤ 30 s at round 1, doubling per consecutive failed probe to the 300 s cap.
During that delay another healthy endpoint serves at baseline latency; the
delay is not client-visible.

## Reading

- **Where adaptive wins:** every scenario with a dead endpoint ahead of a
  healthy one. The saving per query is one `timeout_ms` per dead endpoint
  ahead, paid by fallback on every query and by adaptive only on the queries
  in flight at the failure onset.
- **Where they are equal:** healthy, dead endpoint behind the serving one,
  all dead. Adaptive adds no measurable latency at 50 qps (p50/p95/p99 within
  0.5 ms of fallback on the healthy arm).
- **What fallback does better:** reuse a revived endpoint instantly. It buys
  that by paying the timeout on every query while the endpoint is dead. No
  client sees the difference: the alternative endpoint answers meanwhile.
- **Client-timeout view:** at 2/4 dead every fallback answer is above 1 s,
  which trips a Windows stub resolver's 1 s first retry and doubles the
  offered load; adaptive stays at 33 ms p95.

## Limitations

- Dev box, in-process mocks, loopback. Not run on the RB5009. The
  timer-bound figures transfer; CPU-bound ones were not the subject.
- One failure mode: silent black hole (timeout). Refused and unreachable
  endpoints, and RCODE isolation, were covered by the 2.6 injected-failure
  arms B.2 and B.6
  ([p2.6-09-injected-failure-bench-review.md](p2.6-09-injected-failure-bench-review.md)),
  not re-run here.
- One rate, 50 qps. The onset tax is rate × timeout per dead endpoint; at
  1000 qps it is ~800 queries per dead endpoint before the penalty lands.
- One outage length in the recovery arm (15 s), chosen so the first probe
  lands after revival. A revival landing between probes waits for the next
  round; a flapping endpoint escalates the ladder (2.6 arm B.5, partial).
- UDP only. DoT/DoH endpoints penalize on the first handshake failure and
  were not measured.
- Single run per arm. The dead-endpoint effects are 25–100× the healthy
  spread, so a second run would not change the reading; the healthy-arm
  numbers carry ±1 ms of box noise.

## Conclusion

`fallback` can be removed from `main`. Adaptive dominates in every scenario
with a live alternative, equals fallback where there is none, and its one
concession (delayed reuse of a revived endpoint) costs no client anything.
The removal is `fa9451a` cherry-picked from `phase3-06`, not new code. That
commit rejects an existing `strategy = "fallback"` in TOML or env at load
(`"fallback" was removed after 0.3.3; "adaptive" is the only strategy`); the
production config already sets `adaptive` explicitly.

## Files

- `crates/fah-dns/tests/strategy_ab.rs` — the harness, kept as an `#[ignore]`
  reproducible regression and capacity corpus (adaptive only after the
  removal).
- This file.
