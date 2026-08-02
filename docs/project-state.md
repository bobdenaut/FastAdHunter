# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-08-02

## Now

| | |
| --- | --- |
| Branch | `feat/phase2-http-pipeline` at `0cfa317`, pushed to `origin` **and** `backup` |
| Tree | clean |
| Tests | 778 passing |
| Phase | 2 (`plan/wip/phase2`) |
| Next task | **`p2-08` phase verification** (Opus) |

## Phase 2 status

| Task | State |
| --- | --- |
| p2-00 … p2-06, p2-10 | DONE |
| **p2-07** | **`AWAITING SOAK`** — code complete, gates green; needs on-device evidence |
| p2-08 | WAITING — the next task; its soak also closes p2-07 |
| p2-09 | WAITING — query-log reader |

`AWAITING SOAK` is a status defined in [plan/CLAUDE.md](../plan/CLAUDE.md): code
complete but an acceptance criterion needs the RB5009. The selector skips it like
`DONE`; no phase closes while one exists.

**What flips p2-07 to DONE:** the p2-08 soak, pulling the series from
`/api/v1/history/perf` **on-device** (not an external curl loop — the point is
that the router self-hosts it), confirming a residual per sample across the
window and a row agreeing with `/debug/memory` at the same instant, then stating
the slope over the final third. A *drifting* residual still flips it — the
instrument worked — and opens a leak task.

## Deployed

**0.2.9** on the RB5009. Soak passed: 28 h 26 m, one start, no restart, no OOM;
RSS half-to-half drift +0.28 MB against a 2 MB bar, OLS slope −0.131 MB/h.
Evidence in `docs/code-review/0.2.9-soak-24h.md` + `docs/code-review/soak-0.2.9/`.
Throughput 20 k+ QPS post-mimalloc; FAH-handling-bound, not ingest-bound.

## Open items

- **Stress testing is unblocked** — the soak window it would have disturbed is
  finished.
- `shrink_to_fit` on the cache map is **still undecided**; a soak cannot settle
  it (tables grow from `map.capacity()`, and the cache peaked at 2.2 % of cap).
  Needs a fill-then-drain test.
- The cache byte cap's **eviction path has never fired on-device** — the entry
  cap binds first every time, so p1.5-05's headline criterion is unit-test-only.
- `ListStatus::last_refreshed` reports `null` after a restart alongside
  `last_result: Ok`. Deliberately not fixed — reversing it needs
  RULE_ENGINE.md/API.md review.
- The mimalloc-vs-`System` A/B to isolate the −17 % CPU from the hit-ratio
  confound.
- Remove `MIMALLOC_VERBOSE` from `fah-env` (router-side, liviu's action).
- `main` is behind this branch; merge is a fast-forward when the phase closes.

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (os error 10013) when WinNAT has reserved the ephemeral port block
being drawn. **Environmental** — it fails identically on a stashed clean tree.
Do not attribute it to a change.
