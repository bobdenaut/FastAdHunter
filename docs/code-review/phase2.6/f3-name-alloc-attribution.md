# F3 follow-up — per-query allocation attribution by name shape

Follow-up to F3 in [project-risk-inventory.md](../project-risk-inventory.md)
§Closed, which left "heap names cost 7–8 more allocations per query than
inline names" unattributed. Measured 2026-09-12 on `main` at `07d4d68`.

## Summary

- Every allocation in `Pipeline::handle` is now attributed to one operation
  (table below). Totals reproduce the harness exactly.
- The 7–8 delta has three independent drivers, not one: label count (hickory
  encoder), total label bytes > 32 (hickory `Name` heap spill, one per parse
  and per clone) and text length (`to_utf8` String doubling, plus one `Label`
  spill per label > 24 bytes).
- One allocation site was avoidable without touching ownership, await points
  or response bytes: `domain_of` built its `String` through `format!`, which
  starts at capacity 0 and doubles. It now pre-sizes from `Name::len()` and
  writes through the same `Display` path. 1 allocation instead of 2–4, on
  every path and every name.
- Everything else is required by the response representation or lives inside
  `hickory-proto`. Record-only.

## Decisions

- `domain_of` keeps `Display` (`to_utf8` semantics, punycode decoded), not
  `Name::to_ascii()`, which also pre-sizes but would change IDN text for the
  matcher, the cache key and stats.
- The harness gains a per-handle ceiling per case, set to the measured value.
  A future hickory or allocator change that adds an allocation fails the test
  and is re-baselined consciously.
- The scratch tracer used for the breakdown was deleted, not kept as a tool.
- No `reserve_exact` on the response `Vec`s: allocation count would not
  change, only bytes (352 → 88 and 1088 → 272). Unmeasured benefit.

## Measurements

Method: `GlobalAlloc` wrapper over mimalloc recording every `alloc`/`realloc`
size for one warm `handle` per name shape, then the same for each public
sub-operation in isolation to fingerprint sizes. Stub forwarder whose empty
`NOERROR` answer is negative-cached on the first call, so "cache hit" is the
steady-state path (see inventory §Closed F3). Current-thread runtime,
`block_on` allocates nothing. x86 dev box, debug profile (counts do not
depend on opt level).

### Per-operation breakdown, before this change (allocations per `handle`)

| Operation | blocked inline `blocked.example.com.` (3 labels, 17 B) | blocked heap `a-very-long-subdomain-label-here.blocked.example.com.` (4 labels, 49 B, one label 32 B) | cache-hit inline `example.org.` (2 labels, 10 B) | cache-hit heap `a-very-long-subdomain-label-here.allowed.example.org.` (4 labels, 49 B, one label 32 B) |
| --- | --- | --- | --- | --- |
| `Message::from_vec`: `Vec<Query>` (88 B) | 1 | 1 | 1 | 1 |
| `Message::from_vec`: `Name` label_data spill + regrow (hickory/tinyvec, label bytes > 32) | 0 | 2 | 0 | 2 |
| `domain_of` = `format!("{name}")`, String from capacity 0 (8, 16, 32, 64) | 3 | 4 | 2 | 4 |
| `domain_of`: `Label(TinyVec<[u8; 24]>)` spill inside `Display`, per label > 24 B | 0 | 1 | 0 | 1 |
| `CacheKey` `Box<str>` | 0 | 0 | 1 | 1 |
| response `add_query(query.clone())`: `Vec<Query>` capacity 4 (352 B) | 1 | 1 | 1 | 1 |
| response `add_query`: `Name` clone (label bytes > 32) | 0 | 1 | 0 | 1 |
| response answer `Record`: `Vec<Record>` capacity 4 (1088 B) | 1 | 1 | 0 | 0 |
| response answer: `name().clone()` (label bytes > 32) | 0 | 1 | 0 | 0 |
| `Matcher::decisive_rule` `Arc<str>` (rule text; blocked only) | 1 | 1 | 0 | 0 |
| `Event::dns` `Box<QueryEvent>` (160 B) | 1 | 1 | 1 | 1 |
| `Message::to_vec`: output buffer (512 B) | 1 | 1 | 1 | 1 |
| `Message::to_vec`: `labels_written` `Vec<usize>`, one per name emitted | 2 | 2 | 1 | 1 |
| `Message::to_vec`: `store_label_pointer` suffix `to_vec()`, one per label of the question name | 3 | 4 | 2 | 4 |
| `Message::to_vec`: `name_pointers` `Vec` (capacity 4, 128 B) | 1 | 1 | 1 | 1 |
| **Total** | **15** | **22** | **11** | **19** |

Harness check: 15 × 64 = 960, 22 × 64 = 1408, 11 × 64 = 704, 19 × 64 = 1216.

### Delta drivers (12 name shapes, one factor varied at a time)

| Driver | Effect per query | Where |
| --- | --- | --- |
| Label count | +1 per label (suffix copy in `store_label_pointer`); predicted, unmeasured: +1 regrow past 4 labels (`name_pointers` capacity 4) | hickory `BinEncoder` |
| Total label bytes > 32 | +1 on parse (+1 more when the spill lands before the last label); +1 per `Name` clone into the response (2 on the blocked path, 1 on cache hit) | hickory `Name` / tinyvec |
| Text length | `format!` doublings: 12 chars 2, 20 chars 3, 37–53 chars 4; +1 per label > 24 B | `domain_of` (fixed), hickory `Label` |

### `warm_pipeline_handles_allocate_a_steady_amount`, 64 warm handles, before → after

| Case | Before (`07d4d68`) | After | Per handle |
| --- | --- | --- | --- |
| blocked, inline name | 960 | 832 | 15 → 13 |
| blocked, heap name | 1408 | 1216 | 22 → 19 |
| cache hit, inline name | 704 | 640 | 11 → 10 |
| cache hit, heap name | 1216 | 1024 | 19 → 16 |

Predicted from the breakdown before the change was applied; measured after
it. `warm_adaptive_forwards_allocate_a_steady_amount` unchanged at 1216.

### Record-only (not avoidable without changing representation or hickory)

| Site | Per query | Why it stays |
| --- | --- | --- |
| `Name` clones into `Query` and `Record` | 0–2 (label bytes > 32) | hickory `Message` owns its names |
| Parse spill | 0–2 | inside `Name::read` + tinyvec growth |
| `BinEncoder` label-pointer table | 5–8 | inside hickory; largest chunk, label-count driven |
| `Box<QueryEvent>` | 1 | the events channel carries a 16-byte `Event` by design |
| `decisive_rule` `Arc<str>` | 1 (blocked) | one deliberate allocation per blocked query, `matcher.rs` |
| `to_fah_query_type` `Other(to_string())` | 1 (non-A/AAAA; unmeasured) | outside this pass |

## Files changed

- `crates/fah-dns/src/qtype.rs` — `domain_of` pre-sizes from `Name::len()`,
  writes through `Display`; two unit tests (byte-identical to `to_utf8` for
  ASCII, long-label, punycode, non-FQDN and root names; capacity = `len()`
  for ASCII names).
- `crates/fah-dns/tests/forward_alloc.rs` — per-handle ceiling per case
  (13 / 19 / 10 / 16).

## Remaining TODOs

- None scheduled. The `Other(to_string())` allocation for non-A/AAAA types
  (HTTPS, PTR, TXT) was not measured here; a future pass should trace an
  HTTPS query the same way.
