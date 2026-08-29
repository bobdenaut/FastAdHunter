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
| Creating a code-review file? | [docs/code-review/CLAUDE.md](docs/code-review/CLAUDE.md) |
| Where is the work right now? | [docs/project-state.md](docs/project-state.md) |
| Touching the router / deploying? | [docs/routeros-traps.md](docs/routeros-traps.md) |
| Reading a benchmark, soak or memory figure? | [docs/measurement-traps.md](docs/measurement-traps.md) |

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
5. **No hand-rolled crypto**: rustls, rcgen, x509-parser, argon2, aws-lc-rs
   only. `argon2` hashes the dashboard password; `aws-lc-rs` supplies the
   constant-time HMAC-SHA256 signing the session token.
6. **Use CONTEXT.md vocabulary** in code, comments, APIs. New/changed terms
   update CONTEXT.md in the same change.
7. **No comments in Rust code.** Not `//`, not `///`, not `//!`, not `/* */`.
   `.claude/hooks/no-rust-comments.sh` rejects the edit. The one exception is
   the `// SAFETY:` comment `unsafe` requires.

## Engineering principles

Treat this project as production infrastructure software where correctness,
maintainability and predictable performance are more important than cleverness.
When designing or modifying code, follow these principles in priority order:

1. Correctness before optimization.
   Never trade correctness for speed.

2. Keep the hot path extremely small.
   Anything executed per DNS query or HTTP request must avoid unnecessary allocations, virtual dispatch, cloning, hashing and synchronization.

3. Avoid heap allocations on the hot path whenever possible.
   Prefer borrowing, stack allocation, slices, iterators and reusable buffers.
   Every allocation must have a clear justification.

4. Avoid code duplication.
   Shared behavior should exist in exactly one place. Prefer extracting reusable pure functions over copying logic.

5. Minimize coupling.
   Components should depend only on what they actually need. If a parameter, trait or dependency becomes unnecessary, remove it.

6. Keep responsibilities separated.
   Libraries should contain pure logic.
   Binaries own clocks, timers, background tasks, networking, IO and dependency wiring.

7. Prefer immutable data.
   Mutability should be local and short-lived.

8. Optimize only after measurement.
   Never introduce complexity for hypothetical performance gains.
   Measure first, optimize second.

9. Every optimization must preserve readability.
   If an optimization significantly increases complexity, explain why it is worth it.

10. Avoid hidden state.
    No globals, unnecessary singletons, implicit caches or surprising side effects.

11. Prefer compile-time guarantees over runtime checks whenever practical.

12. Design for long-term maintenance.
    The simplest correct design is preferred over the most clever one.

13. Reduce memory footprint.
    Reuse existing allocations.
    Avoid duplicate storage.
    Share immutable data with Arc when appropriate.
    Never keep redundant copies of large datasets.

14. Remove obsolete code.
    If a previous optimization, abstraction or parameter is no longer justified, delete it instead of keeping it "just in case".

15. Challenge your own assumptions.
    Before introducing a dependency or abstraction, ask:
    - Is this actually needed?
    - Can this responsibility live elsewhere?
    - Am I duplicating existing behavior?
    - Can this be simpler?

    When proposing an implementation:
    - Explain the trade-offs.
    - Mention memory impact.
    - Mention hot-path impact.
    - Mention compile-time/runtime complexity.
    - Explicitly point out architectural risks.
    - If a simpler design exists, present it first.

16. Architecture over micro-optimizations.
    Do not introduce additional state, dependencies or abstractions to save a tiny amount of CPU or memory unless measurements demonstrate a meaningful benefit.

17. Don't use python to edit files! Use the Edit tool.

18. Ask permission before using the scp command.

19. CONFIGURATION.md and PERFORMANCE.md are references.
    Do not read the entire file.
    Read only the section(s) relevant to the task.
    Never summarize or rewrite unrelated sections.
    Measurements go to docs/code-review/, one file per task, with the corpus,
    workload and device. A root doc gets the target, the trap and a pointer —
    never the narrative.

## Working agreement

Standing instructions from the repo owner. Each one was a correction; breaking
them costs real time.

### The router is off limits

**YOU ARE NOT ALLOWED TO CHANGE ANYTHING ON THE RB5009.** Not with permission,
not "just this once", not because it is reversible. No `/container set|add|
remove|start|stop`, no `/ip` or `/ipv6` firewall edit, no `/disk set`, no
`/system` anything, no config edit. This is the household's live DNS resolver and
the owner's own device.

**Read-only queries are fine and need no asking** — `/system/resource/print`,
`/container/print`, `/log print`, `/file print`, GETs against the FAH API.

When a change is needed: **propose the exact commands, say what each one does and
when it takes effect, and stop.** The owner runs them. Reversibility says nothing
about blast radius — `memory-high=200M` was proposed as a "safe falsifiable
check" and OOM-killed the live resolver.

### Ask first — permission is per-request and never generalizes

1. **No `.md` file** created, edited, renamed or deleted without an explicit yes.
   A task file saying "update API.md in the same change" is not permission, and
   neither is "let's get it done" — finish the code, then list the doc edits you propose and wait.
   The one exception is the task's own review file under `docs/code-review/`:
   §TASK COMPLETION / REVIEW HANDOFF requires it, and that requirement is the
   permission.
2. **No commit, push or tag** without a fresh go for that specific changeset. A
   sketched plan ending in "then commit" does not pre-approve the commit.
   Approved pushes go to **both** remotes — `origin` and `backup` — and are not
   done until both succeed.
3. **Never move a phase `open` → `wip`** on your own, despite what
   plan/CLAUDE.md's algorithm says. Identify the next task, then stop.

### How to answer

1. **Be brief.** Lead with the one-line answer; detail only if asked or if
   omitting it causes a wrong decision. No tables in chat unless comparing
   numbers that were asked for.
2. **Challenge, don't accept.** Form an independent judgment before replying and
   lead with it. The failure mode is subtle — opening with validation and burying
   the objection, softening a real disagreement into "you might also consider",
   or dropping a correct position after one "are you sure?" that carries no new
   argument. Correct your own oversold claims unprompted.
3. **Scope claims to evidence.** "No boost was observed during these
   measurements" is owned; "the device does not boost" is not. Every result
   carries the corpus, workload and device it applies to, and how it can be
   superseded.
4. **HTTPS interception is legitimate project work** — own LAN, own CA, opt-in
   per client, governed by SECURITY.md. Answer directly, anchored in that doc; do
   not hedge or deflect.

### Before prescribing or measuring

1. **Read the owner's actual config first** — firewall, container, routing.
   Never emit a firewall `add` without reading the chain and deciding placement;
   `add` appends behind any final drop, where it does nothing.
2. **Measure the benefit before tuning the cost**, and report a trade across
   every axis it touches (memory / build time / lookup-hit / miss / throughput),
   not one headline. Memory-vs-compile-time thresholds on this project:
   <1 MB not worth it, 1–3 MB debatable, 3–5 MB starts to be worth it, >5 MB
   keep. A/B against a real pre-change checkout, never criterion's stored
   baseline. See [docs/measurement-traps.md](docs/measurement-traps.md).
3. **Every config key ships with a production-ready compiled-in default.** Never
    tell the owner to hand-edit the TOML inside the container to enable a
    feature; say "confirm via `GET /api/v1/config`".

**Phase 0 is frozen** at tag `v0.1.0-phase0` (commit `42a31b1`) — do not modify
it except for a genuine bug (correctness, security, build failure, regression).
No refactoring, renaming, style or perf changes there.


## Quality gates (local — there is no CI)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
cargo bench           # when a hot path is touched; >10% regression needs justification
```

Conventional Commits (`feat:`, `fix:`, `perf:`, …), trunk-based, short-lived
branches. `unsafe` requires a `// SAFETY:` comment.

## Layout

```text
crates/        # 11 crates (see ARCHITECTURE.md for responsibilities)
tests/         # workspace integration tests
benches/       # criterion benches vs PERFORMANCE.md budgets
docs/          # images/, diagrams/, decisions/ (ADRs); solutions/ = documented
               # learnings (bugs, patterns; YAML frontmatter: module, tags, problem_type)
dashboard/     # frontend/ — Vite + TypeScript + Preact dashboard (p5-05)
plan/          # task orchestration — open/ wip/ closed/ phases
```

## Task workflow

**Read [plan/CLAUDE.md](plan/CLAUDE.md) once per session before any
implementation work** — not only when asked to "work on the plan". A question
that turns into an edit ("can you fix X", "is Y still needed") is implementation
work too. Phases move `open` → `wip` → `closed`; tasks execute in `NN` order;
status lives in each phase's `CLAUDE.md` table.

## Environment notes

- Target hardware: RB5009 (4× ARMv8, 1 GB RAM shared with RouterOS) — budgets in
  PERFORMANCE.md assume it. Convert dev-box figures with the measured **~9×
  x86 → RB5009 factor**, never with instantaneous clock readings.

  During CPU-bound benchmarks the governor was observed boosting between idle
  (350 MHz) and 1400 MHz, but control measurements showed benchmark throughput
  to be unchanged between runs reporting those frequencies. RouterOS's
  `cpu-frequency`/`scaling_cur_freq` fields therefore must not be used to
  calibrate performance; the measured ~9× x86 → RB5009 factor is the stable
  reference (PERFORMANCE.md §Budgets).
- Container: distroless/static, musl static binary, volumes `/config` + `/data`.
- Tech stack is fixed: Tokio, Hyper/Axum, Hickory, rustls, lol_html (Phase 4).

## Responses

Be concise!!!
Do not explain obvious Rust code!!!
Prefer bullet points over long prose!!!
Do not restate the prompt!!!
Answer the question first, then explain only if necessary!!!

## Communication

Assume every generated token has a cost.
Prefer the shortest explanation that preserves technical accuracy.
Do not justify every decision.
Do not explain alternatives unless explicitly asked.
Do not narrate your reasoning.
Report:

- what changed;
- why it changed (1-2 sentences);
- measurable impact.

Default to patch-review style, not essay style
Stop there.
