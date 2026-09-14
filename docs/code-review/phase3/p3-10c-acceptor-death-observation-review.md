# P3-10c — Acceptor Death Observation — Review

**Task:** [`plan/wip/phase3/p3-10c-acceptor-death-observation.md`](../../../plan/wip/phase3/p3-10c-acceptor-death-observation.md)
**Plan:** [`plan/wip/phase3/p3-10c-acceptor-death-observation-plan.md`](../../../plan/wip/phase3/p3-10c-acceptor-death-observation-plan.md)
**Implemented:** 2026-09-14 · **Base:** `fb5428f` · **Findings:** not yet reviewed

## Implementation Summary

The HTTP acceptor, the HTTPS acceptor and the API accept loop now report an
unplanned end. Each type keeps its `JoinHandle` for stopping and hands it over
only once it is finished; the binary asks all three on the 10 s supervision tick
it already runs, classifies whatever it is handed with the code that already
classifies every other task death, counts it through `record_task_death` and
logs it once at `error` with the acceptor named. The run loop is unchanged and
the resolver keeps answering. Nothing restarts.

Before this, a dead DoT listener ended the process while a dead DoH listener —
DoH is a route on the API server — was silent.

## Decisions as built

| # | Decision | As planned? |
| - | -------- | ----------- |
| 1 | `take_finished_acceptor(&mut self) -> Option<JoinHandle<()>>` on all three types; `Some` only when `is_finished()`; no `shutdown()` changed (D1) | yes |
| 2 | Panic, semaphore-closed and foreign abort all reported, through `Cause` (D2) | yes |
| 3 | Polled from `reap_dead_tasks`; `death_of(name, handle)` extracted from `reap` so one place decides what a death is; names `"HTTP acceptor"`, `"HTTPS acceptor"`, `"API acceptor"` (D3) | yes |
| 4 | `ApiServer::accept_loop` becomes `Option`; `slots: Arc<Semaphore>` hoisted into the struct and passed to `accept()`; `shutdown(&self)` keeps its signature (D4) | yes |
| 5 | Test seam closes only the named acceptor's admission, `test-harness`-gated, sentinel-file triggered, one-shot, with a `warn!` naming the targets (D5) | **plan amended first — see below** |
| 6 | No restart, no exit, `run()` untouched (D6) | yes |
| 7 | Allocation-domain threads out of scope (D7) | yes |

### D5 was rewritten before any code was written

The plan's first approved form put the sentinel check in a
`cfg(test-harness)` **`tokio::select!` arm**. That does not compile:
`select!`'s branch grammar is `<pat> = <fut> (, if <cond>)? => <handler>,` and
has no slot for an attribute. The implementer stopped at that wall, proved it
against the compiler with a throwaway probe file, and reported rather than
picking a route — which is what the handover asked for.

The owner settled the replacement on 2026-09-14: the check runs as a
`cfg(test-harness)` **statement at the top of `reap_dead_tasks`**, not as a
select arm and not as a separate task. An attribute on a statement is ordinary
Rust. Three consequences, all accepted deliberately:

- the 200 ms ticker is gone; there is no test-only polling frequency beside the
  supervision tick;
- the kill lands on a tick boundary, so the e2e budget is 35 s per boot, a
  timeout rather than an expected runtime (~10 s typical);
- `run()` is not edited at all, in any build, which is stronger than what D6
  originally promised.

D5 and D6 in the plan file were rewritten to say this. That edit is part of this
changeset.

## Files changed

| File | Change |
| ---- | ------ |
| `crates/fastadhunter/src/supervisor.rs` | `death_of(name, handle) -> Death` extracted from `reap`, which now calls it; one test for the new public entry point |
| `crates/fah-http/src/server.rs` | `take_finished_acceptor`, gated `close_admission`, three tests |
| `crates/fah-http/src/tls_server.rs` | the same two methods and three tests on `TlsServer` |
| `crates/fah-http/Cargo.toml` | new `[features] test-harness = []`, with the reason recorded above it |
| `crates/fah-api/src/server.rs` | `accept_loop` becomes `Option`; `slots` hoisted into the struct and passed to `accept()`; `take_finished_acceptor`; gated `close_admission`; `shutdown` unchanged in signature |
| `crates/fah-api/tests/api.rs` | three tests through the existing harness |
| `crates/fastadhunter/Cargo.toml` | `test-harness` forwards to `fah-http/test-harness` beside `fah-api/test-harness` |
| `crates/fastadhunter/src/main.rs` | `KillRequest` + `kill_request()` + `trip_kill_sentinel()`, all gated; `reap_dead_tasks` trips the sentinel, then asks the three acceptors and reports through the existing counter and log line |
| `crates/fastadhunter/tests/acceptor_death.rs` | new, two boots |
| `plan/wip/phase3/p3-10c-acceptor-death-observation-plan.md` | D5 and D6 rewritten, §4–§7 aligned |

## Tests

| § | Test | Where |
| - | ---- | ----- |
| 7.1 | `a_handle_handed_over_on_its_own_is_classified_the_same_way` | `supervisor.rs` |
| 7.2 | `a_running_acceptor_is_not_handed_over`, `a_returned_acceptor_is_handed_over_once`, `shutdown_is_safe_after_the_handle_was_taken` | `fah-http/src/server.rs` |
| 7.3 | the same three on `TlsServer` | `fah-http/src/tls_server.rs` |
| 7.3 | the same three on `ApiServer`, through the existing harness | `fah-api/tests/api.rs` |
| 7.4 | `a_dead_http_or_https_acceptor_is_observed_and_dns_keeps_answering`, `a_dead_api_acceptor_is_observed_and_dns_keeps_answering` | `crates/fastadhunter/tests/acceptor_death.rs` |

The HTTP `shutdown_is_safe_after_the_handle_was_taken` goes further than the
other two: it takes the handle, then asserts `shutdown()` still flips the `stop`
watch and still drains the domain threads, which is the part of `Server::shutdown`
that has nothing to do with the acceptor.

Both e2e boots follow the ordering the owner set: prove DNS answers, trip the
sentinel, poll for the observation while poking one throwaway connection per
selected acceptor and ignoring the result, assert the log names the acceptor
with `cause=returned`, prove DNS still answers. Boot 2 also asserts the API port
now refuses a connection — a logged death with a live socket would mean the
wrong thing died — and it is the only place that shows DoH dying without the
resolver dying with it. The first boot additionally asserts `tasks_died == 2`
exactly, so a third death fails the test rather than passing unnoticed.

## Mutation results

**Reported by the implementer, not re-run by the reviewer.** All five behaved as
the plan predicts, including the one that matters most: mutation 5, which
deletes the acceptor half of `reap_dead_tasks` outright, kills both e2e boots
while every per-crate test stays green — the failure mode this task exists to
prevent.

## Collateral: three allocation-steadiness tests corrected

The full-workspace gate went red on tests this task does not touch. Diagnosis,
owner-approved fix, and the reason it is in this changeset:

`intercept_alloc.rs`, `proxy_alloc.rs` and `forward_alloc.rs` each compared two
batches of 64 warm operations with `assert_eq!` on the allocation count. That
asserts bit-for-bit equality between two measurements a scheduler and TCP/TLS
chunking both touch, so it holds on an idle machine and breaks under load. The
new e2e adds two real binaries × 4 worker threads for ~25–40 s, which was enough
to break it — first `intercept_alloc` (3201 → **3200**, a *decrease*), then
`proxy_alloc` (1280 → **1281**, an increase). The same test had already flaked
once during p3-10b's workspace run, before this task existed; it is recorded in
that task's review under §Other observations.

The fix, applied to all three files with the owner's approval: a named
`JITTER_ALLOWANCE = 4` and `last <= previous + JITTER_ALLOWANCE` in place of the
equality, with the assertion message explaining that this checks the absence of
growth rather than equality, and that one leaked allocation per request would
show as +64 — sixteen times the allowance. Every per-request ceiling assertion
was left untouched; those are the guard that catches real growth.

A plain `<=` with no tolerance was considered and rejected: the jitter is
bidirectional, and `proxy_alloc` proved it by failing upward.

Searched for the same pattern across the workspace: five files install a
counting allocator; the two not corrected do not compare batches
(`url_lookup_alloc.rs` asserts an absolute zero on the hot path,
`udp_inflight_cost.rs` only reports the figure). No other cases exist.

## Gates

Run by the reviewer on the final tree:

```text
cargo fmt --all -- --check                                     clean
cargo clippy --workspace --all-targets -- -D warnings          clean
cargo test --all-features --workspace                          62 targets ok, 0 failed
```

The full suite was run **five times** after the allocation-test corrections;
62 targets green every time. It was also run once with the new e2e file moved
aside (61 targets green) to establish that the corrections, not the removal of
the new test, are what made the gate stable.

## Known limitations and open items

- **One unidentified rare failure.** During the correction work, a single
  workspace run reported `-p fah-dns --lib` with 220 passed and 1 failed; the
  test name was lost before it could be captured. `fah-dns --lib` was then run
  four times standalone (221/221 each) and the full suite five more times, all
  green. Something in that target fails rarely under load. It is not in anything
  this task touched, and it is worth finding before the seven-day soak, where a
  rare load-sensitive failure is exactly what a reader will have to interpret.
- **Documentation written in this changeset**, each approved before it was
  written: `CONTEXT.md` §Supervised Task (hard rule 6 fired — the term changed),
  `ARCHITECTURE.md` §long-lived task death, `API.md` §`counters.tasks_died`, the
  matching prose in `requests/telemetry.http` and `requests/health.http`, and the
  comment above the figure in
  `dashboard/frontend/src/pages/health/engine-card.tsx`, which carries the plan's
  §8 wording verbatim.
- **The phase table** shows row 10c as `DONE`. The phase stays in `wip` for
  row 11, whose seven-day soak no longer waits on 10b or 10c — only on the deploy
  decision.
- **No commit, no push, no tag.** The phase directory has not moved.

## Findings

**Reviewed:** 2026-09-14, on the working tree at base `fb5428f` (18 modified
files, 2 untracked). This section supersedes §Mutation results above, which was
written before any mutation had been re-run: all five were applied, run and
reverted during this pass, and the tree was confirmed identical afterwards
(`git diff --stat` back to 18 files, 574 insertions).

No blocker. One should-fix and four notes.

### 1 — should-fix · `requests/telemetry.http:123` names the wrong port for the HTTPS acceptor

The new operator table reads:

```text
#   HTTPS acceptor   port 8443's proxy leg stopped; no SNI filtering
```

8443 is the **API** port (`crates/fah-config/src/schema/api.rs:31-33`,
`default_port() -> 8443`). The HTTPS proxy listener defaults to **8444**
(`crates/fah-config/src/schema/https.rs:86-88`, and `CONFIGURATION.md:250`
documents `port = 8444` for `[https.listen]`). The line therefore points a
reader at the API listener, and the next line of the same table tells them the
API acceptor is a different thing — the document contradicts itself two lines
apart. `fah-config`'s own conflict test encodes both defaults
(`crates/fah-config/src/lib.rs:731-732`).

This is the one place in the changeset that names concrete ports, so it is the
place an operator reads first when `counters.tasks_died` goes non-zero.

Remediation: `8443` becomes `8444` on that line. Nothing else in the table needs
touching — `8080` for the HTTP acceptor is correct.

### 2 — note · `crates/fah-api/src/server.rs:132` now states the opposite of what the task relies on

```rust
// The semaphore is never closed, so acquire cannot fail.
let Ok(slot) = Arc::clone(&slots).acquire_owned().await else {
    return;
};
```

Under `test-harness` the semaphore *is* closed — `close_admission`
(`crates/fah-api/src/server.rs:111-113`) closes exactly this one, and the `else`
branch the comment dismisses is the single `return` the whole observation path
depends on (plan D2, D5). The comment predates the change and was left as it was.

Not a correctness defect: the production build compiles no `close_admission`, so
the sentence stays true for a shipped binary. It is wrong for the build the gate
runs, and it is the comment a later reader uses to decide that the `else` branch
is dead.

Remediation: qualify it ("closed only by the `test-harness` seam") or delete it —
hard rule 7 would delete it.

### 3 — note · the e2e does not pin "counted exactly once"; only the unit test does

Evidence, from mutation 2 re-run here. With `Server::take_finished_acceptor`
mutated to hand over a handle while leaving the field populated,
`fah-http --lib` failed on the intended assertion —

```text
server::tests::a_returned_acceptor_is_handed_over_once ... FAILED
a death is handed over once, not on every supervision tick
```

— while `crates/fastadhunter/tests/acceptor_death.rs` **passed 3/3**.

The cause is the oracle's shape
(`crates/fastadhunter/tests/acceptor_death.rs:146-169`): the poll loop breaks on
`died >= 2` and then asserts `counted == 2`. An acceptor re-counted on every tick
reaches 2 just as fast as two acceptors counted once each, so the two are
indistinguishable at the moment the loop stops. The plan predicted exactly this
split (§7.5 mutation 2 names §7.2.2, not §7.4), so it is not a deviation — but
§Tests above claims the boot "asserts `tasks_died == 2` exactly, so a third death
fails the test rather than passing unnoticed", and that only holds for a death
arriving *before* the loop breaks.

Remediation, if the property is wanted on the wire: after the break, sleep past
one supervision tick (`TELEMETRY_POLL`, `crates/fastadhunter/src/main.rs:47`) and
re-read telemetry, asserting the figure did not move.

Second, smaller point on the same file: both boots resolve `RESOLVED_HOST` for
step 1 and step 5, so step 5 is very likely served from cache. It still proves
the UDP listener, the pipeline and the runtime are alive, which is the criterion
— but it exercises less than step 1 did, and the file does not say so.

### 4 — note · without `--all-features` the new e2e fails with mutation 5's exact signature

Verified by running it:

```text
cargo test -p fastadhunter --test acceptor_death
a_dead_http_or_https_acceptor_... FAILED  both acceptors must be counted within 35s; tasks_died = 0
a_dead_api_acceptor_...          FAILED  the API acceptor's death must be logged within 35s
finished in 38.33s
```

Those two lines are character-for-character what mutation 5 (the acceptor half of
`reap_dead_tasks` deleted) produces. So the strongest piece of evidence this task
has and a plain build-configuration mistake are indistinguishable from the test
output, and the cost of the mistake is about 70 s of timeouts.

Root `CLAUDE.md` §Quality gates does mandate `--all-features` and explains why;
`plan/CLAUDE.md` §Mandatory steps step 3 and its algorithm block still say
`cargo test --workspace`, which is the command a session following that file
alone would run. The precedent this seam follows (`e2e_https.rs`) at least names
`FAH_SECURITY_ALLOW_SKIP` in its message.

Remediation: name the feature in the two timeout messages — "…; if this build was
not made with `--all-features`, the kill seam is compiled out". No skip path, no
`cfg` on the test file.

### 5 — note · `JITTER_ALLOWANCE = 4` is defensible; the residual blind spot and the duplication are not stated

Judged on the axis the tests exist for. A leaked allocation per operation shows
as +64 (`REQUESTS`, `FORWARDS` and `HANDLES` are all 64); the observed jitter was
±1 in both directions (3201→3200 and 1280→1281, §Collateral). An allowance of 4
sits 16× below the signal and 4× above the observed noise, and the one-sided form
(`last <= previous + 4`) is the right shape for jitter that was proved
bidirectional — a bare `<=` would have failed on `proxy_alloc`. The per-operation
ceiling assertions were left untouched, so the absolute bound still binds. The
change is sound.

Two things the review file does not say:

- **What stopped being caught.** The old `assert_eq!` caught growth of +1 per
  batch; the new oracle does not catch growth of 4 or less per batch. In
  `intercept_alloc.rs` only the last two of `BATCHES` batches are compared, so a
  path that adds up to 4 allocations per batch indefinitely now passes. The
  ceiling assertions bound it in absolute terms but would not fire within a run
  of this length. Narrow, but it is the difference between "steady" and "not
  growing fast".
- **`const JITTER_ALLOWANCE: usize = 4;` is declared four times** —
  `crates/fah-http/tests/intercept_alloc.rs:188`,
  `crates/fah-http/tests/proxy_alloc.rs:148`, and twice in
  `crates/fah-dns/tests/forward_alloc.rs` (`:106` and `:217`). The two `fah-dns`
  copies are in the same file and could be one file-level const (principle 4);
  the cross-crate ones cannot share without a dependency nobody wants.

No remediation needed for the value. Optional: hoist the two `forward_alloc.rs`
copies to one file-level const.

## Verification performed by this review

### Gates, run on the final tree

```text
cargo fmt --all -- --check                                                 clean
cargo clippy --workspace --all-targets --message-format=short -- -D warnings   clean
cargo clippy --workspace --all-targets --all-features                      clean
cargo test --all-features --workspace                                      green (run twice)
```

The extra `--all-features` clippy run is not in the gate list; it was added
because the whole D5 seam is invisible to the gate as written.

`tests/acceptor_death.rs` reports **3 passed**, not 2 — the third is
`common::free_udp_port_is_bindable_on_both_protocols`, which every integration
binary in this crate compiles in. Not a defect; noted because §Tests lists two.

### Mutations, re-run

| # | Applied as | Result |
| - | ---------- | ------ |
| 1 | `Server::take_finished_acceptor`'s finished arm returns `None` | `fah-http --lib`: `a_returned_acceptor_is_handed_over_once` **and** `shutdown_is_safe_after_the_handle_was_taken` failed ("a closed admission must end the accept loop"); e2e boot 1 failed (`tasks_died = 1`), boot 2 green. Stronger than the plan predicted — §7.2.3 shares the `handed_over` helper |
| 2 | same arm hands over a fresh finished handle, field left populated | `fah-http --lib`: only the second-call assertion failed, as predicted. e2e **passed 3/3** — see finding 3 |
| 3 | `record_task_death()` skipped when `death.name.ends_with("acceptor")` | boot 1 failed (`tasks_died = 0`), boot 2 green. Exactly as predicted |
| 4 | `std::process::exit(0)` after an acceptor death is logged | both boots failed. Boot 2 at step 5 — `no DNS response for alive.example.com within 5s` (`common/mod.rs:306`); boot 1 at step 3, the telemetry GET (`common/mod.rs:496`), because the process is gone before the next poll. The plan predicted step 5 for boot 1; the test catches it one step earlier |
| 5 | `main.rs:853-874` deleted — the whole acceptor half of `reap_dead_tasks` | both boots failed (boot 1 `tasks_died = 0`; boot 2 "must be logged within 35s"); `fah-http --lib` 91/91, `fah-api --test api` 133/133, `fastadhunter --bin` 37/37 all green. The claim that matters most holds exactly |

Literal note on mutation 2: "return the handle without removing it" is not
expressible — `JoinHandle` is not `Clone` and the field holds
`Option<JoinHandle>`. The nearest faithful form, used here, hands over a
different finished handle and leaves the original in the field, which breaks the
same property.

### The rare `fah-dns --lib` failure — not reproduced, name still uncaptured

Attempted: 2 full `cargo test --all-features --workspace` runs, 3 standalone
`-p fah-dns --lib` runs (221/221 each), 3 more with `--test-threads=32`
(221/221 each). All green. The name remains unknown.

## Axes checked and found acceptable

| Axis | Verdict |
| ---- | ------- |
| **Plan compliance** | Every unit in §4 is present and matches the **rewritten** D5, not the original: the sentinel check is a `cfg` **statement** at the top of `reap_dead_tasks` (`main.rs:846-847`), there is no `select!` arm, no 200 ms ticker, and `run()` (`main.rs:797-808`) is unchanged. D1's `take_finished_acceptor` is the six-line shape the plan describes, on all three types. D4's two changes are both present. D6 and D7 hold. Every §9 acceptance row is closed by a test that exists and, per the mutations above, fails when its subject is removed. No out-of-scope boundary was crossed: nothing restarts, DNS's fatal path is untouched, no new counter or telemetry field |
| **Correctness** | Double-counting is structurally impossible — `take()` empties the slot and the guard requires `Some`, so tick N+1 sees `None` (mutation 2 is the negative control). A live acceptor is never handed over: `is_finished()` gates the arm, and `a_running_acceptor_is_not_handed_over` covers it on all three types. `shutdown()` after a take is a no-op on all three; the HTTP test additionally proves the `stop` watch still flips and the domain threads still join (`server.rs:697-729`). The `ApiServer` semaphore hoist changes nothing about admission — same `MAX_CONNECTIONS`, same acquire-before-accept order, same per-connection `Arc::clone`; only the construction site moved from inside the spawned future to `serve` (`fah-api/src/server.rs:76-85`). Without the feature the seam is absent from the type, the struct and the function: `Engine` has no `kill` field, `kill_request`, `trip_kill_sentinel` and `close_admission` do not exist, and `reap_dead_tasks` is what it was plus the acceptor half. `death_of` awaits only handles already reported finished, so the supervision arm cannot block |
| **Architecture** | `fah-http` and `fah-api` stay siblings that do not import each other; `main.rs:855-870` is the only place that knows all three, and `tests/layering.rs` is green. `death_of` was extracted from `reap` rather than written beside it, so one function decides what a death is. No new trait, no new channel, no new task, no new dependency. The six-line `take_finished_acceptor` is repeated per type rather than abstracted — correct here: a shared trait would have to live in an L1 crate and would buy nothing (principles 15, 16) |
| **Performance** | Nothing runs per query or per request. The added work is three `is_finished()` atomic loads on a tick that already fires every 10 s. `Vec::new()` for the acceptors does not allocate until something dies, and a death is a once-per-process event. Under repeated failure there is no repetition to be had — the slot empties, so a dead acceptor costs one `None` match per tick forever. No formatting, no logging and no syscall on any repeating path outside the feature, where it is one `Path::exists` per tick until the request is consumed. No bench needed; no hot path touched |
| **Memory** | Three `Option` discriminants and one additional `Arc<Semaphore>` handle in `ApiServer` — the semaphore itself was already allocated, only its owner changed. Nothing grows with uptime or traffic: the acceptor slots are emptied, never filled. Under the feature, `KillRequest` holds a small `Vec<String>` and a `PathBuf`, dropped the first time the sentinel is seen |
| **Rust quality** | No `unsafe`, no new `unwrap` or `expect` outside tests, no clone that is not an `Arc` handle. `take_finished_acceptor(&mut self)` is the minimum ownership the operation needs, and matching on `&self.handle` before `take()` avoids taking-and-restoring. `shutdown(&self)` kept its signature on `TlsServer` and `ApiServer` (`as_ref()`), so no caller changed. `death_of` is cancellation-safe in the only way that matters here — the handle it awaits is already complete. `Send`/`Sync` unchanged |
| **Tests** | Three per type in-crate plus two boots on the wire, and the mutations show they fail for the right reasons rather than passing by construction. The per-type tests are not fragile about timing: `handed_over` polls at 10 ms up to 2 s with an assertion that names the expected behaviour, and the 35 s e2e budget is a timeout with about 10 s typical, as designed. The poke-and-ignore construction is correct — treating a refused connect as a failure would fail the test at the moment it succeeded. The gaps are findings 3 and 4 |
| **Regression** | The existing shutdown tests are unmodified (`git diff` on `server.rs` is `+101 -0`) and green. `/health` is unchanged. `counters.tasks_died` widens in meaning, which is the point, and all five documents that describe it were updated together. No API field added or removed; `API.md`'s shape section is untouched. The `ApiServer` field change is private. Nothing in the shipped build differs except the three `is_finished()` loads per tick |
| **Documentation** | `CONTEXT.md:543-561`, `ARCHITECTURE.md:349-355`, `API.md:333-349` and `requests/health.http:10-13` match the code as built and contradict nothing else in those files: the acceptors join the supervised set, the DNS listeners stay the only fatal path, and "the resolver keeps answering" is the property both boots prove. The `engine-card.tsx:15-19` comment is the owner-approved text, unchanged in wording. `requests/telemetry.http` is correct except for finding 1 |

## Status

**PASS WITH DEFERRED FINDINGS** — no blocker. Finding 1 is a one-character
documentation fix and is recommended before the p3-11 soak, since that line is
what an operator reads when the counter moves. Findings 2–5 are notes; deferring
them is a recommendation, not a decision.

## Findings resolved — 2026-09-14, after the review

The owner approved 10c with no architectural defect and asked for four fixes
before commit. All four are applied in this changeset; the architecture of 10c
was not touched.

| # | Severity | Fix | Where |
| - | -------- | --- | ----- |
| 1 | should-fix | `port 8443` becomes `port 8444` for the HTTPS acceptor | `requests/telemetry.http:123` |
| 2 | note | the line `// The semaphore is never closed, so acquire cannot fail.` is deleted; the two lines above it, which are still true, stay | `crates/fah-api/src/server.rs:130-131` |
| 3 | note | the e2e now pins exactly-once on the wire, and says what step 5 really proves | `crates/fastadhunter/tests/acceptor_death.rs` |
| 4 | note | both timeout messages name `--all-features` | `crates/fastadhunter/tests/acceptor_death.rs:163`, `:232` |
| 5 | note | the two in-function `JITTER_ALLOWANCE` copies became one file-level const; the value stays 4 and was not re-opened | `crates/fah-dns/tests/forward_alloc.rs:27` |

### Finding 3, as built

`SUPERVISION_TICK: Duration = Duration::from_secs(10)` joins the file's other
constants. After boot 1's poll loop breaks and the `counted == 2` assertion
passes, the test sleeps `SUPERVISION_TICK + POLL_INTERVAL`, re-reads
`/api/v1/telemetry` and asserts `tasks_died` has not moved. One tick is enough:
the mutation this closes re-counts on *every* tick.

The cache point went into the assertion message of step 5 in both boots rather
than into a comment — hard rule 7 and `.claude/hooks/no-rust-comments.sh`
forbid comments in `.rs`, and string literals are the repo's idiom for this.
Both messages now say the answer is probably a cache hit, which proves the UDP
listener, the pipeline and the runtime are alive rather than that the upstream
path was re-exercised.

### Mutation 2, re-run against the corrected test

This supersedes the mutation-2 row in §Verification performed by this review,
where the e2e passed under the mutation.

```text
a_dead_http_or_https_acceptor_is_observed_and_dns_keeps_answering ... FAILED
assertion `left == right` failed: a death is handed over once, not on every
supervision tick. The poll above stops the moment the figure reaches two, ...
```

The unit test `a_returned_acceptor_is_handed_over_once` still fails on the same
mutation, so the property is now pinned in both places. Mutation 5 was re-run
after the edits and still kills both boots.

### Gates after the fixes

```text
cargo fmt --all -- --check                                      clean
cargo clippy --workspace --all-targets -- -D warnings           clean
cargo clippy --workspace --all-targets --all-features           clean
cargo test --all-features --workspace                           green
cargo test --all-features -p fastadhunter --test acceptor_death 3 passed, 35.00s
```

The e2e went from 24.5 s to 35.0 s for the pair — the cost of boot 1 waiting one
extra supervision tick. `OBSERVATION_BUDGET` is unchanged at 35 s and is still a
timeout, not a runtime.

**Status after the fixes: PASS.** No finding is left open. No commit, no push, no
tag; no task or phase moved.
