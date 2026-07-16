# Phase 0 — Workspace Skeleton

**Objective:** a compiling Cargo workspace with all 10 crates stubbed, the pure
domain types in place, config loading working, a binary that boots and answers
`--healthcheck`, and a distroless Docker image that runs it. Nothing resolves
DNS yet; nothing filters yet.

**Why this order:** foundations bottom-up along the dependency layers (L1 → L4):
types before the crates that use them, config before the binary that loads it,
binary before the image that ships it. Every task leaves the workspace green.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | STATUS |
|---|-----------|---------|--------|
| 1 | `p0-01-workspace-skeleton.md` | Workspace + 10 stub crates, layering enforced, gates green | WAITING |
| 2 | `p0-02-model-types.md` | `fah-model`: Query, Verdict, Client, QueryEvent compile + tests | WAITING |
| 3 | `p0-03-common-logging.md` | `fah-common` error types + `fah-logging` tracing init | WAITING |
| 4 | `p0-04-config-loading.md` | `fah-config`: TOML + defaults + env precedence, typed, tested | WAITING |
| 5 | `p0-05-binary-bootstrap.md` | `fastadhunter` boots Tokio, loads config, `--healthcheck` works | WAITING |
| 6 | `p0-06-docker-image.md` | Static musl build in distroless image, arm64 + amd64 | WAITING |

**Definition of done:** `cargo test --workspace` green; `cargo run -- --healthcheck`
exits 0; `docker build` produces an image that starts, logs its config source,
and passes its healthcheck. No DNS, no API, no rules code exists yet.

**Key risks:** cross-compiling `aarch64-unknown-linux-musl` toolchain setup on
the Windows dev machine (mitigation: p0-06 accepts amd64-local proof + arm64
cross-compile check only; on-device RB5009 validation belongs to Phase 1).
