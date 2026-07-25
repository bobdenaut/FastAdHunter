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

**Provisional, to be validated during `p2-03`.** Not an ADR yet; promote it to one
afterwards if the interface holds.

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

So `p2-03` must **reintroduce retention** for `UrlPattern`/`HttpOption` rules
and pay their memory — the measured ~1 MiB is new storage the task
introduces, not a cost the system already carries. The interface argument above
is unaffected: what was wrong is what reaches the matcher, not its shape.

Note the matcher's *hot path* is pure, but `fah-rules` as a crate is not I/O-free
— the list lifecycle pulls `reqwest` and `tokio`.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 0 | `p2-00-adblock-parser-correctness.md` | Sample-based format detection; `\|\|d^*/path` → URL pattern, `\|\|d^\|` → exact-host DNS rule; `degraded` list status. Verified against the reference ruleset: 0 exceptions lost, 0 new blocks (`docs/code-review/p2-00-review.md`) | Opus | DONE |
| 1 | `p2-01-http-scaffold.md` | `fah-http` crate (L3) + ARCHITECTURE/CONTEXT/CONFIGURATION updates | Sonnet | WAITING |
| 2 | `p2-02-http-proxy-core.md` | Transparent streaming proxy, pass-through fast path (heavy) | Opus | WAITING |
| 3 | `p2-03-url-rules-activation.md` | URL-path + HTTP `$options` matchers activate in fah-rules (heavy) | Opus | WAITING |
| 4 | `p2-04-http-filtering-pipeline.md` | Verdicts wired into the proxy: block responses, events, stats | Sonnet | WAITING |
| 5 | `p2-05-policy-model.md` | Policy = named bundle of lists + settings; schedules; `$client` | Sonnet | WAITING |
| 6 | `p2-06-per-client-enforcement.md` | DNS + HTTP consult policy per client; policy API endpoints | Sonnet | WAITING |
| 7 | `p2-07-memory-accounting.md` | `fastadhunter_memory_component_bytes` + `_residual_bytes`; shared `fah_model::MemoryBreakdown`; `fah-dns` untouched (`docs/code-review/p2-07-review.md`) | Sonnet | DONE |
| 8 | `p2-08-phase2-verification.md` | HTTP benches + budgets, e2e tests, RB5009 dns+http validation | Sonnet | WAITING |

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
