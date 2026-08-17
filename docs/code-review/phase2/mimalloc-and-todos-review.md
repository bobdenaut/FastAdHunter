# Review — `global_allocator = mimalloc` + three in-code TODOs

Reviewed against the working tree of 2026-07-30 (uncommitted). Not a task
review: this is an assessment of a user-authored change plus three review
notes the user left in the source. No code changed as part of this review.

Files in scope:

- `crates/fastadhunter/src/main.rs` — `#[global_allocator]`
- `crates/fastadhunter/Cargo.toml`, `crates/fah-rules/Cargo.toml`, `Cargo.lock`
- `crates/fah-rules/tests/dedup_alloc_bound.rs`
- `crates/fah-rules/src/lifecycle/mod.rs` — TODO 1
- `crates/fah-rules/src/lifecycle/source.rs` — TODO 2
- `crates/fah-rules/src/matcher.rs` — TODO 3
- `requests/settings.http` — `min_ttl_seconds` 30 → 600 (manual-test payload,
  not code; no comment)

## Verification performed

| Check | Result |
| ----- | ------ |
| `cargo build -p fastadhunter` (Windows/MSVC) | passes — `libmimalloc-sys` `cc` build works |
| `docker buildx build --platform linux/amd64` (full Dockerfile) | **exit 0** — the static-musl build compiles mimalloc's C |
| `rust:1.96.0-alpine` toolchain | `/usr/bin/gcc` and `/usr/bin/cc` present; `musl-dev` + `gcc` installed in the base image, so `cc` has both compiler and headers |
| `cargo fmt --check` | fails — trailing whitespace inside the three TODO comment blocks only |
| `cargo clippy --workspace --all-targets -- -D warnings` | fails — `clippy::doc_lazy_continuation` at `matcher.rs:676`, inside TODO 3's doc comment. Nothing else. |

Both gate failures are caused by the TODO comments themselves and disappear
when they are removed or rewritten. The functional change is clean.

## 1. The allocator swap

### What is correct

`crates/fastadhunter/src/main.rs`:

```rust
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;
```

This is the canonical form, in the right crate. `#[global_allocator]` belongs
in the binary and nowhere else — a library setting it would fight every
consumer. L4 is the only correct home, and that is where it is.

The choice itself is well-targeted, and this deserves saying plainly: musl's
`mallocng` is written for size and hardening, not throughput, and it serializes
more than glibc's malloc does under multi-threaded churn. A Tokio
multi-thread runtime allocating per query against `mallocng` is a plausible
component of the measured ~15–16k QPS ceiling, which `docs/code-review/phase1/p1.5-06-review.md`
already localised to FAH's own handling rather than to ingest or conntrack.
This is one of the few single-line changes that can move that number.

### 1.1 `mimalloc` is a regular dependency of `fah-rules`, and must not be

`crates/fah-rules/Cargo.toml` adds it under `[dependencies]`. Its only use in
that crate is `tests/dedup_alloc_bound.rs`, which is a test target.

Consequences of leaving it there:

- an L2 library now carries a C-toolchain build requirement for a test-only
  need — `cargo test -p fah-rules` and any downstream build of the library
  compile mimalloc's `static.c`;
- it reads as if `fah-rules` participates in allocator selection, which it
  must not.

Fix: move it to `[dev-dependencies]`, or apply §1.3 and remove it entirely.

### 1.2 It bypasses `[workspace.dependencies]`

Both crates declare `mimalloc = { version = "0.1" }` inline. Every shared
dependency in this workspace lives in the root `[workspace.dependencies]`
table with a comment stating why it is there — `reqwest`'s `hickory-dns`
note is the clearest example of the convention. Two independent `"0.1"` pins
can drift.

Fix:

```toml
# Root Cargo.toml, [workspace.dependencies]
# musl's mallocng is tuned for size and hardening, not for throughput under
# multi-threaded churn; the shipped artefact is static-musl, so the process
# allocator is mallocng unless replaced. Set as the global allocator in
# `fastadhunter` only (ADR-000N).
mimalloc = "0.1"
```

and `mimalloc.workspace = true` at the use sites.

### 1.3 Recommend reverting the change to `dedup_alloc_bound.rs`

```rust
let ptr = unsafe { MiMalloc.alloc(layout) };   // was System.alloc(layout)
```

`Counting` records `layout.size()` — the bytes *requested*, not the bytes the
allocator commits. Swapping what it forwards to therefore changes **nothing
the test measures**. The assertion is that `MatcherBuilder::with_capacity`
under an adversarial rule ceiling requests less than 40 MiB, and that figure
is a property of the clamp, not of the allocator.

What the change does cost: a C build on the `fah-rules` test path, and the
dependency in §1.1 that has to exist to support it.

Recommendation: restore `System` and drop `mimalloc` from `fah-rules`
altogether.

If it is kept deliberately, three comments in that file are now false and must
be corrected — the doc comment on `Counting` ("Delegates every allocation to
the system allocator"), the inline comment in `alloc` ("forwarded verbatim to
the system allocator"), and the `unsafe impl` block's `// SAFETY:` comment,
which justifies soundness by naming `System` as the delegate. A `// SAFETY:`
comment that names the wrong callee is precisely the rot the root CLAUDE.md
`unsafe` rule exists to prevent; it is not a cosmetic issue.

### 1.4 Docs owe an entry, and this is ADR-shaped

Root CLAUDE.md: a change that contradicts the docs updates the doc in the same
change, or needs an ADR if a decision is being reversed. PERFORMANCE.md
§Principles currently reads:

> **No GC, no hidden allocations** — the hot path is allocation-free;
> allocations happen at load/reload time.

That is not contradicted, but it is now incomplete: the document is silent on
which allocator serves the allocations that do happen, and that is now a
deliberate choice rather than a consequence of the target libc.

What needs recording is the *alternative*: in a year, "why not just use the
platform allocator" is a question with a non-obvious answer, and the answer
("the platform allocator here is `mallocng`, not glibc malloc") is a fact about
the musl static build that nothing else in the tree states.

**Recorded in the module doc of `crates/fastadhunter/src/allocator.rs`, not in
an ADR.** An ADR was drafted and deleted: the decision is a one-line
`#[global_allocator]` inside a single module whose entire purpose is to contain
it, so the rationale belongs where the swap is — a reader changing the allocator
opens that file by definition, and cannot miss it. PERFORMANCE.md golden rule 4
carries the one-line pointer. The two consequences below are recorded there
too, because both are easy to trip over later.

### 1.5 This change cannot be validated on this machine

The baseline being replaced is musl `mallocng`. It exists only inside the
container. A criterion run on Windows compares mimalloc against the Windows
heap, and a run on any glibc host compares it against glibc malloc — neither
is the thing that was swapped out. Recorded here because the local
`cargo bench` gate is the reflex, and here it would produce a number that
looks like evidence and is not.

The valid measurements are on-device:

- the QPS ceiling (`p1.5-06`'s ~15–16k, with `/tool profile` attribution) —
  the number this change is aimed at;
- steady-state RSS at equal cache occupancy.

Both need the same load generator and the same ruleset as the pre-swap run to
mean anything.

### 1.6 It moves the p2-07 baseline, and the running soak is already void

`fah-model/src/memory.rs` documents the residual as:

> binary text and data pages, thread stacks, the tokio runtime, and allocator
> memory **musl has not returned to the OS**.

Two things follow.

First, that sentence is now wrong about the mechanism and should name mimalloc
instead — a small edit, but it is the one comment in the tree that explains
what the residual *is*, so a stale mechanism there mis-teaches the metric.

Second, and materially: mimalloc's retention and purge behaviour differs from
mallocng's, so the residual's absolute value and its shape over time both
change. Pre-swap and post-swap residual series are not comparable. The 0.2.5
household soak was running when the swap was deployed, so its post-restart
segment measures a different allocator from its pre-restart segment; treat T0
as reset at the deploy, not at `2026-07-26T00:05:34Z`.

This is not an argument against the change. It is an argument for doing the
p2-07 persistence work (which is what makes the residual chartable at all)
and *then* starting a clean soak, rather than trying to read a leak signal
across an allocator boundary.

### 1.7 Resolved — the RouterOS growth is page cache

**Superseded on 2026-07-30 by
`docs/code-review/phase2/0.2.7-router-memory-and-throughput.md` §2.** This section
previously called page cache "the leading hypothesis based on the available
evidence" and declined to rule out an allocator-level contribution. That caution
was right at the time and is no longer needed: the question is now closed by
direct measurement, not inference.

On the deployed RB5009, with FAH the only container on RouterOS 7.21.5:

```
  70.29  MiB   process_rss (/api/v1/debug/memory)
+ 525.05  MiB   FAH's total on-disk data under /kingston/fastadhunter/
= 595.34  MiB   vs. measured /container memory-current = 595.0 MiB   ->  0.06 %
```

`memory-current` is cgroup v2 `memory.current`, which charges page cache to
whichever cgroup faulted the pages in; `free-memory` counts that same
reclaimable cache as available. The two counters classify identical bytes
oppositely, which is the whole of the apparent contradiction. Of the 525 MiB,
498.19 MiB is the query log's 500 MiB retention cap held in cache in its
entirety.

Confirmed independently by a container restart the same day: recreating the
cgroup discarded every page-cache charge and `memory-current` fell 595.0 ->
60.1 MiB with RSS at 41.7 MiB, no change to binary, config or workload.

So the earlier revisions' *conclusion* — that the growth is not FAH's heap and
mimalloc would not fix it — was correct; only their confidence was unearned.
The 66 MiB RSS cut in §1.6 failing to move `free-memory` is exactly what this
model predicts: the cut was anonymous memory, while the router's counter is
dominated by reclaimable cache the cut never touched.

Two consequences for the rest of this file:

- The allocator statistics added by this change were *not* what settled it — the
  on-disk arithmetic was. They did, however, expose their own defect in the
  process: `committed` never decrements on purge under mimalloc v3, so the
  derived `allocator_retained_bytes` was removed in 0.2.8 (report §5.2, §6.1).
- "Committed rising while components stay flat is retention" no longer holds as
  a reading. `committed` only ever rises, and exceeds even `peak_rss`. Use
  `residual` against its own history instead.

### 1.8 Local benches regressed — against the wrong baseline

`cargo bench --workspace` after the swap, all seven regressed at p = 0.00:

| Bench | Time | Change |
| ----- | ---- | ------ |
| `full_pipeline/blocked_query` | 1.68 µs | +6.8 % |
| `full_pipeline/forwarded_query_overhead` | 2.28 µs | +9.8 % |
| `startup_phases/1_read_from_data` | 8.84 ms | +3.1 % |
| `startup_phases/2_parse_rule_list` | 170.67 ms | +2.9 % |
| `startup/startup_from_cached_lists` | 325.03 ms | +7.6 % |
| `startup_phases/3_build_matcher` | 102.31 ms | **+16.8 %** |
| `startup_phases/4_build_matcher_two_overlapping_lists` | 198.69 ms | **+33.5 %** |

Two exceed the 10 % threshold root CLAUDE.md says needs justification. Three
reasons this run cannot answer the question it looks like it answers:

1. **Wrong baseline.** Criterion diffed against the previous run on this Windows
   box, so it measured mimalloc against the *Windows* heap. What the change
   replaces is musl `mallocng`, which exists only in the container.
2. **Wrong workload shape, and this is the decisive one.** These are
   single-threaded microbenches. mimalloc's advantage over mallocng is
   per-thread heaps removing cross-thread contention; a single-threaded loop has
   no contention to remove, so the bench measures only per-allocation fast-path
   cost — where mimalloc is weakest against a well-tuned allocator. The
   benchmark structurally cannot show the win it is being asked about.
3. **Wrong architecture, and unpinned.** x86-64 rather than ARMv8, where atomics
   and cache-line behaviour dominate allocator cost; and criterion swings badly
   on this machine without core pinning, so magnitudes are untrustworthy even
   for what it did measure.

What the result does establish is worth keeping: **mimalloc is not universally
faster**, and the two worst hits are the most allocation-heavy paths
(`build_matcher` grows the arena, the record vector and the dedup index). No
PERFORMANCE.md budget is breached — 1.68 µs and 2.28 µs against a < 1 ms p99
target, 325 ms against a 1–3 s startup target — so nothing is broken.

The part that deserves not to be hand-waved: `blocked_query` and
`forwarded_query_overhead` are the single-query paths, and PERFORMANCE.md rule 4
already requires the hot path to be allocation-free. If there is little
per-query allocation, there is little contention for per-thread heaps to remove
— so the win may be smaller than assumed while the fast-path cost is real.
**The swap could net lose on the target.** `crates/fastadhunter/src/allocator.rs`'s throughput justification is
unproven in both directions, and the on-device A/B in its revisit criteria is
the measurement that decides it, not a nicety.

## 1.9 The real memory defect: the scheduler recompiled per list (fixed)

Found by reading the live box, not the code. 24.3 h of `/api/v1/history/perf`:
RSS 58.52 → 53.45 MiB, slope over the final 8.1 h **−0.331 MiB/h**, band
49–62 MiB, and visibly releasing (69.01 MiB at 17:00 → 50.55 at 18:00). Steady
state is healthy.

The outlier was 204.39 MiB — 3 samples of 1459, inside four minutes:

```text
17:49:41   52.4 MiB
17:52:41  204.4 MiB   ← peak
17:53:41   97.7 MiB
17:55:41   77.6 MiB
```

`/api/v1/lists` showed **16 lists refreshing between 17:50:03 and 17:52:14**.
Cause at `lifecycle/mod.rs:434-437`: `refresh_list` calls `compile()`, and
`compile()` rebuilds the *whole* combined matcher from every enabled list — so
16 due lists meant 16 full rebuilds of the same ~680k-rule corpus, each holding
parsed rules, a new 22 MiB matcher beside the live one, and the dedup index. The
transients stacked faster than the allocator could return pages: 60 % over the
128 MB budget, 80 % of the 256 MB container ceiling.

`refresh_all` already fetched everything and compiled **once**. The scheduler was
the one path that did not.

**Fix:** `refresh_due_lists` now fetches all due lists sequentially (unchanged —
deliberate RAM discipline) and compiles once, skipping the compile entirely when
every fetch failed. 16 compiles → 1.

Made assertable: `ListManager::compile_count()` plus three tests — 16 due lists →
exactly 1 compile with every rule verified in the served ruleset; all-failed
batch → 0 compiles, failures still recorded; partial batch → 1 compile, survivor
applied. The all-failed test earned its keep immediately: the first version of
the fix returned early *before* `record_status`, so failed lists would have
reported `NeverAttempted` in `GET /api/v1/lists`.

## 2. TODO 1 — `pending_cache` and OOM (`lifecycle/mod.rs:287`)

The note claims a persistently unwritable `/data` lets each refresh park up to
`MAX_LIST_BYTES` (64 MB) of raw text in RAM, that a few lists can add
100–200 MB instantly, and that nothing retries the flush in the background.

### What is right

- The retention is real. `commit_raw` (`mod.rs:681`) inserts the raw text into
  `pending_cache` on a cache-write failure, and only `refresh_list` /
  list removal ever take it out (`mod.rs:609`, `mod.rs:684`).
- There is no background retry. Confirmed: the only `cache::write` call for a
  list is inside `commit_raw`, which runs on refresh. A pending copy therefore
  survives until that list's next scheduled refresh — hours by default.
- The worst-case total is large, and **nothing caps the number of lists**. The
  only ceiling in the tree is `matcher.rs:307`'s `u16::try_from(...).expect("at
  most 65_535 rule lists")`. So a large `n × 64 MB` really is reachable in
  principle.

### What is wrong

**"fiecare refresh depune câte un String de până la 64 MB"** — no.
`pending_cache` is `Mutex<HashMap<Arc<str>, String>>` keyed by list id, and
`commit_raw` uses `insert`, which **replaces**. Repeated failed refreshes of
the same list do not accumulate; the second overwrites the first and the old
`String` is dropped.

The bound is therefore `n_failing_lists × MAX_LIST_BYTES`, not
`n_refreshes × MAX_LIST_BYTES`. That is a large correction to the risk model:
the growth is bounded by *configuration*, not by *uptime*, which means hard
rule 4 ("memory must not grow with traffic or uptime") is **not** violated.
The bound is merely loose.

It is also loose against practice rather than against the cap: 64 MB is the
ceiling for a *hostile* list. oisd-small is ~1 MB; the large aggregate lists
are ~10 MB. A realistic all-lists-failing scenario on this deployment costs
low tens of MB, not 200.

### What the note gets backwards about severity

The missing retry is the *less* serious half, not the more serious. While a
pending copy exists, behaviour is correct: `list_text` (`mod.rs:762`) prefers
the pending copy over the disk copy, so reads and recompiles both see the
newest text. What is lost is restart durability, until the next refresh.

And the obvious "fix" — drop the text instead of parking it — is a
**correctness regression**, which is presumably why the code parks it.
`compile()` re-reads every enabled list from the `/data` cache. If the pending
copy is discarded, the next refresh of any *other* list recompiles from disk
and silently reverts this list to its stale (or absent) cached text. The rules
would regress with no signal.

### Recommended fix (small, and worth doing)

Bound the aggregate rather than removing the mechanism:

- cap total pending bytes across all lists (8 MB is generous against real list
  sizes);
- when an insert would exceed the cap, do not park the text — instead mark the
  list `degraded` (the status `p2-00` already introduced) so `GET /rules/lists`
  shows it, and log at `warn`.

That converts an invisible RAM cost into a visible status, keeps the
correctness property for every realistic list size, and needs no background
task. A retry loop is the wrong shape for the amount of risk here.

**Priority: low.** The trigger is a persistently unwritable `/data`, and the
realistic cost on this deployment is tens of MB. Worth a follow-up task, not
worth interrupting p2-07.

## 3. TODO 2 — `Vec::with_capacity` from `Content-Length` (`source.rs:55`)

### The stated reason does not hold

> generând alocări/copieri inutile în mimalloc

This runs once per list per `refresh_hours_default` (24 h by default), on the
blocking pool, immediately after an HTTP transfer that took orders of
magnitude longer. Growth by doubling costs ~log₂(N) reallocations and ~2N
bytes of copying in total — for a 10 MB list, roughly 20 MB of `memcpy`
against a multi-second download. As a CPU argument this is unmeasurable.

The argument also got *weaker* with §1's change, not stronger: mimalloc
services large blocks from its own mmap'd regions and can often grow them
without a full copy, where mallocng is likelier to allocate-and-copy. Naming
mimalloc as the reason inverts the actual direction.

### The real reason to do it anyway

**Transient peak RSS.** Doubling means the final growth step holds the old
buffer and the new one at the same time. A 10 MB body's last realloc peaks at
16 MB + 32 MB = 48 MB resident for the copy, against a 10 MB steady state.
On a 1 GB box shared with RouterOS, and with `refresh_due_lists` already
deliberately sequential to avoid exactly this kind of spike
(`mod.rs:730`: "parallel downloads of several 1M-domain lists would spike RAM
on a 1 GB router"), that is a coherent win and consistent with an existing
design decision.

So: do it, for the peak, not for the cycles.

### The sketch as written introduces an amplification

```rust
let capacity = response.content_length()
    .map(|len| (len as usize).min(max_bytes))
    .unwrap_or(0);
```

`max_bytes` is `MAX_LIST_BYTES` = 64 MB. `Content-Length` is declared by the
server and never verified. A hostile or misconfigured source that sends
`Content-Length: 999999999` and then a 1 KB body gets a **64 MB up-front
allocation** out of one header.

Today that costs the attacker 64 MB of actual upload, which the streaming cap
at `source.rs:79` enforces against bytes on the wire. The sketch replaces a
bytes-on-the-wire cost with a free one. It stays *bounded*, so hard rule 4
still holds and this is not a vulnerability — but the crate's own doc comment
frames the cap as "a rogue or misconfigured source must not be able to grow
memory without limit", and trading enforcement-by-transfer for
enforcement-by-ceiling weakens that on purpose-built input.

### Recommended shape

Two independent changes, and the second is the more valuable one:

```rust
// Reject before transferring, not after 64 MB of it: a declared length over
// the cap cannot become a valid list.
if let Some(len) = response.content_length() {
    if len > max_bytes as u64 {
        return Err(LifecycleError::TooLarge { origin: url.clone(), limit: max_bytes });
    }
}

// Pre-size to avoid the doubling peak, but from a ceiling this crate chooses,
// not from a number the server picked. Every real list fits; a lying
// Content-Length buys at most PREALLOC_CEILING, and the streaming cap above
// still bounds the body itself.
const PREALLOC_CEILING: usize = 8 * 1024 * 1024;
let capacity = response.content_length()
    .map_or(0, |len| (len as usize).min(PREALLOC_CEILING));
let mut body: Vec<u8> = Vec::with_capacity(capacity);
```

The early rejection is worth more than the pre-allocation: it turns a 64 MB
download-then-discard into a rejected request, on a path that currently has to
stream the whole hostile payload to discover the problem. Keep the existing
per-chunk check regardless — `Content-Length` may be absent, chunked, or a lie
in the other direction.

**Priority: low-medium.** The early-reject half is the part worth writing.

## 4. TODO 3 — `heap_bytes()`: `len()` → `capacity()` (`matcher.rs:667`)

### It does not compile

The suggestion is:

```rust
self.arena.capacity()
    + self.records.capacity() * std::mem::size_of::<Record>()
    + self.slots.capacity() * std::mem::size_of::<u32>()
```

Those fields are not `Vec`. On the built `Matcher` (`matcher.rs:529-533`) they
are:

```rust
arena: Box<[u8]>,
records: Box<[Record]>,
slots: Box<[u32]>,
lists: Box<[std::sync::Arc<str>]>,
```

`Box<[T]>` has no `capacity()` method — there is no spare capacity to report.
`MatcherBuilder::build` converts each one with `into_boxed_slice()`
(`matcher.rs:514-518`), which *is* the shrink-to-fit. So `len()` already **is**
the capacity, exactly, by construction. The change would not build, and if the
fields were `Vec` it would return the same number.

`self.lists.iter().map(|s| s.len() + arc_overhead)` is likewise already right
for a different reason: the elements are `Arc<str>`, and `str` is exactly
sized — an `Arc<str>` has no growth slack to miss.

### Where `heap_bytes()` genuinely under-counts

The two `HashMap`s, `dnstype: HashMap<u32, (u32, Arc<str>)>` and
`rewrite: HashMap<u32, Arc<str>>`. The function sums a per-**entry** estimate
over `.values()` and never accounts for the table itself. hashbrown allocates
`next_pow2(len / 0.875)` buckets plus one control byte each, so the table can
be close to 2× the live entry count in slots, and the empty slots are resident.

Scale: for a few thousand `$dnstype`/`$dnsrewrite` rules this is tens of KB,
against an arena measured in tens of MB. Real, correctly identified by the
instinct behind the note, and numerically irrelevant to the
`<= 40 MB / 1M domains` budget the function exists to measure.

### The goal in the note is the actual problem

> Asta va face ca raportarea să fie mai apropiată de RSS

**This is the wrong target, and pursuing it would damage p2-07.**

`heap_bytes()` feeds `MemoryBreakdown::ruleset`. The gap between the sum of
components and RSS is not missing component memory — it is binary text and
data pages, thread stacks, the Tokio runtime, and allocator slack, and
`memory.rs` names all four as the *definition* of `residual`. The residual is
not error to be minimised; it is a measurement surface with a specific job,
stated in the same file:

> a leak shows as *residual growing while the named components stay flat*,
> because the growth you legitimately expect has been subtracted out.

Inflating `ruleset` toward RSS breaks that in three separate ways:

1. it double-counts, since the slack is already reported in `residual`;
2. it destroys the signal the design rests on — allocator slack tracked inside
   `ruleset` makes a component that should be flat between recompiles move
   with allocator state, so "components flat, residual rising" stops
   discriminating anything;
3. it can trip `over_accounted()` (`memory.rs:75`), which the design treats as
   an unconditional accounting bug — "always an accounting bug, never a real
   state" — so a deliberate over-estimate would raise a permanent false alarm.

`heap_bytes()` should stay a floor on what the compiled structure asked for.
If closing the gap to RSS is the goal, the mechanism is the p2-07 series:
persist the components and read the residual's *slope*. That is what the
reopened `p2-07` exists to make possible.

**Recommendation: drop this TODO.** Optionally add the two HashMap tables to
the estimate as a separate, correctly-attributed line — but it changes the
reported figure by well under 1%, and it is not what the note was after.

## Summary

| Item | Verdict |
| ---- | ------- |
| `#[global_allocator]` in `main.rs` | Correct, well-targeted, builds for static-musl (verified end to end) |
| `mimalloc` in `fah-rules` `[dependencies]` | **Wrong section** — test-only; move to `[dev-dependencies]` or remove per §1.3 |
| Inline `version = "0.1"` in two crates | Violates the `[workspace.dependencies]` convention |
| `dedup_alloc_bound.rs` → `MiMalloc` | **Revert** — measures `layout.size()`, so the swap changes nothing; three stale comments incl. a `// SAFETY:` if kept |
| Docs for the swap | Missing — `crates/fastadhunter/src/allocator.rs` recommended; `memory.rs`'s residual comment still says "musl" |
| Local benchmarking of the swap | Invalid by construction — the replaced baseline exists only in the container |
| p2-07 / soak impact | Residual baseline shifted; pre- and post-swap series are not comparable |
| TODO 1 `pending_cache` | Real, bounded by list count not by uptime (note's mechanism is wrong); low priority; fix is an aggregate cap + `degraded`, not a retry loop |
| TODO 2 `with_capacity` | Do it for peak RSS, not CPU; the sketch's `.min(max_bytes)` adds a 64 MB header-driven allocation — clamp separately and reject on `Content-Length` early |
| TODO 3 `capacity()` | Does not compile (`Box<[T]>`), no-op if it did, and the stated goal works against p2-07's design — drop it |

Gate status: `fmt` and `clippy` both fail **only** on the TODO comment blocks.
Nothing in the functional change is flagged.
