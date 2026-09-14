---
title: Allocation oracles that only hold on an idle machine, and one flake named but not explained
date: 2026-09-14
category: design-patterns
module: fah-http, fah-dns
problem_type: design_pattern
component: testing_framework
severity: medium
applies_when:
  - a test counts allocations with a counting GlobalAlloc and compares two batches
  - a test asserts exact equality between two measurements of the same workload
  - a ceiling is set to the value the path was measured at
  - a test waits on a resource counter returning to zero before asserting
  - a new long-running integration test is added to the workspace suite
  - the gate is red only when the whole workspace runs, green target by target
symptoms:
  - an allocation test fails by one allocation, in either direction
  - the same test passes standalone and fails under `cargo test --workspace`
  - a different test fails on each full-workspace run
  - a counter reads its starting value where the test expected its final one
  - the failure appears after an unrelated test target was added
---

# Allocation oracles that only hold on an idle machine

Four things are recorded here, all from tests that pass alone and fail in the
full workspace suite. §1, §3 and §4 are settled and their fixes are in the tree.
**§2 is identified but not explained**: the `fah-dns --lib` failure whose name
was lost has been reproduced and named, and why it happens is still open.

§1 and §3 are the same defect at two scales — a measurement the scheduler can
touch, asserted as if it could not. §4 is a different species: a wait that
returned before the work it was waiting for had begun.

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

> **Superseded 2026-09-14 on that last sentence — see §3.** Leaving the ceilings
> alone was wrong: every one of them was calibrated to the exact measured value,
> so they had the same failure mode the batch comparison had, only narrower.

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

## 2. The `fah-dns --lib` failure — identified 2026-09-14, cause unassigned

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

### Hunt, 2026-09-14 — reproduced, and the name is

**`upstream::encrypted::tests::dot_timeout_invalidates_the_pooled_connection_so_the_next_query_reconnects`**,
`crates/fah-dns/src/upstream/encrypted.rs:620`.

```text
thread 'upstream::encrypted::tests::dot_timeout_invalidates_the_pooled_connection_so_the_next_query_reconnects'
panicked at crates\fah-dns\src\upstream\encrypted.rs:620:9:
assertion `left == right` failed
  left: 3
 right: 2
```

The assertion is `assert_eq!(server.accepts.load(Ordering::Relaxed), 2)`. It runs
after the pool has been made to time out against a blackholed DoT server and then
reconnect.

**Measured.** The assertion three lines above it, `tls_handshakes == 2`, passed in
the same run. So the pool counted exactly two handshakes while the server counted
**three accepts**: one TCP connection was accepted that produced no handshake the
pool attributes to itself.

**Hypothesis, not evidence.** The likeliest cause is a connection attempt still in
flight when the blackholed query times out: it reaches the server's `accept()`
without completing a handshake, and load widens the window enough to expose it.
Nothing here verifies that. The reproduction does not distinguish it from any
other source of a third accept.

**Rate — one observation, which is not a rate.** All on the same tree the same
evening:

| Condition | Iterations | Failures |
| --- | --- | --- |
| whole target under load, run A | 30 | **1** |
| whole target under load, run B | 60 | 0 |
| whole target, no load | 30 | 0 |
| the named test alone, weak load | 30 | 0 |

So 1 in 90 under load, from a single hit. A binomial interval around one success
in ninety trials spans roughly 1-in-3000 to 1-in-17, so the only defensible
statement is that it is rare and that load is required — not how rare.

The isolated-test row is the weakest of the four and should not be read as
evidence of a lower rate: running one test removes the intra-target contention
of `--test-threads=16` across 221 tests, and it finishes so fast that the load
loop completed only one run beside it.

**Method that worked**, where `--test-threads=16` alone was not enough:

1. Pre-build both targets with `cargo test --no-run` and take the executable
   paths out of `--message-format=json`. **Run the binaries directly.** Two
   `cargo test` loops contend for the lock on `target/`, so the load loop dies
   and the hunt runs on an idle box while appearing to run under load. That
   happened twice before this attempt worked.
2. Loop `acceptor_death` at `--test-threads=4` as the load — two real
   `fastadhunter` binaries, 25–40 s a run. Give it 20 s of head start.
3. Loop `fah_dns --lib --test-threads=16`, one invocation per iteration, each
   redirected to its own file, and grep the files afterwards.

Why it stayed hidden: the whole `fah-dns --lib` target finishes in 3.25 s, so the
window is narrow and needs real contention rather than a merely busy machine.

**No fix attempted.** Identifying it was the job; the cause and the repair are
their own task.

### Cause hunt, 2026-09-14 — closed without attribution

**State: the cause is not settled. One TOCTOU was found, fixed, and shown not to
be the cause. A second, stronger candidate is identified and a correlation
campaign is running. Nothing is committed.**

#### The direct capture

Reproduced with the probe in place, iteration 36 of 90:

```text
expected 2 accepts, saw 3
accept seq=1 peer=127.0.0.1:65003
tls    seq=1 peer=127.0.0.1:65003 ok=true
accept seq=2 peer=127.0.0.1:64950
tls    seq=2 peer=127.0.0.1:64950 ok=false
accept seq=3 peer=127.0.0.1:65112
tls    seq=3 peer=127.0.0.1:65112 ok=true
```

**Proven:** `seq=2` is not ours. The `tls_handshakes == 2` assertion three lines
above passed in the same run, so the pool made exactly two connections; the two
that completed TLS are those. A foreign client reached the listener and never
finished a handshake against our certificate.

**Retracted.** An earlier reading argued from port ordering — 64950 below 65003,
therefore an older socket, therefore a different origin. That does not follow:
ephemeral ports are allocated per host, not per process, so the ordering says
nothing about which process owns the socket.

#### What the pool's own path can and cannot do

Established by reading `encrypted.rs`:

- `handshakes` is incremented at the top of `connect()` (`:162`), before any I/O.
- `acquire()` holds the slot mutex across `connect().await`, so connects are
  strictly serialised.
- `query()` does not retry on `TimedOut` (`:117`); the second `acquire` is
  unreachable on that path.
- `connect()` has no internal retry: one call, one `tls_exchange` attempt.
- The test server counts raw TCP accepts **before** `acceptor.accept()`.

Therefore `accepts <= handshakes` holds on the pool's path at all times, and the
third accept cannot originate there. Three alternative explanations were
eliminated by arithmetic: hickory opening two TCP connections per exchange would
give `accepts = 2 x handshakes` on every run; a third `connect()` between the two
reads would have shown `handshakes = 3`; concurrent attempts are excluded by the
mutex.

#### First TOCTOU — found, fixed, not the cause

`closed_tcp_addr()` existed in three byte-identical copies
(`src/upstream/encrypted.rs`, `src/upstream/mod.rs`,
`tests/support/mock_upstream.rs`) and did `bind("127.0.0.1:0")` → `local_addr()`
→ `drop(listener)` → return. Callers then connect to that address expecting
refusal. Between the drop and the connect, any other ephemeral bind can claim
the port — including `dot_server()`.

Replaced with `SocketAddr::from(([127, 0, 0, 1], 1))`: no bind, so no window.
Port 1 is privileged and unused, which is the justification — not any assumption
about ephemeral-range boundaries, which are configurable on both Windows and
Linux. No caller and no assertion changed. All four callers verified.

**The flake reproduced after this fix at the same rate, 1 in 90.** The fix is
correct on its own merits and is kept; it is not the cause.

#### Second TOCTOU — the current hypothesis

`free_tcp_port()` in `crates/fastadhunter/tests/common/mod.rs:648` has the same
shape, and the consequence is worse because the port is then handed to a **child
process** to bind. The file's own comment already admits it:

> Ports are chosen by binding an ephemeral socket and releasing it, which is
> inherently racy: between the release and the engine's own `bind`, anything else
> on the machine can claim the port — and under `cargo test --workspace` there
> are dozens of other tests binding ephemeral sockets in parallel.

Their answer was to retry the boot with fresh ports, which fixes their symptom
and leaves the released port available to everyone else.

`acceptor_death` draws 2–4 TCP ports per boot attempt and boots two child
processes per run. In the campaign it looped ~150 times, so **hundreds of
release-then-connect windows** existed beside the `fah-dns` tests. It also makes
plain-TCP connects: `poke()` at `acceptor_death.rs:76`. A plain-TCP client
landing on our DoT listener produces exactly one accept with `ok=false`.

Excluded: the child's DNS upstream is UDP, so it cannot produce a TCP accept.

#### The A/B that made this the leading hypothesis

| Condition | Iterations | Hits |
| --- | --- | --- |
| A — `fah_dns --lib` beside `acceptor_death` | 90 | **1** |
| A again, both ends instrumented | 120 | 0 |
| B — `fah_dns --lib` alone, same threads, no external process | 180 | 0 |

**The A/B argument does not survive its own second run.** After 210 loaded runs
with one hit, the loaded rate is about 1 in 210, not 1 in 90. At that rate the
chance of seeing zero in B's 180 runs is `(1 - 1/210)^180 ≈ 42 %` — so B tells us
almost nothing, and the claim that external processes make the difference is
withdrawn as unsupported. What is left is one captured hit, two TOCTOUs
demonstrated from code, and no attribution.

In-process candidates remain on the table but are now weaker: `dot.rs` has three
plain-TCP clients that would produce the same signature — `:547` sends a
length-prefixed query with no TLS, `:575` and `:591` connect and send nothing —
and `dot.rs:362`'s `listen()` frees its ephemeral port on `Drop`. All three run
in condition B as well and produced nothing in 180 runs.

#### The correlation campaign — run, and it caught nothing

**Result: 0 hits in 120 iterations**, with `acceptor_death` looping beside it and
1 039 `ADPROBE` lines collected. No correlation was possible because no failure
occurred.

What the log did establish about the instrument, for whoever runs it next:

| Signal | Count in 120 iterations |
| --- | --- |
| `ADPROBE drew tcp_port=` — ports released by `free_tcp_port()` | 64 |
| `ADPROBE poke local=… dest=…` — plain-TCP connects | **577** |

`poke()` fires far more often than ports are drawn, and it logs both ends:

```text
ADPROBE poke local=Some(127.0.0.1:61355) dest=127.0.0.1:61348 connected=true
```

That gives **three** correlation keys at the next hit, not the two planned:

1. our `peer=` appears as a poke `local=` — the connection traced end to end;
2. our `listener=` appears as a poke `dest=` — a poke landed on us rather than on
   the child;
3. our `listener=` appears in `drew tcp_port=` — the collision happened.

Any one of them closes the case. The second was not in the plan; it fell out of
reading the log.

Drawn ports in that campaign ranged 51169–61351. The captured stray was 64950,
outside it — but that hit was hours earlier with different ports in circulation,
so this neither confirms nor refutes anything.

#### What this costs, and what to do instead

At roughly 1 hit per 210 loaded runs, a 90-to-180 iteration campaign returns
noise: each costs 20–30 minutes and carries almost no information. Expecting one
capture needs about 600 iterations. **Do not run another blind campaign.**

Three ways forward, in the order worth considering:

1. **Make the collision deterministic.** Write a test that deliberately claims
   the port between `free_tcp_port()` returning and the child binding it, and
   check whether it produces exactly `accepts + 1` with `tls ok=false`. This
   proves the *mechanism* in seconds rather than hours. It does not attribute the
   historical hit, and must not be described as doing so.
2. **Close provisionally** and repair `free_tcp_port()` on its own merits as a
   real TOCTOU, stating plainly that it is not shown to be the cause. Honest, but
   leaves the flake alive.
3. More runs. Hours for one bit.

#### The campaign as it was configured

Both ends were instrumented, all of it test code. The two `ADPROBE` probes were
removed on 2026-09-14 once the campaign closed without a hit; only the DoT
failure-only probe is still in the tree.

- `free_tcp_port()` logged `ADPROBE drew tcp_port=<X>` for every released port.
- `poke()` logged `ADPROBE poke local=<L> dest=127.0.0.1:<P> connected=<bool>`.
- the DoT probe records `accept listener=<A> seq=<N> peer=<P>`.


Campaign: 120 iterations of `fah_dns --lib --test-threads=16` beside
`acceptor_death` in a loop.

#### The deterministic experiment — built 2026-09-14

`reusing_a_freed_port_reproduces_the_third_accept_without_attributing_the_flake`,
in `crates/fah-dns/src/upstream/encrypted.rs`, inside the same `#[cfg(test)]`
module as the flake. It is deterministic and runs in **0.48 s** — no load, no
repetition, no waiting for a rare coincidence.

**The roles are the reverse of the obvious reading**, and getting them backwards
produces a different failure. If our listener occupied the port *before* the
other side bound it, the other side would simply get `AddrInUse` and retry —
which its harness already handles, and which contaminates nothing. The
contamination needs the port to **change owner between being handed out and being
connected to**:

| Step | Who | What happens |
| --- | --- | --- |
| 1 | `acceptor_death` (P1) | `free_tcp_port()`: `bind(:0)` → X → **drop**; X is free again |
| 2 | P1 | writes X into the child's config, spawns the child |
| 3 | — | **the window**: between that drop and the child's `bind(X)` |
| 4 | `fah-dns --lib` (P2) | `dot_server()` calls `bind(:0)` and the OS hands it **X** |
| 5 | the child | `bind(X)` → `AddrInUse`; the harness retries with fresh ports |
| 6 | P1 | connects to X — `poke(X)` or the readiness probe |
| 7 | P2 | our listener accepts it; TLS never completes |

So the **bind** victim is the child `fastadhunter`; the **intruder on the port**
is our own `dot_server`; the **contamination** victim is our test. Step 6 is
where it happens — a connect to a port that changed hands.

**What the test does**, in one process and with no children, because the
mechanism needs port *reuse* rather than two processes:

1. `bind("127.0.0.1:0")`, read X, **drop** — models `free_tcp_port()`;
2. `dot_server_on(X)` binds X explicitly — models our listener winning the freed
   port. Up to 16 draws, so a WinNAT-reserved block costs a retry instead of a
   red run;
3. `TcpStream::connect(X)`, then drop — plain TCP, never a ClientHello, which is
   what `poke` is;
4. the flaky test's own flow: query, blackhole, timeout, reconnect.

**What it produced.** Captured by raising the expected count to 4 for a single
run and restoring it immediately:

```text
accept listener=127.0.0.1:61050 seq=1 peer=127.0.0.1:61051
tls    seq=1 peer=127.0.0.1:61051 ok=false
accept listener=127.0.0.1:61050 seq=2 peer=127.0.0.1:61052
tls    seq=2 peer=127.0.0.1:61052 ok=true
accept listener=127.0.0.1:61050 seq=3 peer=127.0.0.1:61053
tls    seq=3 peer=127.0.0.1:61053 ok=true
  left: 3
 right: 4
```

`accepts = 3` while `tls_handshakes = 2`, and the foreign connection is recorded
`ok=false`. That is the captured signature of the rare failure rather than
something merely similar: the handshake assertion passes, and the accept
assertion is the one that goes red — in that order.

**Harness change.** `dot_server()` is now a wrapper over
`dot_server_on(bind, queries_per_connection) -> io::Result<DotServer>`. No
caller changed, `accepts == 2` in the flaky test is untouched, and nothing
outside `#[cfg(test)]` was modified.

**What it shows, and what it does not.** It shows that the mechanism produces
exactly this signature, in under a second, whenever a connect arrives at a port
that has changed hands. It does **not** attribute the historical failure to this
mechanism: nothing in the recorded hit says which process held the port or which
connect landed on it. The test's name and its failure message both say so, or a
reader in six months would take the reproduction for a root cause.

#### Does Windows re-hand a freed port? Measured 2026-09-14

The reproduction shows the mechanism *can* produce the signature. It says
nothing about whether the operating system actually hands a just-freed port to
the next binder, which is what the `acceptor_death` hypothesis needs. That was
measured directly, outside the repository: a parent binds `:0`, reads X, drops
it, and hands X to four hunter **processes** over a pipe. Each hunter then draws
ephemeral ports and reports whether it was given X.

Two regimes per trial, 100 trials, 4 hunters:

| Regime | Draws | Hits on X |
| --- | --- | --- |
| first 64 draws, sockets held — what `dot_server` does | 25 600 | **0** |
| a 3 s window, bind and release at ~1 ms | 805 306 | **3** |

Per hunter-trial: **2 013 draws, 2 007 of them distinct**. The allocator walks
forward through the range; it almost never repeats. Observed range
49152..65534, 16 383 wide.

All three hits arrived late, after the hunter had already consumed a tenth of
the range:

| Trial | First hit at draw | Elapsed | Distinct ports already drawn |
| --- | --- | --- | --- |
| 28 | 1 332 | 1 953 ms | 1 324 |
| 34 | 1 197 | 1 745 ms | 1 196 |
| 41 | 1 786 | 2 625 ms | 1 785 |

**`bind(:0)` is not slow**, which an earlier reading of a stuck run got wrong:
64 held draws take 1.85 ms. The stuck run was a defect in the measuring
program — it never closed the hunters' stdin, so it hung in `wait()` after the
last trial.

**What this does to the hypothesis.** `fah-dns --lib` runs 222 tests in about
three seconds and makes on the order of a hundred `bind` calls in total — call
it 30 a second. The hunters drew 670 a second, twenty times faster, and still
needed roughly 1 200 draws before X came back. At the suite's real rate that is
forty seconds of drawing, longer than the whole run. In the regime that
actually resembles the suite — the first few dozen draws after the port is
freed — the result is **0 in 25 600**.

So the freed port does not come back quickly. It comes back after a sweep, and
only sometimes. The `acceptor_death` explanation is weak, not dead: the poke
window is seconds long, and a full workspace gate runs many test binaries at
once, so the host-wide draw rate is higher than one process's.

**An argument, not a measurement:** the acceptor's port had *accepted*
connections before it died. On Windows those leave TIME_WAIT entries on that
local port, which makes re-binding it harder, not easier. The port measured
here had accepted nothing, so this is the case most favourable to the
hypothesis — and it still came out negative.

**Do not read 3/100 as a flake rate.** It measures a process drawing 670 ports
a second, not a suite that opens a listener now and then.

#### The in-process dial journal

Built 2026-09-14, all inside `#[cfg(test)]`. `dot_pool_from` — the single funnel
every DoT and DoH pool in this module goes through — records each address it is
handed together with the current thread's name, which under libtest is the test
name. When the `accepts == 2` probe fires it prints every journalled dial on its
own port.

That answers one question and only one: was the extra connection opened by the
instrumented path inside this process? An empty list clears that path. It does
not clear the whole process, and it says nothing about other processes; a PID
lookup at failure time would, but on Windows the socket may already be gone by
then, so it stays a secondary check.

The journal also corrects a claim made earlier in this file: `ok=false` does
**not** mean "no ClientHello was sent". A TLS client that rejects the server's
certificate also leaves the server-side handshake failed. Any client with a
different trust anchor produces the same line as a plain TCP connect.

#### Verdict — 2026-09-14

**Historical cause remains unassigned. The failure mechanism is reproducible,
the production invariant is verified, and the test oracle is demonstrably
contaminable. Permanent failure-only diagnostics are retained for future
occurrences.**

Nothing is named as the cause. `acceptor_death`, `free_tcp_port()` and every
other candidate stay candidates.

The last campaign: **600 runs of `fah-dns --lib` at `--test-threads=16` with
`acceptor_death` looping beside it — 0 hits.** Pooled with the earlier batches
that is **one hit in about 810 loaded runs**. This file quoted roughly 1 in 210
earlier; that came from a single event and was too confident. Another blind
campaign is not worth its cost — the faithful context is a full workspace gate
at about three minutes an iteration, so a run long enough to matter costs most
of a day and still guarantees nothing.

| Claim | Standing |
| --- | --- |
| the mechanism produces this signature | **demonstrated**, deterministically, in 0.48 s |
| the production invariant holds | **demonstrated** — `tls_handshakes == 2` passed in the historical hit |
| the oracle can be contaminated from outside the pool's path | **demonstrated** |
| Windows re-hands a freed port quickly | **refuted** — 0 in 25 600 draws in the regime that resembles the suite |
| what opened the historical third connection | **unknown** |

#### Artefacts, outside the repository

None survive. The correlation campaign's `/tmp/corr-*.txt` and the final
600-run campaign's `/tmp/hunt/` both ended without a failure to keep, and `/tmp`
does not survive a reboot. If a hit ever lands, copy the evidence into this file
immediately.

#### What shipped

| Change | Why |
| --- | --- |
| `closed_tcp_addr` in three files → `127.0.0.1:1` | removes a real TOCTOU in test helpers; not the cause, but wrong on its own |
| unused `TcpListener` import removed from `mock_upstream.rs` | consequence of the above |
| the failure-only accept and TLS probe in `encrypted.rs` | costs nothing on a green run and is the whole diagnosis on a red one |
| the in-process dial journal | names the test that dialled the port, which a PID lookup would not |
| `dot_server_on` and the deterministic reuse test | the reproduction itself |

The probes record while the test runs — they cannot report what they never
observed — but they print only when the assertion fails. No permanent logging,
no extra counter, no work on any production path: every hunk in
`crates/fah-dns/src/` is inside `#[cfg(test)] mod tests`.

#### Standing constraints

- **`accepts == 2` stays as it is.** It is the instrument that exposed the
  contamination. It will go red again, rarely; when it does, what it prints is
  the reason this investigation ended somewhere rather than nowhere.
- **`free_tcp_port()` is not the cause and is not being repaired.** Its race is
  real and its window is microseconds, and the measurement says a freed port
  does not come back inside it.
- **No production code.** Every change is inside `#[cfg(test)]` — verifiable
  with `git diff crates/ | grep "^@@"`, every hunk contexted `mod tests {`.
- The UDP twins — `dead_addr` at `upstream/mod.rs:829` and `swr.rs:493` — are
  the same class with a different symptom: a reused port swallows the datagram
  instead of refusing it, so the caller sees a timeout rather than a refusal.
  Still out of scope.

### Why it matters more than it looks

p3-11 runs a seven-day soak. A test that fails rarely under load is the same
species as the reports that soak will produce, and a reader who has already seen
one unexplained red will discount the next one. It was not found before the week
starts, and that is recorded here deliberately — with the diagnostics left in
place, so the soak's own occurrence explains itself instead of adding another
unexplained red.

## 3. The per-operation ceilings had it too — settled 2026-09-14

### What happened

Two full-workspace gate runs on the same tree failed once each, on a
**different** `fah-http` test per run, both passing three times each when run
alone:

| Run | Test | Assertion |
| --- | --- | --- |
| 1 | `a_saturated_https_lane_leaves_the_http_lane_bounded_and_leaks_no_permit` (`fah-http/tests/sni.rs`) | `left: 2, right: 42` — see §4, a different species |
| 2 | `warm_intercepted_requests_allocate_a_steady_amount` (`fah-http/tests/intercept_alloc.rs`) | `allocated 1601; the ceiling is 25 per request` |

Chasing the second one showed the problem was not that test. **Every ceiling in
all three files is the measured value divided by the batch size**, so none of
them had any headroom at all:

| File | Case | Measured | Ceiling | Headroom |
| --- | --- | --- | --- | --- |
| `intercept_alloc` | pass-through GET | 3200 | 50 x 64 | **0** |
| | blocked script | 1600 | 25 x 64 | **0** |
| | blocked document | 2432 | 38 x 64 | **0** |
| `proxy_alloc` | pass-through GET | 3264 | 51 x 64 | **0** |
| | GET with Connection header | 3392 | 53 x 64 | **0** |
| | blocked script | 1280 | 20 x 64 | **0** |
| | blocked document | 2048 | 32 x 64 | **0** |
| `forward_alloc` | handles, four cases | 832 / 1216 / 640 / 1024 | 13 / 19 / 10 / 16 x 64 | **0** |

Which test fails on a loaded run is therefore arbitrary — whichever one happens
to fragment one allocation differently.

### The fix

The same `JITTER_ALLOWANCE` the neighbouring growth assertion in each file
already carries, on the same measurement, for the same reason:

```rust
assert!(last <= REQUESTS * case.ceiling_per_request + JITTER_ALLOWANCE, ...);
```

**No per-request or per-handle ceiling was changed.** 50/25/38, 51/53/20/32 and
13/19/10/16 are all still the documented costs. What was added is the tolerance,
and its justification is unchanged from §1: one leaked allocation per operation
shows as +64, so 4 is sixteen times below the smallest regression worth catching.

### The rule worth keeping

A ceiling set to the measured value is an equality assertion wearing a
`<=`. Derive the headroom from the signal a real regression would produce, the
same way the batch comparison does, or the ceiling fails on noise and says
nothing about growth.

## 4. A wait predicate that was true before the work started — settled 2026-09-14

### What happened

`a_saturated_https_lane_leaves_the_http_lane_bounded_and_leaks_no_permit`
(`fah-http/tests/sni.rs`) opens `CEILING = 2` TLS connections, queues
`OVERSHOOT = 40` raw sockets behind the permit ceiling, drops everything, and
asserts that all 42 were eventually judged. It read **exactly 2** — the two held
connections, and none of the forty queued.

The wait before the assertion was:

```rust
wait_for(Duration::from_secs(10), || {
    harness.https_open.open() == 0 && harness.http_open.open() == 0
})
```

`open() == 0` is true **before** the queued sockets are picked up as well as
after. Dropping the two held connections releases their permits, the count hits
zero, and the loop returns on its first poll — while the acceptor has not yet
touched the forty. On an idle box the acceptor beats the 50 ms poll, so the test
passes; under load it does not.

### The fix

Wait on a predicate that is false at the start and true only at the end — the
monotonic counter — keeping the permit check as a second term:

```rust
wait_for(Duration::from_secs(10), || {
    harness.counters.snapshot().connections == judged
        && harness.https_open.open() == 0
        && harness.http_open.open() == 0
})
```

The timeout was not raised and no production code was touched.

### The rule worth keeping

**A wait predicate must distinguish "not started" from "finished".** A resource
counter returning to zero cannot: zero is also its value before anything was
taken. Wait on the monotonic thing the assertion is actually about.

## Handover

Nothing is left to hand over. §2 records a closed investigation: the mechanism
is reproducible on demand, the production invariant is verified, and the
historical cause is unassigned and stays that way until the instrumented
assertion goes red again.

When it does, the failure message carries everything — the accept sequence with
peers and TLS outcomes, and the journal of what dialled that port from inside
the process. Read §2 §Verdict first, then take one of the four readings the
journal allows:

1. `peer` is in the journal under the **same** test — the instrumented client
   opened the extra connection. Find the place in that path that can open a
   second one.
2. `peer` is in the journal under a **different** test — in-process cross-test
   contamination, and that is the cause.
3. `peer` is **not** in the journal — the connection came from outside the
   instrumented funnel. Only then is a PID lookup worth its unreliability.
4. `peer` appears **more than once** — correlate the journal's order with the
   accept `seq` to tell a legitimate connect from a concurrent one.

Remember what `ok=false` does and does not mean: the server-side handshake
failed. That includes a plain TCP client that never sent a ClientHello, and
equally a TLS client that rejected the certificate. Do not narrow the suspects
on that line alone.
