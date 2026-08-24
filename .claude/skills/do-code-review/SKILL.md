---
name: do-code-review
description: Review a finished task implementation against its plan file and write evidence-based findings into the task's review file. Use when the user runs `/do-code-review <task>.md <task>-review.md`, or asks to review a plan/task implementation, audit a task against its acceptance criteria, or produce a task review document.
allowed-tools: Read, Grep, Glob, Bash, Write, Edit
---

# do-code-review

Review the implementation of a plan/task file. Report findings only — never fix
code.

## Arguments

`/do-code-review <task-name>.md <task-name>-review.md`

Two whitespace-separated arguments. Read them from the invocation text, not
from a positional dollar-placeholder — substitution is unreliable here (it
resolves to the wrong argument) and eats Windows backslashes. Prefer forward
slashes in any path you pass.

| Arg | Meaning | Resolution |
| --- | --- | --- |
| first | plan / task / spec file — the source of truth for intended behaviour | a path (absolute, or containing a separator) is used as-is; a bare name is globbed repo-wide, `**/<name>`. Also read a `<stem>-plan.md` sibling when one exists |
| second | review output file | a path is used as-is, creating parent directories if needed; a bare name goes to the deepest existing directory in this order: a `*code-review*` directory whose path shares the plan's own phase/module token, any `*code-review*`/`*reviews*` directory, else beside the plan file |

Second argument missing -> `<chosen directory>/<plan stem>-review.md`.

**No arguments at all -> stop and ask which file to review.** There is no
default: do not infer the task from the phase table, the branch name, the last
commit or the most recently modified plan file. Print the usage line and wait.

Resolution failures, each of which stops the run before anything is read or
written:

- bare name matches nothing -> say which argument and what was searched;
- bare name matches more than one file -> list the matches, ask which;
- first argument resolves to a directory, or to a file that is not text -> say so.

Never substitute a near-miss for a name that did not resolve.

## Procedure

1. **Read the plan.** The first argument's file plus its `-plan.md` sibling if
   present. Extract:
   implementation units, acceptance criteria, explicit constraints, out-of-scope
   boundaries, named specs.
2. **Scope the diff.** Find the commit(s) that implemented the task: `git log
   --oneline -20`, matching the plan's stem or slug anywhere in the subject
   (a Conventional Commit scope, a ticket id, a branch name — whichever this
   repo uses). Then `git diff --stat <base>..HEAD` first, never a whole-repo
   read. Uncommitted work: `git diff --stat` / `git status --short`. No commit
   matches and the tree is clean -> say so and ask which range to review.
3. **Enumerate** changed files and symbols from the diff.
4. **Search before reading.** Grep for callers, implementations, trait impls and
   call sites of every changed symbol. Read only the ranges the diff and those
   hits point at (`Read` with `offset`/`limit`). Expand scope only when evidence
   shows a dependency, and say so in the finding.
5. **Run the review checklist** below, all 8 categories.
6. **Gates, as evidence only** (optional, when the verdict depends on it):
   `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
   --message-format=short -- -D warnings`, `cargo test --all-features
   --workspace`. Run them bare — never pipe through `tail`/`head`/`grep`, which
   bypasses the rtk filter and truncates the counts to the last suite. Record
   the counts. Do not fix anything they report — that is a finding.
7. **Write the second argument's file** in the output format below.
8. **Reply in chat with exactly one line:** `Review written to <path>.`
   Findings never appear in chat.

## Review checklist

This is a performance- and memory-sensitive Rust DNS system. Performance and
memory are first-class review gates, not secondary concerns.

**1. Plan compliance**

- Every implementation unit implemented as specified.
- Every acceptance criterion verified.
- Every explicit constraint and out-of-scope boundary honoured.
- Any undocumented behavioural or architectural deviation identified.

**2. Correctness**

- Implementation matches the intended behaviour and contracts.
- Error handling, state transitions, lifecycle, concurrency, cancellation and
  shutdown semantics where applicable.
- Races, deadlocks, task leaks, silently detached failures, wrong error
  propagation, invalid state transitions.

**3. Architecture**

- Architectural boundaries and responsibilities from the plan preserved.
- No new abstraction, dependency, API or coupling introduced without need.
- Problem solved without scope expansion.
- Architectural decisions taken during implementation but not authorized by the
  plan are flagged.
- Repo layering holds: L4 `fastadhunter` -> L3 `fah-dns`/`fah-http`/`fah-api`/
  `fah-stats`/`fah-metrics` -> L2 `fah-rules` -> L1 `fah-model`/`fah-config`/
  `fah-common`/`fah-logging`; siblings never import siblings; `fah-model` stays
  pure data.

**4. Performance**

- DNS hot path and other declared performance-sensitive paths are critical.
- No unnecessary work added to a hot path.
- Inspect allocations, locks, atomics, syscalls, logging, formatting, cloning,
  synchronization, serialization, async scheduling overhead.
- Throughput, latency, tail-latency, CPU and contention regressions.
- Error handling or observability that can create excessive CPU or I/O pressure
  under repeated failures.
- Compare against existing patterns and benchmarks where relevant.

**5. Memory management**

- New allocations, retained state, buffer growth, cloning, ownership patterns
  that raise memory use.
- Task, connection, socket, timer, buffer and resource lifetimes.
- Leaks or unbounded accumulation across repeated operations or failures.
- All new state bounded where appropriate.
- Long-running RSS/allocator behaviour, not only short test runs.

**6. Rust-specific quality**

- Ownership and lifetime correctness; unnecessary clones/copies.
- Async cancellation behaviour; error propagation.
- Panic/`unwrap`/`expect` paths.
- API/type design; unnecessary abstraction or complexity.
- Correct `Send`/`Sync`/concurrency behaviour where applicable.

**7. Tests**

- Tests actually prove the required behaviour.
- Missing coverage for success, failure, boundary, concurrency and lifecycle
  cases.
- Flaky, needlessly timing-dependent, or invariant-free tests.
- Existing tests and benchmarks stay meaningful.

**8. Regression analysis**

- Behaviour changed outside the task's intended scope.
- Especially performance, memory, concurrency, persistence, API compatibility,
  operational behaviour.
- Interactions with neighbouring tasks in the same phase where the plan names a
  dependency or shared invariant.

Review the repository and implementation directly. Do not assume the
implementation is correct because tests pass.

## Finding rules

- **Report only. Do not modify files, do not fix code, do not rewrite the
  implementation.** The one file this skill writes is the second argument's.
- Severity ladder, highest first (`plan/<phase>/CLAUDE.md` §CODE REVIEW):

  | Severity | Meaning |
  | --- | --- |
  | `Critical` | correctness, safety, architectural, performance or memory regression that must be fixed before the task is marked `DONE` |
  | `Major` | real defect, regression or missing guarantee with clear impact |
  | `Should-fix` | genuine defect with bounded impact — fix before merge unless explicitly deferred |
  | `Minor` | small defect or maintainability cost, safe to defer |
  | `Nitpick` | non-blocking improvement, no impact if never done |

- Each finding also states, per the phase rule: 
  status (`OPEN`, `FIXED`, `DEFERRED` or `REJECTED`) · the technical rationale · the impact if left unchanged · fix-before-DONE or explicitly deferred · whether the evidence is measured or inferred.
- Every finding carries: severity · exact file and line/range · concrete
  evidence from the implementation · why it violates the plan, correctness,
  architecture, performance or memory requirements · the smallest appropriate
  remediation direction.
- No manufactured findings. Every finding traceable to the repository, the
  implementation, the plan, or an established project constraint.
- A category checked with nothing found is stated explicitly as checked and
  acceptable.

## Output format

Write the second argument's file following `docs/code-review/CLAUDE.md`: tables
over prose, facts over
explanation, paragraphs of 5 lines maximum, nothing that could be learned by
reading the code.

**New file:**

```markdown
# <TASK-ID> — <Task title> — Review

**Task:** [<task>.md](<relative path>) · **Plan:** [<task>-plan.md](<path>)
· **Spec:** <spec ids, if the plan names any>

## Findings

### Plan compliance — verified
<table: unit / acceptance criterion -> where satisfied, file:line>

### Findings
**F1 — Critical | Major | Should-fix | Minor | Nitpick — <one-line title>**
<evidence at file:line · why it violates the plan/correctness/architecture/
performance/memory · impact if left unchanged · measured or inferred ·
fix-before-DONE or deferred · smallest remediation>

**F2 — …**

### Categories checked, no issue found
<one bullet per checklist category with nothing to report, saying what was
inspected>

### Status
**PASS** | **PASS WITH DEFERRED FINDINGS** | **BLOCKED**
<one line per open finding: what closes it, and where>
```

**Existing file — append, never rewrite.** Add a dated section at the end:

```markdown
### Re-review (<YYYY-MM-DD>)
<what changed since the previous pass — diff --stat, gates, which findings
moved>
<restated Status line>
```

Get the date from `date +%F`; never invent one. Keep prior findings and their
IDs intact; a fixed finding is marked in place (`F2 — FIXED (<date>) — …`), it
is not deleted.
