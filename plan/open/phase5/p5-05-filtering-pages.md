# P5-05 — Filtering Pages

**Phase:** 5 · **Depends on:** p5-04 · **Model:** Opus

## Goal

Custom Rules, Policies, Clients and Rule Tester. Four pages sharing one theme:
FastAdHunter's filtering model is not Pi-hole's, and each page has to express
the real model rather than the familiar one.

## Context

The reshaping is decided in
[capability-matrix.md](../../../docs/dashboard/capability-matrix.md) section
Reshaped, and detailed in
[information-architecture.md](../../../docs/dashboard/information-architecture.md).
Sketches: `CustomRules`, `Policies`, `Clients`, `RuleTester`.

## Scope

**Custom Rules** — the user-rules document endpoints.

- A line editor, not a table. The write validates the whole document and swaps
  it atomically.
- On a validation failure, per-line messages anchor to their lines; nothing is
  written and the user's text is preserved exactly.
- No per-rule delete and no per-rule toggle: the API has no per-rule identity,
  and offering one would promise a write that cannot happen.

**Policies** — the policy CRUD plus the per-policy traffic split from stats.

- One card per policy: list subset — a null subset renders as "every enabled
  list" — blocking-mode override, assignments with days and window, and traffic.
- Schedule timezone and the active-assignment count in the header. That count
  reports a schedule boundary having passed without waiting for a query.
- Confirm before anything that recompiles: create, change the list subset,
  delete. Renaming and reassigning do not recompile and carry no warning. The
  16-policy ceiling is enforced in the form, and the default policy is reserved.

**Clients** — the client list, naming, and per-client policy assignment.

- Rename inline; assign a policy inline with an optional schedule.
- Distinguish an assignment naming this address from one inherited via subnet or
  name — the API returns the policy in force now, and the assignment field is
  absent in the inherited case.
- A client whose window is shut shows its policy, marked not in force.
- State plainly that clients are observed by traffic: no ARP table, no DHCP
  leases, no device inventory.

**Rule Tester** — the verdict dry-run.

- Domain, query type, and either a client (address or name) or a policy.
- Result: verdict, matching rule, source list, deciding policy, and why that
  policy applies.

**Mobile.** `sketch/MobileClients.dc.html` is the source of truth for the
Clients phone layout. It settles: one card per client carrying name, address,
policy chip, counts and last-seen; the solid-versus-dashed assignment
distinction surviving at card size; a shut schedule window called out in words
on the card; and tapping a card expanding its two actions in place rather than
pushing a detail screen — rename and change-policy are one round trip each.

The other three follow the same rules without their own artboards: the rule
editor keeps a usable line-number gutter and its error anchoring at 390 px;
policy cards stack with their assignment rows intact; the Rule Tester is a
single-column form with the result directly beneath it.

## Acceptance criteria

- A validation failure on the rules document anchors every message to the right
  line, and the document is not written.
- Policy operations that recompile are confirmed; those that do not are not, and
  the difference is stated in the UI.
- Assignment changes apply without a recompile.
- Inherited versus direct assignment is visually unambiguous.
- Rule Tester returns and renders all four result fields, in both the client and
  the policy mode.
- No page invents a per-rule identity, a group, or a device inventory.
- Correct in both themes at all three breakpoints, verified at 390 px. Touch
  targets at least 44 px.
- Gates green, cargo and frontend. Bundle size recorded.

## Out of scope

Runtime pages (p5-06). Settings and Diagnostics (p5-07).

## Suggested prompt

> Read docs/dashboard/capability-matrix.md section Reshaped,
> docs/dashboard/information-architecture.md, API.md, and
> plan/wip/phase5/p5-05-filtering-pages.md. Build Custom Rules as a validated
> document editor, Policies with the recompile confirmations, Clients with
> inline naming and assignment including the inherited-versus-direct
> distinction, and the Rule Tester in both its client and policy modes. Design
> the phone layout for each.
