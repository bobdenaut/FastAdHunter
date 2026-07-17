# P2-04 — HTTP Filtering Pipeline

**Phase:** 2 · **Depends on:** p2-02, p2-03 · **Model:** Sonnet

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

## Acceptance criteria

- Blocked request: zero bytes fetched upstream (assert no upstream
  connection).
- Pass-through latency unchanged vs p2-02 baseline (re-run bench, compare).
- API.md + CONTEXT.md updated for RequestEvent/kind filter.
- Gates green.

## Out of scope

Policies/per-client (p2-06), HTML content rewriting (Phase 4).

## Suggested prompt

> Read plan/wip/phase2/p2-04-http-filtering-pipeline.md, RULE_ENGINE.md §HTTP
> matching, ARCHITECTURE.md wiring rules. Wire verdicts into the proxy with
> type-aware block responses, RequestEvent flow, doc updates, and tests.
