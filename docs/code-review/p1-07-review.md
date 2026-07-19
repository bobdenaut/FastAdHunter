# Code Review — p1-07 Statistics and Query Log

**Scope:** `crates/fah-stats/**` (whole crate, new: `stats.rs`,
`aggregates.rs`, `bucket.rs`, `top_n.rs`, `client_registry.rs`, `dto.rs`,
`snapshot.rs`, `query_log/{mod,ring,segment}.rs`, `Cargo.toml`, `lib.rs`) ·
**Reviewer:** chief architect pass · **Date:** 2026-07-18 ·
**Status:** findings 1–8 fixed same day (see "Fixes applied" below); 9–12 are
notes/deferrals with no code change required. Gates green.

## What was delivered (implementation report)

`fah-stats` (L3), consuming `fah_model::QueryEvent` off the bounded channel
the DNS pipeline emits into (producer-side drop-on-full + counter already
live in `fah_dns::Pipeline` since p1-04/06; the binary wires the two in a
later task — siblings never import each other):

- **`Stats`** — the crate's one public handle: `record` (sole write path),
  `spawn_collector(mpsc::Receiver<QueryEvent>)`, snapshot + query-log-flush
  schedulers (`tokio::time::interval`, missed-tick `Delay`), and the read
  API for p1-09 (`snapshot`, `query_log`, `clients`, `set_client_name`).
  Sync state behind `std::sync::Mutex`, the segment writer behind
  `tokio::sync::Mutex` (its methods await) — the `ListManager` split.
- **Aggregates** — totals/percentages + top-N domains + hourly buckets, all
  fixed-memory (24-slot rings, capped counters).
- **Client registry** — IP → first/last seen, per-client 24h buckets,
  optional name; capped at 4096, evict-on-full.
- **Query log** — in-RAM `VecDeque` ring (`ring_entries`, oldest evicted)
  with monotonic sequence numbers as the opaque pagination cursor and
  filters per API.md (`client`, `domain` substring, `verdict`, `from`/`to`);
  plus batched append-only **JSONL** segments (implementer's-choice format —
  chosen for `serde_json` reuse and greppability) under
  `/data/query_log/segments/`, `seg-{index:010}.jsonl` so lexicographic =
  chronological, rotation at 1 MiB, retention by age (`retention_days`, file
  mtime) and total size (`retention_max_mb`), never pruning the active file.
- **Snapshot** — aggregates + registry to `/data/stats/snapshot.json` every
  `snapshot_interval_seconds`, atomic tmp+rename (the `fah-config` pattern),
  loaded on boot; corrupt/missing → clean empty start.
- 39 tests at delivery (50 after review): bounded-memory-under-load proofs
  for every structure, retention pruning in tempdirs, snapshot round-trip +
  corrupt-file, kill-and-restart recovery, cursor pagination walk, channel
  consumption end-to-end.

## Overall assessment

Layering is clean (`fah-stats` → `fah-config`/`fah-model` only; the guard
test passes untouched), ADR-0002 is respected (flat files, no DB), the
persistence patterns correctly mirror the codebase's existing atomic-write
convention, and the collector sits off the hot path with the drop-on-full
boundary on the producer side where it belongs. Every data structure ships
with a test proving its memory bound — the right reflex for hard rule 4.

The findings cluster in two places: **one API-contract break** (the stats
payload said "24h" and mostly wasn't) and **one genuine unbounded buffer**
(the flush staging area — ironic in a crate whose theme is boundedness).
The rest is resilience polish at the edges (flood-eviction policy, failed
flush, prune cadence).

## Findings

### 1. HIGH (spec) — `window: "24h"` but totals, percentages and top domains were lifetime counters

API.md `GET /api/v1/stats` declares `"window": "24h"` over the whole
payload, and `top_clients` + `buckets` honored it — but `queries_total`,
`blocked_total`, `blocked_percent`, `cache_hit_percent`, `top_blocked_domains`
and `top_queried_domains` were monotonically-growing since-boot (and, via
the snapshot, since-forever) counters. After a month of uptime the dashboard
would show ~5M "24h" queries next to buckets summing ~100k, and the top
lists would fossilize around whatever dominated week one. Mixing two windows
in one payload that labels itself with one is a contract break, not a nit.

**Fix:** everything derives from 24-slot hourly rings. Bucket slots gained a
`cache_hits` counter so `cache_hit_percent` can window; top-N became
`HourlyTopN` — one capped `BoundedCounter` per hour slot (256 domains/slot,
worst case 24×256 keys per list, still fixed), slots reset on hour-reuse
exactly like `HourlyBuckets`, reads merge the fresh slots. Lifetime scalars
deleted — single source of truth. (Prometheus-style monotonic totals are
p1-08's job, `fah-metrics`, per CONTEXT.md's Statistics/Metrics split.)

### 2. HIGH (hard rule 4) — `pending_log` was an uncapped `Vec`

The staging buffer between `record` and the periodic segment flush had no
bound. Normal operation self-limits (flush every 5 s), but a stalled disk, a
flush task that panicked, or a binary that simply never spawned the
scheduler turns it into memory that grows with traffic — precisely what
hard rule 4 forbids, in the one crate whose whole design brief was "bounded
everything". The ring next to it is capped; this was the only unbounded
container in the crate.

**Fix:** capped at `MAX_PENDING_LOG = 16_384` (~3k QPS × 5 s flush, several
× the RB5009 budget); overflow drops newest (they remain visible in the
ring) and counts into a new `Stats::query_log_overflow_dropped()` for
p1-08's metrics. Test proves the cap and the count with the flusher never
running.

### 3. MEDIUM — a failed segment flush silently discarded the batch

`flush_query_log` `mem::take`s the pending batch *then* appends; on I/O
error it logged a warning and dropped the entries on the floor. A transient
error (volume briefly read-only, ENOSPC race) meant permanent loss of
entries that were sitting safely in RAM.

**Fix:** on append failure the batch is re-queued in front of whatever was
recorded meanwhile and retried next interval; the finding-2 cap still
applies during re-queue (oldest dropped, counted), so a *persistent* disk
failure degrades to bounded, counted loss instead of either silent loss or
unbounded growth. Test blocks the segment directory with a file, asserts
retention across the failed flush, unblocks, asserts persistence.

### 4. MEDIUM — a client flood evicted the user's named devices

Registry eviction was pure LRU by `last_seen`. Under a spoofed/scanning
source-IP flood — the exact scenario the cap exists for — every flood packet
is a "new client" whose insert evicts the least-recently-seen entry: the
household's real (briefly idle) devices, names included, while flood entries
churn each other. User-assigned names are the registry's only irreplaceable
state (`PUT /api/v1/clients/{ip}`).

**Fix:** eviction key is now `(has_name, last_seen)` — unnamed
least-recently-seen first; a named device can only fall out when the entire
registry is named. Test: named-but-oldest survives, fresher unnamed entry is
evicted.

### 5. LOW (spec) — `domain` filter was case-sensitive against lowercased data

The pipeline lowercases domains before emitting events (p1-05), so a
`GET /api/v1/queries?domain=Ads` filter could never match. **Fix:**
`Stats::query_log` normalizes the needle once (`QueryLogFilter::normalized`)
— no per-entry allocation in the match loop. Test with an uppercase needle.

### 6. LOW (efficiency) — retention pruned on every 5 s flush

Each flush re-scanned the segment directory (up to ~500 files at the size
cap) and stat'd every file, ~17k scans/day for a condition that changes at
most hourly. **Fix:** `SegmentWriter::maybe_prune` — prune on rotation (the
only moment total size crosses a segment boundary), on the first flush
after boot (previous runs' files may have aged out), and at latest hourly
(the age cap is a privacy control; it must advance even with no traffic).
Test covers all three arms.

### 7. LOW — snapshots pinned dead capacity values

`ClientRegistry` (and the counters) serialized their `capacity` field, so a
snapshot written by an old build would resurrect the old cap forever, even
after the constant changed. **Fix:** registry capacity is `#[serde(skip)]`
with the current constant as default; eviction loops became `while` so an
over-cap deserialized load converges instead of hovering above the bound.
(Top-N slots self-heal within 24h by resetting hourly — no change needed
beyond the loop.)

### 8. LOW (efficiency) — needless per-event `String` clone

`Stats::record` cloned `event.query.domain` only to appease a borrow that a
reorder resolves. Collector path, not hot path, but a free win. **Fix:**
borrow directly; clone gone.

### 9. NOTE (record the deferral) — `[stats]`/`[query_log]` keys are class "runtime" but config is baked at construction

Same shape as p1-05's finding 6: CONFIGURATION.md marks every key runtime,
`Stats::new` bakes them in, and nothing can deliver a runtime change until
p1-09. Rebuild-and-swap at config-apply time is an acceptable
implementation. **p1-09 must pick this up** — recorded here so the contract
isn't silently broken.

### 10. NOTE (no change) — ring sequence restarts at 0 after reboot

Persisted JSONL segments can therefore contain repeated sequence numbers
across runs, and a cursor held across a restart is only harmlessly stale
(the ring is RAM-only and starts empty). Segments are an archive, not an
index; if a future consumer needs global ordering it should key on
timestamp. Not worth an I/O read-back of the last segment at boot.

### 11. NOTE (no change) — eviction scans are O(capacity)

`BoundedCounter` (≤256/slot) and the registry (≤4096) find their eviction
victim by linear scan on new-key-at-capacity inserts. Worst case is a
random-subdomain flood driving one scan per event — on the collector task,
off the hot path, bounded CPU, and the counter caps shrank 8× with
finding 1's fix. A heap buys back little here for real complexity.

### 12. NOTE (no change) — snapshot write is atomic but not fsync'd

tmp+rename protects against a torn file on kill -9 (the acceptance
scenario); a power cut can still lose the rename or leave a corrupt file,
which `load` already treats as a clean empty start. Worst case = losing one
300 s window of *statistics*. Matches the codebase's existing atomic-write
convention; fsync is not worth the flash wear on the RB5009.

## Verdict

The skeleton is right: correct layer, correct persistence philosophy, drop
semantics at the correct (producer) boundary, and bounded-memory proofs as
tests rather than comments. But it shipped with the stats payload
contradicting its own `window` label (finding 1) and one container that
violated the crate's founding rule (finding 2) — both worth catching before
p1-08 exports these numbers and p1-09 serves them. The remaining fixes are
the difference between "works on the happy path" and "survives the exact
floods and disk faults the caps exist for." All eight are in; 9 is an
obligation on p1-09, recorded so it doesn't evaporate.

## Fixes applied (2026-07-18)

1. **24h window honored everywhere** (`bucket.rs`, `top_n.rs`,
   `aggregates.rs`, `stats.rs`) — bucket slots track `cache_hits`; new
   `HourlyTopN` (per-hour capped counters, hour-reuse reset, fresh-slot
   merge on read); `Aggregates` reduced to buckets + two `HourlyTopN`s, all
   getters take `now`; lifetime scalars removed. New tests:
   `everything_is_windowed_to_the_last_24h`,
   `hourly_top_excludes_slots_older_than_24h`,
   `hourly_top_merges_counts_across_hours`,
   `hourly_slot_reused_a_day_later_resets`,
   `hourly_top_stays_bounded_under_many_unique_keys`.
2. **Pending batch capped** (`stats.rs`) — `MAX_PENDING_LOG = 16_384`,
   drop-newest + `query_log_overflow_dropped()` counter. New test
   `pending_log_stays_bounded_when_flush_never_runs`.
3. **Failed flush re-queues** (`stats.rs`) — batch restored ahead of newer
   entries, cap enforced during re-queue. New test
   `failed_flush_keeps_entries_for_the_next_attempt`.
4. **Name-aware eviction** (`client_registry.rs`) — victim =
   min `(has_name, last_seen)`. New test
   `eviction_spares_named_clients_over_fresher_unnamed_ones`.
5. **Case-insensitive domain filter** (`query_log/mod.rs`, `stats.rs`) —
   `QueryLogFilter::normalized()` lowercases the needle once per request.
   New test `domain_filter_is_case_insensitive`.
6. **Prune cadence** (`segment.rs`, `stats.rs`) — `append` reports rotation;
   `maybe_prune(…, rotated)` scans on rotation / first-after-boot / hourly.
   New test `maybe_prune_skips_the_scan_until_rotation_or_interval`.
7. **Capacity not pinned by snapshots** (`client_registry.rs`, `top_n.rs`) —
   `#[serde(skip)]` + default-constant on the registry cap; eviction `if` →
   `while` in both structures so over-cap loads converge.
8. **Per-event clone removed** (`stats.rs`) — direct borrow of
   `event.query.domain`.

Also picked up: the missing `from`/`to` filter coverage from the task's own
test list (`filter_by_time_range` in `ring.rs`).

**Verification:** `cargo fmt --check` clean;
`cargo clippy --workspace --all-targets -- -D warnings` clean;
`cargo test --workspace` all green — fah-stats 50 tests (was 39: +11 new),
zero failures elsewhere in the workspace (layering guard included).
