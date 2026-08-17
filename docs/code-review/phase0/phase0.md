# Code Review — Phase 0 (Workspace Skeleton)

**Commit:** `ea597b2` — *feat(phase0): workspace skeleton, model types, config
loading, binary bootstrap, docker image*
**Reviewer:** chief architect / code review
**Date:** 2026-07-17
**Scope:** p0-01 … p0-06 (all of Phase 0)

## Verdict

**APPROVED.** Phase 0 meets its Definition of Done. All local quality gates
pass and the code is clean, well-documented, and faithful to the specs. The
findings below are hardening notes and deferred work — none block Phase 1.

### Gate results (run during review)

| Gate | Result |
| ---- | ------ |
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace` | **38 passed, 0 failed** (config 18, model 8, logging 4, bin 4, healthcheck 2, common 2) |
| `cargo run -- --healthcheck` | exits 0, generates + validates config |

Definition of Done confirmed: workspace green, healthcheck exits 0, Docker image
is distroless/nonroot/static-musl. No DNS/API/rules code exists — correct for
this phase.

## What's good

- **Layering is respected.** Dependency edges point strictly downward
  (L4 → L3 → L2 → L1); no sibling imports. Cargo manifests are the single place
  edges are declared.
- **Defaults match `CONFIGURATION.md` verbatim**, and the match is *enforced by
  test* (`parses_full_reference_toml_verbatim`, `defaults_match_configuration_md_sample`).
  Spec drift will fail the build.
- **`deny_unknown_fields` on every struct** plus a dedicated per-key env layer:
  typos in TOML *and* in `FAH__` env vars fail loudly and name the offending
  key. Tests cover both paths.
- **Env-override module is pure.** Only `collect_env_pairs()` touches
  `std::env`; all logic operates on `&[(String,String)]`, so tests never mutate
  process env. Good testability seam.
- **Unimplemented enum variants are deliberately absent** (`BlockingMode`,
  `UpstreamStrategy`): an unsupported `mode = "nxdomain"` fails at load instead
  of silently degrading. Documented in-code.
- **Docker is thoughtful:** distroless `:nonroot`, static musl, `--locked`
  build, `EXPOSE` for both DNS + API, and the `/seed/config` + `/seed/data`
  trick so a fresh named volume inherits nonroot ownership (first-boot config
  write would otherwise fail). Multi-arch rationale documented in the header.
- **`_logging` handle is bound, not dropped** (`let _logging =`), so the
  subscriber lives for the process. Easy mistake avoided.
- **Doc discipline:** every module/type cites the spec or ADR it implements.

## Findings

Severity: 🔴 address before it bites · 🟡 hardening · 🟢 nit / deferred.

### 🟡 F1 — `--healthcheck` has a disk write side-effect

[main.rs:64](../../../crates/fastadhunter/src/main.rs#L64) runs `Config::load()`
*before* the healthcheck branch, and `load()` writes a default config file on
first boot ([lib.rs:30-38](../../../crates/fah-config/src/lib.rs#L30-L38)). So
`--healthcheck` **mutates the filesystem** — the test
`healthcheck_exits_zero_on_fresh_tempdir_config` even asserts the file appears.

A health probe should observe, not mutate. In the container the entrypoint boots
first so this is masked, but: (a) a healthcheck on a read-only or unwritable
`/config` fails for the wrong reason, and (b) a probe silently creating the very
file whose absence signals a problem hides that problem.
**Recommend:** a read-only load path for healthcheck (load-or-default without the
first-boot write), or gate the write behind the normal boot path only.

### 🟡 F2 — Layering is enforced by convention only

The commit says "layering enforced," but enforcement is manual curation of each
`Cargo.toml`. There is no `deny.toml` / cargo-deny and no guard test, and the
project has no CI. A future edit adding a sibling dependency (e.g. `fah-dns`
importing `fah-api`) would compile clean.
**Recommend:** a cheap workspace-level guard — a `cargo-deny` bans config or a
small test that parses each crate manifest and asserts the allowed-edge set.
Makes the hard rule executable rather than aspirational.

### 🟡 F3 — First-boot config write is non-atomic and assumes the parent dir exists

[lib.rs:33](../../../crates/fah-config/src/lib.rs#L33) does `fs::write(path, …)`
straight to the final path. Two concerns: a crash or concurrent boot mid-write
leaves a truncated TOML; and if the parent directory is missing the write errors
out. The container seeds `/config`, so it's fine there — but `--config
/some/new/dir/x.toml` on bare metal fails.
**Recommend:** write to a temp file + atomic rename, and/or `create_dir_all` on
the parent with a clear error. Low effort, removes a sharp edge.

### 🟢 F4 — Unused dependencies across the skeleton

`fastadhunter` depends on all nine crates but uses only `fah-config` +
`fah-logging`; the L3 stubs (`fah-dns/api/stats/metrics`) declare `fah-rules` +
`fah-model` but their `lib.rs` is a one-line stub. Intentional scaffolding, but
nothing flags an accidental removal, and `fah-stats`/`fah-metrics` → `fah-rules`
may never be needed (stats consume `QueryEvent`/`Verdict` from `fah-model`, not
rules). **Recommend:** either trim edges until the consuming code lands, or leave
a note; revisit when each crate is fleshed out so unused deps don't calcify.

### 🟢 F5 — `fah_model::OperatingMode` is dead until Phase 1 and must be hand-synced with `EngineMode`

`OperatingMode` (fah-model) and `EngineMode` (fah-config) are byte-for-byte
parallel enums. Duplication is *correct* — L1 siblings can't cross-import
(hard rule) — and it's documented at
[engine.rs:27-29](../../../crates/fah-config/src/schema/engine.rs#L27-L29). But
`OperatingMode` currently has zero consumers, and the two enums can silently
drift when a fourth mode is added. **Recommend:** when the binary wires them in
Phase 1, add a round-trip test (`EngineMode` ↔ `OperatingMode`) so drift fails
the build.

### 🟢 F6 — CLI has no `--help` and no usage text

`--help` hits the catch-all and prints `unknown argument: --help` with a failure
exit ([main.rs:39](../../../crates/fastadhunter/src/main.rs#L39)). Minor UX; a
one-line usage string on error and on `--help` would be friendlier. Non-blocking.

### 🟢 F7 — `validate()` is minimal (deferred, by design)

[lib.rs:59](../../../crates/fah-config/src/lib.rs#L59) checks only bind-address
parseability and `min_ttl ≤ max_ttl`. Not yet checked: non-empty
`upstreams.servers`, `DoT`/`DoH` requiring a `hostname` for cert verification,
port ≠ 0. Appropriate to defer to the phase that consumes each field — listed
here so it isn't forgotten.

### 🟢 F8 — `default_true` duplicated in four schema files

Trivial helper copy-pasted in `cache`, `api`, `query_log`, `rules`. Could live
in one `schema` helper. Cosmetic.

## Deferred to later phases (not defects)

- `QueryType::Other(String)` carries an allocation on a type that will sit on the
  hot path; revisit the hot-path mapping in `fah-dns` (Phase 1) against
  `PERFORMANCE.md` (no allocations on hot path). Model purity here is fine.
- Phase-0 healthcheck is process-level; `TODO(phase1)` already flags the switch
  to probing `GET /health` once `fah-api` exists.
- On-device RB5009/arm64 validation is explicitly Phase 1 (per the plan's Key
  Risks).

## Recommendation

Ship Phase 0. Fold **F1** and **F3** into the start of Phase 1 (both touch
config/boot code that Phase 1 will edit anyway), and add the **F2** layering
guard opportunistically. Everything else is deferred or cosmetic.

---

## Fixes applied (2026-07-17)

All eight findings were addressed in the same pass. Gates after the changes:

| Gate | Result |
| ---- | ------ |
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace` | **48 passed, 0 failed** (was 38; +10) |
| `cargo build --locked -p fastadhunter` | ok (Cargo.lock consistent) |

Test count moved 38 → 48: fah-config 18 → 24 (validation + read-only load +
atomic-write), fastadhunter unit 4 → 5 (`--help`), and two new integration
suites (`layering`, `mode_sync`).

### F1 — healthcheck no longer writes to disk ✅

Added [`Config::load_readonly`](../../../crates/fah-config/src/lib.rs) (missing file
→ in-memory defaults, no write). `--healthcheck` now calls it and reports a
load/validation failure on stderr with a non-zero exit. The integration test was
renamed to `healthcheck_exits_zero_without_writing_config` and now asserts the
config file is **not** created.

### F2 — layering is now enforced by a test ✅

New [`crates/fastadhunter/tests/layering.rs`](../../../crates/fastadhunter/tests/layering.rs)
parses every crate manifest and asserts each `fah-*` dependency (including
dev-deps) points to a strictly lower layer, and that all 10 crates are covered.
A stray sibling import now fails `cargo test`. `toml` was added as a dev-dep of
the binary for the check.

### F3 — first-boot write is atomic and creates parents ✅

`write_atomic` in [fah-config/src/lib.rs](../../../crates/fah-config/src/lib.rs)
does `create_dir_all(parent)` then writes to a `.tmp.<pid>` sibling and renames
into place. Covered by `first_boot_creates_missing_parent_directories`.

### F4 — unused dependencies trimmed ✅

Dropped every unused internal edge: the four L3 stubs and `fah-rules` now have an
empty `[dependencies]`, and `fastadhunter` keeps only `fah-config` +
`fah-logging` (plus `fah-model`/`toml` as dev-deps for the guard tests). Each
trimmed manifest carries a comment; ARCHITECTURE.md remains the record of
*intended* edges, and the F2 test keeps re-added edges honest. Cargo.lock updated
(internal edges only, no external crate changes).

### F5 — mode-enum drift is now caught at compile time ✅

New [`crates/fastadhunter/tests/mode_sync.rs`](../../../crates/fastadhunter/tests/mode_sync.rs)
uses exhaustive matches over `EngineMode` and `OperatingMode`, so adding a
variant to one enum but not the other stops compiling; assertions verify the two
agree on every canonical string and reject the same unknown input.

### F6 — `--help`/`-h` and usage text ✅

Added a `USAGE` string; `--help`/`-h` print it and exit 0, and argument errors
now print usage to stderr. Covered by `parses_help_flags`.

### F7 — validation extended ✅

`validate()` now also rejects: a zero `dns.listen.port`/`api.port`, an empty
`dns.upstreams.servers`, and a `dot`/`doh` upstream missing a `hostname`
(required for certificate verification). Four new fah-config tests cover these.

### F8 — `default_true` de-duplicated ✅

Single `pub(crate) fn default_true` lives in
[schema/mod.rs](../../../crates/fah-config/src/schema/mod.rs); `cache`, `api`,
`query_log`, and `rules` import it and dropped their local copies.

### Deferred items — unchanged

The "Deferred to later phases" list above (`QueryType::Other` hot-path
allocation, `GET /health` probe, on-device arm64 validation) is intentionally
left for the phases that own that code.
