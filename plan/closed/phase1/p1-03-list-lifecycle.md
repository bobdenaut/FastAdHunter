# P1-03 — List Lifecycle

**Phase:** 1 · **Depends on:** p1-02 · **Model:** Sonnet

## Goal

Rule lists flow: source → parse → validate → compile → atomic swap, with
`/data` caching and failure resilience.

## Context

RULE_ENGINE.md §List lifecycle + §Sources; ARCHITECTURE.md atomic-swap
principle. The hot path never locks; a failed refresh never degrades
protection.

## Scope

- Sources: remote URLs (reqwest/hyper via rustls), local files on `/data`,
  inline user rules.
- Refresh scheduler: per-list interval (default 24h from config), jittered;
  manual trigger hook (API calls it in p1-09).
- Atomic swap: `arc-swap` (or equivalent) of the compiled ruleset; in-flight
  queries finish on the old set.
- `/data` raw-copy cache; boot compiles from cache without network; async
  refresh after startup.
- Failure policy: download/validation failure → keep previous set, log,
  expose `last_status` per list.
- Default list (OISD basic) enabled on first run per CONFIGURATION.md.
- Tests: swap under concurrent lookups (loom or stress test), boot-from-cache,
  failure-keeps-previous.

## Acceptance criteria

- No lock on the lookup path (reads follow the current ruleset pointer).
- Kill-the-network test: refresh fails, verdicts unchanged, status surfaced.
- Gates green.

## Out of scope

API endpoints for lists (p1-09) — expose an internal handle they will call.

## Suggested prompt

> Read RULE_ENGINE.md §List lifecycle, CONFIGURATION.md §[rules], and
> plan/wip/phase1/p1-03-list-lifecycle.md. Implement sources, scheduler,
> atomic swap and /data caching with the failure-resilience tests.

## Completion note

**Design:** `fah_rules::ListManager` (`crates/fah-rules/src/lifecycle/`) —
sources (`source.rs`: `http(s)://` -> `reqwest` fetch; anything else -> local
file under `/data`), a `/data` raw-copy cache (`cache.rs`: atomic
write-tmp-then-rename, mirroring `fah-config`'s pattern independently), and
the manager itself (`mod.rs`). One `arc_swap::ArcSwap<Matcher>` is the hot
path's only touchpoint — `ListManager::matcher()` is an atomic-refcount-bump
read, never a lock. Every other operation (fetch, parse, recompile, cache
write) is serialized behind an internal `tokio::sync::Mutex` so two
concurrent refreshes (a scheduled tick racing a manual trigger) can't
overwrite each other's newer result with stale data — network fetch itself
runs outside that lock so one slow list never blocks another's refresh.

Because the compiled matcher covers all lists combined (p1-02), any single
list's refresh reparses every list's last known-good raw text and rebuilds
one combined `Matcher` before the atomic swap — matches RULE_ENGINE.md's
lifecycle diagram (one "compile new ruleset" step), runs off the hot path.
Only compiled-matcher bytes are retained long-term; raw text is kept only for
recompilation, not the intermediate `ParsedRuleList`s (p1-02's memory design).

Failure policy: a fetch/read I/O error is the only failure mode (parsing
never fails — RULE_ENGINE.md: bad lines are `parse_errors`, never a
rejection). On failure the previous compiled set keeps serving untouched;
`ListManager::status()` surfaces `RefreshResult::Failed` per list.

User rules (`set_user_rules`) are a synthetic list (`user-rules`, not in
`[[rules.lists]]`) that goes through the same cache/compile/swap path.
Scheduler: one `tokio::spawn` per enabled list, deterministic FNV-1a jitter
(dependency-free, same technique as `matcher.rs`/bench/property tests) before
the first tick so many lists on the same interval don't all refresh at once.

**Tests (24 new, all green):** boot-with-no-cache (empty matcher, no
network), boot-from-`/data`-cache (no network touched), successful refresh
(compiles + caches + swaps), **kill-the-network** (bind-then-drop a local TCP
listener for a guaranteed connection-refused — no real internet needed;
verdicts unchanged, failure surfaced in status), unknown-list-id error,
disabled list excluded from the compiled matcher, local-file source, user
rules, scheduler spawns one task per enabled list, jitter determinism, and an
**8-reader stress test** hammering `lookup` on real OS threads against 200
concurrent `set_user_rules` swaps — no panics, no torn state (loom isn't
available offline; this is a real-thread stress test instead). Remote fetch
tests use a hand-rolled HTTP/1.1 mock over `tokio::net::TcpListener`
(loopback only) since no mock-server crate was available in the offline
registry cache.

**Dependencies added:** `arc-swap` (workspace), `reqwest` (workspace,
`default-features = false, features = ["rustls"]` — no native-tls/openssl,
consistent with SECURITY.md's rustls-only rule). Both resolved from the
already-warmed offline registry cache; `cargo check`/`test` ran with
`--offline`.

**Known risk for p1-11:** `reqwest`'s `rustls` feature pulls in
`aws-lc-rs` (via `rustls`'s default crypto provider), which needs a C
compiler + cmake to build `aws-lc-sys`. Compiled clean on this dev machine;
not yet verified under the Alpine/musl Docker build
(`rust:1.96.0-alpine` + `apk add musl-dev` only — no `cmake`/`perl` installed
yet). If the Docker build fails on `aws-lc-sys`, either add
`cmake perl build-base` to the Dockerfile's `apk add`, or switch to
`reqwest`'s `rustls-no-provider` feature + `rustls::crypto::ring::default_provider()`
(no cmake/perl needed). Flagging now so p1-11 isn't surprised.

**Gates:** fmt/clippy/test all green on the full workspace.

**Post-completion review:** chief-architect review in
[docs/code-review/p1-03-review.md](../../../docs/code-review/p1-03-review.md)
found 10 issues; 1–9 fixed same day (10 deferred to p1-09). Where this note
conflicts with the review's "Fixes applied" section, the review is current:
fetches are size-capped at 64 MiB (streamed, never buffered past the cap);
the first scheduled refresh lands within 60 s of startup, not a random point
in the 24 h interval; the `/data` cache — not an in-memory map — is the
source of truth for raw text, re-read per recompile (only a failed cache
write keeps text in memory, in `pending_cache`); the parse+build runs on
`spawn_blocking`; each list has its own refresh mutex serializing
fetch+commit; the refresh interval is floored at 1 h; disabled lists refuse
refresh; every configured list starts with a visible `NeverAttempted`
status. fah-rules unit tests 65 → 70; gates green after fixes.
