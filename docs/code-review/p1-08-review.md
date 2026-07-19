P1-08 is done — fah-metrics now has a working registry, encoder, and bench, with all workspace gates green.

What was built:

Metrics registry (registry.rs): atomic counters for queries by verdict, cache hit/miss/stale, plus three latency histograms split by pipeline stage (block / cache_hit / forward — mapping directly onto PERFORMANCE.md's three p99 budget rows). Consumes the same QueryEvent channel fah-stats does.
Histogram (histogram.rs): fixed-bucket, lock-free, allocation-free.
Snapshot DTOs (upstream.rs, ruleset.rs) — this crate's own types, not fah-dns/fah-rules types, since siblings can't import each other. fastadhunter (not yet wired — that's p1-09's job) will poll Pipeline::dropped_events(), UpstreamPool::status(), and Matcher::len()/heap_bytes() and push snapshots in via set_dropped_events/set_upstreams/set_ruleset.
process.rs: RSS gauge read straight from /proc/self/status (Linux-only target, no new dependency).
encode.rs: hand-written Prometheus text exposition encoder (justified over the prometheus crate — a small, fixed instrument set doesn't need its protobuf/quantile machinery).
Bench (benches/record.rs): Metrics::record measured at ~18ns/event on this dev x86_64 machine — the plan's "single-digit ns" is an aspirational ballpark; the real cost here is 5-6 atomic RMW ops (verdict + cache + 3 histogram atomics), which no allocation-free design change removes. Still ~5 orders of magnitude under the <1ms budget.
Gates (fmt, clippy -D warnings, test --workspace) are all green, including the layering guard test. Marked p1-08 DONE in plan/wip/phase1/CLAUDE.md.

Not done yet: wiring fah-metrics into fastadhunter's main.rs — the binary doesn't wire any sibling crates together yet, and that wiring naturally belongs to p1-09 (the API task, which needs handles to everything to serve /metrics, /health, etc. anyway).

p1-09 (Full API.md surface) is next and is flagged "heavy"/Opus in the phase table — let me know when you want to move on to it.

----------

What shipped
The layering question resolved as trait ports, no doc changes. fah-api declares StatsSource/TelemetrySource and never imports its L3 siblings; adapters.rs in the binary bridges them. The guard test at layering.rs passes untouched.

fah-api (~2,900 lines): rcgen/rustls TLS with PEM replacement, bearer auth with metrics_public exemption, all 15 API.md endpoints, WS events over a broadcast hub with slow-consumer disconnect, config deep-merge with boot/runtime classification and TOML write-back. 73 tests (45 unit + 28 integration over real HTTPS).

Prerequisites the task didn't mention but needed: lists CRUD required restructuring ListManager (fixed Vec → RwLock + add/remove/update, and per-list scheduler tasks → one interval-driven scheduler). Config gained Config::save/validate and per-list refresh_hours.

Binary wiring: DNS listeners + rule scheduler + stats/metrics/WS fan-out off one QueryEvent channel + API server, all in Engine::start.

Two things worth your attention
1. A real bug the tests caught. My first per-line rule validator parsed each line in isolation, which flagged /ads/banner.gif as invalid — it's a valid adblock URL pattern, but alone it looks like a malformed domain. Fixed properly: the parsers now record bounded error line numbers (capped at 100 — a corrupt 1M-line list must not allocate megabytes of error data), so validation parses the block as one unit.

2. A deviation from API.md I found only by running it. Domains came back as "ads.example.com." (wire form) where API.md documents "ads.example.com". I fixed it at fah-api's presentation boundary rather than changing what the DNS engine stores — that trailing dot is load-bearing inside fah-dns, and p1-04 is already DONE. Live-verified after the fix.

Verified end-to-end, not just green tests
Booted the real binary on ports 15353/18443 (deliberately not 53/8443 — no need to touch AdGuard Home), then: curl without -k refused → with -k served; /api/v1/* 401 without key; a real UDP DNS query for a rule I'd just PUT through the API returned 0.0.0.0 in 0.0225 ms; stats/metrics/query-log all reflected it; cert SHA256 identical across a restart with no key re-announcement.

Two notes on what you saw: the SERVFAIL for the unblocked domain was upstream 1.1.1.1/9.9.9.9 unreachable from the sandbox (4s = two servers × 2s timeout — correct fallback), and stats reading 0 after restart is the 300s snapshot interval, not data loss.

Not done, deliberately
Runtime config changes persist and report applied, but nothing re-reads them live yet — no crate exposes reconfiguration hooks. Compile-duration gauge stays at zero (the lifecycle doesn't time compiles). Full-stack DNS→WS integration tests belong to p1-10.

p1-10 (benches vs PERFORMANCE.md budgets + integration tests) is next. Want me to continue, or review this first? Nothing is committed.