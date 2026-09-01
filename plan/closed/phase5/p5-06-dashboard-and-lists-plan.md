# P5-06 — Dashboard and Lists · Development Plan

**Task:** [p5-06-dashboard-and-lists.md](p5-06-dashboard-and-lists.md) ·
**Depends on:** `p5-05` (shell, typed client, socket manager, shared refresh,
size gate) · **Branch:** `phase5-06` from the completed `phase5-05` ·
**Model:** Opus

---

## 1. Outcome

Two shipped routes — `/` and `/lists` — replacing their `not-yet-built` empty
states, both `built: true` in `src/router/routes.ts`, both correct at 390 px and
at desktop in both themes, both holding their event subscription and their
shared-refresh subscriptions only while mounted.

The Dashboard is the first page in the phase that draws a chart, so it also
lands the fourth build chunk (uPlot) and the bar-chart engineering the rest of
the phase reuses. Lists is the first page that mutates, so it lands the
confirm/mutate/error idiom `p5-07` inherits.

---

## 2. Sources and precedence

Precedence, highest first. This ordering is the instruction for this task and it
is what §3 resolves every conflict against.

1. **The artboards** — `docs/dashboard/sketch/Main.dc.html`,
   `Lists.dc.html`, `MobileDashboard.dc.html`, `MobileNav.dc.html`. They win on
   UI structure, placement, labels, ordering and responsive behaviour.
2. **The API** — `API.md`. The artboards do not win against a field that does
   not exist. Where an artboard draws a figure the API cannot supply, §4 stops
   for an owner decision rather than inventing one. Phase 5 standing constraint
   1 is not overridable by an artboard.
3. **The task file** — `p5-06-dashboard-and-lists.md`. Behaviour, data
   lifecycle, acceptance criteria.
4. **`docs/dashboard/information-architecture.md`** and
   **`visual-system.md`** — where the artboards are silent.

**Inherited from `p5-05`'s review** (status: PASS WITH DEFERRED FINDINGS), which
hands this task exactly two items:

| From p5-05 | Obligation here |
| ---------- | --------------- |
| **m8** — uPlot's stylesheet is a static import under `cssCodeSplit: false` | §7.6 — **measured and decided: (c)**, replace the vendor stylesheet with the rules the chart reaches. `cssCodeSplit` stays `false` per p5-05 |
| **m4** — `Chart` no longer rebuilds per render, but the proof needs a canvas | §7.7 — memoised options and a browser-measured construction count (V9a) |

Also inherited, and not this task's to fix: `p5-05` recorded that **§10 of its
own plan states the route transition backwards** (release-before-acquire) while
the code correctly does acquire-then-release. Nothing here depends on the prose;
noted so it is not re-derived as a defect.

One housekeeping item from the same review carries into this task's commit:
`docs/code-review/phase2.6/p2.6-11-optin-deploy-soak-review.md` is modified in
the working tree and belongs to the parallel 2.6 track. **Stage `p5-06`'s paths
explicitly; never `git add -A`.**

**Artboard figures are not measurements** (phase constraint 8). `512,883`,
`96 %`, `4h 31m` are drawings. Ratios read off an artboard are used only where
the artboard is the only statement of a rule; every other proportion is computed
from the response.

---

## 3. Conflicts, resolved

| # | Conflict | Resolution |
| - | -------- | ---------- |
| C1 | IA §Dashboard: "Stacked permitted/blocked **area**". Task, `visual-system.md` §Charts and `Main.dc.html` all say stacked **bars**. | **Bars.** Three of four sources agree and the artboard is drawn with rect bars. IA §Dashboard's word "area" is stale; §13 proposes the doc edit. |
| C2 | IA §Dashboard: socket subscribes to `stats`, `config_changed` **and** `list_refreshed`. Task: "**The socket subscribes to `stats` only**". `routes.ts` already declares `events: ['stats']`. | **`stats` only.** Task wins over IA (§2 precedence 3 > 4) and the shipped route table already agrees. The Dashboard renders nothing a `config_changed` or `list_refreshed` event would change. §13 proposes the IA edit. |
| C3 | `Lists.dc.html` draws a `DISABLED` pill in the Status column. `DISABLED` is not one of `last_status`'s five values (`ok` \| `degraded` \| `failed` \| `rejected` \| `never`). | **Both.** `enabled === false` renders the `DISABLED` pill the artboard draws, with the real `last_status` beneath it as secondary text when it is not `never`. A disabled list that last failed must not look clean. `StatusPill` gains `never` and `disabled`. |
| C4 | `Lists.dc.html` draws no per-row actions and no `parse_errors`; the task and IA require both. | **Artboard silent, not contradicted.** `parse_errors` appends to the existing mono partition line in the Rules cell — `198,500 dns · 9,181 url · 15,501 inactive · 0 parse errors` — which is literally "beside `enabled` and `rules_total`". Row actions take a new trailing cell after `Total`, inside the artboard's right padding. |
| C5 | `Lists.dc.html` draws only `OK`, `FAILED`, `REJECTED`, `DISABLED`. `degraded` and `never` are undrawn. | **Artboard silent.** `degraded` takes its own pill and its own body line pointing at RULE_ENGINE.md §Supported formats (task requirement); `never` takes a neutral pill and "not fetched yet". |
| C6 | `Main.dc.html` Upstream card bar widths (96 %, 34 %, 62 %) are not proportional to the attempts printed beside them, and no document states what they encode. | **Settled by owner decision D4, option A**: width is `attempts / max(attempts)`, overlay is `failures / attempts`. The artboard's widths are a drawing, not a rule (§2); its *ordering* is monotone in attempts and A preserves it. The card keeps the artboard's footnote verbatim — endpoint health, not share of traffic. |
| C7 | `Main.dc.html` Query-types card says "last 24 h"; the series it is drawn from (`history/summary.per_type`) is range-selected by the chart above it. | **The donut follows the range selector** and its secondary slot states the active range ("last 24 h" · "last 7 d" · "last 30 d"), so at the default range the artboard's text is what renders. One fetch feeds both cards, and `history.enabled = false` correctly disables both. |
| C8 | `MobileDashboard.dc.html` Top-clients rows print a red percentage; `Main.dc.html` prints a `Blocked` count column. `/stats.top_clients` carries `{ip, name, count}` and no blocked figure. | **Both are `GET /clients`.** `blocked_24h` and `queries_24h` are documented there and reproduce both artboards exactly (`3,020` and `10.0 %` are `blocked_24h` and `blocked_24h / queries_24h` of the drawn row). The card renders wholly from `/clients` so the pair comes from one snapshot; `/stats.top_clients` goes unused on this page. See D1. |
| C9 | `Main.dc.html` Ruleset card prints four figures, the fourth "13 enabled lists". `/telemetry.ruleset` carries three. | Needs `GET /lists`. **Owner decision D1.** |
| C10 | `Main.dc.html` Total-queries tile footer reads "6 active clients". No `/stats` field carries it. | `GET /clients` `items.length`, already fetched for C8. Folded into D1. |
| C11 | `Main.dc.html` Uptime tile footer reads "status ok". `/telemetry.process.uptime_seconds` gives the figure; `status` is only on `GET /health`. | Add `health` to the Dashboard's `endpoints`. It is the shared bounded refresh, so the cost is one request per slow interval shared with every reader, and it makes the drawn footer a documented field rather than decoration. `routes.ts` changes `['telemetry','cache']` to `['telemetry','cache','health']`. |
| C12 | `Main.dc.html` Upstream card secondary reads "strategy: adaptive". Not in `/telemetry`. | `GET /config` `dns.upstreams.strategy` (CONFIGURATION.md:97). The Dashboard already needs `/config` for `history.enabled`, so this is free. |
| C13 | `Main.dc.html` Cache card prints a `free` band plus `entry load` / `byte load`. | All documented: `free = capacity − entries` (a stated derivation of two fields, labelled `free`, not an invented figure), `load_percent`, `byte_load_percent`, `hits`, `evictions`. |
| C14 | The 24 h chart could be drawn from `/stats.buckets` instead of `/history/summary`, and would then survive `history.enabled = false`. | **Rejected.** One range would work and two would not, which is worse than one honest disabled state. Task and IA both name `/history/summary`; `/stats.buckets` is unused on this page. |
| C15 | `MobileDashboard.dc.html` prints no figures on the bars and says so; `Main.dc.html` prints them. | Not a conflict — one rule at two widths. The 50 px / 15 px floors produce both outcomes with no phone-specific code, which is exactly what `visual-system.md` §Charts claims. Implement the floors, not two layouts. |

---

## 4. Owner decisions — resolved 2026-08-27

Both decisions were put to the owner and answered. They are recorded here as
settled; §11's work units assume them.

### D1 — the Dashboard's five sources · **APPROVED, all five**

The artboards force more one-shot endpoints onto the landing page than the IA
anticipated. Every one is a documented field; none is polled; all are released
on unmount. The question is whether the landing page should open with this many
requests.

| Endpoint | Forced by | Artboard element |
| -------- | --------- | ---------------- |
| `GET /stats` | task, IA | tile row 1, top-domain tables, `Main` chart-title totals |
| `GET /history/summary` | task, IA | Queries over time, Query types |
| `GET /config` | task (C12) | `history.enabled` state, "strategy: adaptive" |
| `GET /clients` | C8, C10 | Top-clients `Blocked` column, mobile blocked %, "6 active clients" |
| `GET /lists` | C9 | Ruleset card 4th figure, mobile tile footer "13 lists" |

Plus the shared bounded refresh, over `telemetry`, `cache` and `health` as this
decision stood — **widened to five by D1a**, which moves `clients` and `lists`
out of the table above and onto the registry.

**Decision: take all five.** Each is bounded (a household has tens of clients and
fifteen lists), and dropping any of them means shipping a card the artboard does
not draw. All five are approved for Dashboard use.

**The original constraint that `/clients` and `/lists` be fetched once per mount
and never polled is explicitly superseded by D1a** — they are route-scoped
shared-refresh endpoints. What survives from this decision is the approval
itself: *the Dashboard may read all five*. `/stats`, `/config` and
`/history/summary` keep the shape this decision gave them (§5.1) — the route-
scoped invariant is about *sustained* traffic, and one-shots on entry do not
violate it.

### D1a — `/clients` and `/lists` are refresh endpoints · **superseded D1's constraint, 2026-08-27**

D1's original approval attached a constraint that these two stay one-shot. **That
constraint is withdrawn.** Mount snapshots go stale on a page an operator leaves
open, and a Top-clients table frozen at page-entry beside tiles that move every
two seconds is the kind of quiet wrongness that is worse than a slow figure.

`clients` and `lists` become **route-scoped shared-refresh endpoints**, through
the existing mechanism and no other:

- `REFRESH_ENDPOINTS` becomes
  `['health', 'telemetry', 'cache', 'clients', 'lists']`. That union is what
  `useRefresh`, `RefreshCluster`, `preferences.ts` and `registry.ts` are all
  keyed on, so widening it is the whole wiring change.
- `registry.ts`'s `DEFAULT_FETCHERS` gains `clients: getClients` and
  `lists: getLists`.
- `preferences.ts`'s `KEYS` gains `fah-refresh-clients` and `fah-refresh-lists`
  — browser-local, cross-tab, validated against the offered options exactly as
  the existing three are.
- `constants.ts` gains options and defaults for both.
- **No new polling mechanism.** No page starts a timer, no `setInterval` is
  written outside `lifecycle/timers.ts`, and the registry's existing rules carry
  over unchanged: it polls only while a mounted route subscribes, the last
  unsubscribe clears the timer, and `setSuspended` stops everything while the
  document is hidden.

**Intervals.** Both take `[60, 300]` with a default of `300`, matching
`telemetry` and `cache`. Neither inventory changes fast: a new client appears
when it first resolves, and a list changes on an operator action or on its own
`refresh_hours` (24 h by default). No new entry in `REFRESH_LABELS` is needed,
which keeps the selector's vocabulary at `30 s` / `1 m` / `5 m`. If a longer
option is wanted later it is one label and one array entry.

**The invariant is unchanged and is now enforced by the same code as the other
three:** leaving the Dashboard releases both subscriptions, and if no mounted
route declares them the registry clears their timers. Nothing polls `clients` or
`lists` when no mounted route declares them — that is `registry.subscribe`'s
existing refcount behaviour, not new logic.

**The pin test changes rather than disappears.** `REFRESH_ENDPOINTS` is asserted
to be exactly the five names above, so a sixth is still a deliberate edit.

### D2 — the Lists phone artboard · **APPROVED**

The task requires drawing Lists at 390 px into the sketch. A new
`docs/dashboard/sketch/MobileLists.dc.html` and an entry in
`docs/dashboard/sketch/canvas.json` are **approved as part of p5-06**. §9.2
specifies the artboard in full, so drawing it is a transcription rather than a
second design pass, and W14 is where it lands.

### D3 — artboard deviations · **one accepted, one reversed**

**X1 is reversed.** The Lists header keeps the artboard's **"7.41 s · last
compile"** figure. `/telemetry` is fetched **once on Lists mount** as a plain
one-shot — not through `refresh/registry.ts`, not declared in the route's
`endpoints`, no timer. Leaving Lists aborts it like every other in-flight
request, and the route's steady-state traffic stays zero.

That is the distinction the invariant actually draws: it forbids *polling* an
endpoint a page does not render, not *reading* one. A one-shot on mount is the
same shape as the Dashboard's five (D1), and it costs one request per navigation
to Lists.

**Consequence for the Lists route:** `telemetry` is **not** in the route's
`endpoints`. That list is `['lists']` (D1a) — the inventory polls through the
registry; `/telemetry` does not. `telemetry` is a `REFRESH_ENDPOINTS` member
because the Dashboard, Upstreams and Health poll it, so this page reads the same
endpoint through a second, timerless path. That is deliberate and is why W12
asserts it: one `/telemetry` request per Lists mount and no second one, ever.

**X2 stands.** One drawn element remains unimplemented, on API grounds — the
artboards win on structure, placement, labels and ordering (§2), but this one
cannot be satisfied without a poll that runs regardless of what is mounted.

| # | Sketch element | Not implemented | Reason | Where the figure lives instead |
| - | -------------- | --------------- | ------ | ------------------------------ |
| X2 | `MobileNav.dc.html`, drawer footer: **"connected · v0.2.20 · up 4h 31m"** — the `up 4h 31m` clause | the drawer footer renders **connection state · version** only | uptime is `/telemetry.process.uptime_seconds`. The drawer is shell chrome present on every route, so feeding it means a shell-level `/telemetry` read — a poll that runs regardless of what is mounted, which is exactly what the phase invariant's "global state is allowed; global polling is not" rules out. Unlike X1 there is no mount to hang a one-shot on: the shell mounts once and never unmounts | the Dashboard's **Uptime tile**, which both `Main.dc.html` and `MobileDashboard.dc.html` draw |

X2 is not silent in the product — the figure is on the landing screen the drawer
opens onto. It is restated at its point of implementation (§13) and goes into
the task's review file under known deviations.

**X3 — added by D1a, specified in §6.6.** The Dashboard gains two refresh
selectors the artboards do not draw, on Top clients and on the Ruleset card,
because D1a gave those two cards a polled endpoint and therefore an interval an
operator should be able to set. The artboards were drawn when both cards were
static. Reversible in one line each; flagged, not assumed.

### D4 — the Upstream Health bar encoding · **DECIDED: option A, 2026-08-27**

**The owner's decision, and the whole of it:**

```text
bar width  = attempts / max(attempts)      relative workload
overlay    = failures / attempts           failure rate within that bar
state      → the dot and its colour, independently
text       → "N attempts · M failures", verbatim, always visible
```

The bar carries **workload**. The overlay carries **failure rate**. `state` is
carried by the dot, not by the bar's length. Nothing else about an upstream is
computed: **no success rate, no health score, no share of traffic, no
availability percentage, no penalty-derived figure.** R18 and R19 in §8.2 are
the complete list, and a reviewer treats any other upstream arithmetic as a
defect.

Two mechanical rules the decision implies, settled here so they are not
rediscovered at the keyboard:

- **`attempts === 0` draws an empty track** — width 0, no overlay, no division.
  `max(attempts) === 0` (nothing has been asked of any endpoint yet) draws every
  row as an empty track.
- **A sub-pixel overlay is not drawn, and is never widened to be visible.** At
  12 failures in 201,883 the band is 0.006 % of the bar. It disappears, exactly
  as a bar label below the 50 px floor disappears (§7.2) — the figure survives in
  the printed `· 12 failures` text. Giving it a minimum width would draw a
  failure rate the endpoint does not have, which is the one thing this card must
  not do.

The record of how the decision was reached follows.

---

#### The question as it stood

`Main.dc.html` draws a bar per endpoint at 96 %, 34 % and 62 % beside
`201,883 attempts · 12 failures`, `8,204 · 311` and `61,402 · 4`. Those widths
are proportional to nothing in the data — not attempts, not failure rate, not
success rate. **No document fixes the encoding**, so unlike the top-N tables
there is nothing to derive it from:

- `visual-system.md` §Charts: "**Bars** — upstream attempts with failures
  overlaid". Names the two quantities, not the scale.
- IA §Dashboard: "horizontal bar of attempts with failures overlaid, not
  Pi-hole's share-of-queries pie". Same.
- The task: "upstream attempts and failures as bars, never a share-of-traffic
  pie". Same.
- `FrequencyBar` takes a caller-supplied `share` in 0..1 and clamps it; it fixes
  no normalization of its own. The top-N tables' max-normalization is settled by
  `Main.dc.html`'s own figures (`3,140 / 4,021 = 78 %`, as drawn) — the Upstream
  card has no such internal agreement to read off.

Every source agrees on what the bar must **not** be (share of traffic) and none
says what it **is**.

**One structural signal the artboard does give.** Its widths are monotone in
`attempts` — the busiest endpoint draws the longest bar (201,883 → 96 %),
the second busiest the middle one (61,402 → 62 %), the quietest the shortest
(8,204 → 34 %) — while being proportional to nothing. No function of the printed
figures reproduces 96/34/62 (linear, log and success-rate all miss), so the
widths are decorative. But the *ordering* rules out any encoding that shrinks as
volume grows, which is what B does: on these three rows B would draw
0.006 %, 3.8 % and 0.007 %, inverting the artboard's ranking.

Options:

| | Encoding | Reads as | Against it |
| - | -------- | -------- | ---------- |
| **A** | width = `attempts / max(attempts)`; failures overlaid as `failures / attempts` of that width | "how much work each endpoint did, and how much of it failed" | at 12 failures in 201,883 the overlay is 0.006 % of the bar — invisible, so the failure figure is carried only by the printed text |
| **B** | width = `failures / attempts`, i.e. failure rate, full-width scale | "how badly each endpoint is failing" | a healthy resolver draws a near-empty bar; three near-empty bars is a poor at-a-glance state, and it discards attempt volume |
| **C** | width = `attempts / max(attempts)`; **no overlay** — failure count stays text, `state` drives the bar colour (`healthy` / `penalized` / `probing`) | "who is carrying the traffic, and is it healthy" | drops "failures overlaid", which three documents say |

**Chosen: A.** It is the only option that renders both quantities the documents
name, its ranking matches the artboard's, the invisible-overlay objection is
honest rather than fatal (a healthy endpoint *should* show no failure band), and
`p5-08`'s Upstreams page inherits a settled encoding instead of inventing a
second one.

**W10 is unblocked. No open decisions remain in this plan.**

---

## 5. Data flow

### 5.1 Dashboard (`/`)

```text
mount
  ├─ subscribe events: ['stats']                     → route table, shell-driven
  ├─ subscribe refresh: telemetry, cache, health,    → route table, shell-driven
  │                     clients, lists                 (D1a — five endpoints,
  │                                                     one shared registry)
  ├─ GET /stats            once   → tiles r1, top domains
  ├─ GET /config           once   → history.enabled, upstreams.strategy
  │                                 (+ one re-read only on an empty history
  │                                  response — see below)
  └─ GET /history/summary  once + on every range change
socket
  └─ 'stats' frame ≈2 s    → replaces the GET /stats payload wholesale
unmount
  └─ every subscription released, every in-flight request aborted
```

Nothing on the page starts a timer. `/stats` is never polled — the push is the
refresh, and `GET /stats` exists only because the first push is up to ~2 s away.

**A range change refetches `/history/summary` and nothing else.** The donut
re-reads the same response. `/config`, `/clients` and `/lists` are **not**
re-read on a range change.

#### What moves while the page is open

One policy, stated once so there is no second reading:

| Data | Updates while mounted? | Source |
| ---- | ---------------------- | ------ |
| tile row 1, top queried, top blocked | **yes**, ~2 s | `stats` push |
| Upstream health, Cache state, Uptime, HTTP tiles, ruleset figures | **yes**, shared refresh | `telemetry`, `cache`, `health` |
| Top clients, "N active clients" | **yes**, shared refresh (D1a) | `clients` |
| "N enabled lists" | **yes**, shared refresh (D1a) | `lists` |
| Queries over time, Query types | **on range change only** | `/history/summary` |
| `history.enabled`, `upstreams.strategy` | **no — mount snapshot**, one bounded exception below | `/config` |

Five polled endpoints, one mechanism, five timers at most and only while this
route is mounted. Leaving the Dashboard releases all five; the last unsubscribe
clears each timer; a hidden document suspends them all.

`/config` is the only mount snapshot left, and `/history/summary` is not polled
at all — it is a range query, refetched when the operator changes the range.

#### `history.enabled` — read on mount, plus one disambiguation re-read

**The settled rule: `/config` is read once on mount. A range change never
re-reads it.**

One bounded exception, and it exists because an acceptance criterion needs it.
The Dashboard does not subscribe to `config_changed` (C2), so it has no live
signal when Settings switches `history.enabled` off in another tab. After that
switch `/history/*` answers `200` with empty `items` for ever — which a
mount-snapshot of `enabled: true` would render as "no data in this range",
the wrong one of the two states the task requires be distinguishable.

So: **when a `/history/summary` response has `items.length === 0` and the
mount snapshot said `history.enabled === true`, re-read `/config` once before
choosing between the two empty states.** That is a one-shot triggered by a
specific response, not a poll — the same shape as the Lists `/telemetry` re-read
(D3/X1) — and it fires only on a range that came back empty, which is rare and
is exactly the case that needs it. The result replaces the snapshot for the rest
of the mount.

`upstreams.strategy` gets no such treatment: it is boot-only
(CONFIGURATION.md), so a mount snapshot cannot go stale within a session.

| Range | Query | Bars |
| ----- | ----- | ---- |
| 24 h | `from=now−24h`, `resolution=hour` | 24 |
| 7 d | `from=now−7d`, `resolution=day` | 7–8 (UTC day boundaries) |
| 30 d | `from=now−30d`, `resolution=day` | 30–31 |

`to` is omitted (defaults to now). `max_points` is omitted; the default of 5000
makes `stride > 1` unreachable at these ranges, and the decimation footnote is
built anyway because the field is part of the contract.

### 5.2 Lists (`/lists`)

```text
mount
  ├─ subscribe events: ['list_refreshed']
  ├─ subscribe refresh: lists     → inventory, compiled_rules, duplicates_removed
  └─ GET /telemetry        once   → ruleset.compile_duration_seconds  (D3/X1)
list_refreshed event
  └─ coalesced re-read of GET /lists           ← see below
mutations
  ├─ POST   /lists                add             → re-read
  ├─ PATCH  /lists/{id}           enable/interval → re-read
  ├─ DELETE /lists/{id}           remove          → re-read
  ├─ POST   /lists/{id}/refresh   202, per-row "refreshing" until the event
  └─ POST   /lists/refresh        synchronous, blocking, per-list results
unmount
  └─ subscription released, in-flight aborted, `lists` timer cleared by the
     registry's last unsubscribe (no page-level timer exists)
```

**Coalescing is not optional, and it is already built.** `POST /lists/refresh`
emits one `list_refreshed` per list; fifteen lists means fifteen events inside
one second, and a naive "re-read on event" would issue fifteen `GET /lists`.

Since D1a made `lists` a refresh endpoint, the event handler calls
**`registry.invalidate('lists')`** — the same call the `RefreshCluster`'s manual
Refresh button makes. That path already coalesces concurrent requests into one
(`p5-05` V9b: three rapid clicks, one request) and already resets the endpoint's
timer on success. **The 500 ms window this plan previously specified is deleted**
— it would have been a second coalescing mechanism beside a working one, and a
timer outside the registry on a page that now has one inside it.

A mutation does the same: `POST`/`PATCH`/`DELETE` resolve, then
`invalidate('lists')`.

The route declares `endpoints: ['lists']` — **not** `telemetry`.
`compiled_rules` and `duplicates_removed` come from the `lists` response; the
compile duration comes from the `/telemetry` one-shot above.

**`/telemetry` stays a one-shot on this page, deliberately.** D3/X1 settled that
Lists reads it once on mount and does not poll it, and D1a did not reopen that:
D1a is about `/clients` and `/lists` going stale on the Dashboard, and a compile
duration does not drift on its own — it changes only when a compile happens, and
every compile on this page is one the page itself triggered.

**So the re-read carries `/telemetry` with it.** Every path that invalidates the
inventory — a mutation, or a `list_refreshed` batch — recompiles the ruleset,
which is exactly what changes `compile_duration_seconds`. The handler therefore
calls `registry.invalidate('lists')` **and** re-issues the `/telemetry`
one-shot, the latter guarded by its own in-flight flag so fifteen events produce
one of each (V11).

Two read paths on one page is a cost worth naming: `lists` through the registry,
`telemetry` as a direct call. It is what keeps a page that renders one
`/telemetry` field from putting a standing timer on it.

---

## 6. New and changed modules

### 6.1 API client

| File | Contents |
| ---- | -------- |
| `src/api/stats.ts` | `STATS_PATH`, `getStats(signal?)` |
| `src/api/history.ts` | `HISTORY_SUMMARY_PATH`, `getHistorySummary({from, to?, resolution}, signal?)` |
| `src/api/lists.ts` | `LISTS_PATH`, `getLists`, `addList`, `patchList`, `deleteList`, `refreshList`, `refreshAllLists` |
| `src/api/config.ts` | `CONFIG_PATH`, `getConfig(signal?)` |
| `src/api/clients.ts` | `CLIENTS_PATH`, `getClients(signal?)` |
| `src/api/types.ts` | `Stats`, `StatsBucket`, `TopDomain`, `TopClient`, `PolicyStat`, `HistorySummary`, `HistoryItem`, `ListItem`, `ListStatus`, `ListsResponse`, `RefreshAllResult`, `RefreshAllResponse`, `Client`, `ClientsResponse`, and a **narrow** `Config` |

`Config` is typed narrowly —
`{ history?: { enabled?: boolean }, dns?: { upstreams?: { strategy?: string } } }`
— because the response is the whole config tree and typing all of it here would
duplicate `CONFIGURATION.md` in TypeScript and rot. `p5-09` owns the full shape.

Every accessor goes through `api/core.ts`, inheriting the `401` guard, envelope
decoding and `Retry-After` handling rather than reimplementing them.

### 6.2 Chart

| File | Contents |
| ---- | -------- |
| `src/charts/stacked-bars.ts` | The uPlot option factory and the label/hover plugin. **Imports no uPlot symbol at module scope** — types only. |
| `src/charts/format.ts` | `compactCount` (`13.2k`), `percent1`, `bucketWindowLabel` |
| `src/components/chart.tsx` | Extended: an optional `plugins` passthrough and a `paths`-capable options object. Still the only module that touches `uplot` at runtime, still through `await import()`. |

### 6.3 Pages and page components

| File | Contents |
| ---- | -------- |
| `src/pages/dashboard.tsx` | Route component; owns the one-shot fetches and the `stats` listener |
| `src/pages/dashboard/*.tsx` | `tiles.tsx`, `queries-over-time.tsx`, `query-types.tsx`, `upstream-health.tsx`, `top-domains.tsx`, `top-clients.tsx`, `cache-state.tsx`, `ruleset-card.tsx` |
| `src/pages/lists.tsx` | Route component; inventory, coalesced re-read, mutation dispatch |
| `src/pages/lists/*.tsx` | `list-row.tsx`, `list-card.tsx` (phone), `add-list-dialog.tsx`, `refresh-all-dialog.tsx`, `list-status.tsx` |

### 6.4 Shared components extended

- `components/status-pill.tsx` — `Status` gains `never` and `disabled`.
- `components/donut.tsx` — **new.** The ring the artboards draw with
  `stroke-dasharray`, legend beside it. Plain SVG, no library.
- `components/chart.tsx` — see 6.2.

### 6.5 Route table, refresh union, preferences

`routes.ts`:

- `/` and `/lists` get `built: true` and a `load`. No other route's `built` flag
  moves.
- `REFRESH_ENDPOINTS` widens to
  `['health', 'telemetry', 'cache', 'clients', 'lists']` (D1a).
- `/` declares `endpoints: ['telemetry', 'cache', 'health', 'clients', 'lists']`
  — `health` per C11, the last two per D1a.
- `/lists` declares `endpoints: ['lists']` and `events: ['list_refreshed']` — the
  subscription §5.2 mounts, declared in the route table like every other one.
- **No other route gains an event or an endpoint.** `/` keeps `events: ['stats']`
  (C2) and gains no second event type. In particular `/clients` (the page,
  `p5-07`) is left alone; it will declare `clients` when it is built, and until
  then nothing but the Dashboard polls it.

`refresh/registry.ts`: `DEFAULT_FETCHERS` gains `clients: (s) => getClients(s)`
and `lists: (s) => getLists(s)`. Nothing else in that module changes — the
refcounting, coalescing, suspend and timer-reset behaviour all apply to the two
new endpoints unmodified, which is the point of adding them here rather than
building a second reader.

`refresh/preferences.ts`: `KEYS` gains `clients: 'fah-refresh-clients'` and
`lists: 'fah-refresh-lists'`. `endpointForKey` iterates `KEYS`, so cross-tab
propagation covers both with no further edit.

`constants.ts`:

```text
REFRESH_OPTIONS_SECS.clients = [60, 300]      REFRESH_DEFAULT_SECS.clients = 300
REFRESH_OPTIONS_SECS.lists   = [60, 300]      REFRESH_DEFAULT_SECS.lists   = 300
```

`REFRESH_LABELS` is unchanged — both reuse `1 m` and `5 m`.

### 6.6 Where the two new selectors live — deviation X3

`RefreshCluster`'s contract is **one per distinct polled endpoint that has a
user-facing refresh control in the artboard**. A polled endpoint the artboards
give no control to gets no cluster — polling and controlling are separate
questions. The Dashboard now polls five, and the artboards draw exactly two
clusters — Upstream health (`telemetry`) and Cache state (`cache`).

`health` needs none, and that is the contract working rather than an exception
to it: it is polled for one word in a tile footer and the artboard provides no
refresh control for it, so there is no cluster. But `clients` and `lists` each now have a
browser-local interval preference, and a preference with no control is a
setting an operator cannot reach.

**Decision: two further clusters, in the card title bars the artboards already
use for that purpose** — Top clients (bound to `clients`) and Ruleset (bound to
`lists`), in `placement="card"`, the same position and treatment Cache state
draws. This is an artboard deviation, recorded as **X3** beside X2, and it is
the direct consequence of D1a: the artboards were drawn when those two cards
were static.

Reversing it is one line each if the controls are judged clutter — the endpoints
keep polling at their defaults and only the selectors disappear. Flagged rather
than assumed.

#### The same contract applied to Lists — **no cluster on that page**

Stated explicitly so neither implementation nor review has to infer it.

**The Lists page carries no `RefreshCluster`, for any endpoint.** `lists` stays
route-scoped and polled through the shared registry exactly as §5.2 describes —
what it does not get is a user-facing interval selector on this page.

This is the §6.6 contract producing its ordinary result, **not a deviation**, and
X3 is not widened:

- **The artboard draws no refresh control.** `Lists.dc.html` draws *mutation*
  controls — **Refresh all** and **Add list** — and a mutation is not a refresh
  control. **Refresh all** is `POST /lists/refresh`: it makes the server refetch
  every list, which is a different act from choosing how often this browser
  re-reads the inventory. Rendering them side by side would put two
  similar-looking controls with very different blast radii in one title bar.
- **The preference is still reachable**, so §6.6's "a preference with no control
  is a setting an operator cannot reach" does not apply here. `fah-refresh-lists`
  is browser-local and cross-tab (D1a), and the Dashboard's Ruleset cluster sets
  it. One endpoint, one preference, one control — on the page the artboards give
  a control to. Lists reads the same value.
- **Staleness is already handled on this page, and better than a selector would
  handle it.** Every mutation and every `list_refreshed` batch calls
  `registry.invalidate('lists')` (§5.2), so the inventory revalidates immediately
  on the events that actually change it. An interval selector would only govern
  the idle case, which on an inventory that changes on operator action or on a
  24 h `refresh_hours` is the case that matters least.

**Consequence for the code:** `/lists` declares `endpoints: ['lists']` and
renders no `RefreshCluster`. `p5-07` does not inherit a "pages with a polled
endpoint get a cluster" rule from this task — it inherits the §6.6 contract,
which asks what the artboard draws.

---

## 7. Chart engineering

The single hardest unit in this task, and the one the rest of the phase reuses.

### 7.1 Stacking without stacking maths

uPlot has no stacked-bar mode. The artboard draws blocked at the baseline and
permitted above it, both sharing a left edge and width.

**Two series, both drawn from zero, painted back to front:**

- series 1 = `queries` (the full bar), permitted colour
- series 2 = `blocked`, blocked colour, painted second and therefore on top

The visible permitted band is `queries − blocked` by occlusion, so no stacked
sums are computed and the y-axis maximum is `max(queries)` with no accumulation
error. `permitted` is the legend word; `allow` never appears (phase constraint
5).

Both series use `uPlot.paths.bars({ size: [0.9, 60], align: 0 })`. The `0.9`
gap ratio reproduces the artboard's 51.8 px bar in a 54.7 px slot.

### 7.2 The printed-figure floors

uPlot prints nothing on bars. A `hooks.draw` plugin runs after the series and,
per bucket:

```text
barWidthPx = u.bbox.width / items.length * 0.9
if barWidthPx >= 50 : print total above the bar, 9.5 px mono, tick colour
segPx      = u.valToPos(0, 'y', true) - u.valToPos(blocked, 'y', true)
if segPx   >= 15    : print blocked inside the segment, 9 px mono 500, white
```

Both floors are measured in canvas pixels after `devicePixelRatio` division, so
they mean the same thing on a phone and on a retina desktop. At 390 px with 24
buckets a bar is ~11 px and both labels vanish — precisely what
`MobileDashboard.dc.html` states about itself, with no phone branch in the code.

### 7.3 Hover — the only hover state in the system

`cursor: { x: true, y: false }`, `focus: { alpha: 1 }`. On `setCursor`:

- the tooltip is a positioned DOM node, not canvas — dark surface, mono, four
  rows: bucket window, `queries`, `blocked`, `blocked_percent`.
- `blocked_percent` is **read from the item**, never divided (task requirement).
- non-hovered bars dim through per-bar fill: `paths.bars`' `disp.fill.values`
  reads a module-level array rebuilt on cursor change, followed by
  `u.redraw(false)`. If `disp` proves unreliable at the pinned uPlot version,
  the fallback is a second `draw` pass overpainting non-hovered bars with the
  card background at 0.55 alpha; the visual result is identical and whichever
  was used is recorded in the review file.
- touch: `cursor.dataIdx` on `touchstart` gives the mobile artboard's "tap a
  bar" behaviour with no second code path. Pinch-zoom stays off —
  `cursor.drag: { x: false, y: false }` — which is what the artboard's own note
  says.

### 7.4 Axes, theme and the three non-chart states

- y-axis: five gridlines, labels right-aligned **outside** the plot, 10 px mono
  in the tick colour, values through `compactCount`. `Performance`'s treatment,
  as the artboard draws it.
- x-axis: 4 labels at 24 h (`10:00`, `16:00`, `22:00`, `04:00` in the artboard),
  ~5 at 7 d/30 d, in the viewer's local timezone.
- Theme: every colour is a CSS custom property read through `getComputedStyle`
  at option-build time, and the options object is rebuilt when the theme
  changes — uPlot draws to canvas and cannot inherit CSS.
- **Three distinct non-chart states**, which must not be confusable:
  1. `history.enabled === false` → "History is not being recorded", plus what
     turns it back on (Settings → `history.enabled`). Range chips hidden —
     there is no range to pick.
  2. `items.length === 0` with recording on → "No data in this range". Chips
     stay live.
  3. request failed → `ErrorState`.

  **1 and 2 are told apart by `/config`, and an empty response is exactly when
  the mount snapshot may be stale** — so an empty `items` with a snapshot of
  `enabled: true` triggers the single disambiguation re-read specified in §5.1
  before either state renders. The chart shows its loading state for that one
  round trip rather than flashing the wrong answer.

### 7.5 Bundle

`uplot` is 41,845 B gzip (measured in `p5-05`). Current shipped total is
19,347 B gzip against a 153,600 B budget. Projected after this task:
**≈ 62–66 kB gzip, ≈ 43 %** — comfortable, and the build gate decides, not this
estimate.

### 7.6 uPlot's stylesheet — inherited finding **m8**, and this task decides it

`p5-05`'s review deferred m8 here with the reason stated. **An earlier draft of
this plan got it wrong** and specified a postbuild assertion that no shell or
login asset mentions `uplot`, `.css` included. That assertion cannot pass:

`vite.config.ts:41` sets **`cssCodeSplit: false`**, so every stylesheet in the
graph is merged into the one `style-*.css` fetched on the login path —
regardless of whether the module importing it is statically or dynamically
reached. `chart.tsx:3`'s `import 'uplot/dist/uPlot.min.css'` therefore lands in
the global stylesheet the moment any shipped page imports `Chart`, which is what
this task does. The JS split is unaffected and real; only the CSS claim is at
issue, and it is ~1 KB gzip against a 153,600 B budget — a claim-accuracy
problem, not a weight one.

**`cssCodeSplit` is not touched.** `p5-05` chose `false` deliberately — one
stylesheet, no second request and no FOUC on a lazy route — and reversing it for
under a kilobyte would be the tail wagging the dog. Option (b) from the earlier
draft is struck.

#### The measurement

`node_modules/uplot/dist/uPlot.min.css` at the pinned **1.6.32**:

| raw | gzip | brotli |
| ---: | ---: | ---: |
| 1,857 | **772** | 606 |

772 B gzip is **0.5 %** of the 153,600 B budget. **Bytes do not decide this.**
What the file contains does:

| Rule group | Reached by this chart? |
| ---------- | ---------------------- |
| `.uplot` box-sizing, `.u-wrap`, `.u-over`, `.u-under`, `.uplot canvas`, `.u-axis`, `.u-*.u-off` | **yes** — structural, the plot does not lay out without them |
| `.u-cursor-x` / `.u-cursor-y` and the `.u-hz` / `.u-vt` variants | **yes** — §7.3 turns the x cursor on |
| `.uplot` `font-family` and the cursor's `1px dashed #607D8B` | **yes, and both are overridden anyway** — §7.4 requires the chart's chrome be themed from our tokens, so these two are dead weight the moment the chart is themed |
| `.u-legend`, `.u-series`, `.u-inline`, `.u-marker`, `.u-live` | **no** — the legend is disabled; the artboards draw the legend as our own markup below the plot |
| `.u-select` | **no** — `cursor.drag` is `{x: false, y: false}` |
| `.u-cursor-pt` | **no** — bar dimming replaces cursor points |
| `.u-title` | **no** — the title lives in the card title bar |

Roughly **half** the stylesheet is unreachable, and two of the reachable rules
are values this task overrides regardless.

#### Decision: (c)

Drop `import 'uplot/dist/uPlot.min.css'` from `chart.tsx`; write the structural
and cursor rules into `styles/components.css` under a `.chart` scope, with the
font and cursor colour taken from our tokens instead of overridden on top of
uPlot's. Estimated ~15 declarations, well under 300 B raw.

**The reason is not the 570 B saved.** It is that §7.4 already obliges this task
to write themed overrides for the font and the cursor, so (c) writes those rules
*once* rather than layering them over vendor defaults — and it makes the
acceptance criterion literally true for CSS as well as JS instead of true with a
footnote.

**The cost, stated rather than buried:** ~15 lines coupled to uPlot's internal
class names. They are uPlot's documented DOM contract and stable across 1.6.x,
and the version is pinned — but this is now ours to maintain. Two mitigations,
both cheap:

- the block carries a comment naming **exactly which uPlot features it covers**
  (structure, x-cursor) and stating that enabling the legend, drag-select or
  cursor points requires restoring the corresponding vendor rules. `p5-08` reads
  that before it turns anything on for `Performance`;
- W3 renders the chart with the vendor stylesheet removed **before** any other
  chart work, so a missing structural rule shows up as a broken plot in the dev
  gallery immediately, not three units later.

**Confirm at W6 against the real build**, since the decision above is read off
the vendor file and the config rather than off an emitted asset: if any needed
rule turns out to be missing the fallback is (a), one line to restore, and the
review file records which shipped.

**Postbuild assertion under (c):** no shell or login asset, **`.js` or `.css`**,
mentions `uplot`. If the fallback to (a) is taken, the assertion narrows to
`.js` and the review file states that scope in the same sentence as the bundle
figures — never a wider claim than what shipped.

### 7.7 Inherited finding **m4** — `Chart`'s stable-options contract

`p5-05` fixed m4 (the plot rebuilding on every render) but deferred its runtime
proof here, because uPlot needs a canvas 2D context jsdom does not implement.
`ChartProps.options` is documented as needing referential stability; §6.2's
option factory must therefore return a **memoised** object — built once per
`(range, theme)` pair, not per render. W3 hoists it behind a `useMemo` keyed on
exactly those two, and W5 confirms in a browser that switching range rebuilds the
plot once and that a `stats` push, which re-renders the page every ~2 s, rebuilds
it **zero** times. That is m4's missing proof and it is recorded as V9a.

---

## 8. Dashboard — card by card

Order, wording and placement are `Main.dc.html`'s. Sources are API.md's.

| Zone | Artboard | Source |
| ---- | -------- | ------ |
| Tile row 1 (`c4`) | Total queries · Queries blocked · Percentage blocked · Cache hit rate | `/stats`: `queries_total`, `blocked_total`, `blocked_percent`, `cache_hit_percent`. Footers: "N active clients" → `/clients` `items.length` (D1); "watch the live feed" → link; "of all DNS queries"; "inspect the cache" → link |
| Tile row 2 (`c4`) | HTTP requests · HTTP blocked · Compiled rules · Uptime | `/telemetry`: `counters.http.pass + allow + block`, `counters.http.block`, `ruleset.rules`, `process.uptime_seconds`. Footer "3 refused by egress policy" → `counters.http.refused`; "status ok" → `/health.status` (C11) |
| **"since restart" label** | — | A caption on tile row 2, not a tooltip. Row 1 is a rolling 24 h window; `counters.http` is process-lifetime cumulative. No 24 h HTTP figure exists and none is derived. |
| Queries over time (full width) | title + totals in the secondary slot; range chips right | `/history/summary` for the bars **and the totals** — see below. §7 |
| Query types (`c2` left) | donut + legend table with count and % | `history/summary.per_type`, summed over the active range; label set is the fixed rollup set. "other" folds the tail below the fifth entry, as drawn |
| Upstream health (`c2` right) | per-endpoint row: state dot, address, protocol, `N attempts · M failures`, bar | `/telemetry.upstreams[]`: `address`, `protocol`, `attempts`, `failures`, `state`. Secondary "strategy: X" → `/config` (C12). Refresh cluster bound to `telemetry`. Footnote verbatim. Bar per **D4/A**: width `attempts / max(attempts)`, overlay `failures / attempts`, `state` on the dot |
| Top queried (`c2`) | domain · hits · frequency bar | `/stats.top_queried_domains` |
| Top blocked (`c2`) | domain · hits · frequency bar | `/stats.top_blocked_domains` |
| Top clients (`c2`) | client (ip + name) · queries · blocked · share | `GET /clients`, sorted by `queries_24h` desc (C8) |
| Cache state (`c2`) | fresh/stale/expired/free bar, legend, four figures | `/cache`: `fresh`, `stale`, `expired`, `capacity − entries`, `load_percent`, `byte_load_percent`, `hits`, `evictions`. Secondary `entries N / capacity`. Refresh cluster bound to `cache` |
| Ruleset (full width) | compiled rules · duplicates removed · last compile duration · enabled lists | `/telemetry.ruleset` for the first three; the fourth is `GET /lists` items where `enabled` (D1, approved) |

### 8.1 The chart's totals come from the chart's own response

`Main.dc.html` prints `184,233 queries · 23,411 blocked · 12.7 %` in the card
title bar. Those happen to equal the `/stats` figures beside them, because the
artboard draws the 24 h range.

**They are not `/stats` figures, and taking them from `/stats` is a bug at every
range.**

- At **7 d and 30 d** it is plainly wrong: `/stats` is a rolling 24 h snapshot
  (`"window": "24h"`) and has no 7 d or 30 d total. The header would state a
  24 h figure over 30 d of bars.
- At **24 h** it is subtly wrong: `/stats`' window is rolling from *now*, while
  `/history/summary` returns hour-aligned buckets. The two cover different 24 h
  spans, so the printed total would not equal the sum of the bars beneath it —
  a total that disagrees with its own chart.

**The totals are summed from the same `items` that draw the bars**, for every
range:

```text
queries = Σ items[].queries
blocked = Σ items[].blocked
percent = blocked / queries * 100        (0 when queries == 0)
```

The percent is the one aggregate the API does not serve. It is a derivation of
two summed counts over one response — exact, not an average of averages — and it
is listed in §8.2. This does **not** contradict the task's "`blocked_percent` is
served per item — do not derive it": that rule governs the **hover tooltip**,
which reads `items[i].blocked_percent` verbatim (§7.3). The range aggregate has
no served field.

Consequences, both correct:

- the totals change when the range changes, as they must;
- they do **not** move with the `stats` push, because they describe the plotted
  range rather than the live 24 h window. The tiles move; this line does not.

`/stats.queries_total`, `blocked_total` and `blocked_percent` keep feeding tile
row 1, which is where the rolling 24 h window belongs.

### 8.2 Every derived display value on both pages

Phase 5 standing constraint 1 forbids figures the API does not support. A
**derivation** is a stated arithmetic function of documented fields. This table
is the complete list for both pages; **anything not on it is read from a field
verbatim, and nothing else may be derived.** A reviewer checks the pages against
this table.

| # | Display | Formula | Fields | Where |
| - | ------- | ------- | ------ | ----- |
| R1 | `permitted` band | `queries − blocked`, produced by occlusion (§7.1), never computed | `history/summary.items[].queries`, `.blocked` | chart |
| R2 | chart range total — queries | `Σ items[].queries` | same | chart title |
| R3 | chart range total — blocked | `Σ items[].blocked` | same | chart title |
| R4 | chart range total — blocked % | `R3 / R2 × 100`, `0` when `R2 == 0` | same | chart title |
| R5 | HTTP requests tile | `counters.http.pass + counters.http.allow + counters.http.block` | `/telemetry` | tile row 2 |
| R6 | uptime "4h 31m" | formatting of `process.uptime_seconds`, not arithmetic | `/telemetry` | tile row 2 |
| R7 | cache `free` band | `capacity − entries` | `/cache` | Cache state |
| R8 | top-domain frequency bar | `count / max(count)` over the rendered rows — the normalization `Main.dc.html` settles (`3,140 / 4,021 = 78 %`) | `/stats.top_*_domains[].count` | top-N tables |
| R9 | Top clients share bar | `queries_24h / max(queries_24h)` over the rendered rows, same rule as R8 | `/clients` | Top clients |
| R10 | Top clients blocked % (phone) | `blocked_24h / queries_24h × 100` | `/clients` | Top clients, < 768 px |
| R11 | "N active clients" | `items.length` | `/clients` | Total-queries tile footer |
| R12 | "N enabled lists" | count of `items[]` where `enabled` | `/lists` | Ruleset card, Lists header |
| R13 | donut segment % | `per_type[k] / Σ per_type` over the range's summed `per_type` | `history/summary.items[].per_type` | Query types |
| R14 | donut "other" | sum of every label outside the five largest, as the artboard draws | same | Query types |
| R15 | Lists partition bar | each of `rules_active_dns`, `rules_active_url`, `rules_inactive` over `rules_total` | `/lists.items[]` | Lists rows |
| R16 | Lists "13 / 15" | `count(enabled) / items.length` | `/lists` | Lists header |
| R17 | refresh-all summary | `refreshed + failed`, asserted to equal `results.length` | `POST /lists/refresh` | refresh-all result |
| R18 | Upstream bar width | `attempts / max(attempts)` over the rendered endpoints; `0` when `max(attempts) == 0` (D4/A) | `/telemetry.upstreams[].attempts` | Upstream health |
| R19 | Upstream failure overlay | `failures / attempts` of R18's width; `0` when `attempts == 0`; **not drawn below one pixel and never widened to a minimum** (D4/A) | `/telemetry.upstreams[].failures`, `.attempts` | Upstream health |

Explicitly **not** derived, and read verbatim: `blocked_percent` per bucket
(hover, §7.3), `cache_hit_percent`, `load_percent`, `byte_load_percent`,
`compiled_rules`, `duplicates_removed`, `parse_errors`, every `rules_*` count,
every upstream `attempts` / `failures`, `stride`.

**Never derived at all:** a 24 h HTTP figure (the API has none), a combined
DNS + HTTP "queries" total (the pipelines are never summed), an upstream share
of traffic (no per-query attribution exists), a per-client blocked figure from
`/stats` (it is not there — R10 uses `/clients`).

**Upstreams specifically — R18 and R19 are the complete set (D4/A).** No success
rate, no health score, no availability percentage, no uptime-per-endpoint, and
nothing derived from `consecutive_failures`, `failure_runs`, `penalty_round`,
`penalties`, `penalized_seconds_total`, `probes`, `probe_successes` or
`tls_handshakes`. Those fields exist on the response and this card does not
render them; `p5-08`'s Upstreams page decides what it does with them, and
inherits R18/R19 unchanged for the bar.

**Refresh clusters: four on this page** — Upstream health (`telemetry`) and
Cache state (`cache`) as the artboards draw them, plus Top clients (`clients`)
and Ruleset (`lists`) per §6.6's X3. `health` carries none: it is polled, but the
artboard gives it no refresh control — one word in a tile footer, no card to hang
one on — and the contract (§6.6) counts controls, not timers. Zones fed by the
`stats` push or by `/history/*` carry none either, for the other reason: a zone
with no timer has nothing to control.

---

## 9. Phone

### 9.1 Dashboard at 390 px — build to `MobileDashboard.dc.html`

The artboard is the specification. What it settles, and what implements it:

| Artboard | Implementation |
| -------- | -------------- |
| Tiles two-up, 54 px, 24 px figure, 11.5 px label | `.row.c4 > *` already spans 6 of 12 columns under 1199 px; below 768 px the tile's internal type scale changes, nothing else |
| Both tile rows keep their footer strip, shortened ("6 clients", "proxy", "3 refused", "13 lists", "status ok") | short labels are a `<768 px` string swap on the same `Tile` |
| Range selector as full-height chips **above** the chart, `min-height: 44px`, `flex: 1` | the desktop chips move out of the card title bar into the card body under 768 px |
| No printed bar figures; tap for the tooltip | falls out of §7.2's floors. No code branch |
| Top-N as rows, domain given the space and the count right-aligned, ellipsis on overflow | `Table` swaps to a row list under 768 px, not a horizontal scroller |
| "Show all 10" / "Open Clients" trailing row, 44 px | a `.more` row: the top-N tables render 3 rows on a phone, 5 on desktop |
| Fold marker at 844 px viewport | tells the ordering: tiles, chart, then row 2 — the artboard's order, not the desktop order. **Row 2 moves below the chart on a phone** |
| Upstream health and the Ruleset card are absent | the artboard drops both with a stated reason. They stay desktop-only; the closing note is rendered verbatim |
| Cache card compressed: fresh/stale/expired only, refresh cluster above the bar, "Open Cache" row | as drawn |
| Drawer = `MobileNav.dc.html` | **already built in `p5-05`** (`shell.tsx`, `sidebar.tsx`, `layout.css` §Mobile chrome). This task **verifies** it — 44 px rows, scrim, footer with connection state, version, theme, sign-out — and does not rebuild it. Its footer's "up 4h 31m" is the one drawn element the shell does not yet supply; see §13 |

### 9.2 Lists at 390 px — new artboard (D2)

No phone artboard exists. The rule the drawn ones establish is **one card per
list, never a horizontal table**. Specified here so the drawing and the code
agree:

```text
┌──────────────────────────────────────────┐
│ oisd-basic                    [OK]       │  id mono 500 · status pill right
│ https://small.oisd.nl                    │  source, mono, muted, ellipsised
├──────────────────────────────────────────┤
│ ████████████████░░░░  223,182 rules      │  partition bar + rules_total
│ 198,500 dns · 9,181 url · 15,501 inactive│  mono 11 px, wraps
│ 0 parse errors                           │  own line on a phone
├──────────────────────────────────────────┤
│ [ on ]   every 24 h   ·   last 04:00     │  44 px row: switch, interval, age
├──────────────────────────────────────────┤
│  Refresh          Edit          Remove   │  44 px actions, three-up
└──────────────────────────────────────────┘
```

- A `failed` / `rejected` / `degraded` card grows an explanation block between
  the header and the partition — the artboard's amber row background, the
  `last_error` text, and "last good copy still serving".
- A `rejected` card's action row replaces `Refresh` with **`Delete and re-add`**,
  because refresh cannot clear it (see §10.3).
- The page header card (compiled rules · duplicates removed · last compile ·
  enabled/configured) goes two-up on a phone.

**Header note (§4 D3/X1).** The header card renders all **four** figures
`Lists.dc.html` draws — compiled rules · duplicates removed · last compile ·
enabled / configured — two-up on a phone, four-up on desktop. The first, second
and fourth come from the `GET /lists` envelope and its items; "7.41 s last
compile" is `/telemetry.ruleset.compile_duration_seconds`, read by the
**one-shot** `GET /telemetry` on mount (§5.2). The route declares
`endpoints: ['lists']` — the inventory refreshes through the shared registry
(D1a). **`/telemetry` is not one of them**: it stays a direct one-shot fetch, so
no timer polls it.

**No `RefreshCluster` anywhere on this page** — not on this card, not on the
table, for `lists` or `telemetry`. §6.6 settles it: the artboard draws mutation
controls (**Refresh all**, **Add list**), not a refresh control, and
`invalidate('lists')` already revalidates on every event that changes the
inventory. The `lists` interval is set from the Dashboard's Ruleset cluster and
shared browser-wide.

### 9.3 Breakpoints

`visual-system.md` §Responsive, unchanged: ≥ 1200 px full grid; 768–1199 px
halves go full width and the sidebar collapses to icons; < 768 px single column,
sidebar as an overlay drawer, tables scroll inside their own container. The page
body never scrolls horizontally at any width. Both pages are checked at 1400,
900 and 390 px in both themes.

---

## 10. Lists — behaviour

### 10.1 Inventory

Columns exactly as `Lists.dc.html`: List (id + source) · On · Every · Last
refresh · Status · Rules (partition bar + mono line) · Total, plus the actions
cell from C4. Header legend for the three tiers is rendered verbatim, as is the
two-sentence footnote under the table.

The partition bar uses `StageBar` with the artboard's three colours; the legend
carries each word and figure, so colour is never the only signal.

### 10.2 The five statuses

| `last_status` | Presentation |
| ------------- | ------------ |
| `ok` | green pill, no body |
| `degraded` | **its own amber-distinct pill and a body line**: "fetch succeeded, most of the body failed to parse — likely a format misdetection", linking to RULE_ENGINE.md §Supported formats. Never renders as a milder `ok` |
| `failed` | red pill, `last_error`, and "last good copy still serving" |
| `rejected` | amber pill, `last_error`, "content gate refused the body before it could commit", and the recovery action (§10.3) |
| `never` | neutral pill, "not fetched yet" |
| `enabled === false` | `DISABLED` pill; real `last_status` as secondary when not `never` (C3) |

### 10.3 The `rejected` recovery

A `rejected` row's primary action is **`Delete and re-add`**, not refresh and not
disable/enable — the cached copy is the baseline and neither clears it. The
action opens a `ConfirmDialog` stating what will happen, then runs
`DELETE /lists/{id}` followed by `POST /lists` with the same `id`, `url`/`path`,
`enabled` and `refresh_hours`. If the `DELETE` returns `500` the list is kept by
the API and the sequence stops with that message shown — the re-add is never
attempted against a baseline that is still there.

### 10.4 Refresh one vs refresh all — different, and shown differently

- **Refresh one** — `202 Accepted`. The row enters a *pending* state ("refresh
  requested") which is cleared by the `list_refreshed` event for that id, not by
  the response. The event carries no reason, so the re-read supplies it from
  `last_error`.
- **Refresh all** — synchronous and blocking. The button enters a busy state,
  the page shows a modal progress state stating that this runs one pass and
  recompiles once, and the `200` body's `results[]` is rendered as a per-list
  outcome list including `rejected` rows with their `error`. `refreshed + failed`
  equals `results.length` and the summary says so.

Neither is allowed to look like the other. Refresh-all does not fake progress
it cannot know — it is a blocking spinner plus a statement of what is happening,
then results.

### 10.5 Add, and the `409`

`POST /lists` takes either `url` or `path`, with optional `id`,
`enabled`, `refresh_hours`. The dialog offers URL or mounted path as an explicit
choice, not a guess at the string.

A `409` is rendered as **what it is**, using the API's own message:

- a derived-id collision → offer to retry with an explicit `id`, prefilled;
- the same source already configured under another id → name that list and link
  to its row.

A `422` anchors to its field. A `500` on the config write is reported as the API
states it: the mutation did not happen and the file and the engine still agree.

---

## 11. Work units

Ordered by dependency. Each ends with `npm run typecheck`, `npm run test` and
`npm run build` green before the next begins — the `p5-05` discipline.

| # | Unit | Ends when |
| - | ---- | --------- |
| W1 | API types + accessors (`stats`, `history`, `clients`, `config`, `lists`) | typecheck green; unit tests for query-string building and the `refreshAll` shape |
| W1a | **D1a wiring**: widen `REFRESH_ENDPOINTS`, add both fetchers, both preference keys, both constants entries | pin test updated to the five names; preference round-trip and cross-tab tests extended to `clients` and `lists`; a rejected hand-edited interval still falls back to the default |
| W2 | `StatusPill` extension, `Donut`, `format.ts` | gallery renders every status and a donut; tests for `compactCount` and the "other" fold |
| W3 | `charts/stacked-bars.ts` — options, two-series occlusion stacking, axes; **m8/(c) first**: vendor stylesheet dropped, replacement rules in `components.css` | dev gallery draws 24 real bars with a y-axis **and no vendor CSS loaded** — a missing structural rule surfaces here, not three units later |
| W4 | Label plugin — the 50 px / 15 px floors | pure floor function unit-tested at 24/7/30 buckets × 1400/900/390 px |
| W5 | Hover plugin — tooltip, dimming, touch | manual at both widths; `blocked_percent` proven to come from the item |
| W6 | `chart.tsx` plugin passthrough; **confirm m8/(c) against the real build** (§7.6) and add the postbuild assertion | build emits a chart chunk; no shell/login `.js` **or `.css`** mentions uPlot; if (c) failed and (a) shipped, the assertion narrows to `.js` and the review says so |
| W7 | Dashboard shell: route `built`, five declared refresh endpoints, the two one-shots (`/stats`, `/config`), `/history/summary`, `stats` listener, abort on unmount | page renders tiles from a live API; leaving it clears all five timers and issues nothing (V2, V4) |
| W8 | Tiles (both rows) + "since restart" caption | every figure traced; HTTP tiles labelled |
| W9 | Queries over time: range chips, refetch, range totals from `items` (§8.1), decimation footnote, three non-chart states, the disambiguation re-read | range switching correct; totals equal the sum of the drawn bars at all three ranges; `history.enabled=false` distinct from an empty range even when switched off mid-session |
| W10 | Query types, Upstream health (**D4/A**), Cache state, Ruleset, both top-domain tables, Top clients | all cards live; every figure matches a §8.2 row or a verbatim field. The R18/R19 pair is a pure function, unit-tested before it is rendered: normal rows, a `failures == attempts` row, `attempts == 0`, all-zero `max`, and the sub-pixel case asserting **no** minimum width |
| W11 | Dashboard at 390 px against `MobileDashboard.dc.html` | structure matches: two-up tiles, chip selector, row-form top-N, reordering below the fold |
| W12 | Lists: inventory through the registry, partition, five statuses, header card (incl. the `/telemetry` one-shot), `invalidate`-based re-read | rows render; fifteen events produce one `GET /lists` and one `GET /telemetry`; route declares `endpoints: ['lists']` and no timer exists outside the registry; **zero `RefreshCluster` instances render on the page** (§6.6) |
| W13 | Lists mutations: add (+`409`), enable/disable, interval, remove, refresh one, refresh all | every path exercised against a live API |
| W14 | `MobileLists.dc.html` + `canvas.json`, and the phone card layout | artboard and code agree (D2) |
| W15 | Full verification pass (§12) and the review file | every V item recorded |

---

## 12. Verification

Frontend gates (`npm run typecheck`, `npm run test`, `npm run build`) plus the
workspace gates (`cargo fmt --check`, `cargo clippy --workspace --all-targets
-- -D warnings`, `cargo test --workspace`). Then, against a **running**
container, read off a request log and a WebSocket frame log:

| # | Check |
| - | ----- |
| V1 | Every figure on both pages traced to a documented field, in a table in the review file. Every derived figure matches a **§8.2** row and no derivation exists that §8.2 does not list. `permitted` labelled as itself; `allow` absent from both pages |
| V1c | Upstream bars measured from the DOM against R18/R19 on live `/telemetry` data: each width equals `attempts / max(attempts)`, each overlay equals `failures / attempts` of it, the `attempts · failures` text is present on every row, and the state dot is the only thing carrying `state`. No fourth upstream figure anywhere on the card |
| V1a | Chart title totals equal the sum of the drawn bars at 24 h, 7 d and 30 d — checked by reading both off the same response. No `/stats` figure appears in that slot |
| V1b | Top clients and the enabled-list count observed to **change** without a reload after a client appears and a list is toggled in another tab — D1a's whole point, measured rather than assumed |
| V2 | Dashboard mounted 10 min: **zero** `GET /stats` after the first; **one** `/config`; N `stats` frames at the ~2 s cadence; `telemetry`, `cache`, `health`, `clients` and `lists` each at exactly their interval and nothing else (a second `/config` appears only if a range came back empty — V7a) |
| V2a | `REFRESH_ENDPOINTS` asserted to be exactly `['health','telemetry','cache','clients','lists']` — a sixth needs a deliberate edit |
| V2b | Changing the `clients` or `lists` interval issues **zero** requests, stores the value, propagates to another tab, and the next fetch lands at the new cadence — the `p5-05` V9a/V11b method extended to both |
| V2c | `registry.activeTimers()` reads **5** on the Dashboard, **1** on Lists, **0** on any other route |
| V3 | **Zero** `query` frames received while on the Dashboard |
| V4 | Leaving the Dashboard: subscription released, socket closed if nothing else holds it, traffic to zero inside one refresh interval |
| V5 | Leaving Lists: `list_refreshed` released, the `lists` timer cleared, zero requests after — `/telemetry` included, and any in-flight one-shot aborted |
| V5a | Lists mounted 10 min, idle: `/lists` at its interval and **exactly one** `/telemetry`, at mount (D3/X1). `activeTimers()` reads 1 |
| V5b | Dashboard → Lists → Dashboard: the `lists` timer is never torn down and rebuilt mid-navigation more than once per transition, and no duplicate `/lists` request is issued by the two routes sharing the endpoint |
| V6 | Range switching refetches `/history/summary` once per change and nothing else — **no `/config`, `/clients` or `/lists` request on a range change** |
| V7 | `history.enabled=false` via Settings → the chart and donut say "history disabled", visibly different from an empty range (screenshot both) |
| V7a | Switched off **in another tab while the Dashboard stays mounted**: the next range change renders "history disabled", not "no data in this range", at the cost of exactly **one** extra `/config` request (§5.1). An empty range with recording on issues that one re-read too and then says "no data in this range" |
| V8 | An empty range renders "no data in this range", not an error |
| V9 | A `stride > 1` response labels the series decimated (forced with a small `max_points`) |
| V10 | Refresh-all blocks, shows progress, reports per-list outcomes including a `rejected` one |
| V11 | Fifteen `list_refreshed` events → **one** `GET /lists` and **one** `GET /telemetry` |
| V12 | A `degraded` row is visibly distinct from `ok`; a `rejected` row offers delete-and-re-add and the sequence works |
| V13 | A conflicting add shows the API message and names the other list; both `409` kinds exercised |
| V14 | Both themes at 1400 / 900 / 390 px: `scrollWidth == clientWidth` everywhere |
| V15 | Dashboard at 390 px matches `MobileDashboard.dc.html` in structure; drawer matches `MobileNav.dc.html` |
| V16 | Lists at 390 px is one card per list; artboard committed |
| V17 | Every interactive control ≥ 44 px at 390 px, measured from the DOM |
| V9a | **m4's deferred proof** (§7.7): switching range rebuilds the uPlot instance exactly once; 5 min of `stats` pushes rebuild it **zero** times. Counted with a construction counter, in a real browser |
| V18 | Bundle: gzip **and** brotli per asset and total, against 153,600 B. uPlot's **JS** in the chart chunk and absent from shell and login `.js`. The CSS claim is reported at whatever scope §7.6's decision actually supports — never claimed wider |
| V19 | Memory: 5 rounds × both pages, heap flat — the `p5-05` V11 method |
| V20 | Hidden document: polling stops at once, socket closes after the grace, zero requests while hidden |

---

## 13. Repository documents this task proposes to change

**Listing is not permission** (phase CLAUDE.md). Each is proposed here and waits
for the owner's yes at the point the task reaches it.

| Document | Proposed edit | Why |
| -------- | ------------- | --- |
| `docs/dashboard/information-architecture.md` §Dashboard | "stacked permitted/blocked **area**" → **bars** | C1 — three other sources say bars and the artboard draws them |
| `docs/dashboard/information-architecture.md` §Dashboard | socket subscribes to `stats` **only** | C2 — the task and the shipped route table already say so |
| `docs/dashboard/information-architecture.md` §Dashboard | name `GET /clients` as the Top-clients source | C8 — `/stats.top_clients` cannot supply the blocked column both artboards draw |
| `docs/dashboard/sketch/MobileLists.dc.html` (new) + `canvas.json` | the phone Lists artboard | D2 — **already approved**; W14 |

**No shell change — deviation X2 (§4 D3, approved).**
`MobileNav.dc.html`'s drawer footer draws
"connected · v0.2.20 · **up 4h 31m**". The shell reads `/health` once per mount
for the version and does not keep uptime, which is
`/telemetry.process.uptime_seconds`. The drawer is chrome on every route, so
feeding it means a shell-level `/telemetry` read — a poll that runs regardless
of what is mounted, and the phase invariant's "global state is allowed; global
polling is not". **The uptime clause stays out of the drawer footer**; the
figure lives on the Dashboard's Uptime tile, which both artboards draw. p5-06
therefore makes no change to `shell.tsx`, `sidebar.tsx` or `topbar.tsx` beyond
what the route table drives.

---

## 14. Risks

| Risk | Mitigation |
| ---- | ---------- |
| The hand-written uPlot CSS (§7.6/c) silently breaking when `p5-08` enables the legend, drag-select or cursor points | the block comments exactly which features it covers and what to restore; W3 proves the structural subset by rendering with the vendor sheet gone before any other chart work |
| An implementer giving the failure overlay a minimum width so it "shows up" | D4/A forbids it explicitly and W10's unit test asserts the sub-pixel case. A widened overlay draws a failure rate the endpoint does not have |
| uPlot's `disp.fill` per-bar dimming not behaving at the pinned 1.6.32 | §7.3 fallback is specified up front, same visual result, decided by measurement not preference |
| uPlot CSS landing in the shell stylesheet | postbuild assertion (W6), not inspection |
| Bar-label floors tuned on a desktop and wrong on a phone | floors are unit-tested as a pure function across 3 bucket counts × 3 widths (W4) before any rendering work |
| Fifteen `list_refreshed` events fanning into fifteen inventory reads | `registry.invalidate('lists')`, whose existing in-flight coalescing collapses them into one (§5.2); the `/telemetry` one-shot has its own in-flight flag. Verified as V11 with a real refresh-all |
| The Dashboard's read set growing again in a later task | D1/D1a fix it now and name what each endpoint buys; the `REFRESH_ENDPOINTS` pin test (V2a) makes a sixth endpoint a deliberate edit |
| Five timers on one route reading as "the Dashboard polls a lot" | they are five endpoints on one shared registry, all route-scoped, all suspended when hidden, all cleared on unmount — V2c counts them and V4 proves they stop. The alternative D1a rejected was figures that silently go stale |
| `lists` now read by two routes with different freshness needs | one endpoint, one timer, one retained value; the Lists page adds `invalidate` on its event, which the Dashboard benefits from for free. V5b checks the shared transition |
| A `rejected` list's delete-and-re-add half-completing | `DELETE` `500` stops the sequence; the list is still there and the message says so (§10.3) |
| Two visually identical tile rows over different windows | the "since restart" caption is a rendered element with a test, not a comment |

---

## 15. Out of scope

Every other page. `/stats.policies` (Policies owns it). `/stats.buckets` (C14).
Long-range top-N (`/history/top`) — used only where the IA says, which is not
these two pages. Client renaming and policy assignment (Clients, `p5-07`).
The Phase 3 / Phase 4 follow-up reviews the phase file already records.

---

## 16. Handed to a later task — an artboard defect found here

Found while settling this task's HTTP figures, in an artboard this task does not
build. Recorded so `p5-09` does not rediscover it, and so nobody "fixes" the
wrong half of it.

### 16.1 `LiveFeed.dc.html` draws a row that cannot exist · **for `p5-09`**

`docs/dashboard/sketch/LiveFeed.dc.html` draws an HTTP row with a `REFUSED`
pill in the Verdict column (`10:41:01.402`, `203.0.113.9/beacon`, detail
"egress policy · IP literal host"). **No such row can ever render.**

API.md §GET /telemetry is explicit about `counters.http.refused`:

> requests the egress policy refused before any upstream contact … **It is
> counted on the proxy, not on the event stream**, so it is not part of
> `pass + allow + block`

Two consequences, and they are separate:

- **`refused` is not a `verdict` value.** The enum is `pass` | `allow` | `block`
  (RULE_ENGINE.md §Verdicts), for both pipelines.
- **An egress refusal emits no `query` event at all**, so it is not a Live Feed
  row at any verdict. The figure is aggregate-only.

`p5-09` picks one: drop the row from the artboard, or keep the artboard and
record the row as an undrawn element in its own review file, the way X2 is
recorded here. **Not this task's edit** — `LiveFeed.dc.html` is `p5-09`'s
artboard and touching it here would change a file this task neither builds nor
verifies.

**Where the figure legitimately appears, and it already does:** `/telemetry`'s
`counters.http.refused`, in this task's tile row 2 footer — "3 refused by egress
policy" (§8). That reading is unaffected and needs no change.

### 16.2 The half that is **not** a defect — HTTP verdicts are real

Stated because the natural reading of the artboard invites the opposite
conclusion, and getting it wrong would blank a column that carries data.

**HTTP rows carry a real verdict.** RULE_ENGINE.md §HTTP matching runs HTTP
through the *same compiled ruleset* via a typed entry point, so it returns the
same three verdicts as DNS. API.md's own `query` event example is `"kind":
"http"` with `"verdict": "block"`, `"rule"` and `"list"` populated, and
`counters.http` carries `pass`, `allow` and `block` separately from `refused`.

So the Verdict, Rule and List columns are correct as drawn on HTTP rows, and
**must not** be rendered as `—` for the HTTP pipeline. `allow: 0` in API.md's
sample response is a household with no HTTP exception rules yet, not a
structural zero.

This also confirms §8's HTTP requests tile (R5): `pass + allow + block` is the
right sum, and `refused` is correctly kept out of it.

### 16.3 What **pause** means on the Live Feed · **for `p5-09`**

`LiveFeed.dc.html` draws a **pause** chip in the filter bar and says nothing
about what it stops. Recorded here because the answer is not obvious and the
wrong one costs a socket round trip and an extra subscription state.

**Pause is a render control, not a transport control.**

```text
pause    → the table renders a frozen snapshot
           the socket stays connected and subscribed, unchanged
           the ring keeps filling underneath
           a counter shows what arrived while paused
resume   → the snapshot is discarded, the ring renders, the counter clears
```

One buffer, not two: the snapshot is a reference to what the ring held at the
moment of the click, and the ring is the bounded 500-row structure that already
exists. Nothing new grows with uptime.

**Why not stop the subscription:**

- It adds a second axis to subscription state — mounted × paused — where there
  is currently one. Every route-scoped rule in the phase file is written against
  mount and unmount; pause would be the first thing that is neither.
- Resubscribing costs a round trip and leaves a gap of unknown size. Freezing
  the view leaves a gap of *known* size, because the counter states it.
- It buys nothing. The ring is bounded either way, so "not losing events" is not
  on offer — a long pause on a busy resolver overruns 500 rows regardless.

**One thing to reconcile when the task opens.** The phase file's route-scoped
section says idling must never be implemented as `{"subscribe":[]}`, on the
grounds that removing the traffic disables the server's dead-peer watchdog.
API.md §WS /events now documents the other half: an empty list is valid, and a
subscription that omits `stats` makes the server send a `Ping` on the same ~2 s
cadence *for exactly that reason*. `p5-03` appears to have closed the hole the
phase file warns about. It does not change the recommendation above — pause
should not touch the subscription either way — but `p5-09` should confirm which
document is current before relying on either sentence.

**Why this matters to §16.4's row actions:** a frozen table gives row actions a
stable UI snapshot to operate on. The live ring may continue to receive and
evict events underneath it, but those mutations do not alter the paused
snapshot. Any action therefore targets the row data captured by the snapshot,
not a row that may have moved or been evicted in the live ring.
