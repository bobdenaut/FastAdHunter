# P4-02 — Cosmetic Rules Activation — Implementation Plan

Task file: [p4-02-cosmetic-rules-activation.md](p4-02-cosmetic-rules-activation.md).
Depends on p4-01 ([plan](p4-01-html-scaffold-plan.md)) — read its review
file's Implementation Summary first. Written 2026-09-19 against workspace
0.4.1; re-check every line anchor before editing.

## Decision — built, dormant by default

Owner decision 2026-09-19 (phase [CLAUDE.md](CLAUDE.md)): implement the whole
task; the code ships with `[html] enabled = false` and stays dormant on the
deployed box. Nothing here asks for interception or cosmetic lists on the
router — verification is dev-box tests and benches. The cosmetic index is
compiled and swapped with every ruleset whether or not the gate is open; with
the deployed lists it holds zero selectors, so the box's steady-state cost is
the empty index — measure and record that figure alongside the EasyList one.

## Standing rules that bite here

- No comments in Rust (hook). `// SAFETY:` only.
- All rule processing stays in `fah-rules` (L2). `fah-http` only asks
  "selectors for this host under this policy?". Nothing here may name
  `lol_html`.
- `fah-model` stays logic-free: new DTO fields only.
- `.md` edits: list after the gates, wait for the yes per file. Review file
  excepted.
- No commit.

## Read before editing

| Need | Where |
| ---- | ----- |
| Rule kinds, verdict precedence, compiled matcher, dedup | RULE_ENGINE.md §Supported formats (~7–70), §Verdicts (~71), §Compiled matcher (~227), §Deduplication (~236) |
| Policy masks — one ruleset, one mask per rule | RULE_ENGINE.md §Policies (~175–226) |
| Why `Cosmetic` is payload-free today | `crates/fah-rules/src/rule.rs` ~140–200 (`InactiveReason`, `RuleKind`, `ParsedRule`, the 32-byte test) |
| How p2-03 added retention for URL rules | `crates/fah-rules/src/rule.rs` `UrlRule`; `parser/adblock.rs` |
| Where cosmetic lines are classified | `crates/fah-rules/src/parser/adblock.rs` ~37–66 (`COSMETIC_MARKERS`) |
| Counts per list | `crates/fah-rules/src/rule_list.rs` `RuleCounts`; `crates/fah-api/src/routes.rs` ~525–545; `wire.rs` `ListResponse` ~700 |
| Dry-run endpoint | `crates/fah-api/src/routes.rs` `test_rule` ~1312; API.md §`POST /api/v1/rules/test` (~984) |
| No-allocation test pattern | `crates/fah-rules/tests/url_lookup_alloc.rs` (counting `GlobalAlloc` over `MiMalloc`) |
| Atomic swap | `crates/fah-rules/src/lifecycle/mod.rs` `swap_in` (~1371), `matcher()` (~525) |

## Deliverables

1. Parser retains cosmetic rules: `RuleKind::Cosmetic(Box<CosmeticRule>)`.
2. Extended/procedural cosmetics stay inactive, counted separately.
3. `CosmeticIndex` compiled into `Matcher`, swapped with everything else.
4. Allocation-free lookup: host + policy ⇒ effective selector set, with a
   fingerprint p4-03 can key its cache on.
5. Counts: `RuleCounts.cosmetic`; API `rules_active_cosmetic`; dashboard row.
6. `POST /api/v1/rules/test` dry-runs a hostname's selector set.
7. Tests, bench, memory measurement.
8. Doc edits (after the yes).

## Step 1 — parse and retain

### Marker table (replaces `COSMETIC_MARKERS`)

| Marker | Meaning | Classification |
| ------ | ------- | -------------- |
| `##` | element hiding | `Cosmetic`, action `Hide` — unless the selector is procedural (below) |
| `#@#` | hiding exception | `Cosmetic`, action `Exception` |
| `#?#`, `#@?#` | procedural (uBO/ABP extended) | `Inactive(ExtendedCosmetic)` |
| `#$#`, `#@$#`, `#$?#`, `#@$?#` | CSS injection / snippets | `Inactive(ExtendedCosmetic)` |
| `#%#`, `#@%#` | scriptlets | `Inactive(ExtendedCosmetic)` |

Today `#%#` is **not** in the marker list, so a scriptlet line falls through
to the URL parser. Add a test that pins the new classification for each
marker; check what the current code does with `#%#` and record it in the
review if it was compiling scriptlet text into URL rules.

Marker scan stays "first marker position wins" (`min` over `find`), one
byte-load per line as now. Order matters only for the split point: the
domains part is `line[..at]`, the selector is `line[at + marker.len()..]`.

### Procedural detection inside `##`

A `##` selector is `ExtendedCosmetic` when it contains any of:
`:has-text(`, `:contains(`, `:matches-css`, `:matches-attr(`,
`:matches-path(`, `:min-text-length(`, `:upward(`, `:xpath(`, `:remove()`,
`:style(`, `:watch-attr(`, `:others(`, `:-abp-`. Native CSS pseudo-classes
(`:not(`, `:nth-child(`, `:has(`, …) stay `Cosmetic` — the browser evaluates
them through p4-03's style-injection path even where lol_html cannot match
them. Keep the list as one `const` slice; substring search with `memchr`-
backed `str::contains` is fine at parse time (not hot path).

### Cheap syntactic rejection ⇒ `parse_errors`

Empty selector after trim; selector containing `{` or `}`; a domains part
containing whitespace (already rejected); a domains part with an empty label
(`a.com,,b.com`); a **negated exception** (`~a.com#@#.x`) — it has no defined
meaning in this model, so count it rather than guess. Anything else is
retained — deep CSS validity is p4-03's job (lol_html decides what it can
match; the browser decides what it can hide).

### Types (`rule.rs`)

```text
pub enum CosmeticAction { Hide, Exception }

pub struct CosmeticRule {
    pub selector: Arc<str>,          // trimmed, verbatim otherwise
    pub action: CosmeticAction,
    pub domains: Option<Arc<str>>,   // raw "a.com,~sub.a.com", lowercased, None when generic
}

pub enum RuleKind {
    Active(DomainRule),
    Url(Box<UrlRule>),
    Cosmetic(Box<CosmeticRule>),     // boxed: keeps ParsedRule at 32 bytes
    Inactive(InactiveReason),
}

pub enum InactiveReason {
    ExtendedCosmetic,                // replaces Cosmetic
    Unsupported,
}
```

`a_parsed_rule_is_32_bytes` must keep passing. Update every `match` on
`RuleKind` — `rule_list.rs` counts, `matcher.rs` builder, `api` tests that
construct `RuleCounts`.

Memory: EasyList carries ~24 k cosmetic lines; each now costs one `Arc<str>`
(selector) plus, for domain-scoped ones, a second. Measure it (Step 7) rather
than estimating.

## Step 2 — compiled form: `CosmeticIndex`

New file `crates/fah-rules/src/cosmetic.rs`. Built by `MatcherBuilder`, held
by `Matcher` as `cosmetic: CosmeticIndex`, exposed as
`Matcher::cosmetic(&self) -> &CosmeticIndex`. It swaps with the `Matcher`
`Arc`, so list refresh, `PATCH /lists/{id}` and policy edits all replace it
atomically for free.

### Shape — a representation the rewriter reads, never reaches into

```text
pub struct CosmeticIndex {
    selectors: Vec<Selector>,                 // interned, unique by text
    generic: Vec<SelectorId>,                 // sorted
    by_domain: HashMap<Arc<str>, DomainEntry>,// key: lowercased label suffix, no trailing dot
    lists: Vec<Arc<str>>,                     // list names for attribution
}
```

`by_domain` uses the std hasher on purpose: this index is probed once per
HTML response, not per DNS query, so the DNS tier's open-addressed table
(`hash_domain`, `matcher.rs` ~264) is not the model here. Write that
sentence in the review; if the Step 7 bench shows the probe, switch to
`hash_domain` then, not before (principle 8).

```text
pub struct Selector { text: Arc<str>, list: u16, mask: u16 }
pub struct DomainEntry {
    hides: Vec<SelectorId>,          // sorted: `d##s`
    exceptions: Vec<SelectorId>,     // sorted: `d#@#s`
    negated: Vec<SelectorId>,        // sorted: `~d##s` — generic elsewhere, suppressed here
}
pub struct SelectorId(u32);
```

- Derive `serde::Serialize` on all of it (serde is already a dependency of
  `fah-rules`). That is the whole "serializable" requirement: a shape that can
  be written out. **No endpoint, no protocol, no extension** (ROADMAP.md
  §Future — Browser Integration).
- Interning: identical selector text from two lists is one `Selector`; the
  `list` field keeps the **first** list that contributed it, matching how
  `Matcher::decisive_rule` names a list today (RULE_ENGINE.md §Deduplication).
  Record duplicates removed for `/lists` parity if cheap.
- `mask`: the per-policy list mask (`PolicySet::mask_for_list`) OR-ed across
  the lists that contributed the selector. Lookup filters by
  `mask & policy_bit != 0`. Without this a policy that excludes EasyList would
  still get EasyList's cosmetics on every page — the same defect the URL tier
  avoided with per-rule masks. One AND per selector; no second index.

### Domain-scoping semantics (write these into RULE_ENGINE.md)

| Line | Effect |
| ---- | ------ |
| `##.ad` | generic: every host |
| `a.com##.ad` | `a.com` and every subdomain |
| `a.com,b.com##.ad` | either |
| `a.com,~sub.a.com##.ad` | `a.com` and subdomains, except `sub.a.com` and its subdomains |
| `~a.com##.ad` | generic everywhere except `a.com` and subdomains |
| `#@#.ad` | generic exception: `.ad` is dropped from `generic` **and from every `by_domain[*].hides`** at compile time — an exception at any level beats a hide at any level, so `a.com##.ad` does not survive it |
| `a.com#@#.ad` | on `a.com` and subdomains, `.ad` is suppressed whether it came from generic or from any specific entry |

Precedence: an exception at any matching level wins over a hide at any level
(the allow-over-block analog). A negation (`~`) applies to that rule's own
selector only. Subdomain match is `host == d || host.ends_with(".d")`,
computed by walking label suffixes, never by string search.

Interning makes both rules **per selector text, not per rule**: `~sub.a.com`
from `a.com,~sub.a.com##.ad` also suppresses an explicit `sub.a.com##.ad`
from another list, because both are the one `.ad` id. Accept it — the
alternative is one entry per rule, which is the duplicate storage principle
13 forbids — and write the sentence into RULE_ENGINE.md.

Compile-time pre-joins that keep lookup allocation-free:

- Generic exceptions are applied at compile time: removed from `generic` and
  from every `hides` vector.
- Every `~d` goes to `by_domain[d].negated`, whether the rule was generic
  (`~a.com##.ad`) or domain-scoped (`a.com,~sub.a.com##.ad`); lookup checks
  `negated` against generic ids **and** `hides` ids.
- `d#@#s` goes to `by_domain[d].exceptions`; `d##s` to `by_domain[d].hides`.
- All three vectors are sorted so lookup can binary-search.

## Step 3 — lookup

```text
impl CosmeticIndex {
    pub fn lookup<'a>(&'a self, host: &str, policy: PolicyId) -> CosmeticLookup<'a>;
    pub fn is_empty(&self) -> bool;                // fast: no selectors at all
    pub fn heap_bytes(&self) -> usize;
    pub fn len(&self) -> usize;
}

pub struct CosmeticLookup<'a> {
    index: &'a CosmeticIndex,
    matched: [Option<&'a DomainEntry>; MAX_LABELS],   // MAX_LABELS = 16, stack
    matched_len: u8,
    policy_bit: u16,
}
impl<'a> CosmeticLookup<'a> {
    pub fn is_empty(&self) -> bool;
    pub fn for_each(&self, f: impl FnMut(&'a Selector));  // effective set, exceptions applied
    pub fn count(&self) -> usize;
    pub fn fingerprint(&self) -> SetKey;                   // equal ⇔ equal effective sets
    pub fn deciding(&self, selector: &str) -> Option<(&'a str, &'a str)>; // (list, canonical rule) for the API
}
```

- `policy` is `fah_model::PolicyId` (`policy.rs` ~21, a `u8` newtype);
  `fah-rules` does not re-export it.
- `lookup` normalises the host itself — the proxy's `Destination.host` is
  **not** normalised: `request_host` (`fah-http/src/request.rs` ~80) only
  strips the port, and `claim.rs` neither lowercases nor strips a trailing
  dot. Copy the host ASCII-lowercased into a `[u8; 256]` stack buffer
  (hosts are ≤ 253 bytes; longer ⇒ empty lookup), drop one trailing `.`,
  then walk the label suffixes of that buffer, probing `by_domain` with a
  `&str` slice once per suffix: O(labels), no allocation. Hosts with more
  than `MAX_LABELS` labels are truncated to the last 16 — note it in the
  review.
- `for_each` yields: every `generic` id not present in any matched
  `negated`/`exceptions` vector, then every matched `hides` id not present in
  any matched `negated`/`exceptions` vector, filtered by `mask & policy_bit`.
  Presence checks are binary searches over the (≤ 16) matched sorted
  vectors. Bounded, allocation-free.
- `fingerprint`: hash of `(Matcher generation, policy_bit, matched entry
  addresses in suffix order)`. Two hosts with the same matched entries — which
  is nearly every host, since most only hit `generic` — share a key, so
  p4-03's cache compiles the generic set once instead of once per host.
  Include a generation counter (`Matcher::generation()`, incremented per
  `build`) so a swapped ruleset never collides with a stale key.
- **Reality check to write in the review:** once EasyList is loaded,
  `generic` is non-empty, so nearly every `text/html` response is a rewrite
  candidate. The pass-through fast path for HTML is then decided by
  `Content-Type`, not by this lookup. That is fine — it is what the feature
  is — but the task's "hosts with no applicable selectors" case is rare in
  practice and must not be the only thing the benches exercise.

## Step 4 — `Matcher` and lifecycle plumbing

| Place | Change |
| ----- | ------ |
| `MatcherBuilder::add_parsed_list*` (`matcher.rs` ~488–515) | Route `RuleKind::Cosmetic` to `CosmeticIndexBuilder::add(list_id, mask, &rule)`, next to `add_url_rule`. |
| `MatcherBuilder::build` (~737) | Build the index; store it; bump `generation`. |
| `Matcher` (~799) | `cosmetic: CosmeticIndex`, `pub fn cosmetic(&self)`, `pub fn generation(&self) -> u64`; include `cosmetic.heap_bytes()` in `heap_bytes()`. |
| `RefreshStats` / compiled counts (`lifecycle/mod.rs` ~114) | Carry `cosmetic` so `/lists` can report it. |
| `fah_model::RulesetInfo` (`engine.rs` ~62) | **No change.** It carries `rules`, `duplicates_removed` and `compile_duration` only — no per-kind counts; those are `/lists`' job. Do not add one. |

Hot path check: the DNS and URL lookups do not touch the index. Only the
proxy's HTML plan (p4-04) calls `lookup`, once per HTML candidate response.

## Step 5 — counts and API

| Place | Change |
| ----- | ------ |
| `RuleCounts` (`rule_list.rs`) | `cosmetic: usize`; `inactive` now counts only `Inactive(_)`. Add `cosmetic_count()`. |
| `ListResponse` (`fah-api/src/wire.rs` ~700) | `rules_active_cosmetic: usize` after `rules_active_url`; `rules_total` sums four; the `rules_inactive` doc line lists "extended cosmetics, unsupported patterns". |
| `routes.rs` ~525 | Destructure the fourth count. Update the tests at ~1927–2002 that build counts by hand. |
| `dashboard/frontend/src/api/types.ts`, `pages/lists/list-row.tsx` + tests | Add the field and a column/cell. The comment hook applies to `.ts`/`.tsx` too. |
| `POST /api/v1/rules/test` (`RuleTestRequest`/`RuleTestResponse`, `wire.rs` ~791–815) | Optional request field `"cosmetic": true`. When set, resolve `PolicyId` exactly as today, then add to the response: `"cosmetic": { "selectors": <count>, "sample": [ { "selector", "list", "rule" } … ≤ 50 ], "generic": <n>, "specific": <n>, "exceptions_applied": <n> }`. `rule` is reconstructed canonical syntax (`a.com##.x`), `list` from `Selector.list`. Existing fields unchanged. |

No new endpoint.

## Step 6 — tests

| Test | Where | Proves |
| ---- | ----- | ------ |
| Marker classification table | `parser/adblock.rs` tests | Every row of the marker table, plus procedural `##` lines, plus the four rejection cases counted in `parse_errors` |
| Scoping table | `cosmetic.rs` tests | Each row of the domain-scoping table, plus deep subdomains (`x.y.z.a.com`), a host longer than `MAX_LABELS`, a mixed-case host with a trailing dot (`WWW.A.COM.`), and `a.com,~sub.a.com##.ad` on `sub.a.com` and `deep.sub.a.com` (both empty) |
| Exception precedence | `cosmetic.rs` tests | Generic hide + specific exception; specific hide + parent-level exception; generic exception + specific hide (`#@#.ad` beats `a.com##.ad`); `#@#` from a different list than the `##` |
| Policy mask | `cosmetic.rs` tests | Two policies, one excludes the contributing list ⇒ empty set for it, non-empty for the other |
| Fingerprint | `cosmetic.rs` tests | Same matched entries ⇒ equal; different policy ⇒ different; rebuilt index ⇒ different |
| Real EasyList sample | `crates/fah-rules/tests/cosmetic_easylist.rs` with a ~300-line fixture (attribution + licence note in a `.txt` sibling, not in Rust) | Counts by kind; five hand-checked hosts' effective sets |
| Swap mid-lookup | extend `concurrent_lookups_during_repeated_swaps_never_panic_or_see_a_bad_state` (`lifecycle/mod.rs` ~2994) | Readers hammering `cosmetic().lookup` while the manager swaps |
| No allocation on lookup | `crates/fah-rules/tests/cosmetic_lookup_alloc.rs`, copy the counting-allocator harness from `url_lookup_alloc.rs` | `lookup` + `for_each` + `fingerprint` allocate zero bytes for a 6-label host against the EasyList fixture |
| Size pin | `rule.rs` | `ParsedRule` still 32 bytes |
| API | `routes.rs` tests | `/lists` fourth count; `/rules/test` with `"cosmetic": true` names list and rule |

## Step 7 — bench and memory measurement

- `crates/fah-rules/benches/cosmetic_lookup.rs` (+ `[[bench]]` in
  `Cargo.toml`, `harness = false`): lookup for 1-, 3- and 6-label hosts,
  generic-only and specific-hit, EasyList-scale index; and `build` time for
  the index alone.
- Memory and startup: extend the existing `urlbench` example
  (`cargo run --release -p fah-rules --example urlbench`, ADR-0009) or add
  `cosmeticbench` to print cosmetic counts, `cosmetic.heap_bytes()`, total
  `heap_bytes()` before/after, and parse+compile wall time A/B against a
  pre-change checkout. Corpus: the sixteen configured lists **plus** EasyList
  and EasyPrivacy (the deployed corpus carries 39 inactive rules and would
  show nothing).
- Record every figure with corpus, workload and device in the review file;
  convert with the ~9× factor for the RB5009 column and say it is converted.
  Budget context: compiled ruleset ≤ 40 MB (PERFORMANCE.md); startup < 3 s.

## Step 8 — docs (list, then wait)

| File | Edit |
| ---- | ---- |
| RULE_ENGINE.md §Supported formats | The `inactive` bullet: cosmetic moves to a new `page-applicable` bullet ("`##` / `#@#`, active since Phase 4 (p4-02); see §Cosmetic matching"); `inactive` keeps extended cosmetics (`#?#`, `#$#`, `#%#`, procedural pseudo-classes) and unsupported patterns. |
| RULE_ENGINE.md new §Cosmetic matching (after §HTTP matching) | The domain-scoping table, the precedence rule, the procedural list, the interning + first-list attribution rule, the policy-mask rule, and the O(labels) lookup with its `MAX_LABELS` bound. Tables, not prose. |
| RULE_ENGINE.md §Compiled matcher | One line: the cosmetic index is part of the `Matcher` and swaps with it. |
| API.md §`GET /api/v1/lists` | `rules_active_cosmetic` field row; `rules_inactive` meaning updated. |
| API.md §`POST /api/v1/rules/test` | The `cosmetic` request flag and response object, one example. |
| CONTEXT.md §Rule | Done in p4-01 (`page-applicable`); verify the wording matches what shipped. |

## Step 9 — gates, review file, stop

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
cargo bench -p fah-rules --bench cosmetic_lookup
cargo bench -p fah-rules --bench matcher --bench url_matcher   # A/B: unchanged
```

Dashboard: run its own test command from `dashboard/frontend/` (check
`package.json`) since the gates above do not cover it.

Review file `docs/code-review/phase4/p4-02-cosmetic-rules-activation-review.md`
with: counts by kind on the measured corpus, heap and startup deltas, lookup
bench table, the `#%#` finding if any, the `MAX_LABELS` and mask decisions,
docs done/pending.

Chat line:

`Task done. Report written to docs/code-review/phase4/p4-02-cosmetic-rules-activation-review.md. Awaiting "start code review".`

## Out of scope

- Translating selectors to lol_html, deciding hide vs remove, any HTML byte
  (p4-03).
- Extended/procedural cosmetics beyond counting them (backlog).
- Any endpoint that serves the compiled form.

## Hand-off facts for p4-03 / p4-04

- `Matcher::cosmetic().lookup(host, policy) -> CosmeticLookup` — call once per
  HTML candidate, on the response head, after `Content-Type` has qualified.
- `CosmeticLookup::fingerprint() -> SetKey` is the cache key; `for_each` is
  the only way to read selectors; `Selector.text` is `&str`.
- `Matcher::generation()` changes on every swap; a cached compiled set keyed
  by a `SetKey` from an older generation is stale by construction.
- `lookup` takes the raw `Destination.host` and normalises it itself; the
  caller passes what it has.
- `Judged` in `fah-http` (`proxy.rs` ~560) carries the policy *name* only
  today; p4-04 adds the `PolicyId` (from `ClientContext.policy` in
  `judge()`) and the `Arc<Matcher>` the request was judged with.
