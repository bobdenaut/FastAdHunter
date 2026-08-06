# p2-08 — Phase 2 verification

**2026-08-06.** Closes `p2-08-phase2-verification.md`. Raw evidence in
[`p2-08-http-arm/`](p2-08-http-arm/): `rb5009-runs.txt` (4 runs),
`x86-reference.txt` (4 runs).

## Verdict

**Both HTTP budget rows hold on the RB5009, and the dev box was optimistic by
4.4×.** A `dns+http` proxy adds **+161 µs** (min) / **+344 µs** (p50) to a
request against a 1 ms budget, and relays opaque bodies at **271 MiB/s** (min) /
**208 MiB/s** (p50) against a 100 MiB/s target — gigabit is 119 MiB/s, so a
single connection carries line rate with headroom. **A blocked request costs
about half a forwarded one** (152 vs 294 µs at min): it never resolves and never
opens an upstream connection. That is p2-04's design claim, measured on the
target CPU rather than argued from a dev box.

The other three acceptance criteria closed earlier and are recorded elsewhere:
the URL-tier sweep in [`p2-08-url-lookup-arm.md`](p2-08-url-lookup-arm.md) and
[`p2-10-url-substring-index.md`](p2-10-url-substring-index.md), the soak in
[`p2-07-review.md`](p2-07-review.md) §12, dst-nat and the deployed baseline in
[`0.2.10-soak-baseline.md`](0.2.10-soak-baseline.md).

## Decisions

- **Report `min`, `p50` and `p99` together, never one alone.** `min` is the
  intrinsic cost, `p50` what a user typically experiences, `p99` the tail. On
  this device the added cost at p50 is **2.1× the min**, so a doc quoting only
  `min` would claim 6.2× headroom where the user sees 2.9×.
- **The measurement is a probe container, not generated load over the deployed
  path.** The soak established the deployed path cannot supply these numbers —
  ~8 intercepted connections per 6 min, no control over body size, content type
  or origin RTT (`0.2.10-soak-baseline.md` §Remaining TODOs). The probe runs
  client, proxy and origin on the container's own loopback, which isolates
  FastAdHunter's cost and excludes veth, dst-nat, conntrack and origin RTT.
- **Median of four runs.** RouterOS cannot pin container CPU affinity
  (`docs/routeros-traps.md`), and production served DNS throughout, so a single
  run cannot separate an 8 % effect from noise. `min` reproduced within ±3 %
  across the four.
- **The ~9× x86 → RB5009 factor is not used for these rows.** It does not apply
  here — see §Findings.
- **The probe ships in-tree** (`crates/fah-http/examples/httpbench.rs` +
  `Dockerfile.httpprobe`), like `urlbench`, so the arms can be re-run against a
  future change instead of being rebuilt from scratch.

## Measurements — RB5009, median of 4 runs

715 URL rules (the deployed corpus), mimalloc, production FAH 0.2.10 serving
DNS concurrently. Percentages are against the direct arm.

| arm | min | p50 |
| --- | ---: | ---: |
| head direct | 132.7 µs | 238.5 µs |
| head proxied | 294.2 µs | 582.1 µs |
| **head, added** | **+161.5 µs (+122 %)** | **+343.6 µs (+144 %)** |
| head proxied, ruleset attached | 302.1 µs | 607.8 µs |
| head blocked | 151.7 µs | 261.7 µs |
| 8 KiB direct → proxied | 137.6 → 317.9 µs | 222.5 → 612.6 µs |
| **8 KiB, added** | **+180.3 µs (+131 %)** | **+390.2 µs (+175 %)** |
| 1 MiB direct → proxied | 2.150 → 3.696 ms | 2.974 → 5.037 ms |
| **1 MiB, added** | **+1.55 ms (+72 %)** | **+2.06 ms (+69 %)** |
| 8 MiB direct → proxied | 14.82 → 25.53 ms | 18.31 → 30.76 ms |
| **8 MiB, added** | **+10.71 ms (+72 %)** | **+12.46 ms (+68 %)** |

**Against budget:**

| budget | p50 | min |
| --- | --- | --- |
| head-path added latency < 1 ms | 344 µs — **2.9× under** | 162 µs — **6.2× under** |
| opaque throughput ≥ 100 MiB/s, 1 MiB | 208 MiB/s — **2.08×** | 271 MiB/s — **2.71×** |
| opaque throughput ≥ 100 MiB/s, 8 MiB | 260 MiB/s — **2.60×** | 313 MiB/s — **3.13×** |

**Blocked vs forwarded:** 152 vs 294 µs (min), 262 vs 582 µs (p50) — a blocked
request is **48–55 % cheaper** than a forwarded one. It is *not* a claim that
blocking makes the router faster in absolute terms; total load still rises with
traffic. Each blocked request simply costs less than a forwarded one.

**Tail.** Proxied head p99 is **1.24–1.28 ms** absolute across the four runs
against a direct-arm p99 of 0.47–0.64 ms, so the added component at p99 is
**≈660–700 µs**. Blocked requests stay under **600 µs** at p99. Note the p99 of
a difference is not the difference of two p99s — the arms run at different times
against different noise, so ≈680 µs is an estimate of the added tail, not a
measured one.

**Two readings of the percentages, both true.** The denominator is a loopback
fetch — the fastest possible baseline, which makes the ratio the harshest
possible reading. Against a real origin at 10–50 ms RTT the same +344 µs is
under 3 % of the request. Quote the ratio with that sentence attached.

## Findings

**1. The ~9× x86 → RB5009 factor does not apply to this bench.** Per-arm ratios
(median x86 min → median ARM min):

| arm | ratio | arm | ratio |
| --- | ---: | --- | ---: |
| 8 KiB direct | 4.55 | head + rules | 5.35 |
| head blocked | 4.81 | 8 KiB proxied | 5.42 |
| head direct | 4.89 | 8 MiB direct | 6.31 |
| head proxied | 5.24 | 8 MiB proxied | 7.11 |
| | | 1 MiB direct / proxied | 9.93 / 10.09 |

A 2.2× spread, against the flat 8.25–10.0× the URL-lookup probe found across
twelve arms. Those arms were pure CPU; these are syscall- and copy-bound, and
the dev box is Windows while the target is Linux. **The factor stays valid for
CPU-bound work and must not be used to convert HTTP figures** — PERFORMANCE.md
now says so.

**2. The verdict's cost is below this bench's resolution on-device.** Attaching
the ruleset moved the head arm by **+23.7, −4.6, +3.7, +17.8 µs** across the four
runs: the sign flips, exactly as the 1 MiB criterion arm did when it reported
proxied faster than direct. Median is +7.9 µs; the honest statement is
**unresolvable, bounded at roughly ±20 µs**. The x86 figure — **+0.45 to
+0.71 µs** over four runs, tight enough to quote — is the only usable one, and
it is consistent with p2-10's on-device `single_pass_request` of 3.06 µs.

**3. Third corroboration that RouterOS's CPU-frequency fields are not a
calibration input.** Reported `scaling_cur_freq` across the four runs: 350→700,
1400, 350, 700 MHz. Every arm agrees within 6 %. A genuine 4× clock difference
had to show ~4×. This is tighter evidence than the cross-session comparison in
`p2-10-url-substring-index.md` §Retractions, because it is the same probe minutes
apart on an otherwise unchanged device.

## Files changed

| File | Change |
| --- | --- |
| `crates/fah-http/examples/httpbench.rs` | New — on-device probe, 10 arms, same harness as `fah-rules/examples/urlbench.rs` |
| `crates/fah-http/Cargo.toml` | `mimalloc` dev-dependency, example-only |
| `Dockerfile.httpprobe` | New — arm64 throwaway image, deployed URL corpus baked in, mounts nothing |
| `PERFORMANCE.md` | Two HTTP budget rows carry on-device min/p50/ratio; the factor caveat added |
| `docs/code-review/p2-08-http-arm/` | Raw logs, 4 ARM runs + 4 x86 runs |

Gates green — `fmt`, `clippy -D warnings`, 779 tests.

## Remaining TODOs

- **Concurrency is untested.** Every figure here is one connection at a time.
  `[http] max_connections` defaults to 1024 and nothing measures 50 or 500 in
  flight. The DNS side has load evidence (20 k+ QPS on-device); the HTTP side has
  none, and Phase 3 multiplies the question by TLS.
- **The deployed path still rests on 5 requests** (the T0 baseline). The probe
  deliberately excludes veth, dst-nat, conntrack and origin RTT. A curl loop from
  a LAN host against an IPv4-only plain-HTTP origin would give it a real sample —
  function and end-to-end distribution, not the delta above.
- **HTTP interception is IPv4-only.** Mirror `/ipv6/firewall/nat` rules are
  written and unapplied; the soak deferral has expired
  (`0.2.10-soak-baseline.md` §Known gap).
- The probe container and its tar are removed from the router after each use;
  nothing on-device persists from this task.
