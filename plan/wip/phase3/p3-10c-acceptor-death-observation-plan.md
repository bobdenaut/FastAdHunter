# P3-10c — HTTP, HTTPS and the API Server Report Their Own Death — Implementation Plan

**Task:** [`p3-10c-acceptor-death-observation.md`](p3-10c-acceptor-death-observation.md)
**Phase:** 3 · **Depends on:** p3-04 · **Source:** p3-10 Track A, A9 · **Model:** Fable

The task file leaves the mechanism open on purpose. This plan picks it, shows
why the three routes it listed lose, and settles everything else before code.
The route (D1) and the test seam (D5) were reviewed and approved by the owner on
2026-09-14; D5 was rejected in its first form and rewritten. Nothing here is
left for the implementer to decide.

## 1. Objective and scope

When the HTTP acceptor, the HTTPS acceptor or the API accept loop ends without
being told to, the binary notices within one supervision tick, counts it through
`record_task_death` and logs it once at `error` with the acceptor named. The
resolver keeps answering. Nothing restarts.

In scope: `fah-http` (`Server`, `TlsServer`), `fah-api` (`ApiServer`),
`crates/fastadhunter/src/{main.rs,supervisor.rs}`, two `Cargo.toml` feature
lines, tests in five places, three documents (separate approvals).

## 2. Existing code paths — verified, not assumed

| Fact | Site |
| ---- | ---- |
| DNS is watched by a different mechanism: a fatal `mpsc`, one sender per listener, selected in the run loop, and it **ends the process** | `crates/fah-dns/src/server.rs:84`, `:109`, `:117`, `:128`; `main.rs:765-771` |
| The run loop returns on exactly two things — shutdown, and `dns.fatal()`. The supervision arm loops | `crates/fastadhunter/src/main.rs:760-772` |
| Supervision already exists: a 10 s tick, `supervisor::reap`, `record_task_death`, one `error` line naming task and cause | `main.rs:762-785` |
| `reap` classifies correctly today: `Returned`, `Panicked(msg)` off `JoinError::try_into_panic`, `Cancelled` | `crates/fastadhunter/src/supervisor.rs:38-70` |
| `Supervised` **owns** its handle and `reap` consumes it (`task.handle.await`) | `supervisor.rs:5-13`, `:45-47` |
| HTTP acceptor: `Server::handle: Option<JoinHandle<()>>`, spawned in `serve()` `:86` and `serve_domains()` `:124`, aborted in `shutdown()` `:150` | `crates/fah-http/src/server.rs` |
| `Server::shutdown` does three things, only one of which is the abort: abort, flip the `stop` watch, join the domain threads | `crates/fah-http/src/server.rs:149-159` |
| HTTPS acceptor: `TlsServer::handle: Option<JoinHandle<()>>`, spawned in `start()` `:67`, aborted in `shutdown(&self)` `:84` | `crates/fah-http/src/tls_server.rs` |
| Both HTTP and HTTPS run the **same** `accept_loop` | `crates/fah-http/src/server.rs:171-203`, `tls_server.rs:66-72` |
| `accept_loop` returns on exactly one thing: `permits.acquire_owned()` failing, which happens only if the semaphore is closed. An accept error is logged at `debug` and the loop continues | `crates/fah-http/src/server.rs:177-201` |
| API: `ApiServer::accept_loop: JoinHandle<()>` (not `Option`), spawned inside `bind()` `:75`, aborted in `shutdown(&self)` `:101` | `crates/fah-api/src/server.rs:46`, `:75`, `:100-102` |
| The API's accept loop has the same single return path, and its semaphore is created **inside** `accept()`, not stored | `crates/fah-api/src/server.rs:104-112` |
| DoH is a route on the API server, so a dead API acceptor kills DoH silently | `crates/fah-api/src/routes.rs:125` |
| The e2e harness spawns the **real binary as a child process** — no test can reach an internal handle | `crates/fastadhunter/tests/common/mod.rs:142-180` |
| There is a precedent for a `test-harness`-gated seam in this binary, env-driven, with a loud warning | `main.rs:968-1000` |
| `counters.tasks_died` is the supervisor's tally and is already on `/api/v1/telemetry` | `API.md:229`, `:309-317`; `registry.rs:249-251` |
| Three documents state in prose that these acceptors are **not** supervised | `API.md:309-317`, `ARCHITECTURE.md:348-355`, `CONTEXT.md:542-553` |

## 3. Design decisions

### D1 — Route: the type keeps the handle for stopping and hands it over once it is dead

**Decided, and this is the decision the task file says the plan owes.** Each of
the three types gains one method:

```rust
pub fn take_finished_acceptor(&mut self) -> Option<JoinHandle<()>>
```

It returns `Some` only when `handle.is_finished()`, and takes the handle out of
the type when it does. The binary calls it for all three on the supervision tick
it already runs, `await`s whatever it is handed, and classifies it with the code
that already classifies every other task death.

Nothing else moves. Every `shutdown()` keeps its body, its receiver type and its
tests. The handle stays where it is for the whole life of a healthy acceptor,
and changes owner only at the moment it stops being useful for stopping
anything.

**Why the three routes in the task file lose.**

- **Route 2, a death channel per acceptor (the task file's own
  recommendation) — rejected on a defect, not on taste.** A `let _ = tx.send(…)`
  placed after `accept_loop(…).await` **never runs when the task panics**: the
  unwind leaves the task at the panic point and skips the rest of the body.
  Panic is one of the two unplanned ends these loops have (D2), and the channel
  would be blind to it. Making the channel panic-proof means a
  drop-guard, and a drop-guard also fires when the task is **aborted** — which
  is exactly what `shutdown()` does — so a planned stop would be reported as a
  death unless a second piece of state (a flag set before the abort) tells the
  guard to stay quiet. That is two new mechanisms and a new failure mode
  (forgetting to arm the flag) to obtain information the `JoinHandle` already
  carries exactly: `JoinError::try_into_panic` distinguishes panic from
  cancellation, and `classify()` already turns it into a `Cause`.
- **Route 1, move the handle to the binary — rejected on blast radius.**
  `TlsServer::shutdown` would become an empty function, its four tests would
  have to own handles instead of calling it, and `Server::shutdown`'s ordering
  changes: today it aborts the acceptor *before* flipping the `stop` watch and
  joining the domain threads, and the abort would move to a different place in
  `Engine::shutdown`. Redistributing lifetime ownership across a crate boundary
  to gain what D1 gains without it.
- **Route 3, a `Supervised` that observes rather than owns — rejected on
  borrows.** It would have to hold `&mut JoinHandle` borrowed out of
  `self.http`, stored in a `Vec` owned by the same `Engine` that `reap` takes
  `&mut` of. Disjoint-field borrows do not survive being parked in a collection.
  It also changes a supervisor that serves six tasks correctly, to serve three
  that do not fit.

**Costs of D1, stated plainly.** `take_finished_acceptor` is an unusual shape —
"give me the handle, but only if it is dead". It is three lines per type and
directly testable in the crate that owns it. Detection is up to one tick late
(10 s), which is what every other supervised task already accepts.

### D2 — What an unplanned end actually is here

Read before deciding, because it changes what the tests can do: `accept_loop`
(`server.rs:177`) and the API's `accept` (`server.rs:106`) both return on
exactly one condition — the connection semaphore being closed. An accept error
never ends either loop; it is logged and the loop continues (this is unlike
`fah-dns`, which escalates consecutive accept errors to fatal through
`RetryPolicy`).

So an acceptor's unplanned end is one of:

| Cause | Reaches | Reported as |
| ----- | ------- | ----------- |
| Panic inside the loop | the `JoinHandle` | `Cause::Panicked(message)` |
| Semaphore closed (the loop's only `return`) | the `JoinHandle` | `Cause::Returned` |
| Aborted by something other than `shutdown()` | the `JoinHandle` | `Cause::Cancelled` |

All three are reported. `Cancelled` cannot occur in production today — only
`shutdown()` aborts, and by then the run loop has already returned — so it costs
nothing to keep it and it removes a silent case if that ever changes.

### D3 — Where it is polled, and reusing the one classifier

**Decided.** `Engine::reap_dead_tasks` (`main.rs:774-785`) grows a second half:
after reaping `tasks` and `stats_schedulers`, ask the three acceptors, in this
order — HTTP, HTTPS, API — and turn whatever comes back into the same `Death`
the rest of the loop already produces.

`supervisor.rs` gains one function, extracted from `reap` rather than written
beside it, so there is one place that decides what a death is:

```rust
pub async fn death_of(name: &'static str, handle: JoinHandle<()>) -> Death
```

`reap`'s body becomes a call to it. Behaviour is unchanged and its six existing
tests stay green as written.

Names, which are what an operator reads: `"HTTP acceptor"`, `"HTTPS acceptor"`,
`"API acceptor"`. They go through the existing `record_task_death` and the
existing `error` line — no new counter, no new log shape, no new telemetry
field. `counters.tasks_died` widens to include these three, which is the point
and which is why the three documents in §8 have to change.

### D4 — `ApiServer` is brought into the same shape as its two siblings

**Decided.** Two small changes, both about symmetry rather than only about this
task:

1. `accept_loop: JoinHandle<()>` becomes `Option<JoinHandle<()>>`, so the handle
   can be taken at death. `shutdown(&self)` keeps its signature and aborts
   through `as_ref()`.
2. The connection semaphore moves out of `accept()` and into the struct
   (`slots: Arc<Semaphore>`), passed into `accept()` as an argument — which is
   how `fah-http` and `fah-http`'s TLS server already hold theirs
   (`server.rs:42`, `tls_server.rs:21`).

Point 2 is what makes an `ApiServer` death reachable from a test at all (§7.3),
and it removes the asymmetry that made this listener the odd one out. Cost: one
`Arc<Semaphore>` field, already allocated, just held.

### D5 — The test seam: close that acceptor's admission, when the test says so

**Decided, owner-approved 2026-09-14, after a first draft was rejected.** The
acceptance criterion "the process still answers DNS afterwards, proved by a
test" cannot be met without a way to end an acceptor in a running binary: the
e2e harness spawns the real executable as a child (`common/mod.rs:142-180`) and
nothing production-reachable ends those loops. p3-10's A4 recorded the same
wall.

**What the first draft got wrong, kept here so it is not proposed again.** It
killed the acceptor by calling the type's own `shutdown()`. That fails twice:
`Server::shutdown` also flips the `stop` watch and joins the domain threads
(`server.rs:149-159`), so the test would not be isolating an acceptor failure at
all, and an abort produces `Cause::Cancelled` — an intentional stop, which is
the opposite of the thing under test.

**The seam, as approved.** Two `test-harness`-gated environment variables and
one gated method per type:

```rust
#[cfg(feature = "test-harness")]
const KILL_ACCEPTOR_ENV: &str = "FAH_TEST_KILL_ACCEPTOR";      // "http", "https", "api", comma-separated
#[cfg(feature = "test-harness")]
const KILL_WHEN_ENV: &str = "FAH_TEST_KILL_ACCEPTOR_WHEN";     // path of a sentinel file

#[cfg(feature = "test-harness")]
pub fn close_admission(&self) {
    self.permits.close();
}
```

`close_admission` closes **only** that acceptor's connection semaphore. Nothing
else is touched: the `stop` watch is untouched, the domain threads keep running,
in-flight connections keep their permits and finish normally. The accept loop
then ends through its own single production `return` (D2), so the reported cause
is **`Cause::Returned`** — a real unplanned end, not a cancellation.

**The test decides when.** `Engine::run` gains, under `cfg(test-harness)`, a
200 ms interval arm that checks whether the sentinel path exists and, the first
time it does, calls `close_admission()` on each named acceptor and logs a
`warn!` naming them. The env vars are read once when `run()` starts. This is why
the sentinel exists rather than a kill-at-boot: the test has to prove DNS worked
**before** the death for §7.4's last step to mean anything, and a boot-time kill
makes that ordering impossible.

The watcher is an arm of the existing loop, not a task, and deliberately not a
`Supervised` one: a supervised watcher that returns after firing would itself be
reaped as a death and inflate `counters.tasks_died`, which is the number §7.4
asserts on.

**One behaviour the test has to account for**, verified in the loop's shape
(`server.rs:177-181`): the permit is acquired **before** `accept()`, so an idle
acceptor is parked inside `accept().await` and closing the semaphore does not
wake it. It ends on its next trip round the loop. The test therefore opens one
throwaway TCP connection to that port after tripping the sentinel — a plain
connect is enough, the dispatch that follows it is irrelevant.

**Shipped builds carry none of this.** Every piece is behind
`#[cfg(feature = "test-harness")]`; `fah-http` gains the feature (it has no
`[features]` section today), `fah-api` already has one, and the binary's
existing `test-harness` feature forwards to both. The release build is
`cargo build --release --locked -p fastadhunter`, which enables no features —
the warning already written in `crates/fastadhunter/Cargo.toml:15-21` ("Never
pass `--all-features` to a release build") covers this seam too.

### D6 — No restart, no exit, no new run-loop arm in a shipped build

**Decided.** The supervision tick's arm keeps its shape: it calls
`reap_dead_tasks().await` and loops. The only two returns in `run()` stay
shutdown and `dns.fatal()`. The one arm added by D5 is `cfg(test-harness)` and
is absent from every shipped build. This is the property the owner chose the destination
for on 2026-09-13 — a dead dashboard acceptor must not take the resolver with
it — so it is not re-opened here.

A dead acceptor is left dead. The listening socket is released when the loop's
task is dropped, so the port stops accepting; clients get a refusal rather than
a hang. Recovery is a container restart, exactly as with every other supervised
task.

### D7 — What this does not watch

The HTTP allocation-domain threads (`domain::spawn_domain`, `std::thread`) are
not `JoinHandle<()>` tasks and are not covered. Their death is a different
mechanism (thread join at shutdown, `server.rs:154-158`) and a different
question, adjacent to p3-10 A10. Recorded here so the review does not read the
omission as an oversight.

## 4. File-by-file changes

1. **`crates/fastadhunter/src/supervisor.rs`** — extract `death_of(name,
   handle) -> Death` from `reap`; `reap` calls it. No behaviour change.
2. **`crates/fah-http/src/server.rs`** — `take_finished_acceptor(&mut self) ->
   Option<JoinHandle<()>>` on `Server`, plus the `cfg(test-harness)`
   `close_admission(&self)`. `shutdown()` untouched.
3. **`crates/fah-http/src/tls_server.rs`** — the same two methods on
   `TlsServer`. `shutdown(&self)` untouched.
4. **`crates/fah-http/Cargo.toml`** — a `[features]` section with
   `test-harness = []`; the crate has none today (D5).
5. **`crates/fah-api/src/server.rs`** — `accept_loop` becomes
   `Option<JoinHandle<()>>`; `slots: Arc<Semaphore>` becomes a field and an
   argument to `accept()`; `take_finished_acceptor(&mut self)`; the
   `cfg(test-harness)` `close_admission(&self)`; `shutdown(&self)` keeps its
   signature (D4).
6. **`crates/fastadhunter/Cargo.toml`** — the existing `test-harness` feature
   gains `"fah-http/test-harness"` beside `"fah-api/test-harness"`.
7. **`crates/fastadhunter/src/main.rs`** —
   - `reap_dead_tasks` asks the three acceptors and reports through the existing
     `record_task_death` + `error` line (D3);
   - the `cfg(test-harness)` sentinel arm in `run()` (D5).
8. **Tests** — §7.

Layering: no crate learns about another. `fah-http` and `fah-api` stay L3
siblings that do not import each other; the binary is the only place that knows
all three (ARCHITECTURE.md, enforced by `crates/fastadhunter/tests/layering.rs`,
which stays green with no change).

## 5. Control flow after the change

```text
Engine::run, every 10 s
  |- supervisor::reap(&mut tasks)                     unchanged
  |- supervisor::reap(&mut stats_schedulers)          unchanged
  |- http.take_finished_acceptor()   -> Some(handle) -> death_of("HTTP acceptor", h)
  |- https.take_finished_acceptor()  -> Some(handle) -> death_of("HTTPS acceptor", h)
  |- api.take_finished_acceptor()    -> Some(handle) -> death_of("API acceptor", h)
  |- for each death: metrics.record_task_death() + error! { task, cause }
  |- loop continues; DNS keeps answering
```

A healthy acceptor returns `None` every tick and keeps its handle. A dead one
yields it once; the next tick returns `None` because the slot is now empty, so a
death is counted exactly once — the same property `reap`'s `swap_remove` gives
the other tasks, and the one §7.2 pins.

## 6. Runtime, concurrency, performance, memory

- **Hot path:** untouched. Nothing here runs per request or per query; the added
  work is three `is_finished()` loads every 10 s.
- **Memory:** one `Option` discriminant per type, one `Arc<Semaphore>` field
  moved (not added) in `ApiServer`. No allocation on any repeating path.
- **Concurrency:** `is_finished()` and `await` on an already-finished handle do
  not block the run loop. The reap runs on the same tick as telemetry, single
  task, no lock.
- **Shutdown:** unchanged in every path. `Engine::shutdown` still calls each
  type's `shutdown()`; a taken handle means the acceptor is already dead and
  `shutdown()` finds `None`, which is the behaviour `Option` already has for a
  never-served listener. §7.2.3 tests that case rather than assuming it.
- **The D5 seam costs a shipped build nothing:** the two env reads, the 200 ms
  arm and the three `close_admission` methods are all behind
  `cfg(feature = "test-harness")`, which the release build never enables.

## 7. Test strategy

### 7.1 `crates/fastadhunter/src/supervisor.rs`

The extraction is covered by the six tests already there. Add one, because the
new entry point is now public: `death_of` on a handle that panicked yields
`Cause::Panicked` with the message, and on a finished-normally handle yields
`Cause::Returned`.

### 7.2 `crates/fah-http/src/server.rs` — HTTP acceptor (in-crate unit)

`accept_loop` ends only when the semaphore closes (D2), and `permits` is a
private field reachable from `mod tests` in the same file — the TLS tests
already reach `server.permits` this way (`tls_server.rs:251`).

1. **`a_running_acceptor_is_not_handed_over`** — bind, `serve`, assert
   `take_finished_acceptor().is_none()`, then `shutdown()`.
2. **`a_returned_acceptor_is_handed_over_once`** — bind, `serve`, close
   `permits`, poll `take_finished_acceptor()` until `Some` (10 ms, up to 2 s),
   `await` it and assert `Ok(())`; assert the second call is `None`. The second
   half is what stops a death being counted on every tick for the rest of the
   process's life.
3. **`shutdown_is_safe_after_the_handle_was_taken`** — bind, `serve`, close
   `permits`, take the handle, then call `shutdown()` and assert it returns
   without panicking; for `Server` specifically, assert it still does its other
   two jobs — the `stop` watch is flipped and the domain threads are joined —
   by running it in the `serve_domains` shape and checking it returns promptly.
   This is the D4 hazard written as a test: after the acceptor dies, `shutdown()`
   finds `None` where it used to find a handle, and that must be a no-op rather
   than an `unwrap`.
4. **The existing shutdown tests stay green as written** —
   `shutdown_lets_an_in_flight_request_finish` `:659` and
   `shutdown_aborts_a_connection_that_outlives_the_drain` `:695`, unmodified. If
   either needs an edit, the route is wrong and the plan has to be revisited
   rather than the test.

### 7.3 `crates/fah-http/src/tls_server.rs` and `crates/fah-api/src/server.rs`

The same four-test shape per type, including the
`shutdown_is_safe_after_the_handle_was_taken` leg. For `fah-api` this means a
new `mod tests` in `server.rs` (there is none today) that binds an `ApiServer`
on port 0 with the smallest `AppStateBuilder` the crate's own test helpers
already build for `tests/api.rs`; if that builder is not reachable from a unit
test, the test moves to `crates/fah-api/tests/api.rs` and uses the existing
harness, closing `slots` through the field the harness can reach. Whichever of
the two it is, the assertions are identical to §7.2.

Note for all three: closing the semaphore does not wake a loop parked in
`accept().await` (D5), so each of these tests opens one throwaway TCP connection
to the bound port after closing, then polls `take_finished_acceptor()`.

### 7.4 `crates/fastadhunter/tests/acceptor_death.rs` — the whole road (needs D5)

New file, **two boots**, so all three acceptors are covered end to end rather
than one being taken as representative. Both go through `common::boot_with` with
`ApiScheme::Https`, a `dns+http+https` config, and two extra env pairs:
`FAH_TEST_KILL_ACCEPTOR` and `FAH_TEST_KILL_ACCEPTOR_WHEN` pointing at a path
inside the test's config directory that does not exist yet.

**Boot 1 — `http,https`.** In this order, and the order is the point:

1. resolve a domain over UDP and assert it answers — the resolver is **known
   alive before** anything dies, so step 5 is a comparison rather than a claim;
2. create the sentinel file, then open one throwaway TCP connection to the HTTP
   port and one to the HTTPS port, so both parked loops take their next trip
   round and end (D5);
3. poll `GET /api/v1/telemetry` every 500 ms for up to 20 s until
   `counters.tasks_died >= 2`;
4. assert the engine log holds both names — `task="HTTP acceptor"` and
   `task="HTTPS acceptor"` — and `cause=returned`, which is what says the loop
   ended through its own return rather than being cancelled. The harness keeps
   the log at `config_dir/engine.log` (`common/mod.rs:166-167`);
5. resolve again and assert it still answers. **This is the criterion the fatal
   path was rejected for**, and with step 1 in front of it, it says the death
   changed nothing for DNS.

**Boot 2 — `api`.** Same five steps, with one difference that is structural and
not a shortcut: telemetry is served *by the acceptor under test*, so
`counters.tasks_died` is unreadable once it dies. Step 3 becomes "poll
`config_dir/engine.log` until it holds `task="API acceptor"` with
`cause=returned`", and the test additionally asserts that a fresh TCP connect to
the API port is refused — the acceptor is really gone, not merely logged about.
Steps 1, 2 and 5 are unchanged, and step 5 is the one that matters most here:
**DoH rides this acceptor**, so this boot is the only place that shows DoH dying
without the resolver dying with it.

Cost: two boots, each waiting up to one supervision tick — roughly 30 s for the
file. That is the price of covering all three acceptors on the wire instead of
arguing that one stands for the others.

### 7.5 Mutation checks — apply, run, revert, record

Results go in the review file; a claim that a test would fail is not evidence.

1. Make `take_finished_acceptor` return `None` unconditionally in `fah-http` —
   §7.2.2 and §7.4 boot 1 must fail.
2. Drop the "take" and return the handle without removing it (leave the field
   populated) — §7.2.2's second-call assertion must fail.
3. Remove the `record_task_death()` call for acceptor deaths in `main.rs`,
   keeping the log — §7.4 boot 1 step 3 must fail.
4. Have an acceptor death end the run loop instead of being reported — §7.4
   boot 1 step 5 must fail: no answer after the death.
5. **Remove the acceptor half of `reap_dead_tasks()` entirely**, leaving the two
   `supervisor::reap` calls — §7.4 boot 1 steps 3 and 4 and boot 2 step 3 must
   all fail. This is the mutation that catches the binary-side wiring being
   absent rather than wrong, which is the failure mode the whole task exists to
   prevent; the per-crate tests in §7.2–7.3 would stay green through it.

### 7.6 Gates

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
```

`--all-features` is what compiles the `test-harness` seam, so §7.4 only runs
under the gate command as written — the same condition the p3-06 full-mode e2e
already lives under (root `CLAUDE.md` §Quality gates). No bench: no hot path.

## 8. Documentation consequences — proposed, not written

**Each needs its own explicit yes** (§Working agreement). Finish the code, list
them, wait.

- **`CONTEXT.md:542-553` §Supervised Task — required in the same change, and
  hard rule 6 does fire here.** The definition ends with "Not supervised: the
  DNS listeners …, the API accept loop and the HTTP acceptor". Two thirds of
  that sentence becomes false, and the list of supervised tasks gains three
  entries. This is a changed term, not a new one.
- **`ARCHITECTURE.md:348-355`** — "The API accept loop and the HTTP acceptor are
  not supervised" becomes false. The surrounding paragraph — observed, not
  handled; no restart, no exit — stays exactly as it is and now covers three
  more tasks.
- **`API.md:309-317`** — `counters.tasks_died` lists the supervised tasks by
  name; the three acceptors join the list. The sentence "A DNS listener dying is
  a different path and does exit the process" stays true and stays.
- **`docs/code-review/phase3/p3-10c-acceptor-death-observation-review.md`** —
  required by §TASK COMPLETION, and that requirement is its own permission. It
  records which route D5 took and the mutation results.
- **CONFIGURATION.md** — not needed: no config key. The seam is a feature-gated
  env var for tests and must never be documented as an operator control.

## 9. Acceptance mapping

| Task-file criterion | Closed by |
| ------------------- | --------- |
| Each acceptor, ended unexpectedly, produces an observed death — counted and logged with the name | §7.2, §7.3 per crate; §7.4 on the wire for all three, boot 1 for HTTP and HTTPS, boot 2 for the API |
| The process still answers DNS afterwards, proved for at least one acceptor | §7.4, both boots, steps 1 and 5 — the criterion asks for one acceptor and this covers three |
| Each test fails when its reporting is removed | §7.5, five mutations, including the one that deletes the binary-side wiring outright |
| `shutdown()` still stops each acceptor and the existing shutdown tests stay green | §7.2.4 unmodified, plus §7.2.3 for the case D4 creates: `shutdown()` after the handle was already taken |
| Gates green | §7.6 |

## 10. Out of scope — binding

- **No restart.** Death is made visible, not survivable.
- **DNS is untouched.** Its fatal path keeps exiting the process; no acceptor
  joins it (the 2026-09-13 decision).
- **No new counter or telemetry field.** Acceptor deaths land in
  `counters.tasks_died` with everything else.
- **The allocation-domain threads are not covered** (D7), and neither is p3-10
  A10's question about the redundant domain-inbox stop path.
- **No accept-error escalation.** Whether these loops should escalate repeated
  accept failures the way `fah-dns` does is a separate question; this task
  observes the end, it does not change when the end happens.
- **No deploy.** Lands in the same build as row 10b, for p3-11's single deploy.
