# CONTRIBUTING

Conventions for working on FastAdHunter. Short version: performance is the
primary feature — prove your change doesn't cost anything.

## Quality gates (run locally — there is no CI)

The project deliberately runs no CI service. Every gate below must pass on
your machine before merging to `main`:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench            # when touching a hot path — compare against main
```

An optional git pre-commit hook running the first three is recommended.

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

## Tests

- Unit tests live with their crate; cross-crate behavior goes in `tests/`.
- Rule Engine changes ship with parser fixtures (real-world list excerpts)
  and verdict tests.
- Bug fixes include a regression test.
