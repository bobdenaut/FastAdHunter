# Top 3 issues — audit, verification and fix review (2026-09-17)

## Summary

- Audit: whole-repository read of every production Rust line outside inline
  `#[cfg(test)]` modules — 36,654 of the 95,064 tracked lines, all 12 crates
  and the binary — against the tree at `9d51567`. Not read: inline test
  modules, `crates/*/tests/`, `crates/*/benches/`, `docs/code-review/**/*.rs`
  probes, and ~4,000 lines of `tui-monitor`'s UI/state layer. Three defects,
  each contradicting a claim its own doc comment makes. No hot-path
  correctness or memory-bound violation found in `fah-dns`, `fah-rules` or
  `fah-http`.
- Verification, same day: all three mechanisms are real. Two severities were
  overstated — the audit said high / medium / medium; the evidence supports
  medium / low / low.
- Owner's decision, same day: fix #1 and #3, leave #2. Both fixed
  2026-09-17; §Files changed and §Gates below. Uncommitted at the time of
  writing.
- Fix review, same day: both fixes in scope and correct, no hot-path or memory
  change, all gates green on the working tree. Two informational notes,
  nothing open.

## Findings

Severity-ranked, after verification. Each carries the audit's claim, the
verified reading and the fix state.

### V1 — MEDIUM · a rooted `path` in `POST /api/v1/lists` escapes `/data` — FIXED

Audit (rated high): the guard at `crates/fah-api/src/routes.rs:582-591`
rejected `Component::ParentDir` only. The sink,
`crates/fah-rules/src/lifecycle/source.rs:45` `data_dir.join(url)`, discards
the base when `url` is absolute, so `{"path": "/config/apikey"}` became a rule
list the engine opened on every refresh and every compile. The guard's comment
says "traversal must die here, before any `Path::join`" — it did not.

Verified:

- No second guard anywhere in `lifecycle/` or the route (`grep` for
  `has_root`, `is_absolute`, `RootDir`, `Prefix`, `canonicalize`,
  `starts_with(data_dir` — empty). `Path::join` replaces the base on Linux and
  Windows alike.
- What it yields to the caller, who is already the authenticated admin:
  `last_error` (`wire.rs:724`) tells whether the file exists and is readable;
  `rules_total` (`:711`) gives a line count; `POST /rules/test` confirms any
  domain-shaped line. The API key and the Argon2 hash are not domain-shaped,
  so they do not leak through the parser; reads stay inside what the non-root
  container can open. Hence medium, not high: hardening, as the comment says —
  and the hardening is what failed.

Fix (2026-09-17): the guard rejects `..` anywhere, and a rooted path
(`RootDir`, or a Windows `Prefix`) unless it starts with the data dir. A rooted
path *inside* the data dir stays accepted on purpose: API.md:911,
CONFIGURATION.md:386 and the dashboard's placeholder
(`add-list-dialog.tsx:117`) all spell the mounted-file form as
`/data/lists/local.txt`, and a relative-only guard would have broken that
contract. `has_root()` alone misses `C:foo` on Windows, so the components are
matched. The route learns the data dir through one new accessor,
`ListManager::data_dir()`. Tests: `/config/auth-hash` and
`lists/../../outside.txt` are `422`; a rooted path under the harness's data dir
is `201` (with an explicit `id` — `derive_id` splits on `/`, so a Windows
tempdir path yields no usable stem; a test-portability detail, not a defect).
Cost: none on any hot path.

### V2 — LOW · `is_fresh` treats future-stamped slots as fresh — DEFERRED

Audit (rated medium): `crates/fah-stats/src/bucket.rs:248-250` —
`current_hour.saturating_sub(hour) < BUCKET_COUNT` is `true` for any
`hour > current_hour`, so the staleness test catches "too old" and never
"impossible". The RB5009 has no battery-backed RTC; a clock that steps backward
after queries were recorded leaves those slots counted in `totals` (`:124`),
`snapshot` (`:138`) and every per-client `queries_24h` until the ring genuinely
reaches that hour, plus a future-dated entry in the `buckets` array.
`completed_hours` (`:159`) also requires `hour_epoch < current_hour` (`:162`),
so future slots are shown and summed but never rolled up until the clock
reaches them. Proposed fix: treat `hour > current_hour` as stale.

Verified:

- How a future stamp arises: the ring is persisted in `snapshot.json` and
  restored whole at boot (`stats.rs:93-94`), so a container that starts before
  NTP syncs — the case `lifecycle/cache.rs:90-93` documents for list caches —
  restores slots stamped by the previous, synced clock. A backward step of
  ≥ 1 h inside a run does the same.
- Why low: in the restart-before-NTP case the restored slots hold the true last
  24 h, so counting them is the more useful reading while the clock is wrong,
  and the proposed fix would blank the dashboard until sync instead. Either
  way `record` (`:98-108`) overwrites slots as the clock advances and the skew
  is gone within 24 h. Not observed on the RB5009; the router's NTP window at
  boot is seconds.
- If fixed: one line (`hour <= current_hour && ...`) plus a test; state the
  trade-off above in the commit.

Deferred 2026-09-17, owner's decision: the only fix on the table trades one
wrong reading for another; neither is right, and the skew clears within 24 h
either way.

### V3 — LOW · `RollupWriter::boot` reset the cursor and re-appended — FIXED

Audit (rated medium): `crates/fah-stats/src/history/rollup.rs:88-95` read only
the newest `rollup-*.jsonl` and *replaced* `last_hour_written` with `.max()`
of its parseable rows. `None` — an empty file, or rows this build cannot parse
— reset the cursor; `append_hours` (`:105-117`) skips only when the cursor is
`Some`, so every completed hour still in the 24 h ring was appended again,
including hours already in an older day-file. `HistoryReader::summary` sums
rows without dedup at both resolutions (`reader.rs:67-96`), so those hours
double-counted in `GET /api/v1/history/summary`. The field's doc claimed
"idempotent boot".

Verified:

- When the newest file yields `None`: it is empty. `append_line`
  (`history/mod.rs:33-44`) creates and writes in one call, so the window is a
  crash or a full disk between `open` and the first write of a new day file. A
  torn last line does not do it — `filter_map(.ok())` skips it and the cursor
  lands on the previous row. The "rows this build cannot parse" arm was void:
  `per_type` (`fah-model/src/history.rs:27`) shipped with the history in
  `762d01c`, an ancestor of the deployed `d307c36`, so no rollup row without it
  exists.
- Why low: needs an empty newest day file and a restart; the growth is ≤ 23
  rows per such boot, bounded. Not observed.

Fix (2026-09-17): `boot` collects `(day, path)` for every `rollup-*.jsonl`,
sorts newest-first and reads files until one yields a parseable row; the
cursor is that file's max hour. `per_type` carries `#[serde(default)]`, so a
future schema change cannot re-open the second arm. The four-line doc comment
that said "reads only the newest rollup file" was deleted rather than
rewritten (hard rule 7). Tests: an empty newest day file leaves the cursor on
the previous day's last hour and a re-offer appends nothing; a row without
`per_type` sets the cursor.

#### The boot scan's `Vec<(u64, PathBuf)>` — bound, not a concern

Asked after the fix, answered from the code; no change proposed.

| Question | Answer |
| --- | --- |
| Maximum entries | `retention_days + 1`: one file per calendar day, `prune` (`rollup.rs:173-190`) deletes past the cutoff and keeps the current day; prune lag ≤ 1 h (`PRUNE_INTERVAL`, first flush tick after boot) adds at most one file at a day boundary; no file is created while the process is down. Deployed: 30 (`docs/fastadhunter.toml:186`) → ≤ 31, 32 with lag. Validated ceiling 3650 (`fah-config/src/lib.rs:216-221`) → ≤ 3651. Hand-placed future-dated files are never pruned — operator action, not the retention rule |
| Contents retained? | No. Each entry is the day number and the file's `PathBuf`. Files are read one at a time into a `String` dropped at the end of its iteration; rows stream through `filter_map(..).map(..).max()`. The old code already called `entry.path()` per file and dropped all but the newest |
| Startup or runtime? | Startup only: `Stats::boot` (`stats.rs:97`) once from `main.rs:390`. The `Vec` is a local, freed when `boot` returns |
| Evidence of size | None measured; bound by construction. 32 B per entry plus a 48 B path: ~2.5 KB at 31 entries, ~290 KB at 3651, transient before the first query is served. `/file print` on the volume would give the live count; the deployed retention already caps it at 31 |
| Is the bug only the `None` path? | Yes. An empty newest day file → `last_hour_written = None` → `append_hours` re-appends up to 23 restored hours → `HistoryReader::summary` double-counts. The `Vec` has no memory or performance implication |

### Fix review — informational, no action

- **The guard's comment is now narrower than the guard.** `routes.rs:578-581`
  still explains the check as "`..` segments would escape it"; the guard also
  rejects a rooted path outside the data dir. Not wrong, only incomplete. Hard
  rule 7 forbids rewriting it; deleting is the only permitted edit.
- **Rooted acceptance is spelled against the configured data dir.**
  `starts_with` compares components, not canonical paths. Production passes
  `/data` (`main.rs:32`), so `/data/lists/local.txt` matches. A dev box started
  with a relative `--data-dir ./data` rejects every rooted path (fail-closed;
  the relative form still works), and a symlink under `/data` pointing outside
  is not caught. Both consistent with "hardening, not an auth boundary" and
  with CONFIGURATION.md:386, where the owner's own TOML uses the rooted form.

## Checked and found acceptable

| Claim | Verdict |
| --- | --- |
| Audit: "no hot-path correctness or memory-bound violation found in `fah-dns`, `fah-rules`, `fah-http`" | consistent with the two audits closed 2026-09-17; not re-read in verification |
| Audit: `dns.udp_max_inflight = 0` disables UDP admission control — reads like a hard-rule-4 violation | owned decision, not a defect: CONFIGURATION.md:101 documents `0 = no cap, today's behaviour`, `schema/dns/mod.rs:54` asserts it. Worth revisiting on its merits |
| Audit: `/config/apikey` as the example target | the key is not domain-shaped; existence and size are what leak, not the key |

## Checked in the fix review

Scope: the five code files in §Files changed, on the working tree over
`9d51567`. Plan = the V1 and V3 fix paragraphs above; V2 out of scope by
decision. The one deviation from the verification's first proposal ("reject
every rooted path") — a rooted path *under* the data dir stays accepted — is
what API.md:911 and CONFIGURATION.md:386 promise.

| Axis | Evidence | Verdict |
| --- | --- | --- |
| V1 as specified | `routes.rs:582-594`: `ParentDir` anywhere, or `RootDir`/`Prefix` and not under `state.rules.data_dir()` → `422`; accessor at `lifecycle/mod.rs:438` | matches |
| V1 stricter than `has_root()` | `C:foo` has a `Prefix` but no root; the component match rejects it, `has_root()` would not | correct choice |
| V1 no bypass elsewhere | `POST /config` rejects a body carrying `rules.lists` (`config_store.rs:421`); `patch_list` takes `enabled`/`refresh_hours` only (`routes.rs:711-714`); TOML `[[rules.lists]]` is owner-authored | single writer's check |
| V1 error text | not documented in API.md or SECURITY.md; no test pinned the old string | no doc owed |
| V3 as specified | `rollup.rs:82-96`: newest-first sort, read until a row parses, cursor = that file's max hour; `#[serde(default)]` at `history.rs:27` | matches |
| V3 reader consistency | `reader.rs:315` uses the same `from_str::<T>(..).ok()` skip; a row without `per_type` parses in both places | consistent |
| V3 all files empty/corrupt | cursor stays `None`, re-append happens — nothing on disk to dedupe against | right |
| Out of scope | `bucket.rs` untouched; V2 stays deferred | boundary kept |
| Hot path | nothing per query; the guard runs per `POST /lists`, `boot` once from `Stats::boot` | none |
| Memory | `Vec<(u64, PathBuf)>` ≤ `retention_days + 1`, local to `boot`, files read one at a time (table above) | bounded, transient |
| Concurrency / lifecycle | no new state, lock, task or timer; `boot` completes before serving | none |
| Layering | `fah-api` → `fah-rules` already existed; `fah-model` gains a serde attribute only (hard rule 2 kept) | intact |
| Rust quality | accessor returns `&Path` (no clone); two `components()` passes over a request-sized string; explicit `last_hour_written = None` makes `boot` idempotent | fine |
| Comments | `boot`'s stale doc comment deleted, not rewritten (hard rule 7) | compliant |
| Tests V1 | `api.rs:2043-2044` exercise `ParentDir` inside a relative path and a rooted path outside; `:2067-2083` prove a rooted path inside is `201` | each branch covered |
| Tests V3 | `rollup.rs:338-376` prove the cursor survives an empty newest file and a re-offer appends nothing; `:379-395` prove a row without `per_type` sets the cursor | invariant established |

## Files changed — the V1 and V3 fixes

| File | Change |
| --- | --- |
| `crates/fah-api/src/routes.rs` | +7 −4 — the guard: `..` anywhere, or rooted and not under `state.rules.data_dir()` → `422` |
| `crates/fah-rules/src/lifecycle/mod.rs` | +4 — `ListManager::data_dir()` |
| `crates/fah-api/tests/api.rs` | +22 — two rejection rows, one acceptance test |
| `crates/fah-stats/src/history/rollup.rs` | +76 −15 — newest-first walk in `boot`, doc comment deleted, two tests |
| `crates/fah-model/src/history.rs` | +1 — `#[serde(default)]` on `per_type` |

## Gates on the fixed tree (Windows dev box, 2026-09-17)

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean (fix, and re-run in the fix review) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean (one `sort_unstable_by_key` lint fixed on the way; re-run clean) |
| `cargo test -p fah-api -p fah-rules -p fah-stats -p fah-model` | 149 + 139 + 2, 253 + 1 + 2 + 9 + 10 + 1 + 7, 80, 63 — all passed, 0 failed |
| `cargo test --all-features --workspace` (fix review) | 1 662 passed / 0 failed / 12 ignored, 62 suites, exit 0 — a first count through the `rtk` output filter read 3 ignored; the filter drops whole `test result:` lines |
| Red-before-fix | both negative tests fail on the old code by construction: the rooted path was `201`, the empty newest file reset the cursor to `None` |

## Files reviewed

Verification: `routes.rs:548-591`, `source.rs:35-50`, `lifecycle/mod.rs:286`,
`bucket.rs:98-172`, `:248-250`, `lifecycle/cache.rs:84-98`, `stats.rs:90-103`,
`snapshot.rs:14-21`, `aggregates.rs:150-158`, `rollup.rs:38-41`, `:60-117`,
`history/mod.rs:33-44`, `reader.rs:67-96`, `fah-model/src/history.rs:16-28`,
`api.rs:2034-2050`, `wire.rs:700-746`, SECURITY.md:18-26.

Fix review, in addition: `routes.rs:556-640`, `:706-740`, `state.rs:24-25`,
`lifecycle/mod.rs:282-286`, `:438-440`, `main.rs:32`, `:380-392`,
`config_store.rs:421-435`, `rollup.rs:1-200`, `:228-260`, `:338-395`,
`reader.rs:69-90`, `:313-317`, `api.rs:530-594`, `:2034-2083`,
`Dockerfile:76-117`, `add-list-dialog.tsx:27-99`, API.md:908-917,
CONFIGURATION.md:385-386, `docs/code-review/CLAUDE.md`.

## Remaining TODOs

- Commit the five code files and this review on the owner's go.
- V2: recorded; nothing owed until a backward clock step is observed.
- Audit coverage still open: `tui-monitor` UI/state layer (~4,000 lines),
  inline test modules, `tests/` and `benches/` (~58,000 lines) unread.

**PASS WITH DEFERRED FINDINGS** — V1 and V3 fixed and reviewed 2026-09-17;
V2 real, low, deferred by decision.
