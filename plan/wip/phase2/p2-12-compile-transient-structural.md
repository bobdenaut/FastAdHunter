# P2-12 — Compile Transient, Structural Terms

**Phase:** 2 · **Depends on:** [`p2-11`](p2-11-compile-peak-rss.md) (which
answered the allocator term and left these three untested) · **Model:** Opus

**Do not restart p2-11's investigation.** It established that the peak is a
ratchet across compiles driven by mimalloc's deferred purge, shipped
`MIMALLOC_PURGE_DELAY = 0`, and cut peak 230.7 → 181.4 MiB. That finding stands
and the setting is live. This task takes the terms p2-11 explicitly did not
test.

## Goal

Know where the memory goes during a refresh. Correct what turns out to be
wrong — **without changing the resulting ruleset or refresh semantics**.

**≤128 MiB is a product and performance target, not an operational
constraint.** The container runs `memory-high=unlimited` with ~697 MiB free on
the device, and the peak returns to ~52 MiB steady. A 180 MB transient is
therefore not, in itself, an operational problem on the RB5009 — it is a number
that has to be explained rather than a breach that has to be fixed.

So attribution comes first, and a change is justified only where a term is the
wrong *shape*, not merely large. The first deliverable is the accounting; the
patch, if any, follows from it.

## What is already known

Measured 2026-08-08 on 0.2.12, RB5009 (`process_peak_rss` across a real
scheduled refresh at 13:16:35Z) and reproduced structurally on the dev box:

| | |
| --- | --- |
| Device peak, refresh | **180.07 MB** (p2-11 predicted 181.4 MiB — same number) |
| Device peak, boot | 125.14 MB |
| Steady RSS after refresh | 54.38 MB — returns, no persistent cost |
| Device transient to explain | **125.69 MB** |
| Exact peak point | `MatcherBuilder::add_parsed_list_masked` for `big.oisd.nl` |

**The attribution is complete** —
[`docs/code-review/p2-12-compile-transient-attribution.md`](../../../docs/code-review/p2-12-compile-transient-attribution.md),
instrument and raw output in `docs/code-review/p2-12-attribution/`. Measured
with a counting `GlobalAlloc` over mimalloc v3.3.2 (the container's allocator
and version) against the real 26.26 MiB corpus; reproduces to the byte.

| Term | Requested | Class |
| --- | ---: | --- |
| old ruleset, held live by `ArcSwap` until `swap_in` | 25.84 MB | **structural** — the atomic swap |
| all 17 raw list texts, resident simultaneously | 26.26 MB | incidental |
| dedup index (2 511 658 slots × 4 B) | 9.58 MB | structural, 0.82 MB overshoot |
| `big.oisd.nl` `ParsedRuleList` (436 341 rules) | **56.51 MB** | **incidental** |
| — Vec spine, 524 288 cap × 80 B | 40.00 MB | of which 6.71 slack |
| — `Arc<str>` domains | 16.51 MB | 39.7 B/rule |
| matcher arena + records at the peak | 14.26 MB | structural — it is the product |
| = live heap at the peak | 132.45 MB | |
| − old ruleset baseline | −25.84 MB | |
| = **dev-box transient** | **106.61 MB** | |
| device transient − dev-box transient | **19.08 MB** | allocator / OS, bounded not explained |

The peak is the instant the **largest list's parsed form and its arena copy are
both fully live**.

**The dominant term is the parsed form, not the raw text.** `ParsedRule` is
80 B and its `DomainRule.domain` is an `Arc<str>` — one heap allocation per
rule, **135.8 B/rule measured**, for data the builder immediately copies into
an arena at 33.9 B/rule. It is materialized whole and consumed once, in order.

**Two candidates for the 19.08 MB are eliminated, and one is bounded.** Counting
both buffers across every *moving* realloc (measured by pointer identity) adds
**+2.12 MB**, not the ~20 MB a doubling suggests. Fetched list bodies are not
live during the compile — `fetch_and_commit` completes before `compile()`
re-reads `/data`. `heap_bytes()` is accurate to 0.15 %, so the ruleset figure in
`/api/v1/debug/memory` is sound. Size-class rounding is **+10.20 MB, an upper
bound on its RSS contribution rather than a measurement of it**: the ruleset is
1 118 live allocations, so it is large-block bin rounding, and the tails are
never written.

**List order is worth ±19.92 MB and nothing controls it.** Peak by order:
deployed 132.45, largest-first 132.45, **smallest-first 152.37**. The deployment
is at the best case only because `big.oisd.nl` is entry 0 in config order.
Sorting is **not** semantically free — insertion order decides which duplicate
wins and the order the arena is appended in, so a sorted build changes arena
bytes and fails this task's own equivalence guard.

**Uncosted until now:** the compile performs 78 copying reallocs above 1 MiB,
moving **556.19 MB**. Streaming removes the `Vec<ParsedRule>` doublings from it,
which is evidence for the "neutral or better" CPU criterion below.

This does **not** contradict p2-11's retraction of PERFORMANCE.md's
streaming-parse lever. That lever described streaming the *raw text*, which is
26.26 MB and correctly not dominant. The parsed form is a different allocation
that no measurement had reached, because p2-11 abandoned the dev-box repro
(`process_rss` returns `None` on Windows). Measuring live heap instead of RSS
is what made it visible.

## Method

Measure before changing anything, and reuse p2-11's instruments rather than
inventing new ones.

1. **On-device:** ~130 ms poll of `/api/v1/debug/memory` around a triggered
   `refresh_all`. p2-11 verified this tracks `getrusage` to within 2 %.
   **Do not use the history sampler** — a compile is 2.7 s and at 360 s
   sampling a sample lands inside one 0.76 % of the time.
2. **Dev box:** counting `GlobalAlloc` over `fah-rules`' public API against the
   pinned corpus — the instrument is saved at
   `docs/code-review/p2-12-attribution/szprobe.rs`, so re-deriving costs a build
   and not a rewrite. Gives logical live-heap attribution independent of the
   device's allocator/OS RSS behaviour — the one thing the dev box *can*
   measure here, since `process_rss` returns `None` on Windows. It records
   **requested** sizes, so it does **not** model allocator-specific RSS
   overhead, page retention or size-class rounding, and a realloc chain appears
   as its logical result rather than as the transient where both buffers are
   live.
3. Both arms per change. RSS is the budget; live heap is the attribution. The
   gap between them is the allocator term (19.08 MB in the table above), which
   only the device can settle — do not expect the dev box to reproduce it.

## Work, in dependency order

| # | Change | Estimated peak saving | Risk |
| --- | --- | ---: | --- |
| 1 | Stream parse → builder; never materialize `Vec<ParsedRule>` | ≈ −45 MB | public parser signature changes; `RefreshStats::from(&parsed)` and `looks_misparsed()` must accumulate incrementally |
| 2 | Read one list text at a time inside the blocking compile | −16.24 MB | `upper_bound` currently needs every text before the loop — needs a second pass or a bound derived from file size |
| 3 | Pre-size arena/records; tighten the dedup bound toward the real rule count | ≈ −3.7 MB | low |

These are the sizes of the available levers, listed so the decision has numbers
— not a plan to execute.

**Item 3 is roughly 2× smaller than first estimated.** The measurable slack is
2.87 MB in arena/records — what `build()` frees beyond the dedup index, 90.35 →
77.95 MB of which 9.58 is the index — plus 0.82 MB of dedup ceiling overshooting
the 1 148 707 rules actually parsed by 9.3 %. Item 2's figure is now exact:
26.26 MB of texts minus `big.oisd.nl`'s own 10.02 MB.

**Items 1 and 2 remain estimated, not measured.** They are derived from the
measured decomposition by subtracting the term each change removes; nothing is
implemented, so neither has been observed. 1 + 2 together take **estimated**
live heap at the peak 132.45 → ~74 MB and the **estimated** device peak to
≈ 122 MB; 1 alone lands at an estimated ~87 MB live heap. The device-peak
estimates additionally inherit the 19.08 MB allocator term, which is itself a
subtraction across two machines — so they are the softest numbers here and must
not be quoted as results.

**A fourth lever exists and is deliberately not listed above: compiling
largest-list-first.** It is worth 19.92 MB against the worst order and nothing
today at the deployed order, and it changes the compiled ruleset. It belongs in
a task about *guaranteeing* the peak, not one about reducing it.

**Change 1 is the only one with an argument beyond size.** `ParsedRule` holds
135.8 B/rule for data the builder immediately copies into an arena at
33.9 B/rule — a 4× amplification on a buffer that is materialized whole and
consumed once, in order. That is a shape defect, and it would be worth fixing
at half the size. Changes 2 and 3 are size alone.

## Criteria

**Closed 2026-08-08 with zero code, as `p2-11` was.** The first criterion is the
deliverable and it is met. The rest gate a code change that is not made — the
levers are sized, and none is taken — so they carry forward to whichever task
takes one, and are not outstanding work here. Two exceptions are listed under
"Left open" below, because they are open *questions* rather than acceptance
criteria: the allocator term on musl/ARM64, and the absence of a regression
guard for this peak.

- [x] **Every term in the peak is attributed first**, and each is classified as
      structural (the design requires it), incidental (an artefact of how the
      code happens to be written) or allocator. Closing this task with the
      accounting complete and no code changed is a legitimate outcome, as
      p2-11 demonstrated — the deliverable is knowing where the memory goes.
      **Done 2026-08-08, zero repo changes** — 106.61 of 125.69 MB measured
      exactly, 19.08 MB allocator/OS bounded but not explained
      (`docs/code-review/p2-12-compile-transient-attribution.md`).
- [ ] Compile CPU measured before and after, on ARM64. Streaming removes
      436 k `Vec` pushes and a realloc chain, so it should be neutral or
      better — **unmeasured, and must not be assumed.** Current on-device
      compile is 2.75 s.
- [ ] Resulting ruleset **structurally identical, not merely equivalent**.
      Rule count, `duplicates_removed` and corpus verdicts are necessary but
      **not sufficient** — none of them would catch a streaming builder that
      silently reordered rules, changed arena layout or shifted which duplicate
      wins. The guard is an equal **deterministic fingerprint**:

      The guard is `matcher.rs`'s test-module `Fingerprint` — arena bytes +
      flattened records + slot count — **extended with the `url` tier, and
      nothing else.**

      Those three fields already cover the ordering risk: the arena is appended
      in rule order and records are flattened in order, so a reordered build, a
      changed arena layout and a different duplicate winning each change the
      fingerprint. `url` is added because change 1 feeds the URL tier too and
      can affect it independently.

      **Do not extend to the remaining fields.** `policy_mask`, `dnstype`,
      `rewrite` and `clients` are keyed by record index, so a reorder that
      corrupted them already shows in arena + records; `lists` is list identity
      and is untouched by how rules arrive. Adding them means normalising three
      `HashMap`s whose iteration order is unstable, for coverage this criterion
      already has.

      Byte-for-byte `Matcher` equality is not available at all — those same
      `HashMap`s, plus `Arc<str>` addresses that differ between runs, would
      make it report false failures.

      Capture the fingerprint on the **pre-change** checkout over the pinned
      corpus, then require equality after. Fingerprint **and** verdicts, not
      either alone.
- [ ] Refresh semantics unchanged: the old ruleset serves until `swap_in`, a
      failed fetch still keeps the last-good copy.
- [ ] Steady-state RSS unchanged or better (p2-11 improved it 20 %; do not
      regress that).
- [ ] Peak re-measured on-device across a real refresh, not a boot.

## Explicitly out of scope

- The `ArcSwap` old-ruleset term (25.84 MB). That is the atomic-swap guarantee
  (hard rule 3). Serving from a dropped ruleset is not a trade available here.
- `arena_reserve` = 1 GiB, p2-11's suspect for the ~59 MiB of surviving
  ratchet. Separate variable, separate experiment; do not change it in the same
  arm as anything above or neither result will be attributable.

## Left open

- Whether the 19.08 MB allocator term reproduces on musl/ARM64 or exceeds the
  dev box's figure. Size-class rounding bounds it at 10.20 MB and copying
  reallocs at 2.12 MB, both measured — the residue is segment metadata and page
  granularity, which only the device shows.
- List order is worth ±19.92 MB and nothing enforces it. The deployment is at
  the best case because `big.oisd.nl` is entry 0 in config order.
- Carried from p2-11: `MIMALLOC_PURGE_DELAY = 0` is unmeasured above ~0.5 qps.
  Watch minor page faults at flat RSS and revert to 100 if they climb.
- Carried from p2-11: no regression guard exists for this peak — it is invisible
  to every automated gate, which is how it moved unnoticed. Any guard must be
  on-device.
