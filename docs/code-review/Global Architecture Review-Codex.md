# Global Architecture Review - FastAdHunter

Review date: 2026-08-17

Mode: read-only architecture review

Scope: current FastAdHunter architecture before Adaptive DNS and Phase 3 reboot.

Verdict: **PASS WITH REQUIRED CHANGES**

## Classification Legend

- **VALIDATED**: supported by architecture docs, code structure, tests, and current deployment or measurement notes.
- **SUPPORTED**: supported by code and docs, but not yet fully proven under the future workload.
- **ASSUMPTION**: plausible, but not proven by current evidence.
- **RISK**: could cause architectural failure, operational failure, security drift, or future redesign if ignored.
- **UNKNOWN**: insufficient evidence in the current repository.

## Executive Verdict

FastAdHunter has a strong current architecture for the product it is today. DNS filtering, HTTP transparent proxying, API control, telemetry, rule lifecycle, policy scheduling, history, and RouterOS deployment are not accidental pieces. They are wired through a clear runtime owner, use a shared rule/policy model, and mostly preserve clean module layering.

The current design passes for the present DNS + HTTP appliance.

It does not pass as-is for the next two major architecture moves:

- Adaptive DNS needs a first-class upstream-health and endpoint-selection module.
- Phase 3 needs explicit ownership for TLS, certificates, HTTPS interception, event taxonomy, memory bounds, and RouterOS IPv6/443 steering.

The correct decision is therefore:

**PASS WITH REQUIRED CHANGES before Adaptive DNS ships.**

**NO-GO for Phase 3 implementation until the required architecture decisions are written down.**

No source files were edited during this review. This document records findings only.

## Evidence Reviewed

Primary documents:

- [ARCHITECTURE.md](../../ARCHITECTURE.md)
- [CONFIGURATION.md](../../CONFIGURATION.md)
- [PERFORMANCE.md](../../PERFORMANCE.md)
- [SECURITY.md](../../SECURITY.md)
- [README.md](../../README.md)
- [docs/design/adaptive-upstream-selection.md](../design/adaptive-upstream-selection.md)
- [docs/design/adaptive-upstream-selection-benchmarks.md](../design/adaptive-upstream-selection-benchmarks.md)
- [docs/deploy-rb5009.md](../deploy-rb5009.md)
- [docs/routeros-traps.md](../routeros-traps.md)
- [docs/measurement-traps.md](../measurement-traps.md)
- [plan/open/phase3](../../plan/open/phase3)

Primary code areas:

- [crates/fastadhunter/src/main.rs](../../crates/fastadhunter/src/main.rs)
- [crates/fah-dns/src/pipeline.rs](../../crates/fah-dns/src/pipeline.rs)
- [crates/fah-dns/src/upstream/mod.rs](../../crates/fah-dns/src/upstream/mod.rs)
- [crates/fah-dns/src/upstream/encrypted.rs](../../crates/fah-dns/src/upstream/encrypted.rs)
- [crates/fah-http/src/proxy.rs](../../crates/fah-http/src/proxy.rs)
- [crates/fah-http/src/server.rs](../../crates/fah-http/src/server.rs)
- [crates/fah-rules/src/matcher.rs](../../crates/fah-rules/src/matcher.rs)
- [crates/fah-rules/src/policy.rs](../../crates/fah-rules/src/policy.rs)
- [crates/fah-rules/src/lifecycle/mod.rs](../../crates/fah-rules/src/lifecycle/mod.rs)
- [crates/fah-api/src/config_store.rs](../../crates/fah-api/src/config_store.rs)
- [crates/fah-api/src/auth.rs](../../crates/fah-api/src/auth.rs)
- [crates/fah-api/src/tls.rs](../../crates/fah-api/src/tls.rs)
- [crates/fah-model/src/engine.rs](../../crates/fah-model/src/engine.rs)
- [crates/fah-model/src/query_event.rs](../../crates/fah-model/src/query_event.rs)
- [crates/fah-model/src/request_event.rs](../../crates/fah-model/src/request_event.rs)
- [crates/fastadhunter/tests/layering.rs](../../crates/fastadhunter/tests/layering.rs)

## Current Architecture Map

### Runtime Shape

**SUPPORTED**

The `fastadhunter` binary is the composition root. It owns runtime construction and task lifecycle for:

- upstream pool
- rule/list manager
- policy state
- DNS pipeline
- HTTP proxy and listener
- stats and metrics
- API server
- event fanout
- rules scheduler
- stats/history schedulers
- telemetry polling
- SWR workers
- DNS cache cleanup

This is a good module shape. The binary wires L3 modules together without pushing all behavior into the binary itself. Runtime ownership is visible in one place, while protocol behavior remains local to the protocol modules.

**RISK**

Phase 3 will add more runtime actors: HTTPS listener, SNI parser, TLS acceptor, splice path, interception path, generated certificate cache, DoT listener, DoH route, and possibly HTTP/2 stream management. Those actors need the same explicit ownership. They should not be hidden inside incidental constructors or scattered background tasks.

### Workspace Layering

**SUPPORTED**

The workspace has a clean dependency direction:

- L1: `fah-model`, `fah-config`, `fah-common`, `fah-logging`
- L2: `fah-rules`
- L3: `fah-dns`, `fah-http`, `fah-api`, `fah-stats`, `fah-metrics`
- L4: `fastadhunter`

The layering test asserts that internal `fah-*` dependencies point downward rather than sideways or upward.

This is a meaningful guardrail for future work. DNS, HTTP, API, stats, and metrics are sibling modules composed by the binary rather than importing each other directly.

**RISK**

Phase 3 certificate logic can easily violate this shape. API TLS, DoT/DoH TLS, and HTTPS interception all need certificate behavior. If that behavior stays inside `fah-api` and is copied into `fah-http`, the architecture will lose locality and security review will become harder.

### DNS Flow

**VALIDATED**

Current DNS request flow:

1. Receive UDP/TCP DNS query.
2. Parse and validate DNS message.
3. Canonicalize client IP.
4. Load current matcher and active policy.
5. Apply rule decision before cache.
6. If blocked, synthesize local block response.
7. If allowed or passed, check cache.
8. If fresh hit, return cached upstream answer.
9. If stale hit and SWR applies, serve stale and queue refresh.
10. Otherwise forward to upstream.
11. Emit DNS query event.

This ordering is architecturally correct. Rule evaluation happens before cache use. Blocked responses are not cached as upstream answers. Cache stores upstream-derived answers, not policy results.

**SUPPORTED**

The DNS pipeline avoids pinning old matcher/policy snapshots across upstream awaits by dropping loaded state before forwarding. That helps memory locality during ruleset swaps.

**RISK**

SERVFAIL currently participates in stale-serving behavior. That is acceptable inside the DNS response pipeline, but Adaptive DNS must not confuse DNS RCODEs with transport health. A SERVFAIL answer can be a valid transport response.

### HTTP Flow

**SUPPORTED**

Current HTTP request flow:

1. Accept transparent TCP connection.
2. Parse request head.
3. Determine claimed destination from request authority, Host, and URI shape.
4. Apply the same matcher and active policy model used by DNS.
5. Resolve destination host through the configured resolver adapter.
6. Apply egress policy to resolved IPs.
7. Retarget upstream request to literal IP.
8. Preserve original Host header.
9. Stream response.
10. Emit HTTP request event.

This is a good fit for transparent HTTP interception. The resolver-before-egress-check pattern reduces DNS rebinding exposure.

**SUPPORTED**

The HTTP proxy is generic over streams, which creates a useful seam for future TLS-terminated streams.

**ASSUMPTION**

That stream genericity is not proof that Phase 3 interception is simple. HTTP/2, ALPN, upstream TLS verification, generated leaf certificates, and CONNECT-like semantics can still force new architecture decisions.

### Shared Rules and Policy

**SUPPORTED**

DNS and HTTP both use the same matcher/policy machinery:

- `ListManager` owns compiled matcher snapshots.
- `PolicySet` is immutable once published.
- `PolicyState` owns active schedule resolution.
- DNS and HTTP both ask for policy context by client IP.
- DNS lookup and HTTP lookup use protocol-specific matcher entry points, but policy identity is shared.

This is one of the strongest architecture choices in the project. It prevents DNS and HTTP from becoming two unrelated filtering products.

**RISK**

Phase 3 introduces more protocol surfaces: SNI-only HTTPS, intercepted HTTPS, DoT, and DoH. Those paths must reuse the same policy identity model. A new `https` path must not grow its own policy resolver.

### Configuration Ownership

**SUPPORTED**

Configuration has an explicit boot-only vs runtime distinction. The API can persist changes that require restart without pretending they are live-applied. Runtime-supported surfaces are narrower and intentionally wired.

This is a good architecture for an appliance. It is better to be conservative about live mutation than to create partially applied state.

**RISK**

Phase 3 will introduce config that is hard to classify:

- API certificate import
- interception CA import
- generated leaf cache bounds
- SNI no-match policy
- ECH behavior
- interception eligibility
- DoT/DoH listener config
- HTTPS listener config

Each must be explicitly classified as boot-only or runtime. The default should remain boot-only unless a live consumer and safe reload path exist.

### Security Model

**SUPPORTED**

Current security architecture is coherent:

- no hand-rolled crypto
- API key required except explicit health endpoint
- API TLS enabled by default
- private material under `/config`
- runtime data under `/data`
- non-root operation after bind
- root filesystem intended read-only

**RISK**

Phase 3 raises the security bar substantially. Interception CA handling cannot be treated as just another TLS key. The CA private key, API TLS key, generated leaf certs, and imported certificates need separate ownership and explicit invariants.

## State Ownership Map

| State | Current owner | Classification | Finding |
| --- | --- | --- | --- |
| Compiled matcher | `fah-rules::ListManager` | SUPPORTED | Atomic snapshot model is good. |
| Policy set | `fah-rules::ListManager` | SUPPORTED | Immutable published policy set is good. |
| Active policy schedules | `PolicyState` wired by binary | SUPPORTED | Shared by DNS and HTTP; should be reused by Phase 3. |
| DNS cache | `fah-dns::DnsCache` | VALIDATED | Bounded by entries and answer bytes; overhead caveats documented. |
| SWR refresh | DNS pipeline plus binary-spawned workers | SUPPORTED | Needs review with Adaptive DNS endpoint timing. |
| Upstream DNS state | `fah-dns::upstream::UpstreamPool` | RISK | Counters exist, but there is no health model yet. |
| HTTP connection cap | `fah-http::Server` | SUPPORTED | Max connection semaphore is a good current control. |
| HTTP upstream pool | `fah-http::Proxy` / hyper client | SUPPORTED | Idle pool bounded by config. |
| Stats/history | `fah-stats` | SUPPORTED | Aggregation centralized; history is retention-based, not byte-capped. |
| Metrics | `fah-metrics` | SUPPORTED | Good for current DNS/HTTP; insufficient for Phase 3. |
| API config state | `fah-api::ConfigStore` | SUPPORTED | Boot/runtime classification is explicit. |
| API key | `fah-api` under `/config` | SUPPORTED | Single-key model is coherent for current appliance. |
| API TLS cert | `fah-api` under `/config` | SUPPORTED | Fine today; not enough for Phase 3 cert ownership. |
| Interception CA | Not implemented | RISK | Needs owner, interface, storage, import/export rules. |
| Generated leaf cert cache | Not implemented | RISK | Needs hard memory cap and telemetry. |
| HTTPS splice/intercept state | Not implemented | RISK | Needs owner before Phase 3 code starts. |

## Top Architectural Risks

### 1. Adaptive DNS Has No Endpoint-Health Module Yet

Classification: **RISK**

Current upstream forwarding walks configured servers in order. Attempts, failures, and consecutive failures are recorded, but they do not change selection behavior. The accepted Adaptive DNS design already identifies this as the core defect.

Impact:

- A dead primary can impose timeout cost on every query indefinitely.
- Ordered fallback hides client-visible success behind repeated internal failure.
- A naive health implementation could penalize endpoints using the wrong signals.

Required architectural response:

- Define a real endpoint-health module.
- Define endpoint identity.
- Define transport-only failure classification.
- Define skip and recovery behavior.
- Define telemetry before shipping.
- Keep state bounded by endpoint count, not by domain or client.

### 2. `resolve_host` Can Poison Future Health State

Classification: **RISK**

`resolve_host` currently performs A and AAAA lookups through the same forwarding path. Today this affects only counters. Under Adaptive DNS, it could affect endpoint health unless deliberately isolated.

Impact:

- An IPv6 family blackhole could penalize a healthy endpoint.
- HTTP origin resolution could distort DNS client-facing health.
- List/bootstrap traffic could affect user query behavior.

Required architectural response:

- Separate health influence by caller class.
- At minimum distinguish client query, SWR refresh, HTTP host resolution, list bootstrap, and encrypted-upstream bootstrap.
- Preserve partial-family success semantics for `resolve_host`.

### 3. SWR and Adaptive DNS Interactions Are Underspecified

Classification: **RISK**

SWR refreshes may account for a large share of upstream attempts. Treating them exactly like client-visible queries can distort health. Ignoring them entirely can delay detection of broken upstreams.

Impact:

- Endpoint health may reflect background refresh traffic more than user traffic.
- Refresh duplicate behavior can increase when adaptive retry windows expand.
- Failure metrics can misrepresent user-visible impact.

Required architectural response:

- Decide whether SWR failures affect health.
- If they do, define their weight and telemetry.
- Recheck refresh claim lease against configured endpoint count and timeout.

### 4. Phase 3 Certificate Ownership Is Not Decided

Classification: **RISK**

Current API TLS certificate handling lives in `fah-api`. Phase 3 requires CA generation/import/export, generated leaf certificates, TLS interception, DoT/DoH listener certs, and probably certificate API routes.

Impact:

- Duplicated certificate logic across API and HTTP.
- Harder security review.
- Risk of confusing API TLS certs with interception CA.
- Risk of unsafe import/export behavior.

Required architectural response:

- Write an ADR before implementation.
- Prefer a shared certificate module/crate with a small interface and strict invariants.

### 5. Phase 3 Event and Telemetry Model Is Too Narrow

Classification: **RISK**

Current event model has DNS and HTTP concepts. Phase 3 requires more protocol and transport dimensions.

Missing concepts include:

- HTTPS SNI pass/block/splice
- HTTPS interception
- DoT
- DoH
- TLS handshake latency
- upstream TLS verification failure
- generated leaf cert cache hit/miss
- splice byte counts
- HTTP/1.1 vs HTTP/2
- ECH/no-SNI policy outcome

Impact:

- Phase 3 can appear healthy while hiding latency or security failures.
- Benchmark rows cannot map to real telemetry.
- API/TUI can mislead operators.

Required architectural response:

- Define event taxonomy before Phase 3 code.
- Do not overload current `http.forward` to represent HTTPS internals.

### 6. RouterOS IPv6 and 443 Steering Are Not Closed

Classification: **RISK**

The deployment docs already note IPv6 HTTP bypass behavior under current IPv4 NAT rules. Phase 3 must not assume port 443 steering is solved by the existing port 80 model.

Impact:

- HTTPS filtering/interception may work for IPv4 but silently bypass IPv6.
- Rotating global IPv6 prefixes can break hardcoded rules.
- Proxy self-traffic may be accidentally intercepted.

Required architectural response:

- Create a RouterOS 443/IPv6 deployment decision.
- Include skip lists, self-traffic exclusion, ULA behavior, prefix rotation, and verification commands.

### 7. DoH Hostname Bootstrap Depends on OS Resolver

Classification: **ASSUMPTION/RISK**

Encrypted DoH upstreams with hostname URLs may use OS resolver bootstrap during connection setup. That weakens the appliance story where FastAdHunter should not depend on RouterOS `/etc/resolv.conf`.

Impact:

- DoH hostnames may fail before FastAdHunter's own resolver is usable.
- RouterOS one-time resolver file behavior can leak into upstream availability.

Required architectural response:

- Prefer IP-literal DoH upstreams in appliance mode, or
- define explicit bootstrap resolver behavior, or
- document hostname DoH as an operational constraint.

### 8. Phase 3 Memory Budget Is Undefined

Classification: **RISK**

Current cache, stats, and queues have bounds. Phase 3 adds memory owners not yet bounded:

- generated leaf cert cache
- TLS session cache
- TLS buffers
- pending handshakes
- HTTP/2 streams
- splice buffers
- expanded event volume

Impact:

- RB5009-class deployments can regress from bounded appliance behavior to unbounded proxy behavior.

Required architectural response:

- Define hard caps before implementation.
- Add telemetry for each new memory owner.

### 9. Blocking Semantics May Diverge by Protocol

Classification: **RISK**

DNS, HTTP, SNI, and intercepted HTTPS cannot all block in the same wire-level way. But the policy meaning must remain consistent.

Impact:

- Operators may see one policy behave differently across protocols.
- TUI/API policy reporting may become misleading.
- Future `blocking_mode` support may fragment.

Required architectural response:

- Write a cross-protocol blocking semantics ADR.
- Map one policy verdict to protocol-specific response behavior.

## Unvalidated Assumptions

- **ASSUMPTION:** Existing DNS and HTTP measurements predict HTTPS/TLS performance well enough. They probably do not.
- **ASSUMPTION:** Existing generic HTTP stream handling is enough for intercepted HTTPS and HTTP/2.
- **ASSUMPTION:** SWR refresh behavior remains bounded after endpoint skipping and retry behavior change.
- **ASSUMPTION:** Current event channel capacity remains adequate once HTTPS/SNI/DoT/DoH events are added.
- **ASSUMPTION:** API TLS cert lifecycle and interception CA lifecycle can share machinery without changing operator semantics.
- **ASSUMPTION:** RouterOS IPv6 steering can be made equivalent to IPv4 steering without special-case operational rules.
- **UNKNOWN:** Whether HTTP event drops are fully visible in top-level telemetry.
- **UNKNOWN:** Whether hickory cancellation behavior is safe enough for future hedging or racing.
- **UNKNOWN:** Real SNI sniff cost on RB5009.
- **UNKNOWN:** Real TLS interception handshake cost on RB5009.
- **UNKNOWN:** Generated certificate cache hit/miss and memory behavior under household traffic.
- **UNKNOWN:** DoT/DoH listener latency and memory cost on target hardware.

## Required Changes Before Adaptive DNS

### 1. Add an Endpoint-Health Module

Classification: **REQUIRED**

Adaptive DNS needs a module with a small explicit interface and enough internal depth to own endpoint selection.

Minimum responsibilities:

- stable endpoint identity
- health state per endpoint
- transport failure classification
- penalty state
- skip decision
- recovery decision
- bounded counters
- telemetry snapshot

Invariant:

- If endpoints exist, selection must always return a candidate.

### 2. Define Health-Affecting Outcomes

Classification: **REQUIRED**

Adaptive DNS must distinguish transport outcomes from DNS response outcomes.

Can affect health:

- timeout
- connect failure
- UDP/TCP/TLS/HTTP transport failure
- malformed transport response
- connection reset

Must not affect health by itself:

- NXDOMAIN
- NOERROR
- SERVFAIL
- REFUSED
- other DNS RCODEs

SERVFAIL can still trigger stale serving in the DNS pipeline. That is separate from endpoint health.

### 3. Isolate Caller Classes

Classification: **REQUIRED**

The architecture must decide which callers can influence endpoint health.

Caller classes:

- client DNS query
- SWR refresh
- HTTP `resolve_host`
- list bootstrap/fetch
- DoH hostname bootstrap
- future DoT/DoH client-facing queries

Recommendation:

- Client DNS queries should be the primary health signal.
- `resolve_host` family failures should not poison global health.
- SWR should either be lower-weight or separately reported.
- Bootstrap failures should be visible but isolated.

### 4. Add Required Telemetry

Classification: **REQUIRED**

Stage 1 needs telemetry before it can be trusted.

Required fields or equivalent:

- endpoint selected
- endpoint skipped
- skip reason
- penalty state
- transport failures by endpoint
- recovery count
- failure run length
- client-visible failure after fallback
- SWR-originated attempts
- resolve-host-originated attempts
- bootstrap-originated attempts

### 5. Keep Stage 1 Narrow

Classification: **REQUIRED**

Adaptive DNS Stage 1 should not include:

- latency racing
- hedging
- EWMA endpoint scoring
- per-domain health
- per-client health
- unbounded dynamic endpoint state

Those belong after Stage 1 health semantics are proven.

## Required Changes Before Phase 3

### 1. Certificate Ownership ADR

Classification: **REQUIRED**

Decide where certificate logic lives before implementation.

The decision must cover:

- API TLS cert
- interception CA
- generated leaf certs
- import/export routes
- private key storage
- key permissions
- reload vs restart behavior
- telemetry and audit safety

Recommendation:

- Create a shared certificate module/crate rather than duplicating logic in `fah-api` and `fah-http`.

### 2. HTTPS Module ADR

Classification: **REQUIRED**

Define the Phase 3 HTTPS module and its interface.

It should own:

- TCP accept for HTTPS path
- ClientHello parsing
- SNI extraction
- ECH/no-SNI handling
- splice path
- interception decision
- downstream TLS accept
- upstream TLS client
- handoff into HTTP proxy path
- HTTPS event emission

The module should expose a small interface to the binary. Its implementation can be complex, but callers should not need to understand TLS internals.

### 3. Event and Telemetry ADR

Classification: **REQUIRED**

Define the event model before adding Phase 3 behavior.

Required dimensions:

- protocol path: DNS, HTTP, HTTPS-SNI, HTTPS-intercepted, DoT, DoH
- transport: UDP, TCP, TLS, HTTP/2 where applicable
- verdict: pass, block, intercept, splice, error
- TLS stage: client hello, downstream handshake, upstream handshake, verification
- certificate cache: hit, miss, generate, fail
- byte counters for splice/intercept paths
- latency stages for each path

Backward compatibility for API/TUI should be designed deliberately.

### 4. Cross-Protocol Blocking ADR

Classification: **REQUIRED**

One policy verdict must map predictably across protocols.

Decisions needed:

- DNS block response mode
- HTTP block response mode
- SNI block behavior
- intercepted HTTPS block behavior
- ECH behavior
- no-SNI behavior
- per-client interception opt-in
- exclusions that always splice

### 5. RouterOS IPv6 and 443 Deployment ADR

Classification: **REQUIRED**

Define the real deployment architecture for HTTPS steering.

Must cover:

- IPv4 port 443 redirect
- IPv6 port 443 redirect
- skip lists
- FastAdHunter self-traffic exclusion
- ULA routing
- rotating global prefixes
- ECH/no-SNI expectations
- verification commands
- known bypass cases

### 6. Phase 3 Memory Budget ADR

Classification: **REQUIRED**

Define hard caps before implementation.

Required caps:

- active HTTPS connections
- pending TLS handshakes
- generated leaf cert cache entries and bytes
- TLS session cache
- HTTP/2 concurrent streams
- splice buffer size
- intercepted request body buffering, if any
- event queue pressure
- history growth from new events

### 7. Phase 3 Security Invariants

Classification: **REQUIRED**

The following must be testable invariants:

- non-listed clients are never intercepted
- excluded domains always splice
- CA private key never leaves `/config`
- public CA export never includes private material
- API TLS cert and interception CA cannot be confused
- upstream certificate failures are never masked
- generated leaf certs are bounded and auditable
- no-SNI behavior is explicit
- ECH behavior is explicit
- plain HTTP API mode remains explicitly unsafe

## Architectural Invariants To Preserve

- One matcher remains the source of truth for rule decisions.
- One policy identity model is used by DNS, HTTP, SNI, and intercepted HTTPS.
- Rule evaluation happens before DNS cache use.
- Blocked DNS answers are not stored as upstream cache entries.
- DNS RCODEs do not determine upstream transport health.
- Hot paths avoid global locks.
- Long-running workers are owned by the binary or an explicit runtime module.
- L3 sibling modules do not depend on each other directly without an ADR.
- `/config` owns durable config and secrets.
- `/data` owns runtime history and non-secret runtime artifacts.
- API auth is required except explicitly public health endpoints.
- API TLS cert lifecycle and interception CA lifecycle remain distinct.
- RouterOS deployment behavior is treated as architecture.
- Every Phase 3 state owner has a memory bound.
- Every Phase 3 protocol path has telemetry.

## Measurement Gaps

### Adaptive DNS

- Failure run-length distribution is not available from the current build.
- Per-endpoint skip and recovery behavior is not observable yet.
- Client-visible failure after fallback is not clearly separated from per-attempt failure.
- SWR attempts are not sufficiently separated from client attempts for health decisions.
- `resolve_host` traffic is not isolated in telemetry.
- DoH hostname bootstrap behavior is not measured in appliance deployment.

### Phase 3

- No SNI sniff benchmark.
- No splice throughput benchmark.
- No TLS handshake benchmark on RB5009.
- No generated certificate latency benchmark.
- No generated certificate cache memory benchmark.
- No HTTP/2 stream memory benchmark.
- No DoT/DoH listener latency benchmark.
- No event-volume benchmark for HTTPS/SNI traffic.
- No IPv6 443 interception verification.
- No ECH/no-SNI deployment measurement.

### Operations

- RouterOS memory accounting remains tricky.
- `/data` history growth is retention-based, not byte-capped.
- Existing measurements should keep a control arm.
- x86 results should not be promoted to RB5009 truth without measured scaling.
- Another container can affect memory observations on the same target.

## Recommended ADRs

### ADR 1: Adaptive Upstream Health and Selection

Decide endpoint identity, health state, failure classification, skip behavior, recovery, telemetry, and interactions with SWR and `resolve_host`.

### ADR 2: Certificate and CA Ownership

Decide whether cert logic lives in a new crate/module, what owns API TLS certs, what owns interception CA, and how private key material is stored, imported, exported, rotated, and audited.

### ADR 3: HTTPS Transport Architecture

Define the module and interface for SNI sniffing, splicing, interception, TLS accept, upstream TLS, and HTTP handoff.

### ADR 4: Cross-Protocol Policy and Blocking Semantics

Define how one policy verdict maps onto DNS, HTTP, SNI-only HTTPS, and intercepted HTTPS.

### ADR 5: Event and Telemetry Taxonomy

Define event kinds, transport dimensions, latency stages, counters, and backward-compatible API/TUI behavior.

### ADR 6: RouterOS IPv6 and Port 443 Steering

Define deployment rules, bypass expectations, self-traffic exclusion, rotating prefix handling, and verification.

### ADR 7: Phase 3 Memory Budget

Define hard limits and measurement requirements for TLS, HTTPS, generated certs, splice buffers, HTTP/2 streams, and new event volume.

## Go / No-Go

### Adaptive DNS Stage 1

Decision: **GO WITH REQUIRED CHANGES**

Stage 1 can begin, but the first implementation slice must be architectural:

- endpoint health state
- transport-only failure classification
- caller-class isolation
- SWR interaction rules
- endpoint telemetry
- measurement gates

Do not ship Stage 1 until the benchmark and live measurement gates can observe the behavior it is supposed to improve.

### Adaptive DNS Stage 2 and Stage 3

Decision: **NO-GO FOR NOW**

Latency-aware selection, racing, hedging, or more advanced adaptive behavior should wait until Stage 1 health semantics are proven and hickory cancellation or timeout behavior is understood.

### Phase 3 Reboot

Decision: **NO-GO TODAY**

The current architecture is a good base, but Phase 3 needs architecture decisions first:

- certificate ownership
- HTTPS module ownership
- policy semantics
- event taxonomy
- memory bounds
- RouterOS IPv6/443 deployment model
- security invariants

After those ADRs exist, Phase 3 can begin as small vertical slices.

## Final Finding

FastAdHunter is not suffering from a shallow-module problem today. The current design has good locality:

- DNS owns DNS behavior.
- HTTP owns HTTP proxying.
- Rules own matching and policy compilation.
- Stats owns aggregation.
- API owns the control surface.
- The binary owns composition.

The main danger is future pressure. Adaptive DNS and Phase 3 will both introduce hidden state that does not exist today. If that state is named, bounded, measured, and owned before implementation, the architecture can grow cleanly. If it is added opportunistically, FastAdHunter may keep working in simple cases while becoming hard to reason about under exactly the RouterOS deployment conditions it is meant to handle.
