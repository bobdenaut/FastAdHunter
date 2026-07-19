# Code Review — p1-02 Compiled Matcher

**Scope:** `crates/fah-rules/src/matcher.rs`, `benches/matcher.rs`,
`tests/matcher_properties.rs` · **Reviewer:** chief architect pass ·
**Date:** 2026-07-18 · **Status:** all fixes applied same day, gates green.

## Overall assessment

Architecture sound. The arena + 8-byte packed `Record` + fastrange
open-addressing index is the right shape for the PERFORMANCE.md budgets
(1M domains ≤ 40 MB, verdict < 1 ms, allocation-free lookup). The precedence
walk is correct: most-specific suffix first, `allow` returns immediately
(nothing beats allow), `block` deferred so a parent-label allow can still
override it. Probe termination guaranteed — load factor ~0.7 means the table
is never full. Property tests (order-permutation invariance) and the unit
suite cover the RULE_ENGINE.md semantics well. No memory-safety issues, no
unbounded growth.

Six findings, four requiring fixes.

## Findings

### 1. HIGH — hot-path allocation for `Other` query types

`rrtype_bit` called `name.to_ascii_uppercase()`, which allocates a `String`.
`lookup` computes `qtype_bit` on **every** query, and `QueryType::Other`
covers HTTPS/SVCB — types modern Apple and Chrome clients send constantly. So
a large fraction of real traffic allocated once per lookup, violating hard
rule 3 (allocation-free hot path) and the PERFORMANCE.md contract. The
completion note's "zero-alloc verified structurally" claim was false for this
path.

**Fix:** table walk with `eq_ignore_ascii_case` — 15 bounded comparisons,
zero allocation:

```rust
fn rrtype_bit(name: &str) -> u32 {
    const TYPES: [&str; 15] = [
        "A", "AAAA", "HTTPS", "SVCB", "CNAME", "MX", "TXT", "NS", "PTR",
        "SRV", "SOA", "CAA", "DS", "DNSKEY", "NAPTR",
    ];
    TYPES
        .iter()
        .position(|t| name.eq_ignore_ascii_case(t))
        .map_or(0, |i| 1 << i)
}
```

Regression test: `other_qtype_name_is_case_insensitive` (lowercase `"https"`
still matches a `$dnstype=HTTPS` rule).

### 2. MEDIUM — `$dnstype` negation silently dead

AdGuard supports `$dnstype=~A` ("every type except A"). `parse_dnstype_mask`
treated `~A` as an unknown token → mask 0 → the rule compiled but could never
match anything. Failed open (under-blocked — the safe direction), but
silently: the rule looked active and did nothing.

**Fix:** negation implemented. Purely negated values subtract from a
`KNOWN_TYPES_MASK` covering every recognized type; mixed values resolve as
`positive & !negated`:

```rust
const KNOWN_TYPES_MASK: u32 = (1 << 15) - 1;

fn parse_dnstype_mask(raw: &str) -> u32 {
    let mut positive = 0u32;
    let mut negated = 0u32;
    for token in raw.split(['|', ',']).map(str::trim) {
        if let Some(name) = token.strip_prefix('~') {
            negated |= rrtype_bit(name.trim());
        } else if !token.is_empty() {
            positive |= rrtype_bit(token);
        }
    }
    if positive == 0 && negated != 0 {
        KNOWN_TYPES_MASK & !negated
    } else {
        positive & !negated
    }
}
```

Documented limitation (unchanged, deliberate): types the matcher doesn't
recognize carry no bit, so even a negated rule never matches an unrecognized
query type — conservative, never over-blocks what we can't reason about.

Regression test: `dnstype_negation_matches_everything_except_listed`
(`$dnstype=~A` passes A, blocks AAAA and HTTPS).

### 3. LOW — malformed reconstructed rule text

`decisive_rule` with both options emitted `||d^$dnstype=A$dnsrewrite=X`.
AdGuard syntax is one `$` with comma-separated options:
`||d^$dnstype=A,dnsrewrite=X`. Query log / `rules/test` would have shown
non-parseable text. Reporting-only, no verdict impact.

**Fix:** separator tracking — first option gets `$`, subsequent get `,`.
Regression test: `decisive_rule_separates_multiple_options_with_comma`.

### 4. LOW — `heap_bytes` undercounts side-map payloads

Counted the `Arc` control block but not the payload string bytes for
`$dnstype`/`$dnsrewrite` entries. Zero such rules in the bench corpus, so the
28.3 MiB figure is unaffected — but the function feeds the 40 MB budget
claim, so the undercount was a correctness issue in the measurement itself.

**Fix:** per-entry accounting now includes `raw.len()`.

### 5. INFO — cross-list duplicate domains stored per copy (no change)

The same domain appearing in two lists (OISD + StevenBlack overlap is real,
~30–50%) costs two arena copies + two records + two slots. Correct per the
semantics — each list's rule is a distinct decisive rule — and the budget was
benched single-list. **Action for p1-03:** real memory scales with the *sum*
of list sizes, not the union; sizing/limits there must assume that.

### 6. INFO — module doc claimed arena is "lowercased" (doc fix)

`add_rule` copies domain bytes verbatim; lowercasing is the parser's
normalization. Matching works regardless (hash and comparison are both
case-insensitive) — only the reported `DecisiveRule` text would echo a
caller's raw case. Doc comment corrected to state the actual contract.

## Verification

- `cargo fmt --check` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — green (matcher unit tests now 18, incl. 3 new
  regression tests; property tests unchanged and passing)
- Bench figures unaffected (corpus has no `$dnstype`/`$dnsrewrite` rules;
  `rrtype_bit` change removes an allocation, adds none): 28.3 MiB / ~88 ns
  exact hit / ~313 ns deep subdomain / ~64 ns miss.
