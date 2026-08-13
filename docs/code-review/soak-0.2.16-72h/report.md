# Soak 0.2.16 — 72 h, RB5009 (in progress)

Device: RB5009UG+S+, RouterOS 7.21.5 (long-term), 4× ARMv8, 1 GB shared.
Container `fastadhunter`, image `fastadhunter-rosready-0.2.16.tar`, distroless/musl.
Allocator: mimalloc — `MIMALLOC_PURGE_DELAY=0 MIMALLOC_PURGE_DECOMMITS=1
MIMALLOC_ARENA_EAGER_COMMIT=0`.
Workload: live household DNS, no synthetic load.

0.2.16 carries three changes over 0.2.15: the p1-01 **M4** parsed-rule layout
fix (`6933a0f`), and the two refresh-clock fixes (`9976783`). M4 is the only one
that touches the compile path — verified by `git diff 2271a10..HEAD -- crates/`.

| Point | Wall clock | Uptime | Stamp |
| --- | --- | --- | --- |
| Router reboot | 2026-08-13 13:14:32Z | — | — |
| Container start | 2026-08-13 13:15:23Z | — | — |
| T0 | 2026-08-13 13:21:16Z | 313 s | `20260813T132116Z` |
| Manual `POST /lists/refresh` | 2026-08-13 14:04:33Z | 2 950 s | — |
| T1 | 2026-08-13 14:04:58Z | 2 934 s | `20260813T140458Z` |
| T2 | pending | — | pending |

The manual refresh is the run's second clock: fix A holds only if no scheduled
refresh fires before **2026-08-15T14:04:33Z**.

Snapshots live beside this file. Predecessor run:
[../soak-0.2.15-72h/report.md](../soak-0.2.15-72h/report.md), closed at 54.0 h.

## Decisions

- T0 is taken at ~300 s deliberately: `process_peak_rss` is then unambiguously
  the boot compile, which the 0.2.15 references are not.
- The run starts from a router reboot, matching the condition its primary
  reference (0.2.15 boot #2) was measured under.
- M4's answer does not need 72 h — one boot and one refresh settle it. The 72 h
  is for the bounded-memory question 0.2.15 could not finish, and for fix A.
- Fix A is falsifiable only at manual-refresh + 48 h, so the manual refresh time
  is the run's second clock.
- The refresh comparison, not the boot one, is the M4 figure of record: its
  confounds are quantified and it matches the operation the reference measured.

## Bugs found

None. Two fixes verified — see §Verification.

## Measurements — T0

### M4 on-device

**M4 is confirmed on-device. On the controlled refresh comparison, peak RSS
decreased by 16.8 MiB raw; after accounting for the larger parsed corpus and the
colder cache state, the estimated normalised reduction is ~14 MiB, close to the
prior 12 MiB prediction. A separate boot measurement showed a larger −33.93 MiB
reduction, but the cause of the magnitude difference is not established; it
should not be merged with the refresh result.** Steady-state RSS, accounted and
residual are essentially unchanged in both.

### The refresh compile — the controlled comparison

Same operation, same corpus family, both confounds quantified. This is the
figure to quote.

| Field | 0.2.15 (2026-08-13 05:26Z) | 0.2.16 (T1) |
| --- | --- | --- |
| **`process_peak_rss`** | 164.65 MiB | **147.85 MiB** |
| compiled rules | 661 832 | 666 130 |
| parsed rules (compiled + duplicates) | 1 117 949 | 1 139 703 |
| `compile_duration_seconds` | 2.419 s | 2.333 s |
| `allocator_committed` | 385.38 MiB | 288.19 MiB |
| cache resident at the compile | ~3 MiB | 0.43 MiB |

Raw Δ **−16.80 MiB**, with two corrections pulling opposite ways:

| Correction | Direction | Size |
| --- | --- | --- |
| 0.2.16 parsed **+1.9 %** more rules; M4's saving scales with the parsed spine | favours 0.2.15 — makes −16.80 conservative | not isolated |
| 0.2.16's cache held ~2.5 MiB less, and cached bytes are resident at the peak | flatters 0.2.16 | ≈ 2.5 MiB |

Removing only the quantified one gives ≈ **−14.3 MiB**. The unquantified one
runs the other way, so ~14 MiB is a **floor, not a midpoint**: 0.2.16 did more
parsing and still peaked lower.

### The boot compile — larger, unexplained

| Field | 0.2.15 T0 (327 s) | 0.2.15 T1 (81 143 s) | **0.2.16 T0 (313 s)** |
| --- | --- | --- | --- |

| Field | 0.2.15 T0 (327 s) | 0.2.15 T1 (81 143 s) | **0.2.16 T0 (313 s)** |
| --- | --- | --- | --- |
| rules compiled at boot | 589 963 → 662 141 | 662 141 | 661 832 |
| `compile_duration_seconds` | 2.504 s | 2.509 s | **2.375 s** |
| `process_peak_rss` | 137.05 MiB | 119.21 MiB | **85.28 MiB** |
| `allocator_committed` (= peak) | 254.81 MiB | 259.19 MiB | **143.50 MiB** |
| `process_rss` | 51.02 MiB | 59.60 MiB | 50.64 MiB |
| `accounted_bytes` | 22.04 MiB | 26.10 MiB | 22.10 MiB |
| `residual_bytes` | 28.98 MiB | 33.50 MiB | 28.54 MiB |
| `ruleset_bytes` | 21.02 MiB | 21.02 MiB | 21.00 MiB |

Neither boot pairing is clean:

| Pairing | Δ peak | Matched on | Unmatched on |
| --- | --- | --- | --- |
| vs 0.2.15 T1 | **−33.93 MiB** | work — one boot compile of ~662k rules | uptime — T1 read at 22.5 h, so its peak is *at most* the boot compile |
| vs 0.2.15 T0 | −51.77 MiB | uptime — both ~300 s | work — T0 paid two compiles, boot plus a 3-list refresh |

**−33.93 MiB is ~3× the −12.00 MB predicted on Windows/x86**
([p1-01-review.md](../p1-01-review.md) §M4) and ~2× what the better-controlled
refresh comparison shows. The structural saving lands as sized — the largest
list is `big.oisd.nl` at 251 967 rules, and 48 B per `ParsedRule` is ≈ 12.1 MiB
off that `Vec` spine — but the excess is **not attributed**. The realloc
collapse (76 copying moves totalling 476.19 MB dropping to 48 totalling
231.69 MB) is the leading candidate, since those in-flight copies are
themselves resident; confirming it needs realloc instrumentation on-device.

**Do not merge this with the refresh figure.** Two compiles of the same corpus
should not save 2× different amounts, and until that is explained one of the two
is measuring something other than M4.

Steady state is the control that makes the transient claim credible: RSS,
accounted and residual at 313 s sit within 0.4 MiB of 0.2.15's T0 on all three.
M4 is a transient fix and only the transient moved.

`compile_duration` −0.129 s (−5.2 %) against 0.2.15's two readings, consistent
with the review's x86 `2_parse_rule_list` −14.3 %. Single sample, not a claim.

### Baseline

| Field | T0 |
| --- | --- |
| rules / duplicates_removed | 661 832 / 456 117 |
| ruleset_bytes | 21.00 MiB |
| accounted_bytes | 22.10 MiB |
| residual_bytes | 28.54 MiB |
| process_rss / peak | 50.64 / 85.28 MiB |
| allocator_committed (= peak) | 143.50 MiB |
| cache entries / bytes | 235 / 215.8 KiB |
| cache evictions | 0 |
| minor / major page faults | 34 681 / 13 |
| DNS pass / block | 444 / 431 |
| swr completed / failed | 6 / 0 |

Config unchanged from 0.2.15: `refresh_hours_default = 48`,
`history.sample_interval_seconds = 360`, `history.retention_days = 30`,
`stats.snapshot_interval_seconds = 300`.

### Boot

| Δ from start | Event |
| --- | --- |
| +2.400 s | refresh schedule restored from cached copies, `lists=17` |
| +2.401 s | ruleset compiled from cache, `rules=661832` |
| +2.414 s | DNS listeners bound, `udp`/`tcp [::]:53` |
| +2.418 s | privileges dropped, uid/gid 65532 |
| +2.421 s | API listening, SWR pool (3 workers) + cache cleanup started |

No refresh follows. All 17 lists are seeded, so boot pays one compile, not two.

## Verification

### Fix B — `last_refresh` survives a restart

`GET /api/v1/lists` on a 33-second-old process: **17 of 17 non-null**, all
`last_status: ok`, timestamps `2026-08-13T05:26:15–18Z` — the *previous*
process's fetch times, recovered from the cache mtimes. Under 0.2.15 the same
call after a restart returned `null` for all 17 beside `last_status: ok`.

The seeded stamps sit ~6 s earlier than 0.2.15 held in memory (05:26:21Z),
because the mtime is when the file was committed while `record_status` fired
after the compile. The commit time is the more honest of the two.

### Fix A — clock running

`POST /lists/refresh` returned at **2026-08-13T14:04:33Z**, `refreshed=17`. Under
0.2.15 this left the scheduler's due time untouched, so the next scheduled pass
fired at the *original* anchor. Fix A holds if no scheduled refresh fires before
**2026-08-15T14:04:33Z** — watched via `ruleset.rules` and the
`scheduled refresh complete` log line.

### Seeding survives a router reboot

The container starts 51 s after a router reboot, under a monotonic clock younger
than its cache files (age ≈ 7 h 49 m), and logs `refresh schedule restored from
cached copies lists=17` with no refresh following.

This retracts the caveat in
[boot-refresh-clock-and-orphan-sweep.md](../boot-refresh-clock-and-orphan-sweep.md)
§2 — see the retraction recorded there.

## Files changed

None — measurement only. The code is `d6a85c7` and earlier.

## Remaining TODOs

| Item | What flips it |
| --- | --- |
| Fix A | No scheduled refresh before 2026-08-15T14:04:33Z |
| Boot vs refresh disagreement | −33.93 vs −16.80 MiB for the same fix. Until explained, quote the refresh figure only |
| Bounded memory | Three diurnal cycles. Does the overnight +11 MiB step recur and get released, or does its floor ratchet |
| `allocator_committed` trend | 288.19 MiB after one refresh against 0.2.15's 385.38 MiB at 53 h — does it climb the same way |
| Instrumentation | `RssAnon`/`RssFile` land in `PerfSample` in the *next* build, so this run still infers arena retention rather than testing it. `allocator_committed_bytes` is deliberately **not** persisted: it is monotone under mimalloc v3 and can show a rise but never a release |
| M4's unattributed excess | Realloc instrumentation on-device, or accept it as unattributed |
