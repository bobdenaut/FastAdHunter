# Phase 4 — HTML Filtering

**Objective:** ROADMAP.md Phase 4: streaming HTML rewriting powered by
lol_html — element/cosmetic rules (`##`, `#@#`) activate in the Rule Engine
and are applied to HTML responses flowing through the Phase 2/3 HTTP pipeline.
The differentiator AdGuard Home lacks: filtering **inside** pages. Applied
only where required — non-HTML and selector-less traffic passes through
untouched, preserving the pass-through fast path.

**Why this order:** docs + config gating first (new subsystem — docs update in
the same change, per root CLAUDE.md). Cosmetic rule compilation before the
rewriter that consumes it (fah-rules owns all rule processing). Rewriter core
proven in isolation before it touches the proxy (streaming + bounded memory
must be solid first). Pipeline integration wires verdicts, policies, events
and stats together, then proof against budgets.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p4-01-html-scaffold.md` | `[html]` config + gating + doc/diagram updates; rewrite hook stub | Sonnet | WAITING |
| 2 | `p4-02-cosmetic-rules-activation.md` | Cosmetic rules compile into per-hostname selector sets (heavy) | Opus | WAITING |
| 3 | `p4-03-streaming-rewriter.md` | lol_html streaming rewriter: bounded, charset/encoding-aware (heavy) | Opus | WAITING |
| 4 | `p4-04-pipeline-integration.md` | Selective application in HTTP/HTTPS pipeline; policies, events, stats | Opus | WAITING |
| 5 | `p4-05-phase4-verification.md` | Rewrite budgets in PERFORMANCE.md, benches, e2e, RB5009 validation | Sonnet | WAITING |

**Definition of done:** a page loaded through the proxy (plain HTTP, or HTTPS
on an intercepted client) has its ad elements hidden/removed by cosmetic rules;
`#@#` exceptions honored; non-HTML responses and hosts with no applicable
selectors take the pass-through fast path with no added buffering; memory stays
bounded regardless of page size; rewrite overhead holds the budgets added to
PERFORMANCE.md; counts of now-active cosmetic rules visible per list in the API.

**Key risks:** HTML filtering only reaches traffic the proxy can see — plain
HTTP plus intercepted-client HTTPS (set expectations in docs; DNS/SNI layers
keep covering the rest); Content-Encoding — lol_html needs decoded bytes, so
candidate requests must negotiate identity encoding or stream-decompress
(decided and benched in p4-03/p4-04); page CSP can block injected styles
(mitigation: element removal path + documented limitation); lol_html selector
compilation cost per response (mitigation: compiled-selector caching per
hostname, bounded).
