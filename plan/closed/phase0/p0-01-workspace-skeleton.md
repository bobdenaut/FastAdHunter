# P0-01 — Workspace Skeleton

**Phase:** 0 · **Depends on:** — · **Model:** Opus

## Goal

A clean Cargo workspace with all 10 planned crates stubbed, building green
under the local quality gates.

## Context

ARCHITECTURE.md "Workspace Layout" + "Dependency Layering" dictate the crate
map. Empty crates now prevent dependency spaghetti later; declaring the
inter-crate dependencies up front makes the layering mechanical.

## Scope

- Workspace root `Cargo.toml` (resolver 2, `workspace.dependencies`,
  workspace lints: clippy `-D warnings`).
- Crates under `crates/`: `fah-common`, `fah-logging`, `fah-config`,
  `fah-model`, `fah-rules`, `fah-dns`, `fah-api`, `fah-metrics`, `fah-stats`,
  `fastadhunter` (bin). Each `lib.rs`/`main.rs` holds only a doc-comment naming
  its ARCHITECTURE.md responsibility.
- Declare inter-crate dependencies exactly per the L1–L4 layering (empty crates
  may still depend on their lower layers so violations fail at `cargo build`).
- `rust-toolchain.toml` (stable, pinned), `rustfmt.toml` (default profile),
  `.gitignore` for Rust.

## Acceptance criteria

- `cargo build --workspace` and `cargo test --workspace` green.
- `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` green.
- Dependency graph matches ARCHITECTURE.md layering: no upward edge, no
  sibling edge (verify with `cargo tree --workspace --invert` spot checks).

## Out of scope

Any real logic, DNS, API, Docker, or dependency on external crates beyond what
stubs need (keep third-party deps at zero here).

## Suggested prompt

> Read CLAUDE.md (root), ARCHITECTURE.md §Workspace Layout + §Dependency
> Layering, and plan/wip/phase0/p0-01-workspace-skeleton.md. Create the
> workspace exactly as specified. Keep every crate empty except doc-comments
> mapping it to its architecture responsibility.
