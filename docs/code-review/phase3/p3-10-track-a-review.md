# p3-10 Track A — review

Task: [plan/wip/phase3/p3-10-post-merge-performance.md](../../../plan/wip/phase3/p3-10-post-merge-performance.md)

Code read at `93fb85f` (main). Pre-merge comparison against tag
`main-pre-phase3-merge` = `ebc46f1`, worktree `../fah-main-bench`, read-only.

## Implementation Summary

Two passes so far. The first closed four items by reading code; the second, on
2026-09-13, wrote the four tests Track A owed. No production code changed in
either — the local mutations the second pass needed were reverted, and the tree
is clean.

| Item | Disposition |
| ---- | ----------- |
| A1 — the intercepted path has no allocation ceiling | CLOSED — test written, §Execution |
| A2 — how many hyper pools exist after the merge | CLOSED |
| A3 — the ceilings cover one transport out of four | CLOSED — test extended, §Execution |
| A4 — two fixes held by reading, not by a test | CLOSED — tests written, §Execution |
| A6 — DoT's connection ceiling is compiled in | CLOSED |
| A7 — the two listener counter sets can be transposed | CLOSED — assertion written, §Execution |
| A8 — DoH and the API's resource budget | CLOSED for the read; B1 keeps the measurement |
| A10 — what `Rotation` changed about shutdown | CLOSED — inference CONFIRMED, cause corrected |

**Track A has nothing open.** A5 and A9 were decided by the owner on 2026-09-13
and are production code, so they left this task for files of their own:
**`p3-10b-dot-connection-gauge.md`** and
**`p3-10c-acceptor-death-observation.md`**, both `WAITING`, both owed before
p3-11's seven-day soak starts. What remains inside p3-10 is Track B.

Three closed items carry a follow-up that is explicitly not theirs: A6's
adequacy question is a B2 row, A8's saturation measurement is a B1 row, and
A10's "does it matter" judgement is not made here.

## A2 — how many hyper pools exist after the merge

**One hyper pool per HTTP allocation domain, and nothing else in the binary owns
one. The merge did not change that count.**

### Where a pool comes from

`Proxy::new` holds the only `Client::builder` in `fah-http/src`
([proxy.rs:269-273](../../../crates/fah-http/src/proxy.rs#L269-L273)), so one
`Proxy` value is exactly one hyper pool. `pool_max_idle_per_host` is set there
from the `max_idle_per_host` argument.

### How many `Proxy` values the binary creates

The binary never constructs a `Proxy` directly. `http_proxy_factory`
([main.rs:1021-1050](../../../crates/fastadhunter/src/main.rs#L1021-L1050))
returns an `impl Fn() -> Proxy`, so the count is the number of *calls*, not the
number of factories. The factory is built once, and only when the HTTP listener
exists ([main.rs:415-425](../../../crates/fastadhunter/src/main.rs#L415-L425)).

Two call paths, mutually exclusive
([main.rs:642-660](../../../crates/fastadhunter/src/main.rs#L642-L660)):

- `http_runtimes == 0` — `http.serve(Arc::new(make_proxy()))`
  ([main.rs:651](../../../crates/fastadhunter/src/main.rs#L651)).
  **1 `Proxy`, 1 pool.**
- `http_runtimes >= 1` — `http.serve_domains(domains, HTTP_DRAIN_TIMEOUT, make_proxy)`
  ([main.rs:654](../../../crates/fastadhunter/src/main.rs#L654)). `serve_domains`
  spawns `domains.get()` domain threads
  ([server.rs:108-119](../../../crates/fah-http/src/server.rs#L108-L119)) and hands
  each an `Arc` of the same factory. Each domain calls it **once**, at the top of
  `serve_domain`, outside the accept loop — `let proxy = Arc::new(make_proxy());`
  ([domain.rs:139](../../../crates/fah-http/src/domain.rs#L139)).
  **`http_runtimes` `Proxy` values, `http_runtimes` pools.** Not per connection.

`http_runtimes` defaults to `available_parallelism() / 2`, floored at 1
([schema/runtime.rs:18-20](../../../crates/fah-config/src/schema/runtime.rs#L18-L20)),
capped at `MAX_HTTP_RUNTIMES = 64`
([fah-config/src/lib.rs:122](../../../crates/fah-config/src/lib.rs#L122), validated
at [:206-211](../../../crates/fah-config/src/lib.rs#L206-L211)).

### What the HTTPS listener adds: nothing

`TlsProxy` holds no `Client` and no pool. Its fields are resolver, policy,
counters, rules, policies, events, origin port, timeouts, `no_sni`,
`allow_ip_literal_hosts`, interception and the two splice sizes
([https.rs:34-48](../../../crates/fah-http/src/https.rs#L34-L48)).

The binary builds **one** `TlsProxy`
([main.rs:552-556](../../../crates/fastadhunter/src/main.rs#L552-L556)), wraps it
in a single `Arc` and shares that one `Arc` with every domain. With domains
present it calls `https.serve_domains(Arc::clone(proxy), http)`
([main.rs:665](../../../crates/fastadhunter/src/main.rs#L665)), which *borrows the
HTTP server's existing rotation* instead of creating its own
([tls_server.rs:47-58](../../../crates/fah-http/src/tls_server.rs#L47-L58)). The
HTTPS listener spawns no domain, no runtime and no `Proxy`.

The two `TlsProxy::new` sites in `tls_server.rs` (`:126`, `:140`) are inside
`#[cfg(test)] mod tests`, not the binary.

### The arithmetic

Held idle-connection memory = **pools × `max_idle_per_host` × per-connection
buffer**. The binary passes `MAX_IDLE_UPSTREAMS_PER_HOST = 8`
([main.rs:857](../../../crates/fastadhunter/src/main.rs#L857)). The buffer figure
is the 408 KiB grown H1 buffer from `8941770`'s commit message (dev box, x86,
conc 32, `http_runtimes = 2`, default pool and timeout) — quoted from
`git log -1 8941770`, not from memory.

| `http_runtimes` | pools | idle conns per host | held at 408 KiB |
| --------------- | ----- | ------------------- | --------------- |
| 0 (shared runtime) | 1 | 8 | ~3.2 MiB |
| 2 (RB5009 default: 4 cores / 2) | 2 | 16 | ~6.4 MiB |
| 64 (`MAX_HTTP_RUNTIMES`) | 64 | 512 | ~204 MiB |

The `http_runtimes = 2` row reproduces `8941770`'s own count — "Sixteen pooled
connections (8 per host x 2 allocation domains)" — which is the check that this
reading matches the one the fix was measured under. The multiplier is per
*origin host*, so a client touching many hosts scales the 8 accordingly; that is
pre-existing and outside this item.

### Comparison against `main-pre-phase3-merge` (`ebc46f1`)

| | pre-merge `ebc46f1` | post-merge `93fb85f` |
| --- | --- | --- |
| `Client::builder` sites in `fah-http/src` | 1 (`proxy.rs:218`) | 1 (`proxy.rs:269`) |
| `make_proxy()` calls per domain | 1 (`domain.rs:67`) | 1 (`domain.rs:139`) |
| shared-runtime path | 1 (`main.rs:521`) | 1 (`main.rs:651`) |
| `MAX_IDLE_UPSTREAMS_PER_HOST` | 8 (`main.rs:695`) | 8 (`main.rs:857`) |
| pools contributed by the HTTPS listener | n/a (no listener) | 0 |

**Pool count is unchanged: `http_runtimes` before, `http_runtimes` after.** The
concern this item was opened on — that a second listener multiplied the `Proxy`
instances and so divided `8941770`'s benefit — does not hold. The benefit stands
at the same scale it was measured at.

The intercepted path's per-connection upstream `Sender` (no pool, so no reuse and
a different cost shape) is B1's row, not this item's; A2 closes without it.

**Verdict: CLOSED — pools = `http_runtimes`, or 1 when `http_runtimes = 0`;
unchanged by the merge, so `8941770`'s benefit is not divided.**

## A6 — DoT's connection ceiling is compiled in

**Confirmed: the DoT connection ceiling has no runtime control.**

- `pub const DOT_MAX_CONNECTIONS: usize = 64;`
  ([dot.rs:23](../../../crates/fah-dns/src/dot.rs#L23)), re-exported from
  [lib.rs:22](../../../crates/fah-dns/src/lib.rs#L22).
- Its only non-test use is the listener's semaphore:
  `let slots = Arc::new(Semaphore::new(DOT_MAX_CONNECTIONS));`
  ([dot.rs:63](../../../crates/fah-dns/src/dot.rs#L63)), inside `run_with`. The
  permit is taken **before** `listener.accept()`
  ([dot.rs:64-72](../../../crates/fah-dns/src/dot.rs#L64-L72)), so at the ceiling
  the listener pauses and clients queue in the kernel backlog.
- The other two uses (`dot.rs:560-577`) are `#[cfg(test)]` and assert the ceiling
  holds.
- `[dns.listen]` carries exactly five keys, none of them a ceiling: `address`,
  `port`, `dot_enabled`, `dot_port`, `doh_enabled`
  ([schema/dns/listen.rs:4-16](../../../crates/fah-config/src/schema/dns/listen.rs#L4-L16)).
  The struct is `#[serde(deny_unknown_fields, default)]`, so no undeclared key can
  reach it, and `env.rs` maps `["dns","listen",…]` to those same five only.
- By contrast `dns.tcp_max_connections` *is* a config key, defaulting to 1024 and
  validated at
  [fah-config/src/lib.rs:200-204](../../../crates/fah-config/src/lib.rs#L200-L204).
  The two transports are asymmetric in this respect.

Changing 64 is a rebuild: no config key, no environment override, no API write.

Whether 64 is adequate is **not** this item's question — that is B2's row. No
measurement was taken here and no config key is proposed. A line in
CONFIGURATION.md §`[dns.listen]` is a separate edit needing its own approval.

**Verdict: CLOSED — `DOT_MAX_CONNECTIONS = 64` is compile-time only, and
`[dns.listen]` has no ceiling key.**

## A8 — DoH and the API's resource budget

**Confirmed end to end: `/dns-query` draws on the same connection budget as the
dashboard and the admin endpoints.**

Traced from the listener to the route:

1. **One listener, one router, one accept task.** `ApiServer::bind` builds a
   single router from `crate::routes::router(...)`
   ([server.rs:72](../../../crates/fah-api/src/server.rs#L72)) and spawns exactly
   one `accept(listener, router, acceptor)` task
   ([:75](../../../crates/fah-api/src/server.rs#L75)). That is the only `accept`
   call site, and `shutdown()` aborts that one handle
   ([:102-105](../../../crates/fah-api/src/server.rs#L102-L105)).
2. **One semaphore, sized 64.** `const MAX_CONNECTIONS: usize = 64;`
   ([:33](../../../crates/fah-api/src/server.rs#L33)), instantiated once inside
   `accept` as `Semaphore::new(MAX_CONNECTIONS)`
   ([:107](../../../crates/fah-api/src/server.rs#L107)) — one instance for the
   whole server, not per route and not per connection kind.
3. **The permit is taken before `accept()`.** `acquire_owned().await` at
   [:113](../../../crates/fah-api/src/server.rs#L113) precedes
   `listener.accept().await` at
   [:117](../../../crates/fah-api/src/server.rs#L117). At the ceiling the listener
   stops accepting rather than accept-then-drop, so waiting clients sit in the
   kernel backlog. The permit moves into the per-connection task and frees when
   that connection ends
   ([:130-137](../../../crates/fah-api/src/server.rs#L130-L137)).
4. **`/dns-query` is a route on that same router.** It is added to the `admin`
   router after the auth layer is applied — so outside auth, but inside the same
   router tree — gated on `state.tls && state.doh.is_some()`
   ([routes.rs:123-131](../../../crates/fah-api/src/routes.rs#L123-L131)). There is
   no second router, no second listener and no separate budget for it.

**A DoH connection therefore holds one of the same 64 slots a dashboard
connection holds.** Sixty-four concurrent DoH connections stop the API listener
from accepting anything, the dashboard and every admin endpoint included.

Two observations follow; neither is a decision taken here.

- The failure lands exactly when someone is trying to look at why it is
  happening — the admin surface is the thing that stops answering.
- The value is the same 64 as `DOT_MAX_CONNECTIONS` (A6), reached independently:
  two encrypted DNS transports, two unrelated compiled-in constants, one number.
  Both also take the permit before `accept()`, so both degrade the same way.

What the ceiling costs in practice — whether household DoH traffic approaches 64
concurrent connections, and what admin-endpoint latency does at and above it — is
**not** established here. That is B1's row and needs the DoH load generator,
which does not exist.

**Verdict: CLOSED for the read — the budget is shared, 64 slots, permit taken
before `accept()`. The saturation measurement stays B1's.**

## A10 — what `Rotation` changed about shutdown

**The integration audit's inference is CONFIRMED on its conclusion and wrong on
its stated cause.**

The audit's row
([main-phase3-integration-audit.md:56](main-phase3-integration-audit.md)) reads:
*"`Server` keeps a `Rotation` copy for `TlsServer`, so aborting the HTTP acceptor
no longer closes the domain inboxes; stop relies on the `watch` alone."*

### Re-derived from the code

**Pre-merge (`ebc46f1`)** — the senders had exactly one owner. `serve_domains`
built a local `Vec<Sender>` and moved it straight into the spawned task:
`Dispatch::Domains { senders, next: 0 }`
(`../fah-main-bench/crates/fah-http/src/server.rs:123`, inside
`tokio::spawn(accept_loop(...))` at `:118-124`). `Server` kept no copy; there was
no `rotation` field. Aborting `self.handle` dropped the task, dropped the
`Dispatch`, dropped the senders, and `inbox.recv()` in each domain returned
`None`. Abort alone closed the inboxes.

**Post-merge (`93fb85f`)** — `Server` gained a `rotation: Rotation` field
([server.rs:48](../../../crates/fah-http/src/server.rs#L48)), and `serve_domains`
stores a clone in it *before* moving another clone into the accept loop:

```rust
let rotation = Rotation::new(senders);   // server.rs:120
self.rotation = rotation.clone();        // server.rs:121
```

`Rotation` is `#[derive(Clone)]` over `senders: Vec<mpsc::Sender<Handoff>>`
([server.rs:249-253](../../../crates/fah-http/src/server.rs#L249-L253)), so that
clone is a second set of live senders, not a handle to the first.

`Server::shutdown` aborts the acceptor and never drops the field
([server.rs:148-158](../../../crates/fah-http/src/server.rs#L148-L158)): it calls
`handle.abort()`, then `self.stop.send_replace(true)`, then joins the domain
threads. `self.rotation` stays alive as long as the `Server` value does.

**So the inboxes are no longer closed by the abort.** Confirmed.

### Where the audit's cause is wrong

The audit attributes the surviving copy to `TlsServer`. `TlsServer` does take
one — `let rotation = http.rotation();`
([tls_server.rs:48](../../../crates/fah-http/src/tls_server.rs#L48), via the
accessor at [server.rs:144-146](../../../crates/fah-http/src/server.rs#L144-L146)) —
but that copy is not what keeps the inboxes open. `self.rotation` at
[server.rs:121](../../../crates/fah-http/src/server.rs#L121) is assigned
unconditionally, whether or not an HTTPS listener exists and whether or not
`rotation()` is ever called.

**The behaviour therefore also holds in `http`-only mode, with no `TlsServer`
constructed at all** — which the audit's wording does not predict. The accessor is
the *reason the field was added*; the field is the reason the abort no longer
closes the inboxes. Anyone testing this by disabling HTTPS would see the same
behaviour and could wrongly conclude the inference was refuted.

### Stop does still work, via the watch

`serve_domain` selects on `inbox.recv()` and `stop.changed()`
([domain.rs:140-152](../../../crates/fah-http/src/domain.rs#L140-L152)). On the
watch firing it calls `inbox.close()` and drains what is already queued before
leaving the loop. Each domain's receiver comes from `self.stop.subscribe()` at
spawn ([server.rs:113](../../../crates/fah-http/src/server.rs#L113)), and
`Server::shutdown` fires it with `self.stop.send_replace(true)`
([server.rs:153](../../../crates/fah-http/src/server.rs#L153)).

So the second half of the audit's sentence — *"stop relies on the `watch`
alone"* — is confirmed too: the watch is now the only path that ends a domain,
where before there were two.

The binary's order is consistent with this. `https.shutdown()` — abort only;
`TlsServer` has no `stop` channel
([tls_server.rs:83-87](../../../crates/fah-http/src/tls_server.rs#L83-L87)) — runs
before `http.shutdown()`
([main.rs:787-793](../../../crates/fastadhunter/src/main.rs#L787-L793)), so the
HTTPS acceptor stops feeding the rotation before the watch fires.

No test pins any of this either way. Whether the loss of the redundant stop path
matters is a separate judgement and is **not made here**.

**Verdict: CLOSED — inference CONFIRMED (abort no longer closes the inboxes; stop
rests on the watch alone), with its cause corrected: `Server`'s own unconditional
`self.rotation` field holds the senders, not `TlsServer`'s copy, so the behaviour
holds in `http`-only mode too.**

## Owner decisions — 2026-09-13

Four items were carrying a decision rather than a gap. The owner took all four.
Each is recorded here with the reasoning that decided it, so a later reader sees
why the other options lost rather than only which one won.

Nothing below is implemented, and the four split by where the work lives.

**A3 and A4 stay inside p3-10.** They are test work, not production code, and
the owner's decision is their go. Scattering four small test items into four
task files would cost more bookkeeping than the work itself, and Track A cannot
close while items that only close by execution sit outside it.

**A5 and A9 became their own tasks on 2026-09-13**, because both are production
code and p3-10 does not land production code (§Rules, 1):

| Decision | Task | Deadline |
| --- | --- | --- |
| A5 — DoT connection gauge | [`plan/wip/phase3/p3-10b-dot-connection-gauge.md`](../../../plan/wip/phase3/p3-10b-dot-connection-gauge.md) | before p3-11's seven-day soak starts |
| A9 — acceptor death observation | [`plan/wip/phase3/p3-10c-acceptor-death-observation.md`](../../../plan/wip/phase3/p3-10c-acceptor-death-observation.md) | before p3-11's seven-day soak starts |

Both are numbered `10b` / `10c` rather than `12` / `13` so the phase table's
order stays honest: they run before verification, and the header's "verification
last" stays true. p3-11 carries the matching precondition, because task numbering
cannot express "one step inside p3-11 waits for these".

### A3 — extend the ceiling across all four transports

**Decided: extend `forward_alloc.rs` to UDP, TCP, DoT and DoH. Same ceiling.
Do not write an exclusion.**

The point of the extended test is to **prove an invariant, not to discover four
numbers**. Framing happens before the pipeline is reached — `tcp.rs:175` calls
`pipeline.handle(&message_buf, client_ip, transport)` with the message already
decoded — so the per-handle allocation profile is transport-independent by
construction. The extension is a loop over the enum against one ceiling, and
what it pins is exactly that: transport does not change allocations.

That is also why the exclusion option lost. Writing down "TCP, DoT and DoH have
no guard" would have documented a blind spot that costs about as much to close
as to describe, and left three transports unguarded on an assumption nobody had
tested.

- **Owed:** the extended test, passing, with the ceiling unchanged.
- **Not owed:** a per-transport ceiling. If the arms need different numbers, that
  is a finding to report, not a thing to paper over by widening the bound.

### A4 — cover both call sites, and prove the tests discriminate

**Decided: execution, not an exclusion.** Add coverage for F11's supervisor
wiring and for the `refused_claim` / `refused_destination` call sites.

**Verification is part of the deliverable, not a nicety.** Remove each piece of
wiring locally, show the matching test fails, then revert. A test that was never
seen to fail is an assumption, which is the mistake F7 recorded and the one A7's
earlier draft repeated. The mutations are the method and they are reverted; no
production change lands (§Rules, 1).

The two call sites, from the task file: the select arm at `main.rs:769` and both
collections reaped at `:775-776`; `set_requests_refused` at `main.rs:1140` and
`:1598`.

- **Owed:** one test per call site, each shown to fail when its wiring is
  removed, and the tree clean afterwards.

### A5 — DoT gets its own connection gauge

**Decided: a separate gauge. Not shared with TCP, not left uncounted.**

The option that lost is "stays uncounted", and it lost on a cost the original
options table did not name: **it would make a B2 row unmeasurable.** B2 carries
"Peak concurrent DoT connections under household load", which is what says
whether `DOT_MAX_CONNECTIONS = 64` covers this house (A6). No other instrument
sees DoT connections today — `dot.rs:152` passes `None` where the TCP listener
passes its gauge — so leaving it uncounted deletes a measurement the owner has
already approved.

Sharing the TCP gauge fails the same test for a different reason: one number for
both transports cannot answer a question about one of them.

- **Owed:** the counter, its name, its telemetry surface and its documentation.
  **Carried by [`p3-10b-dot-connection-gauge.md`](../../../plan/wip/phase3/p3-10b-dot-connection-gauge.md)**,
  which also records that API.md must move in the same change while CONTEXT.md
  probably need not — `:116` already defines the transport vocabulary.
- **Until it lands:** the first soak of a Phase 3 build would set the final
  `tcp_max_connections` default from partial counters. The soak running now is a
  pre-Phase-3 build with no DoT, so its figures are whole. p3-11 holds the soak
  until p3-10b is `DONE`.

### A9 — `Supervised` for all three acceptors

**Decided: HTTP, HTTPS and the API server all join `Supervised`. Explicitly not
the DNS fatal path.**

The fatal path lost on blast radius. It ends the run loop, which is right for a
DNS listener and wrong for the dashboard: a dead admin acceptor would take the
resolver down with it, in a product whose primary job is answering DNS.
`Supervised` logs the death and counts it through `record_task_death`, so it
reaches telemetry rather than only a log file, and the process keeps resolving.
It also matches how every other long-lived task in the binary is already
handled.

The decision covers all three rows together. Settling only the two Phase 3
touched would have left the oldest one — the HTTP acceptor, which predates the
merge — exactly where it was.

- **Owed:** the wiring for all three. **Carried by
  [`p3-10c-acceptor-death-observation.md`](../../../plan/wip/phase3/p3-10c-acceptor-death-observation.md)**,
  which opens with a design decision the plan must settle before any code:
  `Supervised` owns its `JoinHandle` (`supervisor.rs:5-13`), but all three
  handles are private and needed by their own `shutdown()`, and `JoinHandle` is
  not `Clone`. So "put them in `Supervised`" does not typecheck as stated. The
  decision recorded here is the *destination*, not the mechanism.

## Execution — 2026-09-13: the four tests Track A owed

Written against `453637e`, dev box (x86_64, Windows 11). Gates green afterwards:
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo test --all-features --workspace`.

Each test that claims to catch a failure was made to catch it, by breaking the
thing locally and watching that test — and only that test — fail. Every mutation
was reverted; `git diff` on production code is empty.

### A1 — an allocation ceiling over the intercepted path

**New: `crates/fah-http/tests/intercept_alloc.rs`.** Shaped like
`proxy_alloc.rs` — the same counting `GlobalAlloc` over `MiMalloc` — with the
TLS stack on both legs: an rcgen CA signing the origin's leaf, a `CertStore` CA
for the minted leaf, and a client whose root store holds **only** the FAH CA.

That last point is the evidence the test measures interception rather than
splice: if the connection were spliced, the client would see the origin's own
leaf, signed by a CA it does not trust, and the handshake would fail. It
succeeds, so FAH minted the leaf and `intercept.rs` ran.

Four batches of 64 warm requests over one keep-alive session inside one TLS
connection. The ceiling is asserted on the last batch, and the last two batches
must be equal.

| Case | Allocations per request | Settles |
| ---- | ----------------------- | ------- |
| intercepted pass-through GET | 50 | by the third batch (48 → 49.6 → 50 → 50) |
| intercepted blocked script | 25 | flat from the first |
| intercepted blocked document | 38 | flat from the first |

The pass-through case is the reason for four batches rather than
`proxy_alloc.rs`'s two: with two, the equality assertion would have compared a
warming number against a settled one and failed on a healthy build.

For scale, the plain proxy's own ceilings are 51 / 20 / 32 for the comparable
cases. The intercepted path is not dramatically more expensive per request once
the session is up; its different cost is the per-connection upstream handshake,
which this test does not measure and which is B1's row.

### A3 — the ceilings extended to all four transports

**Changed: `crates/fah-dns/tests/forward_alloc.rs`.** Both pipeline tests loop
every case over a new `TRANSPORTS` constant — `Udp`, `Tcp`, `Dot`, `Doh` —
against the ceilings that were already there. No transport was given a looser
number. `warm_pipeline_misses_stay_under_the_ceiling` needed four times as many
unique names so each transport gets fresh misses rather than the previous
transport's cache.

The four deterministic cases allocate identically on all four transports (832,
1216, 640 and 1024 over 64 handles). That is the invariant made explicit:
`handle` runs after framing, so the transport has nothing left to influence.
The miss cases vary by a few allocations between runs on every transport
equally, which is the allocator noise the ceilings were sized for.

### A4 — cover for the two call sites held by reading

**New: `crates/fastadhunter/tests/wiring.rs`,** three tests. Each was proven by
deleting one line in `main.rs`, running, and restoring:

| Mutation | Result |
| -------- | ------ |
| drop `_ = supervision.tick() => self.reap_dead_tasks().await` | only `the_run_loop_still_reaps_supervised_tasks` fails |
| drop `deaths.extend(supervisor::reap(&mut self.stats_schedulers).await)` | only `reaping_covers_both_supervised_collections` fails |
| drop `metrics.set_requests_refused(refusals)` | only `the_telemetry_poll_still_publishes_the_refusal_split` fails |

**Correction to the item as written.** A4 named `main.rs:1140` **and `:1598`**
as the two call sites feeding the refusal split. `:1598` is not a call site —
it is inside `#[cfg(test)] mod tests`, which opens at `:1564`. There is one
production call site. The unit test that sits at `:1598` exercises
`refusals_of` and `set_requests_refused` directly and would stay green if the
poll loop stopped calling either, which is precisely the gap A4 identified.

**What these tests are, and are not.** They read `main.rs` as source and assert
the wiring is present, the way `crates/fastadhunter/tests/layering.rs` reads the
manifests. They catch a deletion. They do **not** catch a behavioural
regression — a `reap()` that silently stops reaping would pass all three.

The behavioural test is not available from here. The integration harness runs
the real binary as a child process
(`crates/fastadhunter/tests/common/mod.rs:168`), so no test can reach in, kill a
supervised task and watch `record_task_death` rise. Closing that properly needs
a production seam, which this task does not own. Recorded as a known limit of
the cover, not as a second gap: the failure A4 was about is a future edit
removing the wiring, and that is the failure these tests catch.

### A7 — an assertion that tells the two listeners apart

**Changed: `crates/fastadhunter/tests/e2e_https.rs`.** The scenario now sends
one plain-HTTP request to the HTTPS port before reading `/api/v1/telemetry`, and
asserts `non_tls == 1` under `listeners.https` and `== 0` under
`listeners.http`.

`non_tls` is the discriminator because the plain listener cannot produce it at
all: only `https.rs:153` raises it, on the path that reads a ClientHello. The
counter is incremented before the connection is dropped, so the probe's
`read_to_end` returning is proof the increment already happened — no sleep, no
retry.

**Shown to discriminate.** With the two arguments swapped at
`main.rs:618-622`, the HTTPS half of the telemetry document reads all zeros and
the first assertion fails:

```
assertion `left == right` failed: https: the plain-HTTP probe is the one event
only the HTTPS listener can produce
  left: 0
 right: 1
```

The swap was reverted.

`handshakes_completed` was the other candidate and was rejected: only
`intercept.rs:158` raises it, so it stays 0 on the shipped splice path. An
assertion built on it would be blind in exactly the build that ships — the
mistake A7 exists to avoid.
