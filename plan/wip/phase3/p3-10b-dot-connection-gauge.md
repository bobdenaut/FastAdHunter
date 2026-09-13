# P3-10b — DoT Connections Get Their Own Gauge

**Phase:** 3 · **Depends on:** p3-05 · **Source:** p3-10 Track A, A5 · **Model:** Fable

## Goal

A DoT connection is counted. Today none is, and the counters that are not
counting it are the ones a default will be set from.

## Context

`dot.rs:152` hands `None` to `tcp::handle_connection` where the TCP listener
hands a `TcpConnectionGauge`. The merge generalised that loop over `Transport`
so DoT could reuse it, and the bound came with it — a DoT oversize close is
still bounded — but the instrument did not. So
`counters.dns_tcp_connections.{active,peak,closed_oversize}` exclude every DoT
connection.

Two things make this worth doing now rather than later.

**It corrupts a default.** Those counters are what the F1 follow-up reads to set
the final `dns.tcp_max_connections` (project-state.md §Risk inventory
close-out). The soak running today is a pre-Phase-3 build with no DoT, so its
figures are whole. The first soak of a Phase 3 build is the one that would set a
default from half the traffic.

**It deletes an approved measurement.** p3-10's Track B2 carries "Peak
concurrent DoT connections under household load", which is what says whether
`DOT_MAX_CONNECTIONS = 64` covers this house (p3-10 A6). Nothing else in the
binary can see a DoT connection, so without this task that row cannot be run.

The owner decided the shape on 2026-09-13: **a separate gauge**, not the TCP
gauge shared and not left uncounted. Sharing fails because one number for two
transports cannot answer a question about one of them. The reasoning is recorded
in `docs/code-review/phase3/p3-10-track-a-review.md` §Owner decisions.

## Deadline

**Must land before p3-11's seven-day soak starts.** After that the soak is
measuring with the instrument missing, and the run cannot be repaired
afterwards — the traffic is gone.

## Scope

- A DoT connection gauge, separate from the TCP one, carrying the same three
  figures: `active`, `peak`, `closed_oversize`. `closed_oversize` matters as
  much as the others here: DoT's oversize path is bounded today but invisible.
- `dot.rs` passes it where it passes `None`.
- The value travels the same road the TCP gauge already travels: `fah-model`,
  the `fah-metrics` registry, and the binary's telemetry poll, which today reads
  `dns.tcp_connections()` and will read the new accessor beside it.
- Tests. The DoT ceiling tests at `dot.rs:560-577` are the natural hook — they
  already drive connections to the cap. A test must fail if the gauge stops
  being passed.

## Documentation

The workflow decides this, not the task, and the two cases differ:

- **API.md is required in the same change.** It documents `dns_tcp_connections`
  in three places — the payload example at `:227`, the explanation at `:287`, the
  push note at `:391` — so a new counter on `/api/v1/telemetry` contradicts the
  document unless it lands with it. Docs are the source of truth and a change
  that contradicts them updates them in the same change.
- **CONTEXT.md probably is not.** `:116` already defines the transport
  vocabulary, `dot` included, so the gauge reuses an existing term rather than
  naming a new concept. Hard rule 6 bites only if this change coins one — which
  it should avoid.

Both are `.md` edits and **each needs its own explicit yes** (§Working
agreement). Propose them, do not assume them.

## Acceptance criteria

- A DoT connection appears in the new gauge and in `/api/v1/telemetry`; a TCP
  connection appears in the TCP gauge; neither leaks into the other.
- A DoT oversize close increments `closed_oversize` on the DoT gauge.
- The test fails when `dot.rs` is put back to passing `None` — verified by doing
  it locally and reverting, not by assuming.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --all-features --workspace` all green.
- API.md updated in the same change, with its own approval.

## Out of scope

Whether 64 is the right DoT ceiling, and whether `DOT_MAX_CONNECTIONS` deserves
a config key — that is p3-10 A6 and B2's row. Changing any default. Touching the
TCP gauge's own semantics.
