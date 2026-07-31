# Boot: refresh clock from cache mtimes, and the orphan sweep

**Commit:** `7920415` on `feat/mimalloc-and-memory-telemetry` · **Date:**
2026-07-30 · **Files:** `crates/fah-rules/src/lifecycle/{cache.rs,mod.rs}`

Two changes to `ListManager::boot`, both about `/data` being the durable state
the process cannot keep for itself. Neither is deployed yet — see §6.

---

## 1. The defect: every restart compiled the ruleset twice

Observed in the 0.2.8 deploy log:

```text
19:59:02.312  ruleset compiled from cache rules=702166      ← boot compile
19:59:09.322  scheduled refresh complete lists=16 ... 702178 ← 7 s later, all 16
                                                               refetched + recompiled
```

`ListManager::last_attempted` is a `HashMap<Arc<str>, tokio::time::Instant>`,
initialised empty. The due-check reads:

```rust
match last {
    None => true,                                   // every list, every restart
    Some(last) => now >= last + entry.interval(…),
}
```

`Instant` is monotonic and process-relative, so the map **cannot** be persisted
even in principle. Every restart made every list read as never attempted. `/data`
was confirmed by SSH to hold only `lists/`, `stats/`, `query_log/` and
`history/` — no status directory — so nothing else carried the timestamps
either.

**This was waste, not a fault.** Listeners bound at `02.334` and the refresh
finished at `09.322`, so serving was never blocked. The cost was ~2.3 s of ARM
CPU, ~24 MB of downloads and a second ~158 MiB peak-RSS transient, on every
restart, to rebuild a byte-identical ruleset.

## 2. The fix: the cache file's mtime *is* the clock

`cache::write` renames `{id}.raw` into place on every successful fetch, so its
mtime is already the wall-clock time of that list's last successful refresh.
That makes it the persisted form of `last_attempted` with **no new on-disk
state, no schema, and nothing that can fall out of sync with the copy it
describes** — the alternative, a status file, would have to be kept consistent
with the very files it describes.

`cache::age(data_dir, id, now) -> Option<Duration>` reads it; `boot()` calls
`seed_last_attempted_from_cache()`, which inserts `Instant::now() - age` for
each list whose copy is younger than its interval.

### Why it cannot make anything worse

Seeding only ever inserts an instant *earlier* than now, and only when
`age < interval`. Compared with the previous behaviour (no entry ⇒ due
immediately), the most it can delay a refresh by is `interval − age`, which is
the interval semantics working correctly. **The change can remove a redundant
fetch; it cannot delay a due one.**

Every unusable input falls back to exactly the old behaviour — the list stays
due:

| Case | Why it happens | Result |
| ---- | -------------- | ------ |
| No cache file | first-ever boot, or a hand-deleted copy | due |
| `age >= interval` | genuinely stale | due |
| mtime unreadable | platform will not report it | due |
| **mtime in the future** | the RB5009 has no battery-backed RTC, so a container can start before NTP syncs and see a clock behind its own files | due |
| `Instant::checked_sub` returns `None` | the monotonic clock counts from *system* boot, so a router that rebooted minutes ago cannot represent an instant hours in the past | due |

That last row is worth naming: **after a router reboot the fix silently does not
apply** and the double compile still happens. That is deliberate. Refetching
once too often is free; skipping refreshes is not.

### The one behaviour change

mtime records the last *success*; `last_attempted` recorded *attempts*. A list
whose source is down keeps an old file, so after a restart it gets one immediate
retry instead of waiting out its interval. One extra attempt, not a loop — the
retry writes `last_attempted` and the in-memory clock takes over.

### Considered and rejected

**Persisting `last_attempted` to a status file.** More machinery, a schema to
migrate, and it would have to use wall-clock time anyway — so it inherits every
clock-skew problem the mtime has, plus a consistency problem the mtime does not.

**Filling `ListStatus::last_refreshed` from the mtime too.** Proposed, then
dropped. Its doc states the opposite as a deliberate rule ("boot-from-cache does
not count — RULE_ENGINE.md's 'compile from `/data`' is a load, not a refresh")
and a test asserts it. Reversing that is an API-visible semantic change needing
RULE_ENGINE.md and API.md review, not a free side effect of a performance fix.
**Still open:** after a restart the API reports the incoherent pair
`last_result: Ok` + `last_refreshed: null`.

## 3. The orphan sweep

`compile()` iterates the **configured entries** and looks each id's cache file
up; it never enumerates the directory. So a `.raw` file with no config entry was
inert — and never deleted. `DELETE /api/v1/lists/{id}` cleans up after itself,
so the way to strand one is to edit `[[rules.lists]]` while FAH is stopped. Up
to `MAX_LIST_BYTES` (64 MB) each, on a volume shared with the query log.

`boot()` now calls `remove_orphaned_copies()`, which warns and deletes.

### The two guards that make it safe

Both were caught while writing it, and both are red-tested (verified: removing
the guard fails the test).

1. **`user-rules` is not a configured list.** `set_user_rules` writes
   `lists/user-rules.raw` through the same cache, but `USER_RULES_ID` never
   appears in `[[rules.lists]]`. A keep-set built from configured lists alone
   deletes it — and unlike every other copy it is *authored*, not downloaded, so
   nothing can bring it back. Added explicitly.
2. **Disabled lists keep their copy.** A disabled entry is excluded from
   `compile()` but is still configured. Sweeping it would turn
   `PATCH {"enabled": true}` from an instant swap into a download. The keep-set
   is therefore every entry, enabled *or not* — deliberately not the same filter
   `compile()` uses.

### Deliberate limits

- **Boot only.** Here the scheduler has not spawned and no listener is bound, so
  nothing else can be writing to `lists/`. A periodic sweep would race
  `commit_raw` and would have to distinguish a stranded copy from one being
  written this instant, which a directory listing cannot do.
- **`{id}.raw` only.** The `.raw.tmp.{pid}` sidecars `write` renames from are
  left alone for the same reason: a listing cannot tell a leaked one from a
  write in flight. A leaked sidecar is a few MB that the next successful refresh
  of that list replaces anyway.
- A failed `remove_file` warns and continues; the sweep never aborts boot.
- Logged at `warn`, with the list id and byte count — a file was deleted, and
  the operator who edited the config is the only one who can say whether that
  was intended.

## 4. Tests

11 new, all red-checked where a red check is meaningful.

| Test | What it pins |
| ---- | ------------ |
| `a_restart_does_not_refetch_lists_whose_cached_copy_is_still_fresh` | the defect itself; fails with seeding disabled |
| `a_restart_still_refreshes_a_cached_copy_older_than_the_interval` | seeding cannot stretch an interval |
| `a_clock_behind_the_cached_copy_leaves_the_list_due` | the RTC-less fallback |
| `a_hand_deleted_cache_file_is_refetched_without_refetching_the_rest` | `/data` self-heals, surgically |
| `boot_deletes_a_cached_copy_no_configured_list_claims` | the sweep works |
| `boot_never_sweeps_the_user_rules_copy` | guard 1; fails without it |
| `boot_keeps_a_disabled_lists_cached_copy` | guard 2; fails without it |
| `boot_leaves_half_written_temp_files_alone` | sidecars survive |
| `cache::age` × 3 | missing file, ~6 h round trip, future mtime |

Two of these assert a *negative* — that no fetch was attempted — which
`compile_count` alone cannot show, since a failed batch does not recompile
either. They give the survivors source paths that do not exist, so a wrong
refetch flips `last_result` to `Failed` and the test breaks.

No existing test needed changing: the scheduler tests either skip `boot()` or
write their sources to `srcs/` rather than to the `lists/*.raw` cache.

## 5. Gates

`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
and `cargo test --workspace` (32 suites) all clean, 0 failures. No bench run —
neither change touches the hot path; both run once, at boot.

## 6. Verified on-device — 0.2.9, 2026-07-31

Deployed 08:30. The boot log settles both halves, and liviu had unknowingly set
up the better test by **hand-deleting one list's cached copy from `/data`**
before starting:

```text
INFO fah_rules::lifecycle: refresh schedule restored from cached copies lists=15
INFO fastadhunter: ruleset compiled from cache rules=701907
INFO fah_rules::lifecycle: list refreshed list=spy active=347 ...
INFO fah_rules::lifecycle: scheduled refresh complete lists=1 refreshed=1 failed=0
```

`lists=15`, not 16 — the deleted copy could not be seeded, so that list stayed
due and **exactly one** list was refetched. Before this change the line would
have read `lists=16`. The `701907 → 702178` recompile is a genuine one (`spy`
really did change), not the byte-identical redo the fix removes.

**mtimes do survive a container restart on the RouterOS `/data` volume** —
previously assumed from `/file print` output, now observed. The seeding worked
across a stop/remove/repull/start cycle.

**No `deleted cached copy…` lines**, which is the correct outcome and worth
stating: the list was removed from `/data`, not from the TOML, so it is still
configured. It gets refetched, not swept. The orphan sweep only fires on a copy
whose `[[rules.lists]]` entry is gone — the distinction the two near-miss guards
(user-rules, disabled lists) exist to protect.

## 7. Doc changes in the same commit range

- **RULE_ENGINE.md §List lifecycle** — refresh intervals survive restarts via
  the cached copy's mtime; boot sweeps unclaimed copies; a hand-deleted copy
  heals itself.
- **PERFORMANCE.md** — replaced the note saying `compile_duration_seconds` is
  hardcoded to zero (wired up in 0.2.8: **2.317 s**, ~2.2 µs per *parsed* rule,
  1 047 409 parsed → 702 178 compiled) and recorded that a restart no longer
  compiles twice.
