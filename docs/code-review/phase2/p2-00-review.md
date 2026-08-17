# p2-00 — Adblock Parser Correctness

**Task:** [plan/wip/phase2/p2-00-adblock-parser-correctness.md](../../../plan/wip/phase2/p2-00-adblock-parser-correctness.md)
**Findings this closes:** [p2-03-headroom-and-parser-findings.md](p2-03-headroom-and-parser-findings.md) §2 (U1), §3 (U2)
**Gates:** `fmt --check` clean · `clippy --workspace --all-targets -D warnings` clean · `test --workspace` **476 passed, 0 failed** (was 460)

---

## 1. What changed

### U2 — `||domain^<suffix>` no longer collapses to a whole-domain rule

[adblock.rs](../../../crates/fah-rules/src/parser/adblock.rs) replaced

```rust
Some('^') => after_anchor[end + 1..].strip_prefix('$').unwrap_or(""),
```

with an explicit decision over what follows the separator. Only three things
keep the rule a *domain* rule — nothing, `$options`, or `|`:

| After `^` | Classification | `include_subdomains` |
| --------- | -------------- | -------------------- |
| *(empty)* | active DNS rule | `true` |
| `$opts` | active DNS rule | `true` |
| `\|` | active DNS rule | **`false`** |
| `\|$opts` | active DNS rule | **`false`** |
| anything else (`*/path`, `/p`, `*=`) | `InactiveReason::UrlPattern` | — |

`unwrap_or("")` was silently discarding the fifth row, which is how
`||paypal.com^*/pixel.gif$third-party` became a subdomain-inclusive block on
all of paypal.com.

The `|` row is the milder half, and it is deliberately **not** reclassified as
a URL pattern. 171 of the 172 affected rules in the deployed set are
`@@||domain^|` exceptions; making them inactive would delete those exceptions
and let other lists' block rules take effect on domains that are excepted
today. Keeping them active with `include_subdomains: false` preserves every one
and narrows what `|` actually anchors.

**This is the first production use of `include_subdomains: false`** — all three
parsers previously hard-coded `true`. The lookup path already honored the flag
([matcher.rs:586](../../../crates/fah-rules/src/matcher.rs#L586)) and had unit
coverage; a fixture test now exercises it end-to-end through a compiled
matcher.

### U1 — format detection votes over a sample

[format.rs](../../../crates/fah-rules/src/format.rs) now classifies up to
`SAMPLE_LINES = 200` content lines and takes the majority, stopping as soon as
the sample is full so a multi-MB list is never fully scanned.

First-line detection could not work on the lists Phase 2 targets: EasyList
opens with `&rb=&uuid=$third-party` and EasyPrivacy with
`&&sub19=undefined&sub20=undefined` — URL-substring patterns carrying no `||`,
`@@` or `##`.

Two choices worth recording:

- **The adblock markers are ones a hosts entry or bare domain can never
  contain** — `^`, `/`, a `$option` suffix, a leading/trailing `|`. `*` is
  deliberately excluded, because real plain domain lists carry `*.example.com`
  and treating those as adblock syntax would silently deactivate the list.
- **A tie resolves to `PlainDomainList`.** Misreading a domain list as adblock
  is the worse failure: every bare-domain line becomes an inactive URL pattern,
  so the list contributes nothing *and* reports zero parse errors. The loud
  failure is preferable to the silent one.

### The observability gap that hid all of this

A parse yielding 83 rules and 69,514 errors reported `RefreshResult::Ok` —
indistinguishable from a healthy load. Added
[`RefreshStats::looks_misparsed`](../../../crates/fah-rules/src/lifecycle/mod.rs):
more errors than rules, past a floor of 100 errors. It drives

- one `warn!` per misread list at compile time — bounded by list count, not by
  line count, because a list read as the wrong format is **one** fact;
- a new `last_status: "degraded"` in `GET /api/v1/lists`, documented in
  [API.md](../../../API.md).

---

## 2. Acceptance criteria — measured

Verified by compiling the **reference ruleset used for deployment** (all 15
configured lists, fetched from source) before and after, and diffing the full
verdict surface — every `(domain, action, subdomain-flag)` triple.

| Criterion | Result |
| --------- | ------ |
| **0 domains lose an exception** | ✅ 172 domains allowed before, **172 after** |
| **0 new domain blocks introduced by URL-rule parsing** | ✅ 682,734 domains blocked before, **682,734 after** |
| Expected delta only | ✅ 161 removed / 162 added, **all `subs` → `exact`**, nothing else moved |
| Real EasyList detects as `Adblock`, 0 parse errors | ✅ 83,239 rules, **0 errors** (was 83 rules / 69,514 errors) |
| `\|\|d^\|` matches `d`, not `sub.d` | ✅ named test |
| High parse-error ratio is distinguishable from `ok` | ✅ `degraded` + `warn` |
| RULE_ENGINE.md updated in the same change | ✅ + API.md |

The 162 added minus 161 removed is one net entry: a domain that now carries
both an `exact` rule and a `subs` rule from a different list, where the two
previously deduplicated into one.

### The Phase-2 target lists, post-fix

| List | Format | Active DNS | Errors | Cosmetic | UrlPattern | HttpOption |
| ---- | ------ | ---------: | -----: | -------: | ---------: | ---------: |
| EasyList | `Adblock` | 49,687 | **0** | 24,368 | 3,769 | 5,414 |
| EasyPrivacy | `Adblock` | 42,714 | **0** | 34 | 8,716 | 4,135 |

The counts reconcile exactly against the findings doc: EasyList 49,767 − 80
path-form rules = 49,687 active, and 3,689 + 80 = 3,769 URL patterns.
EasyPrivacy 43,023 − 309 = 42,714 and 8,407 + 309 = 8,716.

All 17 previously over-blocked domains are clear — `paypal.com`,
`googleapis.com`, `amazonaws.com`, `s3.amazonaws.com`, `ebay.com`,
`twitter.com`, `x.com`, `bing.com`, `baidu.com`, `citi.com`, `cloudfront.net`,
`akamai.net`, `jsdelivr.net`, `azureedge.net`, `connect.facebook.net`,
`deliveroo.com`, `qualtrics.com`.

---

## 3. Tests added

`crates/fah-rules/tests/parsers.rs`:

- **`path_qualified_rules_never_block_the_whole_domain`** — the named
  regression, ten rules verbatim from EasyList/EasyPrivacy, each asserting the
  domain *and* its `www.` subdomain stay `Pass`, and that the rule lands in the
  URL-pattern bucket rather than being dropped.
- **`domain_anchored_rules_stay_dns_rules`** — the same domains addressed as
  `||domain^`, asserting they still block with subdomain semantics. The pair
  pins the boundary from both sides, so neither the regression nor an
  over-correction can pass.
- **`end_anchored_rules_match_the_host_but_not_its_subdomains`** — `||d^|`
  blocks `d` and not `sub.d`; `@@||d^|` allows `d` and not `sub.d`.
- **`real_easylist_head_is_detected_as_adblock`** — over a new fixture holding
  the genuine first 30 content lines of EasyList.

`format.rs` unit tests cover the sampling itself: URL-substring patterns
detected without an anchor; a single stray adblock line failing to flip a
domain list; wildcard domain entries staying a domain list; hosts entries
outvoting their own domain tokens; detection stopping at the sample.

`lifecycle/mod.rs` covers `looks_misparsed` against the real EasyList numbers
and against ordinary line noise.

Two test-quality points came from review during implementation and are worth
keeping: locate the rule under test **by kind, not by index** (a parser that
later emits an extra entry would silently move it), and always pair a negative
assertion with its positive counterpart.

---

## 4. Notes and limits

- **No behavioural change to the running 0.2.4 beyond the 162 rule shapes
  above.** The deployed lists contain zero path-form rules; the whole live
  delta is `subs` → `exact` on end-anchored rules.
- **Not a hot-path change.** Both fixes are load-time only. Detection now scans
  up to 200 lines instead of 1 — negligible against parsing the list, and
  bounded regardless of list size.
- **Out of scope, deliberately:** `normalize_domain` accepts `_300x250`, which
  is how the misdetected EasyList produced 83 "domains". Harmless once
  detection is fixed, and tightening it risks rejecting valid unusual labels.
  Raise separately if it ever matters.
- **The 200-line sample is a heuristic**, not a proof. It is right for every
  list in the reference set and for both Phase-2 target lists; a pathological
  list whose first 200 content lines are unrepresentative of the rest would
  still be misread — and would now report `degraded` instead of hiding.
- The measurement harness used for the before/after diff was temporary and has
  been removed; §2's procedure is reproducible from the findings doc.
