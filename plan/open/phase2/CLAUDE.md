# Phase 2 — HTTP + Policies

**Objective:** ROADMAP.md Phase 2: HTTP proxy engine for unencrypted traffic
(streaming, pass-through fast path), URL-path rules and HTTP `$options`
activate in the Rule Engine, and the **Policy** concept lands — named bundles
of rule lists + settings assignable to clients and schedules, activating
`$client` rules and per-client enforcement in both DNS and HTTP pipelines.
Operating mode `dns+http` becomes real.

**Why this order:** scaffold + docs first (new crate changes the architecture —
docs update in the same change, per root CLAUDE.md). Proxy core before
filtering (streaming pass-through must be solid before verdicts touch it).
URL rules before the filtering pipeline that consumes them. Policies last on
the rules side, then enforcement wires both pipelines, then proof.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p2-01-http-scaffold.md` | `fah-http` crate (L3) + ARCHITECTURE/CONTEXT/CONFIGURATION updates | Sonnet | WAITING |
| 2 | `p2-02-http-proxy-core.md` | Transparent streaming proxy, pass-through fast path (heavy) | Opus | WAITING |
| 3 | `p2-03-url-rules-activation.md` | URL-path + HTTP `$options` matchers activate in fah-rules (heavy) | Opus | WAITING |
| 4 | `p2-04-http-filtering-pipeline.md` | Verdicts wired into the proxy: block responses, events, stats | Sonnet | WAITING |
| 5 | `p2-05-policy-model.md` | Policy = named bundle of lists + settings; schedules; `$client` | Sonnet | WAITING |
| 6 | `p2-06-per-client-enforcement.md` | DNS + HTTP consult policy per client; policy API endpoints | Sonnet | WAITING |
| 7 | `p2-07-phase2-verification.md` | HTTP benches + budgets, e2e tests, RB5009 dns+http validation | Sonnet | WAITING |

**Definition of done:** router dst-nats port 80 to the container; a plain-HTTP
page loads through the proxy with ad requests blocked at URL level; a "kids"
policy on one client blocks a domain other clients still reach, on a schedule;
non-filtered traffic passes through with negligible added latency; budgets in
the updated PERFORMANCE.md hold on-device.

**Key risks:** most web traffic is HTTPS — Phase 2 filters only the unencrypted
remainder, real value completes in Phase 3 (set user expectations in docs);
transparent interception needs RouterOS dst-nat rules (mitigation: p2-07
documents them, user runs them, rollback is one rule removal); streaming
pass-through latency regressions (mitigation: fast path benched in p2-02
before filtering exists).
