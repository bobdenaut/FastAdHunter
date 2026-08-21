# P2-04 — HTTP Filtering Pipeline

**Phase:** 2 · **Depends on:** p2-02, p2-03 · **Model:** Opus

## Goal

The proxy consults the Rule Engine per request; blocked requests die cheaply;
everything is observable.

## Context

Pipeline mirrors DNS: verdict first, then forward. Blocked HTTP responses are
a UX decision: scripts/images want empty-success (collapse quietly), documents
want a visible refusal. Events flow to fah-stats/fah-metrics via the existing
channel pattern (wired in the binary; siblings stay strangers).

## Scope

- Per-request verdict call on the parsed head (fast path stays streaming —
  verdict happens before any body transfer).
- Block responses by resource type: scripts/styles/images/XHR ⇒ 200 with
  empty body of matching Content-Type (or 204); document requests ⇒ minimal
  403 page naming FastAdHunter + the decisive rule; connection kept alive.
- `RequestEvent` (fah-model, CONTEXT.md updated): timestamp, client, method,
  host+path, resource type, verdict, rule/list, bytes, duration → channel to
  fah-stats (query log grows a `kind: dns|http` discriminator; API.md
  `/api/v1/queries` filter extended — doc updated same change) and
  fah-metrics (HTTP counters/histograms).
- Tests: blocked script gets 200-empty, blocked page gets 403, allowed
  request streams untouched; events observed end-to-end.

## Decide FIRST: what the event channel carries (p2-01 audit)

"Events flow via the existing channel pattern" above is under-specified, and
the ambiguity is the expensive part of this task. **`fah_model::QueryEvent` is
DNS-shaped** — it carries `query: Query` (domain + `QueryType` + client_ip),
`cache_hit`, `upstream_used` and `stale` (RFC 8767). None of those mean
anything for an HTTP request, which needs URL, method, status and bytes.

The pipeline today has exactly one bounded mpsc, one `try_send` per query, one
`dropped_events` counter, and one fan-out task feeding Stats + Metrics +
EventHub. `RequestEvent` cannot ride that channel as-is. Two exits, and this
task must pick one **before** writing the block-response code:

1. **Widen the channel item** to `enum Event { Dns(QueryEvent), Http(RequestEvent) }`
   — keeps one channel, one shed counter, one fan-out. Cost: changes
   `fah-model` (L1) plus every consumer — `fah-stats`, `fah-metrics`,
   `fah-api`'s `EventHub`, and `spawn_event_fanout`. Note `Metrics::record`
   currently takes `&QueryEvent` by reference and buckets on `cache_hit`/
   `stale`; those branches need an HTTP counterpart or an early return.
2. **A second channel** for HTTP — smaller blast radius now. Cost: forfeits the
   single-shed-figure property the observability design is built on; two
   independent drop counters and no one number for "we shed N".

Recommendation: (1). The property in (2) that gets lost is the one the p1.5
metrics work existed to establish. Record the choice here and in
`docs/code-review/phase2/p2-04-review.md`.

## Inherited from the p2-03 review — build `HttpRequest` correctly

Two requirements land on this task because it is the first code to construct a
`fah_model::HttpRequest`. Both were found by review, not by a failing test, and
both fail *silently* — see `docs/code-review/phase2/p2-03-review.md` §"Raised, not
fixed".

1. **Strip the port from `host` and `document_host`.** The field docs say
   "without port" and nothing enforces it. A `Referer` of
   `http://news.org:8080/` yields `document_host = "news.org:8080"`, and then
   `$domain=news.org` **stops applying** — confirmed by probe. A port on `host`
   is worse: `registrable()` reduces it to `com:8080`, inverting the
   third-party test, and the domain-tier walk matches no rule at all. The URL
   itself must keep its port (`||example.com^` relies on `:` being a
   separator) — it is only these two fields that must be bare. Test both.

2. **Resource-type inference is load-bearing, not a nice-to-have.** p2-03
   shipped every request as `ResourceType::Unknown`, and the review then changed
   what `Unknown` means: a type-restricted **block** still declines it (guessing
   wrong would over-block), but a type-restricted **exception** now applies
   (declining it left the block it exists to override standing). Until this task
   infers the type, that asymmetry runs one-sided — `@@…$script` fires while
   `…$script` does not. Derive from `Sec-Fetch-Dest` first (explicit and
   trustworthy), then `Accept`, then the path extension; leave `Unknown` when
   none of them speak, and assert a request with no hints stays `Unknown` rather
   than being guessed.

## Acceptance criteria

- Blocked request: zero bytes fetched upstream (assert no upstream
  connection).
- `host` / `document_host` carry no port, asserted for a `Host: h:8080` request
  and a `Referer` with an explicit port; a `$domain=` rule and a
  `$third-party` rule both still decide correctly through the proxy.
- Resource type inferred from `Sec-Fetch-Dest` / `Accept` / extension, with a
  test that a type-restricted rule fires end-to-end through the proxy and one
  that an unhinted request stays `Unknown`.
- Pass-through latency unchanged vs p2-02 baseline (re-run bench, compare).
- API.md + CONTEXT.md updated for RequestEvent/kind filter.
- Gates green.

## Out of scope

Policies/per-client (p2-06), HTML content rewriting (Phase 4).

## Suggested prompt

> Read plan/wip/phase2/p2-04-http-filtering-pipeline.md, RULE_ENGINE.md §HTTP
> matching, ARCHITECTURE.md wiring rules, and the "Raised, not fixed" section of
> docs/code-review/phase2/p2-03-review.md. Wire verdicts into the proxy with type-aware
> block responses, RequestEvent flow, doc updates, and tests.
