# P2-09 — Query Log Reader (make the persisted segments searchable)

**Phase:** 2 · **Depends on:** p1-07 (query log tiers), p1.5-04 (history read
endpoints, precedent for range-scoped reads) · **Model:** Opus

## Goal

Make the on-disk query-log segments readable, so `GET /api/v1/queries` can
answer a search across the full `[query_log] retention_days` window instead of
only the in-RAM ring.

**The implementation must preserve the existing DNS fast path. A live read must
never perform filesystem I/O.** That is the constraint the whole design serves
— the routing rule, the semaphore, and the refusal to union the two tiers all
exist to hold it.

The following constraints are non-negotiable. Each one has a test in the
acceptance criteria.

**Routing.** `limit` and `cursor` are pagination, never filters. A request
carrying only those is served from the ring and touches no file. Let that drift
and a dashboard's live-tail poll becomes a disk read on every refresh.

**Locking.** The reader must not hold `Stats::segment` (the `SegmentWriter`'s
`tokio::sync::Mutex`) for the duration of a scan. Take the segment list,
release the lock, read the files independently. Holding it blocks
`flush_query_log`, which backs up `pending_log`, which hits `MAX_PENDING_LOG`
and **silently drops query-log entries** — a search that destroys the data it
is searching.

**CPU.** Concurrent scans are bounded by a semaphore. The box has four cores
and DNS has first claim on them; an unbounded scan count is a self-inflicted
denial of service on the resolver.

## Why this exists

The query log has two tiers and only one of them has ever been readable:

| Tier | Written by | Read by |
| --- | --- | --- |
| `Ring` (in-RAM, `ring_entries` = 10 000) | `Stats::record` | `GET /api/v1/queries` |
| `/data/query_log/segments/seg-NNNNNNNNNN.jsonl` | `SegmentWriter` | **nothing** |

So the system retains 7 days of per-query detail on the SSD and can search
roughly 50 minutes of it. On the deployed RB5009 at ~0.86 QPS the ring covers
~2.8 h; at 85 QPS it covers ~2 minutes.

Worse than the limit is how it fails. A range older than the ring returns an
empty `items` array with `200`, so "nothing found" is indistinguishable from
"nothing happened" — the API states this as permanent behaviour today
(API.md §`GET /api/v1/queries`, CONFIGURATION.md §`[query_log]`). This task
removes the limitation and those two doc claims with it.

The phase 1.5 `CLAUDE.md` already carries the correction that named this gap:
*"nothing reads those segments … the persisted segments are write-only until a
segment reader exists."* This is that reader.

## Settled design — do not re-litigate

These were argued to a conclusion with the user. Each row has its reason;
change one only with a reason that beats it.

| Decision | Reason |
| --- | --- |
| **One endpoint**, `GET /api/v1/queries`. No `/history/queries`. | The ring is a cache of the tail of the segments, not a second dataset. Two endpoints would force the caller to know `ring_entries` and current QPS to pick one — internals leaking into the contract. |
| **No filters → ring. Any of the six semantic filters → segments.** | "Started filtering" = "started searching". A rule the user can hold in their head, with no ambiguous middle. |
| `limit` / `cursor` are **not** filters | They are pagination. `?limit=50` must stay a live read and must never touch disk. |
| **No union at the tier boundary** | A search that reaches past the ring misses the ≤ `flush_interval_seconds` (5 s) still sitting in `pending_log`. Accepted: real-time is the WebSocket's job, and merging two sources for a 5 s window is not worth the de-duplication code. |
| **Linear scan. No index, no auxiliary files, no embedded DB.** | 500 MB on a Kingston XS1000 is seconds, and an admin UI may answer in 2 s where DNS may not exceed 1 ms. Extends ADR-0002 rather than contradicting it. |
| Cursor stays **`sequence`** | Already monotonic, already exclusive-and-newest-first in `Ring::query`, already written to every JSONL line. Valid in both tiers, so a cursor never goes stale mid-pagination. |
| `next_sequence` **derived at boot**, not persisted periodically | Persisting a counter every N seconds has a crash window: restart resumes from a stale value and *reissues* sequences already on disk. Duplicates break pagination silently. Gaps do not — a cursor needs ordering, not density. |
| **Routing lives inside `fah-stats`** | `StatsSource::queries()` already takes the whole filter set. Tier selection is an implementation detail, and implementation details do not belong in `fah-api`. No new port. |
| Response carries **`oldest_retained`**, never the tier | Mechanism is internal; *completeness of the answer* is public. `retention_days` is configurable, so the UI's date picker needs the real bound rather than a hardcoded 7. |
| Tier goes to **metrics + `debug!`**, not the response body | Naming `ring` / `segments` in the schema would version-lock the storage design for no user benefit. |

## Scope

### 1. `QueryLogReader` — `crates/fah-stats/src/query_log/reader.rs`

New sibling of `segment.rs`, same module, same crate. Reads what
`SegmentWriter` writes; the shared contract is the **`QueryLogEntry` type
itself**, which already derives `Deserialize` — the decode side exists and is
unused. Do not define a second row struct or a second filter type.

Scan strategy, in this order:

1. **Newest segment first.** `seg-{index:010}.jsonl` sorts lexicographically =
   chronologically. Walk descending.
2. **Skip a segment by time bound.** Read its first line, take
   `event.query.timestamp`; if it is newer than `to`, or the segment's last
   line is older than `from`, close it without reading the rest.
3. **Reject on raw bytes before parsing.** For a `domain` filter, search the
   line bytes for the needle before handing it to `serde_json`. Parsing is the
   bottleneck, not the disk — this ARMv8 core parses far slower than the SSD
   reads: single-threaded work measures **~9× slower than the dev box**
   (PERFORMANCE.md §Budgets). Only parse lines that survive the prefilter.
4. **Stop at `limit`.** With reverse order this is what keeps the common case
   cheap: `?verdict=block&limit=20` at a 55 % block rate reads ~36 rows and
   returns. A full scan only happens for a filter with few or no matches.
5. **Bound concurrency.** A semaphore (1–2 permits) around segment scans, so
   repeated searches cannot starve the DNS path of CPU.

Tolerate damage the same way throughout: `append` does `write_all` with **no
`fsync`** ([`segment.rs`](../../../crates/fah-stats/src/query_log/segment.rs)
`append`), so a power loss can leave a torn line — and once retention starts
pruning, an unreadable file is never worth failing a search over. The rule is
one line and applies everywhere in this task: **a line that does not
deserialize is skipped, a segment that yields no usable line is skipped, and
neither is an error.** Count skips and log them at `warn!` once per scan so
silent data loss is still visible.

### 2. Derived `next_sequence`

`Ring::new` starts at `0` and nothing restores it, while `SegmentWriter::boot`
*does* resume its file numbering. So sequence numbers repeat after every
restart, and pagination across a restart boundary would jump or loop.

The algorithm, stated rather than described by its cases:

```text
for segment in segments, newest → oldest:
    for line in segment, last → first:
        if line deserializes into a QueryLogEntry:
            return entry.sequence + 1
return 0
```

Every damage case falls out of that rule instead of needing its own branch: a
torn final line is skipped by the inner loop, a zero-length or wholly corrupt
segment is skipped by the outer one, and an empty `/data/query_log/segments`
returns `0`. Do not implement it as "read the last line, and handle the
exceptions" — that is the same algorithm with more places to be wrong.

`query_log.enabled = false` → `0`, without touching the directory.

Same shape as `SegmentWriter::boot`, one more step in the same startup path.
Cost is one ≤ 1 MiB read at boot in the normal case. Startup is already
~2 440 ms on device.

### 3. Unified `GET /api/v1/queries`

Routing table, evaluated in `fah-stats`:

| Request | Source |
| --- | --- |
| no semantic filter (with or without `limit` / `cursor`) | ring |
| any of `domain`, `client`, `verdict`, `qtype`, `from`, `to` | segments |

The first row is what keeps a dashboard's live-tail poll off the disk **by
construction**, so no rate limiting is needed for the common case.

`StatsSource::queries()` is sync and infallible today
([`ports.rs`](../../../crates/fah-api/src/ports.rs)). Segment reads are I/O, so
it becomes `async fn queries(&self, …) -> io::Result<QueryLogPage>` — the shape
`HistorySource` already uses. That is the **only** signature change in
`fah-api`; the route handler keeps its current logic.

### 4. `qtype` filter

Genuinely new — `QueryLogFilter` has `client`, `domain`, `verdict`, `from`,
`to` and no qtype. Add `qtype: Option<QueryType>` and match it in
`QueryLogFilter::matches`, so **both** tiers gain the filter, not just the
reader. A live filter set that is a subset of the search filter set would force
the dashboard to build two filter panels.

`fah-api` parses `?qtype=` with the existing `wire::parse_qtype`, which already
round-trips `A` / `AAAA` / anything else. Do not add a second parser.

**Trap:** the wire spelling and the stored spelling differ. `QueryType::Aaaa`
serialises to JSONL as `"Aaaa"` (serde's variant name) but renders on the API
as `"AAAA"` (`wire::qtype_name`). A byte prefilter for qtype must search the
*stored* spelling. Prefer filtering qtype after parse and prefiltering only on
`domain`, where the win is large and the encoding is plain.

### 5. `oldest_retained`

Add to the `GET /api/v1/queries` response:

```json
{
  "items": [ /* unchanged */ ],
  "next_cursor": null,
  "oldest_retained": "2026-07-19T00:00:00Z"
}
```

Timestamp of the first line of the oldest surviving segment, or `null` when no
segments exist. It is a fact about the retained data, so it stays true across
any future storage change. `null` when `query_log.enabled = false`.

### 6. Flush on shutdown

`Engine::shutdown()` stops dns, http and api and never drains `pending_log`
([`main.rs`](../../../crates/fastadhunter/src/main.rs)), so even a clean
`docker stop` silently loses the last ≤ 5 s of query log.

`Stats::flush_query_log()` is already `pub async` and already requeues on
error — the fix is calling it on the shutdown path. Independent of the reader,
folded in here because it touches the same code and the same 5 s window.

### 7. Documentation

Both of these currently document the limitation as permanent and must move in
the same change (root CLAUDE.md: a change that contradicts the docs updates
them):

- **API.md** §`GET /api/v1/queries` — replace "Serves the in-RAM ring only" and
  the "empty array, not an error" paragraph. Document the routing rule in user
  terms ("no filters = live view; any filter = search over retained history"),
  `qtype`, `oldest_retained`, and the ≤ 5 s freshness gap on searches. Also
  drop "The on-disk segments … are not reachable through any endpoint today."
- **CONFIGURATION.md** §`[query_log]` — the "two storage tiers and only one of
  them is readable today" block, and the `ring_entries` comment claiming it is
  the ONLY thing `/queries` can read. `retention_days` now governs how far a
  search reaches, which makes it a user-facing setting rather than a disk
  housekeeping knob — say so.
- **CONTEXT.md** — only if a new term is introduced. "Query log", "ring" and
  "segment" already exist; prefer them over inventing a name for the reader.
- No ADR yet. Deferred by decision until the implementation exists; revisit
  whether "linear scan, no index" is an architectural decision or an
  implementation detail once it is measured.

## Acceptance criteria

- Seeded segment fixtures: a filtered search returns rows older than the ring,
  newest-first, with correct `next_cursor` paging across a segment boundary.
- No-filter request is served from the ring and performs **zero** filesystem
  reads (assert it, don't assume it).
- `?limit=` and `?cursor=` alone do not trigger a segment read.
- Restart with existing segments: `next_sequence` resumes above the highest
  stored sequence; two runs produce no duplicate sequence.
- Damage cases all resolve through the one skip rule, each asserted separately:
  torn final line, zero-length newest segment, a newest segment where **no**
  line parses (must fall through to the previous segment), and an empty
  segments directory (`next_sequence` = 0, search returns an empty page).
- `qtype` filters correctly in **both** tiers, including `Other("HTTPS")`.
- `oldest_retained` matches the oldest surviving segment and is `null` when
  there are none.
- A shutdown after recording entries leaves them on disk (previously lost).
- Concurrent searches respect the semaphore.
- A scan running concurrently with sustained `record` + `flush_query_log`
  does not stall the flush, and `query_log_overflow_dropped()` stays `0`. This
  is the test that proves the lock-coupling hazard above is absent.
- API.md and CONFIGURATION.md contain no surviving claim that the segments are
  unreadable.
- Gates green: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D
  warnings`, `cargo test --workspace`.

**On-device check** (this task is not covered by `p2-08`, which is HTTP-scoped):
after deploy, search a domain older than the ring window and confirm rows come
back, then confirm a bare `GET /queries` still answers in the same latency band
as before.

## Out of scope

- NDJSON streaming / bulk export. Reverse order plus `limit` means a UI page is
  one or two segments; streaming would force the first `Stream`-returning port
  in the API layer for a use case that does not exist yet. Different endpoint,
  different day.
- Any index, sidecar or manifest file.
- Server-side filtering on `WS /api/v1/events` — the dashboard filters the live
  stream client-side.
- Compressed segments.
- Per-client long-term series (phase 1.5 deferred it to the UI work).

## Suggested prompt

> Read `plan/wip/phase2/p2-09-query-log-reader.md`, API.md
> §`GET /api/v1/queries`, CONFIGURATION.md §`[query_log]`, and the root
> CLAUDE.md hard rules. Implement `QueryLogReader` in `fah-stats` beside
> `SegmentWriter`, reusing `QueryLogEntry` and `QueryLogFilter` rather than
> defining new types. Derive `next_sequence` at boot from the newest segment.
> Route `GET /api/v1/queries` inside `fah-stats`: no semantic filter → ring,
> any filter → segments, no union. Add `qtype` to both tiers and
> `oldest_retained` to the response. Call `flush_query_log()` on
> `Engine::shutdown()`. Update API.md and CONFIGURATION.md in the same change.
