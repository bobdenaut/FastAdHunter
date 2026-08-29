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
`WS /events` every ~2 s replaces polling **of `/stats`**. `GET /stats` is still
called once on connect because the first push is up to ~2 s away.

**The push covers half this page, not all of it.** The `stats` message is
byte-for-byte the `/stats` payload: tile row 1, the four top-N tables, the
buckets. The ruleset card, the upstream bars, the HTTP tiles, uptime and cache
state come from `/telemetry`, `/health` and `/cache`, and **nothing pushes
those**. They arrive through the shared bounded refresh — one slow interval, one
in-flight request per endpoint shared by every widget reading it, paused while
the page is hidden. No widget owns a timer.

The socket on this page subscribes to `stats`, `config_changed` and
`list_refreshed`. **Not `query`** — the Dashboard renders no per-query rows, so
receiving the feed would cost the phone and the engine for nothing.

**Tile row 1 — DNS, rolling 24 h.** Total Queries · Blocked · Blocked % · Cache
Hit %.

Pi-hole's fourth tile is "Domains on Lists". FAH's equivalent figure is
`compiled_rules`, which is a ruleset property rather than a traffic figure, so
it moves to the Ruleset card and Cache Hit % takes the tile.

**Tile row 2 — HTTP and engine, since restart.** HTTP Requests · HTTP Blocked ·
Compiled Rules · Uptime.

The second row exists because FAH runs two pipelines. `counters.dns` and
`counters.http` are separate by contract and the UI never sums them into one
"queries" number.

**The row carries a "since restart" label and it is load-bearing.** Row 1 is a
rolling 24 h window; `counters.http` is process-lifetime cumulative and returns
to zero on restart. Two identical-looking rows over different windows is the
trap. The API has no 24 h HTTP figure and the UI does not invent one.

**Full width — Queries over time.** Stacked permitted/blocked area from
`GET /history/summary`. Range selector: 24 h · 7 d · 30 d, mapping to
`resolution=hour` then `day`. When `stride > 1` the chart footnote says the
series is decimated and that every plotted point is a real reading.

Two things the chart states about itself:

- **DNS only.** `history/summary` carries no HTTP series. On a dashboard that
  keeps the pipelines apart everywhere else, silence here would read as a total.
- **`permitted`, not "allowed".** The endpoint gives `queries`, `blocked`,
  `cache_hits` and `per_type`; the lower band is `queries − blocked`. `allow` is
  a different, much smaller thing — the explicit exception verdict — and the two
  words are never swapped (capability-matrix.md §Vocabulary).

**When `history.enabled` is `false`, this chart says so.** The flag is
runtime-mutable, so Settings can turn the recorder off live, after which
`/history/*` answers `200` with empty `items` for ever. "No data in this range"
and "nothing is being recorded" are different facts and the UI reads the flag
from `GET /config` to tell them apart.

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

`parse_errors` sits beside the partition: it counts the unparseable lines of the
copy **currently serving**, never of a refused body, and reads `0` both for a
clean list and for one contributing nothing — so it is presented next to
`enabled` and `rules_total`, which is what tells those two apart.

Failure surface: `last_status` is `ok` \| `failed` \| `rejected` \| `degraded`
\| `never`, and `last_error` carries the reason.

- **`degraded` is not a gentler `ok`.** The fetch worked and most of the body
  failed to parse — a format misdetection, where the list contributes far fewer
  rules than it should. It gets its own presentation and points at
  RULE_ENGINE.md §Supported formats, because it used to be indistinguishable
  from success.
- **`rejected` carries its own way out.** The content gate refused a body and the
  last good copy still serves — not an outage. But a source that legitimately
  restructured stays rejected on every attempt, across restarts, and disabling
  and re-enabling does **not** clear it: the cached copy is the baseline. The
  API's recovery contract is `DELETE` then re-add, and that is the action the row
  offers. This is the page an operator opens when a list is broken; the way out
  belongs on it.

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
with a blocked-share bar — **plus the policy in force and its assignment
source**, added to that response in `p5-03` for this page.

That addition is why the table costs one request. The policy was otherwise only
on `GET /clients/{ip}/policy`, one call per row: a household with forty observed
clients would have paid forty extra requests every time the page opened, to fill
a column the design calls for on every row.

Clients here are **observed by traffic**. There is no ARP table, no DHCP lease
list, and no notion of a client that has never sent a query.

Inline actions: rename (`PUT /clients/{ip}`, `{ "name": null }` clears it) and
policy assignment (`PUT|DELETE /clients/{ip}/policy`, with optional
`days`/`start`/`end`).

The assignment column distinguishes an assignment naming this address from one
inherited via subnet or name — the response gives the policy in force now, and
the assignment source says which case it is. `GET /clients/{ip}/policy` stays the
single-address read and the write path, so the two cannot report different
answers for one address. Assignment changes are live in milliseconds and are
labelled so, in contrast to the policy-edit warning.

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

Background-cleanup panel: runs, entries removed, bytes freed — and
`last_duration_micros`, which is the **only last-value gauge** in a block of
cumulative counters. It describes the most recent sweep, so it is rendered as a
current value and never as a series.

`/cache` and the telemetry counters arrive through the shared bounded refresh,
not a timer belonging to this page.

## Performance

`GET /history/perf`, using `fields` to fetch only the series a chart draws.

This whole page is persisted history, so **`history.enabled = false` is its own
state**, not an empty chart. With the recorder off every series is empty for
ever, which would read as "nothing happened".

Charts: QPS · verdict deltas — `queries_delta`, `blocked_delta` and
`allowed_delta`, which here really is the **`allow` verdict** the engine counted,
not the derived `permitted` band the Dashboard chart draws · latency percentiles
p50 and p99 per class, block / cache_hit / forward · RSS and peak RSS · cache
entries and hit ratio.

Range selector shares the Dashboard's. At the default 60 s sample interval one
day is 1440 samples, so `stride` applies here most often and is surfaced the
same way.

An unknown `fields` name is a `400` by design; the UI only ever sends names
from the documented set.

## Upstreams

`telemetry.upstreams`, `/health`, and `dns.upstreams.strategy` from
`GET /config`.

**The strategy is not optional context, it is what makes the rest readable.**
`telemetry.upstreams[]` does not carry it. Under `fallback` every row publishes
`state: "healthy"`, `penalty_round: 0` and zeros for penalties, probes and
penalized seconds — which API.md is explicit means *no health state exists to
report*, not *everything is fine*. Rendering those zeros without naming the
strategy states the opposite of the truth, so the page reads the strategy from
config and says which one is in force.

Per endpoint: address, protocol, attempts, failures, `consecutive_failures`,
TLS handshakes, and — under the adaptive strategy — whether it is healthy,
penalized or being probed. `family` is `null` for a DoH URL whose host is a
domain name resolved at connect time; that renders as unknown, never as the word
"null".

`/health` reporting `degraded` renders as a banner that explains it rather than
an alarm: under `adaptive` it means no endpoint is currently healthy, under
`fallback` that every endpoint carries a non-zero `consecutive_failures`.
Neither is down — cache hits and serve-stale keep answering, and a penalized
endpoint is still queried when nothing else is left.

## Settings

**Decided: the form is hand-written per config section, not generated from the
API response.** The frontend owns the field grouping, the descriptions, the
mutability labelling, and which keys are exposed at all. A generic renderer
would give up exactly the control this page exists to provide — it cannot write
a field's help text, cannot decide that `rules.lists` belongs elsewhere, and
cannot tell a boot-only key from a live one without being told.

**`GET /config` is the source of current effective values and of nothing else.**
It is not a schema endpoint: it returns the merged configuration with secrets
redacted, and carries no types, no bounds, no enums and no mutability classes.
Every one of those is hand-carried from [CONFIGURATION.md](../../CONFIGURATION.md)
and the backend schema into the frontend. That is the cost of the decision above,
and it is paid deliberately — but nothing on this page may be built as though the
API described its own constraints.

The form is grouped by config section as CONFIGURATION.md organises them.

**`[api]` is not an ordinary section.** `api.tls = false` removes the only origin
on which a `Secure` `__Host-` cookie can exist, so the next restart leaves the
dashboard unable to authenticate at all — and there is no HTTP fallback by
design. `api.address` and `api.port` move the listener out from under whoever is
using it. Either the section stays out of the curated form, or each field is
gated behind an explicit confirmation naming the lock-out. A TLS toggle rendered
like any other boolean is the failure mode.

Every field carries its mutability class: **live** or **restart required**.
Most options are boot-only — `[dns.cache]`, `[dns.upstreams]`,
`[dns.blocking]`, `[stats]`, `log.level` and `history.sample_interval_seconds`.
The runtime set is `history.enabled`, `history.retention_days`,
`rules.refresh_hours_default` and `schedule.timezone`.

`POST /config` returns `applied` and `restart_required`; a persistent banner
holds until a restart is observed via `/health` uptime resetting. The
`config_changed` event refreshes the form.

`rules.lists`, `policies` and `auth.*` are absent from the form. All three are
`422` on this endpoint on purpose — each has exactly one writer. For the first
two that writer is another page; for `auth.*` it is the password-change route,
which verifies the current password and invalidates every session. A deep-merge
patch that could set a password hash would bypass both.

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

**It renders whatever `GET /config` returns, so what that endpoint returns is a
security boundary for this panel.** `auth.*` is redacted at the endpoint from
`p5-04` — a password hash is offline-crackable material and "it is only a hash"
is not a reason to print it into a browser. The panel checks as well as trusts.

One wording note: `policies` is omitted from the response when no policy is
configured, so the zero-config case shows no key at all. Word that as "none
configured" — an absent key reading as ambiguity is precisely what this panel
exists to prevent.

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

Separate Access panel: change password (current password required — an explicit
reauthentication barrier for a privileged operation, and the last one standing
for an unattended logged-in browser), sign out everywhere, and rotate API key
(`POST /config/apikey/rotate`). The new key is returned once; the dialog says so
before the user confirms, and the old key stops working immediately.

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

  **This is the only screen that subscribes to `query`.** It adds the
  subscription on mount and drops it on unmount. Left open, the household's whole
  per-query feed would keep arriving at a phone showing Settings, and the engine
  would keep doing per-query publish work for a page nobody is looking at.
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

**The event socket drives what it can, and only that.** One
`WS /api/v1/events` connection per session. `stats` refreshes the Dashboard,
`config_changed` refreshes Settings, `list_refreshed` re-reads `GET /lists` — the
event is a nudge and carries no reason, so the reason comes from `last_error`.

**The socket subscribes; it does not simply listen.** There is no shell
baseline: each route declares the event types it renders, and the union of the
mounted route's declarations is the subscription. It is re-sent after every
reconnect, and when the union is empty the connection is closed rather than
idled. Server-side filtering is what makes this worth doing: it removes the
bandwidth *and* the lag — a stats-only socket sends one message every two
seconds and cannot fall behind on query volume the way an unfiltered one does.

Reconnect with backoff; a disconnect banner appears because slow consumers are
dropped by design. **A rejected upgrade is not a reconnect loop.** A browser
`WebSocket` exposes no status for a failed handshake, so a `401` is
indistinguishable from a dropped network. After repeated immediate failures the
client makes one authenticated REST probe and acts on what it learns: expired
session → login, transport failure → keep backing off, server unreachable → say
so. Only a real authentication failure returns the user to login.

**Everything the socket does not push has one refresh mechanism.**
`/telemetry`, `/cache` and `/health` are read on a slow shared interval, one
in-flight request per endpoint however many widgets want it. No page starts a
timer of its own, and nothing polls what the socket already pushes.

**Route-scoped data fetching.** The invariant the whole frontend is measured
against: **an inactive page has approximately zero API activity attributable to
it.**

- A page fetches and polls only what the active route renders. Unmounting clears
  its timers and releases its event types.
- Re-entering may show cached data at once, then revalidates on that page's own
  policy. The page cache is bounded — thirteen screens visited is not thirteen
  payloads retained.
- **Shared state yes, shared *schedule* no.** The refresh mechanism above polls an
  endpoint only while a mounted page subscribes to it; the last unsubscribe stops
  that timer. Shared and global are easy to confuse, and only the first is
  allowed.
- **The socket is shared but not persistent.** `stats` belongs to the Dashboard,
  `list_refreshed` to Lists, `config_changed` to Settings, `query` to the Live
  Feed. The other nine screens need none, and on them the connection is **closed**
  rather than held idle.
- **Hidden document:** polling and rendering stop at once; the socket closes after
  a short grace period, so an app switch or a screen lock does not cost a TLS
  handshake. Becoming visible cancels a pending close, or reconnects and restores
  the active route's subscriptions.
- **A socket is never idled by subscribing to nothing.** The 2 s stats cadence is
  what lets the server notice a peer that vanished without closing; a silent
  socket holds a connection slot until TCP gives up. Close it instead.

**The connection indicator has three states**, because a closed socket is correct
on most of the UI: **live** · **not needed here** · **reconnecting**. Only the
last reports a problem, and `aria-live` covers all three. A two-state indicator
would cry fault on nine screens that never wanted a connection.

**The restart-required banner is global state without a global poll.** It
survives navigation and revalidates on entering Settings, or opportunistically
when a mounted page's shared refresh reads `/health`. It will not clear live from
an unrelated page — a banner does not earn a standing timer.

**Errors are shown as the API states them.** The error envelope's `message` is
displayed verbatim; `code` selects the presentation — `422` anchors to fields
or lines, `409` names the conflicting resource, `401` returns to login.

**Empty is not an error — and "disabled" is not empty.** A history window with no
data is a `200` with empty `items` and renders as "no data in this range", never
as a failure. `history.enabled = false` produces the identical response for ever,
so the UI reads the flag and renders that case as its own state. Two different
facts must not share one rendering.

**Approximation is labelled.** `/history/top` merges daily top-N files, so a
domain that missed a day's cut-off contributes nothing for that day. The table
says it answers "what dominated this range", not an exact order.

**Units and meanings never drift.** New API fields may appear; existing ones do
not change meaning. The UI reads documented fields only and ignores unknown
ones.

**Windows are labelled wherever two of them meet.** `/stats` is rolling 24 h;
`/telemetry` counters are cumulative since process start and return to zero on
restart; `/history/*` is the persisted series. A figure from one placed beside a
figure from another says which it is — the Dashboard's two tile rows are the case
this rule was written for.

**One derived figure exists, and it is named.** `permitted` is
`queries − blocked`. Nothing else on any page is computed from data the API did
not measure, and `permitted` is never called "allowed".
