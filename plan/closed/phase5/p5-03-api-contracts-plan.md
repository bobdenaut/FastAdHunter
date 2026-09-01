# P5-03 — Reconciliation — API Contract Additions

**Task:** [p5-03-api-contracts.md](p5-03-api-contracts.md) ·
**Phase:** 5 · **Depends on:** p5-01 · **Branch:** `phase5-03`

Reconciliation only — written 2026-08-26, before `/ce-plan`. No code, tests,
configuration or documentation changed.

**Pre-plan decisions closed by the owner on 2026-08-26** — recorded in §6. One
gate remains open and is not a decision: the API.md edit approval (§6.6).

## 1. Task scope

Three deliverables, no frontend:

1. **`WS /api/v1/events` subscription protocol** — client→server
   `{"subscribe":[...]}`, server-side filtering, plus moving the **engine-side
   publish gate** from "any socket connected" to "any socket wants `query`".
2. **`GET /api/v1/clients` gains the in-force policy and the assignment
   source** — removes the N+1 the Clients page would otherwise do.
3. **Reserved API.md sections** — `## Authentication *(Phase 5 — reserved)*`
   and the `/events` subscription contract, the latter promoted to live wording
   by this task's own commit since this task implements it.

Out of scope: authentication itself (`p5-04`), any frontend (`p5-05`), any
change to what an event's `data` payload contains.

## 2. Relevant files and existing implementation

| Concern | Location |
| ------- | -------- |
| Socket loop, `encode`, stats ticker, discard of inbound frames | [events.rs:130-172](../../../crates/fah-api/src/events.rs#L130-L172) |
| `EventHub`, `has_subscribers()` = `receiver_count() > 0` | [events.rs:46-90](../../../crates/fah-api/src/events.rs#L46-L90) |
| WS upgrade handler — subscribes, hands the receiver to `run_socket` | [routes.rs:1336-1339](../../../crates/fah-api/src/routes.rs#L1336-L1339) |
| Engine fan-out — the gate that moves | [main.rs:638-653](../../../crates/fastadhunter/src/main.rs#L638-L653) |
| `GET /clients` handler — stats only, no policy | [routes.rs:347-356](../../../crates/fah-api/src/routes.rs#L347-L356) |
| Per-IP policy resolution to reuse | [routes.rs:991-1009](../../../crates/fah-api/src/routes.rs#L991-L1009) (`client_policy_response`) |
| `ClientResponse` | [wire.rs:599-622](../../../crates/fah-api/src/wire.rs#L599-L622) |
| `ClientPolicyResponse` | [wire.rs:851-860](../../../crates/fah-api/src/wire.rs#L851-L860) |
| `AssignmentResponse` | [wire.rs:785-793](../../../crates/fah-api/src/wire.rs#L785-L793) |
| `policy_for` — linear over a short array; names pre-resolved to addresses when the snapshot is built | [policy.rs:288-295](../../../crates/fah-rules/src/policy.rs#L288-L295) |
| Client table bound: `DEFAULT_CAPACITY = 4096` | [client_registry.rs:16](../../../crates/fah-stats/src/client_registry.rs#L16) |
| WS test harness — tokio-tungstenite, TLS, `?token=` | [api.rs:2496-2588](../../../crates/fah-api/tests/api.rs#L2496-L2588) |
| API.md sections to edit | [§Clients](../../../API.md#L429), [§Events](../../../API.md#L844), [reserved precedent](../../../API.md#L1033) |

## 3. Dependencies and blockers

- **`Depends on: p5-01` — DONE.** Its Implementation Summary constrains nothing
  here: it touched route *composition* (a two-level `/api` → `/v1` nest with a
  JSON fallback, static merged outside the auth layer), not `/events` or
  `/clients`.
- **Branch** `phase5-03` cut from `phase5-02`, per phase `CLAUDE.md` §Parallel
  track. Phase directory stays where it is; no `open` → `wip` move.
- **`request_coverage.rs` needs nothing.** This task adds **no new routes**, and
  `/api/v1/events` is already in `UNCOVERED` ("a WebSocket — the REST Client
  extension cannot open one"). The gate stays green with no fixture work.
- **No blockers.** Harness, fixtures and route coverage are all already in place.

## 4. Contradictions with the specification or architecture

1. **`stats` is not an `Event` variant.** The four subscribable names are
   `query`, `stats`, `config_changed`, `list_refreshed`, but `stats` is produced
   by the local `ticker` arm ([events.rs:139](../../../crates/fah-api/src/events.rs#L139)),
   not by the broadcast channel. Filtering `stats` means gating the ticker, not
   the receiver. The task reads as one filter; it is two mechanisms.

2. **Unsubscribing `stats` produces exactly the silent socket the phase
   forbids.** The protocol as specified permits `{"subscribe":["query"]}` and
   `{"subscribe":[]}`. Both remove the only guaranteed traffic on an idle
   socket, so `SEND_TIMEOUT` can never fire and a peer that vanished without
   closing holds one of 64 connection slots until TCP gives up. The task pushes
   this onto the client ("the client closes instead of idling", `p5-05`), but the
   **server** is what accepts the message. Phase `CLAUDE.md` says a future silent
   socket "owes a ping first"; there is no server-side ping today.

3. **`run_socket` holds no handle on the hub.** Its signature is
   `(socket, broadcast::Receiver<Event>, Arc<S>)`. A query-subscriber counter
   must be owned by `EventHub` and passed in, so both the signature and the call
   site at [routes.rs:1339](../../../crates/fah-api/src/routes.rs#L1339) change.
   `receiver_count()` is self-maintaining; a hand-maintained counter is not — it
   needs an RAII guard so every `break` path and every panic decrements.

4. **Filtering does not remove lag risk as absolutely as the task claims.** A
   stats-only socket still *receives* every `Event::Query` into its broadcast
   buffer and must drain it. Discard is cheap, so lag becomes very unlikely, but
   "can no longer lag past `CHANNEL_CAPACITY` on query volume" is stronger than
   the design delivers. Only the engine-side gate — when *no* socket wants
   `query` — removes the events entirely.

5. **`GET /clients` cost is real, not nominal.** Calling
   `client_policy_response` per row would be up to 4096 × (two `arc-swap` loads
   + one `ip.to_string()` allocation + a walk of every policy's assignments).
   The snapshots must be hoisted out of the loop and the per-row string
   comparison avoided. Not a hot path, but the task demands a measured figure
   rather than an assumed one.

6. **`assignment` matching is a raw string compare** —
   `existing.client == ip.to_string()` — so a non-canonical IPv6 form in the
   TOML reads as "inherited" rather than "direct". Pre-existing; reusing the
   same helper keeps the two endpoints consistent, which is precisely what the
   acceptance criterion asks for. Do not "fix" it in this task.

7. **`phase5-design-review.md` uses stale task numbers.** Its `p5-03 §Shell`,
   `§Socket manager`, `§Size gate` and `§Self-hosted assets` refer to what is now
   **`p5-05`**, and its `D6` row assigns root CLAUDE.md to `p5-03` where the
   phase table assigns it to `p5-05`. Only its **B9** row and decision-register
   row **122** apply to the current `p5-03`. Do not import the rest.

## 5. Acceptance criteria and how they are currently evidenced

| Criterion | Evidence today |
| --------- | -------------- |
| Default subscription delivers all four event kinds | Partial — [api.rs:2496](../../../crates/fah-api/tests/api.rs#L2496) covers `query` end to end, [api.rs:2548](../../../crates/fah-api/tests/api.rs#L2548) covers `stats`. `config_changed` and `list_refreshed` are covered only at `encode` level ([events.rs:253](../../../crates/fah-api/src/events.rs#L253)), never over a socket |
| `{"subscribe":["stats"]}` receives `stats` and no `query` | **None.** Inbound frames are discarded at [events.rs:161](../../../crates/fah-api/src/events.rs#L161) |
| A later wider subscription widens the set | **None** |
| An unknown name leaves the previous set standing and does not close the socket | **None** |
| A stats-only subscriber proven not to lag under a burst that disconnects an unfiltered one | **None.** Only the unit-level lag proof at [events.rs:341](../../../crates/fah-api/src/events.rs#L341) |
| With every socket off `query`, the engine does no per-query publish work — asserted at the fan-out | **None.** The `has_subscribers` unit test ([events.rs:318](../../../crates/fah-api/src/events.rs#L318)) asserts connection presence, which is the semantics being replaced. The assertion point is [main.rs:642](../../../crates/fastadhunter/src/main.rs#L642), which has no test at all |
| `GET /clients` policy and assignment source agree with `GET /clients/{ip}/policy` in the direct, inherited and unassigned cases | **None** — the fields do not exist |
| Bearer-key clients unaffected; existing tests pass untouched | The `api.rs` suite covers this, and it holds as long as the default subscription stays "everything" |
| `request_coverage.rs` green | Already green; no new routes, `/events` already allowlisted |
| Gates green | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` |

## 6. Decisions — settled by the owner, 2026-08-26

These are inputs to `/ce-plan`, not options. Each one closes a contradiction
raised in §4; the cross-reference says which.

### 6.1 Silent socket — server-side `Ping`, not a rejection (closes §4.2)

- The server sends a WebSocket **`Ping` on the `STATS_INTERVAL` cadence whenever
  `stats` is not in the socket's subscription**. That is the traffic
  `SEND_TIMEOUT` needs in order to detect a peer that vanished without closing,
  and it is what phase `CLAUDE.md` means by "it owes a ping first".
- **`{"subscribe":[]}` stays valid.** A subscription that omits `stats` — or
  omits everything — is accepted, not rejected. The socket stays open.
- This preserves the "unknown or invalid names are ignored, the socket stays
  open" rule intact: **no subscription content is ever a reason to close or
  refuse a socket.** The liveness hole is closed on the server side instead of
  being pushed onto client discipline.
- `p5-05`'s client still closes rather than idling. That stays the client's
  policy; it is no longer the server's only defence.

### 6.2 Query-subscriber tracking — `AtomicUsize` + RAII guard (closes §4.3)

- The counter is an **`AtomicUsize` owned by `EventHub`**, alongside the
  `broadcast::Sender`.
- `run_socket` receives an **RAII guard**, so every exit path decrements: the
  lag-disconnect `break`, the `Closed` `break`, the peer-close and transport
  errors, the send timeout, and an unwind. Manual decrements at each `break`
  are rejected — that is the failure mode a guard exists to prevent.
- The guard also carries the socket's current subscription, so a change of
  subscription adjusts the count in one place rather than at each parse site.
- **`has_subscribers()` is renamed `has_query_subscribers()`.** The name is `pub`
  and re-exported at [lib.rs:34](../../../crates/fah-api/src/lib.rs#L34); keeping
  the old name over changed semantics is exactly the drift the rename prevents.
  The sole call site is [main.rs:642](../../../crates/fastadhunter/src/main.rs#L642).

### 6.3 `GET /clients` — the compact contract (closes §4.5, constrains §4.6)

- **Compact, not the full object.** Each item gains the **policy value** plus an
  **explicit source discriminator**. The full `AssignmentResponse` is **not**
  duplicated per row.
- **Null semantics follow `GET /clients/{ip}/policy`**: the discriminator is
  present exactly where that endpoint's `assignment` field is present — an
  assignment naming the address directly — and absent otherwise, under the same
  `skip_serializing_if` treatment. The three cases map as:

  | Case | `policy` | Discriminator |
  | ---- | -------- | ------------- |
  | Direct — an assignment names the address | that policy's id | present |
  | Inherited — covered by subnet or name | the policy in force | absent |
  | Unassigned | `default` | absent |

  Inherited and unassigned are already indistinguishable on
  `GET /clients/{ip}/policy` by the same rule, so the compact form loses nothing
  the per-client endpoint carries. That is the agreement the acceptance
  criterion tests.
- **Escape hatch, stated up front:** if the implementation proves the page
  cannot be built without a per-row field the compact shape omits, the full
  object is reconsidered — with the reason written down. Absent that proof, the
  compact contract ships.
- The exact field name and Rust type are an implementation-plan detail; the
  contract above is what `p5-05`'s typed client is written against.

### 6.4 Publish-side gate — one gate, and it is the query-subscriber count

- **The query-subscriber count is the only engine-side publish gate.** Nothing
  else in the fan-out is conditioned on subscription state.
- The **per-socket stats ticker stays local to `run_socket`** and is explicitly
  *not* a second engine gate. It reads `StatsSource` on demand; it costs the
  engine nothing per query and gates nothing there. §4.1's "two mechanisms" is
  therefore a fact about the socket, not a second gate to build.

### 6.5 Reserved Authentication section — only what `p5-04` has already frozen

Document these, because `p5-04` has settled them:

- cookie attributes — `__Host-` prefix, `Secure`, `HttpOnly`, `SameSite=Strict`,
  `Path=/`, explicit expiry, token from a CSPRNG ≥ 128 bits;
- the authoritative expiry lives **inside the signed token** and is enforced
  server-side; the cookie's `Expires`/`Max-Age` is a client convenience;
- `GET /config` **redacts or omits every `auth.*` field**;
- `POST /config` carrying `auth.*` returns **`422`**;
- the middleware rule — a valid session cookie **or** a bearer key; the bearer
  path is unchanged;
- the `Origin` rule on the upgrade — required and matching the request's own
  effective origin when cookie-authenticated, irrelevant when
  bearer-authenticated;
- `401` reuses the existing `unauthorized` code, and failure messages reveal
  nothing about which half was wrong;
- auth responses carry `Cache-Control: no-store`.

Do **not** document, because `p5-04` has not settled them: exact route paths,
request and response bodies, token format and signing primitive, session
lifetime and any inactivity timeout, first-run/no-password-set behaviour,
Argon2id parameters. Reserved means frozen enough to write a typed client
against — not a promise API.md cannot keep until `p5-04` lands.

### 6.6 Measurement — dev box, documented conversion (closes §4.5's evidence gap)

- `GET /clients` cost is measured **on the dev box** at the registry's bound
  (4096 clients), against a pre-change checkout in the same session — never
  against a stored baseline
  ([docs/measurement-traps.md](../../../docs/measurement-traps.md)).
- The **conversion method is documented alongside the figure**: the measured
  ~9× x86 → RB5009 factor (PERFORMANCE.md §Budgets), never an instantaneous
  clock reading.
- **No on-device requirement is invented for this task.** The review file states
  the corpus, workload and device the figure applies to, and how it can be
  superseded. This task does not become `AWAITING SOAK`.

### Approval — granted 2026-08-26, and written

The three API.md edits were approved as proposed and are **already in the working
tree** (4 hunks, +69/−2). One forced deviation: the reserved section is headed
`## Session authentication *(Phase 5 — reserved)*` rather than
`## Authentication *(Phase 5 — reserved)*`, because API.md already carries
`## Authentication` for the bearer key — the proposed heading would have been a
duplicate H2 and an anchor collision.

What landed:

| Hunk | Content |
| ---- | ------- |
| §Clients sample + note | `policy`, `assignment_source`, and the presence rule |
| §Events | the **Client → server** subsection: message shape, four names, replace-not-accumulate, default-is-everything, ignore-and-stay-open, empty list valid, server-side filtering, the `Ping` substitute when `stats` is unsubscribed |
| §Events, slow-consumer line | "A subscriber that does not ask for `query` is not sent it, and is not charged for it." |
| §Session authentication *(Phase 5 — reserved)* | cookie attributes, token-side expiry, middleware, WS `Origin` rule, `/config` redaction and `422`, `no-store`, and an explicit list of what `p5-04` still owns |

**API.md is now ahead of the code.** The subscription contract reads as live and
is not implemented until §8 lands. That gap closes inside this task's own commit
and must not survive it.

---

## 7. Implementation plan — order of work

Four work items. A and B are independent and touch disjoint files; A is first
because it is where every open risk lives.

| # | Item | Files |
| - | ---- | ----- |
| A | `/events` subscription protocol + engine gate | `events.rs`, `routes.rs`, `main.rs`, `lib.rs` |
| B | `GET /clients` policy fields | `wire.rs`, `routes.rs` |
| C | Tests freezing both contracts | `events.rs` unit, `api.rs` integration |
| D | Measurement + review file | `docs/code-review/phase5/p5-03-api-contracts-review.md` |

## 8. Item A — `/events` subscription protocol

### A1. The subscription set

A small `Copy` bitset local to `events.rs` — **not** in `wire.rs`. `wire.rs`
holds REST bodies; this is the socket's own protocol and `events.rs` already
owns `Envelope` and `encode`. Keeping it local costs nothing and adds no
coupling.

- Four flags: `query`, `stats`, `config_changed`, `list_refreshed`.
- `Subscription::ALL` is the default, set at connect. This is what keeps every
  existing consumer working (API.md publishes today's everything-behaviour).
- Parsing `{"subscribe":[…]}`: **an unknown name rejects the whole message**, it
  is not partially applied. The previous set stands, a `debug` line is logged,
  the socket stays open. Same for a malformed frame or any other message type.
  This is the stricter reading of the task and it is what the API.md wording now
  says, so the two cannot drift.
- An empty list is valid and clears every flag (decision §6.1).

### A2. The counter and its guard

- `EventHub` gains `queries: Arc<AtomicUsize>` beside the `broadcast::Sender`.
- `has_subscribers()` → **`has_query_subscribers()`**, reading that counter.
  `pub`, re-exported at [lib.rs:34](../../../crates/fah-api/src/lib.rs#L34), one
  call site at [main.rs:642](../../../crates/fastadhunter/src/main.rs#L642).
- New `EventHub::subscribe_socket() -> (broadcast::Receiver<Event>, SocketSubscription)`.
  It creates the receiver **and** increments the counter before returning —
  `Subscription::ALL` includes `query`, so there is no window in which a
  connected socket is uncounted.
- `SocketSubscription` is the RAII guard: it owns `Arc<AtomicUsize>` plus the
  current `Subscription`. `set(next)` applies the delta (increment when `query`
  is gained, decrement when lost, nothing otherwise). `Drop` decrements iff the
  current set still holds `query`.
- **Every exit path is covered by `Drop`**, including the lag-disconnect `break`,
  `RecvError::Closed`, the peer close, transport errors, the `SEND_TIMEOUT`
  branch and an unwind. Manual decrements at each `break` are rejected — that is
  the failure mode the guard exists to prevent (decision §6.2).

### A3. `run_socket`

Signature gains the guard: `(socket, events, stats, subscription)`.

The loop is restructured so each `select!` arm yields `Option<Message>` and
`None` means `continue` — today every arm must produce a `String` and fall
through to one `send`.

| Arm | Change |
| --- | ------ |
| `events.recv()` → `Ok(event)` | check the flag for that event's kind **before `encode`**. Unsubscribed → `None`. Encoding a message nobody asked for is pure waste and is skipped, not merely unsent |
| `events.recv()` → `Lagged` / `Closed` | unchanged |
| `ticker.tick()` | `stats` subscribed → the stats push as today. `stats` unsubscribed → `Message::Ping(<empty>)` instead (decision §6.1) |
| `incoming` → `Message::Text` | parse and apply to the guard, then `continue`. Never closes on bad input |
| `incoming` → other frames | `continue`, as today. This already covers the client's `Pong` — it must not be read as a reason to break |

Both the stats push and the `Ping` go through the same `SEND_TIMEOUT` send, so
the existing timeout is the only liveness mechanism and no second one is added.

**Honest limit, to be recorded in the review file, not oversold in the code:** a
`Ping` is a two-byte frame where a stats push is a full JSON payload, so it fills
a stalled peer's TCP buffer far more slowly. It keeps the socket from being
*silent* and lets a dead peer eventually surface as a send error; it is not as
fast a detector as the stats cadence. A true watchdog would track the returning
`Pong` and close after N missed. That is **deferred, not built** — it is new
behaviour beyond the approved contract.

### A4. The engine gate

One line at [main.rs:642](../../../crates/fastadhunter/src/main.rs#L642):
`hub.has_subscribers()` → `hub.has_query_subscribers()`. Nothing else in the
fan-out changes — the counter is maintained on subscribe, so the per-query check
stays one relaxed atomic read with no lock (decision §6.4).

### A5. The route handler

[routes.rs:1336-1339](../../../crates/fah-api/src/routes.rs#L1336-L1339) switches
to `subscribe_socket()` and passes the guard through the upgrade closure into
`run_socket`. No route added, no auth change, no new state.

## 9. Item B — `GET /clients` policy fields

### B1. Wire shape

`ClientResponse` gains:

```rust
pub policy: String,
#[serde(skip_serializing_if = "Option::is_none")]
pub assignment_source: Option<&'static str>,
```

`Some("direct")` when an assignment names the exact address, `None` otherwise —
matching the presence rule of `ClientPolicyResponse::assignment` exactly
(decision §6.3). `&'static str` because `direct` is the only value the contract
defines; a second value would be an API change, not a code change.

The existing `From<ClientEntry> for ClientResponse` **cannot stay** — it has no
access to the policy snapshots. It is replaced by a constructor that takes the
entry plus a resolved `(policy, source)`. Both call sites need the new fields:
`clients()` and `set_client_name()`
([routes.rs:357](../../../crates/fah-api/src/routes.rs#L357)), which returns the
same type.

### B2. One resolution path, shared by both endpoints

A helper built **once per request** and reused per row:

- `Arc<ActivePolicies>` from `state.policies.current()` — one load, not one per
  client;
- the set of assignment `client` strings from `state.config.current()` — one
  walk, not one per client.

Per row: `active.id_of(active.policy_for(ip))`, falling back to
`DEFAULT_POLICY`, plus a set lookup for the direct case.

**`client_policy_response` is refactored to build a one-entry version of the same
helper and use the same code path.** Agreement between the two endpoints then
holds *structurally* rather than only by test — the acceptance criterion asks for
agreement, and shared code is the only way to keep it after the next edit
(principle 4).

**The string comparison is preserved deliberately.** Parsing assignment clients
into `HashSet<IpAddr>` would be cheaper and would remove the per-row
`ip.to_string()`, but it would also *change* which addresses count as direct — a
non-canonical form in the TOML matches after parsing and does not match today
(§4.6). That would make `GET /clients` disagree with `GET /clients/{ip}/policy`,
which is precisely what the acceptance criterion forbids. Correctness before
optimization; the quirk is preserved, shared, and recorded.

### B3. Cost

Bounded by the client registry's own bound — `DEFAULT_CAPACITY = 4096`
([client_registry.rs:16](../../../crates/fah-stats/src/client_registry.rs#L16)) —
times the assignment list, which is a short array. Two snapshot loads and one
config walk per request regardless of client count. One short `String` per row.
Cold endpoint, no hot-path contact, no new retained state.

## 10. Item C — tests

### C1. Unit, in `events.rs`

- Parse: a valid set replaces; an unknown name leaves the previous set standing;
  a malformed frame likewise; an empty list is accepted and clears everything.
- Guard: connect increments; narrowing away from `query` decrements; widening
  back increments; `drop` decrements; dropping a guard already narrowed off
  `query` does not double-decrement.
- **Filtering determinism.** Two receivers, one filtered, one not, driven
  directly — deterministic, unlike a burst over a real socket.

### C2. Integration, in `api.rs` — the harness already exists

- Default socket receives all four kinds. `query` and `stats` are covered today
  ([api.rs:2496](../../../crates/fah-api/tests/api.rs#L2496),
  [api.rs:2548](../../../crates/fah-api/tests/api.rs#L2548)); `config_changed`
  and `list_refreshed` are published through `harness.server.events().publish(…)`
  and asserted end to end for the first time.
- `{"subscribe":["stats"]}` → `stats` arrives, `query` does not within a bounded
  window while queries are being published.
- A later `{"subscribe":["query","stats"]}` widens it and `query` arrives.
- An unknown name leaves the previous set standing and the socket open.
- A subscription omitting `stats` produces a `Ping` on the cadence —
  tokio-tungstenite surfaces `Message::Ping` on the stream.
- **The engine gate, asserted directly.** With a stats-only socket connected,
  `harness.server.events().has_query_subscribers()` is `false`. That call *is*
  the fan-out's condition, so this asserts the gate itself rather than inferring
  it from a socket receiving nothing — which is what the acceptance criterion
  demands. `spawn_event_fanout` is private to the binary and is not made public
  for a test.
- **Risk, stated now:** "a stats-only subscriber does not lag where an unfiltered
  one disconnects" is timing-sensitive over a real socket. The deterministic
  proof is C1; the integration version is a smoke test with generous bounds. If
  it proves flaky it is deleted rather than papered over with sleeps, and the
  review file says so.

### C3. `/clients`

Direct, subnet-inherited, name-inherited and unassigned, each compared field for
field against `GET /clients/{ip}/policy` for the same address.

### C4. Untouched

Bearer-key tests pass unmodified — the default subscription is everything, so no
existing consumer's behaviour changes. `request_coverage.rs` needs nothing: no
new routes, and `/api/v1/events` is already in `UNCOVERED`.

## 11. Measurement

Per decision §6.6, dev box with the conversion documented, not an on-device
requirement.

- `GET /clients` timed at the registry bound (4096 clients) with a realistic
  assignment count, against a pre-change checkout built in the same session —
  never a stored baseline
  ([docs/measurement-traps.md](../../../docs/measurement-traps.md)).
- **No criterion bench target is added.** `fah-api` has none today, and a cold
  endpoint does not earn a permanent bench, a dev-dependency and a maintenance
  surface. The measurement is taken with a scratch harness that is not committed;
  the figure, corpus, workload and device go in the review file.
- Converted with the measured ~9× x86 → RB5009 factor (PERFORMANCE.md §Budgets),
  never an instantaneous clock reading.
- Nothing here touches a hot path, so no `cargo bench` run is required by the
  gates.

## 12. Gates and completion

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
```

No `cargo bench` — no hot path is touched.

On completion: write
`docs/code-review/phase5/p5-03-api-contracts-review.md` with the Implementation
Summary, per phase `CLAUDE.md` §TASK COMPLETION, and stop. The review file
carries the `/clients` figure, the `Ping`-versus-stats detection limit, the
deferred `Pong` watchdog, and the deleted-if-flaky outcome of C2's burst test.

**Explicitly not in this task:** any auth route or behaviour, any frontend
assumption, any further API contract, any route addition. The API.md edits above
are the whole documentation scope.
