# Global Architecture Review — Reconciled Assessment

Adjudicates [Global Architecture Review.md](Global%20Architecture%20Review.md)
(GAR-A) against [Global Architecture Review-Codex.md](Global%20Architecture%20Review-Codex.md)
(GAR-B) using repository evidence only. Neither review was presumed correct;
every contested claim below was re-verified directly against code, tests,
docs, and the accepted Adaptive spec. Baseline: `main` @ `ad9343c` (two
doc/test commits past `37e5a8f`; architecturally identical).

## 1. Consensus findings (independent convergence)

Both reviews reached PASS WITH REQUIRED CHANGES, and independently agree on:

| Finding | Both classify | Repo evidence |
| --- | --- | --- |
| Layering clean, downward-only, test-enforced; binary is the composition root | VALIDATED/SUPPORTED | `layering.rs` executed, manifests |
| One matcher + one policy identity shared by DNS and HTTP — strongest architectural asset | VALIDATED | `matcher.rs:870,983`, `context_for`, both call sites |
| Rules-before-cache; cache stores answers only; verdict-before-resolve | VALIDATED | `pipeline.rs:270-296`, `proxy.rs:332-343` |
| Upstream ordered walk with zero liveness state = the defect Adaptive targets | VALIDATED | `upstream/mod.rs:182-226` |
| RCODE must never affect transport health | invariant | counters move only in `Err` arm |
| `resolve_host` must be health-isolated | required | spec S1.8 |
| Certificate ownership undecided; must be decided (shared crate + ADR) before Phase 3 code | RISK | rcgen in `fah-api`, consumers are siblings |
| Event taxonomy (`{Dns, Http}`) too narrow for Phase 3 | RISK | `request_event.rs:93-96` |
| Phase 3 memory owners (cert cache, TLS buffers, per-connection state) unbounded and unbudgeted | RISK | absent from p3 plans |
| Measurement gaps: SNI parse, splice, TLS handshake, cert mint, DoT/DoH — all on RB5009 | UNKNOWN | no benches exist |
| Failure run-length distribution missing; gates Stage 1 | blocking | spec S1-G4, lines 106-132 |
| DoH hostname bootstrap depends on the OS resolver | ASSUMPTION/RISK | `encrypted.rs:164-180` |
| History bounded by age, not bytes | accepted | `stats.rs`, retention prune |
| Stage 2/3 stay gated; Stage 1 scope stays narrow | agreed | spec S1.1 |

## 2. Material disagreements

1. **Phase 3 go/no-go.** GAR-B: "NO-GO today until ADRs are written." GAR-A:
   "GO after Adaptive, conditional on the listed decisions/measurements."
2. **Required changes before Adaptive.** GAR-B demands designing an
   endpoint-health module, transport-only classification, caller-class
   isolation, telemetry, and narrow scope. GAR-A treats the accepted spec as
   the design and instead names two code defects that break it.
3. **SWR as health signal.** GAR-B: SWR "should either be lower-weight or
   separately reported." GAR-A accepts the spec's Record decision.
4. **RouterOS IPv6/443 steering.** GAR-B elevates it to a top risk citing
   current IPv6 HTTP bypass; GAR-A treats deployment steering as recorded
   and portable-by-construction.
5. **Coverage disjoint.** Each review carries findings the other lacks
   entirely (adjudicated in §3).

## 3. Evidence-based resolutions

### 3.1 Phase 3 verdict — terminology, not substance

Both require the same things before Phase 3 code: cert home, HTTPS/connector
decisions, event taxonomy, memory caps, measurements. The sequencing (Phase 2
baseline → Stage 1 → benchmarks → Phase 3) is already decided in
`docs/design/adaptive-upstream-selection.md` lines 6-10 and is not either
review's to reopen. **Resolution: identical position — Phase 3 starts only
after Adaptive closes and the named decisions exist.** GAR-B's "NO-GO today"
and GAR-A's "GO conditional on §6" are the same verdict differently worded.

### 3.2 Adaptive "required changes" — GAR-B re-derives decided content

The accepted Stage 1 spec already contains all five of GAR-B's demands:
endpoint model and state machine (S1.2, S1.3 incl. the always-return-a-
candidate hard invariant), transport-only failure classification (S1.4),
caller-class isolation table — pipeline Record / SWR Record / `resolve_host`
Ignore (S1.8, lines 435-437), telemetry (S1.12), narrow scope (S1.1), and
the run-length gate (S1-G4). GAR-request forbids rewriting the spec.
**Resolution: GAR-B's Adaptive section is a restatement of decided work, not
new required work.** The one live blocking measurement both agree on is
S1-G4's run-length distribution.

What GAR-B missed and GAR-A found — both **confirmed directly in code
during this adjudication**:

- **Pooled DoT/DoH connections never reconnect on timeout.**
  `encrypted.rs:89`: `Err(err) if fresh || err.kind() == TimedOut => Err(err)`
  — a timeout on a pooled connection surfaces without touching the slot; only
  non-timeout pooled errors reconnect. An IPv6-prefix-rotation blackhole
  produces exactly timeouts. The spec's own claim ("a persistent v6 DoT/DoH
  connection reconnects", line 602) is contradicted by the code, and S1.7's
  probes would reuse the dead exchange. CONFIRMED — spec/code contradiction.
- **Error-kind erasure.** `map_err(io::Error::other)` at `encrypted.rs:143,154`
  collapses TLS/connect errors into `ErrorKind::Other`; S1.4's path-failure
  tier (penalize-at-1 for unreachable/TLS) is unimplementable for encrypted
  transports as written. CONFIRMED.

### 3.3 SWR weighting — spec stands

SWR refreshes are real queries against real endpoints; a transport failure is
a property of the endpoint, not the caller, and the spec isolates the one
caller whose failures are *designed* (`resolve_host`, S1.8). GAR-B's concern
(background traffic dominating the signal) is the spec's stated intent — SWR
supplies ~69 % of attempts and most probe opportunities. **Resolution: the
accepted spec's Record decision stands; no lower-weighting.** One genuine
GAR-B catch survives: **the SWR claim lease (5 s, `cache.rs:80`) vs the
worst-case walk** — 8 endpoints × 800 ms = 6.4 s exceeds the lease, so an
8-endpoint config can double-refresh. Small, real, worth a check in Stage 1.

### 3.4 IPv6/443 steering — GAR-B half right, half stale

GAR-B's cited evidence (`deploy-rb5009.md:695-706`, "a client reaching a host
over IPv6 bypasses the proxy entirely — verified") is **stale**: p2-14
(`docs/code-review/phase2/p2-14-review.md`) deployed and verified the IPv6
dstnat mirror rules — same machine/URL/moment, 403 over IPv6, per-rule
counters confirming the v6 path. **Resolution: the port-80 IPv6 bypass claim
is falsified; the 443 steering question (v4 *and* v6, skip lists,
self-traffic exclusion, prefix rotation) is genuinely undecided and is a
required Phase 3 decision.** New finding from this adjudication: two repo
docs contradict each other — `deploy-rb5009.md` §"IPv6 is not covered" must
be reconciled with p2-14 (p2-14 wins; it is the later, measured record).

### 3.5 GAR-A findings absent from GAR-B — verified

| Claim | Verification | Verdict |
| --- | --- | --- |
| DNS listener loops exit permanently on socket error; no supervision; healthcheck config-parse-only; netwatch masks death as unfiltered resolution | `udp.rs:27-30`, `tcp.rs:34-37` (warn + return); API loop contrast (`fah-api/server.rs` sleep-and-continue); `Engine.tasks` abort-only | **CONFIRMED — highest operational risk in either review** |
| List refresh commits any fetched body over the last-good `/data` copy before any content check | `lifecycle/mod.rs:637-654`; "the only failure mode is I/O, never parsing" is the stated design; `looks_misparsed` warn-only | **CONFIRMED** |
| `/policies` edit fail-open window (new-mask matcher + stale PolicyId snapshot) | `routes.rs:1031-1035`: `set_policies` → `recompile().await` (seconds) → `republish_policies`. Scope note: only DELETE/reorder shifts indices; POST append is safe | **CONFIRMED, narrow but fail-open** |
| Name-based policy assignments resolve through fah-stats' evictable 4096-LRU | `republish_policies` → `stats.named_clients()` (`routes.rs:1042-1046`) | CONFIRMED |
| SWR/`resolve_host` queries carry no EDNS → >512 B refresh answers pay a TCP retry every refresh, invisible to telemetry | `swr.rs:222-230` — the no-options choice is deliberate and documented; the 512 B payload consequence and its invisibility are not addressed anywhere | CONFIRMED (consequence unowned) |
| LiteralConnector/retarget conflicts with upstream TLS hostname verification | `claim.rs:147-158`, `proxy.rs:126-160` — connector only ever sees an IP literal | CONFIRMED (design conflict, unacknowledged in p3-04) |
| DoH on the admin listener shares trust boundary and 64-conn const ceiling | p3-05 plan + `fah-api/server.rs` MAX_CONNECTIONS const | CONFIRMED (plan-level) |
| Live bearer token tracked in `tui-monitor/config.toml` | file present; already flagged in project-state.md:177 | CONFIRMED |

### 3.6 GAR-B's open UNKNOWN resolved

"Whether HTTP event drops are fully visible in top-level telemetry" — **yes**:
both pipelines share one bounded channel and one drop counter, exported as
`events_dropped` in `/telemetry` (`fah-metrics/registry.rs:174-179,236`).

### 3.7 Claims insufficiently supported (both reviews)

- GAR-B: current IPv6 HTTP bypass (stale doc — §3.4); Adaptive
  required-changes framed as undecided (§3.2); "SWR lower-weight" recommendation
  (contradicts an accepted spec without new evidence).
- GAR-A: "compile transient at 90 % of the notional 256 MB ceiling" is the
  headline for the **non-deployed** allocator setting (230.7 MiB @
  `PURGE_DELAY=100`); the deployed setting peaks at 181.4 MiB ≈ 71 %. The
  risk is real (empirically-, not architecturally-bounded, grows with list
  size) but the headline number was oversold.
- GAR-B's "hickory cancellation behavior" UNKNOWN is real but gates only
  Stage 2/3 racing/hedging — correctly out of scope now.

## 4. Remaining unknowns / unvalidated assumptions (agreed union)

1. IPv6 **upstream** forwarding never exercised by any test or deployment
   (defaults are v4 literals).
2. DoH bootstrap via the distroless container's OS resolver (contents of
   `/etc/resolv.conf` unverified; possible self-loop).
3. Upstream TCP/53 reachability (TC retries mock-tested only); ICMP error
   visibility on connected UDP in the container (S1.4 path tier rests on it).
4. HTTP path under concurrency: every deployed figure is single-connection;
   per-connection memory unmeasured; 1024-permit ceiling never exercised.
5. TLS on RB5009: handshake cost, interception CPU+memory, cert-mint latency,
   splice throughput — and the ~9× factor is proven not to convert
   syscall-bound work, so these need probe containers, not dev benches.
6. `MIMALLOC_PURGE_DELAY=0` above household-idle load; cache byte-cap
   eviction path never fired on-device; all-cores combined load untested.
7. Failure run-length distribution (S1-G4) — the one measurement both
   reviews and the spec agree blocks Stage 1's penalty tuning.

## 5. Architectural blockers

**Operational, fix now (predate both phases; live resolver exposure):**

1. Listener loops' exit-on-error + no supervision + config-parse-only
   healthcheck (§3.5 row 1) — silent unfiltered failover is the worst
   failure path in the system, and Phase 3 listeners would clone the
   template.
2. List-refresh content gate before overwriting the last-good copy; expose
   `parse_errors` (§3.5 row 2).

**Block Adaptive Stage 1 (ship-gates, not start-gates):**

3. S1-G4 run-length measurement (spec's own gate).
4. Encrypted-transport reconnect semantics vs S1.15/S1.7 — fix or amend the
   spec; as written they contradict (§3.2).
5. `io::ErrorKind` fidelity through encrypted transports (S1.4 prerequisite).
6. Outcome observability: SERVFAIL-served counter (`pipeline.rs:405-418`
   currently indistinguishable from an ordinary pass) and per-query endpoint
   attribution — without them Stage 1's benefit cannot be judged.

**Block Phase 3 code (decisions before implementation):**

7. Certificate machinery home (shared crate; ADR).
8. Retarget/connector redesign for upstream TLS hostname verification.
9. DoH/DoT listener placement vs the admin surface.
10. Event/telemetry taxonomy extension.
11. Memory caps for every new Phase 3 state owner (cert cache, handshakes,
    per-connection, splice buffers).
12. RouterOS 443 steering decision, v4+v6 (§3.4).
13. On-device measurements: TLS handshake, TLS-loaded concurrency profile,
    splice probe.
14. Interception opt-in bound to stable identity, not bare IPs.

## 6. Deferrable risks

- `fah_config`/`fah_model` type mirrors; `fah_api::CacheStats` identity-DTO;
  layering-guard blind spots (tui-monitor, renamed deps, unscanned tables).
- Compile transient (p2-12 open) — monitored via persisted `peak_rss`;
  deployed peak 181.4 MiB.
- Policy fail-open window and name-assignments-on-LRU — real, fail-open, but
  narrow trigger surface today; close before policy churn or Phase 3
  listeners multiply exposure (builder-convention wiring belongs with §5.7-14).
- SWR no-EDNS truncation tax — add EDNS to synthetic queries when convenient;
  add upstream-TCP-retry telemetry with §5.6.
- SWR lease vs 8-endpoint walk arithmetic (§3.3) — one-line check in Stage 1.
- fah-common scope creep; `blocking_mode` accepted-but-inert; histogram
  100 ms ceiling; latency attribution stops at path granularity.
- Doc hygiene: `deploy-rb5009.md` §IPv6 stale vs p2-14; `/metrics`/
  "Prometheus" references to a removed endpoint; stale `project-state.md`;
  tracked bearer token (cheap, do promptly, but not architecture).

## 7. Final verdict

**`main` (37e5a8f / ad9343c) is an appropriate architectural foundation for
the next phases. PASS WITH REQUIRED CHANGES — confirmed by both independent
reviews and by direct re-verification of every contested claim.**

- The consensus core (layering, single policy path, bounded state,
  drop-never-backpressure, cache semantics) survived adversarial checking
  from two directions plus this adjudication. No redesign is warranted.
- Where the reviews disagreed, the evidence sided with: the accepted Stage 1
  spec over GAR-B's re-derivation (§3.2, §3.3); GAR-A's code-level findings
  over GAR-B's silence (§3.5, all confirmed); GAR-B's 443-steering demand
  over GAR-A's under-weighting (§3.4, minus the stale bypass claim); and
  neither review's headline where it overshot (§3.7).
- **Adaptive DNS Stage 1: GO** — begin now; ship behind §5.3-6.
- **Phase 3: GO after Adaptive closes**, conditional on §5.7-14 —
  substantively what both reviews independently required.
