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

All four formats are **fully parsed from day one**
(see [ADR-0003](docs/decisions/0003-full-format-parsing-day-one.md)).
Every rule is classified:

- **DNS-applicable** — acts on a domain name; active in Phase 1:
  - `||domain^` block rules and `@@||domain^` exceptions
  - hosts entries and plain domain lines
  - AdGuard DNS extensions: `$dnstype`, `$dnsrewrite`
  - `$client` — parsed and stored, **inactive** until Phase 2 (Policies)
- **non-DNS** — cosmetic (`##`), URL-path patterns, HTTP `$options`
  (`$script`, `$third-party`, …); parsed, counted, stored **inactive** until
  the HTTP/HTML phases. Counts are visible per list in the API.

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

Matching is on the query domain and its parent labels
(`a.b.example.com` matches a rule for `example.com` when the rule's syntax
implies subdomains, as `||example.com^` and hosts semantics do).

## Compiled matcher

- Rule text is parsed **once at load time** into compact match structures —
  domain hash tables / label tries. Exact structure is an implementation
  decision, bound by the budgets in [PERFORMANCE.md](PERFORMANCE.md)
  (1M domains ≤ 40MB, verdict < 1ms p99).
- **No regex compilation at runtime, no regex on the hot path.**
- Lookup cost is O(number of labels) hash probes, allocation-free.

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

- Refresh: per-list interval, default 24h.
- Boot: compile from the `/data` cached copies immediately (no network on the
  startup path); refresh happens asynchronously afterwards.
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
