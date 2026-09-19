# P4-03 — Streaming HTML Rewriter Core — Implementation Plan

Task file: [p4-03-streaming-rewriter.md](p4-03-streaming-rewriter.md).
Depends on p4-02 ([plan](p4-02-cosmetic-rules-activation-plan.md)) — read
its review file's Implementation Summary; the `CosmeticLookup` / `SetKey`
shape is what this task consumes. Written 2026-09-19 against workspace 0.4.1
and **lol_html 3.0.1** (docs.rs, checked 2026-09-19); re-check both before
editing.

## Decision — built, dormant by default

Owner decision 2026-09-19 (phase [CLAUDE.md](CLAUDE.md)): implement the whole
task; the code ships with `[html] enabled = false` and stays dormant on the
deployed box. Nothing here asks for interception or cosmetic lists on the
router — verification is dev-box tests and benches. While the gate is closed
no rewriter, set cache entry or sink buffer is ever constructed; the dormant
box pays binary size only, which the review records.

## Standing rules that bite here

- No comments in Rust (hook). `// SAFETY:` only.
- Everything in this task lives in `fah-http`. No proxy wiring: the rewriter
  is proven in isolation behind the seam p4-01 left in place.
- **No new hot-path lock without disclosure.** The one lock this design
  needs (the output sink, Step 1) is uncontended by construction; the review
  file carries its figure and the owner decides whether hard rule 3 wants an
  ADR. The set cache is lock-free.
- **No new crate** beyond `lol_html` and `encoding_rs`. lol_html does not
  re-export `encoding_rs` (checked on docs.rs 3.0.1), and
  `AsciiCompatibleEncoding::new` takes `&'static encoding_rs::Encoding`, so
  declare it directly at the version lol_html pins — it arrives in the tree
  with lol_html either way. Two workspace crates are not yet declared by
  `fah-http` and need a line each in `crates/fah-http/Cargo.toml`:
  `arc-swap = { workspace = true }` under `[dependencies]` (the set cache)
  and `rand = { workspace = true }` under `[dev-dependencies]` (the seeded
  split tests and page generator). `mimalloc` and `criterion` are already
  dev-dependencies.
- `.md` edits after the gates, one yes per file. No commit.

## Read before editing

| Need | Where |
| ---- | ----- |
| Streaming principle | ARCHITECTURE.md §HTTP Pipeline principles (~170–221) |
| Golden rules, how to measure, the 9× factor | PERFORMANCE.md §Golden rules (~10–33), §Measuring reliably (~221–349) |
| A/B discipline, noise | docs/measurement-traps.md |
| Response funnel and body type | `crates/fah-http/src/proxy.rs` ~54 (`ProxyBody`), ~664 (`to_client_response`) |
| Existing bench rig | `crates/fah-http/benches/proxy.rs` ~132–270 |
| lol_html surface | docs.rs `lol_html` 3.0.1: `Settings`, `send`, `HtmlRewriter`, `MemorySettings`, `Selector`, `OutputSink`, `AsciiCompatibleEncoding`, `errors::RewritingError` |

## lol_html 3.0.1 facts this plan relies on

| Item | Verified surface |
| ---- | ---------------- |
| Send-safe rewriter | `lol_html::send::{Settings, HtmlRewriter, ElementContentHandlers}`; `Settings::new_send()`. The docs note rewriting is sequential, so `Send` buys nothing but the ability to hold the rewriter inside a body polled on any Tokio worker — which is exactly what we need. |
| Settings builders | `append_element_content_handler((Cow<Selector>, ElementContentHandlers))`, `with_encoding(AsciiCompatibleEncoding)`, `with_memory_settings(MemorySettings)`, `with_strict(bool)`, `with_adjust_charset_on_meta_tag(bool)`, `with_graceful_bail_out_on_content_handler_error(bool)`. `element!(sel, \|el\| …)` yields the tuple `append_element_content_handler` takes. |
| Memory | `MemorySettings::new().with_preallocated_parsing_buffer_size(n).with_max_allowed_memory_usage(n).with_graceful_bail_out_on_memory_limit_exceeded(true)`. With graceful bail-out on, exceeding the limit "flushes every input byte it has received but not yet emitted to the sink, as-is" before returning the error. |
| Rewriter | `HtmlRewriter::new(settings, sink)`; `write(&mut self, &[u8]) -> Result<(), RewritingError>`; `end(self) -> Result<(), RewritingError>`. After an `Err`, calling `write`/`end` again **panics** — the rewriter must be dropped, never called again. |
| Errors | `RewritingError::{MemoryLimitExceeded, ParsingAmbiguity, ContentHandlerError}`. `with_strict(false)` avoids `ParsingAmbiguity` bails on ambiguous markup; keep `strict = false` — we would rather rewrite less than fail. |
| Sink | `trait OutputSink { fn handle_chunk(&mut self, chunk: &[u8]); }`, blanket-implemented for `FnMut(&[u8])`; the final chunk has zero length. The rewriter **owns** the sink and exposes no accessor. |
| Encoding | `AsciiCompatibleEncoding::new(&'static encoding_rs::Encoding) -> Option<Self>`; `utf_8()`. `None` for UTF-16 and friends. lol_html never transcodes. |
| Selectors | `Selector: FromStr<Err = SelectorError> + Clone + Send + Sync`. Supported: `*`, type, `.class`, `#id`, all attribute operators (`=`, `~=`, `\|=`, `^=`, `$=`, `*=`, `i`/`s` flags), `:not()`, `:nth-child`, `:first-child`, `:nth-of-type`, `:first-of-type`, descendant and `>` combinators. Not supported: sibling combinators, `:last-child`, `:has()`, everything else. |

## Step 0 — decisions

| Question | Decision | Why |
| -------- | -------- | --- |
| Content-Encoding | **Negotiate identity.** A rewrite candidate asks upstream for `Accept-Encoding: identity` (the request-side edit is p4-04's). A response that still arrives with a `Content-Encoding` other than `identity` passes through unmodified and is counted. No streaming decompressor. | The task's named default; no ADR needed unless it changes. The counter p4-04 adds and the soak p4-05 runs are what would justify a decompressor later. Trade-off to state in the review: candidate documents cross the WAN uncompressed. |
| Memory bound per rewrite | `const PREALLOC: usize = 16 KiB`, `const MAX_REWRITE_BUFFER: usize = 256 KiB`, graceful bail-out **on**. | Worst case = `max_concurrent_rewrites × 256 KiB` (p4-04's bound; 64 × 256 KiB = 16 MiB under the 128 MB RAM budget). A knob only if a measurement asks. |
| Fail-open form | Graceful bail-out on both memory and handler errors; on any `Err` from `write`/`end`: drop the rewriter, drain the sink, then relay the offending chunk and every later chunk untouched. | lol_html already flushed its unprocessed input as-is; nothing is lost, nothing after the failure is transformed, nothing is truncated. |
| Style-injection form | One `<style>` appended to `<head>`: groups of ≤ 64 selectors, each `:where(s1,s2,…){display:none!important}`. | `:where()` takes a forgiving selector list — one invalid selector drops itself, not the group. Specificity 0 plus `!important` still loses to a page's own `!important` display rule; document as a limitation. |
| No `<head>` | A `body` handler prepends the same `<style>` if the `head` handler never fired (shared `Arc<AtomicBool>`). Neither element ⇒ no injection; removals still apply. | Streaming: `<head>` is known absent only when `<body>` arrives. |
| Removal path | Every selector `Selector::from_str` accepts gets `element!(sel, \|el\| { el.remove(); Ok(()) })`. Rejected ones are hide-only, counted per compiled set. | Removal survives a CSP that blocks inline styles; injection survives late DOM insertion. Both, per selector, is the point. |
| Charset | Parse `charset=` from `Content-Type`; `encoding_rs::Encoding::for_label` (direct dependency, see Standing rules); `AsciiCompatibleEncoding::new` ⇒ rewrite with `with_adjust_charset_on_meta_tag(true)`; missing charset ⇒ `utf_8()` with the same adjustment; `None` (UTF-16/32, unknown label) ⇒ no rewriter. | Under any ASCII-compatible encoding, non-ASCII bytes pass through lol_html unchanged, so only non-ASCII-compatible documents are unsafe, and those pass through. |
| Cache key | p4-02's `CosmeticLookup::fingerprint() -> SetKey`, **not** the hostname. | Nearly every host resolves to the generic-only set; host keys would compile the same set once per host and churn `max_selector_cache`. |
| Cache structure | `ArcSwap<CacheMap>` copy-on-write, bounded FIFO by insertion sequence. Hit = one `load` + one probe. | Lock-free on the hit path. Misses are bounded by capacity per ruleset generation; a concurrent double-build is harmless. Not an LRU: an LRU writes on every hit, which is the lock this avoids. Say "bounded FIFO" in the docs. |
| Cache invalidation | The map records the `Matcher::generation()` it was built for; a lookup with a different generation swaps in an empty map first. | Ruleset swap ⇒ cache cleared on the next HTML response; no task, no subscription. |
| Output sink | `struct Sink(Arc<Mutex<BytesMut>>)` implementing `OutputSink` — `handle_chunk` appends, `set_encoding` is a no-op (lol_html never transcodes); `poll_frame` drains the same `Arc` after each `write`/`end`. A closure would do the same work, but a closure's type cannot be named in `RewrittenBody`'s field. | The rewriter owns its sink and exposes nothing back, so output must leave through shared state. Both sides run on the one task polling the body, so the lock is never contended. Rejected: an `unsafe impl Send` newtype over `RefCell` to save an uncontended lock/unlock — no `unsafe` for nanoseconds. The review carries the measured per-chunk cost. |

## Step 1 — module layout

```text
crates/fah-http/src/html/
  mod.rs        pub(crate) surface; HtmlGate re-export (moved from html.rs, p4-01)
  compiled.rs   CompiledSet: style text + removable selectors, from a CosmeticLookup
  cache.rs      SetCache: ArcSwap<CacheMap>, bounded FIFO, generation-aware
  charset.rs    fn decide(content_type: Option<&HeaderValue>) -> Option<AsciiCompatibleEncoding>
  rewriter.rs   fn build(set: &Arc<CompiledSet>, enc, sink) -> send::HtmlRewriter; the consts
  body.rs       RewrittenBody: http_body::Body over Incoming
```

### `CompiledSet`

```text
pub(crate) struct CompiledSet {
    style: Arc<str>,                     // "<style>…</style>", built once
    removable: Vec<lol_html::Selector>,  // parsed once
    selector_count: u32,
    hide_only: u32,
}
impl CompiledSet {
    pub(crate) fn compile(lookup: &CosmeticLookup<'_>) -> CompiledSet
}
```

`HtmlRewriter<'h, O, H>` borrows handlers for `'h`; the body owns an
`Arc<CompiledSet>` and cannot lend a borrow of itself. Per response, build
`Settings` with `Cow::Owned(selector.clone())` for each removable selector
and clone the `Arc<str>` style into the `head`/`body` closures. Bench the
clone cost at 300 selectors (Step 3); if it shows, cache a prebuilt handler
list per set — never a self-referential struct.

### `RewrittenBody`

```text
pub(crate) struct RewrittenBody<B> {
    inner: B,                                              // Incoming in the proxy; any Body<Data = Bytes> in tests and benches
    rewriter: Option<send::HtmlRewriter<'static, Sink>>,   // None once ended or failed open
    sink: Arc<Mutex<BytesMut>>,                            // the Arc inside Sink
    pending: BytesMut,                                     // drained sink output not yet yielded
    state: State,                                          // Rewriting | FailedOpen | Done
    stats: Arc<RewriteStats>,
}
impl<B> http_body::Body for RewrittenBody<B>
where B: http_body::Body<Data = Bytes>, B::Error: Into<Box<dyn Error + Send + Sync>>
{ type Data = Bytes; type Error = Box<dyn Error + Send + Sync>; … }
```

The generic is not optional: `hyper::body::Incoming` cannot be constructed
outside a live connection (hyper 1 has no channel body), so every unit test,
the split property test and the latency bench feed a
`http_body_util::StreamBody` or `Full` instead. The proxy monomorphises
`RewrittenBody<Incoming>`; no `dyn`, no second code path.

`poll_frame`:

1. `pending` non-empty ⇒ yield `Frame::data(pending.split().freeze())`.
2. Poll `inner`. Data frame while `Rewriting`: `rewriter.write(&chunk)`; then
   drain the sink into `pending` (one `lock`, `split()`); on `Ok` go to 1.
   On `Err`: `stats.failed_open += 1`, set `FailedOpen`, take and drop the
   rewriter (never call it again — it panics), drain the sink (graceful
   bail-out already flushed unprocessed input there), then append the raw
   chunk. While `FailedOpen`: relay the chunk untouched.
3. Trailers frame ⇒ pass through.
4. `inner` ends while `Rewriting`: `rewriter.end()` (take it out of the
   `Option` first — `end` consumes), drain, set `Done`; `Err` here is handled
   as in 2. Return `None` once `pending` is empty.
5. `size_hint()` ⇒ `SizeHint::default()` (unknown). `is_end_stream()` ⇒
   `Done && pending.is_empty()`.

Per chunk: lol_html copies the input into its buffer, the sink copies output
into the shared `BytesMut`, `split()` moves without copying. Two copies is the
floor the API allows; measure it and say so.

`stats` records `write_calls`, `write_nanos` (one `Instant` pair around
`write`; the p4-04 metric is derived as sum/count), `rewrites` (on
construction), `failed_open`.

### `SetCache`

```text
pub(crate) struct SetCache { map: ArcSwap<CacheMap>, capacity: usize, hits: AtomicU64, misses: AtomicU64 }
struct CacheMap { generation: u64, next_seq: u64, entries: HashMap<SetKey, (u64, Arc<CompiledSet>)> }
impl SetCache {
    pub(crate) fn new(capacity: usize) -> Self                              // html.max_selector_cache
    pub(crate) fn get_or_compile(&self, generation: u64, lookup: &CosmeticLookup<'_>) -> Arc<CompiledSet>
}
```

Miss: compile, clone the map with the entry inserted (evict the lowest `seq`
when at capacity), `store`. Generation mismatch: start from an empty map.

### Surface for p4-04 (`html/mod.rs`)

```text
pub(crate) fn prepare(
    cache: &SetCache, generation: u64, lookup: &CosmeticLookup<'_>, content_type: Option<&HeaderValue>,
) -> Option<(Arc<CompiledSet>, AsciiCompatibleEncoding)>   // None ⇔ nothing to do

pub(crate) fn rewrite<B>(inner: B, set: Arc<CompiledSet>, enc: AsciiCompatibleEncoding, stats: Arc<RewriteStats>) -> RewrittenBody<B>
```

`prepare` returns `None` on `lookup.is_empty()` before touching the cache,
and on an unusable charset. "No selectors ⇒ no rewriter" is the `Option`.

## Step 2 — tests

| Test | Where | Proves |
| ---- | ----- | ------ |
| Style injected once, in `<head>`; 65 selectors ⇒ two `:where()` groups | `compiled.rs`/`rewriter.rs` tests | injection form |
| Removal, children included | same | removal path |
| Hide-only selector (`a ~ b`, `:last-child`, `:has(x)`) appears in the style block, counted in `hide_only` | same | selector routing |
| No `<head>` ⇒ style prepended to `<body>`; neither ⇒ unchanged apart from removals | same | fallback |
| Charset matrix: `windows-1252` bytes ≥ 0x80 preserved; `utf-16le` ⇒ `None`; missing ⇒ UTF-8; `<meta charset>` switch honoured | `charset.rs` + integration | never corrupt a page |
| Chunk-boundary property | `crates/fah-http/tests/html_rewrite_splits.rs` | six documents ≤ 2 KiB: **every** single split point, then 2 000 seeded random multi-splits (print the seed on failure) — output equals the whole-document rewrite byte-for-byte |
| Fail-open | same file | a document with one attribute value > `MAX_REWRITE_BUFFER` ⇒ `MemoryLimitExceeded`; every input byte present in order after the failure point; prefix before it rewritten; `failed_open == 1`; no panic |
| Bounded memory | `crates/fah-http/tests/html_rewrite_memory.rs`, counting allocator over `mimalloc` — reuse the harness already in this crate, `crates/fah-http/tests/intercept_alloc.rs` | 8 MiB synthetic page **generated chunk by chunk** from the seed and fed through a `StreamBody` in 16 KiB chunks — never materialised, or the page itself is the peak. Take a live-bytes baseline after the rig is built, record the peak delta while streaming, and assert it stays flat between the 1 MiB and 8 MiB runs (O(chunk), not O(document)). The absolute ceiling is set from the first measured peak plus a stated margin and written into the test with that figure in the review — not guessed here (principle 8). |
| Cache | `cache.rs` tests | hit/miss counters; FIFO eviction at capacity; generation change empties on next call; two threads missing the same key end with one entry |
| `Send` | compile-time `fn assert_send<T: Send>()` | `RewrittenBody<Incoming>: Send` |

## Step 3 — bench

`crates/fah-http/benches/html_rewrite.rs` (+ `[[bench]] name = "html_rewrite"
harness = false`):

| Group | Axis | Report |
| ----- | ---- | ------ |
| `html_rewrite_throughput` | page 10 KiB / 100 KiB / 1 MiB × selectors 10 / 300, plus a raw-copy baseline row per size | `Throughput::Bytes` ⇒ MiB/s |
| `html_rewrite_latency` | same matrix through `RewrittenBody<StreamBody<…>>` fed from an `mpsc` channel | added µs vs raw copy |
| `html_chunk_size` | 100 KiB, 300 selectors, chunks 4 / 16 / 64 KiB | MiB/s |
| `compiled_set_build` | 10 / 300 / 3 000 selectors | µs per compile (the cache-miss cost) |
| `handler_clone` | 300 selectors | µs per response for the `Cow::Owned` clones |

Synthetic pages: seeded generator, ~1 tag per 40 bytes, 20 % of elements
with class attributes, 5 % matching a selector. Record the seed and
parameters as the corpus. Dev-box numbers plus the ~9× conversion, labelled
as converted; p4-05 measures the device.

## Step 4 — docs (list, then wait)

| File | Edit |
| ---- | ---- |
| ARCHITECTURE.md §HTTP Pipeline | Under the stage line from p4-01, five lines: hide via `<style>` in `<head>`, remove via lol_html handlers, fail open with graceful bail-out, O(chunk) memory with a 256 KiB ceiling, identity negotiation, bounded FIFO set cache keyed by effective-set fingerprint. |
| CONTEXT.md §HTML Rewriting | The two mechanisms and "fail open", one sentence each. |
| PERFORMANCE.md §Design costs worth knowing | One row: two copies per chunk and the 256 KiB per-rewrite bound. Budget rows are p4-05's. |
| docs/decisions/ | Only if the Content-Encoding decision deviates. |

## Step 5 — gates, review file, stop

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
cargo bench -p fah-http --bench html_rewrite
cargo bench -p fah-http --bench proxy      # A/B vs pre-change checkout: the seam is untouched, expect noise
```

Review file `docs/code-review/phase4/p4-03-streaming-rewriter-review.md`:
bench tables (corpus = seed + parameters, workload, device), the peak-memory
figure, the sink lock's per-chunk cost, the selector-to-path mapping, the
charset matrix, and every limitation (`!important` specificity, documents
with neither `head` nor `body`, hide-only selectors, uncompressed candidate
transfer).

Chat line:

`Task done. Report written to docs/code-review/phase4/p4-03-streaming-rewriter-review.md. Awaiting "start code review".`

## Out of scope

- `to_client_response`, `plan_response`, request headers, events, counter
  exposure, policy gating, the concurrency bound (p4-04).
- Streaming decompression.
- Extended/procedural cosmetics.

## Hand-off facts for p4-04

- `html::prepare(cache, generation, &lookup, content_type)` decides; `None`
  is pass-through. `html::rewrite<B>(inner, set, enc, stats)` builds the
  body; p4-04 extends it with the semaphore permit.
- `RewrittenBody<B>` is generic over the inner body; the proxy uses
  `RewrittenBody<Incoming>`. Its `Error` is `Box<dyn Error + Send + Sync>`,
  the same type `http_body_util::Either` already gives `ProxyBody`.
- `RewrittenBody::size_hint()` is unknown on purpose: drop `Content-Length`
  and let hyper frame it.
- `RewriteStats { rewrites, failed_open, write_nanos, write_calls }`;
  `SetCache { hits, misses }`.
- Fail-open lives inside the body; the caller sees no error.
