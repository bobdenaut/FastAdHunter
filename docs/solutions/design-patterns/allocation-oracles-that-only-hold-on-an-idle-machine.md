---
title: Allocation oracles that only hold on an idle machine, and one flake still unidentified
date: 2026-09-14
category: design-patterns
module: fah-http, fah-dns
problem_type: design_pattern
component: testing_framework
severity: medium
applies_when:
  - a test counts allocations with a counting GlobalAlloc and compares two batches
  - a test asserts exact equality between two measurements of the same workload
  - a new long-running integration test is added to the workspace suite
  - the gate is red only when the whole workspace runs, green target by target
symptoms:
  - an allocation test fails by one allocation, in either direction
  - the same test passes standalone and fails under `cargo test --workspace`
  - the failure appears after an unrelated test target was added
---

# Allocation oracles that only hold on an idle machine

Two things are recorded here. The first is settled and the fix is in the tree.
The second is open, and it is the reason this file exists: a rare failure in
`fah-dns --lib` whose name was lost, which nobody has reproduced since.

## 1. Exact-equality allocation oracles — settled 2026-09-14

### What happened

p3-10c added `crates/fastadhunter/tests/acceptor_death.rs`, which boots two real
`fastadhunter` binaries, four worker threads each, for 25–40 s. With it in the
tree, `cargo test --all-features --workspace` went red on tests the task never
touched:

| Test | Reading | Direction |
| ---- | ------- | --------- |
| `intercept_alloc::warm_intercepted_requests_allocate_a_steady_amount` | 3201 then 3200 | **down** one allocation |
| `proxy_alloc::warm_proxy_requests_allocate_a_steady_amount` | 1280 then 1281 | **up** one allocation |

Both had the same shape: two batches of 64 warm requests, counted by a counting
`GlobalAlloc`, compared with `assert_eq!`.

### Why it was the test, not the code

The invariant these tests exist for is "a warm request does not accumulate".
The assertion written for it was "two batches allocate exactly the same number
of times", which is a stronger claim: it requires that the same logical work
also *fragments* identically. It does not. The workload is a real TLS or TCP
client against a real local origin; under CPU contention a response that arrived
in one read arrives in two, and hyper allocates one more buffer — or one fewer.

The direction matters for the fix. The first failure was downward, which is not
growth at all, and the obvious patch — `last <= previous` — would have passed
it. The second failure was upward, on the same day, and would have failed that
patch. The jitter is bidirectional, so only a tolerance survives both.

### The fix

In all three files with this shape — `intercept_alloc.rs`, `proxy_alloc.rs` and
`forward_alloc.rs` (two sites in the last) — the equality became:

```rust
const JITTER_ALLOWANCE: usize = 4;

assert!(
    last <= previous + JITTER_ALLOWANCE,
    "... one leaked allocation per request would show as +{REQUESTS} here, \
     so a difference within {JITTER_ALLOWANCE} is noise ..."
);
```

`4` is chosen against the signal, not against the noise: every one of these
tests runs 64 operations per batch, so the smallest regression they can see — a
single leaked allocation per operation — shows as **+64**. A tolerance of 4 is
sixteen times below that and above the ±1 actually observed. The per-operation
ceiling assertions were left untouched; those, not the batch comparison, are
what catches real growth.

`assert_eq!` was not merely loosened. It was replaced with the assertion the
test always meant.

### The rule worth keeping

**Never assert bit-for-bit equality on a measurement a scheduler or the network
stack can touch.** Assert the property you care about, scaled to the signal a
real regression would produce. If the smallest meaningful regression is +64,
anything under 64 is not evidence of anything.

Two files with a counting allocator were checked and deliberately left alone:
`fah-rules/tests/url_lookup_alloc.rs` asserts an absolute zero on the hot path,
and `fah-dns/tests/udp_inflight_cost.rs` only reports the figure. Neither
compares batches, so neither has this failure mode.

## 2. Open — one `fah-dns --lib` failure, name lost

### What was seen

During the same session, one run of `cargo test --all-features --workspace`
reported:

```text
test result: FAILED. 220 passed; 1 failed; 2 ignored
error: test failed, to rerun pass `-p fah-dns --lib`
```

The test name was never captured: the command that produced it ran the suite
twice — once to count, once to grep — and the failing output belonged to the run
that was not kept.

### What was done immediately afterwards

`fah-dns --lib` was run four times standalone: 221/221 each time. The full
workspace suite was run five more times: 62 targets green each time. Nothing
reproduced it.

So: something in `fah-dns --lib` fails rarely under load. It is not in anything
p3-10b or p3-10c touched, and it is not the allocation family above — those live
in separate test targets.

### How to hunt it

1. **Never lose the name again.** One run, output to a file, `--no-fail-fast`,
   read the file afterwards. Do not pipe a second run into `grep`.
2. **Reproduce the load that exposed it.** It appeared while `fah-dns --lib` ran
   beside two real binaries on four threads each. Loop the target 20–30 times
   with `crates/fastadhunter/tests/acceptor_death.rs` running alongside, and add
   `-- --test-threads=16` so the target's own tests contend with each other.
3. **Narrow once it falls.** With a name, run that test alone in a loop, with and
   without background load, and see whether contention is sufficient to make it
   deterministic.

### Where to look first

52 of `fah-dns`'s unit tests use `#[tokio::test(start_paused = true)]` and are
immune to machine load. The candidates are the ones on a real clock:

| Module | Real-clock tests |
| ------ | ---------------- |
| `upstream/mod.rs` | 22 |
| `pipeline.rs` | 16 |
| `dot.rs` | 10 |
| `cache.rs` | 10 |

`upstream/mod.rs` is the first suspect — real timeouts against local sockets and
walk deadlines. `dot.rs` is the second: real TLS handshakes, and a 500 ms sleep
in the connection-ceiling test. Both are guesses from shape, not from evidence.

### Why it matters more than it looks

p3-11 runs a seven-day soak. A test that fails rarely under load is the same
species as the reports that soak will produce, and a reader who has already seen
one unexplained red will discount the next one. Find it before the week starts,
or record deliberately that it was left open.

## Handover — prompt for the agent that hunts it

Paste this as that agent's first message. It is written to be self-contained;
everything it needs from this file is repeated inside it.

```text
You are hunting one rare test failure in the FastAdHunter repository
(E:\FastAdHunter). Reply in Romanian, briefly. Your job is to IDENTIFY it, not
to fix it.

WHAT IS KNOWN
On 2026-09-14, one run of `cargo test --all-features --workspace` reported:

    test result: FAILED. 220 passed; 1 failed; 2 ignored
    error: test failed, to rerun pass `-p fah-dns --lib`

The failing test's name was lost: the command that produced it ran the suite
twice — once to count results, once to grep — and the output kept was from the
run that passed. Immediately afterwards `fah-dns --lib` was run four times
standalone (221/221 each) and the full workspace suite five more times (62
targets green each). Nothing reproduced it.

So: something in the `fah-dns` library test target fails rarely, and the only
run that ever showed it was a full-workspace run, where two real `fastadhunter`
binaries were also running on four worker threads each for 25–40 s.

It is NOT the allocation-oracle family described in the file this prompt came
from (`docs/solutions/design-patterns/allocation-oracles-that-only-hold-on-an-idle-machine.md`,
§1) — those live in separate test targets and were fixed the same day.

READ FIRST, AND ONLY THIS
- `docs/solutions/design-patterns/allocation-oracles-that-only-hold-on-an-idle-machine.md`
  — §2 is this bug; §1 is the neighbouring problem, already settled, so you do
  not re-derive it.
Do not read the phase plans or the root documents. This is not task work.

METHOD — in this order
1. Never lose the name again. One run per invocation, output redirected to a
   file, `--no-fail-fast`, then read the file. Do not pipe a second run into
   grep.
2. Reproduce the load that exposed it: loop `cargo test --all-features -p
   fah-dns --lib` 20–30 times while `cargo test --all-features -p fastadhunter
   --test acceptor_death` runs alongside, and add `-- --test-threads=16` so the
   target's own tests contend with each other.
3. Once it falls, narrow: run that one test in a loop, with and without
   background load, and establish whether contention alone makes it
   deterministic.

WHERE TO LOOK IF IT WILL NOT FALL
52 of `fah-dns`'s unit tests use `#[tokio::test(start_paused = true)]` and are
immune to machine load. The candidates are the real-clock ones: `upstream/mod.rs`
(22), `pipeline.rs` (16), `dot.rs` (10), `cache.rs` (10). `upstream/mod.rs` is
the first suspect — real timeouts against local sockets and walk deadlines —
and `dot.rs` the second: real TLS handshakes plus a 500 ms sleep in the
connection-ceiling test. Both are guesses from shape, not evidence; say so if
you end up leaning on them.

Reading a suspect's source to judge whether its assertion depends on "this
happened within N milliseconds" is legitimate and cheap. Deciding it is the
culprit without a captured failure is not.

CONSTRAINTS
- Do not use python for anything (repo rule 17). Use the editor tools, or sed.
- Do not modify any test, any production code, or any .md, except the one file
  named under REPORTING.
- No commit, no push, no tag.
- Do not touch the RB5009 router.
- The machine is shared with a running hourly scheduled task, `FAH-soak-0.3.4`.
  Do not stop or disturb it. Expect your loops to compete with it once an hour.
- Loops cost real CPU and the fans are audible. Prefer 20–30 focused iterations
  over an open-ended run, and stop as soon as you have a name.

REPORTING
Append your result to §2 of
`docs/solutions/design-patterns/allocation-oracles-that-only-hold-on-an-idle-machine.md`
under a new heading `### Hunt, <date>`:
- how many iterations you ran and under what load;
- the captured failure verbatim, if you got one — test name, assertion, values;
- what you concluded, separating measured evidence from inference;
- if you did not reproduce it: say that plainly, with the iteration count, and
  leave the section open rather than closing it on a guess.

Then stop and report in chat in two or three sentences: reproduced or not, the
name if you have it, and what you would do next. Do not fix it.
```
