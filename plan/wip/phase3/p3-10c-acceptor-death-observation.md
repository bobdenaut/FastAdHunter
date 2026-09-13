# P3-10c — HTTP, HTTPS and the API Server Report Their Own Death

**Phase:** 3 · **Depends on:** p3-04 · **Source:** p3-10 Track A, A9 · **Model:** Fable

## Goal

When one of the three acceptors ends unexpectedly, something notices. Today
nothing does, and the process keeps running with a listener that answers
nothing.

## Context

DNS listeners are already covered, and not through `Supervised`: `Server::serve`
clones a fatal sender into each of UDP, TCP and DoT
(`fah-dns/src/server.rs:109`, `:117`, `:128`), and `Engine::run` selects on
`self.dns.fatal()` as one of its three arms (`main.rs:765-771`). A dead DNS
listener ends the run loop and the process reacts.

Three others have nothing:

| Acceptor | Handle | Failure observed by |
| --- | --- | --- |
| HTTP | `Server::handle`, `fah-http/src/server.rs:45`, aborted `:150` | nobody. Predates Phase 3 |
| HTTPS | `TlsServer::handle`, `tls_server.rs:23`, aborted `:85` | nobody. Added by the merge |
| API server | `ApiServer::accept_loop`, `fah-api/src/server.rs:46`, aborted `:102` | nobody. Predates Phase 3. DoH is a route on it |

Each handle is held for stopping and never polled for dying. **A DoT listener
dying is fatal; a DoH listener dying is silent**, because DoH rides the API
server.

The owner decided on 2026-09-13: **all three**, and **not** through the DNS
fatal path. The fatal path lost on blast radius — it ends the run loop, which is
right for a DNS listener and wrong for the dashboard, in a product whose primary
job is answering DNS. Settling only the two Phase 3 touched would have left the
oldest one where it was. The reasoning is in
`docs/code-review/phase3/p3-10-track-a-review.md` §Owner decisions.

## Deadline

**Must land before p3-11's seven-day soak starts.** An acceptor that dies
silently on day three gives a soak that looks clean and measured nothing. That
is the failure this task exists to make visible, and a soak is exactly when it
would happen unseen.

## Design decision — owed by the plan, not settled here

**The decision is "observed death, wired into supervision". The mechanism is
not approved and must be justified in this task's plan before anything is
written.**

The obstacle is concrete. `Supervised` **owns** its handle —
`Supervised { name, handle: JoinHandle<()> }` (`supervisor.rs:5-13`) — but all
three handles are private fields inside types whose own `shutdown()` needs the
same handle to abort it, and `JoinHandle` is not `Clone`. So "put them in
`Supervised`" does not typecheck as stated, and the plan has to pick a route.

Three are on the table:

1. **Move the handle to the binary.** The binary owns it and supervises it;
   `shutdown()` stops aborting. Costs: `Server::shutdown` also flips the watch
   and joins the domain threads, so it cannot simply go away, and
   `TlsServer::shutdown` would become empty. Redistributes lifetime ownership
   across a crate boundary.
2. **A death channel per acceptor, drained by the binary.** Each type keeps its
   handle and its `shutdown()` untouched; the spawned task reports on exit
   through a channel the binary reads. Same shape DNS already uses, with the
   destination being supervision rather than stopping the process. Costs: a new
   channel per type, and the binary grows a place to drain them.
3. **`Supervised` gains a form that observes rather than owns.** Costs: touches
   the supervisor, which currently serves six tasks correctly, to serve three
   that do not fit.

**Recommendation: 2.** It honours the decision without touching `Supervised` or
any `shutdown()`, and it reuses a pattern already in the tree. It is a
recommendation, not an approval: the plan states which route it takes and why,
and the owner approves that before implementation.

Note that 2 is **not** the fatal path the owner rejected. What was rejected is
acceptor death ending the run loop. A channel whose destination is a log line
and a counter is a different thing with the same plumbing.

## Scope

- All three acceptors — HTTP, HTTPS, API — report an unplanned end.
- It reaches telemetry, not only a log: the existing path is
  `record_task_death`, which is what makes a death countable rather than
  something someone has to read a log file to find.
- The process keeps resolving. No route ends the run loop.
- Layering holds: `fah-http` and `fah-api` are L3 siblings and must not import
  each other; the binary wires them (ARCHITECTURE.md, enforced by
  `crates/fastadhunter/tests/layering.rs`).
- Tests, one per acceptor.

## Acceptance criteria

- Each of the three acceptors, ended unexpectedly, produces an observed death:
  counted through `record_task_death` and logged with the acceptor named.
- The process still answers DNS afterwards. A test proves it for at least one
  acceptor — this is the property the fatal path was rejected for.
- Each test fails when its reporting is removed — verified by removing it
  locally and reverting, not by assuming.
- `shutdown()` still stops each acceptor, and the existing shutdown tests stay
  green: `shutdown_lets_an_in_flight_request_finish`,
  `shutdown_aborts_a_connection_that_outlives_the_drain`, and the HTTPS
  equivalent.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --all-features --workspace` all green.

## Out of scope

Restarting a dead acceptor — this task makes death visible, not survivable.
Changing what DNS does; its fatal path stays as it is. Whether the loss of the
redundant domain-inbox stop path matters (p3-10 A10) — that is a separate
judgement and a separate change.
