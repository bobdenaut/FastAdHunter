# p2-03 — URL matcher headroom measurement, and two blockers found on the way

**Date:** 2026-07-25 · **Scope:** measurement only, no code changed ·
**Trigger:** `p2-03-url-rules-activation.md` — *"Memory headroom is the real
constraint — measure it first, before building the matcher out."*

Nothing in `crates/` was modified. A throwaway `examples/url_headroom.rs`
harness was used and deleted; the working tree is clean.

---

## 1. Answer to the headroom question

**The URL matcher is ~1 MiB. It is not the Phase-2 memory risk.**

`p2-03` names ~24 MiB of headroom (128 MB budget − ~104 MiB steady RSS) and
warns that *"full EasyList compiles is therefore not free."* Measured, it is
close to free.

| Corpus | URL-tier rules | Modelled compiled heap |
| ------ | -------------: | ---------------------: |
| EasyList | 9,181 | **0.43 MiB** |
| EasyPrivacy | 12,839 | **0.60 MiB** |
| Both, deduplicated together | 22,020 | **1.03 MiB** |

Adding the same two lists also grows the *existing* DNS tier, which is the
larger of the two costs and still small — measured with the real
`Matcher`, not modelled:

| Corpus | DNS-active rules | `Matcher::heap_bytes()` |
| ------ | ---------------: | ----------------------: |
| EasyList | 49,754 | 1.52 MiB |
| EasyPrivacy | 42,984 | 1.42 MiB |

**Worst case for enabling both lists: ≈ 4.0 MiB** (2.94 DNS + 1.03 URL), before
cross-list dedup against the 683,954 rules already loaded — which will only
reduce it. That leaves **~20 MiB** of the stated headroom.

### What this means for the task

The gating risk in `p2-03` is refuted by measurement. The Phase-2 memory
question should move to where the bytes actually are:

- **`p2-02`** — HTTP connection pools and per-connection buffers. A bounded
  pool of N connections × read+write buffers dwarfs 1 MiB at any realistic N.
- **`p2-05`** — per-policy rulesets. If each policy compiles its own matcher,
  cost scales with policy count × ruleset size, and the ruleset is 21.9 MiB.
  *One* extra full copy exceeds the headroom on its own. This is the row that
  deserves `p2-03`'s "measure it first" treatment.

`p2-03`'s acceptance criterion ("compiled URL-matcher heap measured and
recorded as an absolute number") is satisfied in advance by the table above;
it should be re-measured against the real implementation, but as a
confirmation, not as a gate.

---

## 2. Finding U1 — real EasyList is not detected as EasyList

**Severity: blocker for Phase 2. No live impact today.**
**Location:** [format.rs:20-52](../../crates/fah-rules/src/format.rs#L20-L52)

`detect_format` classifies a list from its **first content line only**, and
matches Adblock on `||`, `@@` or a cosmetic marker. Real EasyList's first
content line is:

```text
&rb=&uuid=$third-party
```

A URL-substring pattern — no `||`, no `@@`, no `##`. It falls through to
`PlainDomainList`, and the whole list is then handed to the plain-domain
parser. EasyPrivacy's first content line is `&&sub19=undefined&sub20=undefined`
and fails identically.

Measured, feeding the shipped parser the real `easylist.txt` (83,516 lines):

```text
format          PlainDomainList
parsed rules    83
parse errors    69,514
active (DNS)    83
UrlPattern      0
Cosmetic        0
```

The 83 "rules" are ad-banner-size tokens — `_300x250`, `_160x600`, `_468x60` —
accepted by `normalize_domain` and compiled as DNS block rules for domains that
do not exist. Harmless in effect, but they are the *entire* result of loading
EasyList.

Forced down the correct parser, the same file yields:

```text
parsed rules    83,239      parse errors 0
active (DNS)    49,767
Cosmetic        24,368
UrlPattern       3,689
HttpOption       5,414
```

**Why this matters beyond a bad load:** ADR-0003 and the Phase-2 CLAUDE.md
architecture note both rest on *"Phase 1 already parses, counts and stores
non-DNS rules inactive; `p2-03` activates a subset that is already in the
index."* For the actual EasyList that premise is false — detection routes the
list to the wrong parser before classification ever runs, so there is nothing
in the index to activate. `p2-03` cannot start on this.

**Direction:** detect from a sample of content lines rather than the first one
(score N lines, take the majority), and treat a high parse-error ratio as a
detection failure rather than a per-line error. Both belong in the same change.

---

## 3. Finding U2 — `||domain^<suffix>` silently becomes a whole-domain DNS rule

**Severity: high. Latent today; catastrophic the moment U1 is fixed.**
**Location:** [adblock.rs:56-59](../../crates/fah-rules/src/parser/adblock.rs#L56-L59)

```rust
let options_str = match terminator {
    Some('^') => after_anchor[end + 1..].strip_prefix('$').unwrap_or(""),
    ...
```

When the text after `^` does not begin with `$`, `unwrap_or("")` **discards it**
and the rule is treated as a bare `||domain^` — an active, subdomain-inclusive
DNS rule. Every path, wildcard and anchor qualifier is thrown away.

So `||googleapis.com^*/gen_204?` — a rule about one tracking path — compiles
into **block googleapis.com and all subdomains**.

Two distinct suffix forms hit this, with very different consequences:

### 3a. `^|` end-anchor form — mild

`||clarity.ms^|`, `@@||link.nzz.ch^|`. The `|` anchors end-of-address. For DNS
the collapse to a domain rule is *nearly* correct; the only error is that
`include_subdomains: true` widens what `|` had anchored. Low harm.

### 3b. `^<path or wildcard>` form — severe

`||paypal.com^*/pixel.gif$third-party` → **blocks all of paypal.com.**

Measured against the two lists Phase 2 exists to support:

| List | Misparsed | `^\|` form | `^path` form | of which **over-blocks** |
| ---- | --------: | ---------: | -----------: | -----------------------: |
| EasyList | 80 | 0 | 80 | 56 |
| EasyPrivacy | 309 | 0 | 309 | 235 |

A sample of the 291 domains that would be blocked whole:

```text
googleapis.com      amazonaws.com     s3.amazonaws.com   cloudfront.net
akamai.net          akamaihd.net      azureedge.net      azurewebsites.net
jsdelivr.net        connect.facebook.net                 paypal.com
ebay.com            twitter.com       x.com              bing.com
baidu.com           citi.com          deliveroo.com      qualtrics.com
```

Enabling EasyPrivacy on a machine with U2 present takes out PayPal, eBay, X,
Bing, Baidu, Citi and four major CDNs at the DNS layer.

### The interaction is the important part

**U1 currently masks U2.** EasyList-family lists never reach the adblock
parser, so their `^*/path` rules never compile. Fixing U1 alone — the obvious,
one-line-looking fix — removes the mask and breaks the user's internet on the
next list refresh.

> **Fix U2 before U1, and ship them together. Never U1 alone.**

**Direction — and the two suffix forms must be treated differently.** A first
draft of this section said "classify any non-`$` suffix as `UrlPattern`". That
is wrong for 3a and would cause new blocking on the live box: the 171
`@@||domain^|` exceptions are currently active allow rules, and reclassifying
them as inactive URL patterns *removes* those exceptions, letting other lists'
block rules take effect on domains that are deliberately excepted today.

The correct split:

- **`^|`** — a legitimate DNS rule form (`|` anchors end-of-address; AdGuard
  treats `||d^|` as `||d^` for DNS). Keep it **active**, but with
  `include_subdomains: false`, which is what `|` actually anchors. Preserves
  all 171 exceptions and narrows the one over-block.
- **`^<path/wildcard>`** — genuinely not domain-only. Classify
  `InactiveReason::UrlPattern`; that is exactly where `p2-03` needs it.

Note that **no parser emits `include_subdomains: false` today** —
[domain_list.rs:27](../../crates/fah-rules/src/parser/domain_list.rs#L27),
[hosts.rs:73](../../crates/fah-rules/src/parser/hosts.rs#L73) and
[adblock.rs:78](../../crates/fah-rules/src/parser/adblock.rs#L78) all hard-code
`true`. The flag is honored at lookup
([matcher.rs:586](../../crates/fah-rules/src/matcher.rs#L586)) and covered by a
unit test, but this fix would be its first production use. Worth a fixture test
proving `||d^|` matches `d` and not `sub.d`.

---

## 4. Live-deployment impact: essentially none

All 15 lists configured on the RB5009 were downloaded from source and audited.

| List | `\|\|dom^` rules | misparsed |
| ---- | ---------------: | --------: |
| oisd (`big.oisd.nl`) | 333,852 | 0 |
| **adguard_sdns_filter** (`filter_1`) | 159,845 | **172** |
| filter_48 | 222,190 | 0 |
| filter_18, filter_2, hosts, spy | — | 0 (not adblock syntax) |
| all others | — | 0 |

The 172 are **all** the mild `^|` form: **171 over-allows** (an exception
widened to subdomains — slightly weaker filtering) and **1 over-block**,
`||clarity.ms^|`, a Microsoft telemetry domain that a blocklist user wants
blocked anyway.

**No corrective action is needed on the running 0.2.4, and the 24 h soak is
unaffected.** These findings gate `p2-03`, they are not a production incident.

---

## 5. Method, and how far to trust the 1 MiB

**Classification** came from the shipped parser (`fah_rules::parse_rule_list`)
via a temporary example, not a reimplementation. Lists that mis-detect were
forced down the adblock path by prepending one synthetic `||fah-probe.invalid^`
line, whose single rule is subtracted from the reported counts.

**DNS-tier heap** is a real `Matcher::heap_bytes()` reading, not a model.

**URL-tier heap** is modelled — the matcher does not exist yet — in the same
shape as the existing one (contiguous pattern arena + fixed 16 B records +
side arena for `$domain=` payloads + a CSR token index, one best-token entry
per rule, adblock-rust style). A `HashMap<u32, Vec<u32>>` index instead of CSR
gives 1.36 MiB; both are reported so the number has a range.

**Calibration.** The same modelling method was run against the *existing*
domain matcher on the same corpus (`small.oisd.nl`, 56,280 rules) and compared
to its real `heap_bytes()`:

```text
modelled   2,000,663 B
real       1,797,998 B
error         +11.3%   (over-estimate)
```

The method is conservative, so 1.03 MiB is an upper bound, not an optimistic
one. At 3× modelling error the conclusion is unchanged.

**Known divergence:** the auxiliary Python classifier used for byte volumes
treats `$important` as DNS-safe; the Rust parser does not. This shifts a
handful of rules between the URL and DNS tiers (EasyList: 3,762 vs 3,689
`UrlPattern`) and does not move a megabyte-scale number. The misparse audit in
§3 and §4 is independent of option handling.

---

## 6. Recommended changes to the plan

1. **New task before `p2-03`** — fix U2 then U1, in one change, with fixture
   tests covering `||d^|`, `||d^*/p`, `@@||d^|` and a real EasyList head. It is
   `fah-rules` + parser work, Opus-sized, and `p2-03` is blocked on it.
   Acceptance should include a **before/after diff of the compiled ruleset
   against the reference ruleset used for deployment** — the expected delta is
   172 rules changing shape and 0 domains losing an exception. Any other
   movement is a regression in a filter set that is live in a household.
2. **`p2-03`** — replace the headroom warning with the measured ~1 MiB, and
   demote its acceptance criterion from a gate to a confirmation.
3. **`p2-05`** — inherit the "measure memory first" framing. Per-policy ruleset
   duplication is the real ≥20 MiB risk; `p2-03` never was.
4. **Phase-2 CLAUDE.md architecture note** — the claim that `p2-03` "activates
   a subset that is already in the index" holds only after U1/U2 are fixed.
   Worth one sentence, since the note is otherwise accurate.

## 7. Reproduction

```sh
curl -sSL -o easylist.txt    https://easylist.to/easylist/easylist.txt
curl -sSL -o easyprivacy.txt https://easylist.to/easylist/easyprivacy.txt
# classify with the shipped parser; force adblock by prepending "||fah-probe.invalid^"
cargo run -p fah-rules --release --example url_headroom -- easylist.txt
```

The misparse audit is a single predicate over each `||`-anchored line: find the
first of `^ $ /`; if it is `^` and the remainder is non-empty and does not start
with `$`, the rule is misparsed.
