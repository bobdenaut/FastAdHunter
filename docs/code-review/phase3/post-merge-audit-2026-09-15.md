# Post-merge audit — Phase 3 integration, `bc49e4e..78238b4`

Read-only. Nothing in the tree was changed. Scope agreed before the pass:
S1 the `ConnectionGauge` move, S2 the `eb693e2` hunks no earlier review names,
S3 instrumentation validity, S4 merge-window tests that could pass with the
wiring they are meant to prove broken.

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

Gates after the N1 fix, 2026-09-15, Windows dev box:

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

The audit pass itself changed nothing. The N1 fix ships in the same commit as
this file:

| File | Change |
| --- | --- |
| `crates/fah-config/src/env.rs` | +7 — the two arms |
| `crates/fah-config/src/lib.rs` | +81 — three tests |
| `crates/fastadhunter/tests/healthcheck.rs` | +30 — one process-level test |

No deletions, no production code outside `env.rs`, no documentation corrected —
CONFIGURATION.md was already right.

## Remaining TODOs

- N2: owner decision — a Health row for the three DNS gauges, or a line saying
  they are telemetry-only.
- N3: nothing owed; fold into the next touch of `strategy_ab.rs`, if ever.

N1 is closed and owes nothing further; its fix is in the same commit as this
file. Nothing here is pushed — `origin/main` and `backup/main` stay at `8fec41d`
until a push gets its own go.

**PASS WITH DEFERRED FINDINGS** — one confirmed defect (N1, Medium, config
surface only) **resolved the same day**, one coverage gap (N2, Low), one
intentional deviation (N3, Info). No defect found in the DNS or HTTP data path,
and no blocker to further Phase 3 work.
