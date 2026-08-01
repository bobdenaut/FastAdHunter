# p2-08 — worst-case URL lookup on the RB5009

> **Partly superseded by
> [`p2-10-url-substring-index.md`](p2-10-url-substring-index.md) (2026-08-01).**
> The verdict below — build the substring index — was acted on the same day and
> is now DONE: 8 KiB fell 5,335.7 → 553.8 µs on-device. Two of this report's
> supporting claims did **not** survive that work and are struck through where
> they appear: the "97 % of the lookup is the unindexed scan" cost model
> (§Where the cost actually goes) and the "no boost above 350–700 MHz"
> inference (§Frequency). The measured figures are untouched and still stand;
> what was wrong was reasoning built on top of them. Kept intact rather than
> rewritten, because both mistakes are instructive.

**Measured 2026-08-01 on the deployed RB5009** (RouterOS 7.21.5) with a
throwaway probe container. Raw evidence in [`p2-08-arm/`](p2-08-arm/):
`x86-reference.txt`, `rb5009-run.txt`, `rb5009-cpu-frequency.csv`.

Answers the question p2-08 was narrowed to: *what does a URL lookup cost in
the worst case — long URLs against rules no token could index — and does that
cost justify a substring index?*

## Verdict

**A substring index is required to meet the 1 ms lookup budget for the
EasyList + EasyPrivacy target corpus on the RB5009. It is not required for the
corpus this router currently runs.**

Both halves are load-bearing; neither is the whole answer.

| Corpus | URL rules | Unindexed | 8 KiB URL, RB5009 | vs 1 ms budget |
| --- | --- | --- | --- | --- |
| EasyList + EasyPrivacy — **the target corpus** | 18,781 | 77 | **5,336 µs** (p99 5,802) | ❌ **5.3× over** |
| The deployed lists — **this router today** | 714 | 3 | **377 µs** (p99 415) | ✔ 2.7× under |

Stating it unscoped in either direction would be wrong. "The URL tier is fine"
is true only of a deployment carrying three unindexed rules. "The URL tier
misses its budget" is true only at the corpus size the project targets. What
the measurement establishes is the **boundary between them**, and the boundary
is the unindexed-rule count — see §Where the cost actually goes.

**The conclusion does not depend on the clock at all** — which is just as well,
since §Frequency's reading of it turned out to be wrong. Even granting the most
generous correction to the RB5009's 1.4 GHz nominal, 5,336 ÷ 4 = **1.33 ms —
still over budget**. No clock assumption rescues it, and `p2-10` confirmed the
verdict by measurement rather than by argument.

## Two benchmarks, two questions

Running one corpus would have answered the wrong question, in either direction.

- **`easylist`** — EasyList + EasyPrivacy, fetched 2026-08-01. 18,781 URL
  rules, **77 unindexed**. Asks: *can the architecture carry a large URL
  corpus?*
- **`production`** — the deployed lists that actually contribute URL rules
  (`filter_1`, `filter_50`). 714 URL rules, **3 unindexed**. Asks: *what does
  this router see every day?*

The gap between them is the finding. The deployed corpus is overwhelmingly
DNS-shaped — 1,043,886 DNS rules against 714 URL rules, and 15 of the 17 lists
contribute **zero** URL rules. Whoever reads only the production number will
conclude the URL tier is free; whoever reads only the EasyList number will
conclude it is unusable. Both are true of different deployments.

> The other 15 lists were dropped from the probe image only after verifying
> they change nothing: the two-list subset compiles to the same 714 rules, same
> 3 unindexed, same 42,253-byte URL heap, with timings inside noise. That cut
> the image from 28 MB of corpus to 7.4 MB.

## Method

The same binary, the same corpora, the same arms on both sides — so the ratio
is a measurement rather than a comparison of two harnesses.

- **Harness:** `crates/fah-rules/examples/urlbench.rs`. Not criterion: the
  probe runs as the entrypoint of a distroless container on a router with no
  shell and no writable cwd, so results come back through RouterOS's log.
  Batched timing (one clock pair per ~1 ms batch) so `Instant::now()` overhead
  does not appear in a 3 µs measurement.
- **Arms:** identical to `benches/url_matcher.rs`, including its byte-identical
  `long_url()` generator.
- **Allocator:** mimalloc on both sides, matching the shipped binary. Lookup is
  allocation-free (`tests/url_lookup_alloc.rs` asserts it), so this only
  affects the reported parse/compile figures — but those would otherwise have
  been musl's default malloc, which production never pays.
- **x86:** core-pinned (`ProcessorAffinity = 4`, `PriorityClass = High`) per
  PERFORMANCE.md §Measuring reliably.
- **RB5009:** own container on its own veth, `cpu-list=cpu3`, no mounts, no
  ports. Corpora baked into the image — `/data` is mounted read-write by the
  production container and a probe has no business near the live cache. The
  production container was never stopped; DNS served throughout.

**Harness validation.** Against EasyList it reproduces the p2-03 review
exactly: 77 unindexed, 1.06 MiB URL-tier heap (the headroom model predicted
~1.03 MiB), 2.75 µs single-pass against the review's 3.09 µs. It is measuring
the same thing the review measured.

## Results

Minimum of ~5,000 batches — the cleanest estimate of true cost, since the
operation is deterministic and the spread is machine noise. The RB5009's p50
sits within 0.5 % of its min, which is what a quiet pinned core looks like.

### EasyList + EasyPrivacy — 77 unindexed

| Arm | x86 min | RB5009 min | RB5009 p99 | Factor |
| --- | --- | --- | --- | --- |
| `mixed_requests` | 13.17 µs | 127.3 µs | 139.4 | 9.66× |
| `single_pass_request` | 2.75 µs | 26.6 µs | 29.4 | 9.68× |
| `long_url_64b` | 4.00 µs | 35.0 µs | 39.4 | 8.74× |
| `long_url_1024b` | 54.88 µs | 452.7 µs | 492.3 | 8.25× |
| `long_url_4096b` | 251.2 µs | **2,091.9 µs** | 2,237.0 | 8.33× |
| `long_url_8192b` | 645.9 µs | **5,335.7 µs** | 5,801.6 | 8.26× |

### Deployed lists — 3 unindexed

| Arm | x86 min | RB5009 min | RB5009 p99 | Factor |
| --- | --- | --- | --- | --- |
| `mixed_requests` | 1.38 µs | 13.8 µs | 16.2 | 10.00× |
| `single_pass_request` | 0.36 µs | 3.35 µs | 3.6 | 9.31× |
| `long_url_64b` | 0.30 µs | 2.67 µs | 2.9 | 8.98× |
| `long_url_1024b` | 6.21 µs | 55.5 µs | 60.2 | 8.95× |
| `long_url_4096b` | 21.2 µs | 195.4 µs | 212.4 | 9.20× |
| `long_url_8192b` | 41.3 µs | 376.8 µs | 415.4 | 9.13× |

Zero budget exhaustions on either corpus — the p2-03 work allowance was never
reached, so none of these figures is a truncated match.

## The x86 → ARM factor

**8.25× – 10.0×, median ~9.05×**, across twelve arms spanning three orders of
magnitude and two corpora. Tight enough to use as a planning constant: a
figure measured on this dev box costs roughly **9× more on the RB5009**.

That constant is worth more than any single number here — it retires the need
to build a container for every future micro-question.

## Frequency — the trap this measurement nearly fell into

The 0.2.9 soak caught the router reporting `cpu-frequency: 350 MHz` against a
1.4 GHz nominal. If a probe finishes before the governor reacts, every figure
is up to 4× pessimistic and would "justify" an index nothing needs. So the
probe was sized to hold a core busy for ~45 s and the clock was sampled
throughout.

**During this run it never clocked up.** 38 of 40 router samples at 350 MHz
(two at 700, one at 466) while `cpu-load` held 25–28 % — one of four cores,
exactly a single pinned thread. The container's own reading of
`/sys/.../cpu0/cpufreq/scaling_cur_freq` agrees: 700 MHz entering the run,
350 MHz leaving it.

> ### ~~This is a finding beyond p2-08~~ — RETRACTED 2026-08-01 by `p2-10`
>
> This section went on to conclude that the RB5009 does not boost, that the
> ~9× factor was *explained by* an effective 350–700 MHz, and that every ARM
> figure here is therefore an upper bound by up to 4×. **All three are wrong.**
>
> The `p2-10` run of the same probe on the same router reported **1400 MHz**,
> sampled repeatedly during execution and returning to 350 MHz afterwards. So
> the governor does boost, and the two runs report clocks differing 4×.
>
> The decisive evidence is not a third frequency reading but a **control arm**:
> the deployed corpus at 8 KiB, which `p2-10`'s change barely touches. Across
> the two sessions it moved **376.8 → 359.6 µs, −4.6 %** — and −5.3 % on the
> x86 box, where the clock was fixed. A genuine 4× clock difference had to
> appear there as ~4×. It did not, so both runs executed at the same effective
> speed and RouterOS's frequency fields are not reporting what the workload got.
>
> The reasoning error was support #1 above: treating the ~9× factor as
> *evidence for* a low clock, when it is simply the measured ratio between two
> machines and needs no clock story at all. Support #2 — "both counters agree"
> — established only that they share a source, not that the source is
> meaningful.
>
> What survives, and is now stronger for having been tested twice: **the ~9×
> x86 → RB5009 factor.** Two sessions, two code versions, reported clocks
> differing 4×, ratios of 8.26/9.51 then 9.92/9.59. Calibrate with the factor;
> report the frequency as a condition, never as an input.
>
> Whether all-cores load behaves differently remains untested and open.

## Where the cost actually goes

> **~~The model below is wrong~~ — RETRACTED 2026-08-01 by `p2-10`.** It fits
> cost against unindexed-rule count across two corpora that also differ **26× in
> total rule count** (18,781 against 714), so every difference between them —
> candidate volume most of all — is charged to the one variable the model names.
> Measured directly by indexing all 77 rules and changing nothing else, the
> unindexed term was **32 %** of an 8 KiB lookup, not 97 %. The self-warning
> below ("a model from two points, not a measurement") was correct and was not
> heeded; the fix is a **control arm**, not a caveat.
>
> The rest was 124 candidate rules each scanning the URL for their first byte a
> byte at a time, ~3 µs apiece at 8 KiB, against 4,983 index probes costing
> ~10 ns each. SIMD removed it. Kept below as written.

Two corpora at four URL lengths permit a two-point fit of cost against
unindexed-rule count. **A model from two points, not a measurement** — but the
shape is unambiguous.

| URL length | Fixed term | Per unindexed rule | At 77 rules: share that is the unindexed scan |
| --- | --- | --- | --- |
| 64 B | 1.4 µs | 0.44 µs | 96 % |
| 1 KiB | 39.4 µs | 5.37 µs | 91 % |
| 4 KiB | 118.5 µs | 25.6 µs | 94 % |
| 8 KiB | 176 µs | 67.0 µs | **97 %** |

The per-rule term is ~6–8 ns per URL byte per unindexed rule at every length —
i.e. each unindexed rule is scanned against the whole URL, which is exactly
what the design says it does.

**What a substring index would buy.** It removes the per-rule term, leaving the
fixed one: 8 KiB at EasyList scale would fall from **5,336 µs to ~176 µs**, a
~30× improvement, and land comfortably inside the budget. That is the
strongest available argument for building it — the cost is not spread thinly
across the lookup, it is 97 % concentrated in one term that an index deletes.

It also bounds the ambition: ~176 µs is the floor at 8 KiB even with a perfect
index, because tokenization and indexed-candidate checks scale with URL length
too. An index makes the budget comfortable; it does not make long URLs free.

## What this does not settle

- **Nothing about the HTTP pipeline end to end.** This measures
  `Matcher::lookup_http` in isolation. Connection handling, header parsing and
  the second hop are p2-02/p2-04 figures, measured on x86 only.
- ~~**Nothing at 1.4 GHz.** Every ARM number here was taken at 350 MHz.~~
  Retracted — see §Frequency. The reported clock does not describe what the
  workload got, and a control arm shows a run reporting 1400 MHz performed the
  same. Whether an all-cores workload behaves differently is still open.
- **Long URLs are ordinary traffic, not an attack.** OAuth redirects, ad-tech
  beacons and analytics payloads routinely carry multi-KB query strings, so the
  8 KiB arm is a realistic tail, not a synthetic worst case. The p2-03 work
  allowance separately caps the genuinely adversarial case.

## Reproducing

```sh
# corpora: EasyList + EasyPrivacy, and the router's URL-bearing lists
scp -r bobdenaut:kingston/fastadhunter/data/lists/. ./corpus/

# x86 reference, core-pinned
cargo run --release -p fah-rules --example urlbench -- ./corpus-easylist ./corpus-prod

# arm64 probe image (legacy docker-archive — OCI layout hangs RouterOS forever)
docker buildx build --platform linux/arm64 -f Dockerfile.probe \
  -t fah-urlbench:1 -o type=docker,dest=fah-urlbench-arm64.tar .
docker run --rm -v "$PWD:/work" quay.io/skopeo/stable copy --insecure-policy \
  oci-archive:/work/fah-urlbench-arm64.tar \
  docker-archive:/work/fah-urlbench-rosready.tar:fah-urlbench:1
```

`.probe-corpus/` and the tars are gitignored: EasyList is GPLv3, 2 MB, and
changes daily. `Dockerfile.probe` documents the full router-side sequence.
