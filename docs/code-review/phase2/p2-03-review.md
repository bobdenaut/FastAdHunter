# p2-03 — URL rules activation

The URL tier. EasyList's request-level rules stop being a counter and start
deciding requests: their text is retained, compiled into a second tier of the
same matcher, and answered through a second typed entry point.

No proxy wiring — that is p2-04. This task ends at `Matcher::lookup_http`.

## What shipped

| Crate | Change |
| --- | --- |
| `fah-model` (L1) | **new** `http.rs` — `HttpRequest`, `ResourceType` |
| `fah-rules` (L2) | `rule.rs` — **new** `UrlRule`, `RuleKind::Url`; `InactiveReason` narrowed |
| `fah-rules` (L2) | **new** `resource.rs` — `$script`/`~image` as a `u16` mask |
| `fah-rules` (L2) | `parser/adblock.rs` — rewritten around a three-way classification |
| `fah-rules` (L2) | **new** `url_matcher.rs` — arenas, 16 B records, token index, matcher |
| `fah-rules` (L2) | `matcher.rs` — `lookup_http`, `verdict_http`, tiered `RuleRef` |
| `fah-api` (L3) | `rules_active_url` split out of `rules_inactive` |

Docs updated in the same change: RULE_ENGINE.md (**new** §HTTP matching, plus
the classification and per-list-count sections), CONTEXT.md (four new binding
terms, one retired), API.md (`rules_active_url` and the partition it belongs
to).

## The three things this task actually had to get right

### 1. Retention, and what it cost

`ParsedRule` deliberately drops the line text — the comment on it records that
carrying text costs an allocation, a copy and a free per rule on the phase that
dominates startup. p2-03 reverses that **for URL rules only**, because unlike a
domain rule there is nothing to reconstruct the pattern from. Cosmetic rules
stay payload-free: they are 24,368 of EasyList alone and Phase 4 cannot use
them yet.

The task asked for a number rather than a shrug. Measured on the real lists,
both arms pinned to one core (unpinned criterion swung 4× on this box and
reported a 40 ms parse that was pure noise):

| Corpus | Pre-p2-03 | With retention | Delta |
| --- | ---: | ---: | ---: |
| EasyList (85,124 lines) | 11.24 ms | 12.71 ms | +1.47 ms (+13.1 %) |
| EasyPrivacy (56,425 lines) | 8.63 ms | 10.48 ms | +1.85 ms (+21.4 %) |
| **Both** | **19.87 ms** | **23.19 ms** | **+3.32 ms (+16.7 %)** |

**≈177 ns per retained rule**, 18,778 `Arc<str>` allocations, 434,927 bytes of
text. The baseline is the real pre-change parser, run from a detached worktree
at `HEAD` on the same corpora — not an estimate.

Against the on-device compile of the full ruleset (2.317 s, `p1.5-06`), a
+3.3 ms parse delta on this box is not a startup cost worth defending against.

### 2. The token index, and the property it must not break

Checking 18,778 rules per request is not viable, so each rule is filed under
one literal token and a lookup only checks rules whose token the URL contains
(the adblock-rust approach). The correctness hazard is specific: **a token that
could match part of a URL token is not findable by an exact probe.** `*track`
matches `mytrack`, whose only token is `mytrack`.

So the pattern's own delimiters decide the key kind — a separator or an anchor
at the pattern's edge, never a `*`:

| Bounded | Key | Reached by |
| --- | --- | --- |
| both sides | exact | the URL token itself |
| left only | prefix | the URL token's first 3 bytes |
| right only | suffix | the URL token's last 3 bytes |
| neither | — | the fallback set, checked every lookup |

The three kinds share one CSR table, separated by distinct FNV seeds so
`Exact("ads")` is never reachable by a prefix probe (asserted). Frequencies are
counted across the whole compile and each rule takes its **rarest** key, so
nothing lands in the bucket half the corpus shares.

This is guarded by an oracle, not by examples:
`the_index_decides_exactly_what_a_full_scan_decides` runs every URL through
both the index and a test-only linear scan over every record and requires them
to agree. An indexing bug is a rule that silently stops firing — invisible to a
latency bench and to any fixture test that does not happen to cover it.

### 3. Both tiers answer a request

`lookup_http` consults the URL tier **and** the domain tier under one
precedence order: any allow beats any block, whichever tier it came from.

Consulting only the URL tier would mean a host blocked for DNS is still fetched
over HTTP whenever a client resolved it some other way. Consulting them
independently would let a URL-tier block survive an `@@||cdn.example.com^`
exception — the exact failure mode that breaks a site when a list is enabled.
Both directions are tested.

`$dnstype`-restricted rules are excluded from the HTTP path: a request asks no
DNS question, so `$dnstype=A` has nothing to match, and applying it anyway
would let a record-type filter block a fetch. `lookup` and `lookup_http` share
one domain walk parameterised by `Option<qbit>`, so the two can't drift.

## Measurements

Real EasyList + EasyPrivacy, pinned to one core.

```text
[url tier] parsed  : 94010 dns, 18778 url, 27715 inactive
[url tier] compiled: 18778 url rules (0 duplicates removed), 77 unindexed
[url tier] heap    : 1114443 bytes (1.06 MiB)
[url tier] whole matcher heap: 4.05 MiB
[url tier] parse: 22 ms   compile: 16 ms

url_verdict/single_pass_request   [2.9241 µs 2.9293 µs 2.9352 µs]
url_verdict/mixed_requests (×4)   [14.208 µs 14.242 µs 14.275 µs]
```

**Verdict latency: 2.93 µs** for a request nothing matches, against the < 1 ms
budget — a 340× margin. Allocation-free, asserted rather than inferred:
`tests/url_lookup_alloc.rs` wraps the production allocator in a counting
`GlobalAlloc` and requires a hard **zero** over 6,000 lookups spanning every
outcome and every option path.

### The heap number, against the prediction

The headroom model predicted **1.03 MiB**; measured is **1.06 MiB**. Close, but
the comparison is not like-for-like and the difference is worth naming rather
than rounding away:

- The model assumed **22,020** URL rules; **18,778** compile. The gap is rules
  carrying options no tier can honour (`$popup`, `$removeparam`, `$important`,
  `/regex/`), which are refused rather than applied without their restriction.
- Per rule that is **59.3 B measured against 49.1 B modelled, +21 %**.

The overshoot is the index, not the layout: records are the modelled 16 B and
the arenas are exactly the retained text. The ~266 KB the model did not carry
is the open-addressing slot table plus an 8-byte hash per bucket (~15,000
buckets) — the cost of *finding* a bucket, which a CSR sketch does not show.

That is a real divergence from the model and it is stated here rather than
buried; it is not a budget problem. Enabling both lists costs **4.05 MiB
all-in**, against ~24 MiB of headroom. The p2-03 headroom finding stands: the
URL tier is not the Phase-2 memory risk, p2-05's per-policy ruleset duplication
is.

### Latency, and the 11× that was left on the table

The first working version measured **31.5 µs** per request. Two fixes, both
kept:

1. **Unanchored patterns tried every start offset** — O(url × pattern) on rules
   that decide nothing. Now only offsets where the pattern's first element can
   match are tried. 31.5 µs → 10.4 µs.
2. **254 rules had no usable token** and were scanned on every request.
   Measured directly (by temporarily disabling the boundedness rule) at **87 %
   of the whole lookup cost** — which is what motivated the prefix/suffix keys
   above. 254 unindexed → 77, and 10.4 µs → 2.93 µs.

Worth recording because the first number was already 30× inside budget. Passing
the budget was never evidence the design was right.

## Decisions worth challenging later

- **Third-party without a Public Suffix List.** Two hosts are same-party when
  they share their last two labels: right for `img.example.com` vs
  `www.example.com`, wrong for `a.co.uk` vs `b.co.uk`. A PSL is a dependency,
  ~200 KB of tables and a refresh story. The error only ever *narrows* a rule,
  so it under-blocks rather than over-blocks. Documented in RULE_ENGINE.md as
  an approximation, not as correct.
- **An unrecognized option drops the rule.** Applying the pattern while
  ignoring the restriction it carries is how `||hltv.org^*=|$popup` becomes a
  block on ordinary navigation. Under-blocking is the safe direction, and the
  refused rules are counted, so widening support later is measurable.
- **`ResourceType::Unknown` matches no type-restricted rule in either
  direction.** `$script` and `~script` both decline it. The proxy will often
  not know the type, and guessing would let a wrong guess block.
- **`$client` decides the tier before the pattern does.** Previously a rule
  with `$client=` *and* an HTTP option was filed under the HTTP option; it is
  now `ClientScoped`. Both are inactive, so nothing observable changes — but
  activating the URL half would have applied a client-scoped rule to every
  client.
- **`HttpRequest`, not `HttpMatchCtx`.** The phase note sketched the latter.
  The type lives in `fah-model` as a request model, and `…MatchCtx` is
  matcher-flavoured naming inside a crate that is not allowed to know about the
  matcher. Flagged rather than silently renamed.

## The counter that was lying

`inactive_count()` was "everything not DNS-active", so the moment URL rules
started filtering, the API would have reported 18,778 actively-filtering rules
as inactive. `rules_active_url` is now a third counter and the three partition
`rules_total`. `RefreshStats` and `looks_misparsed` follow, so a list whose
rules are *all* URL-tier is not mistaken for a misparse.

This is a visible API change on upgrade — `rules_inactive` drops sharply for
adblock-format lists — and API.md says so explicitly.

## Tests

163 unit in `fah-rules` (43 new: parser classification and retention, the
matcher's anchors/wildcards/separator/case handling, every option, index
integrity, dedup, rule reconstruction), plus:

- `tests/url_matching.rs` — 7 end-to-end tests driving verbatim EasyList lines
  through detection → parse → compile → verdict, including `$domain=` judged on
  the document host, cross-tier precedence, and a test that adding URL rules
  moves **no** DNS verdict.
- `tests/url_lookup_alloc.rs` — the hard zero-allocation assertion.
- `tests/parsers.rs` — the p2-00 regression tests now assert the path-qualified
  rules reach the compiled URL tier, not merely that they are not DNS rules.

## Gates

```text
cargo fmt --check                                     clean
cargo clippy --workspace --all-targets -- -D warnings clean
cargo test --workspace                                35 binaries, all pass
cargo bench -p fah-rules --bench url_matcher          numbers above
```

The bench defaults to a synthetic EasyList-shaped corpus so it runs offline and
deterministically; `FAH_URL_CORPUS="a.txt;b.txt"` points it at real lists, which
is how every figure above was produced.

## Not done here

- **Nothing consumes `lookup_http` yet.** Wiring verdicts into the proxy —
  block responses, events, stats — is p2-04. The entry point and the request
  model exist so that task is wiring rather than design.
- **Resource-type inference.** The proxy must derive `ResourceType` from
  `Accept` / `Sec-Fetch-Dest` / the path; until it does, every request is
  `Unknown` and type-restricted rules will not fire. p2-04.
- **77 unindexed rules** are still checked on every request. The remaining
  candidates (a token open on both sides) would need a substring index; not
  worth it at 2.93 µs, but it is the first place to look if a much larger
  corpus ever moves the number.
- **`$important`, `$popup`, `$removeparam`, `$badfilter`, `/regex/`** are
  classified `UnsupportedUrlPattern` and counted. Adding any of them is
  additive work against a counter that already says how much is being left.

## Review pass — defects found and fixed

Six defects, all fixed in place with a failing-first test each. The index
itself held up: a randomized oracle (200 generated rulesets × 40 URLs, random
anchors, wildcards, separators and end-anchors) found **no** case where the
token index and a full scan disagree. That test is now permanent —
`the_index_agrees_with_a_full_scan_on_random_rulesets` — because the existing
oracle ran 16 hand-picked URLs, which proves the shapes we thought of and
nothing else.

### 1. A single request could burn 313 ms of CPU — critical

`match_from` backtracks over `*`, and an unanchored pattern is retried at every
start offset whose first byte could match. The product is **O(url² × pattern)**.
Measured, release build, x86 dev box: one rule shaped `aaa…a*aaa…ab` against a
URL of `a`s cost **313 ms for one lookup** — five orders of magnitude past the
`< 1 ms` p99 budget, and it is the *URL* that drives it. The URL is chosen by
whatever LAN device sent the request; a rule of that shape only has to exist
once in a subscribed list.

Fixed with a work allowance ([`Budget`]) whose unit is **one attempted match
position** — entering `match_from`, or widening a `*`. A rule gets
`2 × url_len + 64` units; the lookup gets a `8_000_000` backstop. Both position
counts are individually bounded by the URL length for any honest pattern (a
leading-`*` rule widens at most once per URL byte; an unanchored one is entered
at most once per URL byte), so the cap leaves a rule its full legitimate cost
and removes only the *product* of the two — which is the square.

Exhaustion reports "no match" — under-blocking, the direction this codebase
already chose everywhere else — and increments a counter surfaced as
`Matcher::url_budget_exhausted()`, because a silently unenforced rule is exactly
what no latency bench would ever show.
`a_long_url_still_matches_an_ordinary_rule` pins that an 8 KB URL still matches
a normal rule with the counter at zero.

The first implementation charged per *byte compared* instead, which is the same
bound but meters the hot loop; it cost 28 % on the real corpus. See "Cost of the
fixes" below — that measurement is the reason the unit is what it is.

### 2. `$dnsrewrite` rules were deciding HTTP requests

`lookup_http` excludes `$dnstype` from the domain walk — "a request asks no DNS
question" — and the same sentence is true of `$dnsrewrite`, which was not
excluded. `||rewrite.example.com^$dnsrewrite=1.2.3.4` **blocked the fetch**.
That option synthesizes a DNS answer; `=1.2.3.4` is a redirect, not a denial,
so reading it as a block refuses a request the rule never said to refuse.
Excluded on the same `qbit.is_none()` signal, with a test asserting the rule is
still decisive for the DNS question it was written for.

### 3. A type-restricted **exception** declining `Unknown` over-blocks

The decision recorded above — "`ResourceType::Unknown` matches no
type-restricted rule in either direction" — is right for a block and backwards
for an exception. Verified: `||cdn.example.com^` plus
`@@||cdn.example.com/assets/$script`, with the type undetermined, returned
**Block**. The exception declines, the domain-tier block does not, and the
resource the list author explicitly permitted is refused.

The stated rationale ("guessing would let a wrong guess block") only ever
covered blocks. Now `Unknown` declines type-restricted **blocks** and applies
type-restricted **allows**, so both directions under-block. This matters now,
not later: until p2-04 infers resource types, *every* request is `Unknown`, and
`@@` exceptions in EasyList carry type options constantly.

### 4. `$domain=example.*` compiled, counted, and never fired

`host_under` had no wildcard arm, so uBO's "under any public suffix" form
matched no host at all. The rule compiled, counted in `rules_active_url`, and
silently did nothing. Implemented with the tail bounded to at most two labels —
the same approximation `registrable` already makes — so `example.*` matches
`example.com` and `www.example.co.uk` but not `example.com.evil.net`.

### 5. `$domain=` was validated as UTF-8 on the hot path, and failed *open*

`domains_of` returned `&str` via `from_utf8(...).unwrap_or("")`. Two problems in
one line: it ran a UTF-8 validation over the whole `$domain=` payload for every
candidate rule on the lookup path, and its failure value — `""` — makes
`domains_apply` return `true`, i.e. **the rule applies everywhere**. A
fail-open default on a restriction is the wrong direction even if the arena
makes it currently unreachable. Now compared as bytes throughout; every
comparison there was ASCII-case-insensitive anyway.

### 6. Over-long payloads were dropped by the compiler without a word

`UrlIndexBuilder::add` returns early when a pattern or `$domain=` payload
exceeds the 16-bit length in `Record` — after the parser has already counted the
rule as URL-tier. Confirmed: a rule with a 70 KB pattern reports `url_count=1`
and compiles to **zero** rules. `rules_active_url` then reports a rule as
filtering while it filters nothing — the same class of lie as "The counter that
was lying" above. Refused at parse time now, where it lands in
`UnsupportedUrlPattern` and is counted.

### Cost of the fixes — measured on the real corpus

The synthetic corpus is not a safe proxy for this change: it compiles **0**
unindexed rules, and the unindexed scan is where the allowance is charged
hardest. Re-measured on real EasyList + EasyPrivacy (85,140 + 56,426 lines,
18,778 URL rules, 77 unindexed — the review's own figures reproduce exactly),
core-pinned per PERFORMANCE.md. Criterion reports each run against the previous
one, so only absolutes are comparable across variants:

| `single_pass_request` | Absolute | vs pre-fix |
| --- | ---: | ---: |
| pre-fix | 3.136 µs | — |
| allowance charged **per byte compared** | 4.014 µs | **+28 %** |
| allowance charged **per match position** | **3.091 µs** | **−1.4 %** |

`mixed_requests`: 15.019 µs → 17.188 µs → **15.100 µs** (+0.5 % vs pre-fix).

The first attempt metered the innermost byte loop, which is the hot one, and
cost 28 % on real lists while the synthetic corpus showed +4 % — the corpus
choice, not the design, was hiding it. Charging instead per *attempted match
position* — one unit on entering `match_from`, one per widening of a `*` —
gives the identical asymptotic bound (both are capped by the URL length for any
honest pattern; their **product** is the square) while leaving the byte loop
untouched. The regression is gone: both figures are inside this box's noise
band.

A differential harness over 4,165 requests produced an **identical verdict
fingerprint** before and after, so none of this moved a verdict that was already
correct.

### Where the time actually goes

Since the corpus choice had already misled one measurement, the lookup was
decomposed rather than reasoned about — real corpus, pinned, per lookup:

| Stage | Cost | Share |
| --- | ---: | ---: |
| Tokenizing the URL | 34–41 ns | ~1 % |
| Index probes (3 per token, 15,037 buckets) | 110–200 ns | 4–7 % |
| **Unindexed scan (77 rules)** | **1.87–2.23 µs** | **67–83 %** |
| Bucket candidate checks | 0.31–1.23 µs | 12–37 % |
| **Total** | **2.66–3.30 µs** | |

**Tokenization and bucket lookup are not where the cost is** — together they
are under 8 % of a lookup, and the open-addressing table is doing its job. Two
thirds to four fifths of every lookup is still the 77 rules no token could file.

This qualifies the section above: reducing 254 unindexed rules to 77 did not
solve that problem, it shrank it. The unindexed set remains the dominant term at
short URLs and the *only* term that matters at long ones (556 µs at an 8 KB
URL). The substring index named under "Not done here" is therefore not a
speculative future optimisation — it is the one change that would move this
number, and the measurement above says how much: ~75 % of the lookup.

### Raised, not fixed

- **Lookup cost scales with URL length × unindexed rules.** 77 unindexed rules
  against an 8 KB URL measured **556 µs** on x86 — over half the p99 budget
  before p2-04 has wired anything up, and the RB5009's ARM core is several
  times slower. The 3.09 µs headline is a short-URL figure. The allowance bounds
  the pathological case but not this one; removing it needs the substring index
  already named under "Not done here", and multi-KB URLs are ordinary traffic
  (OAuth redirects, ad-tech beacons). The decomposition above quantifies the
  prize at ~75 % of a lookup. **This is the first thing to measure on device,
  and the first place to spend effort if it matters.**
- **`HttpRequest::host` and `document_host` must not carry a port.** The
  contract says so and nothing enforces it. Confirmed: a `Referer`-derived
  `document_host` of `news.org:8080` makes `$domain=news.org` silently stop
  applying, and a port on `host` breaks the third-party test and the whole
  domain-tier walk. p2-04 builds these from headers — strip there.
- **Cosmetic detection is `line.contains("##")`,** so a URL rule with `##`
  anywhere in it is swallowed as cosmetic. uBO reads it the same way, so this
  is compatible rather than wrong, but it is not the *stated* rule.
- **Third-party without a PSL** remains as documented; the review's own
  framing of it as under-blocking is confirmed by test.

Gates after the fixes: `cargo fmt --check` clean, `cargo clippy --workspace
--all-targets -- -D warnings` clean, `cargo test --workspace` 664 passing.
