# P2-07 — Phase 2 Verification

**Phase:** 2 · **Depends on:** p2-07 · **Model:** Opus

## Goal

Phase 2 proven: HTTP budgets defined and met, end-to-end tests cover the new
surface, dns+http validated on the RB5009.

## Context

PERFORMANCE.md has no HTTP numbers yet — this task sets them (doc update),
then proves them. On-device steps need the user (RouterOS dst-nat rule for
port 80 → container).

## Scope

- PERFORMANCE.md: add HTTP budget rows — pass-through added latency p99,
  request-verdict latency, proxied throughput, RAM ceiling. **Derive every row
  from measured p2-02/p2-03 data; do not carry a target in from ambition.**
- **Throughput is two rows, not one, because the body is never parsed.**
  Images, ZIPs, PDFs, video, fonts, any non-HTML body: the verdict is taken on
  the *head*, then the bytes are streamed through untouched — no parsing, no
  buffering, no rewriting, ever. That is the design rule, and it is what makes
  a high number achievable at all:
  - **Opaque body pass-through** — pure relay after the head verdict. This is
    the row that can plausibly approach line rate, and the bench must confirm
    the body path performs no per-byte work beyond the copy.
  - **Inspected content** — HTML only, and only from Phase 4. Budget it
    separately and do not let it set expectations for the row above.

  Measure both on-device before writing either number; the earlier
  "saturate 1 Gbps" note was an assumption, and 125 MB/s through userspace on
  this CPU — a box where the DNS engine alone reached 65.8 % of it under load
  (`p1.5-06-review.md`) — is exactly the kind of target that should come from a
  measurement rather than produce one. **That target is now less plausible, not
  more:** at the measured ~9× x86 factor, a throughput figure taken on the dev
  box needs dividing by nine before it means anything here.
- **RAM ceiling ≤128 MB is a claim to verify, not assume.** State the
  post-Phase-2 figure with EasyList + policies loaded and say plainly whether
  it fits. If p2-03 already flagged the headroom, this row confirms or
  contradicts it — either outcome gets recorded.
  Note the ~104 MiB figure this row used to cite was **0.2.3**; measured on
  0.2.4 the steady RSS is **~41.7 MiB** after 10 h of household traffic
  (`docs/code-review/p1.5-09-soak-baseline.md`), so the headroom is far larger
  than the task originally assumed. Re-measure rather than inheriting either
  number.
- **Report the soak in components, not just RSS** (`p2-07`). A soak that only
  has RSS can say "it grew"; with `fastadhunter_memory_component_bytes` and the
  residual it can say *which* structure grew, and a flat residual is what
  actually retires the leak question.
- Bench consolidation: HTTP benches map 1:1 to the new budget rows.
- End-to-end (offline): mock origin + real binary in dns+http mode —
  page with ad script: script blocked (200-empty), page renders; second
  client under stricter policy: page domain itself blocked; WS stream shows
  both kinds of events.
- RB5009 (with the user): document + apply dst-nat rule, browse plain-HTTP
  site through it, verify filtering + pass-through speed, extend
  docs/deploy-rb5009.md with the HTTP section + rollback (drop the nat rule).
- Soak: 24h dns+http; RAM/latency/QPS recorded vs updated budgets.
- ~~**Long-URL verdict latency on-device — this task owns the p2-03
  deferral.**~~ **MEASURED 2026-08-01 —
  [`docs/code-review/p2-08-url-lookup-arm.md`](../../../docs/code-review/p2-08-url-lookup-arm.md).**
  Run ahead of the rest of this task, because the answer decides whether a
  substring index belongs in the phase at all.

  **Outcome: a substring index was required, and has since been built and
  verified on-device — see `p2-10`.** The measurement found 8 KiB against
  EasyList + EasyPrivacy at 5,336 µs, 5.3× over budget, while the corpus this
  router runs cost 377 µs. `p2-10` closed that: every URL rule is now indexed
  and 8 KiB is **553.8 µs** (p99 569.5), inside budget at every measured length
  (`docs/code-review/p2-10-url-substring-index.md`).

  The rest of this task's on-device work (dst-nat, `dns+http` browsing, soak)
  is **not** done and remains as written below.

## Acceptance criteria

- Updated PERFORMANCE.md budget table fully bench-backed; dev numbers meet it.
- On-device: no regression on DNS soak numbers; HTTP pass-through
  imperceptible in normal browsing (user confirms); numbers recorded.
- ✅ **MET — worst-case URL-tier lookup latency measured on the RB5009.** The
  `long_url_*` sweep (64 B / 1 KiB / 4 KiB / 8 KiB) ran on-device against both
  corpora via a throwaway probe container; production was never stopped. Numbers
  and raw evidence in
  [`docs/code-review/p2-08-url-lookup-arm.md`](../../../docs/code-review/p2-08-url-lookup-arm.md)
  + `docs/code-review/p2-08-arm/`.

  8 KiB did **not** leave comfortable headroom at target-corpus scale, which
  re-opened the deferred substring-index work. That is now `p2-10`, DONE and
  verified on-device — 8 KiB fell 5,335.7 → 553.8 µs and `unindexed` is 0 on
  both corpora.

  **The x86 → ARM ratio came out FLAT: 8.25–10.0× across twelve arms spanning
  three orders of magnitude and two corpora (median ~9.05×).** Per this
  criterion's own rubric that is the first branch — the gap is plain CPU
  throughput, so the x86 profile in `docs/code-review/p2-04-review.md` transfers
  directly, and the ~9× is usable as a planning constant rather than only a
  diagnostic. Memory bandwidth is *not* the binding constraint on device.

  Two findings the criterion did not anticipate, both recorded in the report:

  - **The reported CPU frequency is not a calibration input.** This run sampled
    350–700 MHz throughout; the `p2-10` run of the same probe reported
    1400 MHz. A control arm across the two — the deployed corpus at 8 KiB —
    moved −4.6 %, where a genuine 4× clock change had to show ~4×. Both runs
    therefore executed at the same effective speed. What survives is the ~9×
    x86 → RB5009 factor, corroborated by both sessions.
  - ~~**97 % of an 8 KiB lookup is the unindexed scan**, ≈ 176 µs fixed +
    67 µs per unindexed rule.~~ **Wrong — retracted by `p2-10`.** That model
    was a two-point fit across corpora differing 26× in rule count, so it
    charged the whole gap to the unindexed term. Removing that term alone moved
    **32 %**, not 97 %; the rest was candidate rules scanning the URL for their
    first byte one byte at a time.

  x86 reference for the record (EasyList + EasyPrivacy, pinned, mimalloc,
  2026-08-01, minimum of ~5,000 batches): **4.00 µs** at 64 B, **54.9 µs** at
  1 KiB, **251 µs** at 4 KiB, **646 µs** at 8 KiB (p99 992 µs) — already at
  budget on the fast box, scaling slightly super-linearly (8× length → 11.8×
  time).
- Gates green.

## Out of scope

HTTPS (Phase 3), HTML rewriting (Phase 4).

## Suggested prompt

> Read plan/wip/phase2/p2-08-phase2-verification.md and PERFORMANCE.md.
> Set the HTTP budget rows from bench data, consolidate benches, write the
> offline e2e scenarios, then walk the RB5009 dst-nat setup and soak WITH the
> user and record results.
