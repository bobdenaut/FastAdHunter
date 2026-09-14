# p3-10 — post-merge coverage gaps and Phase 3 performance characterization

**STATUS: APPROVED 2026-09-13, nothing executed.** The phase table
(`plan/wip/phase3/CLAUDE.md`, row 10) carries it as `WAITING`, and that row is
the approval. No production code changes.
New harnesses are deliverables in their own right and are not implemented
implicitly; each one needs its own go before it is written.

## Why this exists

Two things happened that no existing task owns.

**The merge reintegrated `main`'s fixes onto Phase 3 code.** Most of that work
is proven by tests that fail loudly, and the merge plan's Step 4 ran them.
Several merge-sensitive paths are not covered by an executable guard, because
the merge either hand-carried a fix into a file that has none, wrote fresh code
under conflict pressure, or added a transport the existing guards do not reach.
Those are Track A.

**The N=2 allocation-domain decision was measured without TLS.**
[project-state.md](../../../docs/project-state.md) line 109 states it flatly:
*"Untested: TLS termination and HTML rewriting — the N decision is re-measured
when Phase 3/4 exist."*
[ADR-0006](../../../docs/decisions/0006-http-allocation-domains.md) lines 82-83
are weaker and conditional — *"if the handshake cost moves the CPU figure,
remeasure N"* — so the ADR does not on its own make the re-measurement owed, and
an earlier draft of this file claimed it did. Phase 3 now exists either way.
That is Track B.

## Boundary — what this task does not own

The **functional** device campaign is not p3-10's — the runbook rows, the probe
scripts, the smoke and testing plans, Android Private DNS, DoH over h2, the 443
steering and the soak watch. None of that moves here.

Who owns it changed on 2026-09-13, when the owner decided not to use the
interception code. `p3-06-after-interception-impl.md` (p3-06b) and
`p3-06-phase3-verification.md` are both `PARKED`, and the part of their scope
that never depended on interception moved to
[`p3-11-verification-sni-scope.md`](p3-11-verification-sni-scope.md): the 443
steering and its rollback, the SNI / DoT / DoH budget rows, the security suite,
Private DNS and a seven-day soak. The ADR-0008 device path — CA install,
intercepted URL blocking, the pinned-app check — is parked with p3-06 and comes
back only with a new decision.

p3-10 owns two things: **capacity and performance characterization**, including
the N sweep under the HTTPS listener on the RB5009 — which no document owns today, it is a
deferred bullet in project-state.md with no task behind it — and **a narrow set
of wiring and coverage gaps found while integrating**, which is Track A.

The second half is deliberate and was nearly cut. Applied strictly, "capacity
and performance only" evicts eight of Track A's ten items, not one: an
allocation ceiling, a counter, a transport's guard coverage and a code read are
none of them performance. The gaps are here because they were found here and
have no other home, and keeping them costs one sentence of scope where
scattering them costs their existence.

**p3-06b contradicts this file, flatly, and that has to be fixed rather than
explained away.** `p3-06-after-interception-impl.md:157`, under §6 Not owed,
says "A new p3-10 task or test plan", and `plan/wip/phase3/CLAUDE.md:26` repeats
it as "No campaign re-run, no p3-10". Neither is qualified.

The *"solely because p3-07…p3-09 landed"* wording sits elsewhere — at `:149`, in
§5 Decision, where it qualifies the campaign re-run and nothing else. An earlier
draft of this section borrowed that qualifier and attached it to the §6 bullet
to make the contradiction disappear. It does not disappear: two documents say
opposite things, and p3-10's existence is the newer decision.

**Both entries need the owner's amendment**, and until they get it a reader who
starts from the phase table is told this file should not exist. That amendment
is a separate edit and is not made by this draft.

## Baseline

| | |
| --- | --- |
| Pre-Phase-3 `main` | tag `main-pre-phase3-merge` = `ebc46f1` |
| Merged tree | landed on `main` 2026-09-13 by fast-forward, `bc49e4e..78238b4`, 105 commits, no merge commit. `eb693e2` is the merge commit inside the branch |
| Captured `main` bench round | `E:/fah-bench-main-r1.txt`, 54 measurements, 2026-09-13 |
| Bench worktree | `../fah-main-bench`, detached at `ebc46f1` |
| Comparison rule | A/B alternated, means and ranges, never a single pair — [measurement-traps.md](../../../docs/measurement-traps.md) |

**The worktree was kept**, by the owner's decision of 2026-09-13, instead of
being removed at the end of the merge plan's Step 4. It is the only pinned
pre-Phase-3 checkout. Recreating it is cheap; re-measuring on a differently
loaded machine is not, so a rebuilt worktree would be a different baseline
rather than the same one. Do not switch its checkout, move it onto a branch or
delete its `target/`; a change of SHA or checkout gets written down here.

## What already exists — do not repeat it

Step 4 of the merge plan closed these. `main` has moved on since — Step 4 ran at
`46c14f7`, the fast-forward landed `78238b4`, and the tip is now `aa1bf30` — but
every commit since is documentation: `git diff --stat 78238b4..HEAD` is four
`.md` files. No code has changed, so re-running any of these is the same
command against the same code.

| Already proven | By |
| --- | --- |
| F1 — DNS-over-TCP ceiling and message bound | three tests, all in the workspace gate |
| F10 — stats flush on clean stop | `crates/fastadhunter/tests/shutdown_e2e.rs` |
| F11 — task supervision wiring | read in `main.rs:769` and `:775-776`; both collections reaped. That is all this row claims. **An earlier draft of this row said no listener is supervised on either side and that Phase 3 changed nothing — both halves were wrong, and F11 was closed here on that basis.** See A9, which is what the reading actually supports |
| `8941770` — idle pool reaper behaviour | `an_idle_upstream_connection_is_reaped_after_the_idle_timeout`, `an_active_upstream_connection_is_reused_across_requests` |
| `refused_claim` / `refused_destination` split | one source, two views; `set_requests_refused` at `main.rs:1140` and `:1598` |
| F3, H1-H3, D1 on the paths that existed on `main` | ceilings in `forward_alloc.rs`, `warm_pipeline_handles_allocate_a_steady_amount`, `proxy_alloc.rs`, plus the bench A/B |
| F2 — UDP in-flight ceiling | `the_configured_tcp_and_udp_ceilings_reach_the_listeners` (`fah-dns/tests/server_integration.rs`) sets `udp_max_inflight = 1` and asserts the second datagram is shed, with `active` and `peak` at 1. The shed path has a real behavioural guard. What has none is the shipped default, which is 0 and therefore inert (F8), and `udp_inflight_cost.rs`, which prints figures and asserts nothing |

## Track A — merge coverage gaps

Ten items. **Not a campaign, and not a blocker for Step 5**: these are gaps in
coverage, not known defects. They divide by how each one can close:

- **Closes by documented decision** — A2, A3, A4, A5, A6, A9, A10. Each has a
  written answer as an accepted outcome: a read recorded, an exclusion named,
  or an observability contract chosen. A3 and A4 also have a test as the other
  branch, but neither is held open waiting for one.
- **Closes only by execution** — A1 needs a new test, A7 a discriminating
  assertion, A8 a measurement. No wording closes these.

Of the three that need execution, **only A1 and A8 need a harness that does not
exist** — the intercepted-path allocation ceiling and the DoH load generator.
A7 adds an assertion to a test that already runs.

Five of the integration audit's own findings — its F1 through F5 — are **not**
imported here. Each already carries a disposition in
`docs/code-review/phase3/main-phase3-integration-audit.md`, with a stated
trigger: an owner decision, a test to add the next time a harness is touched, a
doc line on the next approved pass. They stay there. Copying them would give
each finding two homes, and two homes is how they drift.

Acceptance criteria here are binary, because each item has an answer.

Each item states what is missing and how to tell. **None of them decides the
fix** — a gap that a measurement could close is not the same as a change that
should be made, and p3-10 does not conflate the two (§Rules, 1).

**Where an answer lands.** Several items close by writing something down, which
is only binary if the destination is named. For every one of them the
destination is `docs/code-review/phase3/p3-10-track-a-review.md` — the task's own
review file, which §TASK COMPLETION in `plan/wip/phase3/CLAUDE.md` requires and
thereby permits. **Propagating any of it into a root document — project-state.md
for A5, CONFIGURATION.md for A6, ARCHITECTURE.md for A9 — is a separate edit
needing its own go**, and an item is not held open waiting for that go. Landing
in the review file is what closes it.

### A1 — `intercept.rs` has no allocation ceiling

During conflict resolution `main`'s allocation work was **reapplied by hand**
into the shared `judge` / `emit` functions, and `intercept.rs` was made to
follow. `proxy_alloc.rs` guards the HTTP proxy path; nothing guards the
intercepted path. The reapplication is unverified by any test.

- **Needs:** a new allocation-ceiling test over the intercepted request path,
  shaped like `proxy_alloc.rs`.
- **Accepted when:** the test exists, carries a ceiling, and passes.
- **Harness:** new. Its own go.
- **Closed 2026-09-13.** `crates/fah-http/tests/intercept_alloc.rs`. Four
  batches of 64 warm requests over one intercepted keep-alive session; the
  ceiling is asserted on the last batch and the last two must be equal. The
  pass-through case settles by the third batch (48, 49.6, 50, 50 per request),
  the two blocked cases are flat from the first (25 and 38). Evidence that the
  session really is intercepted rather than spliced: the client's root store
  holds only the FAH CA, so a completed handshake means FAH minted the leaf.

An earlier draft offered a second way to close this — "unless the read shows the
intercepted path never reaches `judge`/`emit`". That read is done and the
escape is shut: `intercept.rs:279` calls `emit`, `:312` calls `judge`. The gap
is real.

### A2 — how many hyper pools exist after the merge

`Proxy::new` (`crates/fah-http/src/proxy.rs:270-272`) is the only site that
configures `pool_max_idle_per_host`. How many `Proxy` instances the binary
creates, and whether each owns one pool that matters, is what this item
establishes — not something to assume before the read.
`8941770` was about held memory: 8 idle connections per host per pool, each
pinning a grown buffer. Phase 3 adds a second listener. If the merge multiplied
`Proxy` instances, the fix's benefit is divided by that factor and only RSS
would ever say so.

Part of the read is already done and is recorded here so nobody repeats it:
`fah-http/src` holds exactly one `Client::builder` (`proxy.rs:269`); `TlsProxy`
carries no pool (`https.rs:34-48`); and the integration audit records the
intercepted path as using a per-connection upstream `Sender` with no pool at
all.

That last point moves the question rather than answering it. If the intercepted
path opens an upstream connection per client connection, then `8941770`'s reuse
does not apply to it, and the cost is a different shape — connection setup per
session instead of held memory. **B1 now carries a row for it**, so the gap this
item found has an owner rather than a mention.

- **Needs:** the remaining read — how many `Proxy` instances the binary creates
  and how many pools that amounts to — then the arithmetic: pools ×
  `max_idle_per_host` × buffer.
- **Accepted when:** the count is written down with its call sites and compared
  against the same count on `main-pre-phase3-merge`. The intercepted path's
  no-pool cost is B1's row, not this item's; A2 closes without waiting for it.
- **Harness:** none. Reading only.

### A3 — F3's ceilings cover one transport out of four

`forward_alloc.rs` pins `Transport::Udp` at every call site — `:232`, `:240`,
`:293`, `:306`. So the per-handle allocation ceilings guard UDP and nothing
else: **TCP, DoT and DoH all pass without one.**

An earlier draft framed this as a DoT gap, which made it look like something
Phase 3 introduced. It is not. TCP is F1's own transport and predates the merge
by weeks; DoT and DoH simply joined an exclusion that was already there. Phase 3
widened the blind spot rather than creating it.

- **Needs:** a decision, not a measurement — extend the test across transports,
  or record which are out of the guard and why.
- **Accepted when:** either the extended test passes, or the exclusion is
  written down naming every transport it covers.
- **Harness:** an extension of an existing test, if the decision goes that way.
  Its own go.
- **Closed 2026-09-13.** Both `forward_alloc.rs` tests loop every case over
  `TRANSPORTS`, against the ceilings that were already there — no transport got
  its own, looser number. `warm_pipeline_misses_stay_under_the_ceiling` needed
  four times the unique names so each transport gets fresh misses. The four
  deterministic cases allocate the same on all four transports (832, 1216, 640,
  1024 over 64 handles), which is the invariant stated rather than assumed:
  `handle` runs after framing, so the transport cannot reach it.

### A4 — two fixes are held by reading, not by a test

Both survived the merge and both were verified by eye. Neither has a test that
covers the call site, so a future change can remove the wiring and leave every
test green.

- **F11's supervisor.** The six `supervisor.rs` tests call `reap()` directly.
  Nothing fails if the select arm at `main.rs:769` is dropped, or if
  `reap_dead_tasks` stops reaping one of its two collections at `:775-776` —
  keeping `self.tasks` and losing `self.stats_schedulers` leaves the stats
  schedulers unsupervised and the suite green.
- **The refusal-counter split** (`28c751d`). `engine.http.refused_*` and
  `listeners.*.refused_*` are two views of one source, fed by
  `set_requests_refused` at `main.rs:1140`. No test asserts that call site
  exists. (This item first named `:1598` as a second call site. It is not one —
  it sits inside `#[cfg(test)] mod tests`, which opens at `:1564`. There is one
  production call site, and the unit test beside it exercises `refusals_of`
  without covering the poll loop that feeds it.)

Neither has ever run on the RB5009: both are 2026-09-12 commits and the
deployed build booted 2026-09-12 01:02 local, hours earlier.

- **Needs:** one test per call site, or a recorded decision that the call sites
  stay uncovered and why.
- **Accepted when:** the tests exist and fail when the wiring is removed —
  verify that by removing it locally, not by assuming — or the exclusion is
  written down with its reason.
- **Harness:** new tests. Their own go.
- **Closed 2026-09-13.** `crates/fastadhunter/tests/wiring.rs`, three tests:
  the run loop still calls `reap_dead_tasks`, that function still reaps both
  collections and still counts through `record_task_death`, and the telemetry
  poll still calls `set_requests_refused` while reading both listeners. Each
  was proven by deleting its own line in `main.rs`, watching exactly that test
  fail, and restoring the file.
- **What these tests are, and are not.** They read `main.rs` as source and
  assert the wiring is present, the way `crates/fastadhunter/tests/layering.rs`
  reads the manifests. They catch a deletion; they do not catch a behavioural
  regression. The behavioural test is not available here: the integration
  harness runs the real binary as a child process
  (`crates/fastadhunter/tests/common/mod.rs:168`), so no test can kill a
  supervised task and watch the count rise. Closing the gap properly needs a
  production seam, which is out of scope for this task.

### A5 — DoT connections are not counted

`dot.rs:152` hands `None` to `tcp::handle_connection` where the TCP listener
hands a `TcpConnectionGauge`. The merge generalised that loop over `Transport`
so DoT could reuse it, and the bound came with it — a DoT oversize close is
still bounded — but the instrument did not. So
`counters.dns_tcp_connections.{peak,closed_oversize}` exclude every DoT
connection.

Those are the counters the F1 follow-up reads to set the final
`tcp_max_connections` default (project-state.md §Risk inventory close-out). The
soak running now is on a pre-Phase-3 build with no DoT at all, so **its figures
are whole**; the gap bites on the first soak of a Phase 3 build, which is when
the default would be set from half the traffic.

- **Needs:** a decision on the observability contract — DoT shares the TCP
  gauge, gets its own, or stays uncounted.
- **Accepted when:** that decision is in the track-a review file. Propagating it
  to project-state.md, next to the F1 follow-up where somebody will actually
  meet it before setting a default from partial counters, is the separate edit
  described above. **Making a counter change is not part of this item** — it is
  production code and needs its own approval (§Rules, 1).
- **Harness:** none. This closes by reading and deciding.

**CLOSED 2026-09-13 — decided: DoT gets its own gauge.** Not shared with TCP,
not left uncounted. "Uncounted" lost on a cost the options did not name: it
would make B2's "peak concurrent DoT connections" unmeasurable, since nothing
else in the binary sees a DoT connection. The counter itself is production code
and left this task for
[`p3-10b-dot-connection-gauge.md`](p3-10b-dot-connection-gauge.md), which p3-11
holds its soak for. Reasoning in
[`p3-10-track-a-review.md`](../../../docs/code-review/phase3/p3-10-track-a-review.md)
§Owner decisions.

### A6 — DoT's connection ceiling is compiled in

`DOT_MAX_CONNECTIONS: usize = 64` is a `const` (`dot.rs:23`) taken as a
`Semaphore` at `dot.rs:63`. `tcp_max_connections` is a config key defaulting to
1024; DoT has no key at all, so changing 64 is a rebuild.

**This item does not claim 64 is wrong, and it does not ask whether 64 is
enough.** It records one fact: the ceiling has no runtime control. Whether the
number is adequate is a device question, so it lives in B2 as its own
measurement row, and whether it then deserves a config key is a later decision
belonging to whoever takes it.

Keeping the two apart is what lets this item finish. Tied to adequacy it could
never close, because B2 is BLOCKED on a deploy decision nobody has taken; as a
recorded observation it closes by reading.

The reading is already done: `[dns.listen]` carries `address`, `port`,
`dot_enabled`, `dot_port` and `doh_enabled`, and nothing else — there is no
ceiling key to set.

- **Needs:** that observation recorded.
- **Accepted when:** it is in the track-a review file. A line in CONFIGURATION.md
  §`[dns.listen]`, where a reader of the DoT settings would meet it, is the
  separate edit. Adequacy is B2's row, not this one.
- **Harness:** none.

### A7 — the two listener counter sets can be transposed without a test noticing

Resolving `main.rs` pushed `spawn_telemetry_poll` to 8 arguments and clippy's
`too_many_arguments` rejected it. The fix grouped the two proxy counter sets
into `ProxyCounterSources { http, https }` — new code written during conflict
resolution, not a merge of two existing sides, and verified by reading only.

**An earlier draft made that struct the finding and asked for a test that
inverts its two fields. The read says no such test can fail.**
`ProxyCounterSources` has exactly one consumer, `main.rs:1132-1140`: both fields
enter one array, are flattened, and their refusals are summed into a single
`RefusalSnapshot` before `set_requests_refused`. A sum does not care about
order, so transposing `http` and `https` there produces identical output.
Nothing downstream of that struct tells the two listeners apart. It stays
recorded as merge-invented code, and the read is the reason **not** to write a
test for it rather than a reason to write one.

The transposition hazard is real one struct further on. `TelemetryAdapter`
keeps the two sets apart (`adapters.rs:250-255`) and `listeners()` maps them
order-preservingly into `ListenerTelemetry { http, https }` (`:301-309`), which
is what `/api/v1/telemetry` serves. Its constructor takes them as two adjacent
positional parameters of identical type — `http` then `https`, both
`Option<Arc<fah_http::ProxyCounters>>` (`adapters.rs:258-262`) — and the binary
passes `proxy_counters.clone()` and the TLS proxy's counters at
`main.rs:618-622`. Swap those two arguments and the dashboard reports HTTPS
traffic under `listeners.http`. Positional arguments are easier to transpose
than named fields, not harder.

The coverage that exists stops just short. `e2e_https.rs:282-297` already
fetches `/api/v1/telemetry` and loops over both listener keys, asserting
`connections >= 1 && requests >= 1 && blocked >= 1` for each. The scenario
drives both listeners, so both keys hold non-zero counters and the loop passes
whether or not the two are transposed. The assertion that follows
(`:298-314`) compares `https.connections` against `https.requests` and only
requires them to differ, which the HTTP side also satisfies. So the data is
already in the test; what is missing is one assertion that discriminates.

- **Needs:** an assertion that ties a counter to the listener that produced
  it — the simplest being a value only one listener can have in that scenario,
  asserted against the key it must appear under.
- **Accepted when:** that assertion exists **and has been shown to
  discriminate**: swap the two arguments at `main.rs:618-622` locally and watch
  it fail. A test assumed to be a net is F7 again.
- **Harness:** none new. `e2e_https.rs` already fetches the telemetry document
  and drives both listeners.
- **Closed 2026-09-13.** The discriminating value is `non_tls`. The scenario
  now sends one plain-HTTP request to the HTTPS port, which `https.rs:153`
  classifies and closes; the plain listener never reads a ClientHello, so it
  cannot produce that counter at all. The test asserts `non_tls == 1` under
  `listeners.https` and `== 0` under `listeners.http`. Shown to discriminate:
  with the two arguments swapped at `main.rs:618-622` the first assertion fails
  with the HTTPS document reading all zeros. The swap was reverted.
- `handshakes_completed` was the other candidate and was rejected: only
  `intercept.rs:158` raises it, so it stays 0 on the shipped splice path and
  the assertion would be blind in exactly the build that ships.

Recorded as a hardening option, **not** owed by p3-10: giving the two
parameters distinct types makes the transposition a compile error instead of a
test's job (engineering principle 11). That is production code and needs its
own approval.

### A8 — DoH and the API's resource budget

`/dns-query` is mounted on the API router (`fah-api/src/routes.rs:125`), outside
the auth layer. Whether DNS-over-HTTPS traffic draws on the same connection
budget as the dashboard and the admin endpoints was the question, and an earlier
draft left it open. **The read settles it: the budget is shared.**

There is one accept loop and one semaphore for the whole API server —
`const MAX_CONNECTIONS: usize = 64` (`fah-api/src/server.rs:33`),
`Semaphore::new(MAX_CONNECTIONS)` at `:107`, the permit taken before
`listener.accept()` so at the ceiling the listener pauses and clients wait in
the kernel backlog. `/dns-query` is a route on that router, so a DoH connection
holds one of the same 64 slots a dashboard connection holds.

Two things follow, and neither is a decision this item takes. Sixty-four
concurrent DoH connections make the dashboard and the admin endpoints wait —
which is the failure that matters, because it arrives exactly when someone is
trying to look at why. And the number is the same 64 as `DOT_MAX_CONNECTIONS`
(A6), reached independently: two encrypted DNS transports, two unrelated
compiled-in constants, one value.

What is **not** established is what the ceiling costs in practice — whether
household DoH traffic comes anywhere near 64 concurrent connections, and what
the admin side sees when it does.

- **Needs:** the read recorded, then a saturation measurement. The conditional
  an earlier draft attached to it — "only if the read confirms a shared
  budget" — has resolved to yes.
- **Accepted when:** the shared-budget finding is in the track-a review file,
  and B1 carries a row for admin-endpoint latency under DoH load at and above
  the ceiling.
- **Harness:** the DoH half of the load generator from §Harnesses, which does
  not exist.

### A9 — three long-lived acceptors lack an explicit failure-observation path

DNS listeners are supervised, and not through `Supervised`. `Server::serve`
clones a fatal sender into each of UDP, TCP and DoT (`fah-dns/src/server.rs:109`,
`:117`, `:128`); `Engine::run` selects on `self.dns.fatal()` as one of its three
arms (`main.rs:765-771`), so a dead DNS listener ends the run loop and the
process reacts.

Nothing equivalent exists for three others. They are not unstoppable and they do
not leak — each is a `JoinHandle` the binary aborts on shutdown. What they lack
is the other half: a path by which their *unplanned* end reaches anyone. The
handle is held for stopping, never polled for dying. Naming all three matters
because only one of them is Phase 3's:

| Acceptor | Handle | Failure observed by |
| --- | --- | --- |
| HTTP acceptor | `Server::handle`, `fah-http/src/server.rs:45`, spawned `:86` and `:124`, aborted `:150` | nobody. **Predates Phase 3** |
| HTTPS acceptor | `TlsServer::handle`, `tls_server.rs:23`, aborted `:85` | nobody. Added by the merge |
| API server | `ApiServer::accept_loop`, `fah-api/src/server.rs:46`, spawned `:75`, aborted `:102` | nobody. Predates Phase 3. DoH is a route on it |

**A DoT listener dying is fatal; a DoH listener dying is silent**, because DoH
rides the API server. The merge did not create this — it added a third unwatched
acceptor to a binary that already had two — but it is the thing F11 leaves open,
and it was hidden here by a wrong claim of symmetry.

Leaving the HTTP acceptor out of the decision would settle two cases of three
and leave the oldest one unaddressed, which is how a gap survives being looked
at.

- **Needs:** a decision covering all three — join the fatal path, join
  `Supervised`, or stay unwatched with that recorded.
- **Accepted when:** the decision is written down for each of the three rows
  above, saying what observes that acceptor's unplanned end, or that nothing
  does and why that is acceptable.
- **Harness:** none to answer it. Any wiring change is production code.

**CLOSED 2026-09-13 — decided: all three report into supervision, never the DNS
fatal path.** The fatal path lost on blast radius: a dead dashboard acceptor
would take the resolver down with it. The wiring is production code and left
this task for
[`p3-10c-acceptor-death-observation.md`](p3-10c-acceptor-death-observation.md),
which p3-11 holds its soak for. **What was decided is the destination, not the
mechanism** — `Supervised` owns its `JoinHandle` while all three handles are
private and needed by their own `shutdown()`, so that task's plan picks a route
and justifies it first. Reasoning in
[`p3-10-track-a-review.md`](../../../docs/code-review/phase3/p3-10-track-a-review.md)
§Owner decisions.

### A10 — what `Rotation` changed about shutdown — a question, not a finding

The merge replaced `Dispatch::Domains` with a `Rotation`, made `Handoff` an
enum, and gave `Server` a `rotation()` accessor so `TlsServer` can hold a copy
(`fah-http/src/server.rs:144`, `:208`, `:251`). That structure is verified.

What is **not** verified is the consequence the integration audit infers from
it: that aborting the HTTP acceptor no longer closes the domain inboxes, so
shutdown now rests on the watch channel alone. That claim is repeated here
because it is worth checking, **not because it has been established** — it was
read from the audit, not re-derived from the code, and no test pins the
behaviour either way.

- **Needs:** a read of the shutdown path with both acceptors present, against
  the audit's inference.
- **Accepted when:** the inference is confirmed or refuted in writing. If
  confirmed, whether it matters is a separate judgement and is not made here.
- **Harness:** none for the read. A test, if the read says one is warranted.

## Track B — Phase 3 performance characterization

A characterization produces numbers, not a verdict. **No pass/fail criteria are
invented here.** Each measurement below names the decision it feeds; a
measurement that feeds no decision does not belong on the list and is not run.

### Two workloads, and they are not interchangeable

The owner's decision of 2026-09-13 splits Track B in two. Every row below is
tagged with the one it belongs to, and the tag is part of the result: a figure
carries its workload the way it carries its corpus and its device.

**W1 — shipped.** The Interception Document's `clients` list is empty, so every
HTTPS connection is peeked for its ClientHello, judged at the SNI, then closed
or spliced byte-for-byte. **The allocation domains never terminate TLS.** They
carry plain HTTP through `Proxy` and splice sessions side by side.

W1 is **not** "no TLS anywhere" — that reading would drop real cost on the
floor. TLS is still terminated in the shipped build, just not on the domains:
the DoT listener on 853 handshakes with every client and mints a CA leaf for
its SNI, and the API server on 8443 terminates for the dashboard and for DoH.
Those are W1 costs and belong in W1 rows.

**W2 — interception on.** A client listed in the Interception Document gets its
TLS terminated on a domain against a minted leaf, and the request is re-issued
upstream over a fresh per-connection `SendRequest` (`intercept.rs:197`,
`:376-390`). This is the path the decision removes from production. It is
measured as **characterization only — the price of turning interception on** —
and never as a figure for the deployed build.

**The rule that keeps the two apart:** a splice number is never evidence about
termination cost, and a termination number is never evidence about the shipped
build. They are different code paths with different allocations; putting one
under the other's heading is the same error as quoting a dev-box figure as an
RB5009 budget.

**Owner decision, taken 2026-09-13: W1 is the gate.** B2's N sweep runs against
the shipped workload, and its result is the verdict on N=2. **W2 stays
characterization-only and cannot move that verdict** — a number measured on a
path production does not execute is not evidence about the build that ships.

The gate is strictly stronger than the measurement it revisits, which is the
reason to be comfortable with it: the 2026-09-07 sweep that set N=2 ran with no
HTTPS listener at all, so W1 adds splice sessions on top of the plain HTTP it
already covered.

### B1 — on the x86 dev box

**Runnable independently of Track A.** Only a measurement that depends on an
unresolved Track A item waits for that item; the rest do not. Gating the whole
of B1 on Track A would park useful benches behind findings that can stay open
for weeks.

| Measurement | Workload | Feeds which decision |
| --- | --- | --- |
| `proxy.rs`'s `https_sni_splice` group — `direct_to_origin` against `through_splice` (`:414-455`, new on the branch, +199 lines) | **W1** | the cost the shipped build actually pays on an HTTPS connection: peek, verdict, then byte copy. This is the splice figure, and it stands for splice only |
| Splice throughput under `splicebench` | **W1** | the throughput and CPU cost of SNI filtering against plain pass-through — the input to deciding whether always-on SNI filtering for every client is acceptable. No threshold is invented here |
| `fah-certs/benches/certs.rs` — leaf minting and handshake cost | **W1 and W2** | whether leaf minting needs a warm path beyond the existing cache. **Live in the shipped build even with nobody intercepted**: DoT mints a CA leaf for the client's SNI on every handshake (`p3-05-dot-doh-listeners.md:19-20`). Report the DoT arm as W1 and any interception arm as W2 — same bench, two workloads, two lines. **Run 2026-09-13**, one round, figures in the B1 file. The bench has no DoT arm and no interception arm of its own — it measures mint, cache hit, prewarm and a Zipf replay — so the split by workload is a reading, not two lines of output |
| DoT and DoH request rate and latency | **W1** | the cost of an encrypted query against a plain one, and how each transport behaves as concurrency rises. This is where the shipped build's TLS termination cost lives — 853 and 8443, not the allocation domains. **Not** whether 64 is the right cap: that is household adequacy and belongs to B2's row |
| Admin-endpoint latency under DoH load, at and above 64 concurrent connections | **W1** | whether DoH saturating the API server's shared 64-slot budget (A8) makes the dashboard unusable, and therefore whether DoH needs its own budget. A8 establishes that the budget is shared; this row is what says whether that matters |
| RSS: burst → idle → collect, plain HTTP and splice separately | **W1** | residual and live heap after a burst, not peak alone. Split by path because they hold memory differently: plain HTTP through `Proxy` keeps idle pooled upstream connections, which is what `8941770` reaps, while a splice session holds two sockets and its `SPLICE_BUF` copy buffers and touches no pool. **The pool question is a plain-HTTP question**, under either workload — see the row below for why |
| `fah-http/benches/intercept.rs` `https_handshake` and `https_h2_download` (422 lines, new on the branch) | **W2** | the per-request cost of termination against pass-through, and the per-session upstream setup — `direct_to_origin` against `spliced` against `intercepted` (`:348-357`). **Characterization only: this is the price of turning interception on, not a figure for the deployed build.** It sets no expectation for the device sweep, and its numbers never appear under a W1 heading |
| Held memory with interception on | **W2** | whether the intercepted path needs upstream connection reuse. It has no pool at all: `intercept.rs:197` hands each connection its own `Sender::handshake`, an `http1`/`http2::SendRequest` per session (`:376-390`), so `8941770`'s reaper never applies to it and the cost is setup per session rather than held memory. Only meaningful if interception is ever switched on |

x86 figures give shape and relative cost. They do not answer a budget question:
PERFORMANCE.md's budgets are RB5009 figures, and the conversion is the measured
**~9× factor**, never a clock reading.

### B2 — on the RB5009, and only there

**BLOCKED — two dependencies, neither owned by this task:**

1. The merged build must be deployed to the router. That decision has not been
   made and is explicitly out of the merge plan's scope.
2. p3-11 must first put the dst-nat 443 rule in place. Without it no HTTPS
   reaches the container, so the TLS arms of the sweep have nothing to measure.
   An earlier draft named p3-06b's re-scope here; p3-06b is `PARKED` since
   2026-09-13 and that work is p3-11's.

**Every row below is W1 — the shipped workload.** The owner settled the sweep's
workload on 2026-09-13: W1 gates, W2 does not.

Worth stating why the question arose at all, so nobody re-opens it by reading
ADR-0006 alone. That ADR asked *"if the handshake cost moves the CPU figure,
remeasure N"*, and under W1 the handshake never happens on an allocation domain.
What the domains carry instead is plain HTTP beside splice sessions — two
sockets, a copy loop and `SPLICE_BUF` in each direction per session. That is a
real load and a real sweep; it is simply not the one ADR-0006 had in mind, and
the decision is that the build which runs outranks the ADR's literal wording.

| Measurement | Workload | Feeds which decision |
| --- | --- | --- |
| N sweep under the HTTPS listener — N=0/2/3/4 | **W1** — owner decision 2026-09-13 | **the only real gate in Track B.** Whether the N=2 choice of 2026-09-07 survives the HTTPS listener, or has to move. project-state.md:109 names this re-measurement as owed; ADR-0006 only makes it conditional. `N=0` is not `N=1`: `NonZeroUsize::new(http_runtimes)` (`main.rs:643`) makes 0 the no-domains shared-runtime path, which is also the rollback-without-rebuild path, so it stays in the sweep as the baseline the decision was made against. **This row cannot answer a 1 Gbit question** — the LAN transfer path caps at ~67–70 MiB/s, about 560 Mbit/s, so it compares the N arms against each other on the workload it can actually drive |
| rustls AES-GCM ms/MiB on the device | **W1 and W2** | **a capacity input for the 1 Gbit decision**, not an answer to it: a CPU-cost figure that says whether encryption alone could keep up at that rate, measured without having to push traffic through a path that cannot carry it. End-to-end 1 Gbit with TLS also depends on the NIC, the forwarding path, memory bandwidth and the scheduler, none of which this row measures. Currently a deferred bullet in project-state.md with no owner |
| DNS p50/p99 under combined plain-HTTP and HTTPS-splice load | **W1** | whether DNS latency under load holds against the N=2 figures already recorded (0.96 / 15.8 ms). The load is the shipped one: DNS answering while the domains carry plain HTTP and splice sessions |
| Held memory after 900 MiB WAN + 15 min, split by path — plain HTTP through `Proxy`, and HTTPS spliced | **W1** | whether the +19 MiB figure for N=2 holds under the workload that ships. The 2026-09-07 figure was taken with no HTTPS listener at all, so what is being asked is what splice adds: two sockets and `SPLICE_BUF` per live session, released when the session ends, against `Proxy`'s idle pooled connections, which `8941770` reaps. **Not** "does `8941770` survive TLS" — splice touches no pool, so that question does not arise on this path |
| Peak concurrent DoT connections under household load | **W1** | whether `DOT_MAX_CONNECTIONS = 64` covers this house, and therefore whether the missing config key (A6) is worth adding. Only the device can answer it: A6 records the absence, this row measures the need |

The 2026-09-07 sweep is the comparison table
([alloc-domains-n-sweep.md](../../../docs/code-review/phase2.6/alloc-domains-n-sweep.md)).
Its LAN transfer pass was **not** a 1 GbE test — the forwarding path caps at
~67–70 MiB/s — and any re-run inherits that ceiling.

## Harnesses — what exists and what does not

| Harness | State |
| --- | --- |
| `crates/fah-http/examples/httpbench.rs` | exists on `main` and on the branch. Run it, do not rebuild it |
| `crates/fah-http/examples/splicebench.rs` | **new on the branch**, 11.7 KB. Whether it covers termination as well as splice is unverified |
| `crates/fah-rules/examples/urlbench.rs` | exists on both |
| `crates/fah-certs/benches/certs.rs` | **new on the branch** — added by `3337aab`, the p3-01 commit — and outside the merge plan's six-crate bench loop, so it had no **post-merge** round until 2026-09-13. An earlier draft of this row said it had had no round at all; that is wrong. `target/criterion/` holds baselines for all four of its benches dated 2026-09-01 and 2026-09-02, from the branch before the merge |
| `crates/fastadhunter/benches/pipeline.rs` | exists, **cannot be built**. `fastadhunter` dev-depends on `fah-api` with `test-harness`; benches link dev-dependencies; the bench profile inherits `release` and drops `debug_assertions`; `fah-api/src/lib.rs` guards that pair with a `compile_error!`. So the DNS hot path has no Criterion coverage at all, and has not had any — this predates the merge and is recorded nowhere else. It is a constraint on Track B, not a row in it: an unbuildable bench feeds no decision. **Do not force `debug_assertions` on to make it build** — that measures a binary that does not ship |
| DoT / DoH load generator | **does not exist.** Writing one is a deliverable with its own go, not a step in a run |
| Intercepted-path allocation ceiling (A1) | **written 2026-09-13** — `crates/fah-http/tests/intercept_alloc.rs`. Shaped like `proxy_alloc.rs`, with the TLS stack on both legs: an rcgen-signed origin the proxy trusts, a `CertStore` CA, and a client that trusts only that CA |
| RSS burst → idle → collect procedure for the splice path (W1) | no runner; the plain-HTTP procedure exists in the soak tooling and may be reusable — unverified |

## Rules this task runs under

1. **No production code change lands.** Findings are recorded; fixes are
   separate, approved changes. The wording matters: A4 and A7 both require
   editing production code *locally* — removing the wiring, swapping the two
   arguments — to prove a test actually catches the failure it claims to catch.
   That edit is the method, and it is reverted. What this rule forbids is a
   production change surviving into a commit.
2. **New harnesses are deliverables, not side effects.** Each is proposed, gets
   its own go, and is written as its own change.
3. **No default is changed** — including the F8 `udp_max_inflight = 0` asymmetry,
   which is the owner's call and predates the merge.
4. Every figure carries corpus, workload and device. Measurements go to
   `docs/code-review/phase3/`, one file per track.
5. The router is not touched. B2's commands are proposed; the owner runs them.

## Deliverables

- `docs/code-review/phase3/p3-10-track-a-review.md` — the ten answers, with
  their evidence, and any test or harness added. An item closed by reading is
  as complete as one closed by a test, provided the reading is written down.
- [`docs/code-review/phase3/p3-10-track-b1-x86.md`](../../../docs/code-review/phase3/p3-10-track-b1-x86.md)
  — the x86 characterization, each number against the decision it feeds.
  **Written 2026-09-13**, six of eight rows measured. It separates what the dev
  box resolves from what it does not, and states that **B1 does not close the W1
  gate** — the splice rows moved 44–101% between two runs of identical code, so
  the gate stays with B2 on the RB5009.
- `docs/code-review/phase3/p3-10-track-b2-rb5009.md` — the device sweep, when it
  is unblocked.
- An amendment to p3-06b §6 and to the phase table in
  `plan/wip/phase3/CLAUDE.md`. Both say plainly that no p3-10 is owed; a
  qualification does not reconcile that with this file's existence, so the
  entries have to change. Separate go, and the owner's call.

## Status

| Track | STATUS | Note |
| --- | --- | --- |
| A1 intercepted-path ceiling | CLOSED | written 2026-09-13: `crates/fah-http/tests/intercept_alloc.rs`, a counting allocator over a real intercepted session (client trusts the FAH CA only, so a completed handshake proves the leaf was minted). Ceilings per request: pass-through 50, blocked script 25, blocked document 38, and the last two batches must be equal so the path cannot creep |
| A2 pool count | CLOSED | pools = `http_runtimes`, unchanged by the merge, so `8941770`'s benefit is not divided. The intercepted path's no-pool cost is B1's row |
| A3 extend the ceiling across all four transports | CLOSED | written 2026-09-13: both `forward_alloc.rs` tests now run every case against `TRANSPORTS`, the same ceilings for all four. The deterministic cases allocate identically on UDP, TCP, DoT and DoH, so the invariant is proven rather than assumed |
| A4 call-site cover for F11 and the refusal split | CLOSED | written 2026-09-13: `crates/fastadhunter/tests/wiring.rs`, three tests, each shown to fail with its own wiring removed and then reverted. **One correction to the item above:** `main.rs:1598` is not a second call site — it sits inside `#[cfg(test)] mod tests` (opens at `:1564`). The refusal split has one production call site, `:1140` |
| A5 DoT connections uncounted | CLOSED | owner 2026-09-13: DoT gets its own gauge. The counter is production code and left for `p3-10b-dot-connection-gauge.md`; p3-11 holds its soak for it |
| A6 DoT ceiling compiled in | CLOSED | `DOT_MAX_CONNECTIONS = 64` is compile-time only and `[dns.listen]` has no ceiling key. Whether 64 suffices is B2's row |
| A7 listener counter sets can be transposed | CLOSED | written 2026-09-13: the hazard is `TelemetryAdapter::new`'s two positional arguments, not `ProxyCounterSources`, whose inversion is unobservable. `e2e_https.rs` now drives plain HTTP at the HTTPS port and asserts `non_tls` is 1 under `listeners.https` and 0 under `listeners.http` — a value the plain listener cannot produce. Swapping the two arguments at `main.rs:618-622` makes it fail; the swap was reverted |
| A8 DoH / API budget | CLOSED for the read | the budget **is** shared — one accept loop, one semaphore of 64, permit before `accept()`, `/dns-query` on the same router. The saturation measurement is B1's row and needs the DoH generator |
| A9 three long-lived acceptors lack a failure-observation path | CLOSED | owner 2026-09-13: all three report into supervision, never the DNS fatal path. The wiring is production code and left for `p3-10c-acceptor-death-observation.md`; what was decided is the destination, not the mechanism |
| A10 what `Rotation` changed about shutdown | CLOSED | the audit's inference is confirmed — abort no longer closes the domain inboxes, stop rests on the watch alone — with its cause corrected: `Server`'s own unconditional `rotation` field, not `TlsServer`'s copy, so it holds in `http`-only mode too |
| B1 x86 characterization | MEASURED — does not close the W1 gate | run 2026-09-13, results in [`p3-10-track-b1-x86.md`](../../../docs/code-review/phase3/p3-10-track-b1-x86.md). Six rows of eight. Usable: plain HTTP, small opaque bodies, the H2 download arms, the prewarm hop. **Unresolved on this box:** the intercepted handshake, both splice groups and `splicebench`'s ranking — each moved 44–101% between two runs of identical code, so no percentage claim rests on them. Still not run: the two DoT/DoH rows (no generator) and splice RSS (no verified runner) |
| B2 RB5009 sweep | BLOCKED | deploy decision not made; p3-11 owes the dst-nat 443 rule. Workload settled 2026-09-13: the sweep gates on **W1**, and W2 cannot move the N=2 verdict |
