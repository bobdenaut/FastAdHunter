# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-08-09

## Now

| | |
| --- | --- |
| Branch | `main`, pushed to **origin and backup** |
| Tree | clean |
| Tests | green — `fmt`/`clippy`/`test`, 886 across 41 binaries |
| Deployed | **0.2.14** on the RB5009 since 2026-08-09T10:33Z — **does not contain the p1-01 fixes below** |
| Phase | 2 (`plan/wip/phase2`) — **every task DONE** |
| **Next** | deploy the p1-01 parser fixes and verify them on-device |

`feat/phase2-http-pipeline` is fully merged into `main` and not deleted.

**Every phase-2 task is DONE.** The `wip` → `closed` move is the owner's to make.

## Phase 2 status

| Task | State |
| --- | --- |
| p2-00 … p2-14 | DONE |

### `p2-13` — closed on-device 2026-08-09

`peak_rss` is in `/history/perf`, verified on `0.2.14`: 117.73 MiB in the series,
the same figure `/debug/memory` reports live, and 241 rows written by `0.2.13`
read back intact. Report:
[`p2-13-review.md`](code-review/p2-13-review.md).

**The refresh peak has not appeared yet.** `refresh_hours = 48`, so the first one
lands up to two days after deploy and should raise the series 117.73 → ~180 MiB.
A flat series until then is not the peak having gone away, and `peak_rss ≈ 180`
beside `rss ≈ 52` afterwards is correct rather than a leak — it is a high-water
mark, not an average.

### `p2-14` — closed on-device 2026-08-09, zero code

IPv6 HTTP interception. The listener was already dual-stack, so the whole gap
was four RouterOS rules; the "no code needed" hypothesis held. Report:
[`p2-14-review.md`](code-review/p2-14-review.md).

The ISP's IPv6 came back the same evening — `IPv6 global UP` at 21:10:07, and
`traceroute6` now completes where it died at the third hop.

The strongest evidence is the per-client one: **one machine, one URL, one
moment, only the address family differing** — a `$client=<lan>/64` rule gave
403 over IPv6 and 404 over IPv4. LAN-to-LAN traffic is proven skipped by the
rule's own packet counter rather than by an absent response.

**One criterion is deliberately carried instead of met**: how far `fah-lan6`
lags a *live* delegation change is unmeasured, because the one rotation observed
was a deliberate reboot. Its failure mode is a LAN-to-LAN request briefly
proxied, not a loss of filtering.

## `p1-01` — reviewed and fixed, in `main`, not deployed

Phase 1's only task that never got a code review. Seven of fifteen findings are
fixed; report and instrument:
[`p1-01-review.md`](code-review/p1-01-review.md), [`p1-01-ab/`](code-review/p1-01-ab/).

On Windows/x86 over the 16 deployed lists refetched 2026-08-09 (22.8 MB;
3 hosts, 12 adblock, 1 plain-domain), boot measured **398.7 → 290.6 ms, ≈ −27 %**,
against a 1.4 % layout floor, with the compiled ruleset identical in every arm.

**Nothing here is on-device.** `compile_duration_seconds` (2.844 s on 0.2.14)
spans read + pre-parse ceiling + parse + build, so a container replace over the
same `/data` cache is a clean before/after — boot-compile to boot-compile.
Until that runs, PERFORMANCE.md's budget rows stay as they are.

Still open from the review: **M4** (`ParsedRule` is 80 B, 48 of them unused on
~every rule — needs `p2-12`'s instrument first), m5, m6 and five nitpicks.

## Deployed

**0.2.14** on the RB5009 since 2026-08-09T10:33Z, `mode=dns+http`, 798,760
compiled rules, boot peak 117.73 MiB. It adds `peak_rss` to the history and
changes nothing else; **no config key moved**, so the `deny_unknown_fields`
ordering trap below did not apply to this release. T0 captures in
`code-review/soak-0.2.14/`.

Its predecessor **0.2.13** was verified over 18.5 h —
[`soak-0.2.13-report.md`](code-review/soak-0.2.13-report.md), and the findings
still describe the running system:

| | |
| --- | --- |
| RSS | 52.0 MiB, slope +0.12 / −1.26 MiB/h (signs disagree → no drift) |
| Peak RSS | 117.82 MiB, unchanged — no refresh ran |
| Stale serves | 9,568, all timed as cache reads, `forward` == misses exactly |
| SWR | `dropped` 0, every stale serve enqueued or deduplicated |
| Upstream failures | 4 of 13,556, all during ISP link outages |

**The stale-serve fix is proven.** At T0 `cache_stale` was 0, so the `FromSwr`
arm had never executed; it now has, 9,568 times, and the latency stages
partition exactly.

Rollback: the last version tarballs are on `kingston/`, so reverting is
`/container/add` from the previous tar rather than a rebuild.

## Deploy ordering — still true for the next one

`deny_unknown_fields` on the root `Config` makes an unknown section a **boot
failure, not a warning**, and nothing migrates the file. Any key a release
removes must be deleted from `/config/fastadhunter.toml` *before* that release
starts.

Getting the order wrong bricks the resolver until the file is fixed —
`/tool/netwatch` fails DNS over to public resolvers meanwhile, so clients keep
working *unfiltered* and nobody complains.

## The router is instrumented

Added 2026-08-09; details and traps in
[`routeros-traps.md`](routeros-traps.md) §Logging.

- `netwatch` `v6-global` / `v4-global` log reachability transitions. `v6-global`
  sits `down` until the ISP fixes routing, and will emit one `IPv6 global UP`.
- `log-wan-ip` logs one line per change of public IPv4, WAN IPv6 or delegated
  prefix.
- `dhcp`/`route`/`pppoe`/`ppp`/`interface` to `kingston/net-log.*.txt`, all with
  `!debug,!packet`.

**The ISP re-delegates a new `/56` on every PPPoE redial**, six observed inside a
week. No router config hardcodes a DIGI global any more — verified by grepping
`2a02:` over a full export. The redials also cause real DNS resolution failures,
not merely address churn.

## Open items

- **HTTP concurrency is unmeasured.** Every p2-08 figure is one connection at a
  time, while `[http] max_connections` defaults to 1024. The DNS side has 20 k+
  QPS of on-device evidence; the HTTP side has none, and Phase 3 multiplies the
  question by TLS.
- **The deployed HTTP path still rests on 5 requests.** The probe deliberately
  excludes veth, dst-nat, conntrack and origin RTT.
- **`to-ports` support on IPv6 dstnat is unverified** — `p2-14` finds out.
- **List order is worth ±19.92 MB on the compile peak** and nothing enforces it;
  the deployment sits at the best case because `big.oisd.nl` is entry 0 in config
  order. Sorting is not free — it changes arena layout and which duplicate wins.
- **~59 MiB of compile ratchet survives `PURGE_DELAY=0`.** Suspect is
  `arena_reserve` = 1 GiB. Worth an experiment only if ≤128 MiB becomes the
  target or Phase 3's footprint makes it tight.
- **19.08 MB of the compile transient is unattributed** — allocator/OS, bounded
  by measurement but not explained (`p2-12`).
- `MIMALLOC_PURGE_DELAY=0` does not thrash at **~1 qps** (measured over 18.5 h,
  page-fault rate flat and traffic-shaped). Unmeasured under sustained load.
- **`heap::string_bytes` documents "Capacity, not length"** but takes `&str` and
  returns `len()`. Under-reports. Unfixed.
- `shrink_to_fit` on the cache map is **still undecided**; needs a
  fill-then-drain test, not a soak.
- The cache byte cap's **eviction path has never fired on-device** — the entry
  cap binds first every time, so p1.5-05's headline criterion is unit-test-only.
- `ListStatus::last_refreshed` reports `null` after a restart alongside
  `last_result: Ok`. Deliberately not fixed — reversing it needs
  RULE_ENGINE.md/API.md review.
- `HTTP_ORIGIN_PORT` is hardcoded to 80, and the egress guard allows no other
  port. Correct in production; it means `http_e2e` needs `127.0.0.1:80` and skips
  when it cannot bind it.
- The mimalloc-vs-`System` A/B to isolate the −17 % CPU from the hit-ratio
  confound.
- Forward rule 4 on the router, "Postgres ACCEPT - FAH", says
  `src-address=172.177.0.2` — a typo for `172.17.0.2`. Harmless; delete rather
  than fix.
- A comment in `crates/fah-rules/src/lifecycle/mod.rs` narrates what the
  scheduler "used to emit", against root CLAUDE.md rule 21.
- **`parse_errors` is counted per list and exposed nowhere.** `ListEntryView`
  and `/api/v1/lists` omit it, and `looks_misparsed` only logs when errors
  outnumber rules — so p1-01's 13 → 1 improvement cannot be confirmed on-device
  without adding the field.
- **`tui-monitor/config.toml` is tracked and holds a bearer token**, while
  `.gitignore` excludes `.vscode/` for exactly that reason.
- **A name makes a client immune to eviction** — `ClientRegistry` drops the
  least-recently-seen *unnamed* entry first (`client_registry.rs:81-85`). On an
  IPv6 privacy address that pins an identity which expires within days. Name
  only stable addresses: IPv4, or an EUI-64 ULA once the device has privacy
  extensions off — the ULA prefix never rotates, so that one holds. The registry
  caps at 4096, so this is data hygiene, not a memory bound. Corollary:
  `$client=<name>` reaches only the address families that carry the name, which
  is why `p2-14` tested with a CIDR term instead.

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (os error 10013) when WinNAT has reserved the ephemeral port block
being drawn. **Environmental** — it fails identically on a stashed clean tree.
Do not attribute it to a change.
