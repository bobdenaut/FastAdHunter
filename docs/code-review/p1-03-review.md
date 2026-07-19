# Code Review — p1-03 List Lifecycle

**Scope:** `crates/fah-rules/src/lifecycle/{mod.rs, source.rs, cache.rs}`,
`crates/fah-rules/Cargo.toml`, workspace `Cargo.toml` ·
**Reviewer:** chief architect pass · **Date:** 2026-07-18 ·
**Status:** findings 1–9 fixed same day (see "Fixes applied" below), 10
deferred to p1-09 by design. Gates green.

## What was delivered (implementation report)

p1-03 done, marked DONE in the phase plan table, all gates green
(fmt/clippy/test workspace-wide, 24 new tests).

Built `fah_rules::ListManager` (`crates/fah-rules/src/lifecycle/`):

- **Sources:** `http(s)://` → reqwest fetch (rustls-only, no openssl);
  anything else → local file under `/data`.
- **`/data` raw-copy cache**, atomic write-tmp-then-rename.
- **One `ArcSwap<Matcher>`** — the hot path only ever does an
  atomic-refcount read, never locks.
- **Refresh** recompiles the combined matcher from all lists' cached raw
  text (matches p1-02's single-matcher-covers-everything design), swaps
  atomically. Fetch/read I/O is the only failure mode (parsing never fails
  per RULE_ENGINE.md) — failure keeps the previous ruleset serving,
  surfaced via `status()`.
- **User rules** as a synthetic list through the same path.
- **Scheduler:** one task per enabled list, deterministic FNV-1a jitter
  (no `rand` dep, same technique as p1-02's benches).

Tests: boot-from-cache (no network), kill-the-network (bind-then-drop a
local port for guaranteed connection-refused — no real internet needed),
8-thread stress test hammering lookups against 200 concurrent swaps (no
loom available offline, real threads instead), local-file source, user
rules, disabled-list exclusion, scheduler task count, jitter determinism.

Flagged for p1-11: reqwest's `rustls` feature pulls `aws-lc-rs`, which
needs cmake + a C compiler to build `aws-lc-sys`. Compiles clean on the
dev machine; the Dockerfile's Alpine stage only has `musl-dev` — may need
`cmake perl build-base` added, or a fall back to the `ring` provider.
Noted in the completion note, not fixed in p1-03 (out of scope).

## Overall assessment

The shape is right. `ArcSwap<Matcher>` as the hot path's single touchpoint
satisfies the no-lock acceptance criterion; the failure policy (I/O is the
only failure mode, previous set keeps serving, status surfaced) matches
RULE_ENGINE.md exactly; the atomic tmp-then-rename cache write is correct;
the test suite is genuinely strong — the bind-then-drop connection-refused
trick and the real-thread swap stress test are the right substitutes for
the tools the offline environment lacks. Error taxonomy is clean, and the
reqwest per-request `.timeout()` correctly covers the body read, not just
the connect.

But the review turns up two HIGH findings that are launch blockers for the
target device, and a cluster of MEDIUMs around the lifecycle's concurrency
story being weaker than its own comments claim. Ten findings, six requiring
fixes.

## Findings

### 1. HIGH — unbounded download size violates "bounded everything"

`ListSource::fetch` ends in `response.text().await` — reqwest buffers the
entire body into memory with no cap. A misconfigured URL, a compromised
list host, or a captive portal serving garbage can return hundreds of MB;
on the RB5009 (1 GB shared with RouterOS) that is an OOM kill of the
household's DNS. Hard rule 4 (memory must not grow with input) applies to
list fetches as much as to traffic. A `Content-Length` check alone is
insufficient (chunked encoding has none).

**Fix:** stream the body via `chunk()` into a pre-sized buffer with a hard
cap (a `MAX_LIST_BYTES` constant, ~64 MiB — an order of magnitude above the
biggest real-world list); exceeding it is a fetch failure, which the
existing failure policy already handles (previous set keeps serving).

### 2. HIGH — first refresh can be delayed a full day; fresh install unprotected

The scheduler sleeps `jitter(id, interval)` — uniform over the **whole**
24 h interval — before the first tick. Consequences:

- **First-ever boot (no `/data` cache):** the default OISD list is absent
  from the compiled matcher until its first refresh — which the jitter can
  push out ~13 h (FNV-1a of `"oisd-basic"` lands where it lands). The
  product's first-run experience is "ad blocker blocks nothing for hours."
- **Stale cache:** boot serves the cached copy (correct), but the async
  refresh RULE_ENGINE.md promises ("refresh happens asynchronously
  afterwards") also waits the same 0–24 h.

The jitter's purpose is de-synchronizing many lists' *steady-state*
refreshes, not delaying the first one.

**Fix:** first refresh shortly after startup with a small bounded jitter
(`jitter(id, SHORT_BOUND)`, ~60 s), then the full interval between
subsequent ticks. One-line change in `spawn_scheduler`; the existing
interval ticker already handles the steady state.

### 3. MEDIUM — CPU-heavy recompile runs on a tokio worker thread

`compile()` reparses every list's raw text and rebuilds the full matcher —
for a 1M-domain corpus that is seconds of CPU on the RB5009's 1.4 GHz
cores. It runs inline in `refresh_list`/`set_user_rules`/`boot`, i.e. on a
tokio runtime worker. The DNS pipeline (p1-04) shares that runtime; during
a recompile one of four workers is gone, and any DNS task queued on it
stalls until work-stealing rescues it — a latency spike precisely when the
box is busiest. The hot path *lookup* is unaffected, but hard rule 3's
spirit ("ruleset changes via atomic swap", off the serving path) extends to
not starving the serving runtime.

**Fix:** run the parse+build inside `tokio::task::spawn_blocking`. The raw
snapshot must be cloned or moved out of the `std::sync::Mutex` first (the
guard is not `Send`), which also removes the current pattern of holding
that mutex across the whole compile.

### 4. MEDIUM — raw text retained in RAM forever, contradicting the stated memory design

The completion note claims "only compiled-matcher bytes are retained
long-term; raw text is kept only for recompilation" — but `raw:
Mutex<HashMap<Arc<str>, String>>` retains every list's full raw text for
the process lifetime. A 1M-domain list is ~20–25 MB of raw text; combined
with the ~28 MiB compiled matcher that roughly doubles the resident
footprint on a 1 GB device, and p1-02's finding 5 (memory scales with the
*sum* of list sizes) makes it worse with multiple lists. The same bytes
already live on disk in the `/data` cache.

**Fix:** drop the in-memory `raw` map; `compile()` reads each enabled
list's text from the `/data` cache (it is the durable source of truth
anyway, and recompiles are rare + off the hot path once finding 3 lands).
Keep an in-memory copy only as a fallback for a list whose `cache::write`
failed (the current best-effort warn path).

### 5. MEDIUM — same-list refresh race commits stale data; comment claims otherwise

`compile_lock`'s doc comment says it exists "so two concurrent refreshes
can never race to overwrite each other's newer result with stale data."
It does not deliver that: the fetch runs **outside** the lock (by design,
so a slow list doesn't block others), so for the *same* list — a scheduled
tick racing the manual API trigger p1-09 adds — refresh A can fetch old
content, refresh B fetch newer content and commit first, then A commits
the stale text over it, into both the `raw` map and the `/data` cache
file. The lock serializes compile+store (real and necessary — keep it);
it cannot order fetch results.

**Fix:** serialize whole refreshes *per list* (one small `tokio::sync::Mutex`
per `ListEntry` around fetch+commit) while `compile_lock` keeps serializing
the global compile+swap. Slow lists still don't block each other; same-list
races become sequential. And correct the comment to state what each lock
actually guarantees.

### 6. MEDIUM — `refresh_hours_default = 0` panics the scheduler

`fah-config` applies serde defaults but never validates a lower bound, and
the env layer (`FAH__RULES__REFRESH_HOURS_DEFAULT=0`) coerces any u32. A
zero reaches `tokio::time::interval(Duration::ZERO)`, which panics —
inside a spawned scheduler task, so it dies silently (handle never awaited)
and that list simply never refreshes again. The `jitter` helper defends
itself (`.max(1)`); the ticker does not.

**Fix:** clamp in `ListManager::new` (`refresh_interval.max(Duration::
from_secs(3600))` or similar floor) — defensive at the point of use, no
Phase-0 config change needed (fah-config is frozen; a validation there
would be a feature addition, not a bug fix, since the panic lives in p1-03
code).

### 7. LOW — refreshing a disabled list "succeeds" with fabricated stats

`refresh_list` on a disabled list fetches the network, writes the `/data`
cache, inserts into `raw` — then `compile()` skips it, `stats.remove(id)`
finds nothing, and `unwrap_or_default()` reports `Ok` with zero
active/inactive/parse_errors regardless of content. Misleading status for
the p1-09 API, plus pointless network+disk work.

**Fix:** early-return `LifecycleError::UnknownList` (or a dedicated
`Disabled` variant) for disabled lists.

### 8. LOW — `boot()` bypasses `compile_lock` and clobbers newer status

`boot()` compiles and stores the matcher, then overwrites each list's
status with `Ok`, without taking `compile_lock`. Correct under the intended
call order (boot before `spawn_scheduler`), but nothing enforces it — a
refresh racing boot could have its newer matcher and its `Failed` status
silently replaced by boot's cache-derived state. Cheap to make unmisusable.

**Fix:** take `compile_lock` for boot's compile+store+status section.

### 9. LOW — `RefreshResult::NeverAttempted` is unreachable; unfetched lists invisible

`record_status` does `entry().or_default()` then immediately overwrites
`last_result`, and `boot` only inserts `Ok` — so `NeverAttempted` is never
observable and `statuses()` omits configured lists that have no cache and
no refresh yet. p1-09's `GET` lists endpoint needs to show "configured,
never fetched" rather than absence.

**Fix:** seed `ListStatus::default()` for every configured list in
`ListManager::new` (or `boot`); the variant then does its job.

### 10. INFO — list id flows unsanitized into the cache path (no change)

`cache_path` builds `data_dir/lists/{id}.raw` from the configured id; an id
containing path separators or `..` escapes the `lists/` directory. The
config file is admin-trusted (SECURITY.md trust model), so this is
defense-in-depth only — worth a one-line reject of ids containing `/`,
`\`, or `..` if p1-09 ever accepts list definitions over the API, which is
the point where the trust boundary moves. Flagged for p1-09, no change now.

## Verdict

Merge-blocking: findings 1 and 2 (device-killing input bound; first-run
protection gap). Should-fix in p1-03 while the file is hot: 3, 4, 5, 6
(concurrency/memory story must match its own documentation before p1-04
builds on this handle). 7–9 are small and worth taking in the same pass.
10 is deferred to p1-09 by design.

Test debt to add with the fixes: oversized-body rejection, first-refresh
timing (scheduler fires within the short bound), disabled-list refresh
rejection, zero-interval clamp.

## Fixes applied (2026-07-18)

### 1. Size cap (HIGH)

`ListSource::fetch` takes a `max_bytes` argument; `ListManager` passes
`MAX_LIST_BYTES = 64 MiB`. Remote bodies are streamed via
`Response::chunk()` into a buffer and abandoned with
`LifecycleError::TooLarge` the moment the cap would be exceeded — never
buffered whole. Local files are length-checked via `fs::metadata` before
reading. UTF-8 conversion is `from_utf8` with a lossy fallback (stray
bytes become parse errors on the affected lines, consistent with the
never-reject policy).

### 2. Prompt first refresh (HIGH)

New `FIRST_REFRESH_JITTER = 60 s`. `spawn_scheduler` sleeps
`jitter(id, FIRST_REFRESH_JITTER)` before its interval ticker (whose first
tick is immediate), so every enabled list's first refresh lands within a
minute of startup — a fresh install gets its protection promptly, and the
jitter still spreads simultaneous startup bursts. Steady state is
unchanged: one refresh per `refresh_hours_default`.

### 3. Compile off the runtime workers (MEDIUM)

`compile()` is now async: it gathers each enabled list's text, then runs
the parse+build inside `tokio::task::spawn_blocking`. No `std` mutex is
held across the compile anymore (the old pattern of holding the `raw` map
lock through the whole parse is gone with the map itself — see 4).

### 4. No resident raw text (MEDIUM)

The `raw` map is gone. The `/data` cache is the source of truth:
`compile()` re-reads each list's text from disk (`list_text`), so raw list
bytes are transient per recompile instead of doubling the resident
footprint next to the compiled matcher. The only in-memory copy is
`pending_cache` — kept solely for a list whose `/data` write failed
(`commit_raw`), where it takes precedence over the stale/absent disk copy
and is dropped again on the next successful write.

### 5. Per-list refresh serialization (MEDIUM)

Each `ListEntry` carries a `refresh_lock: tokio::sync::Mutex<()>` held
across its whole fetch+commit, so a scheduled tick racing a manual trigger
serializes and a stale fetch can no longer commit over a newer one — while
one slow list still never blocks another list's refresh. `compile_lock`
keeps serializing compile+swap globally, and both lock comments now state
what each actually guarantees.

### 6. Zero-interval clamp (MEDIUM)

`ListManager::new` floors the refresh interval at 1 h
(`refresh_hours_default.max(1)`), so a zero from file or env can never
reach `tokio::time::interval`'s zero-period panic. Fixed at the point of
use — fah-config is frozen and the panic lived in p1-03 code.

### 7. Disabled-list refresh rejected (LOW)

`refresh_list` returns the new `LifecycleError::ListDisabled` before any
fetch — no network/disk work, no fabricated all-zero `Ok` stats.

### 8. `boot()` under the lock, no status clobber (LOW)

`boot()` takes `compile_lock` for its compile+store, and writes its
cache-derived `Ok` stats only into statuses still `NeverAttempted` — a
refresh that beat boot to the lock keeps its newer result. (Disk as source
of truth from fix 4 already guarantees boot compiles the newest text.)

### 9. `NeverAttempted` reachable (LOW)

`ListManager::new` seeds a `NeverAttempted` status for every configured
list, so the p1-09 API can distinguish "configured, never fetched" from
"not configured".

### Verification

- `cargo fmt --check` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — green; fah-rules unit tests 65 → 70. New
  regression tests: `oversized_remote_list_is_rejected`,
  `oversized_local_list_is_rejected_before_reading`,
  `refresh_of_disabled_list_is_rejected_without_fetching`,
  `zero_refresh_hours_is_clamped_so_the_scheduler_cannot_panic`,
  `scheduler_first_refresh_lands_within_the_short_jitter_bound` (paused
  tokio clock, `test-util` dev-feature added). Existing kill-the-network
  and 8-reader swap stress tests still green over the reworked internals.
