# P4-01 — HTML Filtering Scaffold — Implementation Plan

Task file: [p4-01-html-scaffold.md](p4-01-html-scaffold.md). This plan is the
step list an agent follows to implement it. Written 2026-09-19 against
workspace 0.4.1 (`a437bec`); re-check every anchor below before editing, the
line numbers drift.

## Decision — built, dormant by default (read first)

| Fact | Consequence |
| ---- | ----------- |
| Owner decision 2026-09-19: all five Phase 4 tasks are implemented and ship dormant — `[html] enabled = false` (phase [CLAUDE.md](CLAUDE.md)) | Build the whole task. The gate stays closed on the deployed box; nothing changes there until the owner runs `POST /api/v1/config {"html": {"enabled": true}}`. |
| Building needs neither interception nor cosmetic lists; using it needs both | Never ask the owner to switch interception on or load lists for this task. Verification is dev-box tests and benches. |
| Docs still say "parked" in ROADMAP.md, README.md, CONTEXT.md, docs/project-state.md, ADR-0009 | This task's doc step replaces that wording with "built, dormant by default". Each `.md` edit needs its own yes (root CLAUDE.md §Working agreement). |

## Standing rules that bite here

- **No comments in Rust or TS.** `.claude/hooks/no-rust-comments.sh` rejects the
  edit. The existing tree carries `///` docs from before the rule; new code
  carries none. `// SAFETY:` on `unsafe` is the only exception.
- **Layering.** `lol_html` and every rewriting type live in `fah-http` (L3).
  Nothing in `fah-model`, `fah-config` or `fah-rules` may name `lol_html`.
  `crates/fastadhunter/tests/layering.rs` enforces the crate graph.
- **`.md` files:** finish the code, run the gates, then list the exact doc
  edits and wait. The review file under `docs/code-review/phase4/` is the one
  exception.
- **No commit.** Report "gates green, awaiting go" and stop.

## Read before editing (targeted, not whole files)

| Need | Where |
| ---- | ----- |
| Pipeline stage list the new stage joins | ARCHITECTURE.md §HTTP Pipeline (Phase 2), lines ~134–221 |
| `runtime` vs `boot` key rule | CONFIGURATION.md §Mutability classes (~18–70) |
| `[http]` section shape to mirror | CONFIGURATION.md "HTTP engine (Phase 2)" block (~221–247) |
| Terms to extend | CONTEXT.md §Rule (~9–24), §Pass-through (~319), §Operating Mode (~501) |
| Why there is no `[http] enabled` | `crates/fah-config/src/schema/http.rs` doc on `HttpConfig` |

## Deliverables

1. `lol_html` pinned at workspace level and declared by `fah-http`.
2. `[html]` config section: `enabled` (runtime), `max_selector_cache` (boot).
3. A pure gating function: HTML active ⇔ mode serves HTTP ∧ `html.enabled`.
4. A live consumer for `html.enabled` in the proxy (so `runtime` is honest).
5. A no-op rewrite seam at the one response funnel both pipelines share.
6. Tests: seam does not copy or buffer; `enabled=false` ⇒ byte-identical.
7. Doc + diagram edits (after the yes).
8. Review file with the measurements.

## Step 1 — dependency pin

| File | Edit |
| ---- | ---- |
| `Cargo.toml` `[workspace.dependencies]` | `lol_html = { version = "3", default-features = false }` — 3.0.1 is current on crates.io (2026-07-29; p4-03 was written against it) — with a one-line TOML comment saying what it is for (TOML comments are allowed; Rust ones are not). Record the exact resolved version in the review file. |
| `crates/fah-http/Cargo.toml` `[dependencies]` | `lol_html = { workspace = true }` |

Checks after adding:

- `cargo tree -p fah-http -i lol_html` — confirm it reaches only `fah-http`
  and the binary.
- Measure and record: clean release build wall time before/after, and the
  static binary size (`cargo build --release` for the host; the container
  image figure lands in p4-05). Budget context: image ≤ 30 MB, 13.0 MiB
  measured (PERFORMANCE.md §Budgets).
- Nothing calls the crate yet. That is expected for this task; p4-03 is the
  first consumer.

## Step 2 — config section

New file `crates/fah-config/src/schema/html.rs`, wired in `schema/mod.rs`
(`mod html;` + `pub use html::HtmlConfig;`) and added to `Config` as
`pub html: HtmlConfig` next to `http`. `Config` is
`#[serde(deny_unknown_fields, default)]`, so the field must exist before any
TOML may name it.

```text
HtmlConfig
  enabled: bool             default false  runtime
  max_selector_cache: usize default 256    boot
```

Decisions, and why they differ from the task text where they do:

| Point | Decision |
| ----- | -------- |
| `enabled` default | `false`. Owner decision 2026-09-19: Phase 4 ships built and dormant (phase [CLAUDE.md](CLAUDE.md)). The *effective* state is `mode.serves_http() && enabled`; switching on is one runtime call, `POST /api/v1/config {"html": {"enabled": true}}`, no restart and no TOML edit, confirmed via `GET /api/v1/config`. The task file's "default follows operating mode" is superseded by this row. |
| `enabled` class | `runtime` — but only because Step 4 gives it a live consumer. Without that consumer it would be `boot`, and CONFIGURATION.md forbids relabelling. |
| `max_selector_cache` class | `boot`, not `runtime` as the task file says. The cache is sized once when the proxy is built, exactly like `[http] max_connections`. Promote it in p4-03 only if the cache gets a live resize path; say which you did in the review. |
| Why an `enabled` key when `[http]` deliberately has none | `[http]`'s switch is the operating mode: "does HTTP listen". HTML rewriting is a second decision inside a listening engine — rewrite or relay. Two decisions, two switches. Write this sentence into the section's TOML comment so the next reader does not "fix" it. |
| Size/time guards | None. Leave room in the struct order; p4-03/p4-04 add `max_concurrent_rewrites` when the memory argument exists. |

Validation in `crates/fah-config/src/lib.rs` `validate()`:

- `max_selector_cache` in `1..=65_536`. `validate_range` (~583) takes `u32`;
  the field is `usize`, so mirror the manual `http.max_connections` check at
  ~174 for the zero bound and add the upper bound beside it.
- **`enabled = true` while `engine.mode` does not serve HTTP is rejected**,
  key `html.enabled`, in every load path. Otherwise `GET /api/v1/config`
  reads "on" while nothing rewrites — the defect `HttpConfig`'s doc names
  ("two switches for one decision"). `ConfigStore::apply_patch` re-validates
  the patched `Config`, so the same rule refuses the API patch in `dns` mode;
  test both.

Add a `schema/html.rs` unit test that every key has a default (mirror
`every_bound_has_a_default` in `http.rs` ~102) and that `enabled` defaults to
`false` — the dormant-by-default decision, pinned so a later "helpful" flip
fails a test.

Env overrides: `crates/fah-config/src/env.rs` `apply_one` (~34) is an
explicit per-key `match` on the split path, not a generic serde map. Add two
arms — `["html", "enabled"]` via `coerce_bool` and
`["html", "max_selector_cache"]` via `coerce_usize` — and one test each,
next to the `FAH__RUNTIME__HTTP_RUNTIMES` tests in `lib.rs` (~1043).

API: `GET /api/v1/config` serialises `Config` whole, so `[html]` appears
without work. `POST /api/v1/config` decides `restart_required` from the
`BOOT_KEYS` list in `crates/fah-api/src/config_store.rs` (~37): a key is
runtime unless it, or its whole section, is listed. Do **not** list `html`
whole — add `html.max_selector_cache` on its own line, so `html.enabled`
stays runtime. The live push happens in `post_config` (`routes.rs` ~1466),
after `apply_patch`, where `set_default_refresh_hours` already pushes
`rules.refresh_hours_default`; call `HtmlGate::set_enabled` there (Step 4).
Add a `config_store.rs` test beside
`a_runtime_key_applies_live_without_requiring_a_restart` (~227): patching
`html.enabled` reports `restart_required: false`, patching
`html.max_selector_cache` reports `true`.

## Step 3 — gating function

In `fah-config` (it already owns `EngineMode::serves_http`):

```text
impl HtmlConfig {
    pub fn active_in(&self, mode: EngineMode) -> bool {
        mode.serves_http() && self.enabled
    }
}
```

Unit tests: `Dns` ⇒ false even with `enabled = true` (the struct method is
mode-agnostic; `validate()` is what refuses that combination at load time);
`DnsHttp` and `DnsHttpHttps` ⇒ follows `enabled`.

In `dns` mode the proxy is never constructed (`main.rs` ~427: the `Server` and
the `Proxy` closure exist only under `serves_http()`), so no HTML code path is
reachable by construction. Add an assertion to the existing mode tests in
`main.rs` (~1655) that pairs `active_in` with `serves_http` — that is the
"`engine.mode = "dns"` ⇒ no HTML code path" criterion, proven at the type
level rather than by a runtime probe.

## Step 4 — live consumer: `HtmlGate`

The gate is read by `fah-http` (both pipelines) and written by `fah-api`
(`POST /api/v1/config`). Siblings never import each other (hard rule 1), so
the type lives one layer down, in a new file `crates/fah-common/src/html.rs`,
exported as `fah_common::html::HtmlGate` and re-exported by `fah-http` for
its own callers. `crates/fastadhunter/tests/layering.rs` already ranks
`fah-common` at L1 (line ~11), so nothing there changes.

```text
pub struct HtmlGate {
    enabled: AtomicBool,
}
impl HtmlGate {
    pub fn new(enabled: bool) -> Self
    pub fn is_enabled(&self) -> bool      // Relaxed load
    pub fn set_enabled(&self, on: bool)   // API's runtime write
}
```

`crates/fah-http/src/html.rs` (p4-03 turns it into `html/`) holds only the
re-export and the seam function `plan_response` from Step 5.

Wiring:

| Where | Change |
| ----- | ------ |
| `Proxy` (`proxy.rs` ~236 struct, ~290–310 builders) | `with_html(gate: Arc<HtmlGate>)` builder; stored as `Option<Arc<HtmlGate>>` like `rules`/`policies`. `None` behaves as disabled. |
| `TlsProxy` / `Interception` (`intercept.rs` ~65) | Same builder so the intercepted leg sees the same gate. One gate instance, shared by `Arc`. |
| `main.rs` (~1115 proxy factory, ~1056 interception, ~1088 TlsProxy) | Build one `Arc<HtmlGate>` from `config.html.active_in(config.engine.mode)`; pass to both; hand the same `Arc` to the API. |
| `fah-api` `state.rs` | `AppState` (~24) **and** `AppStateBuilder` (~62) gain `html: Arc<HtmlGate>`; `main.rs` fills the builder. In `dns` mode no proxy exists, but the gate still does (always `false`, since `validate()` refuses `enabled = true` there) — one field, never an `Option`. |
| `fah-api` `post_config` (`routes.rs` ~1466) | After `apply_patch`: `state.html.set_enabled(config.html.active_in(config.engine.mode))`. Idempotent, so it runs after every apply, like `set_default_refresh_hours`. `restart_required` for the key is already `false` by Step 2. |

## Step 5 — the seam

Both pipelines funnel upstream responses through one function:
`proxy.rs` ~664 `to_client_response(response: Response<Incoming>) ->
Response<ProxyBody>`, called from `Proxy::handle` (~432) and
`TlsProxy::handle_intercepted` (`intercept.rs` ~341). That is the seam.

```text
pub(crate) enum RewritePlan { PassThrough }

pub(crate) fn plan_response(
    gate: Option<&HtmlGate>,
    parts: &http::response::Parts,
) -> RewritePlan            // always PassThrough in this task

pub(crate) fn to_client_response(
    response: Response<Incoming>,
    plan: RewritePlan,
) -> Response<ProxyBody>
```

Rules for the seam:

- `plan_response` runs **after** the head arrives and **before** the body is
  touched. In this task it evaluates `gate.is_enabled()` and returns
  `PassThrough` either way; p4-04 adds the other conditions and variant.
- `ProxyBody` stays `Either<Incoming, Full<Bytes>>`. Do **not** add a third
  variant now; p4-03 designs the rewritten body type and p4-04 adds it. Note
  in the review that `ProxyBody` will become a hand-written three-variant
  `enum` implementing `http_body::Body` (monomorphised match, no `Box<dyn>` —
  ARCHITECTURE.md §HTTP Pipeline "Transport-agnostic connections").
- Both call sites pass through the seam; there is no second funnel. If you
  find a response path that bypasses `to_client_response`, route it through
  rather than adding a second seam (`refuse()` and `block::response` are
  synthesized bodies, not upstream ones — they stay as they are).

## Step 6 — tests

| Test | Where | Proves |
| ---- | ----- | ------ |
| `seam_relays_the_upstream_body_by_reference` | `proxy.rs` tests, through the raw-TCP origin rig from `an_active_upstream_connection_is_reused_across_requests` (~883) | `hyper::body::Incoming` cannot be built in a test — hyper 1 has no channel body, it only comes out of a live connection — so the proof is the variant, not a pointer: with the gate enabled and a `text/html` head, `to_client_response` returns `Either::Left(_)`, the upstream `Incoming` itself with no wrapper. The type is the proof that no copy happens. |
| `disabled_gate_is_byte_identical` | `proxy.rs` tests, reuse the raw-TCP origin rig from `an_active_upstream_connection_is_reused_across_requests` (~883) | Origin serves an HTML body with `Content-Type: text/html; charset=utf-8` and `Content-Length`; through the proxy with `HtmlGate::new(false)` the status, `Content-Length`, `Content-Type` and body bytes equal the origin's. Only `Via` is added and hop-by-hop headers removed (already tested at ~712). Repeat once with the gate enabled — same result, since the plan is always `PassThrough` here. |
| `gate_flips_at_runtime` | `html.rs` tests | `set_enabled(false)` is observed by the next `is_enabled()`; two threads, no lock. |
| `active_in_follows_mode_then_flag` | `fah-config` `schema/html.rs` tests | Table over the three modes × two flag values. |
| Pass-through bench unchanged | `cargo bench -p fah-http --bench proxy` groups `http_pass_through` and `http_opaque_body`, A/B against a checkout of the pre-change commit — never criterion's stored baseline (docs/measurement-traps.md) | The seam costs nothing measurable on the fast path. |

## Step 7 — docs and diagram (list, then wait for the yes)

Prepare these as a list in chat after the gates are green. Do not edit until
each has a yes.

| File | Edit |
| ---- | ---- |
| ARCHITECTURE.md §HTTP Pipeline | Insert a stage between `Pass-through` and the client: `HTML Rewriting ── text/html responses with applicable cosmetic selectors, streamed through lol_html; everything else stays pass-through`. Add one bullet under the principles: rewriting never buffers a document, memory is O(chunk) (p4-03 proves it). |
| CONTEXT.md §Rule | Split the third kind: `inactive` no longer names cosmetic rules; add `page-applicable — acts on the document's elements by CSS selector (`##`, `#@#`). Answered by the HTML Rewriting stage from p4-02.` Keep `inactive` for `$client`-less unsupported patterns and extended cosmetics. |
| CONTEXT.md new terms | **Cosmetic Rule** — a rule addressing page elements by CSS selector; hides (`##`) or exempts (`#@#`), optionally scoped to domains. **HTML Rewriting** — the streaming stage that applies a client's effective cosmetic selectors to a `text/html` response; opt-in per policy, gated by `[html] enabled`, reaches plain HTTP and intercepted HTTPS only. |
| CONTEXT.md §Pass-through | Replace "Response bodies are **always** pass-through: Phase 4 would have added…" with: pass-through is the default for every body; the one exception is an HTML Rewriting candidate (p4-04), which is streamed, not buffered. |
| CONTEXT.md §Operating Mode | One sentence: `[html] enabled` is a second switch inside a mode that serves HTTP; `dns` mode carries no HTML path. |
| CONFIGURATION.md | New block after the HTTPS one: `# ─── HTML rewriting (Phase 4) ───` with `[html]` `enabled = false  # runtime — opt-in; switch on via POST /api/v1/config` and `max_selector_cache = 256  # boot`, each with the two-line reason in the block's comment style. In §Mutability classes add: `html.enabled` is runtime because the proxy reads it per HTML candidate from a shared atomic; the rest of `[html]` is boot. |
| docs/diagrams/architecture.html | Add the `HTML Rewriting (lol_html)` node after the HTTP pass-through node, same edge style. Check how the `.svg` files were produced (look for a generator note in the html); regenerate `architecture.svg` and `architecture-full.svg` the same way, or edit by hand keeping layout and say so in the review. |
| ROADMAP.md §Phase 4, README.md rows 4 + tech stack, docs/project-state.md Phase row | Replace the parked wording with "revived <date>, ADR-0009 superseded" — text supplied by the owner's revival decision. ADR-0009 itself gets a status line at the top (`Superseded <date> by <ADR-0010 or decision ref>`); ADR bodies are not rewritten. |
| RULE_ENGINE.md | **No edit here.** The `inactive — cosmetic (##, Phase 4)` bullet at ~57 stays true until p4-02 activates them. |

Acceptance grep after the docs land (the task's "docs consistent"):

```sh
rg -n -i "parked|would have|inactive until|parsed-only" ROADMAP.md README.md CONTEXT.md ARCHITECTURE.md CONFIGURATION.md docs/project-state.md
```

Every remaining hit must be either historical (ADR-0009's own text) or the
p4-02 hand-off wording ("activates in p4-02").

## Step 8 — gates, review file, stop

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
cargo bench -p fah-http --bench proxy      # A/B, see Step 6
```

Review file `docs/code-review/phase4/p4-01-html-scaffold-review.md` in the
[docs/code-review/CLAUDE.md](../../../docs/code-review/CLAUDE.md) structure.
Its Implementation Summary records: lol_html version, build-time and binary-
size deltas, the `boot` decision on `max_selector_cache`, where `HtmlGate`
lives and why, the pass-through A/B table (corpus, workload, device), and the
doc edits done or pending.

Then the only chat line:

`Task done. Report written to docs/code-review/phase4/p4-01-html-scaffold-review.md. Awaiting "start code review".`

## Out of scope — do not drift

- Any selector compilation or `RuleKind` change (p4-02).
- Any `lol_html` call, body type, cache or charset handling (p4-03).
- Any response condition beyond `is_enabled()`, any per-policy toggle, any
  event/stats/metrics field (p4-04).
- Budget rows in PERFORMANCE.md (p4-05).

## Hand-off facts for p4-02 … p4-04

- Seam: `to_client_response(response, plan)` in `proxy.rs`, fed by
  `plan_response` in `html.rs`; two callers, both listed in Step 5.
- Gate type: `fah_common::html::HtmlGate`, one `Arc` shared by `Proxy`,
  `TlsProxy` and `AppState`.
- Config: `Config.html: HtmlConfig { enabled, max_selector_cache }`;
  `HtmlConfig::active_in(mode)`; `validate()` refuses `enabled = true`
  outside an HTTP-serving mode.
- Runtime/boot split: `BOOT_KEYS` in `config_store.rs` lists `[html]` keys
  one by one, never the section — p4-04 adds `html.max_concurrent_rewrites`
  there.
- Env: `apply_one` in `env.rs` has one arm per `[html]` key; new keys need
  a new arm.
