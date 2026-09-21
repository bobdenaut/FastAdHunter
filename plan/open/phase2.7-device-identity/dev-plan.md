# ADR-0010 — implementation plan

Translates [ADR-0010](../../../docs/decisions/0010-device-identity-from-routeros-rest.md)
into steps. Written 2026-09-21 against `6350fc1`; line anchors drift, re-check
each before editing. Nothing implemented, router untouched.

## Ground rules for every step

| Rule | Consequence |
| --- | --- |
| Workspace green after each step | `cargo fmt --check`, `clippy -D warnings`, `cargo test --all-features --workspace`, dashboard `tsc` + `vitest` — before asking for the step's commit |
| Deployed behaviour unchanged until step 6 | `[routeros] url = ""` is the default; with no source built, the registry never learns a MAC and every path below behaves as at `6350fc1`. A test per step pins that |
| Hot path untouched | `Stats::record`, `ActivePolicies::policy_for`, `Matcher::context_for` keep their cost; the only per-query addition is one `HashSet::insert` on the **first sight of a new address**, inside the insert that already allocates. `cargo bench -p fah-stats record` A/B against `6350fc1`, per docs/measurement-traps.md |
| No comments in code | `.claude/hooks/no-rust-comments.sh`; the why goes in the review file or the docs step |
| One review file per task | `docs/code-review/phase2.7/p2.7-NN-<slug>-review.md`, the one `.md` an agent may create unasked |
| Commit per step, after review | Conventional Commits, `feat(phase2.7/p2.7-NN-<slug>): …`; never pushed by an agent |

Dependency order is strict: **1 → 2 → 3 → 4 → 5 → 6.** Step 4's `fah-config`
part could go earlier, but the tick integration needs 2 and 3, so it stays.

---

## Step 1 — `fah-model` (p2.7-01)

### Files

| File | Change |
| --- | --- |
| `crates/fah-model/src/mac.rs` | new: `MacAddr`, `ParseMacAddrError` |
| `crates/fah-model/src/lib.rs` | `mod mac; pub use mac::{MacAddr, ParseMacAddrError};` beside the `policy` export (~line 39) |
| `crates/fah-model/src/policy.rs` | `ClientSelector::Mac(MacAddr)`; `matches(&self, ip, name, mac: Option<MacAddr>)`; `specificity`: `Mac` = 950; tests ~280–333 |
| `crates/fah-model/src/engine.rs` | `EngineCounters.routeros_poll_failures: u64`, `#[serde(default)]`, after `tasks_died` (~line 95) |
| `crates/fah-rules/src/policy.rs` | mechanical only, so the workspace compiles: `matches(ip, name)` → `matches(ip, name, None)` at ~245, ~290, ~565–567. Real `Mac` handling is step 3 |

### Interfaces

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacAddr([u8; 6]);
impl MacAddr { pub const fn octets(self) -> [u8; 6]; pub const fn from_octets([u8; 6]) -> Self; }
impl FromStr for MacAddr;   // six hex pairs, `:` or `-`, any case; rejects all-zero, broadcast, multicast (bit 0 of octet 0)
impl Display for MacAddr;   // lowercase, colon-separated
impl Serialize / Deserialize for MacAddr; // as the Display string
pub enum ClientSelector { Ip(IpAddr), Network {..}, Name(String), Mac(MacAddr) }
```

### Invariants

- `fah-model` purity: data types and trivial traits only. `FromStr` is the
  same class as `OperatingMode`'s parser. No I/O, no allocation in `MacAddr`.
- `matches(_, _, None)` is `false` for `Mac`; the hot path never learns a MAC.
- Serde form is the lowercase colon string; round-trips exactly.
- Order among selectors: `Name` 1000 > `Mac` 950 > `Ip` 900 > `Network(/n)`.

### Tests

| Test | Asserts |
| --- | --- |
| `mac_parses_both_separators_and_any_case` | `AA-BB-CC-DD-EE-FF`, `aa:bb:cc:dd:ee:ff` → same value; `Display` is `aa:bb:cc:dd:ee:ff` |
| `mac_rejects_short_long_zero_broadcast_multicast_and_junk` | each is `Err` |
| `mac_serde_round_trips_as_a_string` | JSON `"aa:bb:…"` both ways |
| `selectors_match_addresses_names_and_prefixes` (updated) | existing cases with `None` for mac; `Mac(m).matches(ip, None, Some(m))` true; `Some(other)` and `None` false |
| `a_name_outranks_a_mac_which_outranks_an_address` | replaces the three-way ordering test |
| `engine_counters_without_the_new_field_still_deserialize` | old telemetry/perf JSON loads with `routeros_poll_failures == 0` |

### Acceptance

- `cargo test -p fah-model`, workspace green.
- `crates/fastadhunter/tests/layering.rs` unchanged and passing.
- Deployed behaviour: identical (types only).

### Compatibility

- `EngineCounters` is persisted in perf rows and served by `/api/v1/telemetry`;
  `serde(default)` keeps old rows loadable and old dashboards ignore the field.
- `ClientSelector` is never serialized outside memory (config keeps strings),
  so the new variant breaks no file.

---

## Step 2 — `fah-stats` (p2.7-02)

### Files

| File | Change |
| --- | --- |
| `crates/fah-stats/src/client_registry.rs` | `ClientRecord.mac`, `DeviceRecord`, `devices`, `pending`, the new methods, eviction and expiry changes, `heap_bytes`, tests |
| `crates/fah-stats/src/stats.rs` | wrappers on `Stats`: `pending_families`, `apply_poll`, `mapped_clients`, `devices`, `set_device_name`, `delete_device`; `expire_idle_clients` also prunes devices; test helpers |
| `crates/fah-stats/src/heap.rs` | nothing new if `hashmap_bytes::<MacAddr, DeviceRecord>` fits the existing helper; otherwise a sibling |
| `crates/fah-stats/benches/record.rs` | run only, A/B |

### Data model

```rust
struct ClientRecord {
    name: Option<String>,            // legacy, address-level; stays for unmapped addresses
    first_seen, last_seen, buckets, intercepted,
    #[serde(default)] mac: Option<MacAddr>,
}
#[derive(Default, Serialize, Deserialize)]
struct DeviceRecord { name: Option<String>, lease_name: Option<String> }
pub(crate) struct ClientRegistry {
    clients: HashMap<IpAddr, ClientRecord>,
    #[serde(default)] devices: HashMap<MacAddr, DeviceRecord>,
    #[serde(skip, default = "default_capacity")] capacity: usize,
    #[serde(skip)] pending: HashSet<IpAddr>,
}
const MAX_NAMED_DEVICES: usize = 1024;
pub struct PendingFamilies { pub v4: bool, pub v6: bool }
pub enum PollOutcome { Success { full: bool }, Failure }
pub struct DeviceView { mac, name, lease_name, addresses: Vec<IpAddr>, first_seen, last_seen, queries_24h, blocked_24h }
// ClientView gains: mac: Option<MacAddr>, device_name: Option<String>
```

### Behaviour, method by method

| Method | Rule |
| --- | --- |
| `entry(ip, at)` (inside `record`/`record_intercepted`) | unchanged, plus: a **newly inserted** record has `mac: None` and is added to `pending`. Re-recording an existing unmapped record does **not** re-add it |
| eviction at cap (inside `entry`) | victim = min by `(is_named, last_seen)` where `is_named = record.name.is_some() \|\| device(mac).name.is_some()`; admission always succeeds |
| `pending_families()` | families present in `pending` |
| `apply_poll(rows, leases, outcome)` | `Failure`: no change. `Success`: for each `(ip, mac)` with a record: set `record.mac = Some(mac)` (overwrite), ensure `devices[mac]`; if `record.name` is `Some` and the device is unnamed, move it (device gets it, record cleared); if the device is already named, leave the record's name in place. For each `(mac, lease_name)` whose device exists: set `lease_name`. Never create a device from a lease alone. Then `pending.clear()` |
| `expire_idle(now, max_age)` | remove a record if `record.name.is_none()` and `older_than(last_seen, now, max_age)` — a device's name does **not** shield its addresses, a record-level name still does (that is the no-connector case, Fix 1 unchanged). Then `prune_devices()`: remove every device with `name.is_none()` and no record pointing at it. Returns addresses removed |
| `name(ip)` | `record.name` else `devices[mac].name` |
| `device_name(ip)` | `name(ip)` else `devices[mac].lease_name` |
| `named()` | `(ip, effective name)` for every record with one — feeds `Name` resolution, so device-named addresses resolve |
| `mapped()` | `(ip, mac)` for every mapped record — feeds `Mac` resolution |
| `set_name(ip, name)` | record has `mac` → `set_device_name(mac, name)`; else `record.name = name`. Returns the view, or `Err(NameCapReached)` |
| `set_device_name(mac, name)` | device must exist, else `None`; naming a currently unnamed device when 1024 devices are already named → `Err(NameCapReached)`; clearing always allowed |
| `delete_device(mac)` | clears the name; removes the device if no record points at it; `true` if it existed |
| `devices(now)` | one view per device, addresses sorted by `last_seen` desc, sums of 24 h counters, min `first_seen`, max `last_seen` |
| `heap_bytes()` | + device map buckets + name and lease-name string bytes |

`Stats` wrappers take the `clients` Mutex once per call; `apply_poll` is called
**after** the fetch completed, never across an `await`.

### Invariants

- Only traffic writes `last_seen`; only a successful poll writes `mac`.
- After every mutation: an unnamed device has ≥ 1 record pointing at it; named
  devices ≤ 1024; `clients.len() ≤ capacity`.
- `pending` is never persisted and is empty after any successful poll.
- Old snapshots load (`serde(default)` on `mac` and `devices`); a new snapshot
  loaded by `6350fc1` drops the two fields silently (serde ignores unknown
  fields on these structs) — verified by a test that deserializes the new
  JSON into the old shape.
- No per-query cost beyond one `HashSet::insert` on first sight.

### Tests (registry unless noted)

| # | Scenario | Asserts |
| --- | --- | --- |
| L1 | `A1` recorded at `t0`, poll maps `A1 → M`, name `"phone"` set via `A1`; `expire_idle(t0 + 8 d)` | `A1` gone; `M` present, `name == "phone"`, zero addresses; `named()` empty (no address), `devices()` lists `M` |
| L2 | then `A2` recorded at `t0 + 8 d`; `pending_families().v6`; `apply_poll([(A2, M)], [], Success)` | `A2.mac == M`; `device_name(A2) == "phone"`; `named()` contains `(A2, "phone")`; `mapped()` contains `(A2, M)`; `pending` empty |
| L4 (stats) | L1 then `save_snapshot`, `boot` into a fresh `Stats` | `M` named with zero addresses; `A1` absent; `pending` empty |
| L5 | L1 with `M` unnamed | `M` removed with `A1`; later `apply_poll([(A2, M)], [(M, "Liviu ASUS ROG")], Success)` recreates `M` with `lease_name` only |
| C1 | 1024 named devices, all addresses expired; 4200 new addresses | 4096 present, the newest; 1024 devices still named; no failure |
| C2 | map full, every address under a named device; one more | LRU address evicted, its device stays; new address present |
| C3 (stats) | 1024 named; `set_device_name` on a 1025th | `Err(NameCapReached)`; after `delete_device` on one, `Ok` |
| C4 | map full of unnamed-device addresses plus one named device's address; 4096 more | the named device's address is evicted last |
| — | `pending` semantics | new address pending; re-record does not re-add; `Failure` keeps; `Success` clears; boot-loaded records are not pending |
| — | name migration | address named before mapping → device takes it, record cleared; device already named → record keeps its own |
| — | write-through | `set_name(ip)` on a mapped address names the device; on an unmapped one names the record |
| — | snapshot compat | `6350fc1`-shaped JSON loads; new JSON loads into the old struct shape |
| — | no-connector parity | with no `apply_poll` ever called, every Fix 1 test still passes unchanged |
| — | `heap_bytes` | grows with devices and names, shrinks on delete |

### Acceptance

- `cargo test -p fah-stats`, workspace green.
- `cargo bench -p fah-stats record`: A/B against a `6350fc1` checkout, delta
  within noise; figure in the review file with corpus and box.
- Deployed behaviour: identical, because nothing calls `apply_poll` yet.

---

## Step 3 — `fah-rules` and `fah-config` (p2.7-03)

### Files

| File | Change |
| --- | --- |
| `crates/fah-rules/src/policy.rs` | `active_at(now, named, mapped)`; `PolicyState::refresh(policies, named, mapped)`; `Mac` expansion; `parse_selector` (~460); `ClientScope::parse` (~508) drops MAC terms; `PolicySet::resolve(ip, name, mac, now)` (~229–257) |
| `crates/fah-config/src/lib.rs` | `validate_assignment` (~393): a `client` containing `:` must parse as IPv6 or MAC |
| call sites | `crates/fastadhunter/src/main.rs` ~1293; `crates/fah-api/src/routes.rs` ~1227; `crates/fah-dns/tests/policy_enforcement.rs` 125, 221, 228, 276, 282, 378; `crates/fah-http/tests/filtering.rs` 493, 546; `fah-rules` tests using `resolve` (~625–746) |

### Interfaces

```rust
pub fn active_at(&self, now_unix_seconds: i64, named: &[(IpAddr, Arc<str>)], mapped: &[(IpAddr, MacAddr)]) -> ActivePolicies;
pub fn refresh(&self, policies: &PolicySet, named: &[(IpAddr, Arc<str>)], mapped: &[(IpAddr, MacAddr)]) -> bool;
pub fn resolve(&self, ip: IpAddr, name: Option<&str>, mac: Option<MacAddr>, now_unix_seconds: i64) -> PolicyId;
```

- `active_at`: `Mac(m)` expands, in place, to `Ip(ip)` for every `(ip, m)` in
  `mapped`, exactly like `Name` — the most-specific-first order is preserved,
  so a `Mac` assignment's expanded entries sit before `Ip` entries of a less
  specific assignment. `ActivePolicies.assignments` never contains `Mac`.
- `parse_selector`: `/` → prefix; `IpAddr` → `Ip`; `MacAddr` → `Mac`; else
  `Name`. Accepts both MAC separators.
- `ClientScope::parse`: a `Mac` term returns `None`, so the rule is dropped —
  the documented behaviour for a scope the engine cannot honour, and the hot
  path has no MAC to match.
- `validate_assignment`: `client` with `:` and no `/` → must be `IpAddr` or
  `MacAddr`, else `"… is not an address, prefix, MAC or name"`.

### Invariants

- `policy_for` unchanged: exact and prefix matching over expanded entries.
- `publish` still skips identical snapshots; a poll that changes nothing
  republishes nothing.
- `names` (for `$client=<name>`) unchanged; device-named addresses are in it
  because `named()` already resolves through the device.

### Tests

| # | Scenario | Asserts |
| --- | --- | --- |
| L3 | assignments `Name("phone") → kids` and `Mac(M) → kids`; `active_at(now, [(A2,"phone")], [(A2, M)])` | `policy_for(A2) == kids` with either assignment alone; `policy_for(A1) == default` |
| — | expansion never leaks `Mac` | after `active_at`, no `assignments` entry is `Mac` |
| — | `Mac` beats `Ip` | `Ip(A) → a` and `Mac(M) → b` with `(A, M)` mapped → `policy_for(A) == b` |
| — | `parse_selector` | `aa:bb:cc:dd:ee:ff`, `AA-BB-CC-DD-EE-FF` → `Mac`; `fd6c::1` → `Ip`; `aa:bb:cc` → `Name` unless `validate_assignment` rejects it first (config path) |
| — | `$client=aa:bb:…` | `ClientScope::parse` → `None`; a rule so scoped is dropped at compile |
| — | `validate_assignment` | `"aa:bb:cc:dd:ee"` rejected; `"aa:bb:cc:dd:ee:ff"` and `"fd6c::1"` accepted |
| — | no republish | `refresh` twice with equal inputs → `false` the second time |

### Acceptance

- `cargo test -p fah-rules -p fah-config -p fah-dns -p fah-http`, workspace green.
- Deployed behaviour: identical (`mapped` is empty until step 4).

### Compatibility

- `[[policies.assignments]] client = "aa:bb:cc:dd:ee:ff"` becomes valid TOML
  and API input; the dashboard's free-text row accepts it unchanged.

---

## Step 4 — binary: config, RouterOS source, planner, tick (p2.7-04)

### Files

| File | Change |
| --- | --- |
| `crates/fah-config/src/schema/routeros.rs` | new `RouterOsConfig { url, user, password_file, ca_file, timeout_ms }` with defaults |
| `crates/fah-config/src/schema/mod.rs` | `mod routeros; pub use`; `Config.routeros` |
| `crates/fah-config/src/lib.rs` | validation; reference TOML block; tests |
| `crates/fah-config/src/env.rs` | five `FAH__ROUTEROS__*` mappings |
| `crates/fah-api/src/config_store.rs` | `BOOT_KEYS` + `"routeros"`; boot-key test row |
| `crates/fah-metrics/src/registry.rs` | `routeros_poll_failures: AtomicU64`, `record_routeros_poll_failure()`, export (~69, ~120, ~256, ~327) |
| `crates/fastadhunter/src/routeros/mod.rs` | `RouterOsSource` trait, `Tables`, `PollResult`, `PollError` |
| `crates/fastadhunter/src/routeros/planner.rs` | pure `Planner` |
| `crates/fastadhunter/src/routeros/rest.rs` | `RestSource`: reqwest client, parsing |
| `crates/fastadhunter/src/main.rs` | build the source in `run()` (~385–395); `spawn_policy_ticker` (~1277) gains the poll |
| `crates/fastadhunter/Cargo.toml` | none expected: `reqwest` (json) and `rustls` are present |

### Config

```toml
[routeros]                                  # whole section boot-class
url = ""                                    # REST base; empty disables the connector
user = "fastadhunter"
password_file = "/config/routeros-password"
ca_file = ""                                # PEM of the issuing CA; empty = compiled-in roots
timeout_ms = 5000                           # whole-poll budget, 100–10000
```

Validation: `url` empty, or `https://` with a non-empty host (any other scheme
rejected); when `url` is set, `user` and `password_file` non-empty;
`timeout_ms` in `100..=10_000`. Env: `FAH__ROUTEROS__URL` etc.

### Interfaces

```rust
pub struct Tables { pub arp: bool, pub neighbors: bool, pub leases: bool }
pub struct PollResult { pub mapped: Vec<(IpAddr, MacAddr)>, pub leases: Vec<(MacAddr, String)> }
pub enum PollError { Transport(String), Status(u16), Decode(String) }
pub trait RouterOsSource: Send + Sync {
    fn fetch(&self, tables: Tables) -> Pin<Box<dyn Future<Output = Result<PollResult, PollError>> + Send + '_>>;
}
pub struct Planner { last_full: Option<Instant>, every: Duration }   // every = 600 s
impl Planner {
    pub fn plan(&self, now: Instant, pending: PendingFamilies) -> Option<Tables>;
    pub fn completed(&mut self, now: Instant, tables: Tables);        // full → last_full = now
}
```

Planner rules: `last_full` unset or ≥ 600 s ago → all three (full); else
`pending.v6` → neighbors, `pending.v4` → arp + leases, both → all; none →
`None`. First tick after boot is therefore a full refresh, which also maps the
addresses loaded from the snapshot.

`RestSource`: `reqwest::Client` with `https_only(true)`, `timeout`,
`hickory_dns(true)` as the list fetcher, `add_root_certificate` from `ca_file`
when set, basic auth from the password file read once at boot. Issues the
requested GETs **concurrently** (`tokio::join!`). Rows are `serde` structs
with every field `Option<String>` (`#[serde(rename = "mac-address")]` etc.);
a row is kept only if `address` parses, `mac-address` parses, and `status` is
not `failed`/`incomplete`. Leases: `comment` over `host-name`, trimmed,
non-empty. The struct's `Debug` prints no password.

Tick (`spawn_policy_ticker`), per tick, in this order:

1. `plan = source.as_ref().and_then(|_| planner.plan(now, stats.pending_families()))`
2. if `Some(tables)`: `tokio::time::timeout(budget, source.fetch(tables))`;
   `Ok(Ok(r))` → `stats.apply_poll(&r.mapped, &r.leases, Success{full})`,
   `planner.completed(now, tables)`; anything else → `metrics.record_routeros_poll_failure()`,
   `LogThrottle` warn, `stats.apply_poll(&[], &[], Failure)`
3. `policies.refresh(&rules.policies(), &stats.named_clients(), &stats.mapped_clients())`

Boot: `url` set and the password file unreadable → one `error` line, source
`None`, resolver runs. `ca_file` unreadable or not PEM → same.

### Invariants

- At most one `fetch` per tick, by construction of the planner.
- `refresh` runs on every tick, at most `timeout_ms` late.
- No lock across an `await`: fetch, then `apply_poll` under the Mutex.
- Only `GET`s; credentials never logged or `Debug`-printed.
- Parsing cannot panic: all fields optional, no `unwrap`, decode errors are
  `PollError::Decode`.
- `url = ""` → `source` is `None` → tick body identical to today.

### Tests

| # | Where | Scenario | Asserts |
| --- | --- | --- | --- |
| P1 | ticker with `FakeSource` | new IPv6 `A` recorded; tick; fake returns `Err` | one `fetch` call; `A` unmapped; `policy_for(A) == default`; `A` still pending; counter == 1 |
| P2 | same | next tick; fake returns `[(A, M)]` | one call; `A → M`; snapshot published carries `A` under `M`'s assignment |
| P3 | planner | 50 new addresses, both families | one plan, all three tables; after `Success`, no pending |
| P4 | planner | v6-only pending → `Tables{neighbors}` only; v4-only → arp + leases |
| P5 | registry + planner | successful poll leaves `A` unmapped | `A` not pending; next tick `plan == None`; at 600 s a full plan |
| P6 | planner | 20 simulated minutes, nothing pending | exactly 2 plans, at 600 s and 1200 s |
| P7 | ticker | fake hangs; `timeout_ms = 200` | tick completes ≈ 200 ms; `refresh` ran; `A` pending; counter == 1 |
| — | parser fixtures | the ADR's captured rows for the three tables | `failed` row skipped; string booleans ignored; lease name = comment over host-name |
| — | config | defaults; `http://` rejected; `timeout_ms = 20000` rejected; env override; reference TOML; `routeros` classified boot |
| — | boot | `url` set, password file missing → source `None`, no panic |
| — | metrics | counter increments and appears in `engine_telemetry().counters` |

The ticker tests run the tick body as a function (`policy_tick(...)`) rather
than the spawned loop, with a paused Tokio clock.

### Acceptance

- Workspace green; `cargo test -p fastadhunter` includes P1–P7.
- `GET /api/v1/telemetry` carries `counters.routeros_poll_failures`.
- Deployed behaviour: identical with the default config; with a URL set and a
  reachable router, the registry starts carrying MACs (visible in step 5).

### Compatibility

- New optional section; old TOML loads unchanged; `POST /api/v1/config` on any
  `routeros.*` key answers `restart_required: true`.

---

## Step 5 — `fah-api` and dashboard (p2.7-05)

### Files

| File | Change |
| --- | --- |
| `crates/fah-api/src/ports.rs` | `ClientEntry` + `mac`, `device_name`; `DeviceEntry`; `StatsSource` + `mapped_clients`, `devices`, `set_device_name`, `delete_device` |
| `crates/fah-api/src/wire.rs` | `ClientResponse` + `mac`, `device_name`; `DeviceResponse`, `DevicesResponse`, `DeviceNameRequest` |
| `crates/fah-api/src/routes.rs` | `clients` (~413) passes the new fields; `/devices` `get`, `/devices/{mac}` `get`/`put`/`delete` beside `/clients` (~60–64); `PolicyResolver` (~1145) gains `policy_of_device` = policy of the most recently seen address; the write-through `refresh` (~1227) passes `mapped_clients()` |
| `crates/fastadhunter/src/adapters.rs` | the four new pass-throughs |
| test doubles | `crates/fah-api/tests/api.rs` `FakeStats`; `crates/fah-api/src/events.rs`; `crates/fastadhunter/tests/history_e2e.rs` |
| `crates/fastadhunter/tests/routeros_e2e.rs` | new: local HTTPS server with a test certificate serving the three fixtures; asserts basic auth, the paths per plan, and `mac` on `GET /api/v1/clients` |
| `dashboard/frontend/src/api/types.ts` | `Client.mac`, `Client.device_name`; `Device`; `Config.routeros` |
| `dashboard/frontend/src/api/devices.ts` | `getDevices`, `setDeviceName`, `deleteDevice` |
| `dashboard/frontend/src/pages/clients.tsx` + `clients/*.tsx` | group rows by `mac`; device row; rename targets the device when `mac` is set |
| `dashboard/frontend/src/pages/settings/metadata.ts` + fixtures | `[routeros]` section, five fields, `restart` |
| tests | `api.rs`, `clients.test.tsx`, `resources.test.ts`, `settings*.test.ts*` |

### API contract

| Endpoint | Behaviour |
| --- | --- |
| `GET /api/v1/clients` | items gain `mac` (string or null) and `device_name` (user name, else lease name, else null). `family` and `seen_within` unchanged |
| `GET /api/v1/devices` | `{ items: [ { mac, name, lease_name, addresses: [ip…], first_seen, last_seen, queries_24h, blocked_24h, policy } ] }`; `?seen_within=` applies to the device's `last_seen` |
| `GET /api/v1/devices/{mac}` | one item; `404` unknown; `400` malformed MAC |
| `PUT /api/v1/devices/{mac}` | `{ "name": "phone" \| null }`; `200` with the item; `404`; `400`; `409` when 1024 named devices exist and this one is unnamed. Republishes policies (same write-through as a client rename) |
| `DELETE /api/v1/devices/{mac}` | `204`; `404`. Clears the name; removes the device when it has no live address |
| `PUT /api/v1/clients/{ip}` | unchanged shape; writes to the device when the address has a MAC (`409` on the cap) |

### Dashboard

- **Two requests on mount stay** (`/clients`, `/policies`): grouping is
  derived client-side from `mac` on each item; `/devices` is for API users
  and for the rename/delete actions.
- Clients page: one row per device (name from `device_name`, addresses folded
  under it, sums as the row's figures, policy of the newest address, "n
  addresses" badge), MAC-less clients remain single rows exactly as today.
  Rename on a device row → `PUT /devices/{mac}`; on a MAC-less row →
  `PUT /clients/{ip}`. Family and `seen_within` chips unchanged; sort by the
  device's totals.
- Settings page: `[routeros]` section, all `restart`; `password_file` and
  `ca_file` are paths, shown as text.

### Invariants

- Additive API: every existing field keeps its name and type; tui-monitor and
  the Top-clients card need no change.
- No per-row requests on the clients page; `GET /devices` is never called on
  mount.
- A rename or delete republishes the policy snapshot immediately (write-through
  at ~1227), so the identity gap for a rename is milliseconds, as today.

### Tests

| # | Where | Scenario | Asserts |
| --- | --- | --- | --- |
| L6 | `api.rs` | `PUT /clients/{A1}` name, then registry maps `A1 → M`, expires `A1`, maps `A2 → M` | `GET /clients?seen_within=24h` lists `A2` with `device_name == "phone"`, not `A1`; `GET /devices/{M}.addresses == [A2]` and `name == "phone"` |
| L7 | `clients.test.tsx` | fixture with two addresses on one `mac` | one device row `"phone"` with both addresses under it, policy `kids`, totals summed |
| C3 | `api.rs` | 1024 named; `PUT /devices/{new}` | `409`; `DELETE` one; `PUT` → `200` |
| — | `api.rs` | `/devices` list, `404`, `400` (`zz:…`), `PUT` name and clear, `DELETE` `204`/`404`, `seen_within` on devices |
| — | `api.rs` | `GET /clients` items carry `mac` and `device_name`; nulls when unmapped |
| — | `routeros_e2e.rs` | end to end against the local HTTPS fixture server | `Authorization: Basic` present; the first tick requests all three paths; `GET /api/v1/clients` shows the fixture MAC for the fixture address; a `401` from the server increments the counter |
| — | dashboard | grouping, rename routing by `mac`, MAC-less rows unchanged, settings metadata lists the five keys as restart, resources paths |

### Acceptance

- Workspace and dashboard green.
- Deployed behaviour with default config: `mac` and `device_name` are `null`
  everywhere, the clients page renders exactly as at `6350fc1` (test pins it).

---

## Step 6 — docs, deploy, on-device verification (p2.7-06)

### Docs (each edit needs its own yes)

| File | Edit |
| --- | --- |
| CONTEXT.md | new **Device**; §Client amended (identity by MAC when known); §Supervised Task unchanged (the poll lives inside the policy ticker) |
| ARCHITECTURE.md | §Runtime Model: the on-demand poll on the policy tick; §Workspace Layout: `fastadhunter/src/routeros/` |
| SECURITY.md | §Phase 3: "the only identity the container sees" amended; the router credential and CA file under §Data at rest |
| CONFIGURATION.md | `[routeros]` block; §Mutability classes: the section is boot |
| API.md | `mac`/`device_name` on clients; the three `/devices` endpoints; `PUT /clients/{ip}` write-through; `409` |
| RULE_ENGINE.md | `$client=` with a MAC term drops the rule |
| docs/deploy-rb5009.md | the two router commands, the password file, exporting `localbox-ca`, `ca_file`, setting `url` via `POST /config`, restart, verification reads |
| docs/routeros-traps.md | `www-ssl address=` restricts the *source*; `172.17.0.2` is outside the LAN list |
| docs/public-certificate.md | `localbox-ca`: where it lives and how `router-localbox` renews (the ADR's open item) |
| docs/project-state.md | rewrite the "where the work is" entry |
| docs/code-review/phase2.7/p2.7-06-…-review.md | the on-device figures below |

### Deployment (owner-run, proposed by the agent, in this order)

1. `/user/add name=fastadhunter group=monitor address=172.17.0.2/32 password=<generated>`
2. `/ip/service/set www-ssl address=192.168.10.0/24,172.17.0.2/32`
3. Export the `localbox-ca` certificate to PEM; copy it and a one-line
   password file into `/config` on the SSD (mode 0600).
4. `POST /api/v1/config {"routeros": {"url": "https://router.localbox.ro:8443/rest", "ca_file": "/config/localbox-ca.pem"}}` → `restart_required: true`; restart the container.

### On-device verification (read-only, into the review file)

| Check | Expected |
| --- | --- |
| `GET /api/v1/clients?seen_within=1h` | every live address carries `mac`; the two known devices appear with lease names |
| `GET /api/v1/telemetry` | `counters.routeros_poll_failures == 0` after an hour |
| `GET /debug/memory` | `stats.clients` heap before/after, the device map's share |
| router `/log` | no per-request login lines; `/user/print` shows `fastadhunter` last-logged-in from `172.17.0.2` |
| identity gap | one rotation observed: `first_seen` of the new address vs the tick that mapped it, ≤ 20 s + poll |
| a policy on a `Mac` or a device name | still in force on the rotated address the next day |

### Acceptance

- Docs match the shipped code; the ADR's Status line becomes **accepted** with
  the deployment date.
- The review file holds the table above with real figures, corpus and device.

---

## Compatibility summary

| Surface | Old → new | New → old |
| --- | --- | --- |
| Stats snapshot | loads; `mac` and `devices` default | `6350fc1` ignores the unknown fields; names moved to devices are **not** visible to the old binary (they were moved out of the address record) — a downgrade after step 5 loses device names, documented in the release note |
| Config TOML | `[routeros]` optional, default disabled | an old binary rejects the unknown section (`deny_unknown_fields`) — remove it before downgrading |
| Policies TOML | MAC selectors new | an old binary treats a MAC string as a name that matches nothing |
| API | additive fields and endpoints | — |
| Telemetry / perf rows | new counter, `serde(default)` | ignored |

## Risks and their checks

| Risk | Where it is caught |
| --- | --- |
| Container cannot resolve `router.localbox.ro` | step 6 verification; fallback is the container's `/etc/resolv.conf` pointing at the router, read-only check `/container/print` |
| `www-ssl` address list not extended | step 6 command 2; the counter goes up, the log says `connection refused` |
| Phone MAC randomisation set to rotating | documented in CONTEXT.md §Device; the device row simply splits |
| A name that looks like a MAC | `validate_assignment` and `parse_selector` agree: it is a MAC |
| Poll cost on the tick | `timeout_ms ≤ 10 s` by validation; P7 |
| Hot-path regression | `record` bench A/B in step 2 |
