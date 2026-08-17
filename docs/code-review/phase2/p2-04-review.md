# p2-04 — HTTP filtering pipeline

Verdicts reach the proxy. A request is judged on its **head**, before the
destination is resolved, and a block is answered in the shape the resource type
asks for. Both pipelines now publish to one event channel, and the query log
carries either kind.

## What shipped

| Crate | Change |
| --- | --- |
| `fah-model` (L1) | **new** `request_event.rs` — `Request`, `RequestEvent`, `Event`, `EventKind` |
| `fah-http` (L3) | **new** `request.rs` (head → request model), **new** `block.rs` (type-aware refusals), `proxy.rs` takes the verdict and emits events, **new** `Ruleset` port |
| `fah-dns` (L3) | pipeline writes `Event::Dns` on the widened channel |
| `fah-metrics` (L3) | three HTTP families, kept separate from `queries_*` |
| `fah-stats` (L3) | query log carries `Event`; `record_http`; `kind` filter |
| `fah-api` (L3) | `kind=dns\|http` filter, widened query item |
| `fastadhunter` (L4) | fan-out dispatches on the event kind; proxy wired to ruleset + channel |

Docs updated in the same change per root CLAUDE.md: API.md (`GET /metrics`
families, `GET /api/v1/queries` `kind` filter and the widened item, the
persisted-format note), CONTEXT.md (three new binding terms: **Request Event**,
**Event**, and a rewritten **Query Log**; **Resource Type** gained the
`Unknown` asymmetry), ARCHITECTURE.md (pipeline diagram, one-channel property).

## The decision the task demanded first

The task refused to let the block-response code be written before the event
channel's shape was settled. Two exits were named; **(1) widening the channel
item** was taken:

```rust
enum Event { Dns(Box<QueryEvent>), Http(Box<RequestEvent>) }
```

The property that decided it is the one the p1.5 metrics work exists to
establish: **one bounded queue, one `dropped_events` counter, one number for
"we shed N events".** Two channels would each need their own drop counter, and
those two numbers cannot be added — they answer different questions about
different queues. A shed figure is an observability primitive; splitting it in
half to save refactoring is trading a permanent property for a one-off cost.

**The blast radius the task feared did not materialise.** It predicted changes
to "every consumer — `fah-stats`, `fah-metrics`, `fah-api`'s `EventHub`, and
`spawn_event_fanout`". In practice `Stats::record` and `Metrics::record` are
each called from exactly one place — the fan-out — so widening meant *one*
dispatch site plus the two new `record_http` / `record` entry points. The
estimate was written from the type graph rather than the call graph.

Boxing both variants is deliberate: they differ enough in size that an unboxed
enum would make every DNS event pay for the larger variant, on the channel the
DNS hot path writes to.

## Where the verdict sits, and why it is not where the diagram suggests

The pipeline diagram reads Claim → Resolve → Egress Guard → Rule Engine. The
code takes the verdict **immediately after the claim**, before resolution:

- The acceptance criterion is "zero bytes fetched upstream". Resolving first
  would already have leaked the intent to the upstream resolver, which is a
  disclosure a block is supposed to prevent.
- A blocked request then costs no DNS lookup and no TCP connect — the
  difference between "we refused it" and "we refused it cheaply".

`a_blocked_request_fetches_zero_bytes_upstream` asserts it against an origin
that counts accepted connections, so "blocked" means the socket was never
opened rather than that a function returned an enum.

## Block responses are not one response

A browser reacts to a failed subresource very differently from a failed
navigation, so the shape follows the resource type:

| Type | Response | Why |
| --- | --- | --- |
| script, stylesheet, image, font, media, object | 200, empty body, matching `Content-Type` | The element collapses and nothing retries. A 403 makes the page log an error, run a fallback, or fetch the same thing another way. |
| xmlhttprequest | 200, empty `application/json` | `fetch()` callers branch on `response.ok`; a failure triggers retry logic. |
| ping, websocket | 204 | The response is never read. No body at all. |
| document, subdocument, other, **unknown** | 403 naming FastAdHunter and the rule | A person who can read what happened can act on it. |

`Unknown` takes the **visible** form deliberately: a silently blank fetch leaves
nothing to act on, while an explanation is recoverable.

Every block carries `Cache-Control: no-store`. A block is a policy decision that
changes the moment a list refreshes; a cached one would outlive the rule that
caused it with no way for the user to tell.

**Rule text is HTML-escaped.** It comes from subscribed lists — third-party
input — and is rendered by a browser. `rule_text_is_escaped_into_the_page`
drives a rule containing `<script>` and `<img onerror>` through it.

## The two requirements inherited from the p2-03 review

Both were found by review rather than by a failing test, and both fail
*silently*.

**1. Port stripping.** `HttpRequest::host` and `document_host` are documented
"without port" and nothing in the matcher enforces it. With a port attached,
`registrable()` reduces `news.org:8080` to `com:8080` — which inverts the
third-party test, stops `$domain=` applying, and leaves the domain-tier walk
matching nothing. The URL itself **keeps** its port, because `||example.com^`
relies on `:` being a separator. Proven through a real proxy by
`a_domain_scoped_rule_is_judged_on_the_referer_host_without_its_port`, which
sends `Referer: http://news.org:8080/article` and asserts `$domain=news.org`
still fires.

`without_port` had a bug on its first write, caught by its own test: a *bare*
IPv6 address ends in `:<digits>` too, so `2606:2800::1` was being truncated to
`2606:2800:`. The host must be colon-free for the suffix to be a port.

**2. Resource-type inference**, in order of trustworthiness:

`Sec-Fetch-Dest` (the browser stating its own intent) → `Accept` (read only
where the first-listed type is unambiguous; `*/*` is *not* evidence) → the path
extension (a naming convention, not a declaration) → `Unknown`.

Declining to guess is the point. Since p2-03's review, `Unknown` is not
symmetric: a type-restricted **block** declines it, while a type-restricted
**exception** applies. Guessing would therefore be the only way to over-block.

## Where the microseconds go

The task asked for "pass-through latency unchanged vs p2-02". It is —
**32.4 µs added** (71.4 − 39.0) against p2-02's **33.6 µs** (70.1 − 36.5). But
that answer alone would have been misleading, so the per-request work was
decomposed instead of asserted (real corpus, pinned):

| Stage | Cost |
| --- | ---: |
| **Verdict** (`lookup_http`) | **3.96 µs** |
| Claim parse (`Host` → destination) | 114 ns |
| `Referer` → document host | 73 ns |
| Event Strings (host/path/method) | 83 ns |
| `retarget` (origin-form URI) | 211 ns |
| Hop-by-hop strip + `Via` (×2) | ~340 ns |
| `RequestEvent::new` | 83 ns |
| Resource-type inference | 32 ns |
| Clock reads | 40 ns |
| **FastAdHunter's own work** | **≈ 5 µs (15 %)** |
| **Second hop** (hyper parse/serialize ×2, two socket traversals, task wakeups) | **≈ 27 µs (85 %)** |

Three things this settles:

- **p2-02's unmeasured claim was right but understated.** It said "the proxy's
  own CPU work is a fraction of it"; it is 15 %, and the rest is structural —
  a second hop is what a transparent proxy costs and is not ours to optimise.
- **The verdict is 80 % of everything we spend.** Everything p2-04 added
  outside it — inference, claim parsing, event construction, header work —
  totals ~1 µs combined.
- **A first reading was wrong and splitting it fixed that.** Header stripping
  looked like 1.34 µs until the bench's own `HeaderMap::clone` was measured
  separately; the real figure is ~170 ns. Worth recording because it is the
  same error class as the p2-03 review's stale-baseline comparison.

**Honest qualification of "unchanged":** p2-04 added ~4.3 µs of real CPU (the
verdict, mostly). The socket bench cannot resolve that — it sits inside a
±2–3 µs noise band on a 32 µs measurement. Unchanged *as measured end-to-end*,
not free, and proportionally larger on the RB5009's slower core.

This joins up with p2-03: the unindexed scan is 67–83 % of a verdict, so the 77
rules no token could file cost **~2.7–3.3 µs of every proxied request** —
roughly **60 % of all CPU FastAdHunter spends per HTTP request**. That is the
single largest thing under our control, and it is why p2-08 owns the decision.

## Decisions worth challenging later

- **HTTP requests do not feed the domain aggregates.** `/stats`'s top domains
  have meant "names asked for" since p1-07. One page load is a single DNS
  question and then dozens of HTTP requests to the same host, so folding
  requests in would not enrich that table, it would inflate it by whatever a
  site's asset count happens to be. Per-*client* activity **is** fed in —
  "this client made N requests, M blocked" reads the same way whichever
  pipeline refused them, and p2-06 needs exactly that number.
- **`Ruleset` is a trait, not `Arc<ListManager>`.** `fah-rules` is a legitimate
  L2 dependency, so the direct type was available. Answering a verdict needs
  *the current matcher* and nothing else, while `ListManager` is the whole list
  lifecycle — downloads, schedules, `/data` writes. The narrow port states what
  the proxy uses and lets the integration tests drive a compiled `Matcher` with
  no filesystem and no config.
- **`size_hint().exact()` for the byte count.** Counting relayed bytes exactly
  would mean wrapping the body, which puts per-chunk work on the path p2-02
  exists to keep clean. A chunked response therefore reports `0`, which is
  honest rather than wrong.
- **The persisted query-log format changed.** Records now carry
  `"kind":"dns"` / `"kind":"http"`; older ones have no tag and will not parse.
  Tolerated *here specifically* because the log is bounded and pruned by age,
  no endpoint reads the segments yet (`p2-09` builds that reader), and the log
  self-heals within one retention window. API.md says so rather than leaving it
  to be discovered.
- **`Upgrade` is still stripped**, so WebSocket over port 80 cannot complete —
  correct hop-by-hop behaviour for a proxy that does not implement upgrades,
  carried over from p2-02 and still worth naming.

## Tests

47 unit in `fah-http` (claim, request-model derivation, block styles), 10 new
integration in `fah-http/tests/filtering.rs`, plus the model, stats, metrics and
API suites. **701 workspace tests pass**, up from 664.

`tests/filtering.rs` drives a real proxy against a real origin over real
sockets throughout: blocked ⇒ zero upstream connections; blocked script ⇒
empty 200; blocked document ⇒ 403 naming the rule; a URL rule blocking one path
while the rest of the host still streams; `$domain=` judged on a
port-carrying `Referer`; an exception overriding a block across tiers; events
carrying rule, status and byte count; a full channel shedding rather than
stalling the request; and a proxy with no ruleset preserving p2-02 behaviour.

## Gates

```text
cargo fmt --check                                     clean
cargo clippy --workspace --all-targets -- -D warnings clean
cargo test --workspace                                701 pass
cargo bench -p fah-http --bench proxy                 +32.4 µs vs p2-02's +33.6 µs
```

## Not done here

- **Policies and per-client scoping** (p2-05 / p2-06). `$client` rules stay
  `ClientScoped` and inactive.
- **The substring index** for the 77 unindexed rules. The profile above says it
  is the dominant term of the HTTP path, but the number that justifies an
  automaton is one this box cannot produce — `p2-08` owns that decision, with a
  `url_verdict/long_url_*` sweep and an explicit **x86 → ARM factor per
  length**. x86 reference, pinned: 4.39 µs at 64 B, 58.7 µs at 1 KiB,
  276.5 µs at 4 KiB, **714.7 µs at 8 KiB** — already 71 % of budget on the fast
  box.
- **`ProxyStats` still has no consumer.** `blocked` and `dropped_events` joined
  it; nothing publishes them yet. The counters that matter for filtering reach
  `/metrics` through the event channel instead.
