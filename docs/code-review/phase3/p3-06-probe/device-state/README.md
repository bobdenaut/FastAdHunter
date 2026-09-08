# R0 — device state before Step 4, read 2026-09-08

Read-only reconnaissance for
[p3-06-phase3-verification-runbook.md](../../p3-06-phase3-verification-runbook.md)
R0. **Nothing was written to the router.** Every command was a `print`.

This is the rollback reference: the state Step 4 has to be able to return to.

| File | Commands |
| --- | --- |
| [r0-containers.txt](r0-containers.txt) | `/container/print detail`, `/container/envs/print detail`, `/container/mounts/print detail`, `/interface/veth/print detail` |
| [r0-network.txt](r0-network.txt) | `/ip/firewall/nat/print`, `/ip/firewall/filter/print chain=forward`, `/ip/dhcp-server/lease/print`, `/interface/list/member/print` |
| [r0-files.txt](r0-files.txt) | `/file/print` — the `kingston/` tree and a check for any `fah-*` path |

## What R0 settles

| Question | Answer |
| --- | --- |
| Does a `fah-probe` container exist? | **No.** One container only: `fastadhunter` 0.3.3, running, `veth1` |
| Are the campaign tars uploaded? | **No.** Three tars present, all production: 0.3.1, 0.3.2, 0.3.3 |
| Do the campaign directories exist? | **No.** `/file/print where name~"fah-"` returns nothing |
| Which mount lists exist? | Two, both production: `fah-config` → `/kingston/fastadhunter/config`, `fah-data` → `/kingston/fastadhunter/data` |
| Which env lists exist? | One, `fah-env`, four keys: `FAH__RUNTIME__HTTP_RUNTIMES=2`, `MIMALLOC_ARENA_EAGER_COMMIT=0`, `MIMALLOC_PURGE_DECOMMITS=1`, `MIMALLOC_PURGE_DELAY=0` |
| Is `veth3` free? | **Yes.** `veth3` exists, comment `fah-test`, `172.17.0.4/24`, and carries no container (no `R` flag) |
| Does anything steer tcp/443? | **No.** `dstnat` ends at index 10; rules 9 and 10 steer port **80** to `172.17.0.2:8080`; rules 5 and 6 are disabled |

## Consequences for the runbook

- **R1 has nothing to fix.** The `strategy = "fallback"` problem belongs to a
  probe config that does not exist. The probe's `/config` is created fresh at
  its first boot, so R1 collapses into "do not write that key".
- **R5 is a plain `add`.** No `stop`, no `remove` — there is no predecessor.
- **R4 is the first write of the campaign**, not R1.
- **`h1buf-env` is gone.** Only `fah-env` remains, so `fahprobe-env` is built
  from scratch. Its four keys are the ones listed above, with
  `FAH__RUNTIME__HTTP_RUNTIMES` as the axis P10 sweeps.
- **R7 stays owner-only and optional.** Nothing in `dstnat` matches tcp/443
  from the LAN today, so it would be a genuinely new rule — and no measured arm
  needs it.

## Two drifts against the checked-in docs

- [routeros-traps.md](../../../../routeros-traps.md) records production's
  `cpu-list` as `""`. The live value is `cpu0,cpu1,cpu2,cpu3`. Functionally
  identical — all four cores, no restriction — but the recorded form is stale.
- The same file calls `ENV_FAH` a stale list that still exists. It no longer
  appears at all; `/container/envs/print detail` returns four rows, all
  `fah-env`.

Neither is corrected here — this file records what was read, and both belong to
a document with its own edit approval.
