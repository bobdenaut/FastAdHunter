# p1-08 — Prometheus Metrics (`fah-metrics`): RC review

Read-only review of the crate as it stands at Phase 1 feature-complete,
reviewed against the p1-08 task file, PERFORMANCE.md, and the actual wiring
in `fastadhunter` (`main.rs` fan-out + telemetry poller, `adapters.rs`).

**Verdict: sound.** The hot path is genuinely lock-free and allocation-free,
the layering discipline (own DTOs, no sibling imports) is respected, and the
hand-rolled encoder is the right call for a fixed instrument set. No Critical
findings. One Major finding (serve-stale events corrupt the budget-tracking
histogram) is a one-line fix and worth taking before Phase 2; the rest can be
deferred or batched opportunistically.

> **Resolution (2026-07-21):** all fixable findings applied — see
> [§Resolution](#resolution-2026-07-21) at the end for what changed per
> finding. Gates green (fmt, clippy `-D warnings`, 350 workspace tests).

## What was built / shipped

- **Registry** (`registry.rs`) — the one `Metrics` instance, shared via
  `Arc`: atomic counters for queries by verdict (pass/allow/block) and cache
  hit/miss/stale, plus three latency histograms split by the pipeline stage
  that answered (`block` / `cache_hit` / `forward`), mapping onto
  PERFORMANCE.md's three p99 budget rows. `record(&QueryEvent)` is the
  hot-path entry point — 5–6 relaxed atomic RMWs, no lock, no allocation.
- **Histogram** (`histogram.rs`) — fixed 11-bucket (100 µs – 100 ms),
  non-cumulative atomic slots; cumulative counts and the implicit `+Inf`
  bucket are computed at encode time, matching Prometheus `le` semantics.
- **Snapshot DTOs** (`upstream.rs`, `ruleset.rs`) — this crate's own types
  (siblings never import each other); the binary polls
  `Pipeline::dropped_events()`, `UpstreamPool::status()`, and
  `Matcher::len()/heap_bytes()` every 10 s (`TELEMETRY_POLL`) and pushes them
  in via `set_dropped_events` / `set_upstreams` / `set_ruleset`, the latter
  two behind `ArcSwap`.
- **Encoder** (`encode.rs`) — hand-written Prometheus text exposition (0.0.4)
  with HELP/TYPE lines, label escaping, conditional omission of the upstream
  family until a snapshot exists, and the standard
  `process_resident_memory_bytes` gauge. Served verbatim by fah-api's
  `GET /metrics` through the binary's `TelemetryAdapter`.
- **RSS gauge** (`process.rs`) — `/proc/self/status` `VmRSS` parse, Linux
  target only, zero-fallback elsewhere; no new dependency.
- **Bench** (`benches/record.rs`) — `Metrics::record` at ~18 ns/event on the
  dev x86_64 box (the task's "single-digit ns" was aspirational; the floor is
  the 5–6 atomic RMWs, and 18 ns is ~5 orders of magnitude under budget).
- **Tests** — counter/stage-routing/snapshot-replacement correctness,
  HELP/TYPE golden coverage per family, cumulative-bucket + `+Inf`
  semantics, label escaping, `/proc` parsing.
- Deps: `fah-model` + `arc-swap` + tokio `sync`/`rt` only. Gates green.

## Findings

### Critical

None.

### Major

**M1. Serve-stale events land in the `cache_hit` histogram, breaking the
budget signal exactly during outages.**
`Metrics::record` (`registry.rs:86-93`) routes any non-blocked event with
`cache_hit == true` to `duration_cache_hit`. But a stale serve
(`cache_hit=true, stale=true`) only ever happens *after a forward attempt
failed* (`pipeline.rs` `resolve`), and the event's duration is
`started.elapsed()` — which includes the full upstream timeout, 2–4 s with
the current two-server fallback. So during an upstream outage, multi-second
samples pour into the histogram whose finite buckets top out at 100 ms and
whose p99 tracks the "< 1 ms verdict + cache hit" budget. The dashboard then
reports a catastrophic cache-hit latency regression at precisely the moment
an operator is investigating an outage — a false signal from the one
instrument built to detect budget breaches. The struct comment ("their
`cache_hit`/`stale` are always false; the stage split relies on that") shows
the mutual-exclusivity reasoning stopped at blocked queries and missed the
stale case.
*Fix:* route `event.stale` to `duration_forward` (whose samples already
include upstream wait) — a one-line reorder of the `if` chain — or give
stale its own stage label. Adjust the stage-routing test.
*Recommendation:* **fix before Phase 2.** One line in this crate, no API
change; on-device it's masked only because the RB5009's upstreams have been
healthy. (Related, lesser: `cache_hits_total` also counts stale serves, but
`cache_stale_total` is exposed separately so the ratio can be corrected in
PromQL — acceptable.)

### Minor

**m1. The `forward` histogram doesn't measure what the docs claim it does.**
`registry.rs:30-32` says the three histograms match PERFORMANCE.md's budget
rows "exactly", and the HELP text says "In-engine query latency". The forward
budget row is *"Forwarded query overhead added by engine, p99 < 1 ms"* — but
the recorded duration is end-to-end including upstream RTT (typically
10–50 ms, 2 s+ on timeout). The forward histogram therefore cannot be
compared against its budget row; anyone alerting on it against 1 ms gets
permanent false positives. Measuring true overhead needs pipeline-side
timestamps around the upstream await — real work, not this crate's.
*Recommendation:* fix the doc comment and HELP text now (minutes); defer the
overhead measurement to Phase 2 if wanted at all.

**m2. `Metrics::spawn_collector` is dead production code with a stale doc
comment, duplicated in `fah-stats`.**
The binary's `spawn_event_fanout` (`main.rs:320`) consumes the single
`QueryEvent` channel and calls `metrics.record()` directly; nothing outside
this crate's own test calls `spawn_collector`. Its doc comment
(`registry.rs:111-114`, "`fastadhunter` creates one bounded channel per
consumer") describes the pre-p1-09 design that the fan-out superseded — an
agent reading this crate first will wire the next consumer wrong.
`fah_stats::Stats::spawn_collector` is the same dead pattern (p1-07's crate,
noted here because consolidation is the point).
*Recommendation:* delete both (public API nobody calls is a maintenance
liability, and the crates are unpublished so removal is free) or at minimum
rewrite the comments to describe the fan-out. Cheap; do before Phase 2 so
Phase 2 agents don't inherit the misleading docs.

**m3. Torn reads across the histogram's atomics can emit transiently invalid
exposition.**
`buckets`, `count`, and `sum_nanos` are independent `Relaxed` atomics, and
`write_histogram` reads them at different instants (`cumulative_counts()`,
then `count()` for `+Inf`, then `sum_seconds()`, then `count()` *again* for
`_count`). A scrape racing `observe()` can print `+Inf` smaller than the last
finite bucket (invalid monotonicity), or `_sum`/`_count` from different
generations. With the single-writer fan-out the window is nanoseconds and the
next scrape self-corrects, but `Relaxed` gives no cross-variable ordering on
the ARM target, so it is not purely theoretical.
*Recommendation:* cheap hardening, not urgent — read `count` once per
histogram, reuse it for `+Inf` and `_count`, and clamp `+Inf` to at least the
last cumulative bucket. Fine to defer to a Phase 2 tidy-up.

**m4. `compile_duration_seconds` is not just unimplemented — the current
shape prevents it from ever working.**
Known defect (recorded in the phase table): the gauge is hardcoded to zero.
The API-design root cause is worth naming: compile time is event-shaped data
(it changes when a compile happens) forced into a poll-shaped snapshot. The
poller (`main.rs:373-379`) overwrites the whole `RulesetSnapshot` with
`Duration::ZERO` every 10 s, so even a future lifecycle hook that sets it
would be erased within a tick — and the adjacent comment ("the gauge stays at
its last set value") is simply wrong about the code below it.
*Recommendation:* defer the feature to Phase 2 as planned, but when it lands,
move `compile_duration` out of the polled snapshot (own setter, written by
whoever times the compile). Correct the misleading `main.rs` comment now.

**m5. Allocation churn in `encode()`.**
Per line: a `Vec<String>` + `join` for labels (`writeln_metric`); per bucket
line: `format!("{name}_bucket")`; per label value: up to three intermediate
`String`s in `escape_label_value` even when nothing needs escaping. That's
roughly 150–200 small allocations per scrape. This is the scrape path, not
the hot path, so no budget is at risk — but it's trivially avoidable by
writing labels straight into `out` with `write!` and using `Cow` for the
escape, and this is the crate whose header preaches allocation-free
discipline.
*Recommendation:* defer; batch with m3 if the encoder is touched anyway.

### Nitpick

**n1.** `RulesetSnapshot`'s manual `Default` impl (`ruleset.rs:16-24`) is
exactly what `#[derive(Default)]` produces.

**n2.** `write_help_type` doesn't escape HELP text (the spec requires `\\`
and `\n` escaping). All current literals are safe; it's a latent foot-gun for
the next metric added.

**n3.** `UpstreamSnapshot.protocol: &'static str` bakes `'static` into a
public DTO. Correct today (protocols are a closed set of literals) and it
keeps snapshots allocation-light, but it forces churn if protocol strings
ever become dynamic. Fine as is — noting for awareness.

**n4.** The bench replays one identical event, so branches are perfectly
predicted and the ~18 ns understates real mixed-traffic cost somewhat;
combined with unpinned criterion on the dev box, treat the figure as a
ballpark, not a budget line.

**n5.** `events_dropped_total` HELP text says "a consumer channel
(stats/metrics) was full" — since p1-09 there is one fan-out channel; the
wording predates the rewiring. Fold into m2's doc pass.

## What's deliberately fine (reviewed, no action)

- Hand-rolled encoder over the `prometheus` crate: right trade for ~10 fixed
  families; the crate would add dependencies for machinery this registry
  never uses.
- Linear `iter().position` bucket search over 11 entries: optimal at this
  size, no binary search wanted.
- `u128 → u64` nanosecond truncation in `observe`: correctly reasoned SAFETY
  comment (~584-year single duration to overflow).
- Conditional omission of the upstream family until the first snapshot:
  Prometheus handles appearing series; behavior is tested.
- Counters exposed through `f64`: exact below 2^53 — unreachable at RB5009
  query rates.
- `/proc/self/status` parse with zero-fallback and cfg-gated tests: correct
  for a distroless Linux-only deployment target.

## Recommended before closing Phase 1

1. **M1** — stale events out of the `cache_hit` histogram (one line + test).
2. **m1 (doc half)** + **m2** + **n5** — one small doc/dead-code pass:
   delete or re-document `spawn_collector` (both crates), fix the histogram
   stage comment and the two HELP strings, fix the wrong `main.rs` comment
   from m4.

Everything else defers to Phase 2 without risk.

## Resolution (2026-07-21)

All findings addressed in the same session; gates green
(`fmt --check`, `clippy -D warnings`, 350 workspace tests, 0 failures).

- **M1 — fixed.** `record()` routes stale serves to the `forward` stage
  (`cache_hit && !stale` guards the cache-hit histogram); regression test
  `stale_serve_records_into_the_forward_stage` added. Hit/stale counters
  intentionally unchanged.
- **m1 — doc half fixed, measurement deferred.** Histogram field doc and the
  `query_duration_seconds` HELP now state that `forward` is end-to-end
  including the upstream round trip and not comparable to the "overhead added
  by engine" budget row. True overhead measurement stays Phase 2 (needs a
  pipeline-side timer).
- **m2 — fixed.** Both dead `spawn_collector`s deleted (`fah-metrics`,
  `fah-stats`) along with their tests and stale channel-per-consumer doc
  comments; crate docs now describe the binary's single-channel fan-out.
  Side benefit: `fah-metrics` no longer depends on tokio at all.
- **m3 — fixed.** `write_histogram` reads the count once, shares it between
  `+Inf` and `_count` (spec requires them equal), and clamps it to the last
  finite bucket so a racing scrape can't emit non-monotonic buckets.
- **m4 — comment fixed, feature deferred.** The wrong `main.rs` comment now
  states the actual behavior (the poll overwrites the gauge with zero every
  tick) and directs the Phase 2 implementation to an event-driven setter
  outside the polled snapshot.
- **m5 — fixed.** `writeln_metric`/label escaping/HELP lines now write
  straight into the output `String`; per-scrape allocations are down to one
  `_bucket`/`_sum`/`_count` name and one reused bound buffer per histogram
  plus the three `cumulative_counts` Vecs.
- **n1 — fixed.** `RulesetSnapshot` derives `Default`.
- **n2 — fixed.** HELP text is escaped per spec (`\\`, `\n`); test added.
- **n3 — no change, by design.** `&'static str` protocol stays; revisit only
  if protocols become dynamic.
- **n4 — fixed.** A `record mixed events` bench cycling all four event shapes
  was added alongside the single-event ones.
- **n5 — fixed.** `events_dropped_total` HELP now describes the single
  fan-out channel.

**Bench note.** Criterion initially reported `record` at ~47 ns vs the stored
~18 ns baseline (+160%). An A/B on today's machine state (stash → bench old →
restore) showed the *unchanged* code also at ~46.5 ns — the delta is
environmental drift on the unpinned dev box, exactly the failure mode
documented for this machine. The fix itself costs nothing measurable
(47.6 ns vs 46.5 ns, within noise), as expected for one extra predictable
branch.
