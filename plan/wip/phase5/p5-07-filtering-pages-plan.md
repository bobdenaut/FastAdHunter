# P5-07 — Filtering Pages · Development Plan

**Task:** [p5-07-filtering-pages.md](p5-07-filtering-pages.md) ·
**Depends on:** `p5-06` (confirm/mutate/error idiom, one-DOM responsive pattern,
`ContentHeader`, `derive.ts` discipline) · **Branch:** `phase5-07` from the
completed `phase5-06` · **Model:** Opus

---

## 1. Outcome

Four shipped routes — `/rules`, `/policies`, `/clients`, `/rule-tester` —
replacing their `not-yet-built` empty states, all four `built: true` in
`src/router/routes.ts`, all four correct at 390 px and at desktop in both
themes.

**All four declare `events: []` and `endpoints: []`.** Nothing on these pages
subscribes to the socket and nothing on them holds a timer. They read on entry
and re-read on explicit user action. The connection indicator reads
**not needed here** on every one of them, and the socket is closed while one is
the active route.

This is also the task that lands the two pure modules the filtering model needs
and that `p5-08`/`p5-09` inherit: `policy/selectors.ts` (the client-selector
matcher, mirroring `crates/fah-model/src/policy.rs`) and
`policy/assignment.ts` (the direct-versus-inherited classification).

---

## 2. Sources and precedence

Highest first. §3 resolves every conflict against this ordering.

1. **The artboards** — `docs/dashboard/sketch/CustomRules.dc.html`,
   `Policies.dc.html`, `Clients.dc.html`, `RuleTester.dc.html`,
   `MobileClients.dc.html`. They win on structure, placement, labels, ordering
   and responsive behaviour.
2. **The API** — `API.md`, and where the two disagree, **the Rust**. An
   artboard cannot invent a field, an endpoint, a derivation or a mutation.
   Phase 5 standing constraint 1 is not overridable by a drawing.
3. **The task file** — `p5-07-filtering-pages.md`.
4. **`information-architecture.md`** and **`visual-system.md`** — where the
   artboards are silent.
5. **Existing implementation patterns** — only where consistent with the above.

**Artboard figures are not measurements** (phase constraint 8). `176,204`,
`96 %`, `3 / 16` are drawings. Where an artboard's *internal* arithmetic is the
only statement of an encoding rule, it is read as a rule and said so (§8.5 T9).

**Inherited from `p5-06`'s review** (status: `DONE`), and binding here:

| From p5-06 | Obligation here |
| ---------- | --------------- |
| **F1 / fourth fix pass** — a desktop row and its header must be **one grid**, and row actions are 44 × 44 glyphs, never labels, or the actions track varies by state | Clients' desktop table follows both. §8.3 |
| **N1** — a switch or toggle is measured, not eyeballed; 32 × 18 px failed | Every control in this task is measured from the DOM at 390 px (V17) |
| **F16 / third fix pass** — `scrollWidth === clientWidth` is checked at **1200, 1247 and 900 px**, not only at 1400 and 390 | V14 checks four widths |
| **F17 / N4** — a mark or tint that reads in one theme and vanishes in the other; `color-mix(… X%, transparent)` premultiplies to ~1.8 % alpha | The dashed-chip treatment (§7.2) is a border style plus a token, never an alpha wash |
| **Header ownership** — "Lists' Refresh all / Add list sit at the top of the page body, not in the page header. A header slot is a shell seam `p5-07` can decide on" | Decided: §3 C10 |
| Working-tree housekeeping | `docs/code-review/phase2.6/p2.6-11-optin-deploy-soak-review.md` is still modified and belongs to the parallel 2.6 track. **Stage `p5-07`'s paths explicitly; never `git add -A`.** |

---

## 3. Conflicts, resolved

| # | Conflict | Resolution |
| - | -------- | ---------- |
| C1 | `Clients.dc.html` draws the direct assignment's schedule (`mon–fri 21:00 → 07:00`), `window shut now`, and `via 192.168.20.0/24`. `GET /clients` carries none of them — only `policy` and `assignment_source`. | **Owner decision D1(a).** Clients reads `GET /policies` as a second entry one-shot and cross-references. One request, no per-row fan-out, and the response is needed anyway for the assign-policy picker. §5.3 |
| C2 | The artboard's footnote says "the API says which" — for subnet versus name it does not. `assignment_source` is `"direct"` or absent, and absent covers subnet, name **and** unassigned (`wire.rs:610-611`, `routes.rs:383-388`). | **Owner decision D2(a).** The selector is named only when exactly one assignment in the in-force policy matches this client; otherwise the row says `inherited` and claims nothing. §7.3. The artboard's footnote is reworded (§13). |
| C3 | `Clients.dc.html` prints the blocked-share bar at the *percentage* width (10 % bar for 10.0 %, 17.7 % for 17.7 %). `p5-06`'s R8/R9 normalise top-N bars against the row maximum. | **Percentage width**, per the artboard's own internal arithmetic. Two different encodings in one application is a real cost, so it is stated in the legend: the Clients bar is a share of that client's own traffic, not a share of the table. §8.5 T9 |
| C4 | `MobileClients.dc.html` draws a green **live** dot in the top bar. This route holds no subscription. | **not needed here**, per the phase file's three-state indicator. The artboard predates that decision, which is phase-level and wins. Deviation **X1**. |
| C5 | `Policies.dc.html` draws a green/grey dot per assignment and an `ACTIVE NOW` tag per card. The API exposes one global `active_assignments` (`routes.rs:851`) and no per-policy or per-assignment flag. | **Owner decision D4(a).** Dot and tag dropped; the header keeps `active_assignments` verbatim. Deviation **X2**. |
| C6 | `CustomRules.dc.html` says a save means "**No restart, no recompile of the lists**". `set_user_rules` (`crates/fah-rules/src/lifecycle/mod.rs:943-953`) runs a full `compile()` + `swap_in` — the same rebuild the Policies card labels `RECOMPILES`. | **Owner decision D6(a), with the owner's wording:** *"No restart and no list refetch. The ruleset is rebuilt and atomically swapped."* No confirm dialog — the button is already deliberate — but the save is a **blocking busy state**, per R2 below. |
| C7 | `RuleTester.dc.html` says a client name "selects the policy actually in force for it". `routes.rs:1266-1276` sets the policy only on the address branch; a name yields `ClientContext { name, ..default() }`, so the deciding policy stays `default`. API.md §`POST /rules/test` repeats the artboard's claim. | **Owner decision D5(a).** The UI resolves a name to an address through `GET /clients` and sends the address, **visibly** (§8.4). Where it cannot, the result is marked partial, never presented as a working name test. API.md correction proposed (§13). |
| C8 | The task requires "verdict, matching rule, source list, deciding policy, **and why that policy applies**", while the acceptance criterion says "all four result fields". | Not a contradiction. `RuleTestResponse` (`wire.rs:742-751`) has exactly four fields and IA §Rule Tester names exactly those four. **The four are the API's; the "why" is a derived explanation** built from `/policies` and `/clients` by the same module Clients uses. §8.4, T13. |
| C9 | `GET /policies` items are the *configured* policies (`routes.rs:850` maps `config.policies`). `default` is never among them — it is implicit and reserved (`fah-config/src/lib.rs:284-290`). The artboard draws a Default card. | The Default card is **synthetic**: fixed copy, `RESERVED` tag, `lists` rendered as "every enabled list", no assignments, no Edit, no Delete. Its only live figure is traffic, from `/stats.policies` where `policy === "default"`. §8.2 |
| C10 | Three of the four artboards draw a page-specific context line and two draw header buttons. The shell renders `<ContentHeader title={route.title} />` and nothing else (`shell.tsx:194`), which is why Lists' buttons ended up in the page body. | `Route` gains **`ownsHeader?: boolean`**; the shell skips its own header for a route that declares it, and the page renders `<ContentHeader>` itself. `ContentHeader` gains an `actions` slot. Declarative, in the table that is already "the single declarative source", no shared state and no stale closure. **Lists is not retrofitted** — that is a behaviour-neutral change to a shipped page and belongs to whoever next touches it. |
| C11 | `clients` and `lists` are `REFRESH_ENDPOINTS` members with retained values in the registry (`p5-06` D1a). Reusing a retained value on these pages would avoid a request. | **Rejected.** `useRefresh` subscribes, and a subscription starts a timer — which the task forbids on all four routes. Reading the retained value *without* subscribing would need a new registry API and would open the page whose whole job is current state on data up to five minutes old. Plain one-shots. The cost is named: Dashboard → Clients issues a `GET /clients` the registry already holds. §5.5 |
| C12 | `PUT /rules/user` silently drops exact-duplicate rule lines (`routes.rs:1186-1197`), so the document that comes back can be shorter than the one sent. | On a `2xx` the editor is **replaced** with `response.rules.join('\n')` and the page states `N duplicate line(s) removed` when the count differs. Never a silent rewrite. |
| C13 | `Policies.dc.html` header prints `EET-2EEST`; `timezone` is the full POSIX string `EET-2EEST,M3.5.0/3,M10.5.0/4`. | The displayed value is the substring before the first comma, with the full string as the element's `title`. A stated display truncation, T7. |
| C14 | `blocking_mode` is drawn as a policy field, but the only value the config accepts is `null_ip` (`schema/dns/blocking.rs:37-49`, `lib.rs:310-321`). | The control is a two-state override — **inherit the global mode** (`null`) or **`null_ip`** — not a free list. A `PATCH` clearing it sends `"blocking_mode": null`, which the double-option deserializer distinguishes from absent (`wire.rs:801-812`). |

### Deviation registry — the complete list

Every place a built page deliberately departs from its artboard. Anything not
listed here is artboard-verbatim; T7's timezone truncation is a stated display
truncation, not a deviation. X4 and X5 were added by the 2026-08-27 plan
review.

| # | Artboard draws | Shipped instead | Decided |
| - | -------------- | --------------- | ------- |
| X1 | `MobileClients.dc.html` — a green `live` dot in the top bar | `not needed here` | C4 |
| X2 | `Policies.dc.html` — a green/grey per-assignment dot and an `ACTIVE NOW` tag | both dropped; the header keeps `active_assignments` verbatim | C5 / D4 |
| X3 | `Policies.dc.html` — `tv` printed beside an assignment's address | the selector as configured, no resolved name | §8.2 |
| X4 | `Clients.dc.html` / `MobileClients.dc.html` — the shut-window note stops at `window shut now` | branch-2 notes also name the policy in force, so the "Policy in force" column never hides it | §7.3 |
| X5 | `CustomRules.dc.html` — the per-line callout appends a diagnosis ("the exception has no domain between `\|\|` and `^`") | the callout renders the parsed `422` message and nothing more — the diagnosis is text the response cannot supply | §7.1 |

---

## 4. Owner decisions — resolved 2026-08-27

All six were put to the owner and answered. Recorded as settled; §11 assumes
them. Three carry corrections the answers implied but did not state; those are
**R1–R3** and are equally binding.

### D1 — Clients reads `GET /policies` too · **(a), APPROVED**

Two entry one-shots, `/clients` and `/policies`. `/policies` is bounded
(≤ 15 configured policies, each with a handful of assignments), carries every
assignment's selector plus `days` / `start` / `end` plus the `timezone`, and is
needed regardless to populate the assign-policy picker.

**Not** per-row `GET /clients/{ip}/policy`. That endpoint stays what IA calls
it: the single-address read and the write path.

### D2 — the inheritance label is named only when unambiguous · **(a), APPROVED**

Solid chip ⇔ `assignment_source === "direct"`. Dashed ⇔ absent. The row names
the inherited selector (`via 192.168.20.0/24`, `via name tv`) **only** when
exactly one assignment in the in-force policy matches this client. Otherwise:
the word `inherited`, and no claim.

### D3 — "window shut now" is inferred, never computed · **(a), APPROVED**

No clock and no timezone arithmetic in the browser. The inference is over two
responses that already state the answer: `client.policy` names `P`, the policy
in force (§7.3's notation); the direct assignment's owner naming a different
policy means that assignment is not deciding.

> **R1 — the branch order the answer needs, and did not state.**
>
> A direct assignment can also lose to an **open name assignment**, because a
> `Name` selector outranks an `Ip` one (`fah-model/src/policy.rs:144-151`:
> name 1000 > address 900 > prefix length). Testing "window shut" first would
> label an open window shut whenever a name assignment overrode it. The name
> case is therefore tested **before** the schedule case, and a branch that can
> prove neither makes no claim at all. The full ordered procedure is §7.3.

### D4 — the per-assignment dot and `ACTIVE NOW` are dropped · **(a), APPROVED**

The header keeps `active_assignments` verbatim — the field, its label, and the
sentence the task asks for ("reports a schedule boundary having passed without
waiting for a query"). Assignment rows render `client`, days and window as
text with no in-force claim.

**One semantic the count carries and the UI must not restate wrongly:**
`active_assignments` is `state.policies.current().len()`, and `active_at`
expands a `Name` selector into **one entry per matching named client**
(`fah-rules/src/policy.rs:378-388`). So the count is assignments-in-force after
name expansion, which can exceed the number of configured assignment rows. The
label is `assignments in force right now` — never "of the N configured".

### D5 — Rule Tester resolves a name to an address · **(a), APPROVED, with the owner's UX condition**

> *"Nu trebuie să pară că «name works» când de fapt există doar fallback-ul
> imperfect."*

```text
user types a name
  → GET /clients (already read on entry)
  → find the observed client by name — ASCII-case-insensitive, the engine's own
    fold (`eq_ignore_ascii_case`, §6.2); "TV" finds "tv"
  → send POST /rules/test with that client's address
```

The substitution is **shown, not hidden**: the result card reads
`tested as 192.168.10.50 (tv)`. Three branches, and only the first is a
success:

| Match | Behaviour |
| ----- | --------- |
| exactly one observed client (ASCII-case-insensitive) | send the address. Result card names the address it substituted. |
| **more than one** | client names are **not unique** — `client_registry.rs:127-131` `set_name` writes the field with no uniqueness check, so two addresses can carry one name; and the fold means `Tv` and `tv` are two matches too. The form asks which address before it will run. **(R3)** |
| none | send the raw name, and mark the result **partial**: a banner stating that the engine selects a policy only from an address, that this name has never been observed, and that `deciding policy` therefore reads `default` regardless of any assignment. Not styled as a success. |

API.md correction proposed (§13). The doc currently promises more than the
engine does.

### D6 — Custom Rules states the recompile, and does not confirm it · **(a), APPROVED**

The artboard's sentence is replaced with the owner's:

> **No restart and no list refetch. The ruleset is rebuilt and atomically
> swapped.**

No confirm dialog: `Validate and save` is already the deliberate act, and the
document is what the operator just typed. But see R2 — the request blocks.

> **R2 — every recompiling mutation blocks its own HTTP request.**
>
> `ListManager::recompile` (`lifecycle/mod.rs:1240-1244`) takes the compile lock
> and awaits `compile()` inline, and `apply_policies` (`routes.rs:925-931`)
> calls it inside the handler. So `POST /policies`, `DELETE /policies/{id}`,
> a `PATCH` that changes `lists`, **and `PUT /rules/user`** all hold the
> connection open for the whole compile — the figure the Lists header already
> prints as `7.41 s · last compile`, and more on the RB5009.
>
> Consequence, binding on §10: none of these is an optimistic update. Each
> enters a **blocking busy state that states what is happening**, the same
> treatment `p5-06` gave Refresh-all (blocking modal, indeterminate bar, no
> faked progress), and no second mutation can be started while one is in
> flight. `PATCH` without a `lists` change and every `/clients` mutation are
> live in milliseconds and get none of this.
>
> **And a recompiling request, once sent, is never aborted.** Axum drops a
> handler future when its connection closes, so an abort mid-compile can cancel
> between persist and swap — `set_user_rules` with the raw text committed but
> not swapped in, `apply_policies` with the config persisted but the recompile
> unfinished — leaving persisted state ahead of the live matcher, with the
> client unable to know whether the write landed. The busy state is therefore a
> **modal that also blocks in-app navigation**: unmount cannot happen while a
> recompiling mutation is in flight. The unmount-abort rule in §5 covers reads
> and the millisecond-live mutations — which land server-side even when
> aborted — never a recompiling one.

> **R3 — duplicate client names are possible and must be handled.**
>
> Stated above in D5. It also affects Policies: a `Name` assignment matching
> two addresses contributes two entries to `active_assignments`, which is
> correct and is why D4's label avoids "of N configured".

### R1–R3 traceability

A correction that lives only in this section is a correction that gets lost at
the keyboard. Each one is carried into the data flow, a work unit and a
verification row:

| | Data flow | Work unit | Verification |
| - | --------- | --------- | ------------ |
| **R1** — name-override tested before shut-window | §5.3, closing note; the procedure is §7.3 | **W3** (the pure branch order), **W8** (staged against a live API) | **V7** shut window, **V7a** open window overridden by name, **V7b** scheduled name attributes nothing |
| **R2** — recompiling mutations block their own request | §5.1 (Custom Rules, no dialog to hang the wait on), §5.2 (the two mutation shapes), §5.3 (all three live) | **W6** (rules save), **W9** (clients, the live half), **W11** (policies) | **V8** policies blocking, **V8a** the live half timed, **V8b** the rules save timed |
| **R3** — client names are not unique | §5.4 (the disambiguation branch) | **W12** | **V12b** the tester's three branches, **V11a** the Policies count consequence |

**No open decisions remain in this plan.**

---

## 5. Data flow

Every endpoint each page touches, classified. The four classes are **entry
one-shot**, **mutation-triggered reread**, **user-triggered request**, and
**shared retained value** (used by none of these pages — C11).

### 5.1 Custom Rules (`/rules`)

```text
mount
  ├─ subscribe events:   none          → route declares []
  ├─ subscribe refresh:  none          → route declares []
  └─ GET /rules/user     entry one-shot → the document
save (user action)
  └─ PUT /rules/user     BLOCKING (R2) — the request holds open for the whole
                         compile; 2xx replaces the document, non-2xx changes nothing
unmount
  └─ the in-flight GET aborted. A PUT cannot be in flight at unmount: its busy
     modal blocks navigation (R2). No timer existed to clear.
```

**R2 applies here and is easy to miss** because this page has no confirmation
dialog to hang the wait on. `set_user_rules` compiles inline
(`lifecycle/mod.rs:948-949`), so the save is seconds, not milliseconds. The
`Validate and save` button enters a blocking busy state that names what is
happening, and `Discard` and the editor are disabled while it is in flight.
There is no optimistic path: the document is not shown as saved until the
response lands.

### 5.2 Policies (`/policies`)

```text
mount
  ├─ GET /policies   entry one-shot → items, timezone, active_assignments
  ├─ GET /stats      entry one-shot → stats.policies, the only per-policy traffic source
  └─ GET /lists      entry one-shot → the subset picker's list ids, and
                                      compiled_rules for the What-costs-what card
create / delete / edit (user action)
  ├─ POST   /policies              BLOCKING (R2) — recompiles
  ├─ DELETE /policies/{id}         BLOCKING (R2) — recompiles
  ├─ PATCH  /policies/{id}  lists  BLOCKING (R2) — recompiles
  ├─ PATCH  /policies/{id}  name | blocking_mode | assignments
  │                                live, milliseconds, no confirmation
  └─ on success → reread GET /policies only
unmount
  └─ in-flight reads aborted. A recompiling mutation cannot be in flight at
     unmount — its busy modal blocks navigation (R2). A live PATCH aborted
     mid-flight still applies server-side; the next entry re-reads.
```

**R2 splits this page's mutations into two shapes, and they must not look
alike.** The three that recompile hold the connection for the whole rebuild
(`apply_policies` → `recompile()`, `routes.rs:925-931`), so each is
confirmation → blocking busy state → response, with no second mutation
startable meanwhile. The fourth returns in milliseconds and gets neither a
dialog nor a busy state. §10.1 is the table; §10.2 is the treatment.

`/stats` and `/lists` are **not** re-read after a policy mutation: a policy edit
changes neither the list inventory nor the 24 h traffic window. A deleted
policy's traffic row disappears from `/stats.policies` on its own schedule and
the card is already gone.

`GET /stats` is a large response for one field. It is the only source of
`stats.policies` (API.md §`GET /api/v1/stats`), it is read once per mount, and
the alternative is a card the artboard draws with no data behind it.

### 5.3 Clients (`/clients`)

```text
mount
  ├─ GET /clients    entry one-shot → the rows
  └─ GET /policies   entry one-shot → assignment selectors, schedules, timezone (D1)
rename          → PUT /clients/{ip}            {name} | {name: null}
assign policy   → PUT /clients/{ip}/policy     {policy, days?, start?, end?}
clear           → DELETE /clients/{ip}/policy
  └─ every one of the three, on success → reread BOTH /clients and /policies, in parallel
unmount
  └─ in-flight aborted. All three mutations are live and land server-side even
     if aborted mid-flight; the next entry re-reads.
```

**Why both, on every mutation.** The two responses are cross-referenced (§7.3),
so they must describe one instant. A rename republishes the policy snapshot
(`routes.rs:418-420` — a name can move a client into or out of a name
assignment), and an assignment moves a row between policies inside `/policies`
(`routes.rs:985-1000` strips the address from every policy before adding it).
Patching the local copy to mirror that server logic is a second implementation
of the resolver's bookkeeping in the frontend; two small re-reads are cheaper to
be right about.

The mutation responses (`ClientResponse`, `ClientPolicyResponse`) are used for
the error path and to paint the row from the server's **committed** response
while the two re-reads land — never as the page's new source of truth. Nothing
here is optimistic: the chip changes only after the server has answered.

**None of the three mutations recompiles** (`Recompile::No` at
`routes.rs:1002`, `routes.rs:1037`, and a rename that only republishes the
snapshot at `routes.rs:420`), so none is confirmed and none blocks. That is the
visible half of R2: this page's writes are milliseconds and are labelled `LIVE`,
against Policies' seconds. The contrast is the point, not an implementation
detail.

**The two responses feed §7.3's ordered classification, and the order is R1's.**
Every row's chip and note come from that one procedure — name-override tested
**before** shut-window, and no claim at all when neither can be proven. The page
holds no clock and evaluates no schedule.

### 5.4 Rule Tester (`/rule-tester`)

```text
mount
  ├─ GET /policies   entry one-shot → the policy-mode picker, and the "why" explanation
  └─ GET /clients    entry one-shot → the client picker, and name → address resolution (D5)
Test (user action)
  ├─ client mode, a name typed → resolve against the /clients snapshot (D5),
  │                              ASCII-case-insensitively
  │     one match   → send that address, and say so on the result
  │     two or more → BLOCK on a disambiguation prompt (R3); no request is sent
  │     none        → send the raw name, mark the result partial
  └─ POST /rules/test  → one result, appended to a bounded session ring
unmount
  └─ in-flight aborted. The ring dies with the page.
```

**R3 is why the middle branch exists.** `set_name`
(`client_registry.rs:127-131`) writes the field with no uniqueness check, so two
addresses can carry one name and the resolution has no correct single answer.
The form asks rather than picks. This is also the branch that keeps D5's UX
condition honest: nothing on this page may make a name look like it worked when
what ran was the imperfect fallback.

`POST /rules/test` does **not** recompile and does not block — it reads the
running matcher (`routes.rs:1264`). R2 does not apply to this page.

### 5.5 What is deliberately **not** here

- **No route declares `endpoints`.** `REFRESH_ENDPOINTS` stays the five names
  `p5-06` pinned; `registry.ts`, `preferences.ts` and `constants.ts` are
  untouched by this task.
- **No route declares `events`.** No page adds a type to the union, so the
  shell's last release closes the socket on entering any of the four.
- **No `RefreshCluster` on any of the four**, and this is §6.6's contract
  producing its ordinary result rather than an exception: a cluster controls a
  timer, and there is no timer to control.
- **No page reads a registry retained value** (C11).
- **Nothing calls `every()` or `after()`** from `lifecycle/timers.ts`. The one
  permitted use of that module is `nowMs()` for relative-time labels, and
  `subscribeAgeTick` is **not** used — a "2 s ago" that re-renders itself is a
  timer, and these pages have none. Last-seen labels are computed once per
  render from the fetched payload, exactly as `lists.tsx` does with its `now`
  state, which advances only when the page re-renders for another reason.

---

## 6. API client and pure modules

### 6.1 API client

| File | Contents |
| ---- | -------- |
| `src/api/policies.ts` | `POLICIES_PATH`, `getPolicies`, `createPolicy`, `patchPolicy`, `deletePolicy` |
| `src/api/rules.ts` | `USER_RULES_PATH`, `RULES_TEST_PATH`, `getUserRules`, `putUserRules`, `testRule` |
| `src/api/clients.ts` | extended: `setClientName`, `setClientPolicy`, `clearClientPolicy`. **No `getClientPolicy`** — no page reads the per-address endpoint (D1, V5), and an accessor with no caller is dead code (principle 14) |
| `src/api/types.ts` | `Policy`, `PoliciesResponse`, `Assignment`, `CreatePolicyBody`, `PatchPolicyBody`, `ClientPolicyResponse`, `ClientPolicyBody`, `UserRules`, `RuleTestBody`, `RuleTestResult` |
| `src/api/index.ts` | re-exports |

`PolicyStat` already exists in `types.ts` from `p5-06`.

Two shape notes that are easy to get wrong and are pinned by test:

- **`PatchPolicyBody.lists` is a double option.** Absent leaves the subset
  alone; `null` clears it back to "every enabled list"; an array sets it
  (`wire.rs:801-812`). In TypeScript that is `lists?: string[] | null`, and the
  request builder must emit the key with a literal `null` — not drop it —
  when the form chose "every enabled list". Same for `blocking_mode`.
- **`DELETE /policies/{id}` answers `204`** and `DELETE /clients/{ip}/policy`
  answers `204`; `core.ts` already returns without parsing on `204` and `202`.

Every accessor goes through `api/core.ts`. No second fetch wrapper.

### 6.2 `src/policy/selectors.ts` — new, pure

The client-selector matcher, mirroring `crates/fah-rules/src/policy.rs:460-481`
(parsing) and `crates/fah-model/src/policy.rs:127-181` (matching, specificity,
prefix containment).

```text
parseSelector(spec)   → {kind:'ip', ip} | {kind:'network', ip, prefixLen} | {kind:'name', name}
matchesClient(sel, ip, name?)  → boolean
specificity(sel)      → 1000 name | 900 ip | prefixLen
```

Rules copied exactly, and each one is a test case:

- a `/` **commits** the value to being a prefix; a malformed address before it
  is not silently a name;
- prefix length is capped at 32 (v4) / 128 (v6);
- containment compares whole bytes then the masked partial byte;
- **mixed families never match** — a v4 client is not in a v6 prefix however
  the bits line up;
- a name compares **ASCII**-case-insensitively (`eq_ignore_ascii_case`), so the
  fold is an explicit ASCII fold, not `toLocaleLowerCase`;
- a client with no name never matches a `Name` selector.

This is the one module in the task that reimplements engine logic, and it does
so because the alternative is a per-row request the task forbids. It is pure,
it is unit-tested against the Rust's own cases, and it is the first thing a
reviewer checks.

### 6.3 `src/policy/assignment.ts` — new, pure

The classification (§7.3), the schedule-text formatter (§7.4), and the "why
that policy applies" sentence the Rule Tester renders (§8.4). Takes
`(client, policies)` and returns a discriminated result; performs no I/O, holds
no clock.

The `direct` lookup inside it is **raw string equality** on
`assignment.client`, mirroring `routes.rs` (§7.3); `parseSelector` /
`matchesClient` from §6.2 are used only for the `named(P)` test and branch 4's
candidates. The chip style itself is `assignment_source`'s, taken verbatim.

### 6.4 `src/policy/validation.ts` — new, pure

Mirrors the config validator so a form rejects before the API does, and never
*only* the form:

| Function | Mirrors |
| -------- | ------- |
| `validatePolicyId` | `routes.rs:1140-1162` — lowercase alphanumerics plus `.`/`_`/`-`, not starting with `.`, and `default` reserved |
| `parseDays` | `schema/policy.rs:91-127` — `daily`/`all`, comma lists, inclusive ranges, wrapping (`fri-mon`), long forms (`monday`) |
| `parseTimeOfDay` | `schema/policy.rs:136-156` — `HH:MM`, `24:00` accepted as an end bound |
| `bothOrNeither` | `lib.rs:372-391` — `start` and `end` set together or not at all |
| `parseUserRulesError` | the 422 message parser, §7.1 |

Form validation is a courtesy, never the guarantee: every one of these paths
still renders the API's own `422` when the server disagrees.

### 6.5 Components

| File | Status |
| ---- | ------ |
| `src/components/policy-chip.tsx` | **new** — solid / dashed chip plus its note. Colour is never the only signal: solid versus dashed is a border style and the note is words. |
| `src/components/line-editor.tsx` | **new** — §7.1 |
| `src/shell/content-header.tsx` | extended — an `actions` slot beside `cluster` (C10) |
| `src/router/routes.ts` | extended — `ownsHeader?: boolean` on `Route`; four routes flipped to `built: true` with a `load` |
| `src/shell/shell.tsx` | one line — skip the shell header when `route.ownsHeader === true` |
| `src/components/{card,table,confirm-dialog,empty-state,error-state,status-pill,figure}.tsx` | **unchanged.** Reused as they are. |

### 6.6 Pages

| File | Contents |
| ---- | -------- |
| `src/pages/rules.tsx` + `src/pages/rules/{editor-gutter,error-list,how-this-saves}.tsx` | Custom Rules |
| `src/pages/policies.tsx` + `src/pages/policies/{policy-card,default-card,policy-dialog,assignment-rows,cost-card}.tsx` | Policies |
| `src/pages/clients.tsx` + `src/pages/clients/{client-row,assign-dialog,rename-field,legend-card}.tsx` | Clients |
| `src/pages/rule-tester.tsx` + `src/pages/rule-tester/{query-form,result-card,session-ring}.tsx` | Rule Tester |

**No chart on any of these four pages.** The Policies traffic bar and the
Clients share bar are plain `div`s, the same treatment `p5-06` used for
`FrequencyBar` and the upstream bar. Nothing here imports `charts/` or
`components/chart.tsx`, so no route in this task pulls the uPlot chunk — V18
asserts it rather than assuming it.

---

## 7. The three pieces of real engineering

### 7.1 The line editor, and anchoring a `422` to its line

**No editor dependency is added.** A `<textarea>` with a synchronised gutter,
which is the whole of it:

```text
.editor            position: relative, monospace, fixed line-height
  .gutter          absolutely positioned, line numbers, aria-hidden
  textarea         transparent background, white-space: pre, overflow: auto
  .band            absolutely positioned, one per bad line, behind the textarea
  .callout         absolutely positioned, one per bad line, above the textarea
onScroll(textarea) → gutter.scrollTop = band/callout offset = textarea.scrollTop
```

One pure function does all the positioning and is unit-tested on its own:

```text
lineTop(lineNumber, lineHeightPx, scrollTopPx) = (lineNumber - 1) * lineHeightPx - scrollTopPx
```

**Anchoring — primary and fallback, decided by measurement, not preference.**
The artboard draws the message *between* lines 7 and 8. A textarea cannot host
content between its own lines without changing the document.

- **Primary:** the callout floats immediately below the bad line's baseline,
  overlaying the line beneath it. Visually inline, no dependency, no document
  change.
- **Fallback, if occlusion measures badly:** the callout collapses to a marker
  and the message moves to the anchored error list under the editor.

Either way the error list under the editor exists and each entry is a
**button that focuses the textarea and selects that line's range** — which is
what "anchors to its line" functionally requires, and the only thing that works
at 390 px. Which shipped is recorded in the review file, the shape `p5-06` used
for `disp.fill`.

**Parsing the `422`.** The message is one string, joined with `; `
(`routes.rs:1225-1247`):

```text
line 7: invalid rule syntax: "@@||^"; line 12: invalid rule syntax: "|||"; and 3 more invalid line(s)
```

The parser regex-scans for `line (\d+): invalid rule syntax: ` and takes the
span up to the next match as the offending content — it does **not** split on
`; `, because the quoted rule text can contain one. It separately matches
`and (\d+) more invalid line\(s\)` and renders that as a stated truncation. The
server caps reported lines at **100** (`rule_list.rs:12`), so the list is
bounded by construction.

**The parser is total.** A message it cannot parse yields zero anchors and the
raw message in the banner. **It never fabricates a line number.**

**The callout renders the parsed message and nothing more** (X5). The
artboard appends a diagnosis — "the exception has no domain between `||` and
`^`" — that the `422` envelope cannot supply; inventing one would put text on
screen no API field backs, phase constraint 1. The banner and the callouts
carry the API's own words.

**Text preservation is an invariant, not a behaviour.** The editor's state is
written from a response only on a `2xx`. Every non-2xx path — `422`, `500`,
`NetworkError`, an abort — leaves the buffer byte-identical, including trailing
whitespace and blank lines. Tested (V1).

### 7.2 Solid versus dashed, at both widths

`Clients.dc.html` and `MobileClients.dc.html` agree: solid chip = an assignment
naming this address; dashed = inherited. `visual-system.md` §Accessibility
requires colour never carry meaning alone, and `p5-06`'s F17/N4 showed a tint
that reads in one theme and vanishes in the other.

So the distinction is carried three ways, none of them hue: **border style**
(solid / dashed), **weight**, and **the note in words** beside it. The dashed
chip's surface is a token, never a `color-mix(… , transparent)` wash.

### 7.3 The classification procedure — R1's ordering, in full

Inputs: one `client` from `/clients`; the `/policies` response. No clock.

**The chip style is the API's, not the procedure's.** Solid ⇔
`client.assignment_source === "direct"`, dashed ⇔ absent — D2, and the one
signal Rust computes itself. The procedure's `direct` lookup exists only to
*locate* that assignment for its owner and schedule text, and it uses the same
comparison Rust uses — **raw string equality** on `assignment.client`, not
parsed-selector equality (`direct_assignment` and `PolicyResolver` both compare
`assignment.client == ip.to_string()`, `routes.rs:1050-1056`, `1083-1085`).
Same comparison ⇒ the two can disagree only when the two responses describe
different instants; each disagreement shape below claims nothing.

```text
P        = client.policy                       // in force now; "default" when nothing applies
owner(a) = the /policies item whose assignments[] holds a
direct   = the first assignment, in /policies item order then array order, whose
           raw client string === client.ip — string equality, mirroring Rust
           (the API keeps at most one — routes.rs:985-1000 — but a hand-edited
            TOML can hold several, and `direct_assignment` in Rust takes the
            first, so this takes the first too)
named(P) = every assignment whose selector parses as a Name matching
           client.name (ASCII-case-insensitive) and whose owner is P

0. assignment_source === "direct" but direct lookup finds nothing
     SOLID chip P, no note                     // responses from different
                                               // instants; claim nothing
   assignment_source absent → branches 3/4, and any string-equal assignment
   the lookup does find is ignored             // same disagreement, same silence

1. direct exists, owner(direct).id === P
     SOLID chip P
     note: <schedule text of direct> · in force

2. direct exists, owner(direct).id !== P            // the direct assignment is not deciding
   2a. named(P) has at least one schedule-less member
         SOLID chip owner(direct).id
         note: not in force — the name assignment on "<name>" decides (<P>)
   2a'. named(P) is non-empty but every member carries a schedule
         SOLID chip owner(direct).id
         note: not in force — <P> in force     // no attribution: a scheduled
                                               // name assignment cannot be
                                               // proven open without a clock
   2b. named(P) is empty and direct has days or a start/end window
         SOLID chip owner(direct).id
         note: <schedule text of direct> · window shut now — <P> in force
   2c. else
         SOLID chip owner(direct).id
         note: not in force — <P> in force     // no reason claimed

3. no direct, P === "default"
     DASHED chip "default"
     note: inherited · no assignment

4. no direct, P !== "default"
     candidates = assignments matching (client.ip, client.name) whose owner is P
     exactly one  → DASHED chip P, note: via <cidr> for a subnet selector,
                    via name <n> for a name selector (D2's two spellings)
     otherwise    → DASHED chip P, note: inherited
```

**Why the `named(P)` test precedes the schedule test.** A `Name` selector
outranks an `Ip` one even after expansion (`fah-rules/src/policy.rs`, test
`a_resolved_name_keeps_its_specificity_over_a_host_assignment`), so an open
direct window can still lose. Testing the schedule first would print
"window shut now" over an assignment whose window is wide open — R1.

**Why 2a demands a schedule-less witness.** A schedule-less name assignment on
`P` is provably active and provably outranks any open direct window, so the
attribution is proven. A *scheduled* one proves nothing without a clock: it may
be open and deciding, or shut while a subnet assignment lands the client on the
same `P` — attributing it would claim what the responses cannot show (D3).
2a' therefore states only what `P` itself already proves.

**Why 2b is sound once `named(P)` is empty.** If the direct window were open,
only a more specific selector could outrank it — a `Name`. An open name
assignment deciding would make its owner `P`, so `named(P)` would be non-empty.
Empty `named(P)` rules that out: a direct assignment that carries a schedule,
has no name route to `P`, and is not deciding, has a shut window.

**Why 2c and branch 0 make no claim.** A scheduleless direct assignment with no
name route to `P` should be deciding. If it is not — or if `assignment_source`
and the lookup disagree — the two responses disagree: a mutation landed between
them, or a hand-edited config names a policy the compile rejected. Saying
nothing is the correct output; guessing is not.

**Branch-2 notes always name `P`** (X4): the column is titled "Policy in
force", so a row whose chip shows a not-in-force assignment states what *is* in
force in the same breath. The artboards stop at `window shut now`; the words
are extended, never replaced.

### 7.4 Schedule text

One formatter, used by Clients, Policies and the Rule Tester so the three
cannot word it differently.

| `days` | `start`/`end` | Rendered |
| ------ | ------------- | -------- |
| absent | absent | `no schedule — always` |
| absent | set | `daily · 21:00 → 07:00` |
| set | absent | `mon–fri · all day` |
| set | set | `mon–fri · 21:00 → 07:00` |

`days` is echoed as configured with `-` rendered as an en dash for display
only; the value sent back on a `PATCH` is the string the API gave.

---

## 8. Page by page

Order, wording and placement are the artboards'. Sources are the API's.

### 8.1 Custom Rules (`/rules`)

| Zone | Artboard | Source |
| ---- | -------- | ------ |
| Header | title, the context line, `Discard` + `Validate and save` | static copy; `ownsHeader` (C10) |
| Failure banner | "Not saved — one line failed validation…" | rendered only after a `422`; text is the artboard's, with the count from the parsed message |
| `rules.txt` card | line editor; secondary reads `14 lines · 1 invalid` | `GET /rules/user` → `rules`; T1, T2 |
| How this page saves | four paragraphs | static copy, **with C6's replacement sentence** |
| Precedence | three ranked rows plus the Rule Tester pointer | static copy (RULE_ENGINE.md §Verdicts) |

`Discard` is enabled only while the buffer differs from the last fetched
document, and asks for confirmation before throwing away typed text.

### 8.2 Policies (`/policies`)

| Zone | Artboard | Source |
| ---- | -------- | ------ |
| Header | title, context, `New policy` | `ownsHeader`; the button is disabled at the ceiling (T5) |
| Summary card | `3 / 16` · `2` · `4` · `EET-2EEST` | T4, `active_assignments` verbatim, T6, T7 |
| Default card | `RESERVED`, "every enabled list", the two-sentence explanation, traffic | synthetic (C9); traffic from `/stats.policies` |
| Policy card ×N | name + mono id, `Edit`, list chips, assignment rows, traffic bar | `/policies.items[]` verbatim; traffic T8 |
| Empty slot card | `13 policy slots left`, `New policy` | T5 |
| What costs what | `RECOMPILES` / `LIVE` columns and the footnote | static copy; it is the contract §10 implements. **Except the rule count:** the artboard's `752,585` is a drawing (phase constraint 8) — the sentence renders `compiled_rules` from the `GET /lists` response this page already fetches, read verbatim (§8.5) |

Assignment rows render `client` (mono), the client's name where `/clients` is
**not** read — it is not read on this page, so no name is shown — and the
schedule text. The artboard prints `tv` beside `192.168.10.50`; that name is
only in `GET /clients`, which this page does not fetch. **Deviation X3:** the
assignment row prints the selector as configured and no resolved name. Adding
`/clients` here to print one label is not worth a third request; the Clients
page is one click away and is where names live.

### 8.3 Clients (`/clients`)

Desktop columns exactly as `Clients.dc.html`: Address · Name · Policy in force ·
Queries 24 h · Blocked · Blocked share · Last seen · actions.

**One grid for header and rows** — `p5-06`'s F1 was two grids given the same
template, and they drifted. The actions cell is a **44 × 44 glyph** (`edit`),
not a label, so its track cannot vary by row state — F1 closed by construction,
as the fourth fix pass established.

The `⋯` the artboard draws expands the row's two actions in place. On desktop
that is an inline edit region beneath the row; at < 768 px it is
`MobileClients.dc.html`'s expanded card, verbatim: `Rename` and
`Change policy`, 44 px, side by side. **One DOM, one open-row state**, the
`p5-06` pattern.

The search box filters client-side over `ip` and `name`
(`visual-system.md` §Tables). No request.

The two footnote cards are rendered verbatim, with C2's reworded sentence about
what the API does and does not say, and the "observed by traffic" card intact —
no ARP, no DHCP, no inventory, no reverse lookups.

### 8.4 Rule Tester (`/rule-tester`)

| Zone | Source |
| ---- | ------ |
| Domain | free text, trimmed and lowercased client-side to match `routes.rs:1251` |
| Query type | `A` `AAAA` `HTTPS` `PTR` `TXT` as the artboard draws. `parse_qtype` (`wire.rs:261-267`) maps anything but the first two to `Other(name)`, so all five are valid and the chips are not an invented enum |
| Decide as | `a client` / `a policy`, the artboard's two chips |
| client mode | address, or a name resolved per **D5** |
| policy mode | a picker over `/policies.items[].id` plus `default`, which `routes.rs:1281` accepts explicitly |
| Result — verdict | `verdict` verbatim → `BLOCK` / `ALLOW` / `PASS` pill |
| Result — matching rule | `rule` verbatim; `null` on a pass → `no rule matched` |
| Result — from list | `list` verbatim; `"user-rules"` renders as **your custom rules** (`lifecycle/mod.rs:43`), which is what the artboard prints |
| Result — deciding policy | `policy` verbatim |
| **why that policy** | derived, T13 |
| Previous tests | a **bounded** client-side ring, 10 entries, session-only, dies with the page (phase constraint 7) |

**The "why" sentence, by mode:**

| Mode | Sentence | Built from |
| ---- | -------- | ---------- |
| policy | `you chose this policy — assignments are ignored` | the request itself |
| client, address, direct assignment in force | `assignment on this address · <schedule> · in force now` | §7.3 branch 1 |
| client, address, direct assignment overridden by a name | `the assignment on this address is not deciding — the name assignment on "<name>" decides (<P>)` | §7.3 branch 2a |
| client, address, direct assignment's window shut | `assignment on this address · <schedule> · window shut — <P> in force`; when `P !== "default"`, branch 4's candidate lookup run for `P` appends `via <selector>` if it yields exactly one | §7.3 branch 2b + branch 4 |
| client, address, direct not deciding, unprovable why | `not in force — <P> in force`, and no reason claimed | §7.3 branch 2a′ / 2c / 0 |
| client, address, inherited | `via <selector>` or `inherited` | §7.3 branch 4 |
| client, address, nothing assigned | `no assignment covers this address` | §7.3 branch 3 |
| client, name resolved to an address | as above, plus `tested as <ip> (<name>)` | D5 |
| client, name **not** observed | the partial banner — no policy claim at all | D5 |

The frontend runs **no** matching of its own against domains or rules. The
verdict, the rule and the list come from the engine; only the *explanation of
which policy applied* is assembled locally, from the same two responses Clients
uses.

### 8.5 Every derived display value on all four pages

Phase 5 standing constraint 1: no figure the API does not support. A
**derivation** is a stated arithmetic or textual function of documented fields.
**This table is complete. Anything not on it is read from a field verbatim, and
nothing else may be derived.** `p5-06`'s R-table is not extended; this task's
rows are `T*`.

| # | Display | Formula | Fields | Where |
| - | ------- | ------- | ------ | ----- |
| T1 | `14 lines` | `rules.length` | `GET /rules/user` | Custom Rules card secondary |
| T2 | `1 invalid` | count of distinct line numbers parsed from the `422` message, plus the `and N more` remainder when present | error envelope `message` | Custom Rules secondary + banner |
| T3 | `N duplicate line(s) removed` | `sent.length − response.rules.length`, rendered only when `> 0` | `PUT /rules/user` | Custom Rules, after a save (C12) |
| T4 | `3 / 16` | `items.length + 1` over the constant `16` — the ceiling **includes** the implicit default (`fah-config/src/lib.rs:245-249, 263`) | `GET /policies` | Policies summary |
| T5 | `13 policy slots left` | `16 − (items.length + 1)` | same | Policies empty-slot card, and the `New policy` disabled state |
| T6 | `4 assignments configured` | `Σ items[].assignments.length` | same | Policies summary |
| T7 | `EET-2EEST` | `timezone` up to the first `,`; the full string is the `title` | same | Policies summary |
| T8 | policy traffic bar | `blocked / queries`; `queries === 0` draws an empty track and no division — the artboard's own arithmetic (`3,204 / 8,118 = 39.5 %`) | `/stats.policies[]` matched on `policy === id` | Policies cards |
| T9 | client blocked-share bar and `%` | `blocked_24h / queries_24h × 100`, and **the bar's width is that percentage** — not normalised against the table (C3). `queries_24h === 0` draws an empty track and `0 %` with no division — T8's guard, same rule; a listed client can age to zero in the window | `/clients.items[]` | Clients rows, both widths |
| T10 | `unnamed` | `name === null` | `/clients` | Clients rows |
| T11 | `2 s ago` / `2 h ago` | formatting of `last_seen` against the render's own `now`; **no ticker** (§5.5) | `/clients` | Clients rows |
| T12 | solid ⁄ dashed chip and its note | §7.3, over `/clients` + `/policies` | `policy`, `assignment_source`, `items[].assignments[]` | Clients rows, Rule Tester |
| T13 | `why that policy` | §8.4, over the same two responses | same | Rule Tester result |
| T14 | schedule text | §7.4, a formatting of `days` / `start` / `end` | `/policies` | Clients, Policies, Rule Tester |

Explicitly **read verbatim, never derived**: `active_assignments`, `policy`,
`assignment_source`, `timezone` (the full value), every `lists[]` entry,
`blocking_mode`, `queries_24h`, `blocked_24h`, `first_seen`, `last_seen`,
`verdict`, `rule`, `list`, `compiled_rules` (the What-costs-what sentence,
§8.2), and every `stats.policies[].queries` / `.blocked`.

**Never derived at all on these pages:** a per-assignment or per-policy
in-force flag (D4/C5), any evaluation of a schedule window against a clock
(D3), any verdict, rule match or list attribution computed in the browser, any
client-side ranking of policies, and any figure combining `/stats.policies`
with the Dashboard's rolling counters.

---

## 9. Phone

### 9.1 Clients at 390 px — build to `MobileClients.dc.html`

The artboard is the specification. What it settles, and what implements it:

| Artboard | Implementation |
| -------- | -------------- |
| Search field, full width, above the list | the same input the desktop card title bar holds, relocated by CSS `order` |
| One card per client: name + address, chip row, one stats row, last-seen | the desktop row's grid re-laid below 768 px. **One DOM**, the `list-row.tsx` pattern |
| Solid-versus-dashed surviving at card size | §7.2 — border style plus words, so it survives without hue |
| A shut window called out **in words** on the card | §7.3 branch 2b, the same string at both widths — extended past the artboard to name the in-force policy (X4) |
| Tapping a card expands its two actions **in place** | one `openIp` state on the page; the expanded region is the same markup the desktop row expands to |
| `Rename` and `Change policy`, 44 px, one round trip each | `PUT /clients/{ip}` and `PUT /clients/{ip}/policy`. Each mutation is one request; the reread that follows (§5.3) is not part of the interaction |
| Blocked % coloured red beside the bar | T9, with the figure in text so colour carries nothing alone |
| Two footnote cards at the bottom | verbatim |
| Top bar shows `live` | **X1** — renders `not needed here` (C4) |

### 9.2 The other three, at 390 px

No artboards; the task states the rules and they are the acceptance criteria.

- **Custom Rules** — the gutter stays, narrowed to fit a 3-digit number, and
  the error anchoring stays. The two side cards stack under the editor. The
  editor scrolls inside itself; the page body does not scroll sideways.
- **Policies** — cards stack one per row **with their assignment rows intact**.
  The summary card goes two-up. The `What costs what` columns stack.
- **Rule Tester** — a single-column form with the result card **directly
  beneath it**, so a phone sees the answer without scrolling past the form it
  just filled in.

**No second component tree anywhere in this task**, and no viewport listener.
Where an artboard genuinely demands different content, it is declared as a
deviation, not implemented as a branch. The registry in §3 (X1–X5) is the
complete list.

### 9.3 Breakpoints

`visual-system.md` §Responsive, unchanged: ≥ 1200 px full grid; 768–1199 px
halves go full width and the sidebar collapses to icons; < 768 px single
column, drawer, tables scroll inside their own container. All four pages are
checked at 1400, 1247, 1200, 900 and 390 px in both themes — the widths
`p5-06`'s F16 proved matter.

---

## 10. Mutation and error semantics

### 10.1 The recompile boundary — what is confirmed and what is not

Directly from `routes.rs`, not from the artboard:

| Operation | Recompiles | Confirmed | Evidence |
| --------- | ---------- | --------- | -------- |
| `POST /policies` | **yes** | **yes** | `create_policy` → `Recompile::Yes` (`routes.rs:906`) |
| `PATCH /policies/{id}` changing `lists` | **yes** | **yes** | `patch_policy` sets `Recompile::Yes` **only** when `target.lists != lists` (`routes.rs:929-934`) |
| `PATCH` changing `name`, `blocking_mode` or `assignments` | no | no | same handler, `Recompile::No` |
| `DELETE /policies/{id}` | **yes** | **yes** | `delete_policy` → `Recompile::Yes` (`routes.rs:966`) |
| `PUT /clients/{ip}/policy` | no | no | `set_client_policy` → `Recompile::No` (`routes.rs:1002`) |
| `DELETE /clients/{ip}/policy` | no | no | `clear_client_policy` → `Recompile::No` |
| `PUT /clients/{ip}` (rename) | no | no | republishes the snapshot only (`routes.rs:420`) |
| `PUT /rules/user` | **yes** | **no** — D6 | `set_user_rules` compiles inline (`lifecycle/mod.rs:948-949`) |

**The difference is stated in the UI, not only enforced.** A confirming dialog
names the cost in the API's own terms — seconds of CPU on the RB5009, because
per-rule policy masks are built at compile time — and a non-confirming action
carries the `LIVE` treatment the `What costs what` card explains.

**A `PATCH` that touches only `assignments` must not send `lists`.** Sending
the unchanged subset back would still compare equal and still not recompile,
but it makes an assignment edit indistinguishable from a subset edit in the
request log. Only changed fields are sent.

### 10.2 Blocking, per R2

Every row marked **recompiles** above holds its HTTP request open for the whole
compile. Those four operations — three on Policies, and `PUT /rules/user` —
enter a blocking busy state that says what is happening, shows no faked
progress, and refuses a second mutation while one is in flight. This is
`p5-06`'s Refresh-all treatment, and it is the only correct one: an optimistic
update here would show a policy as created seconds before the engine can
decide anything under it.

**The busy state is a modal, and it blocks in-app navigation** — a recompiling
request is never aborted (R2). Cancelling one mid-compile can leave persisted
state ahead of the live matcher and the client ignorant of whether the write
landed, so the request always runs to its response. Unmount-abort in §5 covers
entry one-shot reads; live mutations that do get aborted still land server-side
and the next entry re-reads.

### 10.3 Form-enforced limits

- **The 16-policy ceiling.** `New policy` is disabled, with the reason
  rendered, once `items.length + 1 === 16`. The API's `422` from
  `validate_policies` (`lib.rs:263-273`) is still handled — the form is a
  courtesy, the server is the guarantee.
- **`default` is reserved.** Not offerable as an id (client-side, mirroring
  `routes.rs:1146-1150`), and the Default card carries no Edit and no Delete.
- **One assignment per address.** The assign dialog states that saving replaces
  whatever assignment this address already had, **in whichever policy held it**
  — which is what `set_client_policy` does before it adds.
- **`start` and `end` together or neither.** Enforced in the form and re-stated
  by the API's own message.

### 10.4 Errors

| Status | Rendering |
| ------ | --------- |
| `422` on `PUT /rules/user` | per-line anchors (§7.1); the document is **not** written and the buffer is untouched |
| `422` elsewhere | anchored to its field, with the API's own message |
| `409` on `POST /policies` | `policy <id> already exists` — the API's message, with the id field kept and focused |
| `404` on `PUT /clients/{ip}/policy` | the named policy does not exist — reread `/policies`, because the page's copy is stale |
| `404` on `DELETE /clients/{ip}/policy` | nothing was assigned; the row is already correct, so the reread settles it silently |
| `404` on `PUT /clients/{ip}` | `no client seen at <ip>` — the client aged out of the registry between the read and the write |
| `500` | reported as the API states it; the mutation did not happen |
| `NetworkError` | distinguished from a server refusal, per `core.ts` |
| `401` | never handled here — `core.ts`'s guard owns it |

An empty `/clients` renders `EmptyState` — "no client has asked anything yet" —
not an error. An empty `/policies` renders the Default card alone plus the empty
slot card, which is the zero-config truth.

---

## 11. Work units

Dependency-ordered. Each ends with `npm run typecheck`, `npm run test` and
`npm run build` green before the next begins — the `p5-05`/`p5-06` discipline.

| # | Unit | Files | Ends when |
| - | ---- | ----- | --------- |
| **W1** | API types and accessors | `api/{policies,rules,clients,types,index}.ts` | typecheck green; tests pin the `PATCH` double-option encoding (`lists: null` emitted, not dropped), the `204`/`202` bodiless paths, and every path builder's `encodeURIComponent` |
| **W2** | `policy/selectors.ts` | new + `selectors.test.ts` | parse and match ported case-for-case from `fah-model/src/policy.rs` and `fah-rules/src/policy.rs`: v4/v6, mixed family never matches, `/0` and `/32` and `/128`, a malformed address before a `/` is not a name, ASCII case-fold on names, an unnamed client never matches a `Name` |
| **W3** | `policy/assignment.ts` — **R1's ordering, as a pure function** | new + `assignment.test.ts` | every §7.3 branch covered, **including 2a firing before 2b (R1)** on an open window overridden by a schedule-less name assignment, **2a′ refusing attribution** when every `named(P)` member is scheduled, 2c making no claim, and **both branch-0 disagreement shapes** (`assignment_source` present with no string-equal assignment, and the reverse) claiming nothing. The `direct` lookup is string equality, pinned by a non-canonical-spelling case (`"192.168.010.5"` does not match). Schedule text (§7.4) covered at all four `days`/window combinations |
| **W4** | `policy/validation.ts` | new + `validation.test.ts` | `parseUserRulesError` against the exact string `routes.rs:1225-1247` builds, including a quoted rule containing `; `, the `and N more` tail, the 100-line cap, and an unparseable message yielding **zero** anchors. `parseDays` matches `schema/policy.rs`'s own test vectors including `fri-mon` and `Monday, Wednesday` |
| **W5** | Header seam + chip | `router/routes.ts`, `shell/shell.tsx`, `shell/content-header.tsx`, `components/policy-chip.tsx`, `pages/dev-gallery.tsx` | `ownsHeader` honoured; Dashboard and Lists render byte-identically to before; the gallery draws a solid and a dashed chip in both themes |
| **W6** | Custom Rules — document, editor, **and the blocking save (R2)** | `pages/rules.tsx`, `pages/rules/*`, `components/line-editor.tsx`, `styles/components.css` | route `built: true`; the document round-trips; `lineTop` unit-tested; the gutter tracks the textarea's scroll; **the save enters a blocking busy modal naming the rebuild, disables the editor and `Discard`, blocks navigation while in flight, is never aborted, and starts no second `PUT`** |
| **W7** | Custom Rules — failure path | same | a `422` anchors every message, writes nothing, and leaves the buffer byte-identical; the banner and the error list both render; the primary-or-fallback callout decision is measured and recorded |
| **W8** | Clients — read path, wired to **R1's ordering** | `pages/clients.tsx`, `pages/clients/*` | route `built: true`; **two** requests on mount and no third; one grid for header and rows; **every §7.3 branch reachable against a live API, 2a staged before 2b on a real open window overridden by a schedule-less name assignment**; search filters without a request |
| **W9** | Clients — mutations, **all three live (R2's other half)** | same | rename, assign with and without a schedule, clear; **none raises a confirmation and none blocks**, and each is timed beside a Policies `lists` change so the millisecond/second contrast is a recorded figure; both re-reads fire in parallel on success; every §10.4 row exercised |
| **W10** | Policies — read path | `pages/policies.tsx`, `pages/policies/*` | three requests on mount; summary card figures all trace to §8.5; synthetic Default card; traffic matched by id with a `0/0` fallback |
| **W11** | Policies — mutations, **the recompile boundary (R2)** | same | create / edit / delete with the §10.1 confirmation boundaries and the §10.2 **blocking modal** states — navigation blocked, no abort, no second mutation; a `PATCH` touching only `assignments` sends no `lists` and neither confirms nor blocks; the ceiling and the reserved id enforced in the form and the API's `422`/`409` still rendered |
| **W12** | Rule Tester, incl. **R3's duplicate-name branch** | `pages/rule-tester.tsx`, `pages/rule-tester/*` | both modes; all four API fields plus the derived "why"; D5's three name branches with the ASCII fold — one match substitutes **visibly** (`TV` finds `tv`), **two or more block on a disambiguation prompt with no request sent (R3)**, none renders the partial banner; the branch-2 "why" rows (§8.4) rendered; the session ring bounded at 10 |
| **W13** | Phone pass | `styles/{components,layout}.css` and the four pages | Clients matches `MobileClients.dc.html`; the other three meet §9.2; no second component tree; no viewport listener |
| **W14** | Verification (§12) and the review file | `docs/code-review/phase5/p5-07-filtering-pages-review.md` | every V item recorded, including the ones that could not be produced |

W2 and W3 are deliberately ahead of every page: the classification is the thing
most likely to be subtly wrong, and it is a pure function that can be proven
before anything renders it.

---

## 12. Verification

Frontend gates (`npm run typecheck`, `npm run test`, `npm run build`) plus the
workspace gates (`cargo fmt --check`, `cargo clippy --workspace --all-targets
-- -D warnings`, `cargo test --workspace`). Then, against a **running**
container, read off a request log, a WebSocket frame log and the DOM.

| # | Check |
| - | ----- |
| **V1** | A `422` on the rules document anchors every message to the right line, and the buffer is **byte-identical** afterwards — compared as a string, including trailing whitespace and blank lines, not eyeballed |
| **V1a** | Nothing is written on a validation failure: `GET /rules/user` after the failed `PUT` returns the pre-edit document |
| **V1b** | An unparseable `422` message renders the raw text with **zero** line anchors and no fabricated number (forced by stubbing the envelope) |
| **V1c** | A successful save with a duplicated rule line reports `N duplicate line(s) removed` and shows the returned document (C12/T3) |
| **V2** | **Zero WebSocket subscription on all four routes.** `fahUnion()` reads `[]`; `fahSocketState()` reads `closed`; no `subscribe` frame is sent on entering any of them |
| **V2a** | **Server-side**, not only in the browser: with the browser parked on each of the four routes in turn, no established connection to the API port remains from that browser, **counted at the host serving the API**. On the dev box that is the Docker host's view of the published port — `ss -tn state established '( sport = :8443 )'`, or its conntrack table where the port is DNATed (the runtime image is distroless and carries no `ss`, so the host stands in for the container). On the RB5009 the read-only equivalent is `/ip firewall connection print where dst-port=8443`. A browser-host `ss` is supporting evidence only — it is the client side, not what the task's criterion names. If no server-host view is available in the run, the row is recorded **NOT RUN** with the reason and the unmet criterion is put to the owner — never inferred from the browser alone |
| **V2b** | `registry.activeTimers()` reads **0** on each of the four routes, and `REFRESH_ENDPOINTS` is still exactly `['health','telemetry','cache','clients','lists']` — the `p5-06` pin, unmoved |
| **V2c** | No `setInterval`/`setTimeout` outside `lifecycle/timers.ts`: asserted by a source grep in the test suite, and by `ageTickerRunning()` reading `false` on all four routes |
| **V3** | Route entry/exit: Dashboard → each of the four → Dashboard. The socket closes on entry and reopens on return; the Dashboard's five timers are cleared on leaving and re-established on return; no request is attributable to an inactive route |
| **V4** | **Loading Clients issues exactly two requests** — one `GET /clients`, one `GET /policies` — counted from a request log, not inspected. Staged against an inventory of **at least 20 observed clients**, because an N+1 is invisible at three |
| **V5** | **No per-row `GET /clients/{ip}/policy`, ever.** Asserted by filtering the request log for **`GET` requests** to that path over a full page session including every mutation: **zero**. The page's own `PUT`/`DELETE` assignment mutations hit the same path and are legitimate — a method-blind filter would fail this row on any session that assigns a policy |
| **V6** | Direct versus inherited: a direct assignment renders solid with its schedule; a subnet-inherited client renders dashed with `via <cidr>`; a name-inherited client renders dashed with `via name <n>`; an unassigned client renders dashed `default · inherited · no assignment`. All four staged for real by editing assignments through the API |
| **V6a** | The ambiguous case renders the bare word `inherited` and names no selector — staged with two assignments in one policy that both match one client |
| **V7** | **Closed schedule window**: a client with a direct assignment whose window is shut shows that policy, solid, marked `window shut now — <P> in force` (X4), while `GET /clients` reports that different `policy` for it |
| **V7a** | **R1's ordering, staged**: a direct assignment with an **open** window, overridden by a **schedule-less** name assignment naming another policy, renders 2a's sentence and **not** `window shut now` |
| **V7b** | **2a′, staged**: the same shape with the name assignment **scheduled** renders `not in force — <P> in force` and attributes nothing — no name claim, no shut-window claim |
| **V8** | Policy recompile confirmations: create, a `lists` change, and delete each raise a confirmation naming the cost; each then **blocks** with a stated busy modal until the response lands (R2), a second mutation cannot be started meanwhile, and **attempting to navigate during the busy state neither cancels the request nor changes the route** |
| **V8a** | **R2, the live half.** Assignment changes apply without a recompile and without a confirmation: rename a policy, change its assignments, and assign a client — three mutations, no dialog, and each returns in milliseconds against the seconds a `lists` change takes. Both figures recorded, measured from the response, not estimated |
| **V8b** | **R2 on Custom Rules**, which has no dialog to hang the wait on: `PUT /rules/user` is timed and its duration compared against the `lists`-change figure from V8 — both are a full compile and should be the same order. During it the button shows a busy state naming the rebuild, the editor and `Discard` are disabled, navigation is blocked and the request is not aborted, and a second `PUT` cannot be started. **No optimistic "saved" state appears before the response** |
| **V9** | Default policy protection: no Edit, no Delete on the Default card; `default` is refused as an id in the create form; the API's `422` is still rendered when the form is bypassed |
| **V10** | The 16-policy ceiling: with 15 configured policies, `New policy` is disabled with its reason shown and the slot card reads `0 policy slots left`; a bypassed form gets the API's `422` and renders it |
| **V11** | Schedule and timezone semantics: `active_assignments` is rendered verbatim and observed to **change across a schedule boundary without a page action other than a re-entry** (the server ticks it every 20 s — `main.rs:38`). The timezone is rendered truncated with the full POSIX string as `title`. **No date arithmetic exists in the bundle** — asserted by a source grep for `Date`/`toLocale`/`getTimezoneOffset` inside `src/policy/` |
| **V11a** | **R3 on Policies.** A `Name` assignment matching **two** addresses is staged; `active_assignments` is observed to count **two**, above the one configured assignment row. The label reads `assignments in force right now` and nowhere reads "of N configured" — the two figures are genuinely different numbers and the UI must not equate them |
| **V12** | Rule Tester, **client mode**: an address returns and renders all four API fields plus the derived "why". Verified for a `block` (rule and list populated) and a `pass` (both `null`, rendered as `no rule matched`) |
| **V12a** | Rule Tester, **policy mode**: a named policy and `default` both return and render all four fields; the "why" reads `you chose this policy — assignments are ignored` |
| **V12b** | D5's three name branches: one match substitutes the address **visibly**; two matches block on a disambiguation prompt; no match renders the partial banner and does **not** present `policy default` as an answer |
| **V13** | Every rendered figure on all four pages traced to a field or to a §8.5 `T` row, in a table in the review file. **No derivation exists that §8.5 does not list.** No per-assignment in-force claim anywhere (D4) |
| **V14** | Both themes at 1400 / 1247 / 1200 / 900 / 390 px: `scrollWidth === clientWidth` on the page body at every one. The Clients table scrolls inside its own container where it must, and its measured minimum width is recorded the way `p5-06` recorded 1247 px |
| **V15** | Every interactive control ≥ 44 px on both axes at 390 px, measured from the DOM — the glyph action, the chips, the expanded card actions, the editor's buttons, every dialog control |
| **V16** | Clients at 390 px matches `MobileClients.dc.html` in structure; tapping a card expands its two actions in place and pushes no screen |
| **V17** | Custom Rules at 390 px keeps a usable gutter and its error anchoring; Policies cards stack with assignment rows intact; the Rule Tester result sits directly beneath its form |
| **V18** | Bundle: gzip **and** brotli per asset and total against 153,600 B. Four new lazy chunks. **No chunk in this task references `uplot`** — asserted, not assumed |
| **V19** | Memory: 5 rounds over all four pages, heap flat — the `p5-05` V11 method. The Rule Tester ring is proven bounded at 10 by running 40 tests |
| **V20** | Gates green: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the three frontend gates. **No Rust source is expected to change**, so the cargo counts should match `p5-06`'s 1,195 |

---

## 13. Repository documents this task proposes to change

**Listing is not permission** (phase CLAUDE.md, root CLAUDE.md §Working
agreement 1). Each is proposed here and waits for the owner's yes at the point
the task reaches it.

| Document | Proposed edit | Why |
| -------- | ------------- | --- |
| `API.md` §`POST /api/v1/rules/test` | correct the `client` row: an address selects the policy in force; **a name satisfies `$client` rules and does not select a policy** | C7 — `routes.rs:1266-1276` does not do what the table claims, and the dashboard is now the second reader of that sentence |
| `API.md` §`PUT /api/v1/rules/user` | state that the `422` message is `line N: invalid rule syntax: "…"` joined by `; `, capped at 100 entries with an `and N more invalid line(s)` tail | §7.1 — the dashboard parses that string to anchor errors, so its format is load-bearing and a change to it silently degrades anchoring |
| `API.md` §`PUT /api/v1/rules/user` | state that exact-duplicate rule lines are dropped and the response is authoritative | C12 — `routes.rs:1186-1197` does it and nothing documents it |
| `docs/dashboard/sketch/Clients.dc.html` | reword the footnote: the API says whether an assignment names **this address**; subnet versus name is inferred from `/policies` and is only named when unambiguous | C2 — the drawn sentence claims more than the response carries |
| `docs/dashboard/sketch/CustomRules.dc.html` | replace "No restart, no recompile of the lists" with **"No restart and no list refetch. The ruleset is rebuilt and atomically swapped."** | C6/D6 — the owner's wording |
| `docs/dashboard/sketch/CustomRules.dc.html` | drop the callout's appended diagnosis; the `422` message is the whole callout | X5 — the response cannot supply a diagnosis |
| `docs/dashboard/sketch/Clients.dc.html` + `MobileClients.dc.html` | extend the shut-window note to name the in-force policy (`window shut now — default in force`) | X4 — the "Policy in force" column must not hide it |
| `docs/dashboard/sketch/Policies.dc.html` | drop the per-assignment dot and the `ACTIVE NOW` tag | C5/D4 — the API carries no per-assignment in-force flag |
| `docs/dashboard/information-architecture.md` §Clients | same correction as the Clients footnote | C2 |

The three `information-architecture.md` edits `p5-06` proposed (C1 "area" →
bars, C2 `stats`-only subscription, C8 `GET /clients` as the Top-clients
source) are **still unapplied** and are not this task's to apply.

---

## 14. Risks

| Risk | Mitigation |
| ---- | ---------- |
| `policy/selectors.ts` drifting from the engine's matcher, so a row says `via 192.168.20.0/24` for a client the engine puts elsewhere | It is the one module that reimplements engine logic and it is treated as such: pure, ahead of every page (W2), ported case-for-case from the Rust's own tests. And the classification never renders a policy the responses do not carry: the chip style is `assignment_source`'s, branch-2 chips show the configured assignment marked not in force with the note naming `client.policy` as what is in force, and a branch that cannot prove *why* claims nothing (§7.3) |
| A recompiling request aborted mid-compile, leaving persisted config or the `/data` rules cache ahead of the live matcher | Recompiling mutations are **never aborted** (R2): the busy modal blocks in-app navigation until the response lands, so unmount cannot happen with one in flight. A dropped browser (tab close, Wi-Fi loss) mid-compile remains a server-side exposure that predates this task — it is recorded in the review file, not fixed here |
| The `422` message format changing in Rust and silently un-anchoring every error | The parser is total and falls back to the raw message with zero anchors — a visible degradation, not a wrong line number. The API.md edit (§13) records that the format is load-bearing |
| An implementer computing a schedule window in the browser "just for the dot" | D3 and D4 forbid it explicitly, §8.5 lists it under **never derived at all**, and V11 greps the bundle for date arithmetic inside `src/policy/` |
| R1's branch order getting reordered during a refactor, printing "window shut" over an open window | W3 tests the ordering directly, V7a stages it against a live API, and V7b pins 2a′'s refusal to attribute a scheduled name assignment |
| A blocking recompile read as a hung page | R2's busy state states what is happening and names the compile; the Lists header already prints a real compile duration an operator has seen |
| Two responses cross-referenced from different instants | Both are re-read together after every mutation (§5.3); branch 2c makes no claim when they disagree, and branch 0 makes none when `assignment_source` and the lookup disagree (§7.3) |
| An N+1 reappearing later because a row needs one more field | V5 filters the request log for `GET` requests to `/clients/{ip}/policy` over a whole session, mutations included, and asserts zero — the same "counted, not inspected" discipline the task demands |
| The Clients desktop table scrolling sideways, as Lists did at 900 px | One grid for header and rows, glyph actions with a fixed track, and V14 measures five widths rather than two |
| Duplicate client names silently sending the wrong address | R3 — the tester blocks on a disambiguation prompt rather than picking one |
| The line editor's floating callout occluding the line beneath it | The fallback is specified up front, decided by measurement, and the anchored error list exists either way |
| Bundle creep from four new pages | Four lazy chunks, no new dependency, no chart. V18 reports gzip and brotli and the build gate decides |

---

## 15. Out of scope

- **Runtime pages** (`p5-08`) and **Settings / Diagnostics** (`p5-09`).
- **Adding fields to `GET /clients`** — that was `p5-03`'s and is done. This
  task adds no API route, no config key and no Rust behaviour.
- **Per-rule identity, per-rule delete, per-rule toggle.** The API has none and
  offering one would promise a write that cannot happen.
- **Groups and device inventory.** No ARP, no DHCP, no reverse lookups, no
  membership matrix.
- **Retrofitting Lists' header** to the new `ownsHeader` seam (C10).
- **A server-side WebSocket connection counter.** V2a measures what is
  measurable today; a counted field would be an API change belonging to a later
  task, and is recorded as such if V2a cannot be produced.
- **Naming an assignment's client in the Policies cards** (X3) — that needs
  `/clients`, and the Clients page is one click away.
- **Any change to the shared refresh registry, its preferences or its
  constants.** `p5-06` pinned five endpoints; this task adds none.
