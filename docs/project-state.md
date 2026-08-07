# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-08-07

## Now

| | |
| --- | --- |
| Branch | `feat/phase2-http-pipeline`, last pushed commit `04095d5` |
| Tree | **dirty — p2-09 is written but uncommitted**, ~60 files |
| Tests | workspace green; `fmt`/`clippy` fail **only** in `tui-monitor/` |
| Phase | 2 (`plan/wip/phase2`) — every task implemented; `p2-09` still reads `WAITING` in the phase table |
| Next | the p2-09 on-device check, then the phase-table move and a commit. None of the three are mine to make |

`tui-monitor/` and `fah-top.py` are on this branch and are **off-plan** — a local
monitoring utility, not a phase task.

Build artefacts at the repo root, gitignored (`*.tar`):
`fah-httpbench-arm64.tar` (OCI) and `fah-httpbench-rosready.tar`
(docker-archive, the one shipped to the router). Both rebuildable from
`Dockerfile.httpprobe`.

## Phase 2 status

| Task | State |
| --- | --- |
| p2-00 … p2-08, p2-10, p2-11 | DONE |
| p2-09 | code written and green, **not yet committed**; phase table still says `WAITING` |

`p2-09` was rewritten on 2026-08-06. The former task ("Query Log Reader") would
have made the persisted segments searchable through `GET /api/v1/queries`; it
was blocked because persistence was off on the device. The replacement removes
that endpoint and the query log entirely, and adds one
`GET /api/v1/telemetry` — see
[`p2-09-review.md`](code-review/p2-09-review.md). **The blocker is gone, not
deferred.**

Two things gate the phase move, both the owner's: the on-device check
(§Open items), and moving the row to `DONE`.

## p2-08 — closed on-device 2026-08-06

Report: [`docs/code-review/p2-08-review.md`](code-review/p2-08-review.md), raw
logs in [`code-review/p2-08-http-arm/`](code-review/p2-08-http-arm/).

An in-tree probe container measured the proxy on the RB5009, median of 4 runs,
production FAH serving DNS throughout:

| | min | p50 | budget |
| --- | ---: | ---: | --- |
| Head-path added latency | +161.5 µs | +343.6 µs | < 1 ms ✔ |
| Opaque throughput, 1 MiB | 271 MiB/s | 208 MiB/s | ≥ 100 MiB/s ✔ |
| Blocked vs forwarded | 152 vs 294 µs | 262 vs 582 µs | 48–55 % cheaper |

Three findings worth carrying forward:

- **The dev box was optimistic by 4.4×** (+36.8 µs there, +161 µs here).
- **The ~9× x86 → RB5009 factor does not convert HTTP work** — per-arm ratios
  spread 4.55–10.09×. It stays valid for CPU-bound work only.
- **The verdict's on-device cost is unresolvable** here: the sign flipped across
  the four runs. Only the x86 figure (+0.45–0.71 µs) is quotable.

## The soak — finished, and it closed p2-07

Two windows, one request from the router's own `/history/perf`:

| Window | Span | Sampling | Residual slope (whole / final third) |
| --- | --- | --- | --- |
| T0 process | 2026-08-02T10:18:25Z → 2026-08-04T23:35Z (61.3 h) | 60 s | +0.082 / +0.174 MiB/h |
| Second process | 2026-08-04T23:59:09Z → 2026-08-06T12:11Z (36.2 h) | 360 s | +0.027 / −0.057 MiB/h |

No leak: signs disagree, band is ~37 MiB wide, means agree to 0.4 %. Residual is
**57–58 % of RSS** — the mimalloc-era baseline. Full numbers in
[`p2-07-review.md`](code-review/p2-07-review.md) §12; pass criteria in
[`0.2.10-soak-baseline.md`](code-review/0.2.10-soak-baseline.md).

**The T0 window was cut short by an ISP IPv6 outage** — the owner rebooted and
reconfigured the router while diagnosing it, giving five container starts on
2026-08-04. Cause is external to FastAdHunter. Both windows stand on their own.

## Deployed

**0.2.10** on the RB5009, `mode=dns+http`, steady RSS **46.6 MiB**, compile peak
**181.4 MiB**, 16 lists `ok`, 798,250 compiled rules, `events_dropped_total` 0.

**Three config values are live and differ from the 2026-08-02 baseline:**

- `MIMALLOC_PURGE_DELAY` **100 → 0** — `p2-11`. Unproven under load.
- `history.sample_interval_seconds` **60 → 360** — the perf series is 6× coarser.
- `[query_log]` is still declared in `/config/fastadhunter.toml`. It **must be
  deleted before the next binary starts** — see §Next deploy.

Rollback: the last four version tarballs are on `kingston/`, so reverting is
`/container/add` from the previous tar rather than a rebuild.

## Next deploy is order-dependent

p2-09 removed the `[query_log]` config section. The root `Config` is
`deny_unknown_fields`, so an unknown section is a **boot failure, not a
warning**, and nothing migrates the file.

1. Delete the whole `[query_log]` block from `/config/fastadhunter.toml`.
2. Then start the new container.

Getting the order wrong bricks the resolver until the file is fixed —
`/tool/netwatch` fails DNS over to public resolvers meanwhile, so clients keep
working *unfiltered* and nobody complains. A stale `/data/query_log/` is inert
and can be deleted to reclaim disk.

## Open items

- **HTTP concurrency is unmeasured.** Every p2-08 figure is one connection at a
  time, while `[http] max_connections` defaults to 1024. The DNS side has 20 k+
  QPS of on-device evidence; the HTTP side has none, and Phase 3 multiplies the
  question by TLS.
- **The deployed HTTP path still rests on 5 requests** (the T0 baseline). The
  probe deliberately excludes veth, dst-nat, conntrack and origin RTT. A curl
  loop from a LAN host against an IPv4-only plain-HTTP origin would give it a
  real sample.
- **HTTP interception is IPv4-only.** Mirror `/ipv6/firewall/nat` rules are
  written and unapplied; the soak deferral has expired —
  `0.2.10-soak-baseline.md` §Known gap. Two open points there: `to-ports` support
  on IPv6 dstnat is unverified, and IPv6-only origins are unreachable from a
  ULA-only container.
- `HTTP_ORIGIN_PORT` is hardcoded to 80, and the egress guard allows no other
  port. Correct in production; it means `http_e2e` needs `127.0.0.1:80` and skips
  when it cannot bind it.
- **`MIMALLOC_PURGE_DELAY=0` is unproven above ~0.5 qps.** Watch
  `rate(fastadhunter_process_minor_page_faults_total)` at flat RSS — flat means
  0.2.7's concern was theoretical, climbing means revert to 100. Idle cost
  measured at 6.3 faults/s.
- **~59 MiB of compile ratchet survives `delay=0`** (122 → 181.4 MiB),
  decelerating hard. Suspect is `arena_reserve` = 1 GiB. Worth an experiment only
  if ≤128 MiB becomes the target or Phase 3's footprint makes 75 MB tight.
- **p2-09's on-device check has not been run.** `GET /api/v1/telemetry` against
  the container, confirming every chartable field is populated and agrees with
  the equivalent `/metrics` family read at the same instant.
- **The p2-07 residual moves at the next deploy.** The query log's ring is gone,
  so the `stats` component drops by roughly the ring's accounted size and the
  same bytes reappear in the residual. Expect a step, not a leak; re-baseline
  before reading a slope across the deploy.
- **`heap::string_bytes` documents "Capacity, not length"** but takes `&str` and
  returns `len()`. Under-reports. Unfixed.
- **No regression guard on the compile peak.** Invisible to every automated gate,
  and the dev box cannot help — `process_rss` returns `None` on Windows.
- The mimalloc-vs-`System` A/B to isolate the −17 % CPU from the hit-ratio
  confound.
- Forward rule 4 on the router, "Postgres ACCEPT - FAH", says
  `src-address=172.177.0.2` — a typo for `172.17.0.2`. Harmless; delete rather
  than fix.
- `shrink_to_fit` on the cache map is **still undecided**; needs a
  fill-then-drain test, not a soak.
- The cache byte cap's **eviction path has never fired on-device** — the entry
  cap binds first every time, so p1.5-05's headline criterion is unit-test-only.
- `ListStatus::last_refreshed` reports `null` after a restart alongside
  `last_result: Ok`. Confirmed still true 2026-08-06. Deliberately not fixed —
  reversing it needs RULE_ENGINE.md/API.md review.
- `main` is behind this branch; merge is a fast-forward when the phase closes.
- Any dashboard client can now collapse three pollers into one and drop its
  Prometheus text parsing. Not done for `tui-monitor/`, which also has two
  standing bugs: `history_perf_worker` has no loop, and `parse_summary_data`
  `.or_else()`-chains three guesses at the same field name.

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (os error 10013) when WinNAT has reserved the ephemeral port block
being drawn. **Environmental** — it fails identically on a stashed clean tree.
Do not attribute it to a change.
