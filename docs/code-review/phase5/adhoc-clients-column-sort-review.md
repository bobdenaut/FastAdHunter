# Clients column sort — code review

Reviewed commit `f27237a` *feat(dashboard): sort the Clients table from its
column headers*, committed and not pushed. Ad-hoc dashboard task, no plan file.

## Summary

Four sortable headers — Address, Queries 24 h, Blocked, Blocked share — each
with its own direction toggle and its own opening direction. The implementation
is correct: no bug found in the ordering, the tie-break, the state machine or
the tests, and all three gates pass. Seven findings, none of them a correctness
defect. The two that matter are a duplicated formula the commit was written to
remove (F1) and an address key rebuilt on every comparison, which is what the
IPv6 question turns on (F2). One confirmed layout overflow (F3).

## Decisions

- Tie-break fixed ascending by address whichever way the primary key points —
  a reversed list is the same rows upside down. Correct and documented.
- Opening direction per column: address ascending, the three number columns
  descending.
- Sorting is client-side; the page still reads twice on mount and never again
  (`calls()).toHaveLength(2)` in every new test).
- Rows keyed by `client.ip`, so reordering never moves an expanded row's state.
- The default page order changed, descending by address → ascending.

## Findings

### F1 — `blockedShare` duplicates `blockedPercent` (medium)

`dashboard/frontend/src/pages/clients/share.ts:3` is the same expression as
`dashboard/frontend/src/derive.ts:39`:

```ts
export function blockedPercent(queries: number, blocked: number): number {
  return queries === 0 ? 0 : (blocked / queries) * 100;
}
```

Before the commit: `derive.blockedPercent` plus an inline copy in `ClientRow`.
After: `derive.blockedPercent` plus `share.blockedShare`. Still two copies, and
the commit message's own reason ("two copies of that formula would drift
apart") argues against the module it created. Principle 4.

Fix: delete `share.ts`; `client-row.tsx` calls `blockedPercent(client.queries_24h,
client.blocked_24h)`, and `SORT_KEYS.share` becomes
`(client) => blockedPercent(client.queries_24h, client.blocked_24h)`.

### F2 — the address key is rebuilt on every comparison (medium at IPv6 scale)

`compareAddressesAsc` calls `addressKey` on both operands, so a sort builds
~2·n·log₂n keys instead of n. The tie-break extends that cost to the three
number columns: a column whose counts are equal pays exactly what the address
column pays. The registry caps at 4096 clients
(`crates/fah-stats/src/client_registry.rs:16`), and SLAAC privacy addresses
churn through it, so 4096 rows is reachable on `family=all` or `family=v6`.

Fix: build the key once per row, in its own memo on `[items]` so a header click
does not rebuild it.

```ts
const keyed = useMemo(
  () => items.map((client) => ({ client, key: addressKey(client.ip) })),
  [items],
);
```

Also hoist `SORT_KEYS[sort.column]` out of the comparator — it is two property
lookups per comparison today.

At 4096 rows the un-virtualised render of 4096 `ClientRow`s dominates the click
either way; the sort is the part this commit can cheaply fix. On the default
`family=v4` view of a household the current code is under a millisecond and
nothing is visible.

### F3 — `Queries 24 h` plus its caret overflows the 88 px track (low–medium, confirmed)

When queries is the active column the label starts 9.6 px left of its track and
consumes the whole 10 px gutter, stopping 0.4 px from the Policy track's edge.
No overlap with Policy's text at 1400 px, because Policy's content is 106 px of
a 225 px track — crowding, not collision, on this font. A wider `system-ui`
makes it a collision, and the label jumps 9.6 px sideways on every activation.

Fix: widen the queries track 88 px → 100 px in `.clients-table`'s
`grid-template-columns`, and reserve the caret box on inactive headers so the
label never moves. `grid-tracks.test.ts` pins the column count and the 44 px
action track only, so widening one track does not break it.

### F4 — only the active column shows that it is sortable (low)

Inactive headers are visually identical to the old static labels. Hover colour
is the only hint, and touch has no hover. A dimmed caret on every sortable
header — the box F3 already wants reserved — shows the set at a glance.

### F5 — no sort control below 768 px (low)

`components.css` `@media (max-width: 767px) { .client-head { display: none } }`.
The header row is the only control, so a phone gets the last desktop state, or
address-ascending on a fresh load. The new CSS comment — "there is no sort
button anywhere on this page" — is exactly the gap. Either the artboard says the
phone card list does not sort, and the comment should say so, or the phone needs
its own control.

### F6 — `aria-pressed` is not the sort idiom (nit, no change proposed)

`aria-sort` is the attribute for a sortable column, but it is valid only on
`columnheader`/`rowheader`/`gridcell` and `.clients-table` carries no table
roles, so it cannot be used without giving the grid `role="table"`/`row`/
`columnheader`/`cell` first. `aria-pressed` on a mutually-exclusive set is the
house convention (`components/chip.tsx:4`, `pages/dashboard/queries-over-time.tsx:125`,
`pages/live-feed.tsx:346`), and the `visually-hidden ", ascending"` carries the
direction. Consistent with the codebase; not standard ARIA.

### F7 — doc nit

`address.ts:49` says the ascending order "is also the tie-break when the table is
ordered by query count". It is the tie-break for all three number columns.

## Checked and clean

| Question | Answer |
| -------- | ------ |
| Reorder while a row is expanded or renaming | Safe — `key={client.ip}` at `clients.tsx:387` |
| Test expectations vs fixture | All eight orders recomputed by hand, including the 9118 tie between `.50` and `.22`; correct |
| Fixture change `18204 → 9118` | Local to `clients.test.tsx`; no other file reads it |
| IPv6 flood reaching the page | `getClients` narrows server-side, page opens on `family=v4` |
| Stale docs claiming the old descending order | None; the one hit is a historical review file |
| Other callers of `compareAddressesDesc` | None — the table was the only one |
| Sort stability | Explicit tie-break, not reliant on `Array.sort` stability |

## Measurements

Sort cost, node 22 on the x86 dev box, 200 repetitions, synthetic all-IPv6
rows. Browser-side work — no RB5009 figure applies and the ~9× factor is not
used here. Supersede by re-running against a real `/clients` response.

| n | column | key in comparator | key precomputed |
| --- | ------ | ----------------: | --------------: |
| 64 | address | 0.41 ms | 0.05 ms |
| 64 | queries, counts all equal | 0.38 ms | 0.04 ms |
| 512 | address | 4.59 ms | 0.36 ms |
| 512 | queries, counts all equal | 4.50 ms | 0.34 ms |
| 4096 | address | 48.35 ms | 2.88 ms |
| 4096 | queries, counts spread | 5.46 ms | 2.53 ms |
| 4096 | queries, counts all equal | 48.69 ms | 2.96 ms |

Header widths, headless Chrome 1400 × 900 on Windows (`system-ui` = Segoe UI),
the committed stylesheet and head markup.

| header | track | label | label + caret + gap | slack |
| ------ | ----: | ----: | ------------------: | ----: |
| Address | 268 px | 57.1 px | 69.6 px | +198.4 px |
| Queries 24 h | 88 px | 85.1 px | 97.6 px | **−9.6 px** |
| Blocked | 74 px | 56.5 px | 69.0 px | +5.0 px |
| Blocked share | 120 px | 101.2 px | 113.7 px | +6.3 px |

Gates, run on the committed tree.

| Gate | Result |
| ---- | ------ |
| `tsc --noEmit` | clean |
| `vitest run` | 58 files, 1046 tests passed |
| `vite build` | 137 351 B gzip of the 153 600 B budget (89.4 %), brotli 121 868 B |

## Files changed

| File | ± |
| ---- | - |
| `dashboard/frontend/src/pages/clients.test.tsx` | +155 −39 |
| `dashboard/frontend/src/pages/clients.tsx` | +108 −14 |
| `dashboard/frontend/src/pages/clients/address.test.ts` | +17 −19 |
| `dashboard/frontend/src/pages/clients/address.ts` | +8 −7 |
| `dashboard/frontend/src/pages/clients/client-row.tsx` | +3 −4 |
| `dashboard/frontend/src/pages/clients/share.ts` | +7 (new) |
| `dashboard/frontend/src/styles/components.css` | +37 |

## Fixes applied

All seven taken, on the owner's instruction, in the working tree on top of
`f27237a`.

| # | What was done |
| - | ------------- |
| F1 | `share.ts` deleted. `client-row.tsx` and `SORT_KEYS.share` call `derive.blockedPercent`. One copy of the formula, not three. |
| F2 | `compareAddressesAsc` replaced by `compareKeys`, which compares two keys and builds none. `clients.tsx` builds the keys once in a `keyed` memo on `[items]`, so a header click rebuilds nothing. `SORT_KEYS[sort.column]` hoisted out of the comparator. |
| F3 | Queries track 88 px → 100 px, and the caret box is reserved on every sortable header, so the label does not move when a column is activated. |
| F4 | Every sortable header draws a caret; idle ones at `opacity: 0.35`, pointing the way the first click would open. |
| F5 | The phone does not sort — the header row stays hidden and the CSS says why. The owner's actual ask was the family filter, which `.ch .chips { display: none }` had been hiding on the phone since p5-07: a second placement now renders it on the title's own row, 44 px targets, search on the line below. |
| F6 | Left as `aria-pressed`, the house convention. |
| F7 | `address.ts` doc corrected — the tie-break serves all three number columns. |

New tests. The three marked *falsified* were each shown to fail with their own
wiring broken, then reverted.

| Test | Pins |
| ---- | ----- |
| `builds one address key per row on the read and not one more per click` | F2, and not with a stopwatch — the module is wrapped and the builds counted. *Falsified:* the tie-break was put back to building its own keys. |
| `holds the queries header at 100px, its label plus the sort caret` | F3. *Falsified:* track set back to 88px. |
| `reserves the caret box on idle headers so no label moves when clicked` | F3/F4 — the idle caret dims, it does not `display: none`. |
| `is rendered twice and hidden once…` | F5's CSS pairing, which no rendered test can reach. *Falsified:* the `.chips` override deleted. |
| `sizes the phone chips from their own words…` | The `flex: 1` zero-basis collapse found while fixing F5. |
| `carets every sortable header, the idle ones showing what a click opens` | F4 |
| `repeats the family filter in the title…` | F5's markup, and that picking a chip there re-reads with `?family=`. |

Click cost after the fix, same bench as above — the keys are memoised on the
read, so a click builds none:

| n (all IPv6) | click | before | after |
| ---: | ----- | -----: | ----: |
| 512 | address | 4.68 ms | 0.06 ms |
| 512 | queries, counts equal | 4.68 ms | 0.07 ms |
| 4096 | address | 50.28 ms | 0.76 ms |
| 4096 | queries, counts equal | 50.92 ms | 0.75 ms |
| 4096 | one-off key build on the read | — | 2.03 ms |

Header widths after the track change, same probe: Queries 24 h needs 97.6 px of
100 px, and the figure is identical whether the column is active or idle.

Phone header at 390 px, measured in a 390 px frame: title and the three chips
on one 44 px row, the chips right-aligned to within 0.0 px of the search input's
own right edge on the line below.

| Gate | Result |
| ---- | ------ |
| `tsc --noEmit` | clean |
| `vitest run` | 58 files, 1053 tests passed |
| `vite build` | 137 501 B gzip of the 153 600 B budget (89.5 %), brotli 122 025 B |

## Remaining TODOs

- The 4096-row figures are measured, not pinned. A timing assertion would be
  flaky in CI-less local runs; the key-build count pins the algorithm instead,
  which is what the 66× came from.
- Every layout figure came from a probe page against the real stylesheet, not
  the running dashboard. One pass over Clients on desktop and phone is still
  owed before this ships.
- Not committed. Whether the phone-filter fix rides here or as its own
  `fix(dashboard):` commit is open — it is a p5-07 bug, not sort work.

**PASS** — no correctness defect was found in `f27237a`, and all seven findings
are fixed in the working tree. Not committed.
