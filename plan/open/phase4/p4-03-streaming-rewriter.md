# P4-03 — Streaming HTML Rewriter Core

**Phase:** 4 · **Depends on:** p4-02 · **Model:** Opus

## Goal

A lol_html-based streaming rewriter in fah-http that applies a selector set to
an HTML byte stream — element hiding/removal — with bounded memory, correct
charset handling, and measured overhead. Proven in isolation before touching
the proxy.

## Context

ARCHITECTURE.md: streaming before buffering — nothing loads whole documents.
lol_html is built for exactly this (chunked rewriting, constant memory).
PERFORMANCE.md positioning: this is the feature AdGuard Home lacks — but it
must not cost the pass-through path anything (RB5009: 1 GB RAM shared).

## Scope

- Rewriter takes a p4-02 selector set + an input chunk stream, yields output
  chunks. Two mechanisms, chosen per selector:
  - **Element removal** via lol_html element handlers for plain CSS selectors
    lol_html supports.
  - **Style injection** — one `<style>` block of hide rules appended to
    `<head>` for the remainder (survives dynamic DOM insertion; element
    removal alone misses late-inserted nodes).
  - Document which selectors take which path; unsupported selectors skipped
    and counted, never a hard error.
- Compiled lol_html selector/handler sets cached per hostname (bounded LRU,
  `html.max_selector_cache`), invalidated on ruleset swap.
- Charset: honor `Content-Type` charset and meta-declared encodings within
  lol_html's supported set; unsupported/undetectable encoding ⇒ pass through
  unmodified (never corrupt a page).
- Content-Encoding strategy decided and implemented here at the stream level:
  prefer negotiating identity upstream for rewrite candidates
  (strip/limit `Accept-Encoding`); streaming decompress only if measurement
  shows identity negotiation fails often enough to matter. Record the
  decision as an ADR if it deviates from this default.
- Malformed HTML: lol_html's error handling ⇒ fail open (emit remaining
  input unmodified downstream of the failure point); never truncate a page.
- Bench (criterion): rewrite throughput MB/s and added latency on
  representative pages (small/median/1 MB+), with 0/10/300 selectors —
  numbers feed the budgets p4-05 writes into PERFORMANCE.md.
- Tests: hiding + removal correctness, charset pass-through, chunk-boundary
  splits (selector match spanning chunks), bounded memory on a multi-MB
  streamed document.

## Acceptance criteria

- Rewriting a document streamed in arbitrary chunk sizes yields identical
  output to whole-document rewriting (property test over split points).
- Memory during rewrite is O(chunk), not O(document) (asserted on a large
  synthetic page).
- A page with zero applicable selectors is never passed through the rewriter
  by callers (API shape makes "no selectors ⇒ no rewriter" natural).
- Gates green; bench numbers recorded in the task's completion notes.

## Out of scope

Proxy wiring, events/stats, policy gating (all p4-04).

## Suggested prompt

> Read PERFORMANCE.md, ARCHITECTURE.md (streaming principle), and
> plan/wip/phase4/p4-03-streaming-rewriter.md. Build the lol_html streaming
> rewriter in fah-http with removal + style-injection paths, charset and
> encoding handling, bounded selector caching, and criterion benches; prove
> chunk-boundary correctness with property tests.
