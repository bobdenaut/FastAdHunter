# P4-04 — Pipeline Integration and Selective Application — Implementation Plan

Task file: [p4-04-pipeline-integration.md](p4-04-pipeline-integration.md).
Depends on p4-03 ([plan](p4-03-streaming-rewriter-plan.md)); read the p4-01,
p4-02 and p4-03 review files' Implementation Summaries. Written 2026-09-19
against workspace 0.4.1; re-check every anchor before editing.

## Decision — built, dormant by default

Owner decision 2026-09-19 (phase [CLAUDE.md](CLAUDE.md)): implement the whole
task; the code ships with `[html] enabled = false` and stays dormant on the
deployed box. Nothing here asks for interception or cosmetic lists on the
router — verification is dev-box tests and benches. With the gate closed,
`plan_response` leaves at step 1 (`Disabled`) and `prepare_request` never
touches `Accept-Encoding`, so the dormant box is byte-identical to today; the
pass-through A/B in Step 6 is the proof and the review records it.

## Standing rules that bite here

- No comments in Rust or TS (hook).
- Layering: `fah-http` reads `fah-rules` and `fah-model`; `fah-stats`,
  `fah-metrics` and `fah-api` learn about rewriting only through
  `fah_model::Event`/DTOs and the counters snapshot the binary hands them.
  No sibling edge.
- `fah-model` stays logic-free: new enum + field, nothing else.
- Pass-through must stay byte-identical and unbuffered; every new condition
  is evaluated on the head, never by reading a body.
- `.md` edits after the gates, one yes per file. No commit.

## Read before editing

| Need | Where |
| ---- | ----- |
| Both response funnels | `proxy.rs` `handle` (~365–440), `intercept.rs` `handle_intercepted` (~290–345), `to_client_response` (~664) |
| Request forwarding (where `Accept-Encoding` is adjusted) | `proxy.rs` `to_upstream_request` (~536); `intercept.rs` the `upstream.send(request)` call in `handle_intercepted` |
| Verdict capture and event emission | `proxy.rs` `judge` (~567), `emit` (~624), `Judged` |
| Event DTO | `crates/fah-model/src/request_event.rs` (`RequestEvent` ~39, `new` ~58, `under_policy` ~76) |
| Policy types | `fah-config` `schema/policy.rs` `PolicyConfig` (~41); `fah-model` `policy.rs` `Policy` (~56); `fah-rules` `policy.rs` `PolicySet` (~64), `ActivePolicies` (~273); `fah-api` `wire.rs` `PolicyResponse`/`CreatePolicyRequest`/`PatchPolicyRequest` (~829–890) |
| Counters and telemetry | `proxy.rs` `ProxyCounters`/`ProxyStats` (~66–120); `fah_model::HttpCounters` (`engine.rs`); `fah-metrics` `registry.rs` (~40–75 fields, ~197 verdict counting, ~302 snapshot) |
| Rolling stats | `fah-stats` `stats.rs` `record_http` (~159), `aggregates.rs` |
| Policy semantics — what recompiles, what does not | CONFIGURATION.md §Mutability classes (last two paragraphs); RULE_ENGINE.md §Policies |
| E2E rigs | `crates/fastadhunter/tests/http_e2e.rs` (`config_toml`, `fetch(from, …)`, `collect_http_events`), `e2e_https.rs` (`full_mode_blocks_at_every_layer`, `test-harness` feature), `tests/common/mod.rs` |

## Deliverables

1. Three-condition gate at the seam, cheapest first, plus a concurrency bound.
2. Request-side identity negotiation for candidates, on both pipelines.
3. Framing: rewritten responses carry no `Content-Length`; hyper frames them.
4. `ProxyBody` as a three-variant body.
5. Per-policy toggle `html_filtering`, runtime through the policy endpoints.
6. Observability: event field, counters, rolling stats, metrics.
7. Tests for every gate condition, both pipelines, two clients, fail-open.
8. Docs (after the yes).

## Step 0 — decisions

| Question | Decision |
| -------- | -------- |
| Candidate status codes | `200` only. `206` (`Content-Range`), `304`, `204`, `1xx` carry no rewritable body; error pages are not worth a special case. |
| `HEAD` | Never a candidate: no body to rewrite, and the origin's `Content-Length` must survive. |
| `Content-Type` test | Value starts with `text/html` (ASCII case-insensitive), parameters allowed. `application/xhtml+xml` is not a candidate (XML strictness). |
| Encoding test | No `Content-Encoding`, or exactly `identity`. Anything else ⇒ pass-through, `Skipped(Encoded)`. |
| Concurrency bound | `tokio::sync::Semaphore(html.max_concurrent_rewrites)`, `try_acquire_owned`; no permit ⇒ pass-through, `Skipped(Busy)`. The permit lives in `RewrittenBody` until drop. New boot key, default 64 (worst case 64 × 256 KiB = 16 MiB). |
| Headers dropped on rewrite | `content-length`, `content-digest`, `repr-digest`, `digest`, `content-md5`. Nothing else — **never** touch `Content-Security-Policy` (ROADMAP.md §Two constraints). |
| Where the policy toggle lives | `fah_config::PolicyConfig.html_filtering: Option<bool>` (TOML, `None` = inherit `true`), `fah_model::Policy.html_filtering: bool`, compiled into `PolicySet` as a `u16` bitmask `html_off`, exposed as `PolicySet::html_allowed(PolicyId) -> bool` and copied onto `ActivePolicies` so `judge()` reads it from the `Arc` it already loads. Default policy: `true`. |
| Does the toggle recompile the ruleset? | `set_policies` (`lifecycle/mod.rs` ~1355) only stores the set; the policy handler (`routes.rs` ~1170–1188) then calls `recompile()` when its `Recompile` argument says so, and `republish_policies` after. The toggle rides that handler **unchanged**: a policy edit is a rare admin action and the recompile it already does is accepted. Do not add a "skip the recompile" branch — the `ActivePolicies` copy that `republish_policies` publishes is what makes the toggle live, with or without the recompile. Say so in the review. |
| Event timing | The event is emitted at response-head time (as today, `emit` in `handle`). `rewrite` therefore says what was *decided*: `Rewritten { selectors }` or `Skipped(reason)`. A fail-open mid-stream is counter-only (`rewrite_failed_open`); the event is not rewritten after the fact. Document this in API.md. |
| Rewrite duration | Not a histogram over the event channel (unknown at emit time). `RewriteStats.write_nanos`/`write_calls` from p4-03, exposed as `_seconds_sum` / `_seconds_count` — a Prometheus summary without quantiles, bounded, no per-response allocation. |

## Step 1 — the gate at the seam

`html::plan_response` (p4-01 stub) becomes:

```text
pub(crate) fn plan_response(
    ctx: &HtmlContext,              // gate, cache, semaphore, stats — one Arc on Proxy/TlsProxy
    judged: &Judged,                // html_allowed, host, policy_id, matcher — see below
    matcher: &Matcher,              // &judged.matcher, the Arc<Matcher> the request was judged with
    method: &Method,
    parts: &http::response::Parts,
) -> RewritePlan

pub(crate) enum RewritePlan {
    PassThrough(SkipReason),
    Rewrite { set: Arc<CompiledSet>, enc: AsciiCompatibleEncoding, permit: OwnedSemaphorePermit },
}
```

Evaluation order, each step returning `PassThrough` on failure:

1. `ctx.gate.is_enabled()` — one relaxed load. Else `Disabled`.
2. `judged.html_allowed` — a `bool` captured in `judge()`. Else `PolicyOff`.
3. `method != HEAD && status == 200`. Else `NotHtml`.
4. `Content-Type` starts with `text/html`. Else `NotHtml`.
5. `Content-Encoding` absent or `identity`; no `Content-Range`. Else `Encoded`.
6. `matcher.cosmetic().lookup(&judged.request.host, policy)` — then
   `html::prepare(&ctx.cache, matcher.generation(), &lookup, content_type)`.
   `None` ⇒ `NoSelectors` (empty set) or `Charset` (`prepare` must say
   which; split its `None` into a two-variant reason or return
   `Result<_, SkipReason>`).
7. `ctx.rewrites.clone().try_acquire_owned()`. Else `Busy`.

Steps 1–5 cost a few header lookups; 6 is the one that touches the ruleset;
7 is one atomic. Non-HTML traffic — most of it — leaves at step 3 or 4.

`Judged` (`proxy.rs` ~560) holds `request`, `resource_type`, `verdict` and
the policy **name** today — nothing step 6 can call `lookup` with. Add three
fields, all captured inside `judge()` where they are already in hand:
`policy_id: PolicyId` (from `ctx.policy`, the `ClientContext` `context_for`
returns), `html_allowed: bool` (Step 4), and `matcher: Option<Arc<Matcher>>`
(`None` when `rules` is `None`, which makes step 6 `NoSelectors`). The same
`Arc<Matcher>` must serve `judge()` and step 6: `judge` already loads it
(`rules.matcher()`); keeping it in `Judged` costs one refcount bump and means
a swap between judge and response cannot mix generations.

`to_client_response(response, plan)`: on `Rewrite`, strip the headers listed
in Step 0, then `ProxyBody::Rewritten(html::rewrite(body, set, enc, stats,
permit))`. On `PassThrough`, exactly the p4-01 behaviour.

Both callers pass the plan: `Proxy::handle` (~432) and
`TlsProxy::handle_intercepted` (`intercept.rs` ~341). One function, two call
sites, no second path.

## Step 2 — request side

```text
pub(crate) fn prepare_request(headers: &mut HeaderMap, candidate: bool)
```

`candidate = gate.is_enabled() && judged.html_allowed && matches!(resource_type, Document | Subdocument) && !matcher.cosmetic().is_empty()`.
The last term is one load on the `Arc<Matcher>` `judge()` already holds;
without it a box with the gate on and no cosmetic rules — today's deployed
corpus — would fetch every document uncompressed for nothing.
When true, set `Accept-Encoding: identity` (replace, do not append). Call it
from `to_upstream_request` (`proxy.rs` ~536) and before `upstream.send`
in `handle_intercepted`. Trade-off for the review: candidate documents cross
the WAN uncompressed; the soak's `Skipped(Encoded)` count says whether origins
honour it.

## Step 3 — `ProxyBody`

Replace `type ProxyBody = Either<Incoming, Full<Bytes>>` with:

```text
pub(crate) enum ProxyBody { Upstream(Incoming), Synth(Full<Bytes>), Rewritten(RewrittenBody<Incoming>) }
impl http_body::Body for ProxyBody { type Data = Bytes; type Error = Box<dyn Error + Send + Sync>; … }
```

`poll_frame` is a `match` per variant (monomorphised); `size_hint` and
`is_end_stream` delegate. The error type is not a new decision:
`http_body_util::Either` already makes `ProxyBody::Error`
`Box<dyn Error + Send + Sync>` (its `either.rs`, `type Error`), both hyper
servers accept it today, and `RewrittenBody` uses the same type (p4-03).
Map `Incoming`'s `hyper::Error` with `.into()`. Update `refuse()`,
`block::response` mapping, and every `Either::Left/Right` use.

Framing check: with `size_hint` unknown, hyper 1's HTTP/1.1 server sends
`Transfer-Encoding: chunked` and its HTTP/2 server sends DATA frames with END_STREAM;
an HTTP/1.0 client gets `Connection: close` framing. Write a test for the
h1 case that reads the raw response and asserts `chunked` and no
`Content-Length`.

## Step 4 — policy toggle plumbing

| Place | Change |
| ----- | ------ |
| `fah-config` `PolicyConfig` | `html_filtering: Option<bool>`; `POST /api/v1/config` keeps rejecting `policies` (unchanged rule). |
| `fah-model` `Policy` | `html_filtering: bool`. |
| `fah-rules` `PolicySet::from_config` | Build `html_off: u16`; `html_allowed(id)`; copy the mask into `ActivePolicies` in `active_at`. |
| `fah-rules` `Matcher::context_for` / `judge()` | `Judged.html_allowed = active.html_allowed(ctx.policy)`. |
| `fah-api` | `PolicyResponse.html_filtering: bool`; `CreatePolicyRequest`/`PatchPolicyRequest` optional field; persist through the same path as `blocking_mode`. |
| dashboard `pages/policies*.tsx`, `api/types.ts` | Toggle in the policy editor; tests. Comment hook applies. |

## Step 5 — observability

| Layer | Change |
| ----- | ------ |
| `fah-model` | `pub enum RewriteOutcome { Rewritten { selectors: u32 }, Skipped(SkipReason) }`, `pub enum SkipReason { Disabled, PolicyOff, NotHtml, Encoded, NoSelectors, Charset, Busy }` (serde `snake_case`); `RequestEvent.rewrite: Option<RewriteOutcome>` with `skip_serializing_if = "Option::is_none"`; builder `with_rewrite`. DNS events never set it. |
| `fah-http` `emit` | Takes the plan's outcome; `Judged` unchanged otherwise. |
| `ProxyCounters`/`ProxyStats` | `rewrites`, `rewrite_failed_open`, `rewrite_busy`, `rewrite_skipped_encoded`, `rewrite_write_nanos`, `rewrite_write_calls`, `selector_cache_hits`, `selector_cache_misses`. `snapshot()` folds `RewriteStats` and `SetCache` counters in. |
| `fah_model::HttpCounters` | An `html: HtmlCounters` sub-struct with the same names ⇒ `GET /api/v1/telemetry`. |
| `fah-stats` | `record_http`: when `event.rewrite` is `Rewritten`, bump a `rewritten` counter in the rolling window; `StatsSnapshot.http_rewritten: u64`. History rollups unchanged. |
| `fah-metrics` | From events: `fastadhunter_requests_rewritten_total`. From the telemetry snapshot (the `ArcSwap` precedent of `dns_tcp_connections`): `fastadhunter_html_rewrite_failed_open_total`, `_busy_total`, `_skipped_encoded_total`, `fastadhunter_html_rewrite_write_seconds_sum`/`_count`, `fastadhunter_html_selector_cache_hits_total`/`_misses_total`. |
| `fah-config` | `[html] max_concurrent_rewrites = 64` (boot), validated `1..=4096`. Boot means one line in `BOOT_KEYS` (`fah-api/src/config_store.rs` ~37, the key alone, never the section) plus one `["html", "max_concurrent_rewrites"]` arm in `env.rs` `apply_one` — the two lists p4-01 set up for `max_selector_cache`. |

## Step 6 — tests

Unit (`fah-http`):

| Test | Proves |
| ---- | ------ |
| Gate table: each of the seven conditions failing alone ⇒ `PassThrough(reason)` with the right reason; all passing ⇒ `Rewrite` | order and reasons |
| `prepare_request` sets identity only for candidates | request side |
| Rewritten h1 response: no `Content-Length`, `chunked`; digest headers gone; CSP header intact | framing |
| Busy: semaphore of 1, two candidates ⇒ second is `Busy` and byte-identical | bound |

E2E (`crates/fastadhunter/tests/html_e2e.rs`, plain HTTP; extend
`e2e_https.rs` for the intercepted leg under `--all-features`):

| Case | Assert |
| ---- | ------ |
| Origin serves `/page` (`text/html`, contains `<div class="ad">`) and `/asset.js`; user rules `##.ad` via `PUT /api/v1/rules/user` | `/page` rewritten (style present, div gone), `/asset.js` byte-identical |
| Each gate condition individually: `[html] enabled=false`; policy off; `Content-Type: text/plain`; `Content-Encoding: gzip` sent regardless; host whose only rule is an exception; `charset=utf-16`; `max_concurrent_rewrites=1` with one slow response held open | byte-identical body and `Content-Length` preserved; event carries the matching `skipped` reason |
| Two clients: `127.0.0.2` assigned to a policy with `html_filtering=false`, `127.0.0.1` default | first untouched, second rewritten, both events say why |
| Intercepted HTTPS (`e2e_https.rs` rig) | same page rewritten through the terminate leg; spliced host untouched |
| Fail-open: origin sends one attribute value of 512 KiB | full byte count delivered, `rewrite_failed_open == 1` on `/telemetry` |
| `WS /api/v1/events` | HTTP events carry `rewrite`; DNS events do not |
| `/telemetry`, `/stats`, `/metrics` | the new fields appear with the expected counts |

Benches, A/B against a pre-change checkout:

- `cargo bench -p fah-http --bench proxy` (`http_pass_through`,
  `http_opaque_body`) with the gate **enabled** and non-HTML content: within
  noise of the p2-02 baseline.
- `cargo bench -p fah-http --bench intercept` (group `https_h2_download`):
  unchanged.

## Step 7 — docs (list, then wait)

| File | Edit |
| ---- | ---- |
| CONFIGURATION.md | `[html] max_concurrent_rewrites` line in the block; `[[policies]] html_filtering = true` with its runtime note; §Mutability classes: the toggle applies like assignments (no recompile), `max_concurrent_rewrites` is boot. |
| API.md | §Policies: the field in the example and the table; §Events: the `rewrite` field with both shapes and the "decided at head time; fail-open is counter-only" sentence; §`GET /api/v1/telemetry`: the `html` counters; §`GET /api/v1/stats`: `http_rewritten`; the `/metrics` names if the doc lists them. |
| CONTEXT.md §Pass-through | "except an HTML Rewriting candidate" wording; §Policy: the toggle. |
| ARCHITECTURE.md §HTTP Pipeline | The seven-step gate, one line each, under the stage from p4-01. |

## Step 8 — gates, review file, stop

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
cargo bench -p fah-http --bench proxy --bench intercept   # A/B, see Step 6
```

Dashboard tests from `dashboard/frontend/` as well.

Review file `docs/code-review/phase4/p4-04-pipeline-integration-review.md`:
the gate order with per-step cost, the A/B tables (corpus, workload,
device), the `set_policies` recompile finding, the framing test evidence,
and the uncompressed-transfer trade-off restated with the counter that
tracks it.

Chat line:

`Task done. Report written to docs/code-review/phase4/p4-04-pipeline-integration-review.md. Awaiting "start code review".`

## Out of scope

- Budget rows and on-device numbers (p4-05).
- Streaming decompression, extended cosmetics.
- Any change to how spliced HTTPS is handled — it is untouchable by design.

## Hand-off facts for p4-05

- Counters on `/telemetry` under `http.html`; metric names above.
- `Skipped(Encoded)` is the number that decides whether a decompressor is
  ever justified.
- Worst-case rewrite memory is `max_concurrent_rewrites × 256 KiB`; the
  selector cache is `max_selector_cache × bytes-per-entry` (p4-03 measured
  the entry size).
