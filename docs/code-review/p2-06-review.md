# p2-06 — Per-client enforcement and the policy API

**2026-08-02.** Both pipelines resolve the querying client's policy; policies
and assignments are manageable over the API; stats and the query log gain a
policy dimension.

Gates: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`,
`cargo test --workspace` — all green.

The task's acceptance criteria are met: two clients on two policies get
different verdicts for one name (DNS and HTTP), a schedule boundary flips a
verdict with no restart, an API round trip changes a verdict live, and the
per-query cost is measured rather than asserted.

## Decisions

- **`fah-rules` stays pure; the binary owns the clock.** `PolicySet::active_at`
  is a pure function and `PolicyState` is an atomic swap holder. The tick, the
  wall clock and the client-name source live in `fastadhunter`. An earlier draft
  put a `ClientDirectory` trait object, a `OnceLock` and a spawned task on
  `ListManager` and was removed — see [Rejected](#rejected).
- **Schedules are evaluated off the hot path.** `ActivePolicies` is the client →
  policy map with every window already evaluated and every name-selector already
  resolved to addresses, rebuilt on a 20 s tick. A query walks address selectors
  only: no clock, no timezone, no name lookup, no allocation.
- **Only mask-affecting edits recompile.** The policy set and a policy's `lists`
  decide the per-rule masks, so those recompile (seconds of ARM CPU).
  Assignments, schedules and labels apply in milliseconds.
- **`Matcher::context_for` is the single place a client is identified**, called
  by both pipelines, so DNS and HTTP cannot judge one device differently.
- **`blocking_mode` is left inert, and the docs now say so.** p2-05 recorded "no
  consumer until p2-06"; that was not implementable as written. `BlockingMode`
  has one variant, `null_ip`, and the *global* `[dns.blocking] mode` has no
  consumer either — `response::blocked` synthesizes null-IP unconditionally.
  Wiring a per-policy override of a setting with one legal value would have been
  plumbing no test could distinguish. It becomes real when a second mode lands.

### Rejected

One matcher-shaped parameter was removed after review. `PolicyState::refresh`
took `&Matcher` to read one bit (`has_named_client_scopes`) deciding whether the
snapshot carried the address→name map. `Matcher::context_for` already gates that
lookup on the hot path, so the flag saved only a small bounded map while
coupling `PolicyState` to the matcher and stating one rule in two places.

## Bugs found

### 1. A policy enabling no compiled list saw *everything* (p2-05, HIGH)

The mask array is dropped when filtering would change nothing, and an empty
array tells the lookup to skip the check. The condition compared every record's
mask against the union of the **list** masks. A policy that enables none of the
compiled lists contributes no bit to that union — so the condition held, the
array was dropped, and the policy that should have seen *nothing* saw
everything. Over-blocking, silently, for exactly the policy an operator writes
to be permissive.

Reachable with one list and two policies, which is the smallest realistic setup.
p2-06 is the first task to enforce a policy, which is why it surfaced here.

Fix: compare against `PolicySet::universe()` — every policy bit that exists —
threaded in via `MatcherBuilder::set_policy_universe`. Pinned by
`a_policy_enabling_no_compiled_list_blocks_nothing`.

### 2. HTTP peer addresses were not canonicalized (p2-02, MEDIUM)

`fah_http::Proxy` used `peer.ip()` raw. The listener is dual-stack, so an IPv4
client arrives as `::ffff:192.168.1.50` — which matches no `192.168.1.50`
assignment, and logs the device as a second client distinct from its DNS
identity. `fah_dns::Pipeline::handle` had canonicalized since p1; the proxy
never did.

Fix: `to_canonical()` once in `serve_connection`. Pinned by
`a_v4_mapped_peer_matches_its_v4_policy_assignment`.

## Measurements

Dev box, core-pinned, `--sample-size 200`, per PERFORMANCE.md §Measuring
reliably. Baseline arm built from a detached worktree at `8d65c2e` (p2-05).

`matcher_lookup` — identical bench in both trees:

| | baseline | p2-06 | Δ |
| --- | --- | --- | --- |
| hit_exact | 115.71 ns | 111.58 ns | −3.6% |
| hit_subdomain | 358.24 ns | 344.89 ns | −3.7% |
| miss | 65.12 ns | 65.18 ns | +0.1% |

`policy_resolution` — the four lines `Pipeline::handle` runs per query
(`pipeline.rs:272-275`), p2-06 only:

| arm | time | vs bare `hit_exact` |
| --- | --- | --- |
| zero_config | 120.04 ns | +8.5 ns |
| assignments_1 | 119.55 ns | +8.0 ns |
| assignments_15 | 130.73 ns | +19.2 ns |

`full_pipeline`, `--sample-size 100`:

| bench | baseline | p2-06 |
| --- | --- | --- |
| blocked_query | 3.366 µs [2.964, 3.768] | 2.540 µs [2.279, 2.790] |
| forwarded_query_overhead | 4.685 µs [4.095, 5.269] | 4.573 µs [4.015, 5.131] |

**A zero-config deployment pays +8.5 ns per query — not zero.** It is one
`ArcSwap` refcount bump plus an empty-slice walk, 0.19–0.34% of one pipeline
query. The `full_pipeline` intervals are ±12–15%, so they establish only that no
regression above ~15% exists; the nominal improvement there is not claimed as
one.

Memory: `ActivePolicies` is bounded by the configured assignment count plus the
named-client count (registry cap 4096), rebuilt-and-replaced never accumulated,
and republished only when it changed. Per-policy stat counters are capped at
`PolicyId::MAX`. The policy id on an event is an `Arc` refcount bump.

## Files changed

| file | change |
| --- | --- |
| `fah-rules/src/policy.rs` | `ActivePolicies`, `PolicyState`, `PolicySet::{active_at,universe}` |
| `fah-rules/src/matcher.rs` | `context_for`, `has_named_client_scopes`, `set_policy_universe`, mask-drop fix |
| `fah-rules/src/url_matcher.rs` | tracks name-scoped `$client` rules |
| `fah-rules/src/lifecycle/mod.rs` | `recompile()`; ticker/clock removed from here |
| `fah-model/src/{query_event,request_event}.rs` | `policy` field + `under_policy` |
| `fah-dns/src/pipeline.rs` | `with_policies`, per-query resolution, `lookup_in` |
| `fah-http/src/proxy.rs` | `with_policies`, `lookup_http_in`, peer canonicalization |
| `fah-stats/src/{aggregates,stats,client_registry,dto}.rs` | per-policy counters, `named_clients` |
| `fah-stats/src/query_log/mod.rs` | `policy` filter |
| `fah-api/src/{routes,wire,state,ports,config_store}.rs` | policy CRUD, assignments, `rules/test` context |
| `fastadhunter/src/main.rs` | policy state, 20 s ticker |
| `API.md`, `CONFIGURATION.md`, `CONTEXT.md`, `PERFORMANCE.md` | §Policies, boot→runtime, Active Policies, the figures above |

New tests: `fah-dns/tests/policy_enforcement.rs` (5), `fah-http/tests/filtering.rs`
(2), `fah-api/tests/api.rs` (5), `fah-rules/tests/policy_sharing.rs` (1),
`fah-stats/src/stats.rs` (2).

## Remaining TODOs

- **Not measured on the RB5009.** The +8.5 ns is a dev-box figure; the ~9×
  factor puts it near ~75 ns on-device, still far under any budget. The phase
  soak (`p2-08`) should carry a real policy set.
- **`blocking_mode` stays inert** until a second `BlockingMode` variant exists.
- **`safe_search` not shipped**, per p2-05 — a key that does nothing is the
  p1.5-07 defect.
- The 20 s tick is a compiled-in constant, not a config key.
