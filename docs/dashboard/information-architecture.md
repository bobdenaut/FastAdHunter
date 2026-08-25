# Information Architecture

What the FastAdHunter dashboard contains, how it is arranged, and what each
screen reads from. Pi-hole supplies the *look and the interaction grammar*;
this file is where the structure follows FAH's own model instead.

Every screen below is backed by a route in
[capability-matrix.md](capability-matrix.md). No screen exists without one.

## Navigation

```text
Overview     Dashboard
Filtering    Lists · Custom Rules · Policies · Clients · Rule Tester
Runtime      Cache · Performance · Upstreams
System       Settings · Diagnostics
```

Fixed left sidebar with four labelled sections, exactly Pi-hole's chrome. The
grouping is FAH's: what gets filtered, what the engine is doing right now, and
what the operator configures.

No Disable-Blocking control in the sidebar — there is no endpoint for it.
No Donate item.

## Dashboard

The landing screen. One `GET /stats` on load, then the `stats` push over
`WS /events` every ~2 s replaces polling. `GET /stats` is still called once on
connect because the first push is up to ~2 s away.

**Tile row 1 — DNS.** Total Queries · Blocked · Blocked % · Cache Hit %.

Pi-hole's fourth tile is "Domains on Lists". FAH's equivalent figure is
`compiled_rules`, which is a ruleset property rather than a traffic figure, so
it moves to the Ruleset card and Cache Hit % takes the tile.

**Tile row 2 — HTTP and engine.** HTTP Requests · HTTP Blocked · Compiled
Rules · Uptime.

The second row exists because FAH runs two pipelines. `counters.dns` and
`counters.http` are separate by contract and the UI never sums them into one
"queries" number.

**Full width — Queries over time.** Stacked allowed/blocked area from
`GET /history/summary`. Range selector: 24 h · 7 d · 30 d, mapping to
`resolution=hour` then `day`. When `stride > 1` the chart footnote says the
series is decimated and that every plotted point is a real reading.

**Half / half — Query Types · Upstream Health.** Donut from
`history/summary.per_type`. Upstreams is a horizontal bar of attempts with
failures overlaid, not Pi-hole's share-of-queries pie: FAH does not carry
per-query upstream attribution.

**Half / half ×2 — Top Queried · Top Blocked · Top Clients · Cache State.**
Tables from `GET /stats` with the hit-frequency bar Pi-hole draws behind the
count. Cache State is a compact fresh/stale/expired bar linking to the Cache
page.

**Ruleset card.** `telemetry.ruleset`: compiled rules, duplicates removed, last
compile duration. Compile duration is seconds of CPU on the RB5009 and is shown
because a policy edit pays it.

## Lists

`GET /lists` inventory. Columns: id, source, enabled, refresh interval, last
refresh, status, and the rule partition — `rules_active_dns`,
`rules_active_url`, `rules_inactive` — which sum to `rules_total`.

The three-way partition is FAH-specific and is shown as a stacked bar per row.
A tooltip explains that inactive means no tier answers the rule yet, not that
the rule is broken.

Header shows `compiled_rules` and `duplicates_removed` for the whole ruleset.

Actions: add (`POST /lists`, URL or mounted path), enable/disable and interval
(`PATCH`), remove (`DELETE`), refresh one (`POST /lists/{id}/refresh`, `202`,
outcome arrives as a `list_refreshed` event), refresh all
(`POST /lists/refresh`, synchronous, blocking with per-list results).

Failure surface: `last_status` is `ok` \| `failed` \| `rejected` \| `degraded`
\| `never`, and `last_error` carries the reason. A `rejected` row explains that
the content gate refused a body and the last good copy still serves — this is
not an outage.

`409` on add is shown as what it is: either a derived id collision or the same
source already configured under another id, naming that list.

## Custom Rules

`GET /rules/user` renders as a line editor, not a table. `PUT` validates the
whole document and atomically swaps it.

On `422 validation_failed` the per-line messages anchor to their lines in the
editor. Nothing is written — the editor keeps the user's text and marks the bad
lines.

A read-only counter shows how many lines the current document holds. No
per-rule enable toggle and no per-rule delete button: the API has no per-rule
identity, and inventing one would promise a write that does not exist.

## Policies

`GET /policies`. One card per policy: name, the list subset (`lists: null`
renders as "every enabled list"), `blocking_mode` override, and its
assignments with days and window.

Header shows the schedule timezone and `active_assignments` — how many
assignments are in force *right now*, which reports a schedule boundary having
passed without waiting for a query.

Per-policy traffic comes from `stats.policies` and counts both pipelines.
Clients under no assignment are counted under `default`.

Editing: `POST` / `PATCH` / `DELETE`. The UI warns before any operation that
recompiles — creating a policy, changing `lists`, deleting a policy — because
that is seconds of CPU on the RB5009. Renaming and reassigning do not recompile
and carry no warning. The 16-policy ceiling is enforced in the form.

`default` is reserved and cannot be created.

## Clients

`GET /clients`: ip, name, first seen, last seen, 24 h queries, 24 h blocked,
with a blocked-share bar.

Clients here are **observed by traffic**. There is no ARP table, no DHCP lease
list, and no notion of a client that has never sent a query.

Inline actions: rename (`PUT /clients/{ip}`, `{ "name": null }` clears it) and
policy assignment (`PUT|DELETE /clients/{ip}/policy`, with optional
`days`/`start`/`end`).

The assignment column distinguishes an assignment naming this address from one
inherited via subnet or name — `GET` returns the policy in force now, and
`assignment` is absent in the inherited case. Assignment changes are live in
milliseconds and are labelled so, in contrast to the policy-edit warning.

## Rule Tester

`POST /rules/test`. Inputs: domain, qtype, and either a client (address or
name) or a policy to test under. Output: verdict, matching rule, source list,
deciding policy.

This is Pi-hole's "Search Lists" done against the real engine rather than a
text search, so it answers under a specific client's policy and can answer
hypotheticals — "what would kids see?".

## Cache

`GET /cache` plus `telemetry.counters.swr` and
`telemetry.counters.cache_cleanup`.

Entries by lifetime stage — fresh, stale, expired — as a stacked bar with the
stage meanings written next to it: fresh answers directly, stale answers only
after a failed forward, expired is dead weight awaiting eviction.

Both bounds are shown side by side, entries and bytes, with the higher of
`load_percent` / `byte_load_percent` marked as the one about to evict.

Lifetime counters: hits, misses, evictions.

`POST /cache/clean` is a button with the stale purge as an explicit, unchecked
toggle, worded as giving up serve-stale insurance. The result panel reports
removed, before/after and `freed_bytes`, and states that RSS does not fall by
`freed_bytes` because a clean never shrinks the table slab.

SWR panel: enqueued, deduplicated, dropped, completed, failed.

## Performance

`GET /history/perf`, using `fields` to fetch only the series a chart draws.

Charts: QPS · verdict deltas (queries/blocked/allowed) · latency percentiles
p50 and p99 per class, block / cache_hit / forward · RSS and peak RSS · cache
entries and hit ratio.

Range selector shares the Dashboard's. At the default 60 s sample interval one
day is 1440 samples, so `stride` applies here most often and is surfaced the
same way.

An unknown `fields` name is a `400` by design; the UI only ever sends names
from the documented set.

## Upstreams

`telemetry.upstreams` plus `/health`.

Per endpoint: address, protocol, attempts, failures, `consecutive_failures`,
TLS handshakes, and — under the adaptive strategy — whether it is healthy,
penalized or being probed.

`/health` reporting `degraded` renders as a banner that explains it rather than
an alarm: under `adaptive` it means no endpoint is currently healthy, under
`fallback` that every endpoint carries a non-zero `consecutive_failures`.
Neither is down — cache hits and serve-stale keep answering, and a penalized
endpoint is still queried when nothing else is left.

## Settings

**Decided: the form is hand-written per config section, not generated from the
API response.** `GET /config` is the source of current effective values and of
validation metadata; the frontend owns the field grouping, the descriptions, the
mutability labelling, and which keys are exposed at all. A generic renderer
would give up exactly the control this page exists to provide — it cannot write
a field's help text, cannot decide that `rules.lists` belongs elsewhere, and
cannot tell a boot-only key from a live one without being told.

`GET /config` renders the effective merged configuration with secrets redacted,
grouped by config section as
[CONFIGURATION.md](../../CONFIGURATION.md) organises them.

Every field carries its mutability class: **live** or **restart required**.
Most options are boot-only — `[dns.cache]`, `[dns.upstreams]`,
`[dns.blocking]`, `[stats]`, `log.level` and `history.sample_interval_seconds`.
The runtime set is `history.enabled`, `history.retention_days`,
`rules.refresh_hours_default` and `schedule.timezone`.

`POST /config` returns `applied` and `restart_required`; a persistent banner
holds until a restart is observed via `/health` uptime resetting. The
`config_changed` event refreshes the form.

`rules.lists` and `policies` are absent from the form. They are `422` on this
endpoint on purpose — they have exactly one writer each, on their own pages.

### All settings — a read-only panel at the bottom

The curated sections cover what the UI models. Everything else in the effective
config is rendered below them, read-only, in full.

This exists because a hand-written form is a **subset by construction**: a key
the API gains is invisible until someone adds a field for it, and silent
omission is the failure mode — an operator cannot tell "not exposed here" from
"not set". The raw panel removes that ambiguity without generating anything, and
its presence is what makes the curated-form decision safe.

Read-only is deliberate. Editing an arbitrary key needs the type, the bounds and
the mutability class the curated fields carry by hand; offering an edit box
without them would invite a `422` the UI could not explain.

### Writing config

**`POST /config` receives only the keys that changed.** The endpoint is a
partial deep-merge, and the form must never submit the document it read back.

Submitting the whole of `GET /config` would write every value the UI last saw,
including keys it does not model — so a key edited by hand in the TOML, or added
by a newer build, is silently overwritten with a stale value the moment anyone
saves an unrelated field. Sending a diff makes the blast radius exactly the
fields the operator touched.

Corollary: the form tracks its own dirty state per field. It cannot derive what
changed by comparing against a re-fetch, because `config_changed` may have
altered the server's copy in between.

Separate panel: rotate API key (`POST /config/apikey/rotate`). The new key is
returned once; the dialog says so before the user confirms, and the old key
stops working immediately.

## Diagnostics

**Decided: Diagnostics stays nested under System.** Health, Memory and Live Feed
are operational views — reached when something is being investigated, not part
of daily use — so they do not earn a fourth top-level section. The sidebar keeps
four sections, and System holds Settings and Diagnostics.

Everything that answers "is it healthy and where is the memory".

- **Health** — `/health` version, uptime, status, with the `degraded` wording
  above.
- **Live Feed** — `WS /events` `query` items. This is Pi-hole's query-log
  *presentation* over an ephemeral source. Columns: time, kind (dns/http),
  client, domain, verdict, rule, list, duration, and for HTTP items method,
  path, resource type, status, bytes. Filters are client-side over what the
  ring holds: verdict, kind, client, domain substring. The panel states that it
  starts empty on page load, holds a bounded number of rows, and retains
  nothing — there is no server-side query store to search.
- **Answer outcomes** — `counters.dns.answers`: `servfail_synthesized`,
  `servfail_relayed`, `refused_relayed`.
- **Shed** — `counters.events_dropped`, one number covering both pipelines
  because they share one bounded channel.
- **HTTP refusals** — `counters.http.refused`, the egress policy refusing a
  request before any upstream contact. Called out because it is the only signal
  of a LAN client probing.
- **Memory** — `GET /debug/memory`: per-structure heap plus `residual_bytes`,
  with the two `allocator_committed_*` fields marked as carrying no
  compatibility promise.

## Cross-cutting behaviour

**The event socket drives the UI.** One `WS /api/v1/events` connection per
session. `stats` refreshes the Dashboard, `config_changed` refreshes Settings,
`list_refreshed` re-reads `GET /lists` — the event is a nudge and carries no
reason, so the reason comes from `last_error`. Reconnect with backoff; a
disconnect banner appears because slow consumers are dropped by design.

**Errors are shown as the API states them.** The error envelope's `message` is
displayed verbatim; `code` selects the presentation — `422` anchors to fields
or lines, `409` names the conflicting resource, `401` returns to login.

**Empty is not an error.** A history window with no data is a `200` with empty
`items` and renders as "no data in this range", never as a failure.

**Approximation is labelled.** `/history/top` merges daily top-N files, so a
domain that missed a day's cut-off contributes nothing for that day. The table
says it answers "what dominated this range", not an exact order.

**Units and meanings never drift.** New API fields may appear; existing ones do
not change meaning. The UI reads documented fields only and ignores unknown
ones.
