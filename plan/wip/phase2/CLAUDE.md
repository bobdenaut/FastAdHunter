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

## Follow-up from the p2-03 review — DECIDED 2026-08-01, no longer deferred

**A substring index is required to meet the 1 ms lookup budget for the
EasyList + EasyPrivacy target corpus on the RB5009. It is not required for the
corpus this router currently runs.**

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
short-URL figure. At 8 KiB the same corpus costs 5.3× the *whole* budget, and
even 4 KiB costs 2,092 µs. The conclusion survives the frequency caveat: the
RB5009 held **350 MHz** through the run and does not boost a single busy core,
but correcting all the way to its 1.4 GHz nominal still gives 1.33 ms.

**What an index buys, and what it does not.** At 8 KiB the cost is ≈ 176 µs
fixed + **67 µs per unindexed rule**, so at 77 rules **97 % of the lookup is the
unindexed scan** — an index removes that term, 5,336 µs → ~176 µs (~30×). It
does not make long URLs free: ~176 µs is the floor even with a perfect index,
because tokenization and indexed-candidate checks scale with URL length too.

**Not yet a task.** The decision is recorded; sequencing it against `p2-05`
through `p2-09` is open. It is Phase 2 work by origin but nothing in the phase's
definition of done depends on it — the deployed corpus meets budget today.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 0 | `p2-00-adblock-parser-correctness.md` | Sample-based format detection; `\|\|d^*/path` → URL pattern, `\|\|d^\|` → exact-host DNS rule; `degraded` list status. Verified against the reference ruleset: 0 exceptions lost, 0 new blocks (`docs/code-review/p2-00-review.md`) | Opus | DONE |
| 1 | `p2-01-http-scaffold.md` | `fah-http` (L3) binds `[http]` only when `engine.mode` names http; dual-stack bind shared via `fah_common::listen`; ARCHITECTURE/CONTEXT/CONFIGURATION/PERFORMANCE + diagrams (`docs/code-review/p2-01-review.md`) | Sonnet | DONE |
| 2 | `p2-02-http-proxy-core.md` | Transparent streaming proxy; `HostResolver` port moved to L1 and shared; default-deny egress guard at L1 judging the **resolved** address (shared with Phase 3); connection handler generic over the stream, proven over `DuplexStream`; pass-through +35 µs in-process vs a 1 ms budget (`docs/code-review/p2-02-review.md`) | Opus | DONE |
| 3 | `p2-03-url-rules-activation.md` | URL tier live: retention (+3.3 ms parse, measured against the real pre-change parser), 16 B records + exact/prefix/suffix token index, `lookup_http` over both tiers, no regex; 18,778 EasyList+EasyPrivacy rules in **1.06 MiB**, verdict **3.09 µs** (real corpus, pinned, post-review), allocation-free. **Reviewed 2026-08-01: 6 defects fixed** — an unbounded matcher (one request measured at 313 ms), `$dnsrewrite` deciding HTTP requests, type-restricted exceptions over-blocking on `Unknown`, `$domain=example.*` never firing, a fail-open `$domain=` payload, and over-long payloads dropped while counted active (`docs/code-review/p2-03-review.md`) | Opus | DONE |
| 4 | `p2-04-http-filtering-pipeline.md` | Verdicts wired into the proxy on the **head**, before resolve — a block costs no DNS lookup and no upstream connection (asserted against a connection-counting origin). Type-aware block responses; one widened event channel (`fah_model::Event`) so the shed figure stays one number; `kind=dns\|http` through the query log and API. Port stripping + resource-type inference landed as p2-03's review required. Added latency **32.4 µs** vs p2-02's 33.6 µs, of which FastAdHunter's own work is **≈5 µs — 80 % of it the verdict** (`docs/code-review/p2-04-review.md`) | Opus | DONE |
| 5 | `p2-05-policy-model.md` | Policy = named bundle of lists + settings; schedules; `$client` | Sonnet | WAITING |
| 6 | `p2-06-per-client-enforcement.md` | DNS + HTTP consult policy per client; policy API endpoints | Sonnet | WAITING |
| 7 | `p2-07-memory-accounting.md` | **REOPENED 2026-07-26, NARROWED 2026-07-30** — instrumentation all shipped (component `heap_bytes()`, both metric families, `/debug/memory`, `AllocatorStats` in 0.2.8); the one thing left is persisting `MemoryComponents` + `minor_page_faults` into `PerfSample` and `/history/perf`, so a soak can chart residual rather than only RSS. The reopen's motivating question is **answered** — the RouterOS climb was page cache (`docs/code-review/0.2.7-router-memory-and-throughput.md`) — so this is now a safety net, not an investigation, and its priority drops accordingly | Sonnet | WAITING |
| 8 | `p2-08-phase2-verification.md` | HTTP benches + budgets, e2e tests, RB5009 dns+http validation. **One acceptance criterion already MET out of order (2026-08-01): the on-device long-URL sweep.** A substring index is required for the EasyList+EasyPrivacy target corpus (8 KiB = 5,336 µs, 5.3× over budget) and not for this router's own lists (377 µs); x86→ARM factor is a flat ~9×; the RB5009 does not boost a single busy core (350 MHz under load). Do not re-run it — `docs/code-review/p2-08-url-lookup-arm.md`. Everything else in the task is untouched | Sonnet | WAITING |
| 9 | `p2-09-query-log-reader.md` | `QueryLogReader` over the persisted segments; unified `GET /queries` (no filter → ring, any filter → segments); `next_sequence` derived at boot; `qtype`; `oldest_retained`; flush on shutdown | Opus | WAITING |

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
