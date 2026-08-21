# I want an architecture audit, not a design proposal and not code

I want to determine whether main @ 37e5a8f is truly the correct foundation for the next phases, and which architectural assumptions from Phase 2 are still unvalidated.

## Task: Global Architecture Review — FastAdHunter before Adaptive DNS and Phase 3 reboot

 We are now at the clean post-Phase-3-reset `main` baseline:

 ```text
 main → 37e5a8f
 ```

 phase2-stable → a5646a1
 origin/main → 37e5a8f
 backup/main → 37e5a8f

 The old Phase 3 is no longer present in any active branch, remote-tracking ref, tag, or worktree. Its history exists only in the verified offline bundle.

 The three performance optimizations and the `tui-monitor` fix have been reintroduced and independently revalidated on the Phase 2 tree.

 We are **not implementing anything yet**.

### Objective

 Perform a **read-only global architecture review** of the current repository and determine whether this is the architecture we should build the next year of FastAdHunter on.

 This is not a code review and not a feature review.

 Do not assume the Phase 2 architecture is correct merely because Phase 2 is closed.

 The dual-stack upstream discovery is the model problem: a significant architectural assumption remained unvalidated until Phase 3 exposed it. The purpose of this review is to find the next such assumptions **before implementation**.

### Scope

 Review the complete system architecture, including at minimum:

 1. **System boundaries**
    - client → MikroTik → FAH → Internet
    - DNS path
    - HTTP path
    - future HTTPS/SNI/TLS-interception path
    - upstream DNS transport
    - RouterOS/container boundary
 2. **Layering and crate (all) boundaries**
    - `fah-api`
    - `fah-common`
    - `fah-config`
    - `fah-dns`
    - `fah-http`
    - `fah-logging`
    - `fah-metrics`
    - `fah-model`
    - `fah-rules`
    - `fah-stats`
    - `fastadhunter`
    - `tui-monitor` (outside crates)
    Identify any dependency or ownership direction that is architecturally wrong, fragile, or likely to create future layering violations.
 3. **Shared policy semantics**
    - Where is allow/block actually decided?
    - Is there one authoritative policy-resolution path?
    - Can DNS, HTTP, SNI, and future interception evaluate the same request under different identities or policies?
    - Are there duplicated semantics that should instead be shared?
 4. **State ownership and lifecycle**
     For every major stateful subsystem, identify:
    - owner;
    - lifecycle;
    - mutability;
    - boot-only vs live configuration;
    - restart requirements;
    - failure behavior;
    - concurrency model.
    Pay particular attention to:
    - ruleset;
    - policy/client state;
    - DNS cache;
    - upstream state;
    - TLS/CA state;
    - telemetry/history;
    - HTTP/TLS connection state.
 5. **Upstream DNS architecture**
     Review:
    - IPv4/IPv6;
    - ordered fallback;
    - endpoint health;
    - `resolve_host`;
    - SWR;
    - transport failure semantics;
    - DNS RCODE semantics;
    - UDP/TCP/DoT/DoH architecture;
    - source-address behavior;
    - reconnect behavior after IPv6 prefix rotation.

    Do not design Adaptive DNS here.
    Instead, identify the architectural requirements that Adaptive DNS must satisfy.
 6. **HTTP/HTTPS architecture**
     Review the intended future model:

    ```text
    client
    ```

      → MikroTik
      → FAH
      → SNI / TLS policy
      → TLS interception
      → HTML filtering
      → upstream

    Identify:
    - where TLS termination belongs;
    - certificate/CA ownership;
    - connection lifecycle;
    - replay/splice boundaries;
    - whether the architecture permits transparent pass-through;
    - whether HTML filtering is correctly isolated from TLS concerns;
    - whether any future P3 feature is currently fighting the existing architecture.
 7. **Performance architecture**
     Do not optimize.

    Identify the architectural hot paths and define where performance contracts need measurement:
    - DNS query;
    - cache hit;
    - block;
    - upstream selection;
    - policy resolution;
    - matcher;
    - SNI parse;
    - TLS interception;
    - splice/copy;
    - HTML filtering;
    - telemetry/history.
    Explicitly identify measurements that are currently missing and could later lead to another “we only discovered it in Phase 3” situation.
 8. **Memory architecture**
     Review:
    - bounded vs unbounded state;
    - ownership of large structures;
    - cache bounds;
    - history retention;
    - allocator interaction;
    - transient allocations;
    - per-connection memory;
    - compile-time vs steady-state memory.
    Do not tune constants; identify architectural risks and missing invariants.
 9. **Failure isolation**
     For each subsystem, answer:

    ```text
    what can fail?
    ```

    what state changes?
    what remains available?
    what recovers automatically?
    what requires restart?

    Look specifically for failures in one subsystem that could accidentally poison another subsystem's health/state.
 10. **Security architecture**
      Review:
     - trust boundaries;
     - CA/private-key ownership;
     - API/config mutation boundaries;
     - certificate import/export;
     - logging/secrets;
     - client identity;
     - TLS interception trust model;
     - malformed/hostile input boundaries.
 11. **Observability architecture**
      Determine whether the telemetry model can actually answer:
     - what path a request took;
     - which subsystem caused latency;
     - whether an upstream failed;
     - whether a failure was client-visible;
     - whether memory is growing;
     - whether a performance change actually helped.
     Identify observability gaps that should be fixed structurally before future feature work.
 12. **Deployment architecture**
      Separate:
     - portable FAH architecture;
     - RouterOS/container-specific behavior;
     - RB5009-specific assumptions;
     - WAN/IPv6 assumptions;
     - future public/private service exposure.

### Required review method

 For every major architectural conclusion, classify it as:

- **VALIDATED** — supported by code + tests + measured deployment evidence;
- **SUPPORTED** — code/design is coherent, but deployment evidence is incomplete;
- **ASSUMPTION** — currently unvalidated;
- **RISK** — evidence or design indicates a real architectural problem;
- **UNKNOWN** — insufficient evidence to judge.

 Do not silently promote assumptions to facts.

### Required output

 Produce:

 1. **Architecture verdict**
    - `PASS`
    - `PASS WITH REQUIRED CHANGES`
    - `FAIL`
 2. **Current architecture map**
    - component boundaries;
    - data/control flow;
    - state ownership.
 3. **Top architectural risks**
     Ranked by impact, not by number of findings.
 4. **Unvalidated assumptions**
     Especially those that could become expensive to discover after Phase 3 has started.
 5. **Required architectural changes before Adaptive DNS**
 6. **Required architectural changes before Phase 3 reboot**
 7. **Architectural invariants**
     A concise list of properties future implementations must not violate.
 8. **Measurement gaps**
     What must be benchmarked or validated before future implementation.
 9. **ADRs**
     Identify which findings are significant enough to become explicit Architecture Decision Records. Do not create or edit ADR files yet; only recommend which decisions need one.
 10. **Go / no-go**
      State clearly whether:
     - Adaptive DNS Stage 1 can begin;
     - Phase 3 can later begin after Adaptive;
     - or more architecture work is required first.

### Important constraints

- Read-only.
- No code changes.
- No config changes.
- No benchmark changes.
- No RouterOS changes.
- No commits.
- No new dependencies.
- Do not rewrite the Adaptive DNS specification.
- Do not design Stage 2/3.
- Do not optimize anything.

The goal is not to produce a larger architecture document.
The goal is to identify **architectural mistakes or unvalidated assumptions before we build the next Phase 3**.

Be adversarial. If an architectural decision is only “probably fine”, mark it as an assumption rather than accepting it.

**DO NOT SKIP:**
You findings will be saved under `docs/code-review/CLAUDE-Global-Architecture-Review.md`.
