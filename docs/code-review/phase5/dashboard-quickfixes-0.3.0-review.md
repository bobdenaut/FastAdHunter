# Review — dashboard quick fixes after the 0.3.0 deploy (2026-08-29)

**Scope:** the uncommitted working-tree changeset on `main` after `ef9924e` —
7 modified + 3 new files, all under `dashboard/frontend/`; no Rust change, so
the cargo gates are unaffected by construction.

## Summary

- Live Feed: ms column `toFixed(1)` → `toFixed(3)` (table + phone card); new
  Cache column (`HIT`/`MISS`, `—` on HTTP and blocked rows) replacing the
  `cached` marker in Detail; footnote updated.
- Clients: address-family chips (all/IPv4/IPv6); descending sort on a padded
  numeric address key (`clients/address.ts`, 7 tests).
- Queries-over-time tooltip: side chosen by plot midpoint (memory-trend
  idiom) with a clamp as safety net, replacing the fit test.
- `Chip` extracted to `components/chip.tsx`; one definition, two importers,
  `.chips` CSS present.
- Verified, not taken from the summary: typecheck clean; **939/939 tests,
  55 files**; build green at **129,553 B gzip (84.3 % of budget), 114,850 B
  brotli**.

## Verification notes

- `FeedCache` semantics checked against ADR-0001: `allow` reaches the cache,
  so HIT/MISS on `pass`/`allow` DNS rows is correct; `block` and HTTP rows
  render `—` — a lookup that never happened is not a miss. Tests cover all
  four shapes.
- v4-mapped addresses cannot reach `isV6`'s colon test as false v6:
  `pipeline.rs:286–290` canonicalizes `::ffff:a.b.c.d` once at ingress.
- Padded-hex group comparison is order-correct for IPv6 (equal-width
  lowercase hex compares lexicographically as it does numerically; the engine
  emits Rust's lowercase `Display` form).
- `clients.test.tsx` now selects rows by address (`rowFor`), removing the
  test's coupling to sort order — the right fix, not an index shuffle.
- Tooltip: box width no longer enters the side decision, so the
  flip-oscillation class is gone; `redraw` behaviour untouched.

## Findings

### QF1 — Minor: malformed addresses sort first, and the doc comment says last

`addressKey` returns `v9:<literal>` for an unparseable address and the
comment promises it "sorts last". Under the page's **descending** order,
`v9:` > `v6:` > `v4:`, so a malformed row leads the table instead. Unlikely
in practice (the API echoes engine-parsed `IpAddr`s, so the guard is
defensive), but behaviour and comment disagree.

**FIXED.** Prefix changed to `v0:`, which sorts below both families under
the descending order; comment now states the direction it holds for. New
test pins it (`sorts a malformed address last under the descending order`).

### QF2 — Nitpick: `localeCompare` for key ordering

Keys are ASCII, so this was stable, but locale has no business in the order.

**FIXED.** `compareAddressesDesc` in `clients/address.ts` — plain code-unit
comparator, the one comparator both the page and the tests now use.

### QF3 — Nitpick: empty-state sentence assembly

The "none matching {family} {needle}." string was built from four inline
conditionals.

**FIXED.** Extracted to `NoMatchDescription`, one shape per filter
combination, including the unreachable neither-filter branch.

**PASS.** All three findings fixed and verified: typecheck clean, **940/940
tests** (the new malformed-order test added), build green at **129,623 B
gzip (84.4 %), 114,908 B brotli** — +70 B gzip against the pre-fix build.
