# p2-10 — URL substring index, and two retractions

**2026-08-01.** Closes the substring-index work re-opened by
[`p2-08-url-lookup-arm.md`](p2-08-url-lookup-arm.md). Raw evidence in
[`p2-10-arm/`](p2-10-arm/): `x86-before.txt`, `x86-after.txt`,
`rb5009-run.txt`.

## Verdict

**The URL-tier budget is met at every measured URL length.** Every rule in both
corpora is now indexed — `unindexed` is **0**, from 77 — and the worst case
measured on the deployed RB5009 fell **5,335.7 → 553.8 µs** against a 1 ms
budget, for **+2,372 bytes** of heap.

| RB5009, EasyList + EasyPrivacy | before | after | × |
| --- | --- | --- | --- |
| mixed_requests | 127.269 µs | 38.029 µs | 3.3 |
| single_pass_request | 26.586 µs | 4.435 µs | 6.0 |
| 64 B | 34.984 µs | 9.457 µs | 3.7 |
| 1 KiB | 452.700 µs | 64.149 µs | 7.1 |
| 4 KiB | 2,091.920 µs ❌ | 249.693 µs | 8.4 |
| **8 KiB** | **5,335.680 µs** ❌ | **553.760 µs** ✔ | **9.6** |
| 8 KiB p99 | 5,801.600 µs | 569.520 µs | 10.2 |

x86, pinned, minimum of ~5,000 batches — the clean before/after, since both
arms ran on the same box under the same conditions:

| x86 | before | after | × |
| --- | --- | --- | --- |
| 64 B | 4.004 µs | 1.038 µs | 3.9 |
| 1 KiB | 54.875 µs | 6.058 µs | 9.1 |
| 4 KiB | 251.167 µs | 24.911 µs | 10.1 |
| **8 KiB** | **645.900 µs** | **55.807 µs** | **11.6** |
| mixed_requests | 13.170 µs | 4.083 µs | 3.2 |

The deployed corpus, which this change barely touches, is the control arm and is
used as such below: 376.8 → 359.6 µs on-device, 39.617 → 37.504 µs on x86.

## What the 77 rules actually were

p2-08 called them "unindexed" and inferred they were unindexable. They were not
— the *tokenizer* was destroying them. A token is a run of `[a-z0-9_%]`, so a
pattern is filed only if some token survives at 3 bytes or more:

```text
"/fp/es.js"   tokens: fp, es, js      all 2 bytes
"t.co^"       tokens: t, co           all <= 2 bytes
"0.0.0.0^"    tokens: 0, 0, 0, 0      all 1 byte
"/oo/cl.js"   tokens: oo, cl, js      all 2 bytes
```

Every one of these carries plenty of literal text. What it does not carry is
literal text *between separators*. `/fp/es.js` is four short tokens and one
**nine-byte literal run**.

So the fallback tier keys on **literal runs** — maximal stretches containing
neither `*` nor `^`, the only two metacharacters — whose every byte must appear
verbatim, in order and contiguously, in any URL the pattern matches. Runs span
`/`, `.` and `?` precisely because tokens do not. That files all 77.

## Three changes, in order of what they bought

1. **Literal-run n-gram tier** (`KeyKind::Ngram`). 645.9 → 441.2 µs on x86.
   Consulted only when no bounded token key exists, so the 18,704 rules that
   already had one are untouched and still cost one probe per URL *token*.
2. **`memchr` for the unanchored first-byte scan.** 441.2 → 58.2 µs — the
   largest single win, and not an indexing change at all.
3. **`*`-widening skips to the next literal** instead of retrying every offset,
   plus restricting the two-needle scan to lowercase letters (§Case, below).
   58.2 → 55.8 µs.

### What pays for the sweep

A run-window key is reachable only by sliding, so a lookup rolls a 24-bit window
along the **whole URL** — not per token, since a run spans separators. Two
things keep that affordable:

- **A Bloom prefilter**, indexed by the packed window rather than its hash:
  folding 24 bits costs no multiply where the hash costs two, and nearly every
  window matches nothing. Empty when the tier is empty, which is also the flag
  that skips the sweep entirely. Measured cost of the whole sweep at 8 KiB:
  **~2.4 µs**.
- **A per-lookup visited bitmap.** A window can land on the same key hundreds of
  times in an 8 KiB URL, and each hit would otherwise cost a full O(url) match
  per rule in the bucket. N-gram buckets sort last in the CSR so this can be a
  fixed 1,024-bit stack array; overflow past it is demoted to the scan rather
  than dropped, and a test pins that.

### Case

`byte_matches` folds the *text* byte, so `tb.to_ascii_lowercase() == pb` can
only hold for two bytes when `pb` is **lowercase**. A separator or digit is
matched by itself alone, and an uppercase `pb` under a case-insensitive rule is
matched by nothing at all. An unconditional
`memchr2(pb, pb.to_ascii_uppercase())` therefore hands the two-needle scan every
`/` and `.` — most first bytes in a real ruleset — for no benefit.

## Two retractions from p2-08

Both were reasoning built on correct measurements. Neither was caught by a
test, because neither was a code defect.

### 1. "97 % of an 8 KiB lookup is the unindexed scan"

The model — ≈176 µs fixed + 67 µs per unindexed rule — was a two-point fit
across two corpora that differ **26× in total rule count** (18,781 against 714).
Every difference between them, candidate volume most of all, was charged to the
one variable the model named. The report's own caveat, *"a model from two
points, not a measurement"*, was correct and was not heeded.

Measured directly, by indexing all 77 rules and changing nothing else:
**645.9 → 441.2 µs on x86 — 32 %, not 97 %.**

Instrumenting what remained found **124 candidate checks against 4,983 index
probes** at 8 KiB. The probes cost ~10 ns each; the checks cost ~3 µs each,
which is exactly an 8 KiB scalar byte-scan. The cost was never the index. It was
every candidate rule looking for its own first byte one byte at a time.

**The fix for the method, not just the number: a control arm.** A caveat on a
two-point fit does not make it safe. One arm the change barely touches,
measured in the same session, is what turns a comparison into evidence.

### 2. "The RB5009 does not boost; every ARM figure is an upper bound"

p2-08 sampled 350–700 MHz throughout, 38 of 40 samples at 350, corroborated by
the container's own `scaling_cur_freq`. This session's run of the same probe on
the same router reported **1400 MHz**, sampled repeatedly during execution and
returning to 350 MHz after the workload completed.

The decisive evidence is not a third frequency reading but the **control arm**:

| deployed corpus, 8 KiB | before | after | Δ |
| --- | --- | --- | --- |
| RB5009 (reported 350 MHz → 1400 MHz) | 376.820 µs | 359.600 µs | −4.6 % |
| x86 (clock fixed) | 39.617 µs | 37.504 µs | −5.3 % |

A genuine 4× clock difference had to appear there as ~4×. It appeared as
−4.6 %, matching the x86 delta where the clock did not move. Both runs executed
at the same effective speed, and RouterOS's `cpu-frequency` / `scaling_cur_freq`
fields do not report what the workload got.

The specific error was treating the ~9× x86→ARM factor as *evidence for* a low
clock. It is a ratio between two machines and needs no clock story. The second
support — "both counters agree" — established only that they share a source.

**What survives, stronger for being tested twice: the ~9× factor.** Two
sessions, two code versions, reported clocks differing 4×, ratios of 8.26 and
9.51, then 9.92 and 9.59. The drift toward 9.9 on EasyList is plausibly SIMD —
AVX2 scans 32 B/cycle against NEON's 16 — but that is a hypothesis, not a
measurement.

## Correctness

The property the tier rests on is unchanged and still enforced: **the index must
decide exactly what a full scan decides.** Both oracles pass — the fixture list
and the 200-round randomised-ruleset comparison against `lookup_scanning`.

New tests:

- every long-enough literal run is indexed; only a pattern with no run of
  `MIN_TOKEN_LEN` falls back to the scan
- a two-sided-unbounded token is reached at any offset, and a separator
  correctly breaks the run
- a bounded key is preferred over an n-gram key
- a repeated n-gram does not re-check the same rule
- overflow past `NGRAM_BUCKET_CAP` demotes to the scan without losing rules
- all four key kinds stay clear of each other in the shared table

One existing test changed meaning and was rewritten rather than deleted:
`a_pathological_pattern_cannot_spend_an_unbounded_lookup`. The rarest-key rule
now files that pattern under `aab`, which its attack URL does not contain — so
the index never reaches it and the work allowance is never exercised. That is a
real improvement, but relying on it would have left the allowance untested, so
the URL now holds both windows the pattern offers.

`unsafe`: none. Allocation on the hot path: none (`tests/url_lookup_alloc.rs`
still passes; the visited set is a stack array).

## Gates

`cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`,
`cargo test --workspace` — **705 passed, 0 failed**.

## What this does not settle

- **Nothing about the HTTP pipeline end to end.** This measures
  `Matcher::lookup_http` in isolation.
- **All-cores behaviour is still untested**, including whether the frequency
  fields mean anything under that load.
- **The n-gram key choice is frequency-ranked across the n-gram tier only**,
  which is a weak signal at 77 rules. It was good enough here — the sweep costs
  ~2.4 µs at 8 KiB — but a corpus that puts a URL-common window like `.js` on a
  hot rule would pay one candidate check per lookup for it. Not observed; worth
  re-checking if the tier ever grows by an order of magnitude.
- **`NGRAM_BUCKET_CAP` is 1,024**, ~13× the measured need. Overflow degrades to
  the pre-existing scan, so the failure mode is the old behaviour rather than a
  wrong answer.
