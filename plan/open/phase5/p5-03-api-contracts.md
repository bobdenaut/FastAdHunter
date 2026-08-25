# P5-03 — API Contract Additions

**Phase:** 5 · **Depends on:** p5-01 · **Model:** Opus

## Goal

Every API-shape change the dashboard needs, made once and frozen, so the typed
client in `p5-05` is written against a settled contract rather than discovering
one page at a time. Two backend changes and the reserved documentation for a
third.

## Context

The dashboard design was verified against `routes.rs` and found two gaps that are
API problems, not UI problems:

1. **`WS /api/v1/events` has no subscription filter.** `events.rs::run_socket`
   forwards every `Event::Query` to every subscriber and discards anything the
   client sends. A phone parked on Settings receives the full per-query feed —
   the channel is sized for ~250 QPS — and `EventHub::has_subscribers()` turns on
   per-query publish work in the engine as soon as any socket connects.
2. **`GET /api/v1/clients` carries no policy.** The Clients page is designed with
   a policy column on every row, distinguishing an assignment naming the address
   from one inherited via subnet or name. The only source is
   `GET /clients/{ip}/policy`, one request per client — an N+1 on page load.

API.md's compatibility contract permits added fields; both changes are additive.

## Scope

### 1. `/events` subscription protocol

- **Client → server message**, the only one the socket accepts:

  ```json
  {"subscribe": ["stats", "query"]}
  ```

- **Event names**: `query`, `stats`, `config_changed`, `list_refreshed` — the
  four the server already publishes. No wildcards, no new names.
- **Default is every event**, for exactly one reason: API.md publishes today's
  behaviour, where a client connects and receives everything. A default of
  "nothing" silently breaks every existing consumer. The contract is additive —
  a client that wants less says so.
- **Subscribe replaces, it does not accumulate.** One message sets the whole set,
  so unsubscribing is sending a smaller list, and there is no separate
  `unsubscribe` verb to keep consistent.
- **Unknown or invalid names**: the socket stays open and the message is ignored,
  with a `debug` log. A malformed frame must not disconnect a working dashboard —
  contrast `/history/perf`'s `fields`, which is a `400` because a typo there
  silently removes a chart's series, where here the previous set simply stands.
- **Filtering happens server-side, before the send.** This is what removes the
  reconnect loop as well as the bandwidth: a stats-only subscriber sends one
  message per two seconds and can no longer lag past `CHANNEL_CAPACITY` on query
  volume.
- **Filtering happens at the hub, before per-query publish work — not only before
  the send.** `EventHub::has_subscribers()` today returns "is any socket
  connected", and the binary's fan-out uses it to decide whether to do per-query
  work at all: the client-name lookup and boxing the record. **Replace those
  semantics with a has-query-subscribers check**, so a stats-only dashboard
  subscriber does not trigger query-event work anywhere in the engine.

  Filtering only on the send path would leave the publish cost exactly where it
  is and fix the bandwidth alone. That is half the fix, and it is the half that
  does not matter on a LAN.

  The counter is maintained on subscribe/unsubscribe, so the fan-out's check stays
  what it is today — one cheap read, no lock on the path that runs per query.
- **No new per-client buffering.** The existing `tokio::broadcast` capacity, the
  lag-disconnect and `SEND_TIMEOUT` remain the backpressure design and are
  correct. Adding a per-socket queue would be new unbounded-ish state solving a
  problem the filter removes.
- **The 2 s stats cadence is load-bearing for liveness — do not let a socket go
  silent.** `SEND_TIMEOUT` exists because a peer can vanish without closing ("a
  phone leaving Wi-Fi — the normal dashboard client"), and it can only fire if
  something is being sent. A subscription of *no* event types would remove the
  only traffic on an idle socket and let a dead one hold a connection slot until
  TCP gives up. The client closes instead of idling (`p5-05`); if a future change
  ever wants a silent socket, it owes a ping first.

### 2. `GET /clients` gains the in-force policy

- Each item gains the policy in force for that address **and** whether the
  assignment names the address directly or is inherited via subnet or name — the
  same distinction `GET /clients/{ip}/policy` reports by the presence or absence
  of its `assignment` field.
- Field names and null semantics follow that endpoint, so the two cannot drift.
  A client under no assignment reports `default`, as `stats.policies` does.
- Additive: existing consumers see new keys and ignore them.
- Cost check for the review file: this is a per-row policy resolution on a
  request that already walks every observed client. Not a hot path, bounded by
  the client table's own bound — state the measured cost, do not assume it.

### 3. Reserved documentation

API.md gains the Phase 5 contracts **marked reserved**, following the precedent
already in that file (`## Certificates *(Phase 3 — reserved)*`):

- `## Authentication *(Phase 5 — reserved)*` — the routes, the cookie attributes,
  what the WebSocket upgrade accepts, and the `auth.*` rules on `/config` that
  `p5-04` implements;
- the `/events` subscription contract, reserved until this task's own commit
  promotes it, since this task implements it.

**Reserved means the contract is frozen enough to write a typed client and tests
against, and clearly not yet shipped.** The commit that implements a route
promotes its section to live wording. Documenting unbuilt routes as live would
make API.md lie until `p5-04` lands.

## Acceptance criteria

- Protocol tests, before `p5-05` exists: default subscription delivers all four
  event kinds; a `{"subscribe":["stats"]}` socket receives `stats` and **no**
  `query`; a later `{"subscribe":["query","stats"]}` widens it; an unknown name
  leaves the previous set standing and does not close the socket.
- A stats-only subscriber is proven not to lag under a sustained query burst that
  would disconnect an unfiltered one.
- With every connected socket subscribed away from `query`, the engine performs
  no per-query publish work — asserted at the fan-out, not inferred from the
  socket receiving nothing. A stats-only subscriber must cost the engine what no
  subscriber costs it.
- `GET /clients` returns policy and assignment-source per item, agreeing with
  `GET /clients/{ip}/policy` for the same address in the direct, inherited and
  unassigned cases.
- Bearer-key clients are unaffected: existing tests pass untouched.
- `request_coverage.rs` green — new routes carry fixtures under `requests/`, or
  an `UNCOVERED` entry with its reason.
- Gates green.

## Out of scope

Authentication itself (`p5-04`) — this task writes its reserved contract, not its
code. Any frontend (`p5-05`). Any change to what an event's `data` payload
contains.

## Suggested prompt

> Read API.md §Events and §Clients, `crates/fah-api/src/events.rs`,
> `crates/fah-api/src/routes.rs`, and
> plan/open/phase5/p5-03-api-contracts.md. Add the `/events` subscription
> protocol with server-side filtering and move the engine-side publish gate to
> match, add the in-force policy and assignment source to `GET /clients`, and
> write the reserved API.md sections. Freeze the protocol with tests before any
> frontend exists. Propose the API.md edits and wait for approval before making
> them.
