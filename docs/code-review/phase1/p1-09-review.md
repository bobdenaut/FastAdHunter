# p1-09 — REST + WebSocket API: RC review

Release-candidate review before Phase 2. Reviewed as a Rust reviewer would for
Servo/Tokio: ownership, lifetimes, API design, allocations, duplication,
maintainability. Scope: `crates/fah-api` (13 modules + integration suite),
its wiring in `crates/fastadhunter` (`main.rs`, `adapters.rs`), and the
API.md contract. Formatting/naming ignored unless it affects correctness,
performance, maintainability, or API quality.

## What was built / shipped

The full API.md surface (~2,900 lines): every documented endpoint, bearer-key
auth, HTTPS by default, live WS events.

- **Layering held without doc changes.** `fah-api` never imports its L3
  siblings: it declares [`ports.rs`](../../../crates/fah-api/src/ports.rs)
  traits (`StatsSource`, `TelemetrySource`) and the binary implements them in
  `adapters.rs`. Only `fah-rules` (L2) and `fah-config`/`fah-model` (L1) are
  held directly. The integration suite runs against fakes of those ports —
  real rustls, real axum, real `ListManager`, real config store.
- **Auth** (`auth.rs`): `Authorization: Bearer` on everything under
  `/api/v1/`, `/health` + `/metrics` exempt per runtime-mutable
  `metrics_public`, constant-time key comparison, `?token=` accepted only on
  the events route so the key never lands in REST URLs/proxy logs.
- **Keys** (`keys.rs`): 32-byte hex key generated on first boot, printed
  once, stored 0600 in `/config`, rotation swaps via `ArcSwap` and
  invalidates the old key immediately.
- **TLS** (`tls.rs`): rcgen self-signed pair generated into `/config`,
  stable across restarts, user-supplied PEM honored untouched, h2 + http/1.1
  ALPN.
- **Server** (`server.rs`): hand-rolled accept loop over
  `hyper_util::conn::auto` — needed for `serve_connection_with_upgrades`
  (WS) and the per-stream `TlsAcceptor`.
- **Config** (`config_store.rs`): deep-merge → `deny_unknown_fields`
  validation → atomic temp-then-rename write-back → boot/runtime
  classification via diffed dotted paths. A rejected patch touches nothing.
- **Lists CRUD** (`routes.rs`): persists the new list set to
  `fastadhunter.toml` *before* mutating the `ListManager`, serialized by one
  admin-plane mutex; duplicate id and duplicate source both 409 with
  actionable messages; readable id derivation from URL/path.
- **User rules**: the block is parsed as one unit (per-line validation would
  misread `/ads/banner.gif`), error line numbers capped, 422 with per-line
  messages, atomic swap on success.
- **Events** (`events.rs`): one `tokio::broadcast(256)` hub; lagging sockets
  are disconnected (`RecvError::Lagged`) instead of back-pressuring; 2s
  periodic stats push shares the socket loop.
- **Wire boundary** (`wire.rs`, `timestamp.rs`): API.md shapes pinned by
  golden tests, RFC 3339 in one place, wire-form trailing dot stripped at
  presentation only.
- **Tests**: 45 unit + 28 integration over real HTTPS — auth failure modes,
  TLS on/off, list persistence across the file boundary, WS end-to-end with
  `?token=`, config write-back reflected in the TOML.

## Findings

### Critical

None.

### Major

#### M1 — API listener is unbounded and the TLS handshake has no deadline

[server.rs:88-109](../../../crates/fah-api/src/server.rs#L88-L109) spawns a task
per accepted connection with no cap, and
[server.rs:127](../../../crates/fah-api/src/server.rs#L127) awaits
`acceptor.accept(stream)` with no timeout.

- **Rationale:** hard rule 4 — *bounded everything; memory must not grow with
  traffic*. Every other surface honors it (event channel drops, broadcast
  lags, cache evicts). This is the one place where a misbehaving LAN client
  grows memory/fds without limit: open TCP connections that never handshake
  (each pins a task + socket forever), or simply many concurrent connections.
- **Impact if unchanged:** a buggy dashboard, a port scanner, or a slowloris
  on the admin port can exhaust fds/memory on the 1 GB RB5009 — taking down
  DNS with it, since it is the same process. Not reachable from the WAN, but
  "LAN-only" includes every phone and IoT device in the house.
- **Recommendation: fix before Phase 2.** A connection cap (semaphore; deny
  by dropping when full) plus a handshake deadline (a few seconds) is ~20
  lines and closes the class.

### Minor

#### m2 — a silently-dead WS peer parks the socket task for the OS timeout

[events.rs:149](../../../crates/fah-api/src/events.rs#L149): `socket.send(...)`
has no deadline. A peer that vanishes without FIN/RST (phone leaves Wi-Fi —
the normal dashboard client) leaves the task parked in `send` once the TCP
buffer fills. The lag-disconnect never fires because the task is stuck in
`send`, not `recv`; the OS gives up only after its retransmission timeout
(minutes to hours). Each occurrence pins a task, a socket, and a broadcast
receiver slot. **Fix before Phase 2** — wrap the send in a
`tokio::time::timeout`; the 2s stats cadence guarantees traffic to trip it.

#### m3 — list `id` and `path` reach the filesystem unvalidated

`POST /api/v1/lists` accepts any `id`/`path` string;
[cache.rs:9](../../../crates/fah-rules/src/lifecycle/cache.rs#L9) then builds
`/data/lists/{id}.raw` and
[source.rs:25](../../../crates/fah-rules/src/lifecycle/source.rs#L25) does
`data_dir.join(path)` — where `..` segments escape `/data` and an absolute
`path` replaces it entirely. The caller is the authenticated admin (who
already holds config write), so this is hardening, not an open door — but an
id like `../../config/apikey` writing a `.raw` next to the API key is a class
of bug worth closing at the boundary, and a charset guard also keeps ids
readable in TOML, metrics labels and file listings. **Fix before Phase 2:**
constrain `id` to `[a-z0-9._-]` (no leading dot), keep `path` inside the
data dir after join.

#### m4 — a failed engine mutation after a successful persist leaves the file and the engine disagreeing

The list mutations persist first, then mutate the `ListManager`
([routes.rs:301-308](../../../crates/fah-api/src/routes.rs#L301-L308), patch and
delete likewise). Persist-first is the right order — the durable record must
be the thing that can fail — but if the engine mutation then errors, the TOML
now describes a state the running engine doesn't have, violating API.md's
"the file and the running engine never disagree". Today that failure is
nearly unreachable (both sides read the same source of truth under one
mutex), which is why this is Minor. **Fix before Phase 2 (cheap):** on engine
failure, best-effort re-persist the previous set before returning the 500.

#### m5 — per-query WS publish work is paid even with zero subscribers

[main.rs:329-339](../../../crates/fastadhunter/src/main.rs#L329-L339): the
fan-out resolves `client_name` (a lock + `String` clone in `fah-stats`) and
boxes a `QueryRecord` for every query, then
[events.rs:67](../../../crates/fah-api/src/events.rs#L67) throws it away when no
dashboard is connected — which is the state the appliance idles in ~24h/day.
Not the DNS hot path (the channel decouples it), but it is per-query work
with a per-query allocation, bought for nothing.
**Fix before Phase 2:** expose `EventHub::has_subscribers()`
(`Sender::receiver_count() > 0`) and skip both the name lookup and the
publish when false.

### Nitpicks

#### n1 — exempt-path match is exact

[auth.rs:24](../../../crates/fah-api/src/auth.rs#L24): `GET /health/` (trailing
slash) is 401, not 404. Fails closed, so harmless — noted only so nobody
"fixes" it with prefix matching, which would be the dangerous direction.

#### n2 — API.md error-code drift

API.md's error example shows `"code": "invalid_rule_syntax"` and lists
`restart_required` among common codes; the implementation emits neither
(`validation_failed` covers the former; `restart_required` is a response
*field*, not an error). Docs are the contract — align API.md with the real
code set (`unauthorized`, `not_found`, `validation_failed`, `bad_request`,
`conflict`, `internal`). **Fix with the next API.md edit.**

#### n3 — `serve_connection`'s match arms are identical calls

[server.rs:125-137](../../../crates/fah-api/src/server.rs#L125-L137): both arms
call the same builder method on a differently-typed stream. Generics/dyn
would cost more than the duplication saves. Leave.

#### n4 — `qtype_name` allocates for the two constant spellings

[wire.rs:170-176](../../../crates/fah-api/src/wire.rs#L170-L176): `"A"`/`"AAAA"`
become `String`s per row. Admin plane, bounded by `limit=1000`. Leave.

## What's deliberately fine

- **The triple DTO copy** (`fah_stats::Snapshot` → `ports::StatsOverview` →
  `wire::StatsResponse`) looks like duplication but is the documented
  layering tax: each mapping is field-for-field, each side can evolve alone,
  and the golden tests pin the outer one. Collapsing them would re-couple the
  siblings the ports exist to separate.
- **Hand-rolled accept loop** — justified: `axum::serve` cannot do
  per-stream TLS accept + WS upgrades in this shape.
- **Hand-rolled `percent_decode`** — 30 lines, tested, only has to survive a
  hex token; a `percent-encoding` dependency would be more code in the tree.
- **Constant-time key compare** — comparison, not crypto; correctly outside
  SECURITY.md's "no hand-rolled crypto".
- **Config write-back atomicity** — verified: temp-file-then-rename in
  `fah-config` (`write_atomic`), same pattern in the list content cache.
- **`user_rules().await.unwrap_or_default()`** — `user_rules` returns
  `Option<String>` ("never set"), not a swallowed `Result`. Correct.
- **Per-connection `Router` clone** — axum routers are `Arc`-backed; this is
  the intended pattern.

## Recommended before closing Phase 1

Fix M1, m2, m3, m4, m5 (all small and local); align API.md's error-code list
(n2). n1/n3/n4 stay as recorded intent.

## Resolution (2026-07-21)

All recommended findings fixed in the same session; n1/n3/n4 deliberately
unchanged as recorded above.

- **M1 fixed** — `server.rs`: a 64-permit semaphore taken *before* `accept`
  (at the ceiling the listener pauses and clients queue in the bounded
  kernel backlog; no accept-then-drop), plus a 10 s TLS-handshake deadline.
  The listener is no longer the one surface where memory could grow with
  traffic.
- **m2 fixed** — `events.rs`: every WS send runs under a 15 s
  `tokio::time::timeout`; a silently-vanished peer is dropped at the next
  stats push instead of parking the task on a full TCP buffer.
- **m3 fixed** — `routes.rs`: `validate_list_id` locks ids to
  `[a-z0-9._-]` with no leading dot (unit + integration tested, including
  `../../config/apikey`); `path` sources reject `..` components;
  `derive_id` now lowercases so derived ids always pass.
- **m4 fixed** — create/patch/delete all best-effort re-persist the previous
  list set when the engine mutation fails after a successful write, so the
  TOML keeps describing the running engine.
- **m5 fixed** — `EventHub::has_subscribers()` added; the binary's fan-out
  skips the per-query clone, boxing and client-name lookup when no dashboard
  is connected.
- **n2 fixed** — API.md's error section now shows the real code set
  (`unauthorized`, `bad_request`, `not_found`, `conflict`,
  `validation_failed`, `internal`).

**Also shipped in this session** (requested alongside the review): the cache
admin surface — `GET /api/v1/cache` (entries by fresh/stale/expired stage,
hit/miss/eviction counters, `load_percent`), `POST /api/v1/cache/clean`
(removes expired; `?stale=true` purges the RFC 8767 window as an explicit
choice), and `GET /api/v1/debug/memory` (ruleset bytes, cache estimate,
process RSS from procfs). Layering preserved via a new `CacheSource` port
implemented by the binary over `fah_dns::Pipeline::cache_stats/cache_clean`;
the cache itself gained per-entry stage classification, lifetime
hit/miss/eviction counters and a documented coarse byte estimate. After
review pushback the estimate accounts for the full footprint, not just
entries: hash-table slabs at bucket granularity (occupied or not), key
strings, the `Arc` refcount header on each answer, record buffers, a flat
per-record allowance, and 16-byte allocator rounding — `freed_bytes`
deliberately excludes the slab, which a clean never returns to the
allocator. API.md,
CONTEXT.md (§Cache stages, §Port) and `requests/cache.http` updated in the
same change.

**Verification.** `cargo fmt --check` clean; `cargo clippy --workspace
--all-targets -- -D warnings` clean; **368 workspace tests, 0 failures**
(18 new). The touched cache-hit hot path re-benched: criterion first
reported "+103% regressed", and an A/B against the stashed pre-change code
showed the *unchanged* code at the same 3.81 µs (p = 0.12, no difference) —
the stored baseline is stale on this unpinned box (same drift episode as the
p1-08 review); the real cost of the added relaxed atomic increment is not
measurable.
