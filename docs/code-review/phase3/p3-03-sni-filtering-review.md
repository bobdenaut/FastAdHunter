# p3-03 — SNI Filtering — Review

**Status:** PASS WITH DEFERRED FINDINGS — review 2026-09-02, approved fixes
applied same day (two passes; see §Review findings). Task `DONE` 2026-09-02
by owner decision; the RB5009 splice-throughput row and the bench-fidelity
fix are p3-06's (its plan §Step 1 "p3-03 carry-over").

## Implementation Summary

A second listener (`[https.listen]`, default 8444, the dst-nat target for LAN
:443) reads the TLS **ClientHello only**, asks the existing domain matcher about
the SNI hostname under the client's policy, then **closes** (block) or
**byte-splices** (pass/allow). Nothing on this path terminates TLS: no rustls
server config, no leaf, no CA.

| Crate | Change |
| --- | --- |
| `fah-config` (L1) | **new** `schema/https.rs` — `HttpsConfig` / `HttpsListenConfig` / `SniConfig` / `NoSni`; `Config.https`; three validations |
| `fah-rules` (L2) | `Matcher::lookup_host` / `lookup_host_in` — the domain tier with `qbit = None`, allocation-free |
| `fah-model` (L1) | `Event::HttpsSni(Box<RequestEvent>)`, `EventKind::HttpsSni`, wire tag `https-sni` |
| `fah-http` (L3) | **new** `sni.rs` (ClientHello parser), **new** `https.rs` (`TlsProxy`), **new** `tls_server.rs` (`TlsServer`); `server.rs` accept loop lifted into a shared generic; `ProxyCounters.non_tls` |
| `fah-api` (L3) | `as_http()` also answers for `Event::HttpsSni`; `"https"` added to `BOOT_KEYS` |
| `fastadhunter` (L4) | `https_enabled`, `HTTPS_ORIGIN_PORT`, `egress_exceptions` extracted and shared, `build_tls_proxy`, bind/build/serve wiring, telemetry poll, fan-out arm |

No new crate; no layering change (`crates/fastadhunter/tests/layering.rs`
passes unchanged). No new **release** dependency anywhere — `tokio-rustls`,
`rustls` and `rcgen` enter `fah-http` as `[dev-dependencies]` only, and all
three already build in the workspace via `fah-api` / `fah-certs`.

## Decisions

- **The buffered ClientHello is immutable and forwarded verbatim.** The parser
  walks the record stream through a non-allocating cursor (`read_u8/u16/u24`,
  `skip`, `copy_into`) that skips each 5-byte header and reassembles across
  records; it never compacts or rewrites the buffer, because those exact bytes
  are what the origin must receive. The name is copied into a 253-byte stack
  array, so the only heap allocation is the returned `Box<str>`.
- **The idle deadline is session-wide, not per-direction.**
  `copy_bidirectional_with_sizes` keeps tokio's half-close handling; a thin
  `Activity<S>` adapter (`S: Unpin`, so no `unsafe`, no pin-project) stamps one
  shared `AtomicU64` on every ready poll of either direction, and a watchdog
  sleeps to `last + idle` in `tokio::select!`. Hand-rolling the copy loop was
  rejected: half-close is where such loops get it wrong.
- **`TlsProxy` gets its own `ProxyCounters` instance**, same type, so
  `/telemetry` reads one snapshot per listener. `non_tls` was added to
  `ProxyCounters`/`ProxyStats` (the HTTP proxy never increments it) and
  `spawn_telemetry_poll` folds the TLS proxy's `refused_destination` into the
  existing `requests_refused` sum. No new metric family.
- **A no-SNI observation is classified, never spliced.** The container cannot
  recover the pre-DNAT destination (`docs/routeros-traps.md` — measured
  `ENOENT`), so `no_sni = "pass" | "block"` decides only how the *closed*
  connection reads in the feed. `NoSni::Block` needs a `DecisiveRule`, and the
  honest one is the setting that decided it: list `[https.sni]`, rule
  `no_sni = "block"`.
- **`[https]` is boot-class in its entirety**, so `"https"` joined `BOOT_KEYS`
  in `fah-api`. Not in the plan's checklist, but the plan's own classification
  demands it: without the entry `POST /config` would answer
  `restart_required: false` for an apply that never happened.

### D1–D10 as applied

| # | Applied |
| --- | --- |
| D1 | Yes — `Records` cursor, immutable buffer, one `Box<str>` |
| D2 | Yes — `Activity<S>` + `idle_watchdog`, `copy_bidirectional_with_sizes` |
| D3 | Yes — `tokio::time::timeout(hello_timeout, TcpStream::connect(..))`; failure counts `upstream_failures` and emits `status 0`, `bytes 0` |
| D4 | Yes — separate `ProxyCounters`; `non_tls` added; telemetry poll sums both `refused_destination` |
| D5 | Yes — `Allow` splices exactly like `Pass`; `decisive_rule` + `active.id_of(ctx.policy)` as `proxy.rs` does |
| D6 | Yes — `as_http()` answers for both variants; `kind` still from `event.kind().as_str()` |
| D7 | Yes — `SocketAddr::new(peer.ip().to_canonical(), peer.port())` first thing |
| D8 | Yes — lowercase, LDH-and-dots, ≤253 bytes, no empty/leading/trailing label, no trailing dot, label ≤63 |
| D9 | Yes — dev-dependencies only, versions/features exactly as specified |
| D10 | Yes — defaults 60 000 / 10 000; the WebSocket caveat is under Remaining TODOs |

Two departures from the letter of the plan, both recorded rather than silent:

1. **`TlsProxy::new` takes `origin_port`** (binary passes 443) rather than
   hard-coding it, so the integration tests can point it at an ephemeral origin.
   The prompt's test section requires this shape.
2. **A resolve/connect failure emits the judged verdict** (`pass` *or* `allow`),
   not literally `pass`. On this branch the verdict is already known and
   downgrading an `allow` to `pass` in the feed would misreport it.

## Measurements

Dev box (x86_64, Windows 11, loopback), not the RB5009. Diagnostic only — no
PERFORMANCE.md budget row is claimed or edited; p3-06 owns the on-device figure.
Corpus: 1 MiB origin→client per connection, raw TCP, criterion `sample_size 20`.

| Arm | Run 1 (median) | Run 2 (median) | Range across both runs |
| --- | --- | --- | --- |
| `https_sni_splice/direct_to_origin` | 907.58 MiB/s (1.1018 ms) | 907.34 MiB/s (1.1021 ms) | 858–954 MiB/s |
| `https_sni_splice/through_splice` | 148.55 MiB/s (6.7318 ms) | 134.25 MiB/s (7.4487 ms) | 112–200 MiB/s |

The direct arm is stable across runs; the splice arm is not (criterion reports
`p = 0.47` between runs, i.e. the difference is noise, but the within-run CI
spans ~1.8×). Each iteration includes connect + ClientHello + splice teardown,
so this is a per-connection figure, not steady-state throughput. It is
**superseded** by any RB5009 measurement and by any dev-box run that pins the
workload the way `docs/measurement-traps.md` requires.

| Suite | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | green, 0 failed |
| `cargo test -p fastadhunter --test layering` | 1 passed |
| `fah-http` lib (`sni` + `tls_server`) | 12 SNI unit tests, 4 `TlsServer` tests |
| `fah-http/tests/sni.rs` | 5 integration tests |
| `fah-rules` | 4 new `lookup_host` tests |
| `fah-config` | 5 new tests (3 in `schema/https.rs`, 2 in `lib.rs`) |
| `fah-model` | round-trip + spelling tests extended to `https-sni` |

Acceptance criteria covered by assertion:

- **Blocked domain, zero bytes upstream** — `a_blocked_sni_costs_the_origin_nothing`:
  connection-counting TLS origin stays at 0 accepts, `resolve_failures == 0`
  (proving the verdict is taken before `resolve`), `blocked == 1`.
- **Spliced session byte-identical** — `a_tls_session_completes_end_to_end_through_the_splice`:
  a real `tokio-rustls` client handshakes to a real `tokio-rustls` origin
  through the splice and a 64 KiB payload round-trips byte-for-byte.
- **Garbage on the port** — closed, `non_tls == 1`, listener still serves the
  next connection.
- **Egress guard** — an SNI resolving to `192.168.77.1` gives
  `refused_destination == 1`, 0 origin accepts.
- **`no_sni` matrix** — a real rustls hello addressed to an IP (no SNI
  extension) closes under both settings, event verdict `pass` vs `block`,
  `host == ""`, 0 origin accepts.
- **Real hellos** — the parser is asserted against ClientHellos captured from a
  live `tokio-rustls` client, not only hand-assembled ones (the hand-assembled
  builder remains, because fragmentation and the mutation loop need a fixture
  whose every length field is addressable).

## Files changed

| File | Change |
| --- | --- |
| `crates/fah-config/src/schema/https.rs` | new |
| `crates/fah-config/src/schema/mod.rs` | `mod https`, re-exports, `Config.https` |
| `crates/fah-config/src/lib.rs` | re-exports; address/port/`max_connections`/api-port-collision validations; 2 tests |
| `crates/fah-rules/src/matcher.rs` | `lookup_host`, `lookup_host_in`; 4 tests |
| `crates/fah-model/src/request_event.rs` | `Event::HttpsSni`, `EventKind::HttpsSni`, `Event::https_sni`, match arms, tests |
| `crates/fah-http/src/sni.rs` | new — parser + 12 tests |
| `crates/fah-http/src/https.rs` | new — `TlsProxy`, close-or-splice, `Activity` adapter |
| `crates/fah-http/src/tls_server.rs` | new — `TlsServer` + 4 tests |
| `crates/fah-http/src/server.rs` | accept loop lifted into `pub(crate) accept_loop<F, Fut>` |
| `crates/fah-http/src/proxy.rs` | `non_tls` on `ProxyCounters`/`ProxyStats`/`snapshot` |
| `crates/fah-http/src/lib.rs` | module decls + `pub use` |
| `crates/fah-http/Cargo.toml` | 3 dev-dependencies |
| `crates/fah-http/benches/proxy.rs` | `https_sni_splice` arm |
| `crates/fah-http/tests/sni.rs` | new — 5 integration tests |
| `crates/fah-api/src/ports.rs` | `duration`/`as_dns`/`as_http` arms |
| `crates/fah-api/src/config_store.rs` | `"https"` in `BOOT_KEYS` |
| `crates/fah-dns/src/pipeline.rs` | test-only panic arm widened |
| `crates/fastadhunter/src/main.rs` | `https_enabled` (+ test), `HTTPS_ORIGIN_PORT`, `egress_exceptions`, `build_tls_proxy`, bind/build/serve, telemetry poll, fan-out |
| `crates/fastadhunter/tests/outcome_telemetry.rs` | panic arm widened |

## Known limitations

- **No-SNI and ECH are undeliverable, not merely unfiltered.** With no
  recoverable pre-DNAT destination there is nothing to splice to, so such a
  connection is closed whatever `no_sni` says. The DNS layer remains the
  backstop for those domains. Constraint measured on-device, not inferred.
- **QUIC / HTTP-3 untouched.** UDP :443 is a separate transport; a client that
  reaches an origin over QUIC bypasses this listener entirely.
- **`idle_timeout_ms` cuts long-lived idle sessions.** WebSocket-over-TLS and
  other long-poll connections idle beyond 60 s are closed and must reconnect.
  The owner may raise the key; `0` is rejected at load (there is no
  "disabled").
- **Metrics are not split per protocol.** `record_http` is reused, so
  `/telemetry`'s request counters merge HTTP and HTTPS-SNI. Splitting them is a
  small additive `protocol`-labelled counter — flagged, not built.
- **The parse is not a TLS implementation.** A hello whose SNI is absent past
  `MAX_HELLO_BYTES` (16 KiB) is classified as no-SNI rather than searched
  further; that is the bound, not a bug.
- **The dashboard shows `https-sni` rows but cannot filter for them.**
  `dashboard/frontend/src/pages/live-feed/filters.ts` lists
  `KINDS = ['dns', 'http']`, and `live-feed/detail.tsx` gates the HTTP detail
  block on `row.kind === 'http'`, so an SNI row renders through the DNS-shaped
  branch with empty method/path. Frontend deliberately not edited by this task.

## Remaining TODOs

### Doc edits — proposed 2026-09-01, **applied 2026-09-02** with owner approval

CONFIGURATION.md (`[https]` block, `[egress]` note, §Mutability paragraph),
SECURITY.md (§Later phases, two bullets), API.md (§Events kinds + `https-sni`
item shape; §telemetry `refused` note for n4), ARCHITECTURE.md (§Listeners
HTTPS SNI, §HTTP Pipeline close-or-splice paragraph), ROADMAP.md (§Phase 3
bullet). The text below is what landed, kept for the record.

#### 1. CONFIGURATION.md — new section, to sit after the `[http]` block and before `# ─── Egress`

````text
# ─── HTTPS SNI filtering (Phase 3) ─────────────────────────────────────
# Inert unless [engine] mode is "dns+http+https". Nothing on this path
# decrypts: the ClientHello is read, the SNI is judged, and the connection is
# then closed or spliced byte-for-byte (SECURITY.md).
[https.listen]
address = "::"                # boot    — as [http.listen]: "::" is one
                              #           dual-stack socket, IPV6_V6ONLY off
port = 8444                   # boot    — NOT 443 (privileged, ADR-0004) and
                              #           NOT 8443, which [api] port already
                              #           uses — two listeners on one default
                              #           port would make dns+http+https fail
                              #           to boot on an untouched config. The
                              #           router dst-nats 443 here. A config
                              #           setting this equal to [api] port is
                              #           rejected at load, by name

[https]
max_connections = 1024        # boot    — ceiling on concurrent spliced
                              #           sessions; the accept loop is the same
                              #           one [http] uses, permit before accept.
                              #           Splice memory is 2 x 16 KiB per
                              #           session, so this key is the bound on
                              #           it (~32 MB at the default)
hello_timeout_ms = 10000      # boot    — deadline for a client to finish
                              #           sending its ClientHello, and the
                              #           deadline on the upstream connect. A
                              #           blackholed destination must not hold
                              #           a max_connections permit for the
                              #           kernel's SYN-retry window. 0 is
                              #           rejected at load
idle_timeout_ms = 60000       # boot    — a spliced session with no activity in
                              #           BOTH directions for this long is
                              #           closed (one session-wide deadline, not
                              #           one per direction). Long-lived idle
                              #           connections (WebSocket over TLS) are
                              #           cut and must reconnect; raise it if
                              #           that matters. 0 is rejected at load

[https.sni]
no_sni = "pass"               # boot    — "pass" | "block". A ClientHello with
                              #           no plaintext SNI (or an ECH-encrypted
                              #           one) is CLOSED EITHER WAY: the
                              #           container cannot recover the
                              #           pre-dst-nat destination
                              #           (docs/routeros-traps.md — measured
                              #           SO_ORIGINAL_DST = ENOENT), so there is
                              #           nothing to splice to. This key decides
                              #           only how that closed connection is
                              #           classified in events and metrics
````

`[egress]`'s existing comment block gets one sentence appended, since the
section is now genuinely shared rather than reserved:

````text
# Phase 3's SNI path uses these same rules at port 443 — the SNI hostname is
# the destination claim, judged after resolution exactly as a Host header is.
````

And §Mutability classes gets one paragraph after the `[http]` one:

````text
The whole `[https]` section is **boot** on the same terms: the listener and its
semaphore are built once at bind, and the two timeouts become per-connection
deadlines held by the proxy handle. `[https.sni] no_sni` is read per connection
but from the boot-time handle, so changing it needs a restart like the rest.
````

#### 2. SECURITY.md — new subsection under §Later phases

````text
- **Phase 3 — SNI filtering** decrypts nothing. The ClientHello is parsed as
  bytes (strict bounds on every length; a malformed or non-TLS connection is
  closed and counted, never forwarded), the SNI hostname is judged by the same
  Rule Engine and per-client policy the DNS path uses, and the connection is
  then either closed or relayed **uninspected** in both directions. No key
  material, no CA and no `/config` access exist on this path. The SNI hostname
  is attacker-controlled, so it is handed to the resolver and then to the
  **egress guard** (`fah_common::egress`), which judges the *resolved* address
  — the same open-relay defence the `Host` header gets on :80.
- **ECH / no-SNI is a hard transport limit, not a policy choice.** A TLS
  connection reaching the dst-nat'd :443 with no plaintext SNI has **no
  recoverable destination**: the container's netns holds no conntrack record of
  the router-side NAT, so `getsockopt(SO_ORIGINAL_DST)` returns `ENOENT`
  (measured on-device 2026-08-31, `docs/routeros-traps.md`). Such a connection
  is closed; `[https.sni] no_sni` only decides whether it is *reported* as pass
  or block. The DNS layer remains the backstop for domains hidden behind ECH.
````

#### 3. API.md — §`WS /api/v1/events`

Replace "A `query` event carries both pipelines (p2-04), tagged by `kind`:" with:

````text
A `query` event carries all three pipelines, tagged by `kind` — `dns`, `http`
or `https-sni` (p3-03):
````

and append after the "Every key is always **present**" paragraph:

````text
An `https-sni` item is an HTTPS connection judged at the TLS ClientHello, with
no decryption. It fills the HTTP-shaped fields it can and empties the rest:
`domain` is the SNI hostname (empty when the hello carried none), `method` and
`path` are `""`, `resource_type` is `"unknown"`, `status` is `0` — the outcome
is connection-level, there is no HTTP status — `bytes` is the upstream→client
total of the spliced session however it ended (clean close, error or idle
deadline), `0` on a block, a refused destination or a failed connect, and
`duration` is ClientHello-to-upstream-connected (the request-latency
analogue), not the session length. There is no per-kind filter on this
socket; a client selects `https-sni` items by the `kind` field.
````

#### 4. ARCHITECTURE.md — new §Listeners subsection after §HTTP (Phase 2)

````text
### HTTPS SNI (Phase 3)

- TCP on `[https.listen]`, default **8444** — not 443 (privileged) and not 8443
  (`[api] port`; a shared default would make `dns+http+https` fail to boot, so a
  config setting them equal is rejected at load). The router dst-nats 443 here.
- Bound **only** when `engine.mode` is `dns+http+https`, on the same reasoning
  as HTTP above.
- The accept loop is **the same code** as HTTP's: `server::accept_loop` is
  generic over the per-connection handler, so the permit-before-accept ceiling
  cannot drift between the two listeners.
````

and a paragraph in §HTTP Pipeline (or a new §HTTPS Pipeline beside it):

````text
**HTTPS is close-or-splice, not a pipeline.** `TlsProxy` reads the ClientHello
under a deadline, extracts the SNI, and takes a verdict through
`Matcher::lookup_host_in` — the domain tier only, since a nameless connection
has no URL and no resource type. A block returns before any resolution or
connect, so it costs zero bytes upstream. A pass resolves the SNI host, judges
the resolved address with the shared egress guard, forwards the buffered
ClientHello verbatim and then relays both directions with
`copy_bidirectional`, uninspected, under one session-wide idle deadline. TLS is
never terminated here; that is p3-04.
````

#### 5. ROADMAP.md — §Phase 3 — HTTPS, one line added to the bullet list

````text
- SNI-level HTTPS filtering for every client, no setup and no decryption
  (`p3-03`) — blocked domains die at the ClientHello; ECH/no-SNI is closed, not
  forwarded (measured transport limit, SECURITY.md)
````

### Follow-ups for the owner to schedule (not done here)

- Dashboard: add `https-sni` to `KINDS` and give it a detail branch
  (`live-feed/filters.ts`, `live-feed/detail.tsx`). Phase 3 already triggers a
  dashboard re-review per ROADMAP.md.
- p3-06: on-device dst-nat of 443, the RB5009 splice throughput row, and a real
  phone/browser walkthrough. The dev-box figure above is not a budget.
- Optional, if the owner wants http vs https-sni split in `/telemetry`: an
  additive `protocol`-labelled counter set.


## Review findings — 2026-09-02 (main agent, no subagents)

Base: uncommitted working tree on `phase3-03` over `0383c9c`; `git diff --stat`
15 modified + 5 new source files. Gates re-run by the reviewer: clippy clean,
`fah-http`/`fah-config`/`fah-model`/`fah-rules` tests green (matches the review
file's claim). Findings are ranked; each says whether it gates `DONE`.

### 1. Plan compliance

| Unit / criterion | Status | Evidence |
| --- | --- | --- |
| Config surface (Step 1) | done | `schema/https.rs`; validations in `lib.rs:124-149`; `[https]` in `BOOT_KEYS` |
| Parser (Step 2) | done | `sni.rs`; immutable buffer, record-spanning cursor, one `Box<str>` |
| Handler (Step 3) | done, 2 defects | `https.rs`; see M1, M2 |
| Listener (Step 4) | done | `tls_server.rs`; shared `accept_loop` (decision 2) |
| Counters / exports (Step 5) | done | `non_tls` added; separate `ProxyCounters` instance |
| Binary wiring (Step 6) | done | bind before drop, serve after; `egress_exceptions` shared |
| `lookup_host_in` (decision 3) | done | `matcher.rs:876-884` + 4 tests |
| `Event::HttpsSni` (decision 6) | done | serde tag `https-sni`; every exhaustive match updated |
| Blocked = zero bytes upstream | proven | accept counter 0, `resolve_failures == 0` |
| Byte-identical splice | proven | 64 KiB echo through real rustls handshake |
| Budget-fast | diagnostic only | dev-box bench; on-device row is p3-06 → `AWAITING SOAK` |
| Docs | proposed, not applied | correct per working agreement; 2 text defects (m5, m6) |

Deviations, all recorded in the review file: `origin_port` parameter
(authorised by the prompt), judged verdict emitted on connect failure
(justified), `"https"` in `BOOT_KEYS` (required by the plan's own
classification). No unauthorised architecture change; no new release
dependency; layering test unchanged.

### 2. Findings

**M1 — Major — `bytes` is 0 whenever the splice ends by idle deadline or error.**
`https.rs:191-203`: the `Err` arm and the watchdog arm both return `0`, and
`copy_bidirectional` only reports counts on a clean double-EOF. A browser keeps
an HTTPS socket open after the page loads; with `idle_timeout_ms = 60000` the
proxy closes most such sessions before the client does, so the event and the
`response_bytes` metric undercount by roughly everything (inference from the
code path; not measured on traffic). Fix: count upstream→client bytes inside
`Activity` (bump an `AtomicU64` on the client side's `poll_write`
`Ready(Ok(n))`) and report it from all three exit arms. **Fix before DONE.**

**M2 — Major — session length feeds the per-request latency histogram.**
`duration = hello→close` (plan decision 6) is observed by
`request_duration_forward` (`registry.rs:191-196`). A ten-minute keep-alive
session lands in the same histogram as 20 ms HTTP fetches, so the published
p99 becomes "longest session", not request latency. Fix: stamp `duration` at
splice start (hello → upstream connected), the request-latency analogue; if
session length is wanted, it needs its own gauge. Amends decision 6 — owner
call — but **fix before DONE** (two lines).

**M3 — Major — the idle watchdog is untested.** It is the only bound on permit
lifetime for a peer that vanishes without FIN; nothing asserts it fires or that
the permit returns. Also untested: connect deadline (D3), `Allow` splices
(D5), event on refused destination. Add: idle 200 ms, client handshakes through
the splice, stays silent → EOF on the client, `available_permits` back to
max. **Fix before DONE** (tests only).

**M4 — Major (perf, diagnostic) — splice is 6× under direct on loopback.**
148 vs 907 MiB/s, i.e. +5.6 ms per MiB with 16 KiB buffers (≈64 read+write
pairs per MiB per direction). Not a gate here, but if any of that survives the
~9× factor the RB5009 ceiling sits near 25 MiB/s, under gigabit LAN. p3-06 must
A/B `SPLICE_BUF` 16 vs 64 KiB on-device before writing the budget row, and the
bench needs a steady-state arm (one connection, N MiB) beside the
per-connection one. **Defer to p3-06; record as headline risk.**

**m1 — Minor — `idle_timeout_ms = 0` closes every splice at once** and passes
validation. Reject 0 like `max_connections` (or define 0 as disabled and
implement it). Fix before DONE — validation + one test.

**m2 — Minor — only the `api.port` collision is validated.**
`https.listen.port == http.listen.port` or `== dns.listen.port` produces a
bind-race error, not a named one. Cheap to extend; defer or fix.

**m3 — Minor — a rejected SNI name is indistinguishable from no SNI.**
`normalize` (`sni.rs:257-290`) returns `None` for `_` and for anything outside
LDH, and the handler then emits `host = ""`. An operator sees a `no_sni` row
and cannot learn which name failed. Allow `_` (matcher and resolver take it)
and `debug!` the rejection with the byte length. Fix before DONE — small.

**m4 — Minor — an IP-literal SNI bypasses `[egress] allow_ip_literal_hosts`.**
`1.2.3.4` passes `normalize`; the HTTP claim path refuses it (`claim.rs:82`).
The egress guard still judges the resolved address, so there is no relay into
the LAN; the inconsistency is the finding. Defer; mention in the SECURITY.md
proposal.

**m5 — Minor — proposed API.md text is wrong.** It says `?kind=https-sni`
selects SNI events. The events socket filters by message class only
(`Subscription` = query/stats/config); the only `kind` query parameter is the
top-N route (`routes.rs:274`, values blocked|queried|clients). Correct the
proposal before the owner approves it.

**m6 — Minor — proposed CONFIGURATION.md wording** "no activity in EITHER
direction" reads as per-direction; the watchdog is session-wide. Say "in both
directions".

**n1 — Nitpick — `read_client_hello`** rescans from byte 0 per 2 KiB chunk and
copies through a stack buffer into a `Vec::new()` that grows 2→4→8→16 KiB.
`Vec::with_capacity(HELLO_CHUNK)` + `read_buf` removes the copy and reallocs.
Bounded, off the DNS hot path; defer.

**n2 — Nitpick — telemetry `refused` fold** (`main.rs:780-793`) via
`into_iter().chain().reduce()`; two `map_or(0, ..)` sums read better. Defer.

**n3 — Nitpick — `accept_loop` wraps `on_conn` in an extra `Arc`**, one
refcount bump per accept on top of the closure's own clone. `F: Clone` would
do. Defer.

**n4 — Nitpick — `requests` on `TlsProxy` counts accepted connections**
(garbage included) while the HTTP instance counts requests; same field, other
meaning. Note it in `/telemetry` docs; no code change.

### 3. Areas checked, no finding

- Parser bounds: every length checked before slicing; mutation loop covers all
  bytes; zero-length records bounded by the 16 KiB cap; `Short` with
  `!truncated` correctly maps to `NotTls`.
- Verdict before resolve; `Block` returns with no upstream contact (asserted).
- Half-close: `copy_bidirectional` semantics kept; `Activity` is `Unpin`, no
  `unsafe`.
- Memory: per connection 2 × 16 KiB + hello Vec + one `Box<str>`; total bounded
  by `https.max_connections`; freed on close.
- Concurrency: permit held for the whole connection; watchdog and copy are one
  task; no locks on the path.
- Layering: `fah-http` gains no sibling import; `fah-config` still L1.
- Regression: HTTP `Server` behaviour unchanged (closure over the same
  `Arc<Proxy>`); existing proxy/filtering suites green; `Event` serde for
  `dns`/`http` unchanged.

### 4. Status (as reviewed)

BLOCKED for DONE until M1, M2, M3, m1, m3 applied; m5, m6 to correct in the
doc proposals. Owner approved that set 2026-09-02.

### 5. Fixes applied — 2026-09-02

| Finding | Change | Verified by |
| --- | --- | --- |
| M1 | `Activity<'a, S>` now borrows two stack `AtomicU64`s (`last`, `written`) instead of an `Arc`; the client-side adapter counts `poll_write` `Ready(Ok(n))`, and `splice` returns that count from every exit arm (clean, error, idle) | `an_idle_spliced_session_is_closed_and_its_permit_returned` asserts `bytes >= 4` after an idle close |
| M2 | `emit` takes `duration: Duration`; the splice path stamps it before `splice` runs (hello → upstream connected); every other path passes `started.elapsed()` as before. **Amends plan decision 6** (`duration = hello→close`): session length no longer enters `request_duration_forward` | same test asserts `duration < idle` |
| M3 | new integration test: idle 400 ms, `max_connections = 1`; a silent session is closed, `bytes` and `duration` are asserted, and a second connection is serviced afterwards (permit returned) | `fah-http/tests/sni.rs`, 6 tests |
| m1 | `https.hello_timeout_ms == 0` and `https.idle_timeout_ms == 0` rejected by name in `fah-config` | `a_zero_https_timeout_is_rejected_rather_than_read_literally` |
| m3 | `normalize` admits `_`; a rejected name is logged at `debug` with its length and a lossy `Debug`-escaped rendering before it is classified as no-SNI | `an_underscore_is_a_name_the_resolver_will_take` |
| m5 | API.md proposal no longer claims a `?kind=` filter; states `bytes`/`duration` semantics | text above |
| m6 | CONFIGURATION.md proposal says "BOTH directions", one session-wide deadline; notes 0 rejected for both timeouts | text above |
| m2 | `https.listen.port` is checked against `[api] port`, `[http.listen] port` and `[dns.listen] port`; the error names the colliding section | `the_https_listener_may_not_share_another_listener_port` (3 cases) |
| n1 | `read_client_hello` reads with `read_buf` into a `Vec::with_capacity(HELLO_CHUNK)` capped by `limit(want)`; the 2 KiB stack chunk and its copy are gone, growth is 2→4→8→16 KiB by `reserve` only when full | existing SNI tests (truncated, fragmented, oversized) |
| n2 | telemetry `refused` is two `map_or(0, ..)` sums, always published | compiles; `/telemetry` unchanged in meaning |
| n3 | `accept_loop` requires `F: Clone` and clones the closure per accept; the wrapper `Arc` is gone (`Sync` bound dropped with it) | both `max_connections` tests |

Memory effect of M1: two fewer heap allocations per spliced session (the
`Arc<AtomicU64>` and its clone are gone); the two counters live on the task's
stack for the splice's lifetime.

Gates after fixes: `cargo fmt --all -- --check` clean; `cargo clippy
--workspace --all-targets -- -D warnings` clean; `cargo test --all-features
--workspace` green (fah-http: 66 unit, 6 SNI integration).

### 6. Status (final)

**PASS WITH DEFERRED FINDINGS.** Deferred: M4 (splice throughput A/B, on-device
budget row) → p3-06; m4 (IP-literal SNI vs `allow_ip_literal_hosts`) →
backlog, owner's call. n4 closed by the API.md §telemetry note. Docs applied.
Task moves to `AWAITING SOAK` pending the RB5009 row.

## Review findings — second pass, 2026-09-02 (commit `40ca0cc`, main agent, no subagents)

Base: the committed tree at `40ca0cc` (working tree clean), i.e. the first
pass plus its §5 fixes. Gates re-run: `cargo clippy --workspace --all-targets
-- -D warnings` clean; `cargo test -p fah-http -p fah-config -p fah-rules
-p fah-model` green, 0 failed (`fah-http/tests/sni.rs` 6 passed). Numbering
continues from the first pass.

### 1. Plan compliance (re-verified against the plan file)

| Plan item | Status | Evidence |
| --- | --- | --- |
| Step 1 config: types, defaults 8444/1024/10 000/60 000, `deny_unknown_fields`, `#[serde(default)]` on `Config.https` | done | `schema/https.rs`, `schema/mod.rs:44-45`, `a_config_without_an_https_section_still_parses` |
| Step 1 validations: `max_connections`, port vs api (plan) + http/dns (m2), timeouts (m1) | done | `lib.rs:127-167` + 3 tests |
| Step 2 parser: `HelloScan` shape, record reassembly, every length bounded, normalise (lowercase, LDH+`_`, ≤253, label ≤63), 16 KiB cap → `NoSni` | done | `sni.rs`; 12 unit tests incl. mutation loop and real rustls hellos |
| Step 3 handler, 1–4 | done | `https.rs:87-162`; block returns before `approved_address` |
| Step 3 "EOF / deadline before a hello → close, `non_tls`, no event" | done as specified | `https.rs:100-109` — but see m8 |
| Step 4 listener: shared bind, `serve`, `shutdown` aborting, `PORT_SETTING` | done in `fah-http`, **`shutdown` never wired in the binary** | `tls_server.rs:59-63`; see M5 |
| Step 5 counters: separate instance, `non_tls`, `lib.rs` exports | done | `proxy.rs:83,101,115`; `lib.rs:33-38` |
| Step 6 wiring: gate, bind before drop, build after, serve, telemetry, fan-out | done except lifecycle | `main.rs:393-400,419-428,548-550,573-577,737-748` |
| Decision 2 shared `accept_loop` + mirrored `max_connections` test | done | `server.rs:100-124`; `tls_server.rs:143-180` |
| Decision 3 `lookup_host_in`, 3 named assertions (block, `@@`, `$client`) | done, + `$dnstype` exclusion | `matcher.rs:879-885`, 4 tests |
| Decision 6 field mapping, `bytes`, `duration` (amended by M2) | done | `https.rs:279-309`; API.md §events |
| Decision 7 exhaustive `https_enabled` | done | `main.rs:634-639` + test |
| Tests §Integration (6 named) | 5 of 6 fully; "event on refused destination" counter-only | see m9 |
| Bench arm | done, diagnostic | see n5 |
| Out of scope respected (no TLS termination, no QUIC, no fronting attempt) | yes | no rustls in release deps |

### 2. Findings

**M5 — Major (lifecycle) — `Engine` does not own the HTTPS listener, so
`shutdown()` never reaches it.** `main.rs:393` binds `https` as a local of
`Engine::start`; the struct (`main.rs:286-292`, built at `:600-605`) has
`dns`, `http`, `api`, `tasks` and no `https`. `TlsServer` is therefore dropped
at the end of `start`, its `JoinHandle` detaches, and the accept loop outlives
`Engine::shutdown()` (`main.rs:609-617` aborts DNS, HTTP, API and the tasks —
not HTTPS). Not observable today because the runtime is torn down right after
`shutdown()` (`main.rs:254-260`), but plan Step 4 specifies the `Server`
mirror ("`shutdown()` aborting"), `TlsServer::shutdown` is dead code from the
binary's side, and the `dns.fatal()` exit path would keep accepting on :8444
for as long as anything kept the runtime alive. Fix: `https:
Option<fah_http::TlsServer>` on `Engine`, store it, abort it in `shutdown()`.
**Fix before DONE** (three lines).

**m7 — Minor (memory) — the ClientHello buffer lives for the whole splice.**
`serve_connection` owns `hello` (capacity 2→16 KiB) and lends it to `splice`
(`https.rs:160,175`); it is freed only when the session ends. Per spliced
session that is +2 KiB typical, +16 KiB worst, on top of the 2 × 16 KiB copy
buffers: +2 MiB typical / +16 MiB worst at `max_connections = 1024`, which the
CONFIGURATION.md sizing ("2 x 16 KiB per session, ~32 MB") does not include.
Fix: move `hello` into `splice`, `drop(hello)` after `write_all`. Owner's
call; one signature change.

**m8 — Minor (observability) — `non_tls` counts silence, not only garbage.**
EOF before a hello and the `hello_timeout` deadline both increment `non_tls`
(`https.rs:100-109`), alongside `NotTls`. Browsers open speculative
connections and close them unused; each lands here (at once on close, or
after 10 s if left open), so on a real LAN the counter will be dominated by
benign preconnects and the "garbage on :443" reading API.md §telemetry gives
it is diluted. The HTTP twin `non_http` counts parse errors only
(`proxy.rs:304-305`), so two same-named fields carry different meanings.
Options: a separate `hello_timeouts` counter, or count EOF/deadline nowhere
and say so. Defer; not a correctness issue. Corollary worth one line in
CONFIGURATION.md: a silent preconnect holds a permit for up to
`hello_timeout_ms`.

**m9 — Minor (tests) — first-pass M3 was closed with one of its four
tests.** Still unasserted: (a) an `Allow` verdict splices and the event says
`allow` (D5 — the success path of the deviation the first pass justified);
(b) the connect deadline (D3): an approved but blackholed address returns
within `hello_timeout` with `upstream_failures == 1` and an event of
`bytes 0`; (c) the refused-destination *event* — `tests/sni.rs:343` asserts
the counter only. All three fit the existing harness. Fix before DONE, or
mark M3 "partially applied" in §5 rather than "fixed".

**n5 — Nit (bench fidelity) — the splice bench bypasses `TlsServer`.**
`benches/proxy.rs` `splice_in_front_of` runs its own accept loop: no permit,
and no `set_nodelay` on the accepted client socket, which production's
`accept_loop` sets. The measured relay differs from the shipped one in the
TCP option most relevant to a byte relay. p3-06 should build the harness on
`TlsServer::bind/serve` before the M4 A/B. Folds into M4.

**n6 — Nit — `HelloScan::Incomplete` arm at `https.rs:119` is
unreachable**: `read_client_hello` loops on `Incomplete` and returns `NoSni`
at the cap. Harmless; leave or collapse.

**n7 — Nit — `to_upstream` (`https.rs:186`) is written and never read.**
`Activity` demands a `written` counter for both directions; the upstream one
is dead. Leave.

**n8 — Nit — the bound is 16 384 bytes *including* record headers**, so the
largest legal single-record hello (16 384 body + 5) is classified `NoSni`. No
client comes near it (PQ hybrids add ~1.2 KiB). Doc wording only, if at all.

**n9 — Nit (doc) — domain fronting is an inherent SNI-filter bypass** and the
new SECURITY.md bullet lists ECH/no-SNI but not it: SNI `allowed.cdn` plus an
inner `Host: blocked.cdn` on one CDN address passes the SNI judge; DNS is the
backstop, as for ECH. One sentence in the Phase 3 bullet — owner approval
needed before the edit.

### 3. Areas checked, no finding (second pass)

- Parser re-walked: `advance_record` checks header and body bounds before any
  index; `skip`/`copy_into` clamp to `record_end`; zero-length records loop at
  most `len / 5` times; `Short && !truncated` → `NotTls` is right for a
  non-handshake record interleaved mid-hello; bytes after the hello are never
  read (`trailing_records_after_the_hello_do_not_disturb_the_scan`); the
  rejected-name `debug!` is level-gated and `Debug`-escaped.
- `read_client_hello`: `reserve` before `read_buf` keeps `Vec::chunk_mut`'s
  hidden 64-byte grow off the path; growth is exactly 2→4→8→16 KiB;
  `limit(want)` caps the length; rescans are ≤ 8 per connection.
- Hello forward: `write_all(hello)` runs before the watchdog, but ≤16 KiB into
  a fresh socket's empty send buffer cannot block; no deadline needed.
- Address selection: first approved address, no fallback — identical to
  `Proxy::approved_address`; `resolve_host` returns A before AAAA, so a v4
  route is tried first. Parity, not a regression.
- Idle: `Activity` stamps on every `Ready` poll including EOF; after a
  half-close the surviving direction still refreshes; the `select!` drop closes
  both sockets; the watchdog re-arms once per refresh, never per byte;
  `last`/`to_client` on the task stack outlive the pinned `copy`; no `unsafe`.
- Verdict order: `judge` precedes `approved_address`; `Block` returns with no
  resolve (asserted by `resolve_failures == 0`).
- Deps and layering: `Cargo.lock` +3 dev lines only; `rcgen 0.14` /
  `tokio-rustls 0.26` match `fah-certs` / `fah-api`; `layering.rs` unchanged.
- Fan-out: `Event::HttpsSni` reaches `metrics.record_http` and
  `stats.record_http`; stats records policy and client only, so `host = ""`
  never enters a domain table; no `_ =>` arm in fah-api / fah-stats /
  fah-metrics / binary swallows the kind.
- `Server` unchanged in behaviour: closure clone = one `Arc` bump per accept,
  as before; HTTP and DNS suites green.
- Applied docs match code: API.md `duration_ms` = hello→connected, `bytes`
  reported on every exit; CONFIGURATION.md "BOTH directions", 0 rejected;
  §telemetry `refused` note; SECURITY.md Phase 3 bullets.

### 4. Fixes applied — second pass, 2026-09-02

| Finding | Change | Verified by |
| --- | --- | --- |
| M5 | `Engine` gains `https: Option<fah_http::TlsServer>`; stored at construction; `shutdown()` aborts it between HTTP and API | `cargo test -p fastadhunter` green; `TlsServer::shutdown` now has a caller |
| m9 (a) | `an_allowed_sni_is_spliced_and_reported_as_allow`: a block rule for `origin.test` plus its `@@` exception; handshake completes, echo round-trips, event verdict `Allow`, `blocked == 0` | `fah-http/tests/sni.rs` |
| m9 (b) | `an_unreachable_upstream_is_reported_within_the_hello_deadline`: SNI resolves to `192.0.2.1` (TEST-NET-1, approved by the guard, never answers), `hello_timeout = 500 ms`; client closed within 5 s, `upstream_failures == 1`, `resolve_failures == refused_destination == 0`, event `pass` / `bytes 0` / `status 0`. Run alone it takes 0.53 s on the dev box — the deadline, not an OS unreachable error, ends it | same file |
| m9 (c) | `an_sni_resolving_to_a_private_address_is_refused` now takes the event channel and asserts the event (`pass`, `bytes 0`, `status 0`) and `upstream_failures == 0` | same file |
| — | `harness_with` takes a `Limits { hello, idle, max_connections }` (clippy `too_many_arguments`) | compiles |
| m7 | `splice` takes `hello: Vec<u8>` by value and drops it right after `write_all`; per-session memory during the relay is now exactly the 2 × 16 KiB copy buffers the CONFIGURATION.md sizing states | existing splice tests |
| m4 | `TlsProxy::with_ip_literal_hosts(bool)` (default `false`); an IP-literal SNI is refused before `judge`/`resolve` unless `[egress] allow_ip_literal_hosts`, counted as `refused_claim` like the HTTP claim path (no event, as there); the binary passes the config value; `/telemetry` `refused` now folds `refused_claim + refused_destination` for both listeners | `an_ip_literal_sni_is_refused_before_resolution_unless_allowed` (`tls_server.rs`): refused → `refused_claim 1`, `resolve_failures 0`; allowed → `refused_claim 0`, `resolve_failures 1` |
| M4, n5 | not fixed here — carried into `plan/wip/phase3/p3-06-phase3-verification-plan.md` §Step 1 as "p3-03 carry-over" (owner-approved edit 2026-09-02) | — |

Gates after fixes: `cargo fmt --all -- --check` clean; `cargo clippy
--workspace --all-targets -- -D warnings` clean; `cargo test -p fah-http`
green (`tests/sni.rs` 8 passed); `cargo test -p fastadhunter` green.

### 5. Status (second pass)

**PASS WITH DEFERRED FINDINGS** — M5, m9, n9, m7, m4 applied (n9:
SECURITY.md §Later phases, domain-fronting bullet, owner-approved
2026-09-02). Deferred, each with an owner in its plan file (edits
owner-approved 2026-09-02): M4 + n5 → p3-06 §Step 1; m8 → p3-04 §Step 4
(`non_tls` / `hello_timeouts` split) and p3-06 §Step 4.6 (soak reading);
dashboard `https-sni` filter/detail → p3-06 §Step 5 (dashboard re-review);
n6–n8 leave. CONFIGURATION.md `[egress] allow_ip_literal_hosts` comment now names
the SNI path (owner-approved 2026-09-02).
Task marked `DONE` 2026-09-02 (owner decision); the on-device throughput row
is owed by p3-06, not by this task.
