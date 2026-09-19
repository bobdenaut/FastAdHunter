# CONTRIBUTING

Conventions for working on FastAdHunter. Short version: performance is the
primary feature — prove your change doesn't cost anything.

## Quality gates (run locally — there is no CI)

The project deliberately runs no CI service. Every gate below must pass on
your machine before merging to `main`:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --all-features --workspace
cargo bench            # when touching a hot path — compare against main
```

An optional git pre-commit hook running the first three is recommended.

### The dashboard has its own two gates

Run both from `dashboard/frontend/`:

```sh
npm run typecheck      # tsc --noEmit
npx vitest run
```

**Never run Prettier here.** The frontend ships no Prettier config, so
`npx prettier --write` reformats to Prettier's defaults rather than to the
repo's style — double quotes for single, its own line breaks — and rewrites
every file it is pointed at, burying the actual change in a hundred lines of
churn. Match the formatting of the file you are editing by hand.

### `test-harness` is a dev-profile-only feature

`fah-api` and `fastadhunter` each carry a non-default `test-harness` feature. It
carries the relaxed login rate limiter and the known-password `AuthState`
constructor that the integration harnesses and the p5-04 measurement legs need.
**Shipping it would leave the appliance with no effective online-guessing
control**, so a `compile_error!` in `crates/fah-api/src/lib.rs` fails any build
that enables it without `debug_assertions`.

Consequences to know before you hit them:

- `cargo test --all-features` (dev profile) is the intended context and is the
  gate above.
- The p3-06 full-mode e2e (`crates/fastadhunter/tests/e2e_https.rs`) needs the
  binary's `test-harness` feature: only then does `main.rs` add the upstream
  trust anchor from `FAH_TEST_UPSTREAM_ROOT`, which its loopback origin needs
  to be verified on the terminate leg. Without the feature the test fails (it
  does not pass on the fail-closed path); `FAH_SECURITY_ALLOW_SKIP=1` is the
  only way to accept that degraded run or a `127.0.0.x:443` origin that cannot
  bind, and a gate run never sets it.
- **`cargo test --release --all-features` does not compile.** That is the guard
  working, not a break.
- Building the measurement harness needs the flag back on:
  `RUSTFLAGS="-C debug-assertions=yes" cargo build --release --locked -p fastadhunter --features test-harness`.
- **Never add `--all-features` to a release build**, and never to the
  `Dockerfile`. The shipped path is
  `cargo build --release --locked -p fastadhunter`, and `resolver = "2"` keeps
  dev-dependency features out of it.

## Performance discipline

- Budgets live in [PERFORMANCE.md](PERFORMANCE.md); benches live in `benches/`.
- Touching a hot path (pipeline, matcher, cache, listeners)? Run
  `cargo bench` on `main` and on your branch; a regression > 10% needs
  explicit justification in the PR/commit description.
- Every PR description states its runtime cost: allocations added? hot path
  touched? new dependencies?

## Commits & branching

- **Conventional Commits**: `feat:`, `fix:`, `perf:`, `docs:`, `refactor:`,
  `test:`, `chore:` (+ optional scope: `feat(fah-rules): …`).
- **Trunk-based**: short-lived branches off `main`, merged quickly.
  No `develop` branch.

## Code rules

- `rustfmt` default profile; `clippy -D warnings` — no exceptions parked
  "for later".
- `unsafe` requires a `// SAFETY:` justification comment and review; prefer
  not needing it.
- Respect the dependency layering in [ARCHITECTURE.md](ARCHITECTURE.md):
  dependencies point downward, siblings never import each other,
  `fah-model` stays pure (data types and trivial traits only).
- Use the vocabulary of [CONTEXT.md](CONTEXT.md) in code, docs and APIs.
  New/changed domain terms update CONTEXT.md in the same change.
- Hard-to-reverse decisions with real trade-offs get an ADR in
  `docs/decisions/`.
- Log and stderr messages are ASCII only. RouterOS prints anything else as
  hex bytes (an em-dash shows as `E28094`), and its log is where the
  container's output is read.

## Tests

- Unit tests live with their crate; cross-crate behavior goes in `tests/`.
- Rule Engine changes ship with parser fixtures (real-world list excerpts)
  and verdict tests.
- Bug fixes include a regression test.
- A new `/api/v1/` route needs a fixture in `requests/*.http`, or
  `crates/fah-api/tests/request_coverage.rs` fails. Nothing in those files is
  ever issued by the suite, so a destructive route still takes a fixture.
