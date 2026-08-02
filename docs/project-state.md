# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-08-02

## Now

| | |
| --- | --- |
| Branch | `feat/phase2-http-pipeline` at `63bea82`, pushed to `origin` **and** `backup` |
| Tree | clean |
| Tests | 779 passing |
| Phase | 2 (`plan/wip/phase2`) |
| Next task | **`p2-09` query-log reader** (Opus) — p2-08 is soaking, not blocking |

## Phase 2 status

| Task | State |
| --- | --- |
| p2-00 … p2-06, p2-10 | DONE |
| **p2-07** | **`AWAITING SOAK`** — code complete; needs the running soak |
| **p2-08** | **`AWAITING SOAK`** — dev-box work complete; needs the running soak |
| p2-09 | WAITING — the next task that can actually be worked |

`AWAITING SOAK` is defined in [plan/CLAUDE.md](../plan/CLAUDE.md): code complete
but an acceptance criterion needs the RB5009. The selector skips it like `DONE`;
no phase closes while one exists.

## The soak — both open tasks depend on it

**T0 `2026-08-02T10:18:25Z`.** Read after `2026-08-03T10:18:25Z`; 72 h is the
better window. Baseline, capture commands and pass criteria:
[docs/code-review/0.2.10-soak-baseline.md](code-review/0.2.10-soak-baseline.md).

**Do not change the router or the config while it runs.** A mid-window change
gives any step in the series two explanations, which is the ambiguity the soak
exists to remove.

**What flips p2-07:** the series pulled from `/api/v1/history/perf` **on-device**
— a residual per sample across the window, a row agreeing with `/debug/memory` at
the same instant, and the slope over the final third stated. A *drifting*
residual still flips it and opens a leak task.

**What flips p2-08:** the same window plus a short generated-load run afterwards
for the on-device HTTP numbers. This soak **cannot** produce them — ~8 intercepted
connections per 6 minutes, with no control over body size or origin RTT.

## Deployed

**0.2.10** on the RB5009, `mode=dns+http`, started `2026-08-02T10:18:25Z`.
Steady RSS **58.13 MiB**; peak at boot **123.74 MiB** (96.7 % of the 128 MB
budget — the compile transient, not steady state). 16 lists `ok`,
1 138 898 parsed → 794 931 compiled, 1 policy. DNS unaffected by the HTTP
pipeline: 331 of 333 blocks under 100 µs.

**Rollback is available.** The last four version tarballs are on `kingston/`, so
reverting is `/container/add` from the previous tar rather than a rebuild — the
old container *entry* is gone, but the image is not. Disable the `fah-liveness`
scheduler around the swap.

## Open items

- **HTTP interception is IPv4-only.** Verified 2026-08-02: a host reached over
  IPv6 bypasses the proxy entirely. Mirror `/ipv6/firewall/nat` rules are written
  and **deliberately deferred to after the soak** —
  `docs/code-review/0.2.10-soak-baseline.md` §Known gap. Two open points there:
  `to-ports` support on IPv6 dstnat is unverified, and IPv6-only origins are
  unreachable from a ULA-only container.
- **Compile-time peak RSS lever** — streaming list parsing may reduce it.
  Deferred until a heap profile says whether raw text or the pre-dedup index
  dominates the transient (PERFORMANCE.md §Budgets).
- `HTTP_ORIGIN_PORT` is hardcoded to 80, and the egress guard allows no other
  port. Correct in production; it means `http_e2e` needs `127.0.0.1:80` and skips
  when it cannot bind it.
- The mimalloc-vs-`System` A/B to isolate the −17 % CPU from the hit-ratio
  confound.
- Remove `MIMALLOC_VERBOSE` from `fah-env` (router-side, liviu's action).
- Forward rule 4 on the router, "Postgres ACCEPT - FAH", says
  `src-address=172.177.0.2` — a typo for `172.17.0.2`. Harmless (FAH does not use
  postgres); delete rather than fix.
- `shrink_to_fit` on the cache map is **still undecided**; a soak cannot settle
  it (tables grow from `map.capacity()`, and the cache peaked at 2.2 % of cap).
  Needs a fill-then-drain test.
- The cache byte cap's **eviction path has never fired on-device** — the entry
  cap binds first every time, so p1.5-05's headline criterion is unit-test-only.
- `ListStatus::last_refreshed` reports `null` after a restart alongside
  `last_result: Ok`. Deliberately not fixed — reversing it needs
  RULE_ENGINE.md/API.md review.
- `main` is behind this branch; merge is a fast-forward when the phase closes.

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (os error 10013) when WinNAT has reserved the ephemeral port block
being drawn. **Environmental** — it fails identically on a stashed clean tree.
Do not attribute it to a change.

The retry loop in `tests/common/mod.rs` survives it, but only while each boot
draws the minimum number of ports: a third draw per attempt made it lose the
race noticeably more often, which is why `Ports::http()` is lazy.
