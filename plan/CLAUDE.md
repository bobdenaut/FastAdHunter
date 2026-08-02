# Workflow — Context-Driven Task Orchestration for AI Agents

**Structure:** this directory exists to simplify the workflow and prevent
loading the entire project into context.

**This file is workflow only.** Engineering principles, hard rules and response
style live in the repo root [CLAUDE.md](../CLAUDE.md), which is always loaded.
They were duplicated here and drifted — rules 19 and 20 existed only in this
copy, so they went unread until they were broken. One copy, in the file that is
always loaded (principle 4).

## Phase execution order

Implement phases sequentially, lowest number first (`phase0`, `phase1`, …).

## Task naming and order

Task files follow `p<phase>-<NN>-<slug>.md` (e.g. `p0-01-workspace-skeleton.md`).
`NN` is zero-padded and defines execution order within the phase — the lowest
`NN` with status `WAITING` is picked first.

Status lives only in the phase's `CLAUDE.md` table (last column) — task files
carry no status field. Valid values: `WAITING`, `DONE`, `BLOCKED` (a task that
failed its test-fix retries — see "Mandatory steps" §3), `AWAITING SOAK`.

`AWAITING SOAK` — code complete and gates green, but an acceptance criterion
needs on-device evidence a dev box cannot produce. The selector skips it like
`DONE`, so the phase keeps moving; the table cell must name what flips it. A
phase is not finished while one exists.

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
