# P2-00 — Adblock Parser Correctness

**Phase:** 2 · **Depends on:** — · **Model:** Opus

> **Not HTTP work.** This corrects the `fah-rules` foundation and would sit in
> Phase 1 if that phase were still open (`p1-06` is taken by
> `p1-06-upstreams.md`, and Phase 1 is closed). It carries a `p2-` prefix
> because `NN` is a scheduling position inside the active phase, nothing more.
> It runs **first**: `p2-03` is blocked on it, and it touches no HTTP code.

## Goal

The EasyList-family parser classifies real-world lists correctly: EasyList and
EasyPrivacy are recognized as what they are, and a rule qualified by a path or
wildcard never compiles into a whole-domain DNS verdict.

## Context

Full findings, measurements and reproduction:
[docs/code-review/phase2/p2-03-headroom-and-parser-findings.md](../../../docs/code-review/phase2/p2-03-headroom-and-parser-findings.md).

Two defects, discovered while measuring `p2-03`'s headroom against the real
lists Phase 2 exists to support.

**U1 — real EasyList is not detected as EasyList.**
[format.rs:20-52](../../../crates/fah-rules/src/format.rs#L20-L52) classifies a
list from its **first content line only**. EasyList's is
`&rb=&uuid=$third-party`; EasyPrivacy's is `&&sub19=undefined&sub20=undefined`.
Neither carries `||`, `@@` or a cosmetic marker, so both fall through to
`PlainDomainList`. Measured on the shipped parser, real EasyList yields **83
rules and 69,514 parse errors** — the 83 being banner-size tokens (`_300x250`,
`_468x60`) that `normalize_domain` accepts as domains.

**U2 — `||domain^<suffix>` silently becomes a whole-domain DNS rule.**
[adblock.rs:56-59](../../../crates/fah-rules/src/parser/adblock.rs#L56-L59):

```rust
Some('^') => after_anchor[end + 1..].strip_prefix('$').unwrap_or(""),
```

When the text after `^` does not start with `$`, `unwrap_or("")` **discards
it** and the rule compiles as a bare `||domain^` — active, subdomain-inclusive.
So `||paypal.com^*/pixel.gif$third-party` becomes **block paypal.com**. 389
rules across EasyList + EasyPrivacy; 291 of them over-blocks, including
`googleapis.com`, `amazonaws.com`, `cloudfront.net`, `akamai.net`,
`jsdelivr.net`, `connect.facebook.net`, `paypal.com`, `ebay.com`, `x.com`,
`bing.com`, `baidu.com`, `citi.com`.

**The two interact, and the order of the fix is not optional.** U1 currently
*masks* U2: EasyList-family lists never reach the adblock parser, so their
`^*/path` rules never compile. Fixing detection alone removes the mask and
breaks browsing on the next list refresh.

> **Fix U2 first, then U1, in one change. Never U1 alone.**

## Scope

- **U2 — split the two suffix forms** in `adblock.rs`. They are not the same
  defect and must not get the same fix:
  - **`^|`** (end-of-address anchor) — a legitimate DNS rule form; AdGuard
    treats `||d^|` as `||d^` for DNS. Keep it **active**, with
    `include_subdomains: false`, which is what `|` actually anchors.
  - **`^<path / wildcard / anything else>`** — not domain-only. Classify
    `InactiveReason::UrlPattern`. This is precisely the bucket `p2-03`
    activates, so the rules land where they are needed.
  - Note **no parser emits `include_subdomains: false` today** —
    [domain_list.rs:27](../../../crates/fah-rules/src/parser/domain_list.rs#L27),
    [hosts.rs:73](../../../crates/fah-rules/src/parser/hosts.rs#L73) and
    [adblock.rs:78](../../../crates/fah-rules/src/parser/adblock.rs#L78) all
    hard-code `true`. The flag is honored at lookup
    ([matcher.rs:586](../../../crates/fah-rules/src/matcher.rs#L586)) and unit
    tested, but this is its first production use. Prove it with a fixture.
- **U1 — detect from a sample, not from line one.** Score the first N content
  lines (N bounded, e.g. 100) and take the majority format; a single
  unrepresentative line must not decide. Keep detection allocation-light — it
  runs per refresh, not per query, but the same list can be tens of MB.
- **Surface detection failure instead of swallowing it.** A parse that produces
  83 rules and 69,514 errors currently reports `RefreshResult::Ok`
  ([lifecycle/mod.rs:111-117](../../../crates/fah-rules/src/lifecycle/mod.rs#L111-L117))
  — nothing distinguishes it from a healthy load, which is why this hid. A high
  parse-error ratio is a *detection* failure, not thousands of line failures:
  report it as such through `ListStatus`, and log it at `warn` once per refresh
  with the id, ratio and detected format. Bounded output — no per-line spam.
- **RULE_ENGINE.md** in the same change: document format detection (sampling,
  and what a detection failure looks like) and the `^|` vs `^<path>` semantics.
  CONTEXT.md only if a term changes.
- **Regression corpus.** Add the EasyList/EasyPrivacy heads as fixtures (a
  bounded excerpt, license-permitting — the first ~200 content lines is enough
  to pin U1). Do not vendor multi-MB lists into the repo.

## Acceptance criteria

- **0 domains lose an exception.** The 171 `@@||domain^|` rules in the
  reference ruleset stay active allow rules. An exception silently becoming
  inactive is a regression, not a cleanup.
- **0 new domain blocks are introduced by URL-rule parsing.** Explicit test:
  `||paypal.com^*/pixel.gif` must **not** produce a block verdict for
  `paypal.com` (nor any subdomain). Same for `||googleapis.com^*/gen_204?` and
  `||amazonaws.com^*/prod_analytics`. This is the critical bug the review
  found; it gets its own named test, not a line in a larger one.
- **Before/after diff of the compiled ruleset against the reference ruleset
  used for deployment.** Expected delta: 172 rules change shape, 0 domains lose
  an exception, 0 domains gain a block. Any other movement is a regression in a
  filter set that is live in a household — investigate before proceeding.
- Real EasyList detects as `Adblock` and parses with **0 parse errors**
  (measured: 83,239 rules — 49,767 DNS-active, 24,368 cosmetic, 3,689
  `UrlPattern`, 5,414 `HttpOption`). EasyPrivacy likewise.
- `||d^|` matches `d` and **not** `sub.d`; `@@||d^|` allows `d` and not `sub.d`.
- A list whose parse-error ratio is high reports a distinguishable status, not
  `Ok`, and logs once at `warn`.
- RULE_ENGINE.md updated in the same change.
- Gates green (`fmt --check`, `clippy -D warnings`, `test --workspace`).

## Out of scope

Activating URL rules (`p2-03` — this task only ensures they are *classified*
correctly and reach the right bucket). Cosmetic rules (Phase 4). Any HTTP code.
`normalize_domain` accepting `_300x250` — noted, harmless once detection is
fixed, and tightening it risks rejecting valid unusual labels; raise separately
if it ever matters.

## Suggested prompt

> Read RULE_ENGINE.md, docs/code-review/phase2/p2-03-headroom-and-parser-findings.md,
> and plan/wip/phase2/p2-00-adblock-parser-correctness.md. Fix U2 first
> (`^|` stays active with include_subdomains:false; `^<path>` becomes
> UrlPattern), then U1 (sample-based format detection + a distinguishable
> status for a high parse-error ratio). Add the fixtures and the named
> over-block regression test, update RULE_ENGINE.md, and report the
> before/after compiled-ruleset diff against the reference ruleset.
