# p2-08 — worst-case URL lookup on the RB5009

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

**The conclusion survives the frequency caveat** (§Frequency below): the core
ran at 350 MHz throughout, and even granting the most generous correction to
the RB5009's 1.4 GHz nominal, 5,336 ÷ 4 = **1.33 ms — still over budget**. No
plausible clock assumption rescues it.

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

**It never clocked up.** 38 of 40 router samples at 350 MHz (two at 700, one at
466) while `cpu-load` held 25–28 % — one of four cores, exactly a single
pinned thread. The container's own reading of
`/sys/.../cpu0/cpufreq/scaling_cur_freq` corroborates it: 700 MHz entering the
run, 350 MHz leaving it.

Two independent supports for this being real rather than a RouterOS reporting
quirk:

1. A Cortex-A72 at a genuine 1.4 GHz against this desktop should land nearer
   4–5×, not the measured 9×. The 9× is consistent with an effective
   350–700 MHz.
2. Both the router's own counter and the container's kernel interface agree.

**This is a finding beyond p2-08: the RB5009 does not boost a single busy
core.** PERFORMANCE.md's budgets assume 4×ARMv8 at 1.4 GHz; any single-threaded
hot path on this device appears to run at a quarter of that. Whether it boosts
under all-cores load is untested and now a distinct open question — the 20k QPS
ceiling suggests it might.

## Where the cost actually goes

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
- **Nothing at 1.4 GHz.** Every ARM number here was taken at 350 MHz. If a
  future RouterOS or a loaded-all-cores workload does boost, these are upper
  bounds by up to 4×.
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
