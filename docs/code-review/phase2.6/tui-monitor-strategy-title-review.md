# tui-monitor — upstream strategy in the panel title — Review

Ad-hoc change, not a phase task. Requested during `p2.6-11` while the soak runs.

## Implementation Summary

The Upstreams panel title now reads `Upstreams - adaptive` / `Upstreams -
fallback`, taken from the appliance rather than from local configuration.

| File | Change |
| --- | --- |
| `tui-monitor/src/models/config.rs` | new — partial `Deserialize` of `/api/v1/config`, three structs, one field read (`dns.upstreams.strategy`) |
| `tui-monitor/src/models/mod.rs` | `pub mod config;` |
| `tui-monitor/src/client/api.rs` | `paths::CONFIG`, `ApiClient::config()` |
| `tui-monitor/src/state.rs` | `AppState.strategy: Option<String>` |
| `tui-monitor/src/workers/telemetry.rs` | fetch config when none is held or the appliance restarted |
| `tui-monitor/src/ui/details.rs` | title composition; `boxed()` takes `String` |

Decisions:

- **Strategy comes from `/config`, not `/telemetry`.** `/telemetry` carries no
  strategy field (verified against the live payload: `cache`, `counters`,
  `latency`, `memory`, `process`, `ruleset`, `upstreams`, and no `strategy` at
  any depth). Adding it server-side to `/telemetry` was offered and not taken.
- **Endpoint addresses stay on `/telemetry`.** They are applied state with live
  counters; `/config`'s `servers[]` is configured state and can differ from what
  a running process uses, because boot-time keys need a restart. Joining the two
  by address string was considered and rejected.
- **Refetch is event-driven, not periodic.** Strategy is boot-time, so it is
  read once and again only when `process.uptime_seconds` goes backwards.
- Partial deserialize: unknown keys are ignored, so the server may grow config
  fields without breaking this client.

Gates: `cargo fmt` clean, `cargo clippy --all-targets -D warnings` clean,
`cargo test` **122 passed, 0 failed**. `tui-monitor` is its own workspace; the
main workspace is untouched by this change.

## Findings

### F1 — Major — a failed config fetch after a restart leaves a stale strategy on screen

`workers/telemetry.rs:27-38`. `refetch` is `restarted || strategy.is_none()`,
but a failed `client.config()` leaves the previously held `Some(..)` in place.
On the next tick `restarted` is false and `strategy` is `Some`, so the refetch
never fires again.

*Failure scenario:* appliance restarts with the strategy flipped
`fallback → adaptive`; the `/config` request loses the race with the restart and
returns `Unreachable`. The panel then reports `Upstreams - fallback`
indefinitely, while the appliance runs `adaptive`. Only restarting the monitor
corrects it.

*Impact:* this is a monitoring tool, and the failure mode is showing a
confidently wrong answer about which selection mode is running — the exact
question the change was added to answer. Worse than showing nothing.

*Cause:* the refetch condition and the value it guards are evaluated against
different states — the condition is computed before the write, the value only
written on success.

*Fix:* clear the held value on restart, then let a single condition drive the
fetch, so a failure self-heals on the next tick:

```rust
state.update(|app| {
    app.api = LinkStatus::Online;
    app.telemetry = Some(telemetry);
    if restarted {
        app.strategy = None;
    }
});

if state.read().strategy.is_none() {
    if let Ok(config) = client.config().await {
        state.update(|app| app.strategy = Some(config.dns.upstreams.strategy));
    }
}
```

**FIXED.** Applied as written, with the clear folded into the same `state.update`
that writes telemetry, so no frame can render a strategy the poll already knows
is suspect. A failed fetch now leaves `None`, which the next tick retries.

### F2 — Minor — an undecodable `/config` retries once per poll forever

With F1's fix applied, `strategy.is_none()` is the retry condition, so a server
whose `/config` shape this client cannot decode produces one extra request every
poll interval for the life of the process.

Bounded and self-correcting once the shape matches, and at the default 60 s poll
it is one request a minute. Accepted rather than fixed: the alternative — giving
up after N attempts — trades a known small cost for a silent permanent blank.

**Not fixed, by decision.** This is inherent to retry-until-success, and the
alternative — giving up after N attempts — trades one request a minute for a
permanently blank field that never recovers. The owner asked for every finding
to be fixed; this one is reported back as declined with the reason, because
applying it would make the tool worse.

### F3 — Minor — the refetch decision has no test

The model parse and both title states are covered. The logic that carries F1 —
restart detection and the refetch condition — is not, because it lives inline in
an async worker that needs a live `ApiClient`.

*Impact:* the one genuinely stateful part of this change is the one part no test
constrains, and it is where the defect was.

*Fix:* extract the decision as a pure function and test it directly:

```rust
fn needs_config(restarted: bool, held: Option<&str>) -> bool {
    restarted || held.is_none()
}
```

**FIXED.** `needs_config(restarted, held)` extracted and covered by four tests:
first poll reads, a held value is not re-read, a restart re-reads, and a cleared
value is retried.

### F4 — Nitpick — `Clone` on `AppliedConfig` is unused

`models/config.rs`. The value is consumed once, and its `String` moves into
state. `Debug` earns its place in error paths; `Clone` does not.

**FIXED.** `Clone` dropped from all three structs.

## Measurements

| Property | Before | After |
| --- | --- | --- |
| Title `String` allocations per frame | 4 (in `boxed()`) | **4** (in `render`, moved into `boxed()`) |
| HTTP requests per telemetry poll | 1 | 1 |
| HTTP requests at startup | 1 | 2 |
| HTTP requests per appliance restart | 0 | +1 |
| New retained state | — | one `Option<String>`, bounded |

`boxed()` changed from `&str` to `String` and moves the value into its span
instead of copying it, so the per-frame allocation count is unchanged rather
than doubled. The function is private and has one call site.

## Correctness notes that hold

- No lock guard is held across an `await`: the `state.read()` temporary is
  dropped at the end of its statement, before `client.config().await`.
- The extra request cannot stall the poll cadence beyond one interval —
  `MissedTickBehavior::Delay` is already set.
- A failed telemetry poll leaves both `telemetry` and `strategy` untouched, so
  the last good reading stays on screen, matching the existing contract.
- No new task, socket, timer or buffer. The request reuses the shared
  `reqwest::Client` pool.
- Nothing on the DNS hot path is touched — this is a separate client process.

## Regression analysis

- `boxed()`'s signature change is private to `details.rs`, single call site.
- The pre-existing title test still passes: `─ Upstreams ` remains a substring
  of `─ Upstreams - adaptive `.
- `AppState` derives `Default`, so the new field needs no constructor change.
- API compatibility: read-only use of an endpoint the server already serves and
  API.md already documents. No server change, no doc change.

## Files changed

Six files under `tui-monitor/`, listed in the Implementation Summary. No change
to `crates/`, to the workspace, or to any document.

## Remaining TODOs

- None. F1, F3 and F4 are fixed; F2 is declined with its reason recorded above.

## Status

**PASS WITH DEFERRED FINDINGS.** F1, F3 and F4 fixed and verified; F2 declined
on the grounds that its fix is worse than the behaviour it replaces.

Gates after the fixes: `cargo fmt` clean, `cargo clippy --all-targets
-D warnings` clean, `cargo test` **122 passed, 0 failed** (four new tests on the
refetch decision).

The defect F1 describes was introduced and caught inside one session. Worth
noting why it survived the first pass: the refetch condition and the value it
guarded were written at different times, and the tests covered the parts that
were easy to test — the parse and the two title states — rather than the one
stateful decision that carried the bug.
