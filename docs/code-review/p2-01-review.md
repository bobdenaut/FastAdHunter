# p2-01 — HTTP Crate Scaffold and Doc Updates

**Task:** [plan/wip/phase2/p2-01-http-scaffold.md](../../plan/wip/phase2/p2-01-http-scaffold.md)
**Gates:** `fmt --check` clean · `clippy --workspace --all-targets -D warnings`
clean · `test --workspace` **497 passed, 0 failed** (was 484)

---

## 1. What landed

`fah-http` exists as an 11th crate at L3. It binds `[http.listen]`, accepts,
and closes — no proxying (p2-02), no filtering (p2-04).

The listener exists this early for one reason: **binding is the part that fails
in deployment.** Privileged ports, address families, a port already held — all
of it is cheaper to get right against an empty engine than against a proxy.

## 2. The duplication that was avoided

`fah-http` needs the same TCP bind `fah-dns` already had: dual-stack with
`IPV6_V6ONLY` cleared explicitly, plus the error message that distinguishes
`EACCES` from `EADDRINUSE`. Copying it would have been ~35 lines duplicated in
a spot where divergence is invisible — one engine serving IPv6 and the other
silently not is exactly the failure `docs/deploy-rb5009.md` §5 documents at
length.

They are L3 siblings and may not import each other, so it moved **down** to
`fah_common::listen` (L1) and both call it. `fah-dns` lost `dual_stack`,
`bind_udp`, `bind_tcp` and `bind_error`; its tests for them moved too, minus
one that stayed to pin that DNS still passes *its own* setting name into the
shared, now-parameterized error.

The cost is `tokio` + `socket2` on `fah-common`. That is zero in the dependency
graph: both existing dependents (`fah-rules`, `fah-api`) already carry tokio,
and socket2 was already in-tree. ARCHITECTURE.md's warning that `fah-common`
must not become a dumping ground still holds — the test for admission is
"behaviour two crates must not disagree about", and a bind that decides which
address families answer is that.

## 3. `dns` mode does not bind the port

The acceptance criterion, and the one design decision worth stating:
`engine.mode = "dns"` means the socket is **never created**, not created and
left unattended.

A bound-but-unserved port still holds the address against anything else on the
host, and still completes a client's `connect()` — from the client's side that
is indistinguishable from a hung proxy, and it is worse than a refused
connection, which fails fast and clearly.

`http_enabled` matches `EngineMode` exhaustively rather than with `_ => false`.
A fourth mode should fail to compile until someone decides what it means for
HTTP, instead of defaulting to "off" and leaving an operator with a mode that
names http and a port nothing listens on.

## 4. Config: `[http]` is boot, including the key that looks runtime

`max_connections` is boot. It looks runtime-shaped, but the semaphore is sized
once in `Server::bind`, so calling it runtime would report an apply that never
happens — the defect p1.5-07 found across ~16 keys and fixed. `BOOT_KEYS` lists
the section whole, matching the existing convention there.

`idle_timeout_ms` and `header_timeout_ms` **have no consumer yet** and their
doc comments say so. They are declared because the section should land complete
in CONFIGURATION.md, and flagged because a config key that silently does
nothing is the same lie in a smaller font. p2-02 gives them consumers.

Default port is **8080, not 80**. The container is unprivileged after
ADR-0004's drop, and the router dst-nats 80 to it — so HTTP never needs the
privileged bind that forced ADR-0004 on DNS in the first place.

Validation runs unconditionally, not only when the mode names http: the file is
written back whole, so a malformed `[http.listen]` should be rejected while the
operator is editing it, not on the restart months later that first turns the
mode on. `max_connections = 0` is rejected — a ceiling of zero accepts nothing
while looking configured.

## 5. The layering test earned its keep

`crates/fastadhunter/tests/layering.rs` failed on the first run:

```text
unknown crate `fah-http` — add it to layer()
```

That is the guard working as designed. It parses every manifest and refuses to
pass until a new crate is explicitly assigned a layer — so the crate could not
be added without someone stating where it sits. Both `layer()` and the
crate-count assertion were updated (10 → 11). ARCHITECTURE.md and CLAUDE.md now
point at this test, since neither previously mentioned that the rule is
enforced rather than merely written down.

## 6. Docs updated in the same change

| Doc | Change |
| --- | --- |
| ARCHITECTURE.md | `fah-http` in the overview, layout and layering; new **§HTTP Pipeline**; §Listeners restructured into a shared preamble + DNS/HTTP subsections |
| CONTEXT.md | new terms **HTTP Engine**, **Pass-through**, **Interception**; **Operating Mode** sharpened to say it is the *only* switch |
| CONFIGURATION.md | `[http]` + `[http.listen]` in the reference; mutability note explaining why the section is boot |
| PERFORMANCE.md | three HTTP rows in the budget table, marked *to measure* rather than invented |
| README.md | crate list (the "HTTP Engine (Phase 2)" box stays — still accurate, it does not proxy yet) |
| CLAUDE.md | layering rule, crate count 10 → 11, new reading-protocol row |
| diagrams | `architecture.svg` and `architecture.html` L3 row re-laid out for five boxes; `architecture-full.svg` already had `fah-http` drawn dashed as planned — now solid |

## 7. Deliberately not done

- **`fah-rules` is not a dependency yet.** The task describes L3 as "may depend
  on fah-rules and L1" — that is the permitted ceiling, not a requirement. An
  unused dependency now would be decoration; it arrives with p2-04, when
  verdicts are actually consulted.
- **No benches.** PERFORMANCE.md's HTTP rows say *to measure*. p2-02 benches
  the pass-through path before any filtering exists, so a later regression has
  a baseline to fail against — inventing a number here would give it a fake one.
- **Nothing deployed.** The 0.2.5 soak is running (T0 `2026-07-25T20:50:23Z`);
  this changes no runtime behaviour in `dns` mode, so there is no reason to
  disturb it.

## 8. Tests

Nine new, beyond the moved ones:

- `http_starts_only_in_modes_that_name_it` — the acceptance criterion, both
  directions.
- `bind_does_not_accept_until_serve_is_called` — ADR-0004's split is a property,
  not an implementation detail.
- `serve_accepts_then_closes_the_connection` — EOF, not a response.
- `max_connections_sizes_the_permit_pool_without_stalling` — the pool is sized
  from config and a ceiling of 1 throttles rather than deadlocks.

  **Named for what it checks, not for what the semaphore is for.** It does
  *not* prove the cap binds, and cannot here: the scaffold releases its permit
  the instant it closes, so no window exists in which a second connection could
  be made to wait. The first draft was called
  `concurrency_is_capped_by_max_connections`, which promised exactly that and
  would have read as covered. The real test needs connections with duration and
  belongs to **p2-02**, where a permit is held for the life of a transfer.
- `an_invalid_listen_address_is_rejected_by_section_name`,
  `the_hint_names_the_callers_own_setting` — an HTTP bind failure must not send
  an operator to `[dns.listen]`.
- `only_the_unspecified_v6_address_is_dual_stack`,
  `a_bare_ipv6_literal_parses_without_brackets` — the two facts the shared
  helper exists to keep identical across engines.
- `defaults_are_unprivileged_and_dual_stack` — pins 8080 against a future
  "shouldn't HTTP be on 80?".
