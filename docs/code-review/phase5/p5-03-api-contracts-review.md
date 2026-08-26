# p5-03 — API Contract Additions — Review

## Implementation Summary

The `/events` socket now takes a subscription and filters server-side, the
engine's per-query publish gate moved from "any socket connected" to "any socket
wants queries", and `GET /clients` carries the policy in force plus whether the
assignment names the address directly.

| What | Where |
| ---- | ----- |
| `Subscription` bitset, `SubscribeMessage`, `SocketSubscription` RAII guard | `crates/fah-api/src/events.rs` |
| `EventHub.queries` counter, `subscribe_socket()`, `has_query_subscribers()` | `crates/fah-api/src/events.rs` |
| `run_socket` filtering, the `Ping` substitute, inbound frame handling | `crates/fah-api/src/events.rs` |
| `PolicyResolver`, `client_response`, `client_policy_response` refactor | `crates/fah-api/src/routes.rs` |
| `ClientResponse.policy` / `.assignment_source`; `From<ClientEntry>` removed | `crates/fah-api/src/wire.rs` |
| The engine-side gate | `crates/fastadhunter/src/main.rs` |
| Protocol and contract tests | `crates/fah-api/src/events.rs`, `crates/fah-api/tests/api.rs` |
| §Clients, §Events client→server, §Session authentication (reserved) | `API.md` |

### The subscription protocol

`{"subscribe":[…]}` is the only message the socket acts on. The set replaces
rather than accumulates, the default is every event so no existing consumer
changes behaviour, and **any unusable frame leaves the previous set standing**
without closing the socket — an unknown name rejects the whole message rather
than being partially applied, which is the stricter reading and the one API.md
now states. An empty list is valid.

Filtering happens **before `encode`**, not merely before the send: a message
nobody asked for is never rendered.

### The engine-side gate

`EventHub` owns an `AtomicUsize`. `subscribe_socket()` increments it and hands
back a `SocketSubscription` whose `Drop` decrements — so the lag-disconnect, the
channel close, a peer close, a transport error, the send timeout and an unwind
all decrement through one path. `has_query_subscribers()` replaced
`has_subscribers()`; the name changed because the semantics did, and there is one
call site (`main.rs`). The fan-out's per-query check is still one relaxed atomic
load with no lock.

### The `Ping`

A socket that has unsubscribed `stats` would otherwise go silent, and
`SEND_TIMEOUT` can only fire if something is being sent. The stats ticker now
emits `Message::Ping` on the same cadence when `stats` is not subscribed, through
the same timed send.

### `GET /clients`

`PolicyResolver` is built once per request — one policy snapshot, one walk of the
configured assignments into a `HashMap` — and read per row. `client_policy_response`
was refactored onto the same type, so the two endpoints agree on "direct"
structurally rather than only by test.

The lookup deliberately stays a comparison of the configured `client` string
against `ip.to_string()`. Parsing the configured side into an address would be
cheaper and would remove the per-row `String`, but it would change which
addresses count as direct (a non-canonical form in the TOML) and make the two
endpoints disagree — which is exactly what the acceptance criterion forbids.
First-wins on duplicate `client` keys is preserved via `entry().or_insert_with()`,
matching the `find` it replaced.

### Decisions carried in from the plan

All six pre-plan decisions were settled by the owner before implementation and
are recorded in `plan/wip/phase5/p5-03-api-contracts-plan.md` §6. Nothing was
re-decided here.

### Tests

`cargo test --all-features --workspace`: 44 test binaries, 0 failures.

Unit (`events.rs`, 13 tests in the module):

- a subscription message replaces the whole set; an empty list is valid;
- an unusable frame (unknown name, wrong type, wrong key, non-JSON, empty)
  leaves the previous set standing;
- filtering decides per event kind, including `stats`;
- the counter tracks sockets independently, narrowing and widening move it, a
  guard already narrowed off `query` does not double-decrement on drop;
- **the deterministic filter proof**: a burst past `CHANNEL_CAPACITY` lags the
  unfiltered receiver while the filtered one drains it and holds no backlog.

Integration (`tests/api.rs`, real TLS WebSocket):

- the default subscription delivers all four kinds — `config_changed` and
  `list_refreshed` are asserted end to end for the first time;
- a stats-only socket receives no `query`, and `has_query_subscribers()` is
  `false` while it is connected — the engine gate asserted at the fan-out's own
  condition, not inferred from the socket receiving nothing;
- widening restores delivery, proven with a marker query from a distinct client
  address so a leaked burst event cannot be mistaken for it;
- two unusable frames in a row leave the set standing and the socket still
  applies a valid one afterwards;
- a socket without `stats` receives a `Ping`;
- `/clients` reports policy and assignment source for the direct,
  subnet-inherited, name-inherited and unassigned cases, each compared against
  `GET /clients/{ip}/policy` for the same address.

No new routes, so `request_coverage.rs` needed no fixture and no `UNCOVERED`
entry. Bearer-key tests were not modified.

### Measurement — `GET /clients`

Dev box (x86, `--release`), 4096 clients in the registry, median of 21 requests
after 3 warm-ups, over the real TLS listener. A/B against a **pre-change git
worktree at `ff38de0` built in the same session**, not a stored baseline.

| Build | Median |
| ----- | ------ |
| Pre-change (`ff38de0`) | 4.69 / 4.75 / 4.77 ms |
| With the change, no assignments configured | 5.99 / 5.99 / 6.05 ms |
| With the change, 8 assignments configured | 6.03 ms |

Added cost ≈ **+1.28 ms per request at 4096 clients**, ≈ 0.31 µs per client.
Configured assignments do not move it measurably — the per-row work is a hash
lookup, and the one walk of the assignment list is per request.

Part of that delta is the larger response, not the resolution: every item gained
a `policy` string and most gained nothing else, and the figure is measured
end-to-end over TLS including JSON serialization.

Converted with the measured ~9× x86 → RB5009 factor (PERFORMANCE.md §Budgets):
≈ **11.5 ms added on the RB5009 at the registry's 4096-client bound**. A
household sees tens of clients, where the same figure is tens of microseconds.
Cold endpoint, no hot-path contact, no new retained state. The measuring harness
was scratch and is not committed.

**Scope of this figure:** dev box, `--release`, 4096 synthetic clients, 0 or 8
assignments, one request at a time. It says nothing about concurrent load and is
superseded by any measurement on the device itself.

### Known limitations and deferred items

1. **The `Ping` is a weaker liveness detector than the stats push.** It is a
   two-byte frame where a stats push is a full JSON payload, so it fills a
   stalled peer's TCP buffer far more slowly. It keeps the socket from going
   silent and a dead peer eventually surfaces as a send error, but detection is
   slower than on a stats-subscribed socket. **A `Pong` watchdog — track the
   reply, close after N missed — is deliberately not implemented**: it is
   behaviour beyond the approved contract.
2. **No integration burst test.** The lag/filter proof is deterministic at unit
   level. An integration version would be timing-sensitive over a real socket,
   and the alternative was sleeps or retries, which were ruled out. It was not
   written rather than written and papered over.
3. **`events.rs`'s module doc is now narrower than the code.** It still describes
   the broadcast fan-out as reaching all connected sockets. Hard rule 7 forbids
   adding or editing Rust comments, so it was left as it stands; API.md §Events
   carries the current contract.
4. **API.md's reserved section is headed `## Session authentication`**, not
   `## Authentication`, because `## Authentication` already documents the bearer
   key — the proposed heading would have been a duplicate H2 and an anchor
   collision. Approved wording, changed heading only.
5. **`Ordering::Relaxed` on the counter.** It orders no other data; it is a
   single location read as a hint by the fan-out and asserted by tests through
   the same location. Nothing depends on it synchronizing with anything else.

## Findings

Boundary: implementation attributable to p5-03 on `phase5-03`, against the p5-03
plan and the approved API.md contract. p5-01, p5-02 and P2.6 not re-reviewed.

Gates re-run by the reviewer: `cargo fmt --all -- --check` green,
`cargo clippy --workspace --all-targets -- -D warnings` green,
`cargo test --all-features --workspace` green (`fah-api`: 88 unit + 69
integration + 2 coverage, 0 failures).

Plan compliance: A1–A5, B1–B3, C2, C3, C4 and the documentation scope are
implemented as specified. C1's third item and one acceptance criterion are not —
finding 1. No unauthorized architectural decision was found; the two deviations
from the plan text are findings 2 and 4.

### Major

**1. The lag/burst acceptance criterion is not evidenced by the test that claims
it.** — **Status: FIXED** `a_filtered_socket_drains_a_burst_that_would_lag_an_unfiltered_one`
([events.rs:557](../../../crates/fah-api/src/events.rs#L557)) drains `filtered`
with `try_recv` inside the publish loop and never drains `unfiltered` at all. It
proves "a receiver that is drained does not lag, one that is not does" — which is
true with or without the subscription filter. `stats_only.wants(&event)` appears
only inside an assertion; deleting the whole filter would not fail the test.
Nothing exercises `run_socket`'s actual drain rate, and the plan's C2 burst smoke
test was not written (limitation 2). *Impact:* the criterion "a stats-only
subscriber is proven not to lag under a sustained query burst that would
disconnect an unfiltered one" is unmet, and the review's "holds no backlog" claim
is a property of the test harness, not of the socket loop. The residual is real:
while **any** socket keeps the engine gate open, every stats-only socket still
receives the full firehose into its broadcast buffer and stays eligible for
`RecvError::Lagged` — which disconnects it over traffic it declined. The plan
predicted this in §4.4; the implementation did not close it and the review file
does not record it as a limitation. *Fix before `DONE`* — either a test that
drives `run_socket` (or an equivalent loop) against a burst, or an explicit
downgrade of the criterion recorded here with the residual stated.

**2. `EventHub::subscribe()` survives and now silently bypasses the gate.**
— **Status: FIXED**

[events.rs:86](../../../crates/fah-api/src/events.rs#L86) is still `pub` on a
type re-exported at [lib.rs:34](../../../crates/fah-api/src/lib.rs#L34), and its
receiver is uncounted. With no other socket connected,
`has_query_subscribers()` stays `false`, `spawn_event_fanout` never calls
`publish_query`, and such a receiver gets zero query events forever while
appearing correctly subscribed. Its only remaining callers are two unit tests.
*Impact:* a public API whose failure mode is silence, not an error — exactly the
drift the `has_subscribers` → `has_query_subscribers` rename was chosen to
prevent (plan §6.2). *Fix before `DONE`*: delete it and move the two tests to
`subscribe_socket()` (principle 14), or make it private.

### Minor

**3. `client_policy_response` regressed from short-circuit to full-map build.**
— **Status: FIXED**

[routes.rs:1002-1046](../../../crates/fah-api/src/routes.rs#L1002-L1046): the
old path was one `find` over the assignment list, allocating only the matched
`AssignmentResponse`. It now calls `PolicyResolver::build`, which clones every
assignment's `client` `String` and materializes a full `AssignmentResponse` (a
`String` plus three cloned `Option`/`Vec` fields,
[routes.rs:830](../../../crates/fah-api/src/routes.rs#L830)) for **every**
configured assignment, to answer one lookup. Hit by `GET`, `POST` and `DELETE`
`/clients/{ip}/policy` and by `PUT /clients/{ip}`. Bounded by the assignment
count (short array), no hot-path contact, and it buys the structural agreement
the acceptance criterion asks for — so the trade is defensible, but it is a cost
the plan did not price. *Not blocking.*

**4. `/clients` builds assignment values it never reads.** — **Status: FIXED**

`client_response`
uses `resolver.assignment_of(ip).map(|_| DIRECT_ASSIGNMENT)`
([routes.rs:352](../../../crates/fah-api/src/routes.rs#L352)) — only presence
matters. Plan §B2 specified "the set of assignment `client` strings"; the
implementation stores `HashMap<String, AssignmentResponse>`. Sharing one type
with finding 3's endpoint is the reason, and it is the smaller evil, but the
deviation is undocumented in the Implementation Summary. *Not blocking.*

**5. No frame-size bound on the events socket.** — **Status: FIXED**

[routes.rs:1374](../../../crates/fah-api/src/routes.rs#L1374) takes
`WebSocketUpgrade` with axum's defaults (64 MiB message / 16 MiB frame). Every
`Message::Text` is now handed to `serde_json::from_str`
([events.rs:124](../../../crates/fah-api/src/events.rs#L124)) and, at `debug`,
logged whole ([events.rs:191](../../../crates/fah-api/src/events.rs#L191)). The
frame allocation itself predates p5-03 (inbound frames were already received and
dropped); the parse is new and roughly doubles peak for a hostile frame — a
valid-JSON `subscribe` array of megabytes materializes a `Vec<String>` beside it.
Authenticated-only, so this is hardening, not an exploit; it is still unbounded
state on a 1 GB box shared with RouterOS (hard rule 4). *Concrete fix:*
`upgrade.max_message_size(4096)` at the call site, or a length check before
`parse`. *Not blocking.*

**6. Two doc comments now contradict the code.** — **Status: FIXED**

[main.rs:639-641](../../../crates/fastadhunter/src/main.rs#L639-L641) still says
the publish work is bought "when a dashboard is actually connected" and calls
`no-subscribers` the idle state — both describe the replaced semantics, and this
sits directly above the gate that changed. The `events.rs` module doc
([events.rs:3-7](../../../crates/fah-api/src/events.rs#L3-L7)) still says the
channel fans every event out to all connected sockets; the author recorded that
one (limitation 3) but not `main.rs`. Hard rule 7 forbids adding or editing Rust
comments — it does not forbid **deleting** them, which is the resolution that
does not leave a false statement next to the code it describes. Owner's call.

**7. The RB5009 conversion is applied to a mixed delta.** The measured +1.28 ms
covers policy resolution *and* a larger response body — 4096 extra `policy`
strings serialized and pushed through TLS — as the Implementation Summary itself
notes. The ~9× x86 → RB5009 factor is a CPU factor; the transfer share of the
delta does not scale by it, so "≈11.5 ms on the RB5009" is an upper bound rather
than an estimate. Also unreported: any spread (median of 21 with no min/max or
IQR), and the 8-assignment row is a single run against three for the others. The
conclusion — cold endpoint, tens of microseconds at household scale — is
unaffected. *Not blocking; the figure's scoping paragraph should say "upper
bound".* **Status: OPEN**

**8. The integration test for unusable frames cannot distinguish ignored from
partially applied.** — **Status: FIXED**
 `an_unusable_subscription_frame_leaves_the_previous_set_standing`
([api.rs:2805](../../../crates/fah-api/tests/api.rs#L2805)) sends
`{"subscribe":["query","nonsense"]}` and `not json at all`, then a valid widening
frame, and only samples `has_query_subscribers()` after the valid one. A
partially-applied bad frame — which contains `query` — would produce the same
final `true`. The test does prove the socket survived; the reject-whole-message
rule is proven only by the unit test at
[events.rs:508](../../../crates/fah-api/src/events.rs#L508). *Cheap fix:* assert
`false` between the bad frames and the good one.

**9. API.md overstates the contract in two places.** — **Status: FIXED**

- §Events slow-consumer line: "A subscriber that does not ask for `query` is not
  sent it, and is not charged for it." It *is* charged the broadcast drain and
  remains eligible for lag-disconnection whenever another socket holds the engine
  gate open (finding 1). Only the all-sockets-off-`query` case is free.
- §Events client→server: "An unknown name, a malformed frame or any other message
  leaves the previous set standing, is logged at `debug`". Binary and `Pong`
  frames take `Some(Ok(_)) => continue`
  ([events.rs:283](../../../crates/fah-api/src/events.rs#L283)) with no log. The
  "leaves the set standing" half is accurate; the logging half is not.

*Not blocking; both are wording.*

### Nitpick

**10. `pub` with no reach.** `Subscription`, `Subscription::ALL` and
`SocketSubscription` are `pub` inside `mod events;`, which is private
([lib.rs:20](../../../crates/fah-api/src/lib.rs#L20)) and re-exports only `Event`
and `EventHub`. Nothing outside the crate can name them. Harmless, but the
`pub const ALL` reads as contract surface it is not. **Status: OPEN**

**11. `assignment_source: "direct"` does not imply `policy` came from that
assignment.** `policy_of` reads the live snapshot (schedule-aware);
`assignment_of` reads the configured list unconditionally. A direct assignment
inside a closed schedule window therefore reports `direct` beside whatever policy
is actually in force. `GET /clients/{ip}/policy` has always behaved this way, so
the endpoints agree and the acceptance criterion holds — API.md just does not say
it. **Status: OPEN**

**12. Two `String` allocations per row on `/clients`.** `policy_of` allocates
from an `Arc<str>` the snapshot already owns
([policy.rs:307](../../../crates/fah-rules/src/policy.rs#L307)) and
`assignment_of` allocates `ip.to_string()` — 8192 short allocations per request
at the 4096 bound, plus the ~800 KB body the endpoint already built before this
task. All transient, nothing retained, and the plan priced the `ip.to_string()`
deliberately (§B2, to preserve the string-compare quirk). Recorded as the shape
to revisit only if this endpoint ever stops being cold. **Status: OPEN**

**13. `PUT /clients/{ip}` gained the new fields untested.** — **Status: FIXED**

[routes.rs:390](../../../crates/fah-api/src/routes.rs#L390) returns the same
`ClientResponse`, so it now carries `policy`/`assignment_source`;
`clients_list_and_naming_round_trip` asserts only `name` on that response.

**14. Unrelated change in the working tree.**
`docs/code-review/phase2.6/p2.6-11-optin-deploy-soak-review.md` (+48, a day-1
soak reading) is modified on `phase5-03`. Phase `CLAUDE.md` §Parallel track keeps
2.6 work off this branch chain. Exclude it from this task's commit.
**Status: OPEN**

### Verified clean

| Focus area | Verdict |
| ---------- | ------- |
| Subscription state transitions | Replace-not-accumulate, empty list valid, unknown name rejects the whole message, socket never closed on bad input — code and tests agree with API.md |
| RAII accounting on every exit path | `Drop` is the only decrement; `Lagged`, `Closed`, peer close, transport error, `SEND_TIMEOUT` and unwind all reach it. Increment happens after `sender.subscribe()`, so no window exists where a socket is counted but not receiving |
| Double-decrement | Guarded by `if self.current.wants_query()` in `Drop`; delta logic in `set` is exhaustive over the four transitions and tested |
| Engine gate correctness | `spawn_event_fanout` produces only query events, so gating it on the query count alone is right; `config_changed`/`list_refreshed` publish unconditionally from `routes.rs` and still reach their subscribers. `Relaxed` is sound — a single location, read as a hint, ordering no other data |
| Stats vs broadcast separation | Ticker stays local to `run_socket`, reads `StatsSource` on demand, gates nothing in the engine (plan §6.4) |
| Ping behaviour | Sent only when `stats` is unsubscribed, on the same cadence, through the same `SEND_TIMEOUT` send. Client `Pong` lands on `Some(Ok(_)) => continue` and is not a disconnect |
| Filtering before encode | `Ok(event) if !subscription.current().wants(&event) => continue` precedes `encode` — confirmed, not merely before the send |
| `/clients` vs `/clients/{ip}/policy` | One `PolicyResolver` type serves both; agreement is structural. First-wins on duplicate `client` keys preserved by `entry().or_insert_with()`, matching the replaced `find` |
| Snapshot consistency | Both snapshots loaded once per request and reused across all rows — an improvement over the previous per-call loads |
| Cancellation | Every `select!` arm is cancel-safe (`broadcast::recv`, `Interval::tick`, tungstenite's buffered `poll_next`); the `continue` paths add no new hazard |
| Panic/unwrap | No new `unwrap`/`expect` outside tests; `encode`/`encode_stats` keep their `unwrap_or_else` fallbacks |
| Layering | `fah-api` gains no dependency; `ClientEntry` import moved `wire.rs` → `routes.rs`; `layering.rs` green |
| Bearer-key regression | Default subscription is `ALL`, existing tests unmodified and passing |
| `request_coverage.rs` | No new routes; green |

## Fixes applied — 2026-08-26

Findings 1, 2, 5 and 8 were approved and fixed. Findings 3, 4, 6, 7, 9, 11, 12
and 13 stay deferred as recorded above. No other behaviour was touched.

### F2 — `EventHub::subscribe()` deleted

Gone from `events.rs`. Its two unit tests (`subscribers_receive_published_events`,
`a_subscriber_that_falls_behind_is_told_it_lagged`) now take their receivers from
`subscribe_socket()` and hold the guard, so every receiver in the tree is counted
and there is no longer a public way to get an uncounted one.

### F1 — the burst proof now drives the real socket loop

**The seam.** `run_socket` is now a thin concrete wrapper over a private
`drive_socket<T, S, E, F>` generic in the transport
(`T: Stream<Item = Result<Message, E>> + Sink<Message, Error = F> + Unpin`).
`axum::extract::ws::WebSocket` satisfies it unchanged; nothing public moved, no
route or state changed, and the loop body is byte-for-byte the previous one. The
only behavioural difference is that the final close is `SinkExt::close` rather
than the inherent `WebSocket::close` — both send a Close frame and flush.

**The test.** `a_stats_only_socket_drains_a_burst_that_disconnects_an_unfiltered_one`
runs two real `drive_socket` tasks against one `EventHub`, over an in-memory
transport (`futures-channel`, added as a **dev-dependency only**). Both sockets
model the **same peer**: an outgoing channel that accepts two messages and then
stops. The only difference between them is the subscription.

The burst publishes `CHANNEL_CAPACITY + 64` query events, yielding between each
so both tasks are scheduled. The unfiltered socket fills its peer's two slots
(one stats push, one query), parks in `send`, stops draining, and falls
318 events behind a 256-slot channel; when the test finally drains its peer it
resumes, reads `Lagged`, and disconnects. The stats-only socket sends nothing
for those events, drains all 320, and is still running when the unfiltered one
has already died — asserted with `!JoinHandle::is_finished()` **after** the
unfiltered task has completed, so it is a real observation and not a "hasn't got
round to failing yet". It then exits on the peer's close, and its emitted
messages are exactly `["stats"]` — not one burst event leaked.

**Mutation-checked.** With the filter disabled (`wants` short-circuited to
false), the test fails on the delivered set (`["query", "query"]` vs `["stats"]`)
and the surviving socket dies by `SEND_TIMEOUT` instead — 15.02 s versus 0.00 s.
The test therefore discriminates on the filter, which the replaced test did not.

**No sleeps, no retries, no wall-clock dependency.** Every wait is an `await` on
a channel or a `JoinHandle`. The whole unit module runs in 0.00–0.01 s.

**The residual named in finding 1 stands and is not closed by this test.** A
stats-only socket still receives every `Event::Query` into its broadcast buffer
whenever another socket keeps the engine gate open, and it still parks in `send`
on each 2 s stats push. What the filter buys is that it parks **once every two
seconds instead of once per event** — that is why it drains and the unfiltered
one does not. A peer stalled long enough for `SEND_TIMEOUT` would lag a
stats-only socket too. Only the engine-side gate — no socket wanting `query` —
removes the events entirely.

### F5 — inbound frames capped at 4096 bytes

`events::MAX_CLIENT_MESSAGE_BYTES = 4096`, applied as
`upgrade.max_message_size(…)` in `events_socket`. Axum's default was 64 MiB. A
legitimate `subscribe` message is tens of bytes. Read-side only — outbound stats
pushes are unaffected.

New test `an_oversized_frame_is_refused_and_releases_the_subscription` sends an
8 KiB frame and asserts `has_query_subscribers()` falls to `false`, which proves
both the cap and that the RAII guard releases on a transport error.

### F8 — the intermediate assertion

`an_unusable_subscription_frame_leaves_the_previous_set_standing` now asserts
`!has_query_subscribers()` after the two unusable frames and before the widening
frame. **What it proves and does not prove:** the subscription is persistent
state, so a partial application of `{"subscribe":["query","nonsense"]}` would
leave the counter `true` until a later usable frame replaced it — this assertion
catches that. It cannot catch a partial application the server has not read off
the socket yet, since there is no ack in the protocol. It fails only on a real
regression, never spuriously. The airtight proof of reject-whole-message stays
the unit test.

### New — recorded, not fixed

**15. The 4096-byte cap narrows API.md's "never closes the socket" rule.**
— **Status: FIXED** (see the second cleanup round below). An
oversized frame is now a transport error, so the socket closes — where API.md
§Events says an unusable message "never closes the socket". The rule was written
about message *content*, and the cap is about size, but the two sentences now sit
next to each other and the document does not say so. An API.md edit needs the
owner's approval (root CLAUDE.md §Working agreement 1), so it is proposed, not
made.

### Verification

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | green |
| `cargo clippy --workspace --all-targets -- -D warnings` | green |
| `cargo test --all-features --workspace` | green — 44 test binaries, 0 failures |
| `fah-api` | 88 unit (one test replaced, not added), 70 integration (+1), 2 coverage |

`Cargo.lock` gained `futures-channel` as a `fah-api` dev-dependency; it was
already in the tree transitively, so no new crate is downloaded and the runtime
binary is unchanged.

The `docs/code-review/phase2.6/p2.6-11-…` working-tree change (finding 14) was
not touched and must stay out of this commit.

## Cleanup round — 2026-08-26

Findings 3, 4, 6, 9, 13 and 15 approved and fixed. Findings 7, 11 and 12 stay
deferred. No API semantics changed; the four `/clients` agreement cases and the
whole suite are unchanged and green.

### F3 + F4 — one resolution path, no map for a single address

The string-compare quirk now lives in **one** place, `assignment_key(ip)`, and
both endpoints call it. Two shared free functions carry the rest:

| Function | Used by |
| -------- | ------- |
| `assignment_key(ip)` | both — the `ip.to_string()` form that is compared |
| `policy_in_force(active, ip)` | both — snapshot lookup, `default` fallback |
| `direct_assignment(config, key)` | the single-address path — the original short-circuiting `find` |

`client_policy_response` no longer builds a resolver. It loads the two snapshots
and runs the same `find` the code had before p5-03, allocating exactly one
`AssignmentResponse` — the matched one. The per-assignment `String` clone and
`AssignmentResponse` construction are gone from `GET`/`POST`/`DELETE`
`/clients/{ip}/policy` and from `PUT /clients/{ip}`.

`PolicyResolver` now holds `HashSet<String>` instead of
`HashMap<String, AssignmentResponse>` — `/clients` only ever needed presence, so
the values were pure waste (finding 4). This also matches what plan §B2
specified.

**Agreement is still structural.** Both endpoints resolve the policy through
`policy_in_force` and both decide "direct" by comparing the configured `client`
string against `assignment_key(ip)`. `HashSet::contains(&key)` and
`find(|a| a.client == key)` answer identically over the same iteration, so the
container difference cannot make them disagree; first-wins on duplicate keys is
irrelevant to a presence test. The four-case agreement test
(`clients_carry_the_in_force_policy_and_agree_with_the_per_client_endpoint`)
passes unchanged.

### F13 — the `PUT` response asserted

`clients_list_and_naming_round_trip` now asserts `policy == "default"` and the
absence of `assignment_source` on the naming response, so the shared
`client_response` shape is covered on both routes that return it.

### F6 — the two stale comments deleted

`main.rs`: the three-line block above the gate ("only bought when a dashboard is
actually connected — no-subscribers is the appliance's idle state") is gone.

`events.rs`: the module-doc paragraph is gone; only the one-line header remains.
**Collateral, stated plainly:** that paragraph's later sentences (broadcast drops
rather than back-pressures; a lagging socket is closed) were still accurate, and
they went with the stale first sentence. Hard rule 7 blocks rewriting a comment
to keep half of it — the hook rejects any edit whose replacement text contains a
comment line — so the choice was the whole paragraph or a standing falsehood.
Both facts are in API.md §Events, which is where the contract belongs.

### F9 + F15 — API.md

| Change | Wording |
| ------ | ------- |
| Slow-consumer line | "not sent it" no longer claims "not charged for it". A new paragraph says that while any socket still asks for `query` every socket receives and drains those events, and a stalled peer can still fall behind; only when no socket asks does the engine stop producing them |
| Frame handling | An unknown name or an unusable text frame leaves the set standing and is logged at `debug`; **binary frames and `Pong` are ignored silently** — which is what the code does |
| 4096-byte cap | Documented as a WebSocket-layer limit, explicitly contrasted with an in-cap malformed message: an oversized frame **closes the connection**, a malformed one does not |

### Verification

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | green |
| `cargo clippy --workspace --all-targets -- -D warnings` | green |
| `cargo test --all-features --workspace` | green — 44 test binaries, 0 failures |
| `fah-api` | 88 unit, 70 integration, 2 coverage |

The `docs/code-review/phase2.6/p2.6-11-…` working-tree change (finding 14) was
not touched and must stay out of this commit.

## Status

**PASS WITH DEFERRED FINDINGS.** Fixed and verified: 1, 2, 3, 4, 5, 6, 8, 9, 13,
15. Deferred: **7** (the RB5009 figure is an upper bound, not an estimate),
**11** (`direct` does not imply the policy came from that assignment), **12**
(two `String` allocations per row at the 4096-client bound) — all three are
recorded above, none changes behaviour. **14** is operational: keep the P2.6
soak edit out of this commit.

---

## Second review pass — 2026-08-26

Independent re-review of the whole p5-03 change against the task file, the plan
and API.md. Boundary respected: p5-01, p5-02 and P2.6 not re-reviewed; the
subscription protocol and the `/clients` payload were not redesigned.

Base for the diff: `ff38de0`. The change is entirely in the working tree
(`API.md`, `Cargo.lock`, `crates/fah-api/{Cargo.toml,src/events.rs,src/routes.rs,src/wire.rs,tests/api.rs}`,
`crates/fastadhunter/src/main.rs`) — 904 insertions, 77 deletions.

Gates re-run by this reviewer: `cargo fmt --all -- --check` green,
`cargo clippy --workspace --all-targets --message-format=short -- -D warnings`
green, `cargo test --all-features --workspace` green — 44 test binaries, 0
failures. `Cargo.lock` gains one line (`futures-channel` under `fah-api`); no new
crate enters the tree.

### Previously-recorded fixes — verified in the code

| Finding | Verified |
| ------- | -------- |
| 1 | `a_stats_only_socket_drains_a_burst_that_disconnects_an_unfiltered_one` drives two real `drive_socket` tasks over a 2-slot in-memory peer; no sleep, no wall-clock wait on the passing path |
| 2 | `EventHub::subscribe()` is gone; the only receiver constructor is `subscribe_socket()` |
| 3 + 4 | `client_policy_response` is back to the short-circuiting `find` via `direct_assignment`; `PolicyResolver` holds `HashSet<String>` |
| 5 | `upgrade.max_message_size(MAX_CLIENT_MESSAGE_BYTES)` present — but see finding 16 |
| 6 | Both stale comment blocks deleted (`main.rs` gate, `events.rs` module doc) |
| 8 | The intermediate `!has_query_subscribers()` assertion is present |
| 9 + 15 | API.md carries the corrected slow-consumer paragraph, the silent-ignore rule for binary/`Pong`, and the oversized-frame-closes-the-connection contrast |
| 13 | `clients_list_and_naming_round_trip` asserts `policy` and the absence of `assignment_source` on the `PUT` response |

Deferred findings 7, 11, 12 and 14 re-checked and unchanged; 10 re-checked and
unchanged (`Subscription`, `Subscription::ALL`, `SocketSubscription` are still
`pub` inside a private module).

Focus areas re-verified independently and clean: subscription state transitions
(`Subscription::parse` returns `None` on the first unknown name, so no partial
apply); RAII accounting (`Drop` is the sole decrement, guarded by
`wants_query()`; the guard is moved into the `on_upgrade` closure, so a failed
upgrade drops it and decrements); the engine gate (`spawn_event_fanout` is the
one call site and produces only query events; `config_changed`/`list_refreshed`
still publish unconditionally from `routes.rs`); stats-vs-broadcast separation
(ticker local to `drive_socket`, `Ping` only when `stats` is unsubscribed,
through the same `SEND_TIMEOUT` send); filtering before `encode` (the `Ok(event)
if !…wants(&event) => continue` arm precedes the `encode` arm); `/clients`
agreement with `/clients/{ip}/policy` (both compare against `assignment_key(ip)`
and both resolve through `policy_in_force`).

### New findings

**16. The 4096-byte cap does not bound the inbound allocation — `max_frame_size`
is still 16 MiB.** — **Status: OPEN**

`events_socket` sets only `max_message_size`
([routes.rs:1387](../../../crates/fah-api/src/routes.rs#L1387)). In
tungstenite 0.29 those are two different limits, enforced at two different
points:

| Limit | Where enforced | Value here |
| ----- | -------------- | ---------- |
| `max_frame_size` | `read_frame`, from the frame header, **before** the payload is read — `in_buffer.reserve(len)` follows immediately (`protocol/frame/mod.rs:181-190`) | axum default, 16 MiB |
| `max_message_size` | `check_max_size(payload.len(), …)` **after** the frame payload is buffered (`protocol/mod.rs:699`) | 4096 |

So a hostile authenticated peer that declares a 15 MiB text frame still makes the
server reserve and read 15 MiB before the 4096-byte check rejects it. The socket
limit is 64 connections, so the worst case is ~1 GiB of transient buffer on a
1 GB box shared with RouterOS — the same hard-rule-4 concern F5 was raised for,
left open by the fix. This is *evidence from the dependency's source*, not
inference from behaviour; the existing test
(`an_oversized_frame_is_refused_and_releases_the_subscription`, 8 KiB) passes
either way because 8 KiB is under both limits.

*Impact:* authenticated-only, so hardening rather than an exploit — but the
recorded mitigation does not do what the review file says it does. *Concrete
fix:* add `.max_frame_size(events::MAX_CLIENT_MESSAGE_BYTES)` beside the existing
call, and extend the oversized-frame test with a declared length above 4096 to
cover the frame path. *Consequence for API.md:* the sentence "**Frames are capped
at 4096 bytes**" is currently false — 4096 is the *message* cap. Adding
`max_frame_size` makes the documented wording true, which is the cheaper
reconciliation than editing the document.

**17. The cleanup round's claim is wrong for `PUT /clients/{ip}`.** —
**Status: OPEN**

§Cleanup round F3+F4 states the per-assignment `String` clone is gone "from
`GET`/`POST`/`DELETE` `/clients/{ip}/policy` **and from `PUT /clients/{ip}`**".
It is not: `set_client_name` still calls
`client_response(entry, &PolicyResolver::build(&state))`
([routes.rs:393](../../../crates/fah-api/src/routes.rs#L393)), and
`PolicyResolver::build` clones **every** configured assignment's `client` into a
`HashSet` in order to answer one address. The single-address path
(`direct_assignment`, a short-circuiting `find` with no allocation) already
exists and is what `client_policy_response` uses.

*Impact:* bounded by the assignment count on a cold route — the cost is small and
not blocking. The defect is the record: a review file that overstates what was
fixed is the drift the fix rounds exist to prevent. *Fix:* either resolve the
single row through `policy_in_force` + `direct_assignment` on that path, or
correct the sentence.

### Nitpick

**18. `AssignmentResponse` carries a `Clone` derive with no remaining user.** —
**Status: OPEN** Added for the `HashMap<String, AssignmentResponse>` that the
cleanup round removed ([wire.rs:774](../../../crates/fah-api/src/wire.rs#L774)).
No `.clone()` on that type survives anywhere in `crates/fah-api`. It widens a
`pub` wire type's trait surface for nothing (principle 14).

### Verdict of this pass

No correctness, concurrency, lifetime or cancellation defect was found in the
implementation. Findings 16 and 17 are both about a stated mitigation being
narrower than its record; 18 is dead surface. Nothing here blocks the task on its
own — 16 is the one worth fixing in this commit, because it is two words and it
makes an API.md sentence true.

## Status — after the second review pass

**PASS WITH DEFERRED FINDINGS.** Fixed and verified: 1, 2, 3, 4, 5 (partially —
see 16), 6, 8, 9, 13, 15. Open: **7**, **11**, **12** (recorded, no behaviour
change), **10**, **16**, **17**, **18**. **14** stays operational: keep the P2.6
soak edit out of this commit.

---

## Fix round — findings 16, 17, 18 — 2026-08-26

Approved and applied: **16**, **17**, **18**. Findings **7**, **10**, **11** and
**12** stay deferred exactly as recorded. **14** unchanged and untouched: the
P2.6 soak file is still modified in the working tree and stays out of this
commit. No API semantics changed; API.md needed no edit.

### F16 — the frame cap now matches the documented contract

`events_socket` sets **both** limits:

```rust
.max_message_size(events::MAX_CLIENT_MESSAGE_BYTES)
.max_frame_size(events::MAX_CLIENT_MESSAGE_BYTES)
```

`max_frame_size` is the one enforced from the frame header, before
`in_buffer.reserve(len)`. The 16 MiB reserve a hostile authenticated peer could
force per socket is gone; the ceiling is now 4096 bytes on both limits, so
API.md's "Frames are capped at 4096 bytes" became true without a documentation
edit. The 4096-byte contract itself is unchanged.

**Tests — and why there are two.** The existing
`an_oversized_frame_is_refused_and_releases_the_subscription` was extended to
read the socket to its end, so it proves the server actually rejects and closes
rather than only that the guard released. It does **not** discriminate on this
fix: an 8 KiB frame trips the message cap too, and the test passes with
`max_frame_size` removed — verified by mutation. It is a contract test, not the
proof of this finding.

The proof is the new
`a_frame_header_declaring_a_huge_payload_is_refused_before_the_payload`. It
writes a raw masked text-frame header declaring **8 MiB** directly into the
TLS stream under tokio-tungstenite (`WebSocketStream::get_mut`) and sends no
payload at all. That is the exact case the two limits treat differently:

| Build | Behaviour |
| ----- | --------- |
| With `max_frame_size` | refused from the header; the socket closes at once |
| Without it (mutation) | the header is accepted, the payload is awaited, only the 2 s stats pushes arrive — the test fails on its 5 s deadline (`test result: FAILED`, 5.04 s) |

Mutation-checked in both directions. No sleep on the passing path; the test
completes in 0.03 s.

### F17 — `PUT /clients/{ip}` no longer builds a resolver for one address

`client_response` now takes the resolved `(policy, assignment_source)` instead of
a `&PolicyResolver`, so the two callers pick the path that fits:

| Route | Path |
| ----- | ---- |
| `GET /clients` | one `PolicyResolver` per request, read per row — unchanged |
| `PUT /clients/{ip}` | `policy_in_force` + `direct_assignment` — the same single-address path `GET`/`POST`/`DELETE` `/clients/{ip}/policy` uses |

The per-assignment `String` clone into a `HashSet` is gone from `PUT`, which is
what the cleanup round claimed and did not deliver. Semantics are byte-identical:
both paths compare the configured `client` against `assignment_key(ip)`, so the
accepted string-comparison behaviour (§4.6, plan §B2) is preserved, and
`HashSet::contains` versus the short-circuiting `find` answer identically for a
presence test. `clients_list_and_naming_round_trip` (which asserts `policy` and
the absence of `assignment_source` on the `PUT` response) and the four-case
agreement test both pass unchanged.

### F18 — the dead derive removed

`AssignmentResponse` loses `Clone` ([wire.rs:774](../../../crates/fah-api/src/wire.rs#L774)).
It had no user left after the cleanup round replaced
`HashMap<String, AssignmentResponse>` with `HashSet<String>`; clippy and the
suite confirm nothing cloned it.

### Verification

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | green |
| `cargo clippy --workspace --all-targets --message-format=short -- -D warnings` | green |
| `cargo test --all-features --workspace` | green — 44 test binaries, 0 failures |
| `fah-api` | 88 unit, **71** integration (+1), 2 coverage |

Working tree touched by this round: `crates/fah-api/src/routes.rs`,
`crates/fah-api/src/wire.rs`, `crates/fah-api/tests/api.rs`. Nothing else.

## Status — after the fix round

**PASS WITH DEFERRED FINDINGS.** Fixed and verified: 1, 2, 3, 4, 5, 6, 8, 9, 13,
15, 16, 17, 18. Deferred, none changing behaviour: **7** (the RB5009 figure is an
upper bound, not an estimate), **10** (`pub` items with no reach outside a
private module), **11** (`direct` does not imply the policy came from that
assignment), **12** (two `String` allocations per row at the 4096-client bound).
**14** is operational: keep the P2.6 soak edit out of this commit.
