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
  this CPU — nominally 1.4 GHz but measured running single-threaded work at
  350–700 MHz, on a box where the DNS engine alone reached 65.8 % of it under
  load (`p1.5-06-review.md`) — is exactly the kind of target that should come
  from a measurement rather than produce one. **That target is now less
  plausible, not more:** at the measured ~9× x86 factor, a throughput figure
  taken on the dev box needs dividing by nine before it means anything here.
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

  **Outcome: a substring index is required to meet the 1 ms budget for the
  EasyList + EasyPrivacy target corpus on the RB5009 (8 KiB = 5,336 µs, 5.3×
  over), and is not required for the corpus this router currently runs
  (377 µs).** The boundary is the unindexed-rule count — 77 against 3 — not the
  URL-rule count. Even 4 KiB against EasyList is 2,092 µs.

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

  8 KiB does **not** leave comfortable headroom at target-corpus scale, so the
  deferred substring-index work is **re-opened** — see
  `plan/wip/phase2/CLAUDE.md` §"Follow-up from the p2-03 review".

  **The x86 → ARM ratio came out FLAT: 8.25–10.0× across twelve arms spanning
  three orders of magnitude and two corpora (median ~9.05×).** Per this
  criterion's own rubric that is the first branch — the gap is plain CPU
  throughput, so the x86 profile in `docs/code-review/p2-04-review.md` transfers
  directly, and the ~9× is usable as a planning constant rather than only a
  diagnostic. Memory bandwidth is *not* the binding constraint on device.

  Two findings the criterion did not anticipate, both recorded in the report:

  - **During the measurements a single busy core remained at 350–700 MHz and no
    boost to the nominal 1.4 GHz was observed.** 45 s of a pinned core at 100 %
    (38/40 router samples at 350; the container's own `scaling_cur_freq`
    agrees), against the 1.4 GHz PERFORMANCE.md assumes. Every ARM figure here
    is therefore an upper bound — and the verdict survives correcting all the
    way to nominal (5,336 ÷ 4 = 1.33 ms, still over budget).
  - **97 % of an 8 KiB lookup is the unindexed scan.** Cost ≈ 176 µs fixed +
    67 µs per unindexed rule, so an index would take 5,336 µs → ~176 µs (~30×).
    That ~176 µs is also the floor with a *perfect* index, since tokenization
    scales with URL length too.

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
