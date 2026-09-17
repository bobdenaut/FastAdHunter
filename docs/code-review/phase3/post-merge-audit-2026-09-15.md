# Post-merge audit — Phase 3 integration, `bc49e4e..78238b4`

Read-only. Nothing in the tree was changed. Scope agreed before the pass:
S1 the `ConnectionGauge` move, S2 the `eb693e2` hunks no earlier review names,
S3 instrumentation validity, S4 merge-window tests that could pass with the
wiring they are meant to prove broken.

Revised 2026-09-17: N2, SP2 and R2 closed, the R1 commit named, and the R1 file
table completed.

Companion documents, not repeated here:
[main-phase3-integration-audit.md](main-phase3-integration-audit.md) (F1–F9,
status-pass 2026-09-14), [plan/plan-merge.md](../../../plan/plan-merge.md)
§Step 2 and §Step 4, [p3-10-track-a-review.md](p3-10-track-a-review.md).

## Summary

- Base settled: **`bc49e4e`**, not the tag. `git merge-base 78238b4 bc49e4e` is
  `bc49e4e`, and it is the second parent of `eb693e2`. `main-pre-phase3-merge`
  (`ebc46f1`) is its parent, one commit behind, and that commit touches
  `plan/plan-merge.md` alone (+74/−29). The trees are identical under `crates/`;
  the tag stays a rollback point and the `E:/fah-main-bench` checkout, not a
  diff base.
- The hand-decision surface of `eb693e2` is **36 files**, not the 67 that
  `git show --cc --stat` prints — `--stat` also lists files taken whole from one
  side. Computed as the intersection of `git diff --name-only <parent> eb693e2`
  over both parents.
- **One confirmed defect (N1) — now RESOLVED.** Two config keys documented with
  a `FAH__` environment override that was not implemented, and whose use
  **failed boot**. Re-verified independently on 2026-09-15 against the built
  binary, with a supported variable as the positive control; the verdict and the
  Medium severity held, and the first-boot write below came from that pass. The
  two arms and the anti-drift coverage landed the same day — see §N1 Resolution.
- Everything else read in S1–S4 is correct. The gauge move releases on every
  path; the refusal split, the DNS gauges and `tasks_died` reach
  `/api/v1/telemetry`; CONFIGURATION.md and API.md describe the ceilings
  accurately, including their inert default.
- **A second pass on 2026-09-15**, after the N1 fix, hunted defects that a green
  suite can miss — scope in §Second pass, findings SP1–SP8. It was extended the
  same day with a listener-configuration sweep, which widened SP1 and added
  SP4–SP8. Three more confirmed defects, all on the configuration surface and
  all fail-closed at boot: **SP1** (Medium) — three of the six listener port
  pairs are compared by nothing, so a colliding pair validates clean and can sit
  latent until the boot that first enables it; **SP4** (Medium) — `[api] address`
  accepts any IPv6 literal, including the `::` CONFIGURATION.md offers, and then
  cannot bind it; **SP6** (Low) — the `AddrInUse` arm of `bind_error` drops the
  config key and tells the operator another process holds a port this process
  holds itself. **SP5** (Low) records that the API is the one listener whose bind
  failure never passes through `bind_error` at all.
- **All four are now RESOLVED**, same day, on the owner's approval and to a rule
  agreed before any code was written — §Second-pass resolutions carries the rule,
  the diff, the twelve new tests and the before/after reproductions. Two things
  there are not cleanup and are called out in their own subsection: the policy
  makes `https.listen.port` checked **less** often than before, and the e2e
  harness had to stop drawing duplicate ports. SP2, SP3 and SP8 were excluded by
  instruction and are untouched.
- **The two fix commits were then reviewed independently** — scope and findings
  in §Independent review of the fix commits, prefix `R`. One should-fix and six
  notes; no blocker, and no defect in the DNS or HTTP data path. **R1** was N1's
  class surviving in a second population: `fah-http`'s two `PORT_SETTING`
  constants advertised `FAH__HTTP__LISTEN__PORT` and `FAH__HTTPS__LISTEN__PORT`,
  which `apply_one` had no arm for, and the SP6 fix printed one of them on every
  port collision. **RESOLVED the same day** — the two arms landed, the five
  constants moved to `fah-config` where a test can reach them, and both mutation
  controls fail as they must (§R1 Resolution). R2 closed 2026-09-17; R3–R7 stay
  open as notes.

## Decisions

- Verdicts are against the shipped tree (`main` at `8fec41d`), not the merge
  commit. The merge is the decision surface; correctness is today's code.
- F1–F9, p3-10 Track A, p3-11 and the dashboard as a review surface were out of
  scope by instruction and were not reopened.
- A touched file is not a finding. S1, S2 and most of S3 produced no rows;
  §Established correct records what was settled, not what was read.
- `fah-common/src/connections.rs`, `cache.rs`, `qtype.rs`, `swr.rs`, `claim.rs`
  and `snapshot.rs` are **not** in the hand-decision set — git took one side
  whole. They were still read where S1 required it.

## Findings

### N1 — a documented environment override that stops the container from booting

**Status: RESOLVED 2026-09-15.** The row below is the state that was found, in
the past tense, and **every line number in it is anchored to `8fec41d`** — the
tip before the fix — so it stays verifiable after the arms shifted the file.
§N1 Resolution carries the current numbers. Severity and title are unchanged:
they describe the defect, not its disposition.

| | |
| --- | --- |
| Severity | **Medium** — operator-facing, fail-closed at boot; no risk to a running resolver |
| Class | **confirmed defect** + **documentation drift** + **coverage gap** |
| Evidence (at `8fec41d`) | `crates/fah-config/src/env.rs:42-127` — `apply_one` matched an explicit allowlist of paths, with no `["dns", "tcp_max_connections"]` and no `["dns", "udp_max_inflight"]` arm. The `_ =>` arm at `:120-126` returned `ConfigError::UnknownEnvKey`, which `apply_env_overrides` (`:19-32`) propagates, so the load failed. `CONFIGURATION.md:98` states `Env: FAH__DNS__TCP_MAX_CONNECTIONS`, `:116` states `Env: FAH__DNS__UDP_MAX_INFLIGHT`, and `:11-13` states the general `FAH__` contract. Splitting is on `__`, so both names resolve to two segments (`["dns", "tcp_max_connections"]`) — the names were right, the arms were missing. On a fresh `/config` volume the default TOML is written before the override is applied (`write_atomic` at `fah-config/src/lib.rs:50` precedes `apply_env_overrides` at `:56`), so the container leaves a defaults-only `fastadhunter.toml` on disk and then exits — a restart loop whose config file does not carry the operator's intent. That written file is also the manual escape: edit the TOML, drop the variable. **That ordering is unchanged and out of N1's scope** |
| Affected path | `crates/fah-config/src/env.rs`, `CONFIGURATION.md` §`[dns]` |
| Why it matters | These are the two keys F8 leaves as the owner's call. The RB5009 container already carries its overrides in `fah-env` (`FAH__RUNTIME__HTTP_RUNTIMES=2`, whose arm exists at `env.rs:45`), so environment variables are the deployment's working mechanism. An operator following CONFIGURATION.md to arm the UDP ceiling gets a resolver that does not start, and the error names the variable unknown rather than unsupported. Both keys are in `BOOT_KEYS` (`fah-api/src/config_store.rs:38-39`), so the API cannot apply them at runtime either — the TOML file is the only route that works |
| Coverage gap beside it (at `8fec41d`) | `env_unknown_key_is_rejected` (`fah-config/src/lib.rs:868-873`) pinned a typo, and `env_interception_paths_are_rejected` (`:886-900`) pinned two paths that are *meant* to be refused. Nothing pinned a documented variable to the arm implementing it, so the doc and the allowlist could diverge with the suite green. Both tests still exist and still pass, at `:950` and `:968` |
| Recommended action | Owner decision, two shapes: add the two arms (`coerce_usize`, mirroring `["runtime", "http_runtimes"]` at `env.rs:45-47`), or delete the two `Env:` lines and state the keys are file-only. Either way a test walking CONFIGURATION.md's `Env:` names against `apply_one` closes the gap for good. **Taken 2026-09-15: the first shape, plus the anti-drift test — see §N1 Resolution** |

#### N1 Resolution — 2026-09-15

Owner chose to implement rather than to delete the documentation.

| What | Where |
| --- | --- |
| The two missing arms, `coerce_usize`, mirroring `["runtime", "http_runtimes"]` at `:45` | `crates/fah-config/src/env.rs:49-54` (+7) |
| Each variable reaches its field; `"x"` is `InvalidEnvValue`; `0` still fails validation for `tcp_max_connections` and still means "no cap" for `udp_max_inflight` | `dns_tcp_max_connections_env_override_applies_and_is_validated`, `dns_udp_max_inflight_env_override_applies_and_zero_stays_no_cap` |
| Anti-drift: every `Env: FAH__…` name in CONFIGURATION.md must reach an `apply_one` arm. Behavioural, not source-text — it drives `apply_env_overrides` and rejects `UnknownEnvKey`, and fails if the scan finds fewer than three names, so a reformatted document cannot empty it silently | `every_env_variable_configuration_md_documents_has_an_override_arm` |
| Process-level proof, child process with `.env(...)` per the `healthcheck.rs` precedent, no `std::env::set_var`. Asserts stderr carries no "does not map to a known config key" **and** that the run reached `dns-probe`, so the test cannot pass by failing earlier | `the_documented_dns_ceiling_env_overrides_survive_a_real_config_load` |

Negative control: no mutation was needed. The pre-fix tree is the control — both
variables did return `UnknownEnvKey` there, reproduced on the built binary by the
independent re-verification, so both new tests would have failed on it.

CONFIGURATION.md is unchanged: the two `Env:` lines are now true. The
write-then-env ordering (`lib.rs:50` before `:56`) is untouched and out of N1's
scope — any unknown `FAH__` variable still leaves a defaults-only TOML before
the load fails.

### N2 — the three DNS connection gauges have no dashboard consumer

**Status: RESOLVED 2026-09-17.** The Health page's Backpressure card now carries
the three as `active / peak` rows plus the UDP `shed` count, with a footnote
naming the ceilings `peak` sizes
(`dashboard/frontend/src/pages/health/backpressure-card.tsx`); `Counters` in
`api/types.ts` models the three objects, and `diagnostics-health.test.tsx` pins
the four rows verbatim. The row below is the state that was found.

| | |
| --- | --- |
| Severity | Low |
| Class | **coverage gap** |
| Evidence | `dashboard/frontend/src/api/types.ts` models `tasks_died` (`:102`) and the refusal split (`:61`), and `pages/health/engine-card.tsx:31` renders `counters.tasks_died`. `dns_tcp_connections`, `dns_dot_connections` and `dns_udp_inflight` appear nowhere under `dashboard/frontend/src`. The API path is proven: written at `main.rs:1232-1234`, serialized at `fah-metrics/src/registry.rs:325`, shape pinned by `fah-api/tests/api.rs:1153-1162`, and read off the live binary by `shipped_path_e2e.rs:244` |
| Affected path | `dashboard/frontend/src/api/types.ts`, `dashboard/frontend/src/pages/health/` |
| Why it matters | These three are the sizing instruments for `[dns] tcp_max_connections`, `udp_max_inflight` and the compiled-in DoT cap — the figures CONFIGURATION.md:95-97 tells the operator to retune from after a soak. Today that reading is only available through `GET /api/v1/telemetry`, a different workflow from the one the Health page otherwise serves |
| Recommended action | Owner decision: a Health row for the three, or an explicit line saying they are telemetry-API-only sizing figures. Not a defect — nothing is wrong, one consumer is absent |

### N3 — `strategy_ab.rs` is an A/B with one arm

| | |
| --- | --- |
| Severity | Info |
| Class | **intentional deviation** |
| Evidence | `crates/fah-dns/tests/strategy_ab.rs:471-507` — both entry points are `#[ignore]` and loop `for strategy in [UpstreamStrategy::Adaptive]`; `strategy_name` at `:84-88` matches a single variant; the file contains **0** `assert`s. The disposition is recorded: [strategy-ab-fallback-vs-adaptive.md](../phase2.6/strategy-ab-fallback-vs-adaptive.md) §Files keeps it "as an `#[ignore]` reproducible regression and capacity corpus (adaptive only after the removal)" |
| Affected path | `crates/fah-dns/tests/strategy_ab.rs` |
| Why it matters | Named and shaped as a comparison it can no longer make. A reader who runs it expecting a second arm finds none, and the `{label}` suffix, the one-element loops and `strategy_name` are scaffolding for a variant that was deleted (principle 14). It discriminates nothing and is not claimed to |
| Recommended action | None required — the disposition covers the intent. If the file is next touched, flatten the loops and drop `strategy_name`, or rename it to what it now is. Not worth a change of its own |

## Second pass — targeted defect hunt, 2026-09-15

Separate scope from S1–S4, run after the N1 fix and recorded here rather than in
its own file because it covers the same tree. Read-only; nothing was changed.
**Line numbers below are anchored to `f32f214`**, not to `8fec41d`. Classes
asked for: config/doc/runtime mismatch, lifecycle leaks on error and abort
paths, telemetry inert under the shipped default, tests that pass with their
subject broken, bounds and exhaustion, serialization mismatch, cross-feature
interaction. Findings carry an **`SP` prefix** (second pass), not the `N` series
above: the `N` numbers belong to the merge audit, and `N4`/`N5` would read as a
continuation of it and collide with the `N5` that
[project-state.md](../../project-state.md) §Risk inventory already uses for
another review.

**Extended the same day with a listener-configuration sweep**, on one question:
which invalid, contradictory or mutually incompatible listener configurations
does the product accept as valid, and what happens afterwards. That sweep
widened SP1 — and corrected two of its claims — and produced SP4–SP8. It ran the
binary built from `f32f214` against scratch TOML files outside the repository,
since `--healthcheck` validates the config before it probes DNS: `healthcheck
failed: config:` means validation refused the file, `healthcheck failed:
dns-probe:` means validation passed it. The control for that discriminator is
`[https.listen] port = 8443`, refused with `must differ from [api] port (8443)`.
Startup behaviour was reproduced by running the binary on high ports with a
scratch `--config` and `--data`; the scratch directory was outside the tree and
was removed.

### SP1 — three pairs of the listener port-collision matrix are compared by nothing

**Status: RESOLVED 2026-09-15** — see §Second-pass resolutions. The row below is
the state that was found, and its line numbers stay anchored to `f32f214`, the
tip before the fix.

*Widened and corrected by the listener sweep. The first writing of SP1 said
`http.listen.port` "is compared with nothing" and that the `https` family had no
test; both are wrong. `http.listen.port` is a right-hand operand in **both**
loops (`lib.rs:143`, `:159`), so it is compared with `dot_port` and with
`https.listen.port`, and `the_https_listener_may_not_share_another_listener_port`
(`lib.rs:729-742`) covers all three `https` pairs. The defect is real but is a
different shape: it is about pairs, not about one key, and it is three pairs, not
one. The row below supersedes the earlier one.*

| | |
| --- | --- |
| Severity | **Medium** — operator-facing; fail-closed at boot, but the configuration that causes it validates clean, the API persists it, and the abort names no key at all |
| Class | **confirmed defect** + **coverage gap** |
| Evidence | `fah-config/src/lib.rs:138-170` builds two comparison loops and no third: `dot_port` against `{dns, api, http, https}`, **only when `dot_enabled`** (`:138-156`), and `https.listen.port` against `{api, http, dns}`, unconditionally (`:158-170`). Of the six pairs among `{dns, api, http, https}`, the three that do not involve `https` are in neither loop: **`dns–api`, `dns–http`, `api–http`**. Reproduced on the binary built from `f32f214`, with `--healthcheck` as the discriminator (validation first, DNS probe second): `[dns.listen] port = 5399` + `[api] port = 5399`, `[dns.listen] port = 5399` + `[http.listen] port = 5399`, and `[api] port = 8080` + `[http.listen] port = 8080` each passed validation and failed only at `dns-probe`. The control `[https.listen] port = 8443` was refused at `config:` |
| Affected path | `crates/fah-config/src/lib.rs` §`validate` |
| Why it matters | Every bind uses `?` — DNS `main.rs:409`, HTTP `:428`, HTTPS `:436`, API `:639` — and the API binds **last**, so on any colliding pair that includes it the surface needed to undo the value is the one that does not come up. Startup with DNS and the API both on `127.0.0.1:15399` aborts with `fastadhunter: Only one usage of each socket address (protocol/network address/port) is normally permitted. (os error 10048)` — no protocol, no address, no key (SP5). The latent path is the sharp one: `[http.listen]` is validated unconditionally **on purpose**, and the comment at `lib.rs:127-130` gives the reason — reject a malformed `[http.listen]` while the operator is editing it, "not on the restart months later that first turns the mode on". Yet `api.port` = `http.listen.port` is outside the matrix, so under the shipped `mode = "dns"` it is accepted, persisted and harmless; `POST /api/v1/config {"engine":{"mode":"dns+http"}}` then runs the same `validate` (`fah-api/src/config_store.rs:129-131`), passes, answers `restart_required: true`, and the restart does not come back. `8080` is the HTTP proxy's own default, so the pair is not exotic |
| Coverage gap beside it | Both families that exist **are** tested — `dot_port_must_not_collide_with_another_listener_unless_dot_is_off` (`lib.rs:692-708`) and `the_https_listener_may_not_share_another_listener_port` (`:729-742`, all three `https` pairs). That is the gap: every collision test in the suite asserts a pair the code already checks. Nothing enumerates the matrix, so an absent pair is indistinguishable from a present one and the suite stays green |
| Recommended action | Owner decision. Smallest verification is unchanged in shape: a table test driving all ten pairs of `{dns, dot, http, https, api}` through `validate` and requiring an error for each. It fails on **three** pairs today. The `dot` pairs need the table to say what it expects while `dot_enabled = false`, which is where SP7's question about one policy for all five listeners lands |

### SP2 — the DoT leaf pre-warm is awaited outside the handshake deadline, holding an accept permit

**Status: RESOLVED 2026-09-17.** The pre-warm is awaited inside
`timeout_at(deadline, …)`, the same deadline that already bounded the
ClientHello read and `into_stream` on either side of it (`dot.rs:141-150`), so
a slot is held at most `HANDSHAKE_TIMEOUT` past accept. On expiry the
connection is closed like any other handshake timeout; a mint already
dispatched to the blocking pool runs to completion and its leaf still lands in
the store. Pinned by
`a_stalled_leaf_pre_warm_is_cut_at_the_handshake_deadline_and_frees_its_slot`:
a one-thread blocking pool held by a stalled task, one SNI connection, closed at
the 300 ms deadline with the gauge back to 0. On the pre-fix code the same test
waits out its 5 s guard and fails. The row below is the state that was found.

| | |
| --- | --- |
| Severity | **Low** — structural, not observed; the measured mint cost argues against it being live |
| Class | **potential issue** |
| Evidence | `fah-dns/src/dot.rs:118-156` — `deadline` wraps the ClientHello read (`:119`) and `into_stream` (`:147`), but `prewarm(...).await` at `:143-146` sits between them **untimed**. The permit is taken before `accept` (`:70`) and held for the connection's life (`:101`), so a slow mint holds one of `DOT_MAX_CONNECTIONS = 64` slots for a time nothing bounds. Any SNI from any client mints when a CA exists (`:134-136`), with no allow-list and no rate limit, on the shared blocking pool (`:188`) |
| Affected path | `crates/fah-dns/src/dot.rs` |
| Why it matters, and why it is only Low | PERFORMANCE.md:64 measures `certs_mint` at **450.88 µs on the RB5009** (criterion, 2026-09-04), below the TLS handshake it precedes — 64 parallel mints are ~29 ms of CPU across four cores. So the bound is nominal rather than absent in practice. [fah-certs-leaf-cache-audit.md:67](fah-certs-leaf-cache-audit.md) states the same reliance for the *same*-host burst — "the only thing bounding it is that minting is fast, which nothing enforces" — where single-flight serialises the waiters. The **unique**-host case on DoT is not covered there and does not serialise |
| Recommended action | Owner decision; nothing owed on the measured evidence. Smallest verification: an injected slow mint, or `max_blocking_threads = 1` with several unique hosts, measuring `accept`-to-permit-release. If it exceeds `HANDSHAKE_TIMEOUT` (`dot.rs:28`, 10 s) the limit is nominal |

### SP3 — `wiring.rs` asserts on main.rs source text, not on behaviour

| | |
| --- | --- |
| Severity | Info |
| Class | **intentional deviation** |
| Evidence | `fastadhunter/tests/wiring.rs:4-12` reads `src/main.rs` as a string; the assertions are `contains("self.reap_dead_tasks().await")` (`:31`), `contains("record_task_death")` (`:51`) and `contains("metrics.set_requests_refused(refusals)")` (`:60`). They pass while the call is *written*, whether or not it runs — an `if false` around it keeps them green |
| Affected path | `crates/fastadhunter/tests/wiring.rs` |
| Why it matters | Recorded only so a later reader does not mistake them for wiring proofs. The intent is declared in their own failure messages, which cite p3-10 A4 and F11, and the behavioural proof exists beside them: `acceptor_death.rs:157-182` boots the binary and reads `tasks_died` off live telemetry |
| Recommended action | None. `acceptor_death.rs` is the test that discriminates |

### SP4 — `[api] address` accepts any IPv6 literal and then cannot bind it

**Status: RESOLVED 2026-09-15** — see §Second-pass resolutions. Line numbers
below stay anchored to `f32f214`.

| | |
| --- | --- |
| Severity | **Medium** — operator-facing, fail-closed at boot. The worst of the Mediums: the value is one CONFIGURATION.md offers, the API accepts and persists it, and the restart the API asks for is what removes the only surface that could undo it |
| Class | **confirmed defect** + **documentation drift** + **coverage gap** |
| Evidence | `fah-api/src/server.rs:59-64` assembles the bind address as `format!("{address}:{port}").parse::<SocketAddr>()`. That syntax requires brackets around an IPv6 literal, so `"::"` becomes `":::8443"` and fails to parse. `validate` accepts the same string: `validate_ip("api.address", …)` (`fah-config/src/lib.rs:126`, `:498-506`) parses it as an `IpAddr`, where `"::"` is valid. `fah-common/src/listen.rs:29-35` exists for exactly this and its doc comment names the trap — "an IPv6 literal needs brackets in socket-address syntax, so the string round-trip would reject `::` as `:::53`" — and DNS (`fah-dns/src/server.rs:53`), HTTP (`fah-http/src/server.rs:58`) and HTTPS (`fah-http/src/tls_server.rs:28`) all call it. The API does not. Reproduced on the built binary: `[api] address = "::"` passes `--healthcheck` (reaches `dns-probe`), then `ERROR fastadhunter: fastadhunter failed to start error=invalid [api] address: invalid socket address syntax` — after the API key, the dashboard password, the self-signed certificate and `interception.json` have already been generated and logged |
| Documentation drift | `CONFIGURATION.md:436` hands the operator the value: "address, when set to a literal IP rather than `0.0.0.0` or `::`, is included in the SANs…". `::` is presented as one of the two normal non-literal choices, and it is unbindable |
| Affected path | `crates/fah-api/src/server.rs`, `CONFIGURATION.md` §`[api]` |
| Why it matters | `api.address` is in `BOOT_KEYS` (`fah-api/src/config_store.rs:56`), so `POST /api/v1/config` merges it, runs the same `validate`, writes the TOML and answers `restart_required: true` — a documented "fine, restart". Recovery is then only a hand edit of `/config/fastadhunter.toml` or a `FAH__API__ADDRESS` override plus a restart. Second consequence, independent of the crash: the API and the dashboard cannot listen on IPv6 at all, and DoH rides the API listener (`main.rs:625-627`), so DoH is IPv4-only while the DNS listeners are dual-stack by deliberate design (`fah-common/src/listen.rs:7-13`) |
| Coverage gap beside it | Every call site in the suite passes an IPv4 literal — `fah-api/tests/api.rs:629` and `fastadhunter/tests/history_e2e.rs:472` both use `"127.0.0.1"`. No IPv6 address ever reaches `ApiServer::bind`, so the suite is green and consistent |
| Recommended action | Owner decision. Smallest verification: `ApiServer::bind("::1", 0, None, state)` in `fah-api/tests/api.rs` |

### SP5 — the API is the one listener whose bind failure never passes through `bind_error`

**Status: RESOLVED 2026-09-15** — see §Second-pass resolutions. Line numbers
below stay anchored to `f32f214`.

| | |
| --- | --- |
| Severity | **Low** — diagnostic only, but it is what SP1 and SP4 are read through |
| Class | **confirmed defect** (diagnostic) |
| Evidence | `fah-api/src/server.rs:65` is a bare `TcpListener::bind(addr).await?`. The other three wrap the failure: `fah-dns/src/server.rs:56-61`, `fah-http/src/server.rs:59-61`, `fah-http/src/tls_server.rs:29-31` all call `fah_common::listen::bind_error`, which names the socket type, the address, the config key and — the point of ADR-0004 gate 2 — separates `EACCES` from `EADDRINUSE`. Reproduced: DNS and API on `127.0.0.1:15399` aborts with `fastadhunter: Only one usage of each socket address (protocol/network address/port) is normally permitted. (os error 10048)`, nothing more. The same collision one crate over, DNS against HTTP, prints `binding TCP 127.0.0.1:15399: … — another process in this network namespace already holds it` |
| Affected path | `crates/fah-api/src/server.rs` |
| Why it matters | Two arms. The one reproduced above is the port collision SP1 lets through. The second was read, not reproduced — Windows binds low ports freely, so `EACCES` could not be shown here: the API binds at `main.rs:639`, **after** the privilege drop at `:480`, so `[api] port = 443` validates clean and fails with a bare `Permission denied` without the `CAP_NET_BIND_SERVICE` hint `bind_error` exists to supply (`fah-common/src/listen.rs:100-104`). ADR-0004's whole argument is that those two failures need opposite fixes. A third consequence is inert today: the API also bypasses `bind_tcp`, so it never clears `IPV6_V6ONLY` — moot only because SP4 makes an IPv6 API address unbindable in the first place |
| Recommended action | Owner decision; SP4 and SP5 touch the same six lines. Smallest verification: `grep -rn "bind_error" crates/` — four listener bind sites, three calls |

### SP6 — `bind_error` drops the config key on `AddrInUse` and blames a process that does not exist

**Status: RESOLVED 2026-09-15** — see §Second-pass resolutions. Line numbers
below stay anchored to `f32f214`.

| | |
| --- | --- |
| Severity | **Low** — diagnostic; it misdirects on the one listener failure the product causes itself |
| Class | **confirmed defect** (diagnostic) + **coverage gap** |
| Evidence | `fah-common/src/listen.rs:98-108`. The `PermissionDenied` arm (`:100-104`) interpolates `port_setting`; the `AddrInUse` arm (`:105-107`) returns a fixed string and drops it. The doc comment at `:96-97` states the contract it does not keep: "`port_setting` names the config key to change, e.g. `\"[dns.listen] port, or FAH__DNS__LISTEN__PORT\"`". Reproduced with `mode = "dns+http"` and DNS and HTTP both on `127.0.0.1:15399`: `binding TCP 127.0.0.1:15399: … (os error 10048) — another process in this network namespace already holds it`. Neither `[http.listen]` nor `FAH__HTTP__LISTEN__PORT` appears, and the port is held by this same process's DNS listener, bound a moment earlier |
| Affected path | `crates/fah-common/src/listen.rs` |
| Why it matters | A port collision is the listener failure the product can inflict on itself, and it is the single arm where the key is withheld and the blame is placed elsewhere. The message sends the operator hunting a foreign process in a namespace that has none. On the RB5009 the container is distroless with no shell, so there is nothing there to hunt with |
| Coverage gap beside it | `address_in_use_names_the_conflict_not_the_privilege` (`listen.rs:139-144`) asserts only `contains("already holds")`; `the_hint_names_the_callers_own_setting` (`:171-184`), the test that does pin the key, exercises `PermissionDenied`. The suite is green and accurate about what it checks |
| Recommended action | Owner decision. Smallest verification: add `assert!(text.contains("FAH__HTTP__LISTEN__PORT"))` to the `AddrInUse` test and watch it fail |

### SP7 — the `https` collision loop refuses configurations that cannot collide

**Status: CLOSED 2026-09-15** by SP1's rule, not by a change of its own — one
policy now covers all five listeners, and it answers both questions this row
raised. Line numbers below stay anchored to `f32f214`.

| | |
| --- | --- |
| Severity | **Low** — fail-closed, and it is the safe direction; recorded for the inconsistency, not the outcome |
| Class | **potential issue** |
| Evidence | `fah-config/src/lib.rs:158-170` runs unconditionally and compares ports only, never `address`. Two configurations refused on the built binary, both with `config: invalid value for https.listen.port: must differ from [api] port (8443)`: `engine.mode = "dns"` with `[https.listen] port = 8443`, where `TlsServer::bind` is never called at all (`main.rs:435-441`); and `[https.listen] address = "192.168.1.1"` beside `[api] address = "10.0.0.1"`, both on 8443, where the addresses are disjoint and no conflict is possible |
| Affected path | `crates/fah-config/src/lib.rs` §`validate` |
| Why it matters, and why only Low | Refusing early is the policy the comment at `lib.rs:127-130` defends, and it is the right default. It is recorded because the file now holds three different policies for three comparable keys: `dot_port` gated on its own enable flag (`:138`), `https.listen.port` unconditional (`:158`), and the remaining three pairs unchecked in either direction (SP1). Nothing in the code says why they differ, so a reader cannot tell which is the intent and which is the omission |
| Recommended action | None required on its own. If SP1 is closed, the matrix has to settle one policy for all five listeners — and that is the moment to decide whether `address` participates and whether a disabled listener's port is exempt |

### SP8 — DoH has no status surface when `[api] tls = false` silences it

| | |
| --- | --- |
| Severity | **Low** — observability; the gating itself is correct |
| Class | **coverage gap** |
| Evidence | `[dns.listen] doh_enabled = true` with `[api] tls = false` passes validation. `main.rs:629-634` emits a boot `warn!` and `fah-api/src/routes.rs:123-128` never registers `/dns-query`. DoT in the same position gets a machine-readable disposition instead: `DotListener::Closed { reason }` (`fah-api/src/state.rs:19-22`), built for all four cases at `main.rs:612-624` and served on `GET /api/v1/certificates` (`fah-api/src/certs.rs:295`). `grep` for `DotListener` outside `state.rs` returns that one call site; nothing under `crates/fah-api/src` is its DoH equivalent |
| Affected path | `crates/fah-api/src/certs.rs`, `crates/fah-api/src/state.rs` |
| Why it matters | This **refines the second pass's own "probed clean" row** rather than overturning it: the route gating is correct and `main.rs:629`'s warning is accurate. What is missing is the runtime disposition beside DoT's. Once the boot log has scrolled, a DoH client gets a 404 and no endpoint says why. `tls = false` is documented as UNSAFE, so the combination is rare — that is what keeps it Low, not the clarity of the outcome |
| Recommended action | Owner decision: a `doh` field beside `dot` in `CertificatesResponse`, or a line in CONFIGURATION.md saying the boot warning is the only signal there will be |

### Second-pass resolutions — 2026-09-15

Owner approved implementing SP1, SP4, SP5 and SP6 together, and set SP1's policy
before any code was written. SP2, SP3 and SP8 were explicitly excluded and are
untouched. **Line numbers in this section are the post-fix ones**; the finding
rows above keep their `f32f214` anchors so they stay verifiable.

#### The rule SP1 now implements

Decided with the owner before implementation, and it settles SP7's question in
the same stroke: **shape is validated always, relationships only when both ends
are live.**

| Part | What it says |
| --- | --- |
| What is compared | Sockets that actually bind, not config keys. Six sockets from five keys: DNS UDP and DNS TCP on `dns.listen.address:port`, DoT on `dns.listen.address:dot_port`, HTTP, HTTPS, API. DoH is absent — it binds nothing and rides the API listener |
| Which of them collide | Same protocol **and** same port **and** overlapping addresses. DNS UDP is the only UDP socket, so it can collide with nothing; five TCP sockets remain, ten pairs |
| When addresses overlap | Equal; or either is `::`, which covers both families because `bind_tcp` clears `IPV6_V6ONLY` by our own decision; or one is `0.0.0.0` and both are IPv4. A concrete v4 and a concrete v6 never overlap, and two different concrete addresses of one family never overlap |
| The gate | A listener participates only when it would bind: DoT on `dot_enabled`, HTTP and HTTPS on `engine.mode`. DNS and the API always bind |
| What stays unconditional | Address **syntax**. A malformed `[http.listen] address` is wrong in every mode, so `validate_ip` still runs on all four — the comment at `lib.rs:127-130` keeps its meaning |

The gate is safe because the flip is always revalidated: `POST /api/v1/config`
validates the whole merged candidate (`fah-api/src/config_store.rs:129-131`) and
boot revalidates the whole file. A parked collision is therefore refused at the
moment it is enabled, by validation, with the key named — not at bind time.

#### What changed

| Finding | Change | Where |
| --- | --- | --- |
| SP1 | Both comparison loops replaced by one table of five TCP sockets, each with address, port and an `active` flag, compared pairwise | `crates/fah-config/src/lib.rs:512-570` (`validate_listen_sockets`), called from `validate` at `:142-150` |
| SP1 | The overlap relation, written locally because `fah-config` is L1 and may not import `fah-common` | `lib.rs:501-510` (`is_dual_stack`, `addresses_overlap`) |
| SP1 | `validate_ip` returns the parsed `IpAddr` instead of discarding it, so each address is parsed once | `lib.rs:478-485` |
| SP1 | "Would this listener bind?" given one definition | `crates/fah-config/src/schema/engine.rs:40-55` (`EngineMode::serves_http`, `serves_https`) |
| SP4 + SP5 | `format!("{address}:{port}").parse()` replaced by `listen_addr`, and `TcpListener::bind` by `bind_tcp` + `bind_error`. The API now uses the same construction as DNS, HTTP and HTTPS | `crates/fah-api/src/server.rs:62-65`, with `PORT_SETTING` at `:27` |
| SP6 | The `AddrInUse` arm interpolates `port_setting` and no longer asserts a foreign process | `crates/fah-common/src/listen.rs:105-108` |

The `AddrInUse` hint now reads:

```text
 — already in use, by another listener of this process or by another
   process in this network namespace ([dns.listen] port, or FAH__DNS__LISTEN__PORT)
```

#### Two consequences that are not cleanup

**`https.listen.port` is now checked less often than before.** It was the only
listener compared unconditionally; under one policy it is compared only when
HTTPS binds. `the_https_listener_may_not_share_another_listener_port`
(`lib.rs:790-809`) asserted the `https`–`http` collision under the default
`mode = "dns"`, where neither listener binds, so its three cases gained
`mode = "dns+http+https"`. The relaxation is the agreed policy, not an
oversight, and the two new tests below pin the behaviour that replaces it.

**The e2e harness had to be fixed, and it is not a refactor.** `Ports` drew every
port with `bind(:0)` then dropped the socket, with nothing stopping two draws
returning the same number. While only `https`–`http` was compared that duplicate
was rare enough to pass unnoticed; with the matrix complete, a duplicate stops
being an `EADDRINUSE` at bind — which `is_port_conflict`
(`crates/fastadhunter/tests/common/mod.rs:242-262`) recognises and retries — and
becomes a validation refusal whose message matches no needle, failing the gate
outright. It surfaced twice under `--workspace`, on
`bad_upstream_cert_is_not_masked` and then on
`an_idle_spliced_session_is_closed_and_its_permit_returned`, both with
`[http.listen]` and `[https.listen]` on port 60670. Fixed at the source:
`free_tcp_port_excluding` (`common/mod.rs:665-673`) and `Ports::taken`
(`:91-97`) make the harness incapable of generating a config the product rightly
rejects. **The needle list was deliberately left alone** — widening what counts
as retryable would let a genuine wrong refusal hide behind a retry.

#### Tests added

| Where | What it pins |
| --- | --- |
| `fah-config/src/lib.rs:811-849` | `every_active_listener_pair_is_compared_for_a_port_collision` — one case per newly-covered pair, each asserting the blamed key **and** the other endpoint |
| `:851-863` | `a_collision_message_names_both_endpoints_with_address_and_port` |
| `:865-878` | `a_listener_that_does_not_bind_cannot_collide` — a parked `api.port = 8080` beside the HTTP default, HTTPS's port under `dns+http`, and a disabled DoT port equal to `[api]` and to `[dns.listen]` |
| `:880-896` | `enabling_a_listener_is_what_makes_a_parked_collision_fail` — the same config passes in `dns` and is refused in `dns+http`. This is SP1's latent path, closed at the flip |
| `:898-913` | `two_listeners_on_disjoint_addresses_may_share_a_port` — two concrete IPv4s, a v4 beside a v6, and `0.0.0.0` beside a concrete v6 |
| `:915-941` | `a_wildcard_address_overlaps_the_concrete_ones_it_covers` — nine ordered pairs through the real validator |
| `:943-956` | `address_overlap_is_decided_by_family_and_wildcards` — the relation itself, both directions |
| `fah-common/src/listen.rs:146-154` | `address_in_use_keeps_the_config_setting_and_does_not_blame_a_foreign_process`. `address_in_use_names_the_conflict_not_the_privilege` (`:139-144`) kept, its needle updated to the new wording |
| `fah-api/tests/api.rs:5167-5181` | `the_api_binds_an_ipv6_literal_and_serves_on_it` — `::1` binds, the base URL brackets it, and a real request is served |
| `:5183-5204` | `the_unspecified_ipv6_api_address_binds_one_dual_stack_socket` — `::` binds, and an IPv4 peer reaches the same socket |
| `:5206-5226` | `an_api_bind_conflict_names_the_api_port_setting` — a second bind on a held port is `AddrInUse`, names the address and `FAH__API__PORT`, and does not mention `CAP_NET_BIND_SERVICE` |
| `:5228-5245` | `an_unparseable_api_address_names_the_api_section` |

`HarnessOptions` gained `address` and `port`, and `start_with` was split so
`try_start_with` returns `io::Result<Harness>` — the conflict test has to read
the error rather than panic on it.

#### Verified on the rebuilt binary

The same scratch configurations that produced the findings, re-run after the fix.
Scratch files lived outside the repository and were removed.

| Configuration | Before | After |
| --- | --- | --- |
| `[dns.listen] port = 5399` + `[api] port = 5399` | validation passed | ``config: `api.port`: 0.0.0.0:5399 overlaps [dns.listen] port ([::]:5399)`` |
| `[dns.listen] port = 5399` + `[http.listen] port = 5399`, `dns+http` | validation passed | ``config: `http.listen.port`: [::]:5399 overlaps [dns.listen] port ([::]:5399)`` |
| `[api] port = 8080` under `mode = "dns"` | validation passed | still passes — the HTTP listener binds nothing |
| the same, flipped to `dns+http` | validation passed, then the API bind died | ``config: `http.listen.port`: [::]:8080 overlaps [api] port (0.0.0.0:8080)`` |
| `[api] address = "::"` | `failed to start: invalid [api] address: invalid socket address syntax` | `INFO API listening url=https://[::]:18443` |
| DNS and API both on `127.0.0.1:15399`, real start | `Only one usage of each socket address … (os error 10048)` | refused at config load, both endpoints named, before anything binds |

The last two rows are the point of the whole set: the failure moved from a bare
errno after the DNS listeners, the API key, the password and the certificate had
already been created, to a named refusal before any socket is opened.

#### Gates after the fix, Windows dev box

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --all-features --workspace` | **1632 passed, 0 failed**, two consecutive full runs |
| `cargo test -p fah-config --lib` | 97 (90 before) |
| `cargo test -p fah-common --lib` | 44 (43 before) |
| `cargo test -p fah-api --test api` | 137 (133 before) |

#### The duplicate the fix created, and its removal

Adding `EngineMode::serves_http` / `serves_https` briefly left `main.rs` stating
the same fact a second time in `http_enabled` / `https_enabled`. Both matches
were exhaustive, so a fourth mode would have failed to compile in both and the
duplication could not have drifted silently — but it was still two statements of
one fact (principle 4), and the owner asked for it to go.

Both free functions are **deleted**. The two call sites read
`config.engine.mode.serves_http()` (`main.rs:427`) and `.serves_https()`
(`:435`). The p2-01 acceptance tests stay in `main.rs` —
`http_starts_only_in_modes_that_name_it` and
`https_starts_only_in_the_mode_that_names_it` (`:1650-1665`) — asserting the same
thing through the new spelling, so the criterion stays visible to the binary that
has to satisfy it. Net `main.rs`: **+8 −28**.

One thing was lost and is worth stating rather than hiding: the six-line doc
comment above `http_enabled` explained *why* the match is exhaustive — a fourth
mode should fail to compile instead of silently defaulting to "off" and leaving
an operator with a mode that names http and a port nothing listens on. The
exhaustive match moved to `schema/engine.rs:40-55`; the rationale did not,
because CLAUDE.md hard rule 7 forbids an agent writing Rust comments. The
behaviour is self-enforcing without it, but the reasoning now exists only in this
file and in git history.

#### Left open on purpose

**SP4's documentation arm closed through the code**: `CONFIGURATION.md:436`
offered `::` for `[api] address` and the fix makes that true, so no edit was
needed there.

**That was read too widely at the time — "CONFIGURATION.md needed no correction"
was written, and it is false for SP1.** The relaxation the new policy introduces
is exactly what two entries described as unconditional:

| Where | Said | True after `2eb5018` |
| --- | --- | --- |
| `CONFIGURATION.md` `[dns.listen] dot_port` | "must differ from every other listener port" | Only from a listener that actually binds, and only on an overlapping address. Under the shipped `mode = "dns"`, `dot_port` equal to `[http.listen] port` validates clean |
| `CONFIGURATION.md` `[https.listen] port` | "A value equal to `[api]`, `[http.listen]` or `[dns.listen]` port is rejected at load, by name" | Only when HTTPS binds; `dot_port` belongs in that list now; and only on an overlapping address |
| `ARCHITECTURE.md` §HTTPS SNI | "a config setting them equal is rejected at load" | Reads unconditional; true only in `dns+http+https` |

All three corrected 2026-09-15 alongside the R1 fix. The root-document sweep that
found them also confirmed the rest: `API.md:1118` still holds — the new
`["https", "listen", "port"]` arm is a different path from
`https.interception.clients`, which still fails boot as an unknown key — and
`CONTEXT.md:354`, `ROADMAP.md:45`, SECURITY.md, README.md, PERFORMANCE.md,
RULE_ENGINE.md and CONTRIBUTING.md say nothing this batch invalidates.

One statement the fix made **true** rather than stale: `ARCHITECTURE.md:72-73`,
"Every engine binds through `fah_common::listen`", was false while SP5 stood —
the API did not — and is accurate from `2eb5018` on.

### Probed clean in the second pass

Recorded so the ground is not re-covered. No action attaches to any row.

| Question | Verdict |
| --- | --- |
| Do CONFIGURATION.md's documented defaults match `schema/`? | **Yes**, on every key extracted, `[https.sni] no_sni = "pass"` and both `[egress]` keys included |
| Is DoH served when `[api] tls = false`, contradicting the boot `warn!`? | **No.** The route is gated on `state.tls && state.doh.is_some()` (`fah-api/src/routes.rs:123`), so `main.rs:629`'s warning is accurate. The sweep refined this into **SP8** — the gating is right, the runtime disposition beside DoT's is missing |
| Is the minted-leaf cache bounded? | **Yes** — capacity with LRU eviction (`fah-certs/src/leaf.rs:205-240`), pinned at capacity + 88 by `store.rs:1372` |
| Are the leaf-cache figures observable? | **Yes** — `size`, `capacity` and `superseded` reach `/api/v1/certificates` (`fah-api/src/certs.rs:156-168`) |
| Does the DoT accept loop leak a permit on the retry path? | **No.** `slot` is loop-local, so `continue` after an accept error drops it (`dot.rs:69-91`) |
| Are DNS listener deaths missing from `tasks_died`? | **No** — they travel the `fatal` channel (`fah-dns/src/server.rs:160`) and end the run, a different contract from the acceptors that keep answering |

Added by the listener sweep. A negative result here is worth the same as a row
above it: these were the paths most likely to hold a second SP1, and they do not.

| Question | Verdict |
| --- | --- |
| Does every route that can write `fastadhunter.toml` run the same validation? | **Yes, all of them.** `persist_lists` (`fah-api/src/routes.rs:792-802`), the policy handlers (`:1176`) and `post_config` (`:1455`) all go through `ConfigStore::apply_patch`, which validates the merged candidate **before** it persists and before it swaps (`config_store.rs:129-143`). `post_config` additionally refuses `rules.lists`, `policies`, `https.interception` and `auth` outright (`routes.rs:1405-1451`), so the two-writer paths cannot reach it. The API key, the dashboard password and the interception document write their own files, never the TOML |
| Is there any config write that skips validation? | **One, and it is not a route.** `interception_store::load_or_migrate` calls `file.save(config_path)` at `:134` — at boot, only when `carried` (a `[https.interception]` migration), on content `Config::load` has already read and validated moments earlier. No route reaches it |
| Do `FAH__` overrides bypass `validate`? | **No.** `load_inner` applies the environment layer and then validates the result, `fah-config/src/lib.rs:56-58`. Same function, same errors, whichever layer supplied the value |
| Is `dns.udp_max_inflight = 0` an unchecked ceiling beside `tcp_max_connections = 0`, which **is** refused? | **No — it is intentional and pinned.** `0` means "no cap": the default is `0` (`schema/dns/mod.rs:29`), `UdpInflightGauge::new` feeds it to `NonZeroUsize::new` so `None` admits everything (`fah-dns/src/udp.rs:28-40`), and `lib.rs:864-873` asserts the override keeps it. `runtime.http_runtimes = 0` is the same shape — shared runtime (`main.rs:675`). Not an inconsistency with the five `== 0` refusals at `lib.rs:172-204` |
| Is the bind order still what ADR-0004 requires? | **Yes.** DNS (`main.rs:409`) and HTTP (`:428`) bind before the privilege drop at `:480` and serve after it. The API binds at `:639`, after the drop — which is correct for an unprivileged admin port and is also why SP5's second arm exists |
| What is still reachable after a bind fails at startup? | **Nothing.** `Engine::start` propagates with `?`, `run` returns `ExitCode::FAILURE` (`main.rs:264-268`) and the process exits, closing whatever was already bound. The API binds last, so on any earlier failure there is no HTTP surface at all: recovery is a hand edit of `/config/fastadhunter.toml` or a `FAH__` variable, plus a restart. `--healthcheck` does not help — it passed on every SP1 and SP4 reproduction |

## Independent review of the fix commits — 2026-09-15

Read-only pass over `f32f214` (N1) and `2eb5018` (SP1 / SP4 / SP5 / SP6) by a
reviewer who did not write them. Nothing was changed. Scope: plan compliance
against §N1 Resolution and §Second-pass resolutions, correctness, architecture,
performance, memory, Rust quality, tests, regression. **Line numbers below are
anchored to `2eb5018`.** Findings carry an **`R` prefix** — `N` belongs to the
merge audit, `SP` to the second pass.

Test counts in §Second-pass resolutions were re-run and match: `cargo test -p
fah-config --lib` 97, `cargo test -p fah-common --lib` 44.

### R1 — two `PORT_SETTING` strings name environment variables `apply_one` refuses

**Status: RESOLVED 2026-09-15** — see §R1 Resolution. The row below is the state
that was found, in the past tense, and its line numbers stay anchored to
`2eb5018`, the tip before the fix. Severity and title are unchanged: they
describe the defect, not its disposition.

| | |
| --- | --- |
| Severity | **should-fix** — operator-facing; following the hint fails the boot, and the hint is now printed on the one failure the product inflicts on itself |
| Class | **confirmed defect** + **coverage gap** |
| Evidence | `fah-http/src/server.rs:29` is `"[http.listen] port, or FAH__HTTP__LISTEN__PORT"` and `fah-http/src/tls_server.rs:15` is the `https` equivalent. `fah-config/src/env.rs:42-133` has no `["http", …]` and no `["https", …]` arm at all — the `_ =>` arm at `:127-132` returns `UnknownEnvKey`, which `apply_env_overrides` propagates and `load_inner` fails on. Setting either variable stops the process from starting. The other three constants are sound: `FAH__API__PORT` (`env.rs:121`), `FAH__DNS__LISTEN__PORT` (`:57`), `FAH__DNS__LISTEN__DOT_PORT` (`:61`) |
| What `2eb5018` changed about it | The `AddrInUse` arm previously dropped `port_setting` (that was SP6). It now interpolates it (`fah-common/src/listen.rs:105-108`), so the variable is printed on a port collision — the most common listener failure — where before nothing was printed. The `PermissionDenied` arm (`:100-104`) already carried the same wrong hint, silently |
| Affected path | `crates/fah-http/src/server.rs`, `crates/fah-http/src/tls_server.rs`, `crates/fah-config/src/env.rs` |
| Why it matters | Identical in class to N1: a documented `FAH__` name with no arm behind it, fail-closed at boot. N1's own §Recommended action said a test walking documented names against `apply_one` "closes the gap for good" — it closes the CONFIGURATION.md arm only. `every_env_variable_configuration_md_documents_has_an_override_arm` (`fah-config/src/lib.rs:1096`) scans the three `Env:` lines in the document; the five `PORT_SETTING` constants are a second population of advertised names and nothing walks them |
| Mitigating | SP1 now refuses an HTTP/HTTPS port collision at validation, so the `AddrInUse` hint for those two listeners is reached only when a foreign process holds the port. It is still reached, and `PermissionDenied` on `[http.listen] port` below 1024 always was |
| Recommended action | Owner decision, two shapes, mirroring N1: add `["http", "listen", "port"]` and `["https", "listen", "port"]` arms (`coerce_u16`), or drop the `, or FAH__…` suffix from the two constants. Either way, the durable guard is a test that drives all five `PORT_SETTING` strings through `apply_env_overrides` and rejects `UnknownEnvKey` — the same shape as the CONFIGURATION.md test, over the other population |

#### R1 Resolution — 2026-09-15

Owner chose to implement rather than to stop advertising, and set the guard
before any code was written: the variable is the practical knob in a distroless
container, where `/container set envlist=…` and a restart beat reaching into the
`/config` volume. **Line numbers in this subsection are the post-fix ones.**

| What | Where |
| --- | --- |
| The two missing arms, `coerce_u16`, mirroring `["dns", "listen", "port"]` at `env.rs:57` | `crates/fah-config/src/env.rs:104`, `:106` (+4) |
| The five `PORT_SETTING` strings, moved to the one crate that can verify them — `fah-common` owns `bind_error` but is an L1 sibling of `fah-config` and may not import it, so the allowlist is out of its reach | `crates/fah-config/src/port_setting.rs` (new, 5 constants), `pub mod port_setting` at `lib.rs:5` |
| Each listener imports its own, aliased so the call sites and the two existing `fah-dns` argument tests are untouched | `fah-dns/src/server.rs:6`, `fah-http/src/server.rs:17`, `fah-http/src/tls_server.rs:7`, `fah-api/src/server.rs:16` |
| Anti-drift: the variable name is parsed **out of the constant**, not written as a literal beside it, so a renamed constant carries the test with it. Three things fail it — a setting that advertises no variable, a variable with no arm, an arm that writes another field | `every_advertised_port_variable_reaches_the_field_its_setting_names` (`lib.rs:1133`) |
| The two keys documented in the style of the surrounding entries | `CONFIGURATION.md:227`, `:259` |

The text of all five constants is byte-identical to what the four bind sites
printed before the move. `bind_error` still takes `&str` and `fah-common` gained
no dependency. Layering holds: `fah-dns`, `fah-http` and `fah-api` (L3) already
depended on `fah-config` (L1).

**Negative control — two mutations, run and reverted.** The test cannot be run on
the pre-fix tree at all: it references `fah_config::port_setting`, which does not
exist there, so it would fail to compile rather than fail on the defect. Mutation
of the working tree is the control that actually discriminates.

| Mutation | Result |
| --- | --- |
| Both new `apply_one` arms deleted | Fails on the load: ``a bind failure tells the operator to set FAH__HTTP__LISTEN__PORT, but the next config load refuses it, so the process cannot start with it set: environment variable FAH__HTTP__LISTEN__PORT does not map to a known config key (`http.listen.port`)`` — the R1 defect, reproduced |
| `["http", "listen", "port"]` made to write `config.https.listen.port` | Fails on the value: ``FAH__HTTP__LISTEN__PORT did not reach the field `[http.listen] port, or FAH__HTTP__LISTEN__PORT` names — left: 8080, right: 9999``. `8080` is `http.listen.port`'s untouched default, so the field assertion discriminates and not just the arm's existence |

`9999` is not the default of any of the five ports (53, 853, 8080, 8444, 8443),
which is what makes the second mutation visible.

| Gate after the fix, Windows dev box | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --all-features --workspace` | **1633 passed, 0 failed** (1632 before) |
| `cargo test -p fah-config --lib` | 98 (97 before) |

Clippy required one deviation from the shape agreed beforehand: `[(&str, fn(&Config) -> u16); 5]`
is refused as `very complex type used`, so the test carries a local
`type PortField = fn(&Config) -> u16;`.

`every_env_variable_configuration_md_documents_has_an_override_arm` now walks
five names instead of three; its `>= 3` floor (R5) is unchanged and still passes.

Scope held: no HTTP/HTTPS **address** overrides, no change to the general `FAH__`
contract at `CONFIGURATION.md:11-13`, SP2/SP3/SP8 and R2–R7 untouched.

### R2–R7 — notes

| # | Where | Finding | Direction |
| --- | --- | --- | --- |
| R2 | `fah-config/src/lib.rs:811-849` | `every_active_listener_pair_is_compared_for_a_port_collision` drives **5 of the 10 pairs** — `api–dns`, `http–dns`, `http–api`, `https–http`, `dot–https`. `dot–api` is covered at `:753` and the three `https` pairs at `:790`, leaving **`dot–dns` and `dot–http` asserted by nothing**. SP1's §Recommended action asked for all ten. Real risk is low: `validate_listen_sockets` is one uniform double loop, and every socket appears in at least one covered case, so deleting a row from the table still fails the suite | **RESOLVED 2026-09-17** — both cases added; all ten pairs are asserted: seven here, `dot–api` at `:754`, the three `https` pairs at `:790` |
| R3 | `lib.rs:505-510` | `addresses_overlap` decides family with `is_ipv4()`, so an IPv4-mapped literal misses: `::ffff:10.0.0.1` beside `10.0.0.1` on one port returns `false` and validates clean. Fail-open, and the bind failure now reports through `bind_error`, so the outcome is a named error rather than a bare errno | Canonicalize v4-mapped v6 before comparing, if the shape is ever worth the line |
| R4 | `lib.rs:551-562` | The blamed key is the later entry in the table, not the edited one. A `dns–api` collision reports `api.port` even when the operator changed `[dns.listen] port`. The message names both endpoints with address and port, so it stays navigable | None required; recorded so the asymmetry is not read as a bug |
| R5 | `lib.rs:1113-1118` | The anti-drift floor is `documented.len() >= 3`, exactly today's count. A reformat that leaves 3 of 5 `Env:` names matchable passes silently. The test also reads `../../CONFIGURATION.md` from a lib unit test, so `cargo test -p fah-config` depends on a file outside the crate | Assert an exact count, or keep the floor and accept the window |
| R6 | `fah-api/tests/api.rs:5183-5204` | `the_unspecified_ipv6_api_address_binds_one_dual_stack_socket` binds `[::]:0` — every interface — for the duration of the run, and needs IPv6 present on the host. `the_api_binds_an_ipv6_literal_and_serves_on_it` (`:5167`) already proves `listen_addr` handles the bracket trap on `::1` | Keep if the dual-stack assertion is wanted; note the host dependency |
| R7 | `lib.rs:512` | `validate_listen_sockets(config: &Config, addresses: ListenAddresses)` receives both the whole config and the addresses derived from it — two sources for one field. Minor: `free_tcp_port_excluding` (`fastadhunter/tests/common/mod.rs:665`) reuses `DNS_PORT_DRAWS` (`:33`) for TCP draws | Cosmetic; fold into the next touch |

### Checked and found acceptable

| Category | Verdict |
| --- | --- |
| Plan compliance | Every unit in §What changed exists at the claimed anchor: `validate_listen_sockets` called from `validate` at `lib.rs:142-150`, the overlap relation at `:501-510`, `validate_ip` returning its parse at `:478`, `EngineMode::serves_http`/`serves_https` at `schema/engine.rs:40-55`, `PORT_SETTING` at `fah-api/src/server.rs:27`, the `AddrInUse` arm at `fah-common/src/listen.rs:105-108`. SP2, SP3 and SP8 are untouched, as instructed |
| Correctness | The agreed rule is implemented as written. `addresses_overlap`'s truth table matches the `IPV6_V6ONLY` policy `fah-common/src/listen.rs:7-13` commits to: `::` covers both families, `0.0.0.0` covers IPv4 only, a concrete v4 and a concrete v6 never overlap. DoT is compared on `dns.listen.address`, which is where `fah-dns/src/server.rs:72` binds it. `port = 0` cannot produce a collision storm — `validate_nonzero_port` refuses it for the four always-on keys first, and a disabled DoT socket is skipped |
| Socket inventory | The five-entry table matches the production bind sites exactly. `grep` for `bind_tcp(`/`bind_udp(`/`TcpListener::bind` outside tests returns DNS UDP, DNS TCP, DoT, HTTP, HTTPS, API and nothing else; `fah-api/src/tls.rs:6` is the SAN probe socket, not a listener |
| Architecture | `fah-api` (L3) → `fah-common` (L1) is downward and the dependency was already declared. `EngineMode::serves_*` gives "would this listener bind" one home, used by both `validate` and `main.rs:427`/`:435`; `http_enabled`/`https_enabled` are deleted rather than left beside it. Both matches stay exhaustive, so a fourth mode still fails to compile |
| Performance | No hot-path contact. `validate` runs at boot and on `POST /api/v1/config`. The socket table is a five-element array on the stack, `format!` runs only on the error return, and `validate_ip` returning `IpAddr` removes a second parse of each address |
| Memory | No retained state, no new allocation outside error strings, nothing that grows with traffic or uptime |
| Rust quality | No `unwrap`/`expect` added on a production path. `bind_tcp`'s socket2 branch is synchronous, so the API bind has no new cancellation hazard. `ListenSocket` holds `SocketAddr` and `&'static str` — no clones |
| Regression | No configuration that previously worked is newly refused: address-aware comparison is strictly more permissive for the `https` and `dot` pairs, and the three newly-compared pairs failed at bind anyway. `is_port_conflict`'s needles (`fastadhunter/tests/common/mod.rs:236-251`) do not match the new validation wording — `"two TCP listeners cannot bind one socket"` contains no `"socket address"` — so a genuine refusal still fails the gate instead of being retried |
| SAN path, newly reachable | SP4 makes an IPv6 `[api] address` bindable for the first time, so `fah-certs/src/api.rs:72-91` receives one. It parses to `IpAddr`, filters unspecified, and the baseline already carries `::1` (`:18-21`) — the new state is handled |
| DNS / HTTP data path | No defect found. Neither commit touches it |

**Verdict of this pass: PASS with one should-fix (R1) and six notes.** R1 is not a
regression of the fix's own logic — it is the N1 class surviving in a second
population of advertised `FAH__` names, which `2eb5018` made visible on the
`AddrInUse` path. **R1 is RESOLVED**, same day, on the owner's approval and to a
shape agreed before any code was written; R2 closed 2026-09-17, R3–R7 stay open
as notes.

## Established correct

Recorded so a later reader does not re-derive them. No action attaches to any row.

| # | Question | Verdict |
| --- | --- | --- |
| S1 | Did the `ConnectionGauge` / `OpenConnection` move to `fah-common` (L1) keep permit and gauge release on every path? | **Yes.** `OpenConnection::enter` is taken after `accept` and before the spawn in all three acceptors — `fah-dns/src/tcp.rs:111`, `fah-dns/src/dot.rs:99`, `fah-http/src/server.rs:200` — with no early return in between. `Accepted::register` (`fah-http/src/domain.rs:50`) and `::detach` (`:73`) destructure and drop both on failure; `Rotation::send` (`fah-http/src/server.rs:277-294`) drops the handoff when every domain is gone; `Accepted::serve` (`domain.rs:29-45`) holds both across the served future, so an abort releases them too. Layering holds: L3 `fah-dns` and `fah-http` depend downward on L1 |
| S1 | Is one gauge shared between the HTTP and HTTPS lanes? | **No.** `TlsServer` builds its own semaphore and gauge (`fah-http/src/tls_server.rs:37-38`); `fah-dns` wraps the same type in separate `TcpConnectionGauge` / `DotConnectionGauge` instances (`fah-dns/src/server.rs:96-97`) |
| S2 | `absolute_url` gained a `host` + `Option<port>` signature at the merge. Does the HTTPS lane now build URLs a rule cannot match? | **No.** `judge` sets the port only when the claim differs from the listener's origin port (`fah-http/src/proxy.rs:577`), and the origin ports are the real ones — `HTTPS_ORIGIN_PORT` at `main.rs:1111`, `HTTP_ORIGIN_PORT` at `:1137`. A claim on 443 renders `https://host/path` |
| S3 | Do the DNS gauges added at the merge have a real writer, and does the value reach the API? | **Yes.** `main.rs:1232-1234` pushes `snapshot()` for TCP, DoT and UDP every `TELEMETRY_POLL`; `fah-metrics/src/registry.rs:325` serializes them; `fah-api/tests/api.rs:1153-1162` pins the shape |
| S3 | Is any `peak` read by two consumers, so one steals the other's value? | **No.** `take_peak()` has exactly one production reader, the perf sampler (`main.rs:1439`, `:1442`), and it is the resetting per-interval figure API.md:605-610 describes. `TcpConnectionGauge::snapshot` uses the non-resetting `peak()` (`fah-dns/src/tcp.rs:35`), the process-lifetime mark API.md:305-308 describes. Two `peak` fields, two documented meanings, no shared reader |
| S3 | Is `concurrent_connections` structurally 0 under the shipped default, and is that stated? | **Yes to both.** `default_mode()` is `EngineMode::Dns` (`fah-config/src/schema/engine.rs:21-23`) and `https_enabled` needs `DnsHttpHttps` (`main.rs:933`), so neither proxy listener binds by default. API.md:606-609 says exactly that |
| S3 | Does the refusal split double-count between `engine.http.refused_*` and `listeners.*.refused_*`? | **No**, and API.md:197-199 states the relationship. The poll sums per-listener snapshots and `set`s rather than accumulates (`main.rs:1236-1244`), so the tick is idempotent |
| S3 | Do CONFIGURATION.md and API.md describe the two ceilings as they behave? | **Yes.** CONFIGURATION.md:99-104 calls `udp_max_inflight = 0` "no cap … active and peak stay 0"; API.md:325-328 repeats it. That is F8's consequence written down before this audit, not drift |
| S4 | Does `the_configured_tcp_and_udp_ceilings_reach_the_listeners` discriminate? | **Yes.** It sets `udp_max_inflight: 1` and `tcp_max_connections: 1` and proves behaviour, not parsing: one shed datagram never answered, one TCP connection held until the first closes, both gauges asserted (`fah-dns/tests/server_integration.rs:517-617`) |
| S4 | Does `both_refusal_causes_reach_telemetry_on_their_own_fields` discriminate? | **Yes.** It boots the binary and asserts `(refused_claim, refused_destination) == (1, 1)` — "neither summed nor swapped" — beside `block == 0` and `response_bytes == 0` (`fastadhunter/tests/http_e2e.rs`) |

## Measurements

The audit pass itself ran no gate and no bench; the tip's gate results are in
[project-state.md](../../project-state.md) §Now.

Gates after the N1 fix, 2026-09-15, Windows dev box. These are not the final
numbers for this file — the SP1 / SP4 / SP5 / SP6 fix landed later the same day
and §Second-pass resolutions carries its own gate table:

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --message-format=short -- -D warnings` | clean |
| `cargo test --all-features --workspace` | **0 failed** across every target |
| `cargo test -p fah-config` | 90 passed (87 before the fix) |
| `cargo test -p fastadhunter --test healthcheck` | 6 passed (5 before) |

| Surface | Count |
| --- | --- |
| `eb693e2` files differing from **both** parents | 36 |
| of those, under `crates/` | 21 |
| already named by plan-merge §Step 2 / §Step 4 or by F1–F9 | 12 |
| read in this pass | 9, plus the 6 gauge call sites S1 needed |

## Files changed

No audit pass changed anything: not S1–S4, not the second pass, not the listener
sweep. Every line below is a fix, and all of it ships in the same commit as this
file.

The N1 fix:

| File | Change |
| --- | --- |
| `crates/fah-config/src/env.rs` | +7 — the two arms |
| `crates/fah-config/src/lib.rs` | +81 — three tests |
| `crates/fastadhunter/tests/healthcheck.rs` | +30 — one process-level test |

The SP1 / SP4 / SP5 / SP6 fix:

| File | Change |
| --- | --- |
| `crates/fah-config/src/lib.rs` | +253 −40 — the socket table, the overlap relation, `validate_ip` returning its parse, seven tests |
| `crates/fah-config/src/schema/engine.rs` | +16 — `serves_http`, `serves_https` |
| `crates/fah-api/src/server.rs` | +7 −7 — `listen_addr` + `bind_tcp` + `bind_error`, and `PORT_SETTING` |
| `crates/fah-api/tests/api.rs` | +93 −9 — four bind tests, `HarnessOptions::{address, port}`, `try_start_with` |
| `crates/fah-common/src/listen.rs` | +15 −4 — the `AddrInUse` arm and one test |
| `crates/fastadhunter/tests/common/mod.rs` | +29 −8 — `free_tcp_port_excluding`, `Ports::taken` |
| `crates/fastadhunter/src/main.rs` | +8 −28 — `http_enabled` / `https_enabled` deleted, both call sites and both p2-01 tests on the `EngineMode` methods |

No documentation corrected: CONFIGURATION.md was already right for
N1, and the SP4 fix makes `:436` right rather than needing an edit. The sweep's
scratch TOML files, and the reproduction files re-run after the fix, were written
outside the repository and removed.

The R1 fix:

| File | Change |
| --- | --- |
| `crates/fah-config/src/port_setting.rs` | new, +9 — the five constants |
| `crates/fah-config/src/lib.rs` | +36 — `pub mod port_setting`, one test |
| `crates/fah-config/src/env.rs` | +4 — the two arms |
| `crates/fah-dns/src/server.rs` | +1 −5 — two local constants replaced by one import |
| `crates/fah-http/src/server.rs` | +1 −3 — the same |
| `crates/fah-http/src/tls_server.rs` | +1 −2 — the same |
| `crates/fah-api/src/server.rs` | +1 −2 — the same |
| `CONFIGURATION.md` | +11 −5 — one `Env:` line under each of the two port entries, and the `dot_port` and `[https.listen] port` wording corrected per §Left open on purpose |
| `ARCHITECTURE.md` | +3 −2 — §HTTPS SNI, the same correction |

The review pass that found R1 changed no code. Its only edit is this file —
§Independent review of the fix commits, the summary bullet that points at it, the
`R` rows in §Remaining TODOs and the closing verdict. It ran
`cargo test -p fah-config --lib` and `cargo test -p fah-common --lib` to confirm
the 97 / 44 counts §Second-pass resolutions claims, and nothing else.

The 2026-09-17 fixes — N2, SP2 and R2, on the owner's approval:

| File | Change |
| --- | --- |
| `crates/fah-config/src/lib.rs` | +10 — the `dot–dns` and `dot–http` rows in the pair-matrix test |
| `crates/fah-dns/src/dot.rs` | +45 −2 — the pre-warm await inside `timeout_at`, and the stalled-mint test |
| `dashboard/frontend/src/api/types.ts` | +15 — `DnsConnectionsGauge`, `DnsUdpInflight`, three `Counters` fields |
| `dashboard/frontend/src/pages/health/backpressure-card.tsx` | +22 — four rows, one footnote line, `activeAndPeak` |
| `dashboard/frontend/src/pages/diagnostics-health.test.tsx` | +12 — the three gauges in the fixture, one test |

Gates for that batch, Windows dev box: `cargo fmt --all -- --check` clean;
`cargo clippy --workspace --all-targets -- -D warnings` clean with and without
`--all-features`; `cargo test --all-features --workspace` **1657 passed, 0
failed** (1656 before); `tsc --noEmit` clean; `vitest run` 625 passed. Both new
tests were run red before their fix landed.

## Remaining TODOs

- N2: **closed** 2026-09-17 — the three gauges are on the Health page's
  Backpressure card. See §N2.
- N3: nothing owed; fold into the next touch of `strategy_ab.rs`, if ever.
- SP1, SP4, SP5, SP6: **closed** 2026-09-15, nothing further owed. See
  §Second-pass resolutions.
- SP2: **closed** 2026-09-17 — the pre-warm sits inside the handshake deadline.
  See §SP2.
- SP3: nothing owed.
- SP7: **closed by SP1's rule** rather than by its own change. One policy now
  covers all five listeners, and it answers both open questions — `address`
  participates, and a listener that does not bind is exempt.
- SP8: owner decision, still open — a `doh` field beside `dot` in
  `CertificatesResponse`, or a documented line saying the boot warning is all
  there is. Excluded from the fix pass by instruction.
- R1: **closed** 2026-09-15, nothing further owed. See §R1 Resolution.
- R2: **closed** 2026-09-17 — both cases added; every pair is asserted.
- R3–R7: nothing owed; fold into the next touch of the files named.
- The follow-up the fix created is **closed** the same day: `main.rs`'s
  `http_enabled` / `https_enabled` are deleted and the two call sites use
  `EngineMode::serves_http` / `serves_https` directly. One statement of the fact,
  not two. The rationale comment that sat above them could not move with the
  match — see §Second-pass resolutions §The duplicate the fix created.

N1, SP1, SP4, SP5, SP6, R1, N2, SP2 and R2 are closed and owe nothing further.
N1's fix is `f32f214`; the SP fixes are `2eb5018`, the commit that also carried
the first writing of this file; R1's fix is `b67ecec`. Where the remotes stand
is a `git ls-remote` question, not a sentence in this file — an earlier version
named a commit here after it had stopped being true.

**PASS WITH DEFERRED FINDINGS** — five confirmed defects, **all five resolved
the same day** (N1, Medium; SP1 and SP4, Medium; SP5 and SP6, Low), and the
independent review's should-fix (R1) with them. N2, SP2 and R2 closed
2026-09-17. Still open: one coverage gap (SP8, Low), five notes (R3–R7), two
intentional deviations (N3 and SP3, Info). SP7 closed through SP1's rule rather
than a change of its own. **No defect was found in the DNS or HTTP data path**,
by any of the four passes, and nothing here blocks further Phase 3 work.

The independent review adds the one lesson the fix set did not draw for itself:
closing a documentation-to-code drift for the names a *document* advertises does
not close it for the names the *code* advertises. `PORT_SETTING` is the second
population, it was never walked, and two of its five entries are wrong (R1).

What the defect set said as a group, and what the fix answered: the
configuration layer validated types and ranges thoroughly and relationships only
partly, and the gap was not random. Three of six listener port pairs were
compared, one bind site out of four reported its failures properly, and one arm
of `bind_error` out of two kept the contract its own doc comment stated. A
configuration that passed `validate` and then could not start was reachable
three separate ways — SP1, SP4, and `[api] port` below 1024 — and in each the
API, the surface that would undo the value, was the component that failed to
come up. All three now fail at validation with the key named, before a socket is
opened; the third still fails at bind, but through `bind_error`, so it says
`CAP_NET_BIND_SERVICE` instead of `Permission denied`.
