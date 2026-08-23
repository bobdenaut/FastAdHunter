---
title: Adding an attribution field to a hot-path event without a schema break
date: 2026-08-22
category: design-patterns
module: fah-dns
problem_type: design_pattern
component: observability
severity: medium
applies_when:
  - a per-request event must start carrying a new fact (which backend answered, what kind of failure was served)
  - a deployed consumer already parses that event and cannot be upgraded in lockstep
  - the producing path is hot, so the new fact must cost no allocation and no clone
  - the datum originates below the event — in a trait the pipeline calls — and has to be threaded up
symptoms:
  - the event says a query passed but not whether the client got an answer or a synthesized SERVFAIL
  - "a bool like upstream_used cannot say which upstream, so per-endpoint attribution is impossible"
  - adding the field to the constructor would churn every fixture and trip clippy::too_many_arguments
  - the function that would carry it already returns a 4-tuple and would grow to six positional values
related_components:
  - fah-dns/pipeline
  - fah-dns/upstream/pool
  - fah-model/query_event
  - fah-metrics/registry
tags:
  - telemetry
  - event-schema
  - wire-compatibility
  - serde-default
  - hot-path
  - trait-signature
  - version-skew
---

# Adding an attribution field to a hot-path event without a schema break

## Context

p2.5-05 needed two facts on `QueryEvent` that were not there: **which upstream
answered**, and **whether what reached the client was an answer or a failure**
(and if a failure, synthesized locally or relayed from upstream). Three
constraints met at once:

- the fact originates in `Forwarder::forward`, three layers below the event;
- a deployed `tui-monitor` parses the event stream and is not upgraded in
  lockstep;
- the path is per-query — no allocation, no clone.

The naive shape (add two parameters to `QueryEvent::new`, add two slots to the
tuple `Pipeline::resolve` returns) fails all three differently: it churns the
twenty `QueryEvent::new` call sites, trips `clippy::too_many_arguments` on a
public constructor under `-D warnings`, and produces a six-positional return
where any branch can invent a nonsense combination.

## Guidance

Five moves. Each one is independently reusable; together they make the addition
free on the wire and free in memory.

### 1. Carry the new datum in a small owned struct beside the trait

Not in the shared model crate — it never crosses the event channel
([upstream/mod.rs:41](../../../crates/fah-dns/src/upstream/mod.rs#L41)):

```rust
pub trait Forwarder: Clone + Send + Sync + 'static {
    fn forward(&self, query: &Message)
        -> impl std::future::Future<Output = io::Result<ForwardOutcome>> + Send;
}

#[derive(Debug)]
pub struct ForwardOutcome {
    pub message: Message,
    pub endpoint: u8,
}
```

Widening the trait's return type costs a compiler-driven sweep of every
implementor (14 in this workspace, mostly test and bench fakes, each becoming
`Ok(ForwardOutcome::new(response, 0))`). That sweep is the point: after it, the
datum is impossible to forget, and `resolve` cannot read an endpoint the
forwarder never set.

**Do not derive `Clone` on it.** It owns a `Message`; a derive that nothing uses
is an invitation to an accidental heap-heavy clone on the query path.

### 2. Attach optional event attribution with a chained setter, never by growing the constructor

Follow whatever idiom the event type already uses for optional attribution — here
`under_policy`, added for `policy` in p2-06
([query_event.rs:90](../../../crates/fah-model/src/query_event.rs#L90)):

```rust
pub fn with_outcome(mut self, answer: AnswerOutcome, endpoint: Option<u8>) -> Self {
    self.answer = answer;
    self.endpoint = endpoint;
    self
}
```

An eighth parameter on a **public** `new` trips `clippy::too_many_arguments`, so
the alternative is not "churn twenty sites" — it is "churn twenty sites *and*
add an `#[allow]` to a public constructor".

Name the cost out loud: the compiler no longer forces every construction site to
state an outcome, so a future caller of `new` silently emits the default. An
end-to-end test has to take over that job (see Examples). This trade only works
because the field is *optional attribution*; see When to Apply.

### 3. Make the field free on the wire with `default` + `skip_serializing_if` on both sides

([query_event.rs:62-65](../../../crates/fah-model/src/query_event.rs#L62))

```rust
#[serde(default, skip_serializing_if = "AnswerOutcome::is_answered")]
pub answer: AnswerOutcome,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub endpoint: Option<u8>,
```

An ordinary answered query then serializes **byte-identically to before the
change** — the deployed older consumer sees no new keys at all, and the newer
consumer reads pre-change records as defaults. Version skew is covered in both
directions by construction, with no schema-version field to maintain.

`Option::is_none` is free; an enum needs its own predicate matching serde's
`fn(&T) -> bool` contract:

```rust
impl AnswerOutcome {
    pub fn is_answered(&self) -> bool {
        matches!(self, Self::Answered)
    }
}
```

The same two attributes carry the counters into `DnsCounters.answers` and the
persisted `PerfSample.answers_delta`, so old JSONL history rows keep parsing.

### 4. Past ~4 return values, replace the tuple with a private struct of named constructors

Not for tidiness — for what it forbids. A 6-tuple lets any branch of `resolve`
emit a nonsense `(cache_hit, upstream_used, stale, outcome, endpoint)`
combination. Four constructors make each combination unspellable per branch
([pipeline.rs:38](../../../crates/fah-dns/src/pipeline.rs#L38)):

```rust
struct Resolved {
    response: Message,
    cache_hit: bool,
    upstream_used: bool,
    stale: Option<StaleServe>,
    outcome: AnswerOutcome,
    endpoint: Option<u8>,
}

impl Resolved {
    fn blocked(response: Message) -> Self { /* … */ }
    fn from_cache(response: Message, stale: Option<StaleServe>) -> Self { /* … */ }
    fn forwarded(response: Message, outcome: AnswerOutcome, endpoint: u8) -> Self { /* … */ }
    fn synthesized_servfail(response: Message) -> Self { /* … */ }
}
```

`Resolved::forwarded` is the only constructor that sets `endpoint: Some(_)`, so
"an endpoint is named only when an upstream actually answered" is a property of
the type rather than of five branches agreeing. Knock-on: the caller reads
`resolved.cache_hit` instead of `resolved.1`, and the event emitter took
`&Resolved` and dropped from nine parameters to five — deleting the
`#[allow(clippy::too_many_arguments)]` that had been there since before this
change.

### 5. Bound a packed index at config validation, not at the cast

`endpoint` is a `u8` walk index, filled from `enumerate()`
([upstream/mod.rs:196](../../../crates/fah-dns/src/upstream/mod.rs#L196)):

```rust
u8::try_from(index).unwrap_or(u8::MAX)
```

That conversion alone leaves the *only* guard on the query path, silently
saturating. Put the real bound where configuration is checked instead —
`MAX_UPSTREAM_SERVERS = 8` rejected in `fah_config::validate` — so every
accepted index is ≤ 7 and `u8::MAX` becomes a sentinel that cannot collide
with a real endpoint. The cast stays as defence-in-depth, not as the contract.

## Why This Matters

- **Measured, not assumed: `size_of::<QueryEvent>()` is 160 bytes before and
  160 bytes after.** The new `AnswerOutcome` (1 B) and `Option<u8>` (2 B) land
  in padding the struct already carried, so the bounded event channel's
  footprint does not move. "Additive" and "additive *and* free" are different
  claims, and only the second one survives a `size_of` read.
- Hot-path A/B against a pre-change checkout: the forwarded-query bench moved
  **+0.4 % of means** against a **±5 %** control-arm band. Cost of the whole
  chain — widened trait return, `Resolved`, two extra event fields, one extra
  `match` in the metrics registry — is below this box's noise floor.
- The wire result is the one that compounds: an operator can deploy the new
  engine under an old dashboard and lose nothing, because ordinary events did
  not change a single byte.

## When to Apply

Apply when a hot-path event gains **optional attribution** — a fact that
enriches the record but whose absence is a valid state — and a consumer exists
that cannot be upgraded with the producer.

Do **not** use the chained setter when the new field is *mandatory for
correctness*. There the churn is the feature: grow the constructor (or add a
second named constructor) so the compiler stops every call site, and accept the
`#[allow]` if it comes to that.

Do not push the carrier struct (`ForwardOutcome` here) into the shared model
crate unless it genuinely crosses a channel; a type only one consumer sees does
not belong in L1.

Skip move 4 while the return is three or four values — a struct there is
ceremony. It earns its keep once the branches can disagree.

## Examples

**Pin backward compatibility with a literal pre-change fixture, not a
round-trip.** A round-trip only proves the new code agrees with itself; it
cannot catch a field that silently became required:

```rust
#[test]
fn pre_change_event_json_deserializes_to_answered_without_an_endpoint() {
    let json = r#"{
        "query": { "domain": "ads.example.com", "qtype": "A",
                   "client_ip": "192.168.1.10",
                   "timestamp": {"secs_since_epoch": 0, "nanos_since_epoch": 0} },
        "verdict": "Pass",
        "duration": {"secs": 0, "nanos": 250000},
        "cache_hit": false,
        "upstream_used": true
    }"#;
    let event: QueryEvent = serde_json::from_str(json).expect("old events must still parse");
    assert_eq!(event.answer, AnswerOutcome::Answered);
    assert_eq!(event.endpoint, None);
}
```

And assert the *absence* of the key, which is what the deployed consumer
actually depends on:

```rust
assert!(!serde_json::to_string(&base).unwrap().contains("answer"));
```

**Replace the constructor's lost compile-time force with one end-to-end test.**
Move 2 gave up "the compiler makes you state an outcome". Buy it back where the
producer and the consumer meet — in this workspace that is the binary, the only
crate allowed to see both L3 siblings:

```rust
let metrics = Metrics::new();
match rx.try_recv().expect("the pipeline must emit one event") {
    Event::Dns(event) => metrics.record(&event),
    Event::Http(_) => panic!("the DNS pipeline emitted an HTTP event"),
}
let counters = metrics.engine_telemetry().counters.dns;
assert_eq!(counters.answers.servfail_synthesized, 1);
```

One such test per outcome, each asserting the counter that moved **and the two
that did not**. That pairing is what fails if the pipeline ever stops calling
the setter.

## Related

- [docs/code-review/phase2.5/p2.5-05-outcome-telemetry-review.md](../../code-review/phase2.5/p2.5-05-outcome-telemetry-review.md)
  — the review that produced moves 1, 4 and 5 (m3, n1/n2, m4), and the P1
  benchmark A/B whose numbers are quoted above.
- [docs/measurement-traps.md](../../measurement-traps.md) — how to read that
  A/B. The control arm is what made the +0.4 % trustworthy; a single
  measurement pair per arm read +8 % and reversed under interleaving.
- [recovering-io-errorkind-from-a-foreign-error-type.md](recovering-io-errorkind-from-a-foreign-error-type.md)
  — the other half of the same `Forwarder` contract: this doc widens what a
  success carries, that one widens what a failure carries.
