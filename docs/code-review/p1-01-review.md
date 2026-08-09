# Code Review — p1-01 Rule Parsers

**Task:** [plan/closed/phase1/p1-01-rule-parsers.md](../../plan/closed/phase1/p1-01-rule-parsers.md)
**Reviewed:** 2026-08-09 · **M1–M3 and m1–m4 fixed the same day**, gates green
(886 tests), A/B'd against a `689d9c5` worktree on the deployed corpus
**Reviewed at:** `689d9c5` — the code as it stands, including p2-00/p2-03/p2-05 changes to it

## Summary

The only Phase-1 task that never got a review. Ownership, borrowing and
lifetimes are clean: parsers borrow `&str` end-to-end, the only owned data is
the `Arc<str>` payloads that outlive the text by design, no `unsafe`, no
interior mutability. **No Critical findings.** Five Major, five Minor, five
Nitpick — m4 was filed Minor and re-rated Major once measured. Eight are fixed;
m5, m6 and the nitpicks stand. On Windows/x86 over the deployed corpus, **boot
measured 398.7 → 290.6 ms (≈ −27 %)**, almost all of it m4, and **M4 took the
compile transient's peak down a further 12.00 MB**. Not measured on the RB5009.

Two of the Majors are the same root cause: the comment-prefix test and the
"refuse where it is counted" rule are each hand-copied per parser, and the
copies disagree.

**Scope:** `format.rs`, `parser/{mod,hosts,domain_list,adblock}.rs`,
`domain.rs`, `rule.rs`, `rule_list.rs`, `resource.rs`, `tests/parsers.rs`.

## Decisions

- Parsers are pure and allocation-bounded per rule, not per query — the
  acceptance criterion holds. Nothing here touches the hot path.
- `Arc<str>` for option payloads is correct: the compiled side clones them into
  side maps, so the clone is a refcount bump, not a copy.
- Format detection by sampling (not line one) is right, and the rationale in
  `format.rs` is the kind that must not be re-derived. Left alone.
- M4 is **not** the streaming-parse rewrite `p2-12` declined; it is the local
  partial that document did not size.

## Bugs found

Line numbers are the reviewed revision `689d9c5`. **m4 keeps its id but is a
Major** — it was filed Minor on inspection and re-rated once measured: it is the
largest single win of the whole set, worth more than the other six together.

| # | Sev | Where | Defect | State |
| - | --- | ----- | ------ | ----- |
| M1 | Major | `domain.rs:8`, `matcher.rs:481`,`:510` | Parser accepts, compiler drops silently | **fixed** |
| M2 | Major | `parser/adblock.rs:43` | `#` is a comment in 3 parsers, a live rule in the 4th | **fixed** |
| M3 | Major | `format.rs:137` | One heap allocation per hosts line | **fixed** |
| M4 | Major | `rule.rs:18` | `ParsedRule` is 80 B; 48 B of it is unused on ~every rule | **fixed** |
| m1 | Minor | `parser/domain_list.rs:18` | Whitespace pre-check is dead — `normalize_domain` already rejects it | **fixed** |
| m2 | Minor | `rule_list.rs:59` | 4 passes over the rule vec to produce 3 counters | **fixed** |
| m3 | Minor | `parser/adblock.rs:225`,`:346`,`:356` | `Arc::from(x.to_ascii_lowercase().as_str())` — String, then a second copy | **fixed** |
| m4 | **Major** | `parser/mod.rs:30` | `rule_upper_bound` tokenizes the whole text; only hosts needs tokens | **fixed** |
| m5 | Minor | `parser/adblock.rs:47` | 4 substring scans/line; swallows a URL rule containing `##` | open |
| m6 | Minor | `rule.rs:142` | `ParsedRule` is a single-field newtype with no invariant | open |
| n1 | Nit | all parsers | No BOM strip — a UTF-8 BOM costs the first line | open |
| n2 | Nit | `resource.rs:29` | `ALL_TYPES` hardcodes `12` instead of `TYPES.len()` | open |
| n3 | Nit | `parser/hosts.rs:30` | Up to 11 `eq_ignore_ascii_case` per host token | open |
| n4 | Nit | `parser/adblock.rs:112` | `\\$` mis-escapes; a retained `\$` can never match | open |
| n5 | Nit | `domain.rs:8` | Accepts empty labels (`a..b`) and bare IPs | open |

### M1 — accepted by the parser, dropped by the compiler · fixed

`normalize_domain` applies no length bound. `MatcherBuilder::add_rule` returns
early when `u8::try_from(domain.len())` fails, and again when
`ClientScope::parse` returns `None`. Both rules were already counted by
`active_count()` → `RefreshStats.active` → the API's DNS rule count.

Same defect class p2-03 fixed for the URL tier, with the rule stated verbatim
at `parser/adblock.rs:205-217` ("Refused *here*, where it is counted"). The
domain tier never got it. Reachable from any list and from
`PUT /api/v1/rules/user`.

**Fixed:** `MAX_DOMAIN_LEN = 253` in `normalize_domain`; `$client` validated
with `ClientScope::parse` — the compiler's own parser, so the two cannot
disagree — and classified `Unsupported` otherwise. The throwaway `ClientScope`
costs one allocation on the handful of rules carrying `$client` (0 across the
deployed corpus, 1 apiece in EasyList/EasyPrivacy). The fail-closed drops in
`add_rule` and `UrlIndexBuilder::add` stay: both are reachable through the `pub`
builder without a parser.

### M2 — `#` comments become live URL rules · fixed

`format.rs:47`, `parser/mod.rs:34` and `parser/domain_list.rs:15` treat `#` as
a comment. `parser/adblock.rs:43` skips only `!` and `[`. A `# comment` line
then misses all four cosmetic markers, misses `domain_rule`, and reaches
`url_rule` as an active substring pattern.

Over-block risk is low (URLs rarely carry spaces), but the entry is counted in
`rules_active_url` — the exact lie that counter was split out to end. Also
undershoots `rule_upper_bound`'s ceiling; benign, `reserve_dedup` grows.

**Fixed:** one `format::is_ignorable` predicate for the detector,
`rule_upper_bound`, `hosts` and `domain_list`. The adblock parser splits the
test around its marker scan — `!`/`[` before it, `#` after it — because `###id`
opens with `#` and is a rule. Both orderings are pinned by a test.

Incidental: a hosts file carrying `!` or `[` lines now skips them instead of
counting parse errors.

### M3 — allocation per hosts line · fixed

`looks_like_ip` does `token.split('.').collect::<Vec<&str>>()`, and
`hosts::parse` calls it once per non-comment line — ~1.19M alloc/free pairs on
the phase measured at 81 % of a 2.44 s startup (PERFORMANCE.md).

**Fixed:** the parts are walked in place — 0 allocations per line, down from 1.
Measured: the all-hosts bench moves 180.2 → 143.9 ms (−20.1 %), the mixed real
corpus −4.2 % to −8.4 % depending on how much of it is hosts-format.

### M4 — `ParsedRule` layout · measured, then fixed

Measured 2026-08-10 at `2271a10`, Windows/x86, mimalloc v3.3.2, over the 17
lists refetched 2026-08-09. Instruments: [m4probe.rs](p1-01-ab/m4probe.rs)
(layout, occupancy), [szprobe.rs](p2-12-attribution/szprobe.rs) (transient).

48 of the 80 B are three `Option<Arc<str>>`. `MatcherBuilder` keeps those three
in side maps keyed by record index (`matcher.rs:326-332`); the parsed side has
no equivalent. Boxing them and `RuleKind::Url` — sizes from the compiler, not
arithmetic:

| Type | Now | After M4 |
| --- | ---: | ---: |
| `ParsedRule` | 80 B | **32 B** |
| `DomainRule` | 72 B | 32 B (opts boxed, 48 B) |
| `UrlRule` | 72 B | 8 B (behind a `Box`) |

Cost: **1,803 boxes over 964,154 rules — 0.19 %.**

| Field behind the box | Rules setting it | Share |
| --- | ---: | ---: |
| any of the three, active tier | 1,084 of 963,397 | 0.11 % |
| — `$dnstype` / `$dnsrewrite` / `$client` | 0 / 1,084 / 0 | all in `filter_59` |
| `RuleKind::Url` | 719 of 964,154 | 0.07 % |
| — `$domain` / `$method` / `$client` | 0 / 0 / 0 | — |

Lists are parsed one at a time and dropped (`lifecycle/mod.rs:1096-1119`), so
the win is the largest single list's `Vec` spine:

| Corpus | Peak in | Spine | After M4 | Δ |
| --- | --- | ---: | ---: | ---: |
| refetched, 964,154 rules | `filter_48` | 20.00 MB | 8.00 MB | **−12.00 MB** |
| device `/data`, 1,148,780 rules | `big.oisd.nl` | 40.00 MB | 16.00 MB | **−24 MB** |

**The two corpora differ by one list**: `big.oisd.nl` publishes 253,558 entries
(its own header, 2026-08-09T17:05:33Z) against the 436,345 the device's `/data`
holds. `p2-12`'s absolute figures scope to its pinned copy — on the refetched
corpus the transient peaks at 105.93 MB, not 132.45 MB, and inside a different
list. Both savings clear CLAUDE.md's >5 MB keep rung.

**Fixed — what the change actually bought**, same probe, same corpus:

| | before | after |
| --- | ---: | ---: |
| peak, requested | 105.93 MB | **93.93 MB** (−12.00) |
| peak, mimalloc usable | 119.96 MB | 107.96 MB (−12.00) |
| copying reallocs >1 MiB | 76 moves, 476.19 MB | **48 moves, 231.69 MB** |
| compiled ruleset | 616,087 rules, 19.65 MB | identical |

The realloc collapse is the part the sizing did not predict: 32-byte elements
halve the bytes the `Vec` doublings copy, and that shows up as CPU. Two arms,
`2271a10` vs the working tree, alternating, two passes agreeing within 1 %:

| Bench | before | after | Δ | layout floor (p1-01 A/B) |
| --- | ---: | ---: | ---: | ---: |
| `2_parse_rule_list` | 151.9 / 155.8 ms | 131.0 / 132.4 ms | **−14.3 %** | +1.2 % |
| `startup_from_cached_lists` | 338.2 / 337.1 ms | 313.2 / 313.2 ms | **−7.2 %** | −0.6 % |
| `3_build_matcher` | 118.2 ms | 111.6 ms | −5.6 % | +7.9 % |
| `blocked_query` | 1.766 / 1.735 µs | 1.812 / 1.810 µs | +3.4 % | **+11.7 %** |

`blocked_query` moves inside its own layout floor and the compiled matcher it
exercises is byte-identical, so the hot path is untouched — as designed, since
nothing here reaches past `MatcherBuilder`. `3_build_matcher`'s first baseline
pass is discarded: its CI spanned 120–298 ms.

### m1–m4 · fixed

- **m1** — the pre-check and `normalize_domain` record the same error for the
  same lines, so the line is scanned once.
- **m2** — `RuleCounts` + `ParsedRuleList::counts()`, one exhaustive-match pass;
  the three accessors delegate to it. `inactive` is counted rather than
  subtracted, so a new `RuleKind` variant is a compile error, not a miscount.
- **m3** — `folded(text, AsciiCase)` in `parser/adblock.rs` covers all three
  sites. `normalize_domain` keeps its own fold, fused into the validity scan it
  must run anyway; routing it through the helper would add a second pass over
  1.19M domains. `$method` pays a scan it did not before — rare, tiny values.
- **m4 (Major)** — only a hosts list is tokenized; the other two bound by
  content line. `detect_format` runs twice per list as a result (≤200 sampled
  lines each, against a full-text `split_whitespace` saved — that one decodes
  every char, `lines()` does not). Ceiling *values* are unchanged for real
  lists, whose content lines carry one token anyway. **Roughly a quarter off
  boot on this corpus** — the ceiling was decoding all 22.8 MB so 3 of 16 lists
  could be bounded correctly, which is also why the win tracks format mix.

`an_adversarial_list_body_cannot_inflate_the_dedup_allocation`
(`matcher.rs:1648`) changed input: a whitespace-token body now detects as a
plain-domain list and bounds to 1. The attack still reaches `MAX_PREALLOC_RULES`
through hosts shape, so the body is `0.0.0.0 a a a…` and both assertions stand.

## Measurements

### A/B — M1–M3 + m1–m4, measured

**On Windows/x86, over the 16 deployed lists refetched 2026-08-09 (22.8 MB;
3 hosts, 12 adblock, 1 plain-domain), boot measured 398.7 → 290.6 ms, ≈ −27 %,
against a 1.4 % layout floor.**

Nothing here is an on-device figure. **PERFORMANCE.md's budgets must not be
updated from this**; that needs `/history/perf` on the RB5009.

**Corpus:** as above, compiling to 616,086 rules — a fresh download, not the
router's 798,760; §M4 sizes the gap. **Device:** x86 dev box, Windows, mimalloc,
pinned to 4 cores, box idle. **Method:** one compile per process; three arms, 11
iterations per arm per mode, arm order alternating, first 2 dropped. Raw output
and instrument in [p1-01-ab/](p1-01-ab/) (`run3-quiet-3arm.txt`).

Arms: `a` = `689d9c5`; `cur` = working tree; **`b` = `a` plus one never-called
`pub fn`** — identical behaviour, different code placement, so `layout vs a` is
the floor below which no delta means anything.

| Metric | before (a) | after (cur) | layout (b) | after vs a | layout vs a | spread a |
| --- | --- | --- | --- | --- | --- | --- |
| **boot — startup/refresh total** | 398.65 ms | **290.59 ms** | 404.33 ms | **−27.1 %** | +1.4 % | 7.8 % |
| parse total | 127.95 ms | **119.25 ms** | 128.25 ms | **−6.8 %** | +0.2 % | 1.9 % |
| ├ hosts, 3 lists | 13.16 ms | 12.06 ms | 13.20 ms | −8.4 % | +0.3 % | 10.1 % |
| ├ adblock, 12 lists | 101.47 ms | 97.20 ms | 101.87 ms | **−4.2 %** | +0.4 % | 2.1 % |
| └ plain-domain, 1 list | 13.29 ms | **9.98 ms** | 13.16 ms | **−24.9 %** | −1.0 % | 4.1 % |
| compile total | 251.35 ms | 244.61 ms | 254.14 ms | −2.7 % | +1.1 % | 3.5 % |
| add_parsed_list | 104.55 ms | 106.20 ms | 107.45 ms | +1.6 % | +2.8 % | 6.7 % |
| build | 18.56 ms | 18.86 ms | 18.75 ms | +1.6 % | +1.0 % | 10.9 % |
| control — FNV over corpus | 18.67 ms | 18.68 ms | 18.52 ms | +0.1 % | −0.8 % | 7.7 % |
| peak working set (phases) | 120.43 MiB | 120.43 MiB | 120.45 MiB | −0.0 % | +0.0 % | 0.4 % |
| peak working set (boot) | 149.04 MiB | 145.20 MiB | 149.06 MiB | −2.6 % | +0.0 % | 2.7 % |

Row confidence: **established** — boot, parse total, adblock, plain-domain, each
clearing its layout floor by 10× or more. **Not established here** — `hosts
−8.4 %`, whose delta sits inside the baseline arm's 10.1 % spread; the all-hosts
bench below carries that claim. **Marginal** — `compile total −2.7 %` against a
1.1 % floor. **Null** — `add_parsed_list`, `build`, both peaks, each blind to a
real 2 % move.

**The win tracks the corpus's format mix, not its size.** The pre-parse ceiling
still tokenizes hosts lists, so an all-hosts corpus sees almost none of the boot
improvement.

**Equivalence, all three arms, every iteration:** `compiled_rules=616086`,
`compiled_url_rules=719`, `compiled_heap_bytes=20607748`,
`parsed_active=963396`. No verdict changed.

**`parse_errors` 13 → 1**, with arm `b` still at 13 — so the drop is the code,
not the rebuild. All 12 came from `filter_2`, an AdGuard *hosts*-format list
carrying `!` headers the hosts parser counted as broken lines. M2's other half —
`#` comments compiled as URL rules — measured **zero** effect here: no adblock
list in this corpus uses `#` comments, so that fix is protection, not a win.

### General bench suite — `benches/pipeline.rs`, synthetic 1M-line hosts list

Same three arms, box idle, bench exes run directly per §Measuring reliably.

| Bench | before (a) | after (cur) | after vs a | layout floor (b vs a) |
| --- | --- | --- | --- | --- |
| `2_parse_rule_list` | 180.2 ms | **143.9 ms** | **−20.1 %** | +1.2 % |
| `startup_from_cached_lists` | 328.5 ms | **289.0 ms** | **−12.0 %** | −0.6 % |
| `4_build_two_overlapping` | 168.9 ms | 157.6 ms | −6.7 % | −3.2 % |
| `3_build_matcher` | 89.15 ms | 88.48 ms | −0.7 % | **+7.9 %** |
| `1_read_from_data` | 9.14 ms | 8.82 ms | −3.5 % | −5.7 % |
| `blocked_query` | 2.052 µs | 2.130 µs | +3.8 % | **+11.7 %** |
| `forwarded_query_overhead` | 2.819 µs | 2.856 µs | +1.3 % | **+15.0 %** |
| `sustained_throughput` (QPS) | 592.3 Ke/s | 590.6 Ke/s | −0.3 % | +3.3 % |
| peak working set | 284.88 MiB | 278.93 MiB | −2.1 % | −0.1 % |

**Parse reads −20.1 % here against −6.8 % on the real corpus.** Both are correct
for their input: this bench's list is 100 % hosts, the format where M3's removed
allocation has maximum leverage. **−6.8 % is the number that describes the
deployment**; −20.1 % is a best case.

**Cost did not move.** Every stage is down or inside its floor. Residual
accounting closes it: boot 398.7 = compile 251.4 + residual 147.3 → boot 290.6 =
compile 244.6 + residual 46.0. The residual (read + pre-parse ceiling +
lifecycle) fell; nothing rose. `matcher_lookup` and `policy_resolution` moved
≤1.6 %, inside their own layout floor — the hot path is untouched, as designed.

### Two wrong numbers this review produced

Both are recorded because the raw output of the failed passes is also checked
in, and someone would otherwise re-derive them.

- **boot −53.1 % is superseded by −27.1 %.** The first A/B ran while the box
  played a fullscreen video. The baseline arm does more work and suffered more
  from contention, so the *gap* inflated: baseline 680.9 ms loaded vs 398.7 ms
  idle, treatment 319.1 vs 290.6. Same code, same corpus, 2× different answer.
- **A reproducible −8 % on `matcher_lookup` was entirely the same background
  load.** It held across two mirrored passes with CIs under 3 %, on a path this
  changeset does not touch, and vanished on an idle box. It was diagnosed here
  as code layout; arm `b` disproves that — layout moves those benches ±1.4 %.

**A regression the gates could not see.** An earlier pass put adblock parse at
+6.2 %. Cause was in the M2 fix: `starts_with(['!', '['])` builds a multi-`char`
pattern searcher, plus a second `starts_with('#')`, both per line. One byte load
and three compares took the arm to −4.2 %. Gates were green throughout.

## Files changed

M1–M3 and m1–m4, gates green (fmt, clippy `-D warnings`, 886 tests across 41
binaries).

| File | Change |
| ---- | ------ |
| `format.rs` | `is_ignorable`; allocation-free `looks_like_ip`; edge test |
| `domain.rs` | `MAX_DOMAIN_LEN`; length test |
| `parser/adblock.rs` | `#` after the marker scan; `$client` validated; `folded`; 4 tests |
| `parser/mod.rs` | `is_ignorable`; hosts-only token count; bound test |
| `parser/hosts.rs` | `is_ignorable`; comment/header test |
| `parser/domain_list.rs` | `is_ignorable`; dead pre-check removed |
| `rule_list.rs`, `lib.rs` | `RuleCounts` + `counts()`, exported |
| `lifecycle/mod.rs` | `RefreshStats::from` walks the rules once |
| `matcher.rs` | adversarial-bound test re-pointed at hosts shape |
| `rule.rs` | **M4**: `DomainOpts` + `DomainRule::plain`/accessors, `Url(Box<UrlRule>)`, 3 layout tests |
| `parser/{adblock,hosts,domain_list}.rs` | M4 construction sites |
| `matcher.rs` | M4: one `opts` test replaces three field reads on `add_rule` |
| `docs/code-review/p1-01-ab/` | A/B instruments, runners, raw output (new) |

## Remaining TODOs

| Item | Recommendation |
| ---- | -------------- |
| M4 | Fixed — see §M4 |
| m5 | Carried from [p2-03-review.md](p2-03-review.md) §Raised, not fixed — still open |
| m6, n1, n2, n3, n4, n5 | Leave unless the file is opened for another reason |
| On-device confirmation | Every figure here is Windows/x86. Boot and peak RSS need a deploy the owner runs, over the `/data` bytes `0.2.14` compiled — the comparison expires when the scheduled refresh lands (≈ 2026-08-11T10:33Z) and `big.oisd.nl` drops to 253,558. PERFORMANCE.md's budget rows stay as they are until then |

## What is right

Sampling format detection and its documented failure history; the tie-break
rationale (misreading a domain list as adblock fails silently, the other
direction does not); the single-pass `has_uppercase` fold; `ParseErrorLog`'s
bounded line list; refusing an empty pattern and a zero-folding type set rather
than compiling a match-everything rule.
