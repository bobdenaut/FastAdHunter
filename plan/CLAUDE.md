# Workflow — Context-Driven Task Orchestration for AI Agents

**Structure:** this directory exists to simplify the workflow and prevent
loading the entire project into context.

## Engineering Principles

Treat this project as production infrastructure software where correctness, maintainability and predictable performance are more important than cleverness.

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

## Phase execution order

Implement phases sequentially, lowest number first (`phase0`, `phase1`, …).

## Task naming and order

Task files follow `p<phase>-<NN>-<slug>.md` (e.g. `p0-01-workspace-skeleton.md`).
`NN` is zero-padded and defines execution order within the phase — the lowest
`NN` with status `WAITING` is picked first.

Status lives only in the phase's `CLAUDE.md` table (last column) — task files
carry no status field. Valid values: `WAITING`, `DONE`, `BLOCKED` (a task that
failed its test-fix retries — see "Mandatory steps" §3).

## Folders holding phases by status

- `open` — phases waiting to be picked up
- `wip` — phase under active work (**at most one** phase directory at a time)
- `closed` — finished phases

## Mandatory steps (work in this exact order)

These steps are the entry point for every session, including after a crash or
restart — always begin at Step 1.

1. Scan `wip`.
   - If it contains a phase directory:
     1. Open it (e.g. `wip/phase0`).
     2. Read its `CLAUDE.md`.
     3. Work the tasks with status `WAITING`, lowest `NN` first.

2. If `wip` is empty:
   - Move the lowest-numbered phase directory from `open` to `wip`.
   - Continue from Step 1.

3. When a task completes (e.g. `wip/phase0/p0-01-workspace-skeleton.md`):
   - Run the quality gates (whole workspace, per root CLAUDE.md — there is no CI):

     ```sh
     cargo fmt --check
     cargo clippy --workspace --all-targets -- -D warnings
     cargo test --workspace
     ```

   - If a gate fails: attempt a fix and re-run, up to 3 attempts. Still failing →
     mark the task `BLOCKED` in the phase `CLAUDE.md`, stop, report to the user.
   - Otherwise mark it `DONE` in the phase `CLAUDE.md`.
   - If every task in the phase is `DONE`:
     - Move the whole phase directory from `wip` to `closed/`.
     - Report: "`<phase>` done, gates GREEN. Waiting for approval to commit."
     - Wait for approval. One commit covers everything since the last commit:
       code changes and the `open` → `wip` → `closed` moves together (staging
       delete + add in the same commit lets git detect the rename; a separate
       move-commit would show as plain deletion). Conventional Commits with the
       task reference:

       ```text
       feat(phase0/p0-01-workspace-skeleton): initialize Cargo workspace
       ```

## Equivalent algorithm

```text
WHILE `open` is not empty OR `wip` is not empty
    IF `wip` is empty THEN
        Move lowest-numbered phase from `open` to `wip`
    END IF
    Read wip/<phase>/CLAUDE.md
    Select first task with STATUS = WAITING
    IF none THEN
        Move wip/<phase> to closed; CONTINUE
    END IF
    Read the task file; execute the task
    Run gates: fmt --check, clippy -D warnings, test (workspace)
    IF failing THEN
        Fix + re-run, up to 3 attempts
        IF still failing THEN mark BLOCKED, STOP, report END IF
    END IF
    Mark task DONE in wip/<phase>/CLAUDE.md
END WHILE
```
