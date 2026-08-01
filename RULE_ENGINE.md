# RULE ENGINE

The single component that loads, parses, manages and processes rule lists,
compiles them into matchers, and answers verdicts. (There is no separate
"Filter Engine" — see [CONTEXT.md](CONTEXT.md).)

## Supported formats

Format is auto-detected per list.

| Format | Example | Detection hint |
|--------|---------|----------------|
| hosts | `0.0.0.0 doubleclick.net` | IP-prefixed lines |
| plain domain list | `doubleclick.net` | bare domains |
| EasyList / uBlock Origin | `||ads.example.com^$third-party`, `##.ad-box` | `||`, `##`, `$options` |
| AdGuard | same family + `$dnstype`, `$dnsrewrite`, `$client` | AdGuard-specific options |

### Detection is a vote over a sample, not a look at line one

Detection classifies the first 200 content lines (comments and blanks skipped)
and takes the format most of them look like; scanning stops once the sample is
full, so it never costs a pass over a multi-MB list.

A single line is not enough evidence. Real EasyList opens with
`&rb=&uuid=$third-party` and EasyPrivacy with `&&sub19=undefined&sub20=undefined`
— URL-substring patterns carrying none of the `||`/`@@`/`##` markers a
first-line check looks for. Both would be read as plain domain lists, which
hands the entire list to the bare-domain parser.

The adblock markers are ones a hosts entry or a bare domain can never contain —
`^`, `/`, a `$option` suffix, a leading/trailing `|` address anchor — so a
domain list is never mistaken for an adblock list. `*` is deliberately **not** a
marker, because plain domain lists in the wild carry `*.example.com` entries.

**A tie resolves to plain domain list.** Reading a domain list as adblock is the
worse error: every bare-domain line becomes an inactive URL pattern, so the list
silently contributes nothing and reports no parse errors at all.

When a parse yields more failures than rules (past a floor of 100 errors), the
list is reported as **`degraded`** rather than `ok` — see
[API.md](API.md) `GET /api/v1/lists` — and logged once at `warn`. That is the
signature of a misdetected format, and it is one fact about one list, not
thousands of line errors.

All four formats are **fully parsed from day one**
(see [ADR-0003](docs/decisions/0003-full-format-parsing-day-one.md)).
Every rule is classified:

- **DNS-applicable** — acts on a domain name; active in Phase 1:
  - `||domain^` block rules and `@@||domain^` exceptions
  - hosts entries and plain domain lines
  - AdGuard DNS extensions: `$dnstype`, `$dnsrewrite`
  - `$client` — active since Phase 2 (`p2-05`); see §Policies
- **request-applicable** — acts on a URL; active since Phase 2 (`p2-03`):
  URL-path patterns, wildcards, address anchors, and the HTTP `$options`
  (`$script`, `$third-party`, `$domain=`, …). See §HTTP matching.
- **inactive** — cosmetic (`##`, Phase 4), and patterns or options no supported
  syntax expresses. Parsed and counted, but no tier answers them. Counts are
  visible per list in the API.

A rule belongs to exactly one of the three. The split is decided by what the
rule *addresses*, not by its syntax family: `||ads.example.com^` is a domain
rule, `||ads.example.com^$script` is a request rule (the option can only be
judged once a request exists), and `||ads.example.com^*/pixel.gif` is a request
rule because it names a path.

Unparseable lines are skipped and counted (`parse_errors` per list) — one bad
line never rejects a list.

## Verdicts

For a query the engine returns exactly one verdict:

| Verdict | Meaning |
|---------|---------|
| `allow` | an exception rule (`@@`) matched — forward, never block |
| `block` | a block rule matched — synthesize blocked response |
| `pass`  | nothing matched — forward normally |

**Precedence: allow > block.** Within a class, first match wins; rule order
inside a list and list order are otherwise not significant.

A rule the asking client's Policy cannot see, or that a `$client` option scopes
to somebody else, takes no part in this at all — it is filtered *during* the
walk, not after. The difference matters for exceptions: an `@@` rule in a list
the policy does not enable must not suppress a block the policy still carries.

Matching is on the query domain and its parent labels
(`a.b.example.com` matches a rule for `example.com` when the rule's syntax
implies subdomains, as `||example.com^` and hosts semantics do).

## HTTP matching

An HTTP request is a different question from a DNS query, so it gets its own
typed entry point over the **same** compiled ruleset — not a second matcher,
and not a trait object (PERFORMANCE.md forbids virtual calls on the hot path):

```text
DNS question  (domain, qtype)  ──► lookup_dns
HTTP request  (url, host, method, resource type, referer) ──► lookup_http
```

The request model is `fah-model`'s `HttpRequest` — pure data, every field
already extracted by the proxy. Deriving the resource type from the wire and
the document host from `Referer` is the HTTP pipeline's job, not the engine's.

### Both tiers answer a request

`lookup_http` consults the URL tier **and** the domain tier, with one shared
precedence order: **any allow beats any block**, whichever tier it came from.

- A URL rule can decide it (`||ads.example.com^*/pixel.gif$third-party`).
- A domain rule can decide it too: `||ads.example.com^` blocks that *name*, and
  a request addressed to that name is exactly what it blocks. Subdomain
  semantics carry over unchanged.
- An `@@||cdn.example.com^` exception therefore overrides a URL-tier block, not
  only a domain-tier one.

**`$dnstype`-restricted rules never decide a request.** A request asks no DNS
question, so `$dnstype=A` has nothing to match against; applying it anyway
would let a record-type filter block a fetch.

### Supported pattern syntax

| Syntax | Meaning |
| ------ | ------- |
| `\|\|domain/path` | anchored at the host or any label boundary inside it |
| `\|http://…` | anchored at the start of the URL |
| `/ads/banner` | matches anywhere in the URL |
| trailing `\|` | the pattern must reach the end of the URL |
| `*` | any run of bytes |
| `^` | one separator character, **or** the end of the URL |

Separator is the adblock set: anything that is not a letter, digit, `_`, `-`,
`.` or `%`. Matching is case-insensitive unless the rule carries `$match-case`.

**`/regex/` literals are not supported** and are classified inactive rather
than silently never matching — a regex engine on the per-request path is
exactly what PERFORMANCE.md rules out.

### Supported options

`$script`, `$image`, `$stylesheet`/`$css`, `$xmlhttprequest`/`$xhr`,
`$document`, `$subdocument`/`$frame`, `$font`, `$media`, `$websocket`,
`$ping`/`$beacon`, `$object`, `$other` — and each negated (`~script`).
Negation folds at compile time into a single mask, the same way `$dnstype=~A`
does; a set that folds to nothing (`$script,~script`) is refused rather than
compiled into a rule that would match everything.

`$third-party` / `$first-party` (and their `~` forms), `$domain=` (evaluated
against the **document** host, `|`-separated, `~` entries veto), `$method=`,
`$match-case`.

**An option the engine does not recognize drops the rule.** Honouring a
pattern while ignoring the restriction it carries is how a narrow rule becomes
a broad one — `||hltv.org^*=|$popup` must not block ordinary navigation.

### Third-party is approximated without a Public Suffix List

Two hosts are same-party when they share their last two labels. This gets
`img.example.com` vs `www.example.com` right and `a.co.uk` vs `b.co.uk` wrong.
A PSL would cost a dependency, ~200 KB of tables and a refresh story; the error
direction here only ever *narrows* a rule, so it under-blocks rather than
over-blocks. Revisit if measurements show `$third-party` accuracy matters.

### Cost

Rules are indexed by one literal token each, so a request checks only the rules
whose token the URL actually contains, never the whole corpus. Lookup is
allocation-free, like the DNS one. Measured against EasyList + EasyPrivacy
(18,778 URL rules) on a dev box: **2.93 µs** for a request nothing matches,
against the < 1 ms budget.

## Policies

A **Policy** (CONTEXT.md) is a named subset of the rule lists plus setting
overrides, assigned to clients and optionally to schedules. It changes *which
rules participate* in a lookup, never how they match.

```text
lookup_dns  (domain, qtype, client context)
lookup_http (request,       client context)
```

The client context carries the resolved policy and the client's identity
(address, optional name). Both entry points have a context-free form that means
"the default policy, no identifiable client" — which is what every caller did
before Policies existed and what `rules/test` still does.

### One ruleset, one mask per rule

All policies share **one** compiled ruleset. Each rule carries a 16-bit mask of
the policies that can see it, and a lookup tests one bit.

The alternative — a compiled matcher per policy — costs the whole corpus again
per policy. Measured on two policies over four overlapping lists: 12.099 MiB
against 6.839 MiB for their union. At deployed scale that is roughly +17 MiB
per policy against ~24 MiB of headroom, so the *second* policy would overrun
PERFORMANCE.md's budget on its own. The mask array is ~2 MiB at 1.06 M rules
and **flat** in the number of policies.

The mask is per *rule* rather than per *list* because deduplication collapses a
rule appearing in several lists into one record attributed to the first. Keying
visibility off that attribution would hide the rule from every policy that
enabled only one of the other lists, so the builder unions the masks of every
list a duplicate arrives from.

Nothing is allocated when no policy narrows anything. The default
single-policy deployment therefore carries no masks and pays no runtime cost.

### `$client` — an inline per-client policy

`||ads.example.com^$client=192.168.1.50|~laptop` scopes a rule to particular
clients: addresses, CIDR blocks and client names, `~` negating a term. Positive
terms must match and negated ones must not. A payload that compiles to no
selector at all makes the rule **inactive** rather than unrestricted — dropping
a restriction is how a one-device rule becomes a network-wide one.

`$client` says *who*, not *what*, so it is orthogonal to the tier: a
domain-shaped rule stays in the domain tier and a URL-shaped one in the URL
tier. Payloads live in a side map keyed by record index, like `$dnstype` — the
public lists carry essentially none of these, so the cost belongs on the rules
that use it rather than on every record. Two rules differing only in `$client`
are two rules, and the reported decisive rule echoes the scope back.

## Compiled matcher

- Rule text is parsed **once at load time** into compact match structures —
  domain hash tables / label tries. Exact structure is an implementation
  decision, bound by the budgets in [PERFORMANCE.md](PERFORMANCE.md)
  (1M domains ≤ 40MB, verdict < 1ms p99).
- **No regex compilation at runtime, no regex on the hot path.**
- Lookup cost is O(number of labels) hash probes, allocation-free.

## Deduplication

All enabled lists compile into **one** matcher, and that matcher holds
**distinct rules only**. Popular lists overlap heavily — AdGuard's
`filter_48` and HaGeZi's `pro` are close to the same corpus — and storing an
overlap twice costs domain bytes, a record and hash slots against the 40 MB
budget for nothing.

- **Identity is the whole rule**: domain, action (block/allow), whether it
  covers subdomains, `$dnstype`, `$dnsrewrite`. Two rules collapse only when
  all of those match; the domain compares case-insensitively, because lookup
  does.
- **Verdicts are unaffected.** A block in list A and an allow in list B for
  the same domain are *different* identities, so both survive and allow >
  block still decides. A `$dnstype`-scoped rule and a plain one for the same
  domain likewise both survive.
- **First contributor wins attribution.** The surviving rule is credited to
  the first list that supplied it (compile order: enabled lists in
  configuration order, then user rules). Attribution is informational — the
  query log and `rules/test` report it — and is never an input to a verdict.
  There is no "matched in N lists" reporting.
- **Per-list counts stay parse-based.** `GET /api/v1/lists`' `rules_total` /
  `rules_active_dns` / `rules_active_url` / `rules_inactive` describe what each
  list contains; the envelope's `compiled_rules` and `duplicates_removed`
  describe the merge. The `rules_active_url` column arrived with `p2-03` and
  was **split out of `rules_inactive`**, which until then counted 18,778 of
  EasyList + EasyPrivacy's rules as doing nothing while they were filtering.
- **The URL tier deduplicates separately.** Identity there is the pattern, the
  action, the anchors, the resource-type mask and the option payloads. Its
  duplicate count is *not* folded into `duplicates_removed`, because the
  arithmetic identity below is stated over the domain tier and folding two
  tiers into one figure would break it.
  Compiling logs the duplicate count at `DEBUG` — every list refresh recompiles
  the whole combined ruleset, so at `INFO` a boot refresh would repeat the same
  line once per list. The per-list outcome is logged at `INFO`, by name, when
  that list refreshes.
- **`compiled_rules` + `duplicates_removed` does not equal the sum of the
  per-list `rules_active_dns`** — user rules are in the merge but not in the
  list array, so the identity is
  `sum(rules_active_dns) + user_rules_active − compiled_rules = duplicates_removed`.
- Dedup happens at build time only. The lookup hot path is untouched by it
  (marginally faster, with fewer slots to probe).

## List lifecycle

```text
source (URL | /data file | user rules via API)
   │ download / read
   ▼
parse + validate  ──errors──► keep previous compiled set, log, expose in API
   │ ok
   ▼
compile new ruleset (off the hot path)
   │
   ▼
ATOMIC SWAP  ── queries in flight finish on the old set; new queries see the new set
   │
   ▼
cache raw copy in /data
```

- Refresh: per-list interval, default 24h, measured from the list's last
  successful fetch — **including across restarts**. The cached copy's mtime is
  what carries that time, since the in-memory clock is monotonic and dies with
  the process. A restart therefore refreshes only the lists actually due, not
  all of them.
- Boot: compile from the `/data` cached copies immediately (no network on the
  startup path); refresh happens asynchronously afterwards. A list with no
  cached copy — first-ever boot, or one deleted by hand — is due at once, so a
  hand-edited `/data` heals itself on the next tick.
- Boot also deletes cached copies no configured list claims, naming each in the
  log. Editing `[[rules.lists]]` while stopped would otherwise strand them
  permanently, since compiling reads the configured lists rather than the
  directory. Disabled lists keep their copy (re-enabling must not need a
  download), and so do user rules, which are authored rather than downloaded.
- Failure policy: a failed download or a list that fails validation **never**
  degrades protection — the previous compiled set keeps serving.
- The hot path never takes a lock; readers follow the current ruleset pointer.

## Sources

1. **Remote lists** — user-configured URLs (`[[rules.lists]]`, API-manageable).
2. **Local lists** — files on the mounted `/data` volume (external SSD on the
   RB5009); refreshed by re-reading the file.
3. **User rules** — inline personal rules via `PUT /api/v1/rules/user`,
   validated line-by-line, atomically swapped like any list.

A small curated default list (OISD basic) ships enabled — the product blocks
ads out of the box.

## Debugging

`POST /api/v1/rules/test` dry-runs a verdict for a domain/qtype/client and
reports which rule in which list decided it. The query log records the decisive
rule and list for every blocked/allowed query.
