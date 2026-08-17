# p2-05 — Policy model

**2026-08-01.** Policy, Schedule and Assignment become real; `$client`
activates. Enforcement in the two pipelines is p2-06 and deliberately not here.

## Verdict

**The memory risk the task flagged does not materialise, and the margin is not
close.** Policies cost a per-rule mask array — ~2 MiB at deployed scale, *flat*
in the number of policies — instead of a second copy of the corpus, and a
deployment that defines no policies carries nothing at all.

Gates green: `cargo fmt --check`, `cargo clippy --workspace --all-targets
-D warnings`, `cargo test --workspace` — **754 passed, 0 failed**.

## The measurement, taken before the model hardened

The task asked for this first, and it is what decided the design. Four lists
(179,799 domain + 19,484 URL records), two policies overlapping on one list:

| | heap |
| --- | --- |
| union ruleset, compiled once | **6.839 MiB** |
| policy A alone / policy B alone | 5.610 / 6.489 MiB |
| **one `Matcher` per policy** | **12.099 MiB** (1.77×) |
| **shared + per-record mask** | **7.219 MiB** (+0.380 MiB) |

The mask array is exact arithmetic — `records × 2 bytes` — so the only
extrapolated term is the deployed baseline. At 1,043,886 DNS + 18,778 URL rules
that is **+2.03 MiB once**, against **+~17 MiB per policy** for the per-policy
compile. The deployed ruleset is 21.9 MiB against ~24 MiB of headroom under
PERFORMANCE.md's 128 MB budget, so the second policy would have overrun it
alone.

`tests/policy_sharing.rs` asserts all of this rather than asserting a comment:
the rule count does not move with the policy count, and the heap delta equals
the mask array **exactly**.

## Design decisions

### Per rule, not per list

The obvious encoding is a mask per *list* — 8 bytes × 17 lists, essentially
free. It is wrong. Deduplication collapses a rule appearing in several lists
into one record attributed to the **first** (`Matcher` §Deduplication), so a
rule in both `oisd` and `hagezi` is attributed to `oisd` alone; a policy
enabling only `hagezi` would stop seeing it. The builder therefore unions the
masks of every list a duplicate arrives from, which needs somewhere per-record
to put the union. Both tiers do it, and both have a test — this is silent
under-blocking, not a crash.

### The empty array is the flag

The mask array is dropped when every record's mask equals the union of all the
list masks, and an empty array is what tells the lookup to skip the test. That
covers two cases, not one: no policies configured, and policies that all enable
every list. The first draft compared against a `u16::MAX` sentinel and kept a
useless array for three policies over a shared list set.

### Filtering happens during the walk

A rule the policy cannot see returns `None` from `check` and `applies == false`
in the domain walk — it is absent, not merely losing. Filtering the *result*
instead would let an excluded list's `@@` exception suppress a block the policy
still carries. `an_excluded_lists_exception_cannot_suppress_a_block` pins it.

### An inactive schedule falls through, and this reverses my first draft

I wrote — in code, doc comment and test — that an assignment whose schedule has
expired drops the client to the **default** policy: "kids on school nights"
leaves the device unrestricted the rest of the week. The test failed against
the implementation, which fell through to the next matching assignment, and the
implementation was right.

Consider a subnet-wide `guest` policy plus a scheduled per-device `kids`
override. Short-circuiting to the default means that during the day the device
escapes the subnet policy its operator configured — *fewer* restrictions than
were asked for, silently, and only for the one device carrying an override.
Falling through keeps specificity meaning "more specific wins **when it
applies**", and the default is reached only when nothing covers the client at
all. Both cases now have a test.

### `start == end` is a full day

The wrap arithmetic gives a window that opens at 09:00 and closes at 09:00 the
next day. My test was named `an_empty_window_matches_nothing` and its body
asserted the opposite; the behaviour is defensible and the name was not.
Renamed, documented on the field, and the single-day case pinned separately.

## Timezones: POSIX TZ, not a tzdb

A schedule is local wall-clock time and the container has no local time — the
image is distroless and ships no `/usr/share/zoneinfo`. Three options were put
to liviu; the choice was a POSIX TZ string
(`EET-2EEST,M3.5.0/3,M10.5.0/4`): DST-correct for recurring rules, ~40 bytes,
no new dependency against the fixed tech stack, and no ~1 MB of bundled IANA
data in an image that has none.

`fah-config/src/tz.rs` implements it in ~300 lines: offsets (POSIX signs them
**west-positive**, so `EET-2` is UTC+2 — the single easiest thing to get
backwards, and it has its own test), `Mm.w.d/time` transitions, and
`days_from_civil`/`civil_from_days`. The `Jn` and `n` Julian forms are
**rejected** rather than misread — accepting a form the evaluator cannot
express would move a schedule by a day silently.

What the DST tests actually pin:

- spring forward: 02:59 → 04:00, offset +2 → +3 at the exact instant
- fall back: the same wall-clock hour twice, offset +3 → +2
- each rule's `/time` is read in the offset **in force just before** it —
  standard entering DST, DST leaving it. Reading both in standard time moves
  the autumn switch by an hour, and that is what the fall-back test catches.
- southern hemisphere, where DST wraps the year boundary and the interval test
  has to invert
- week 5 means "the last", in a month whose weekday occurs only four times
- at the policy level: 19:30 UTC is inside a 21:00–22:00 Bucharest window in
  February and outside it in August

Two constants in my first draft were wrong arithmetic (a fall-back instant off
by 4.5 days, and a Bucharest offset taken as +2 in August when EEST is +3).
Both were caught by the tests, which is the point of having computed the
instants independently.

## `$client`

Activated as the task required. It was classified inactive on the reasoning
that applying it would apply it to every client — true only while nothing could
identify one, which is exactly what this task adds.

- Payload retained on both `DomainRule` and `UrlRule`. `$client` says *who*, an
  HTTP option says *what*; a rule carrying both is a URL rule scoped to a
  client, not an inactive one.
- Compiled to selectors in a side map keyed by record index, like `$dnstype`.
  Justified by counting: **0** across the deployed corpus, **1 each** in
  EasyList and EasyPrivacy. This is a user-rules feature, so the cost belongs
  on the rules that use it.
- Fail closed. A payload that compiles to no selector (`$client`, `$client=`,
  `192.168.1.0/99`) makes the rule inactive rather than unrestricted.
- Part of a rule's identity in both tiers' dedup, or two rules differing only
  in who they apply to would collapse into one.
- `decisive_rule` echoes the scope, so the query log reports the rule that
  actually fired.

### A defect found on the way

Every existing site that set `options.unsupported` also set
`options.http_scoped`, so the unsupported check living in the URL arm caught
them all. That invariant was accidental and undocumented. `$client` breaks it —
it is scoped to a client, not to a request, so `||ads.example.com^$client` is
domain-shaped and unusable, and would have compiled into an **unrestricted**
domain block. The check is now hoisted above both tiers.

`InactiveReason::UnsupportedUrlPattern` became `Unsupported` in the same change:
it is no longer URL-specific, and a variant that names the wrong tier is the
kind of quiet inaccuracy that survives for years. `ClientScoped` is gone
entirely, as `UrlPattern`/`HttpOption` went in p2-03.

## Layering

`fah-model` holds `Policy`, `Schedule`, `Assignment`, `ClientSelector`,
`PolicyId`; `fah-config` mirrors the schema; `fah-rules` converts and compiles.
The mirror is not duplication by accident — `fah-config` and `fah-model` are
both L1 and `crates/fastadhunter/tests/layering.rs` forbids the edge. This is
the same shape as `EngineMode` against `fah_model::OperatingMode`, which
carries the same note.

`PosixTz` lives in `fah-config` because `Config::validate` has to reject a
malformed timezone while the operator is editing, and `fah-rules` (L2) can
reach down to it.

## Hot path

Core-pinned, both arms built from the same source tree state, baseline from a
detached worktree at `f921555`:

| `matcher_lookup` | baseline | p2-05 | Δ |
| --- | --- | --- | --- |
| hit_exact | 117.03 ns | 107.82 ns | −7.9 % |
| hit_subdomain | 366.35 ns | 361.23 ns | −1.4 % |
| miss | 66.37 ns | 66.95 ns | +0.9 % |

No regression against PERFORMANCE.md's 10 % bar. I would not claim the
`hit_exact` figure as a win — nothing in this change makes a lookup faster, and
code layout moves that benchmark by more than this on its own. The zero-config
path adds one test against an empty slice, which predicts perfectly.

Allocation-free lookup is unchanged: `tests/url_lookup_alloc.rs` still passes,
and the client context is `Copy`.

## What this does not do

- **No enforcement.** Neither pipeline resolves a client to a policy yet;
  `lookup_in` / `lookup_http_in` exist and are tested, and p2-06 threads them
  through DNS and HTTP. Until then every lookup uses the default context, which
  is exactly the pre-Policies behaviour.
- **No API.** No policy endpoints; `GET/POST /api/v1/config` carries the
  sections because they are part of `Config`, and that is all.
- **`blocking_mode` has no consumer.** Declared, validated, and documented as
  inert until p2-06 — the same treatment `[http] idle_timeout_ms` had before
  p2-02, and stated in the field's own doc comment so it cannot be mistaken for
  something that works.
- **No `safe_search`.** The task listed it as a no-op hook. A config key that
  silently does nothing is the p1.5-07 defect, and nothing in Phase 2 or the
  ROADMAP gives it a consumer, so it is not shipped rather than shipped inert.
- **Policies are `boot`.** A schedule evaluates live, but which policies exist
  decides the compiled masks, so changing the set needs a recompile. The engine
  sets policies then compiles, in that order.
- **Not measured on the RB5009.** The heap arithmetic is exact and the lookup
  delta is inside noise on x86, so there is nothing here that the ~9× factor
  would turn into a budget question. The phase soak (p2-08) is where a real
  policy set should be carried on-device.
