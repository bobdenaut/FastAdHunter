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


## Findings — consolidated 2026-09-03; full history: `git show e35203e:docs/code-review/phase3/p3-03-sni-filtering-review.md`

Two review passes and two fix rounds. Fixed and withdrawn items are omitted —
git has them (n5 was closed by the p3-06 harness rebuild; m8's counter split
landed in p3-06 post-review work A). Still open:

| id(s) | Issue | Status | Where |
| --- | --- | --- | --- |
| M4 | splice throughput per connection 6–9× under direct on loopback; `SPLICE_BUF` 16 vs 64 KiB (memory axis `2 × SPLICE_BUF × max_connections`, 32 vs 128 MiB) decided only on the device | deferred | p3-06 review §Pre-declaration P1 (LAN-vs-loopback definition still an owner decision) |
| dashboard | live-feed `KINDS = ['dns','http']` — `https-sni` (and `https`) items cannot be filtered and render through the DNS branch | deferred | p3-06 review §Findings I1 — needs a named task (phase3-audit §5) |
| n6 | `HelloScan::Incomplete` arm in `serve_connection` is unreachable (`read_client_hello` returns `NoSni` at the cap) | won't-fix | harmless, leave |
| n7 | `to_upstream` counter is written and never read | won't-fix | `Activity` API symmetry |
| n8 | the 16 384-byte hello bound includes record headers, so the largest legal single-record hello classifies `NoSni` | won't-fix | no client comes near it |

**PASS WITH DEFERRED FINDINGS** — 5 open rows (2 deferred, 3 won't-fix). Task marked DONE 2026-09-02 (owner decision); the on-device throughput row is owed by p3-06.
