# CLAUDE.md

Guidance for AI agents working in this repository.

## Working language

The user may write in Romanian (or English). Agents **always respond in
English**, regardless of the language the user wrote in.

Ignore IDE/markdown-lint diagnostics (MD060, MD028, etc.) silently — do not
narrate or explain them in chat.

## What this is

FastAdHunter — network-wide ad blocker in Rust. DNS filtering first (Phase 1),
HTTP/HTTPS/HTML later. API-first, Docker-native, ARM64-first (MikroTik RB5009,
RouterOS container). Performance is the primary feature.

## Read before changing anything

| Question | Document |
| -------- | -------- |
| What does this term mean? | [CONTEXT.md](CONTEXT.md) — the vocabulary is binding |
| How is it structured? | [ARCHITECTURE.md](ARCHITECTURE.md) |
| What ships when? | [ROADMAP.md](ROADMAP.md) |
| Endpoint shapes? | [API.md](API.md) |
| Config options? | [CONFIGURATION.md](CONFIGURATION.md) |
| Rule formats / verdicts? | [RULE_ENGINE.md](RULE_ENGINE.md) |
| Perf rules + budgets? | [PERFORMANCE.md](PERFORMANCE.md) |
| Auth / TLS / hardening? | [SECURITY.md](SECURITY.md) |
| Conventions? | [CONTRIBUTING.md](CONTRIBUTING.md) |
| Why is X this way? | [docs/decisions/](docs/decisions/) (ADRs) |

Docs are the source of truth and were approved before any code. A change that
contradicts them needs the doc updated in the same change — or an ADR if the
decision is being reversed.

## Reading protocol — do NOT read all docs

Reading everything costs ~13k tokens; a task needs 2–3k. This file is always
loaded — beyond it, read only what the task touches:

| Task touches | Read |
| ------------ | ---- |
| `fah-rules` (parsers, matchers, verdicts) | RULE_ENGINE.md + CONTEXT.md |
| `fah-dns` (pipeline, cache, upstreams) | ARCHITECTURE.md (+ ADR-0001) |
| `fah-http` (proxy, pass-through, interception) | ARCHITECTURE.md §HTTP Pipeline + CONTEXT.md |
| `fah-api` (endpoints, auth) | API.md (+ SECURITY.md if auth/TLS) |
| `fah-config` / config options | CONFIGURATION.md |
| `fah-stats` / `fah-metrics` | ARCHITECTURE.md (+ ADR-0002) |
| Docker / deployment | SECURITY.md + CONFIGURATION.md |
| Perf-sensitive change / benches | PERFORMANCE.md |
| Naming a new concept | CONTEXT.md |
| Planning a phase | ROADMAP.md |

**Skip README.md** — human/marketing-facing; this file supersedes it for
agents. Read a doc section-by-section (Grep for the heading) when only one
section is needed.

## Hard rules

1. **Dependency layering** (ARCHITECTURE.md): L4 `fastadhunter` → L3 `fah-dns`,
   `fah-http`, `fah-api`, `fah-stats`, `fah-metrics` → L2 `fah-rules` → L1
   `fah-model`, `fah-config`, `fah-common`, `fah-logging`. Dependencies point
   downward only. Siblings never import each other — the binary wires them via
   channels. `crates/fastadhunter/tests/layering.rs` enforces this.
2. **`fah-model` purity**: data types and trivial traits only. No business
   logic, no I/O, no parsers, no cache.
3. **Hot path**: no locks, no allocations, no regex. Ruleset/config changes via
   atomic swap. Rule Engine runs BEFORE the cache; the cache never stores
   verdicts.
4. **Bounded everything**: memory must not grow with traffic or uptime.
5. **No hand-rolled crypto**: rustls, rcgen, x509-parser only.
6. **Use CONTEXT.md vocabulary** in code, comments, APIs. New/changed terms
   update CONTEXT.md in the same change.
7. **After every done task**: DO NOT post to user in chat-screen what was implemented, 
   just create a review file unde docs/code-review/ (e.g. docs/code-review/p1.5-03-review.md) 
   and announce that the task is DONE and the new filename.

## Quality gates (local — there is no CI)

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench   # when a hot path is touched; >10% regression needs justification
```

Conventional Commits (`feat:`, `fix:`, `perf:`, …), trunk-based, short-lived
branches. `unsafe` requires a `// SAFETY:` comment.

## Layout

```text
crates/        # 11 crates (see ARCHITECTURE.md for responsibilities)
tests/         # workspace integration tests
benches/       # criterion benches vs PERFORMANCE.md budgets
docs/          # images/, diagrams/, decisions/ (ADRs)
dashboard/     # empty until the dashboard phase — do not scaffold
plan/          # task orchestration — open/ wip/ closed/ phases
```

## Task workflow

Implementation work is driven by [plan/CLAUDE.md](plan/CLAUDE.md): phases move
`open` → `wip` → `closed`; tasks execute in `NN` order; status lives in each
phase's `CLAUDE.md` table. When asked to "work on the plan", start there.

## Environment notes

- Target hardware: RB5009 (4×ARMv8, nominally 1.4 GHz, 1 GB RAM shared with
  RouterOS) — budgets in PERFORMANCE.md assume it. **Treat the nominal clock as
  an architectural specification, not the observed operating frequency:** during
  measurement a single busy core stayed at 350–700 MHz with no boost to nominal
  observed. Convert dev-box figures with the measured **~9× x86 → RB5009
  factor** instead (PERFORMANCE.md §Budgets).
- Container: distroless/static, musl static binary, volumes `/config` + `/data`.
- Tech stack is fixed: Tokio, Hyper/Axum, Hickory, rustls, lol_html (Phase 4).
