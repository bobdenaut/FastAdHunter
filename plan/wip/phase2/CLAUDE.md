# Phase 2 — HTTP + Policies

**Objective:** ROADMAP.md Phase 2: HTTP proxy engine for unencrypted traffic
(streaming, pass-through fast path), URL-path rules and HTTP `$options`
activate in the Rule Engine, and the **Policy** concept lands — named bundles
of rule lists + settings assignable to clients and schedules, activating
`$client` rules and per-client enforcement in both DNS and HTTP pipelines.
Operating mode `dns+http` becomes real.

**Why this order:** the parser correctness fix (`p2-00`) comes before
everything — it is not HTTP work at all, but `p2-03` activates rules this
phase's target lists do not currently classify correctly, so nothing downstream
is trustworthy until it lands. Then scaffold + docs (new crate changes the
architecture — docs update in the same change, per root CLAUDE.md). Proxy core
before filtering (streaming pass-through must be solid before verdicts touch
it). URL rules before the filtering pipeline that consumes them. Policies last
on the rules side, then enforcement wires both pipelines. Memory accounting
lands immediately before verification, so the phase's soak can attribute
growth to a component instead of only reporting RSS. Then proof.

## Architecture note — widening the Rule Engine interface

**Validated by `p2-03` — the interface held as written.** `lookup_http` was
added as a second typed entry point over the same compiled ruleset, with no
trait object and no second matcher. Two things the note left open were decided
in implementation and are recorded in `docs/code-review/p2-03-review.md`:

1. **A request consults both tiers**, under one precedence order. The note only
   said the domain index would be *reused* by the non-intercepted HTTPS path;
   it turns out the intercepted path needs it too, or a host blocked for DNS is
   still fetched over HTTP.
2. **The request type is `fah_model::HttpRequest`**, not `HttpMatchCtx`. It is
   a request model living in a crate that must not know the matcher exists.

Still not an ADR. Promote it after `p2-04` proves the interface survives having
an actual consumer — one entry point with no caller is a weaker claim than the
note deserves.

The matcher is **protocol-model aware rather than transport aware**. It exposes
typed interfaces for each request model while remaining independent of
networking, sockets and protocol transport — it never sees UDP, TCP, TLS or
QUIC, only a DNS question or an HTTP request.

Today that interface is DNS-typed:

```rust
pub fn lookup(&self, domain: &str, qtype: &QueryType) -> MatchDecision
pub fn verdict(&self, domain: &str, qtype: &QueryType) -> Verdict
```

`(domain, QueryType)` is a DNS question. A URL-path rule or an HTTP `$option`
cannot be expressed through it: an HTTP match needs host **plus** path, method,
resource type (script / image / xhr) and a third-party flag — and `$client`
identity, which both models need.

**Direction for `p2-03`:** add a *second typed entry point* over the same index
(`lookup_http(&HttpMatchCtx)`), rather than generalising to a trait object.
`&dyn` dispatch on the hot path breaks the no-virtual-call rule in
PERFORMANCE.md. Target shape: **one matcher, one typed interface per request
model, zero I/O on the hot path.**

HTTPS is expected to reuse the HTTP request model after TLS termination, so no
third matcher interface should be required for Phase 3:

```text
DNS    ──────────────────────────────→  lookup_dns()
HTTP   ──────────────────────────────→  lookup_http()
HTTPS  ──→ TLS termination ──────────→  lookup_http()
HTTPS  ──→ not intercepted ──────────→  host-only match over the same domain index
           (SNI hostname only)
```

**SNI is not DNS.** It is a TLS ClientHello field, carries no `QueryType`, and
therefore cannot go through `lookup_dns()` — that signature does not accept it.
What the non-intercepted path reuses is the **domain-matching primitive** (the
compiled domain index), not the DNS request model. Whether that surfaces as a
host-only entry point or as an `HttpMatchCtx` with the host populated and no
path — in which case path-dependent rules correctly fail to match — is a
`p2-03` decision, deliberately left open here.

Either way **neither** HTTPS path justifies a `lookup_https()`. Adding one later
would be a symptom of transport leaking into the matcher, not a new requirement.

This is a widening, not a redesign: the rules themselves are model-specific
(`$dnstype` is meaningless for HTTP; `$script` / `$third-party` / path anchoring
are meaningless for DNS). New HTTP request types belong in `fah-model` (pure
data), not in `fah-rules`.

**`p2-03` is not "activate what is already in the index" — that phrasing was
wrong twice over, and both halves are now settled.** It appeared here and in
`p2-03` on the strength of ADR-0003's "stored inactive, counted, activated by
later phases".

1. *Classification* was broken until `p2-00`: real EasyList was misdetected
   entirely, and 389 path-qualified rules were misfiled as whole-domain DNS
   rules. Fixed — see `docs/code-review/p2-00-review.md`.
2. *Storage* never existed. `RuleKind::Inactive(InactiveReason)` is a bare
   `Copy` enum with no payload; `ParsedRule` deliberately drops the line text.
   Nothing of `||paypal.com^*/pixel.gif` survives parsing except one
   discriminant saying "URL pattern". ADR-0003 carries a correction note.

So `p2-03` had to **reintroduce retention** for those rules and pay their
memory — the ~1 MiB is new storage the task introduced, not a cost the system
already carried. The interface argument above is unaffected: what was wrong is
what reaches the matcher, not its shape.

**Settled by `p2-03`:** retention costs **+3.32 ms** of parse across EasyList +
EasyPrivacy (+16.7 %, measured against the real pre-change parser in a detached
worktree, both arms core-pinned) and the compiled tier is **1.06 MiB** for
18,778 rules. `InactiveReason` now carries only what genuinely stays inactive —
`Cosmetic`, `ClientScoped`, `UnsupportedUrlPattern` — and `UrlPattern` /
`HttpOption` are gone, because those rules compile.

Note the matcher's *hot path* is pure, but `fah-rules` as a crate is not I/O-free
— the list lifecycle pulls `reqwest` and `tokio`.

## Note on `p2-09`'s position

`p2-09` was added after the phase was planned, so it sorts *after* the phase
verification task. That is deliberate rather than an oversight: `p2-08` is
HTTP-scoped (proxy benches, dst-nat, `dns+http` on-device) and would not have
covered a query-log reader anyway, so `p2-09` carries its own on-device check
instead. The phase is not done until both are.

If the order is ever cleaned up, the fix is renaming `p2-08` → `p2-10`, not
renumbering `p2-09`.

## Follow-ups from the mimalloc review (not yet tasks)

Both surfaced while reviewing `fah-rules` during the allocator work
(`docs/code-review/mimalloc-and-todos-review.md` §2–§3). Small, independent, and
neither blocks anything.

1. **Bound `pending_cache` in aggregate.** A persistently unwritable `/data`
   parks each failing list's raw text in RAM, bounded by `lists ×
   MAX_LIST_BYTES` — set by configuration rather than uptime, so hard rule 4
   holds, but loose. Cap the total (8 MB is generous against real list sizes) and
   mark the list `degraded` instead of parking beyond it. **Do not "fix" it by
   dropping the text**: `compile()` re-reads `/data`, so the list would silently
   revert to its stale cached copy on the next unrelated refresh.
2. **Use `Content-Length` on list fetch.** Reject before transferring when the
   declared length already exceeds `max_bytes`, instead of streaming up to 64 MB
   to discover it. Separately, pre-size the body from a ceiling `fah-rules`
   chooses (not from the server's number) to avoid the doubling peak, where the
   last growth step holds both buffers at once.

## Follow-up from the p2-03 review — DONE 2026-08-01 as `p2-10`

**A substring index was required to meet the 1 ms lookup budget for the
EasyList + EasyPrivacy target corpus on the RB5009. It has been built and
verified on-device: `unindexed` is 0 on both corpora and 8 KiB fell
5,335.7 → 553.8 µs (p99 569.5), inside budget at every measured length.**
Report: `docs/code-review/p2-10-url-substring-index.md`. The measurement that
motivated it is below, kept because the two corpora are still the argument for
scoping any claim about this tier.

Measured on-device by a throwaway probe container; production served DNS
throughout. Report and raw evidence:
[`docs/code-review/p2-08-url-lookup-arm.md`](../../../docs/code-review/p2-08-url-lookup-arm.md)
+ `docs/code-review/p2-08-arm/`.

| Corpus | URL rules | Unindexed | 8 KiB URL, RB5009 | vs 1 ms |
| --- | --- | --- | --- | --- |
| EasyList + EasyPrivacy — the target | 18,781 | 77 | **5,336 µs** (p99 5,802) | ❌ 5.3× over |
| The deployed lists — this router today | 714 | 3 | 377 µs (p99 415) | ✔ 2.7× under |

**Scope the claim in both directions.** "The URL tier is fine" is true only of a
deployment carrying three unindexed rules; "the URL tier misses its budget" is
true only at target-corpus scale. **The boundary is the unindexed-rule count,
not the URL-rule count** — the deployed lists are overwhelmingly DNS-shaped
(1,043,886 DNS rules against 714 URL rules; 15 of 17 lists contribute *zero*
URL rules), which is why this router is comfortable and the target corpus is
not.

The deferral rested on "3.09 µs sits 300× inside the budget", which was the
short-URL figure. At 8 KiB the same corpus cost 5.3× the *whole* budget, and
even 4 KiB cost 2,092 µs.

**Two claims from that measurement did not survive `p2-10`, and both are worth
remembering as mistakes rather than deleting.**

1. ~~"97 % of the lookup is the unindexed scan; ≈ 176 µs fixed + 67 µs per
   unindexed rule."~~ A two-point fit across corpora differing **26× in rule
   count** cannot attribute the gap to one variable. Indexing every rule moved
   645.9 → 441.2 µs on x86 — **32 %**, not 97 %. The rest was candidate rules
   scanning the URL for their first byte a byte at a time, ~3 µs apiece at
   8 KiB; SIMD removed it.
2. ~~"A single busy core remained at 350–700 MHz and no boost to the nominal
   1.4 GHz was observed, so every ARM figure is an upper bound."~~ The `p2-10`
   run of the same probe reported **1400 MHz**. A control arm across the two
   sessions — the deployed corpus at 8 KiB, barely touched by the change —
   moved **−4.6 %**, where a real 4× clock change had to show ~4×. Both runs
   executed at the same effective speed; RouterOS's frequency fields do not
   predict throughput. The ~9× x86 → RB5009 factor is what both sessions agree
   on and is the calibration input.

**Both fixes shipped as `p2-10`.** `unindexed` is 0 on both corpora, 8 KiB is
553.8 µs on-device (p99 569.5) against the 1 ms budget, for +2,372 bytes of
heap.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 0 | `p2-00-adblock-parser-correctness.md` | Sample-based format detection; `\|\|d^*/path` → URL pattern, `\|\|d^\|` → exact-host DNS rule; `degraded` list status. Verified against the reference ruleset: 0 exceptions lost, 0 new blocks (`docs/code-review/p2-00-review.md`) | Opus | DONE |
| 1 | `p2-01-http-scaffold.md` | `fah-http` (L3) binds `[http]` only when `engine.mode` names http; dual-stack bind shared via `fah_common::listen`; ARCHITECTURE/CONTEXT/CONFIGURATION/PERFORMANCE + diagrams (`docs/code-review/p2-01-review.md`) | Sonnet | DONE |
| 2 | `p2-02-http-proxy-core.md` | Transparent streaming proxy; `HostResolver` port moved to L1 and shared; default-deny egress guard at L1 judging the **resolved** address (shared with Phase 3); connection handler generic over the stream, proven over `DuplexStream`; pass-through +35 µs in-process vs a 1 ms budget (`docs/code-review/p2-02-review.md`) | Opus | DONE |
| 3 | `p2-03-url-rules-activation.md` | URL tier live: retention (+3.3 ms parse, measured against the real pre-change parser), 16 B records + exact/prefix/suffix token index, `lookup_http` over both tiers, no regex; 18,778 EasyList+EasyPrivacy rules in **1.06 MiB**, verdict **3.09 µs** (real corpus, pinned, post-review), allocation-free. **Reviewed 2026-08-01: 6 defects fixed** — an unbounded matcher (one request measured at 313 ms), `$dnsrewrite` deciding HTTP requests, type-restricted exceptions over-blocking on `Unknown`, `$domain=example.*` never firing, a fail-open `$domain=` payload, and over-long payloads dropped while counted active (`docs/code-review/p2-03-review.md`) | Opus | DONE |
| 4 | `p2-04-http-filtering-pipeline.md` | Verdicts wired into the proxy on the **head**, before resolve — a block costs no DNS lookup and no upstream connection (asserted against a connection-counting origin). Type-aware block responses; one widened event channel (`fah_model::Event`) so the shed figure stays one number; `kind=dns\|http` through the query log and API. Port stripping + resource-type inference landed as p2-03's review required. Added latency **32.4 µs** vs p2-02's 33.6 µs, of which FastAdHunter's own work is **≈5 µs — 80 % of it the verdict** (`docs/code-review/p2-04-review.md`) | Opus | DONE |
| 5 | `p2-05-policy-model.md` | Policy/Schedule/Assignment + `$client` active. **The phase's memory risk is closed**: all policies share one compiled ruleset with a 16-bit per-rule visibility mask, so N policies cost **+2.03 MiB flat** at deployed scale instead of ~+17 MiB each (measured 12.099 MiB per-policy vs 6.839 MiB shared, over four lists) — and a deployment with no policies allocates nothing. Schedules are POSIX TZ, DST-correct, no tzdb dependency. Found and fixed an accidental parser invariant that would have compiled `\|\|d^$client` into an **unrestricted** domain block (`docs/code-review/p2-05-review.md`) | Opus | DONE |
| 6 | `p2-06-per-client-enforcement.md` | Both pipelines resolve the client's policy per request via one precomputed snapshot (schedules evaluated and names resolved on a 20 s tick in the **binary** — `fah-rules` stays a pure library). Policy CRUD + `/clients/{ip}/policy`; only mask-affecting edits recompile. Per-policy stats, `?policy=` log filter, `rules/test` policy context. **Two defects fixed**: a p2-05 mask-drop that let a policy enabling no compiled list see *everything*, and an uncanonicalized v4-mapped peer that broke HTTP assignments. Zero-config cost measured at **+8.5 ns/query**, documented rather than claimed as free (`docs/code-review/p2-06-review.md`) | Opus | DONE |
| 7 | `p2-07-historical-memory-breakdown.md` | **Feature complete 2026-08-02, gates green.** `MemoryComponents` split out of `MemoryBreakdown` and persisted into `PerfSample` with `minor_page_faults`, both `#[serde(default)]`; the sampler reuses the telemetry poll's breakdown (and its RSS, so a row's residual is single-instant) instead of collecting twice; `/history/perf` serves `memory` + `minor_page_faults` with the residual derived per row through the same `residual()` the live path uses; `fastadhunter_memory_collection_seconds` removed. ~6.9 MB/30 days at 60 s, hot path untouched (`docs/code-review/p2-07-review.md` §11). **Flips to DONE when the p2-08 soak proves the instrument on-device:** a residual per sample across the whole window, live and persisted agreeing, and the slope over the final third **stated** — a *drifting* residual is a finding that opens a leak task, not a reason to hold this one open | Opus | AWAITING SOAK |
| 8 | `p2-08-phase2-verification.md` | **Dev-box work complete 2026-08-02, gates green (779 tests).** Opaque-body bench added beside the head-path arm: head adds **+36.8 µs**, and the added cost is flat 8 KiB→1 MiB then scales at 8 MiB — *which* cost is not attributed, the bench separates neither copying, buffering, wakeups nor cache effects. `http_e2e` boots the real binary in `dns+http` (page relayed, ad script blocked empty, second client blocked by `$client`, all three on the events socket and as `kind=http`); boot machinery extracted to `tests/common`. PERFORMANCE.md rows replace the placeholders with target separated from measurement, and record the **123.74 MiB boot peak vs the 128 MB budget** plus the deferred streaming-parse lever. 0.2.10 built, deployed, dst-nat live; T0 captured (`docs/code-review/0.2.10-soak-baseline.md`). **Found: HTTP interception is IPv4-only** — mirror rules written, deliberately deferred to after the window. Earlier out-of-order criterion (long-URL sweep) still stands; do not re-run it. **Flips to DONE when the soak closes** and the generated-load run supplies the on-device HTTP numbers | Opus | AWAITING SOAK |
| 9 | `p2-09-query-log-reader.md` | `QueryLogReader` over the persisted segments; unified `GET /queries` (no filter → ring, any filter → segments); `next_sequence` derived at boot; `qtype`; `oldest_retained`; flush on shutdown | Opus | WAITING |
| 10 | *(no task file — done directly)* | URL substring index: literal-run n-gram tier so every rule is indexed (`unindexed` 77 → 0), plus SIMD (`memchr`) for the unanchored first-byte scan and the `*`-widening retry. On-device 8 KiB **5,335.7 → 553.8 µs**, 11.6× on x86, +2,372 bytes heap; retracts two claims from the p2-08 measurement (`docs/code-review/p2-10-url-substring-index.md`) | Opus | DONE |

**Definition of done:** router dst-nats port 80 to the container; a plain-HTTP
page loads through the proxy with ad requests blocked at URL level; a "kids"
policy on one client blocks a domain other clients still reach, on a schedule;
non-filtered traffic passes through with negligible added latency; budgets in
the updated PERFORMANCE.md hold on-device.

**Key risks:** most web traffic is HTTPS — Phase 2 filters only the unencrypted
remainder, real value completes in Phase 3 (set user expectations in docs);
transparent interception needs RouterOS dst-nat rules (mitigation: p2-08
documents them, user runs them, rollback is one rule removal); streaming
pass-through latency regressions (mitigation: fast path benched in p2-02
before filtering exists).
