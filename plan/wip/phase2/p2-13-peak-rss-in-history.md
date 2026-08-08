# P2-13 — Expose `peak_rss` in History

**Phase:** 2 · **Depends on:** [`p2-12`](p2-12-compile-transient-structural.md)
(which attributed the transient and took no lever) · **Model:** Opus

## Goal

Make the compile peak observable over time. `p2-11` moved it unnoticed and
`p2-12` closed with the levers sized and untaken — both rest on a figure that no
persisted series carries.

**This task adds no compile optimisation.** The parsed-form amplification, the
one-text-at-a-time read and the pre-sizing stay unbuilt. A guard comes first
because optimising a number nobody can observe is the wrong order.

## Why this is the cheap guard

`peak_rss` comes from `getrusage(ru_maxrss)` and is a **monotone high-water
mark**. It therefore does not need to be sampled *during* the 2.75 s compile to
capture it — a 360 s sampler still records the step, only later. That is what
makes a guard possible at all: `p2-12`'s method note rules out catching a
compile by sampling RSS, and this field sidesteps it.

Decimation preserves the property. `Decimator` keeps every Nth point, and a
monotone series survives that with its steps intact — a peak can be delayed in
the series, never erased.

## Work

The value is already in hand at every step; nothing new is read or plumbed.

| # | Change | Where |
| --- | --- | --- |
| 1 | `PerfSample` gains `peak_rss: u64`, `#[serde(default)]` | `fah-model/src/perf.rs` |
| 2 | Populate from the `MemoryBreakdown` the function already takes — `memory.process.map(\|p\| p.peak_rss)` | `fastadhunter/src/main.rs`, `build_perf_sample` |
| 3 | `PerfSampleResponse.peak_rss: Option<u64>` with `skip_serializing_if` | `fah-api/src/wire.rs` |
| 4 | `PerfFields`: field, `ALL`, `NONE`, `NAMES` (`[_; 10]` → `[_; 11]`), `enable` | `fah-api/src/wire.rs` |
| 5 | `HistoryPerfResponse::new` maps it like the others | `fah-api/src/wire.rs` |

No new syscall: `build_perf_sample` already receives `&MemoryBreakdown`, whose
`process: Option<ProcessStats>` carries the field. No storage format change —
the rows are JSONL and `serde(default)` reads existing ones back as 0.

**Follow `minor_page_faults` exactly**: `u64` in the model, `Option` on the wire.
0 means "row predates this task, or `getrusage` is unavailable" — not "the peak
was zero". Do not invent a second convention for the same situation.

## Criteria

- [ ] A row written before this task deserialises, with `peak_rss` 0 and every
      other field unchanged.
- [ ] `?fields=peak_rss` serves it alone; an unknown name still lists the
      accepted set back, now with 11 entries.
- [ ] The series is non-decreasing within one container lifetime, and a restart
      is the only thing that resets it. State this where the field is
      documented — a reader who does not know it will misread a restart as a
      drop.
- [ ] Retention cost stated. One `u64` plus its JSON key is ~20 B/row: at 60 s
      ≈ +0.9 MB/30 days, at the deployed 360 s ≈ +0.15 MB, against p2-07's
      ~6.9 MB/30 days at 60 s.
- [ ] Gates green.
- [ ] Deployed and the series read back on-device — **after the 0.2.13 soak
      ends**, since a deploy restarts the container and zeroes the counters the
      soak is accumulating.

## Out of scope

- Every compile lever in `p2-12` — items 1, 2, 3 and the list-order fourth.
  Open a separate task if the series ever justifies one.
- A threshold, alert or automated fail. This makes the peak *visible*; deciding
  what value is too high needs the series to exist first.
- The TUI. Displaying it is worth doing and is not this task — the guard is the
  persisted series, and `/history/perf` is what a reader queries.

## What would flip the decision p2-12 recorded

The peak scales with the **largest single list**, not the corpus total —
`big.oisd.nl` alone is 56.51 of the 106.61 MB dev-box transient. Roughly double
that list and the peak goes ~180 → ~237 MB on a box sharing 1 GB with RouterOS.
This series is what shows it happening.
