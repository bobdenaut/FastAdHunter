# Phase 2.7 — Device identity from the RouterOS REST API

**Objective:** implement
[ADR-0010](../../../docs/decisions/0010-device-identity-from-routeros-rest.md):
a client's durable identity is its MAC address, read from the router over
REST, read-only, on demand. Policies and names follow a device across IPv6
privacy-address rotation. Fix 1's address expiry stays; the device outlives its
addresses.

**Why this order:** the ADR's own order, bottom of the crate graph first, and
every step leaves the workspace green and the deployed behaviour unchanged
(`[routeros] url = ""`). `fah-model` types before anything can name them; the
registry before the resolver that reads it; the resolver before the tick that
feeds it; the binary before the surfaces that show it; docs and the router
last, when there is something true to write.

**The ADR is closed.** Nothing here reopens REST vs netlink, MAC identity,
on-demand polling, the 10 min full refresh, one poll per tick, whole-poll
timeout, pending-set retry, or the two retention lifetimes. A step that seems
to need a different decision stops and reports.

**Standing rules that bite here:** no comments in Rust or TS; no commit
without a go for that step; no `.md` edit outside `docs/code-review/phase2.7/`
without a yes; the RB5009 is read-only for agents — every router command in
this phase is proposed, never run.

Full plan, all steps: [dev-plan.md](dev-plan.md). Task files are split from it
after the plan is reviewed.

| Task | Slug | Status |
| ---- | ---- | ------ |
| p2.7-01 | `fah-model`: `MacAddr`, `ClientSelector::Mac`, poll-failure counter | WAITING |
| p2.7-02 | `fah-stats`: IP → MAC, devices, pending set, two lifetimes, cap | WAITING |
| p2.7-03 | `fah-rules` + `fah-config`: `Mac` resolution, selector parsing, validation | WAITING |
| p2.7-04 | binary: `[routeros]` config, REST source, planner, policy-tick poll | WAITING |
| p2.7-05 | `fah-api` + dashboard: `mac`/`device_name`, `/devices`, grouped clients page | WAITING |
| p2.7-06 | docs, deploy runbook, on-device verification | WAITING |
