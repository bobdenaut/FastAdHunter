# Device identity comes from the RouterOS REST API

Status: **proposed 2026-09-21**, verified against the device the same day
(§Verification on the device, all open items closed), awaiting the owner's
final review. Nothing implemented, nothing changed on the router.

Every per-device control is keyed to an exact source IP, and 6–7 devices on
this LAN replace their IPv6 address about once a day
([ipv6-privacy-rotation-review.md](../code-review/phase2.6/ipv6-privacy-rotation-review.md)).
Fix 1 of that review (idle expiry, `seen_within`) shipped in `6350fc1` and
stops the registry growing. It does not make a policy or a name survive a
rotation. This ADR decides where a durable identity comes from: **the MAC
address, read from the router over its REST API, read-only.**

## Decision

| Item | Decision |
| --- | --- |
| Identity | the device's MAC address, `MacAddr([u8; 6])` in `fah-model` |
| Source | RouterOS REST, read-only GETs: `/ip/arp`, `/ipv6/neighbor`, `/ip/dhcp-server/lease` |
| When | **on demand**: on a tick where at least one address without a MAC appeared, fetching only that family's tables; plus a full refresh of all three every 10 min; **never more than one poll per tick**; no fixed 20 s polling |
| Who polls | the binary (L4 owns networking), inside the existing 20 s policy ticker, before `PolicyState::refresh` |
| Who maps | `fah-stats`: the registry stores IP → MAC once learned and groups addresses under a device; pure logic, no I/O |
| Who resolves | `fah-rules`: `ClientSelector::Mac` expands to the device's current addresses at snapshot build, the same shape `Name` has today |
| Hot path | untouched: no lookup per query, no new lock; the registry mutation takes the existing `clients` Mutex on the tick |
| Off by default | `[routeros] url = ""` disables the connector; nothing changes for a deployment without a router user |

## Why REST, and why not the local neighbour table

The container is **routed**, not bridged: `veth1` on the `CONTAINERS` bridge,
gateway `172.17.0.1` ([routeros-traps.md](../routeros-traps.md) §Container
network). Every client packet reaches it dst-natted, so the source MAC it could
read is the router's, never the client's — tui-monitor's `auto_name` exists for
exactly this reason (`tui-monitor/src/config.rs`). The kernel neighbour table
of the container's namespace therefore holds one entry, the gateway.

| | REST connector (chosen) | Local neighbour table via netlink |
| --- | --- | --- |
| Router change | one read-only user, `www-ssl` reachable from `172.17.0.2` | move `veth1` onto `BRIDGE`; repoint DHCP `dns=`, RA `dns=`, two `:53` dstnat targets, the `/ip/dns` watchdog, every firewall rule naming `172.17.0.2` |
| Blast radius | none on the resolver path | the household's live DNS, re-plumbed |
| Unverified | nothing | whether a RouterOS container may issue `RTM_GETNEIGH` at all |
| Security | credentials for a read-only user, over verified TLS | LAN reaches the container at L2; the router firewall no longer sits between them |
| Coupling | MikroTik-specific by construction; this project is ARM64-first for the RB5009 and the appliance already assumes RouterOS (ADR-0004) | portable |
| Precedent in repo | `tui-monitor/src/client/routeros.rs` already issues two of the three GETs read-only | none |

The owner chose REST on 2026-09-21. Portability is the price; the deployment
has one router and the project's hardware target is that router.

## Required RouterOS permissions

A dedicated user in the group tui-monitor already proves, restricted to the
container's address. The group `monitor` exists on the device with
`policy=local,read,api,rest-api` and everything else negated
(`/user/group/print`, 2026-09-21); its user `monitor` logs in over REST from
`192.168.10.10/32` daily, so that policy set is the verified minimum on
RouterOS 7.21.5. `read` covers the three tables, `rest-api` gates `/rest`;
`local` and `api` are in the proven set and are kept rather than trimmed
untested. No `write`, no `sensitive`, no `web`, no `password`.

**Proposed commands, owner-run, none applied by an agent:**

```routeros
/user/add name=fastadhunter group=monitor address=172.17.0.2/32 password=<generated>
/ip/service/set www-ssl address=192.168.10.0/24,172.17.0.2/32
```

- `address=172.17.0.2/32` on the user: the login is refused from anywhere but
  the container, so a leaked password is useless off the `CONTAINERS` bridge.
  The same pattern the `monitor` user already carries.
- The second command is **required, not hardening**: `www-ssl` is restricted
  to `address=192.168.10.0/24` today (`/ip/service/print detail`), and the
  container is `172.17.0.2`. Without it every poll is refused at the service,
  before authentication. Takes effect immediately; revert by setting the
  previous list back.
- **Input chain: accepted, no rule needed** (read 2026-09-21). The IPv4
  `input` chain is: 1 accept `established,related,untracked`; 2 drop
  `invalid`; 3 accept ICMP; 4 accept to `127.0.0.1`; 5–7 disabled (`WWW`
  80/443/8443); 8 WireGuard on `DIGI`; 9 Winbox via `wg0`; 10 **drop
  `in-interface-list=!LAN`**; then the chain's default accept.
  `/interface/list/member` holds `LAN = {BRIDGE, CONTAINERS}` and
  `WAN = {ether1, DIGI}`. A SYN from `172.17.0.2` arrives on `CONTAINERS`,
  matches nothing in 1–9, is not `!LAN` at 10, and is accepted by default;
  the reply path is rule 1. The only gate is the service's own `address=`
  list, hence the second command above.

## Credentials and configuration

New section, all keys **boot-class** (the client is built once, like every
listener):

```toml
[routeros]
url = ""                                    # boot — REST base, e.g. "https://router.localbox.ro:8443/rest"; empty disables the connector
user = "fastadhunter"                       # boot
password_file = "/config/routeros-password" # boot — one line, mode 0600, read once at boot
ca_file = ""                                # boot — PEM of the router's certificate; empty verifies against the compiled-in roots
timeout_ms = 5000                           # boot — budget for one whole poll, all three GETs together; 100–10000
```

- **The password is a file, not a key.** Every secret this appliance holds is
  a standalone file under `/config` (`auth-hash`, the API key, the CA key —
  SECURITY.md §Data at rest); `GET /api/v1/config` has no redaction layer and
  "secrets redacted" holds by construction because no key is a secret. A TOML
  password would appear in that response, on the dashboard settings page and
  in the TOML `config_store` writes back. The file's *path* is a key; its
  content never leaves the process, is never logged and has no `Debug` output.
- **TLS is verified, always.** Credentials travel as HTTP basic auth, so the
  peer must be the router. `www-ssl` serves certificate `router-localbox`:
  `CN=router.localbox.ro`, issuer `CN=localbox-ca`, valid to 2036-09-06
  (`openssl s_client`, 2026-09-21). That issuer is a private CA, so `ca_file`
  is its certificate in PEM, copied by the owner to `/config`, and `url` names
  the host as `router.localbox.ro` so the name matches; the container resolves
  it through its own upstreams (the public `A` record exists,
  [public-certificate.md](../public-certificate.md) §Open items). An IP-literal
  URL would fail name verification by design. There is **no accept-any
  option.** tui-monitor's `danger_accept_invalid_certs(true)` is a monitor on
  a dev box; the resolver holds a router login and does not get it. A
  `http://` URL fails validation. Open item: `localbox-ca` is not described
  anywhere in this repository; where its key lives and how `router-localbox`
  is renewed belongs in [public-certificate.md](../public-certificate.md) or a
  sibling before the connector ships.
- Same HTTP client stack as the list fetcher: `reqwest` on rustls, already in
  the binary; a second `Client` with the router's trust settings, nothing new
  in the tree.
- Boot without the file when `url` is set is a configuration error, reported
  once at `error`; the connector stays off and the resolver runs.

## How an address becomes a device

| Address | Table | Row fields used | Note |
| --- | --- | --- | --- |
| IPv4, DHCP client or reservation | `/ip/dhcp-server/lease` | `address`, `mac-address`, `host-name`, `comment` | reservations are listed even while the device is offline |
| IPv4, any active host | `/ip/arp` | `address`, `mac-address`, `status` | covers static-IP hosts; `failed`/`incomplete` rows are skipped |
| IPv6, temporary (RFC 4941) or stable | `/ipv6/neighbor` | `address`, `mac-address`, `status` | the router learned the neighbour when it forwarded the device's query, so the row exists by the time FAH sees the address; `failed` rows are skipped |
| HTTPS proxy source (temporary GUA) | `/ipv6/neighbor` | same | the proxy records the same source IP into the same registry; one map serves both pipelines |

### When the router is asked

| Trigger | Tables fetched | Bound |
| --- | --- | --- |
| an IPv6 address without a MAC was recorded since the last successful poll | `/ipv6/neighbor` | one poll per tick |
| an IPv4 address without a MAC was recorded since the last successful poll | `/ip/arp` + `/ip/dhcp-server/lease` | same poll if both families are pending |
| 10 min since the last full refresh (`FULL_REFRESH`, a constant) | all three | one per 10 min |
| nothing pending, refresh not due | none | zero requests |

- The registry keeps a **pending set**: addresses recorded with no MAC since
  the last *successful* poll. The tick reads it, plans the fetch, polls at
  most once, applies the result, then `refresh` runs. Fifty new addresses in
  one tick are one poll.
- After a **successful** poll an address still unmapped (absent from the
  tables — a `failed` neighbour row, a host behind `wg0`) leaves the pending
  set; it is retried by the next full refresh, not every tick. After a
  **failed** poll the pending set is kept, so the next tick retries, still at
  most once per tick.
- Steady state on this LAN: about seven rotations a day and the odd new IPv4
  host, so a handful of demand polls per day plus 144 full refreshes (432
  GETs), against 4320 polls (12 960 GETs) a day for fixed 20 s polling.
- Worst case under a spoofed-source flood or a dead router: one poll per
  tick, the same ceiling fixed polling would have had, and `timeout_ms`
  bounds each.

Each poll is parsed into `(IpAddr, MacAddr)` pairs and `(MacAddr, lease name)`
pairs, bounded by the LAN's table sizes. The registry then:

- sets `mac` on every client record whose IP appears, and **overwrites** a
  different MAC when the router says so (DHCP reassignment); the router is
  authoritative;
- keeps the mapping once learned: the router's neighbour cache expiring an
  idle address does not unlearn it here;
- stores a **lease name** per MAC (`comment` over `host-name`) as display
  data only. The NAME column shows the user-assigned name, else the lease
  name. A lease comment does **not** match `ClientSelector::Name`: a policy
  must not silently change because a comment on the router did.

**Fix 1 semantics stay.** Idle expiry still removes an *address* unseen for
`client_idle_expiry_days`, whether or not its device is named. The device
record — MAC, user name, lease name — is what persists; its lifetime is decided
in §Device retention, and the lifecycle in §Required lifecycle tests.

**Names move to the MAC.** A name set through `PUT /api/v1/clients/{ip}` is
stored on that address's MAC when one is known, else on the address as today.
At boot, a legacy IP-keyed name moves to the device the first poll maps it to.
`$client=~laptop` rules and `ClientSelector::Name` therefore follow the device
once the connector is on, with no rule edits.

## `ClientSelector::Mac`

- Written as the six-octet colon form, `aa:bb:cc:dd:ee:ff`, case-insensitive,
  accepted wherever a selector is (`[[policies.assignments]]`, `POST`/`PATCH
  /api/v1/policies`, the policy dialog's free-text row).
- Resolved at snapshot build, not per query: `PolicySet::active_at` receives
  the registry's current `(IpAddr, MacAddr)` pairs beside the named set, and
  `Mac(m)` expands to every address mapped to `m`, exactly as `Name` expands
  today. `ActivePolicies::policy_for` stays an exact-address lookup.
- Specificity `950`: above one address (`900`), below a name (`1000`). A MAC
  is a device statement; a name is the operator's own word for that device.
  Reviewable.
- `matches(ip, name)` gains a `mac: Option<MacAddr>` argument; the
  Interception Document stays IP/CIDR-keyed in this ADR (SECURITY.md §Phase 3),
  a MAC form for it is a later, separate change.

## The identity gap

| Step | When |
| --- | --- |
| Device rotates, first query from the new address | t₀; the fan-out records the address, unnamed, no MAC, into the pending set |
| Next policy tick: the pending address triggers a poll of its family's tables, then `refresh` | ≤ 20 s after t₀ (`POLICY_TICK`), plus the poll, bounded by `timeout_ms` |
| Snapshot published with the new address under the device's assignments | same tick |

**Maximum gap: one tick plus one poll, about 20 s, during which the address
resolves to `default`.** Polling before `refresh` on the same tick is what
keeps it to one tick rather than two; a poll that fails or times out leaves
the address pending, the snapshot is rebuilt from what is already known, and
the next tick retries. The gap after a failed poll is therefore one tick per
failure, never a silent permanent lapse.

### `timeout_ms` bounds the whole poll, and `refresh` by the same amount

- The GETs a poll needs are issued **concurrently** (`futures::join`), not one
  after another; a normal poll costs one round trip.
- `timeout_ms` wraps the **entire** poll in one `tokio::time::timeout`, connect
  and body included. Worst case is therefore `timeout_ms`, never three times
  it; with the default, `refresh` runs at most 5 s late on that tick.
- Validation rejects `timeout_ms > 10_000` (half of `POLICY_TICK`) and
  `< 100`, so no configuration can let a poll consume a tick. The ticker keeps
  `MissedTickBehavior::Skip`; with the poll bounded at half a tick it never
  skips.
- A refused connection, a `401` or a `503` returns immediately; only a
  black-holed peer costs the full budget. The tick's cost is then
  `timeout_ms` of waiting inside one async task on the runtime: nothing on the
  DNS path, no lock held while waiting.
- The poll's `Result` is logged through `LogThrottle` and counted; a panic in
  the parser would end the policy ticker (a task death), so the parser is
  `serde` with every field optional and no `unwrap`, and the poll is covered
  by fixture tests with the rows captured in §Verification.

Failure modes: router unreachable or `401` — logged through `LogThrottle`,
counted in `counters.routeros_poll_failures`, known mappings kept, pending
addresses stay on `default` and stay pending. One attempt per tick, no retry
inside a tick, so a dead router costs one bounded request per 20 s while
anything is pending, nothing once the pending set drains, and nothing on the
resolver path either way.

## Surfaces

| Surface | Change |
| --- | --- |
| `GET /api/v1/clients` | items gain `mac` (nullable) and `device_name` (user name, else lease name, else null); one item per address, unchanged otherwise |
| `GET /api/v1/devices` | new: one item per MAC with `name`, `lease_name`, `addresses[]`, summed `queries_24h`/`blocked_24h`, `policy` |
| `PUT /api/v1/devices/{mac}` | name a device; `PUT /api/v1/clients/{ip}` keeps working and writes through to the MAC when known |
| `DELETE /api/v1/devices/{mac}` | clears the user-assigned name: a device with no live address is removed at once, one with live addresses stays as an unnamed device (lease name only). `204`; `404` for an unknown MAC. The way out of §Device retention and the release in cap test C3 |
| Clients page | one row per device with its addresses folded under it, NAME from the device; the `seen_within` and family chips stay |
| tui-monitor | unchanged; its own lease lookup becomes redundant once FAH serves `device_name`, removable later |
| Docs | CONTEXT.md gains **Device** (a MAC-identified thing owning one or more Clients); ARCHITECTURE.md §Runtime Model lists the poll; SECURITY.md §Phase 3 "the only identity the container sees" is amended; CONFIGURATION.md `[routeros]`; API.md the three endpoints |

## Device retention

Two records, two lifetimes, **one setting**:

| Record | Lives while | Leaves when |
| --- | --- | --- |
| Address (client) | seen within `client_idle_expiry_days` | idle expiry on the policy tick, Fix 1 unchanged; a named *device* no longer shields its addresses |
| Device (MAC) | it carries a user-assigned name, **or** at least one live address | its last address expires and it has no user name — removed on the same tick, nothing to keep; or its name is cleared and it has no live address |

- A named device with zero addresses is retained **indefinitely**, on
  purpose: it is the phone away for a fortnight, the case this ADR exists
  for. Dropping it would recreate the lapse. It is bounded by the operator's
  own naming, not by traffic: the set is exactly the devices someone typed a
  name for, and `DELETE /api/v1/devices/{mac}` (or clearing the name) is the
  way out. The registry cap still applies to the whole map.
- A lease name (router `comment`/`host-name`) does not count as a name for
  retention: it is re-learned from the router on the next poll for free, so an
  unnamed device is rebuilt the moment one of its addresses is seen again.
- **No second retention setting.** Named devices are a finite, hand-curated
  list; unnamed devices vanish with their last address. Neither grows with
  traffic or uptime (hard rule 4), so nothing needs a knob. If a concrete need
  appears — say, a device renamed and forgotten for a year — the first
  response is the delete endpoint, not a timer.

### The cap: two maps, two bounds, admission never fails

| Map | Bound | Occupied by |
| --- | --- | --- |
| addresses (`clients`) | `DEFAULT_CAPACITY` = 4096, unchanged | one entry per observed IP; **a device with zero addresses occupies nothing here** |
| devices | live MACs, each backed by at least one address entry, so ≤ 4096; plus named zero-address devices, capped at **`MAX_NAMED_DEVICES` = 1024** | one entry per MAC |

- A new address is **always admitted**. At 4096 the eviction loop removes the
  least-recently-seen address whose device is unnamed first, then the
  least-recently-seen address of a named device — today's rule with "named"
  read off the device instead of the address. A named zero-address device is
  not in this map, so it can neither be evicted from it nor block an insert.
- Naming beyond 1024 named devices is refused with `409` and a message naming
  the cap; the operator frees one with `DELETE /api/v1/devices/{mac}`. 1024 is
  a hundredfold over this LAN and a hard number so the bound is a constant,
  not a policy. Reviewable.
- Memory at both caps: 4096 address records as today, plus ≤ 5120 device
  records of a MAC, two optional short strings and a small address set.
  Bounded by the two caps and small relative to the existing registry; the
  figure is measured, not estimated: `heap_bytes` extends to the device map
  and `GET /debug/memory` reports it on the device.

Cap tests, alongside the lifecycle tests:

| # | Scenario | Asserts |
| --- | --- | --- |
| C1 | 1024 named devices, every address expired; then 4200 new addresses recorded | 4096 addresses present, the newest ones; all 1024 named devices still present; no insert failed |
| C2 | address map full, every address belonging to a named device; one more address | the least-recently-seen address is evicted, its device remains (named); the new address is present |
| C3 | 1024 named devices; `PUT /api/v1/devices/{mac}` on a 1025th | `409`, nothing stored; after `DELETE` on one, the same `PUT` succeeds |
| C4 | address map full of unnamed-device addresses; one named device with one address; 4096 more addresses | the named device's address is the last to go and is evicted only once every other address is newer than it |

## Required lifecycle tests

The change is accepted only with these, in this order, each a real test with
its file named:

| # | Layer | Scenario | Asserts |
| --- | --- | --- | --- |
| L1 | `fah-stats` registry | device `M` learned from address `A1` at `t0`, named `"phone"`; `expire_idle(t0 + 8 d)` | `A1` gone; `M` still present with name `"phone"` and zero addresses; `names()` still yields `"phone"` for `M` |
| L2 | `fah-stats` registry | then `A2` recorded at `t0 + 8 d`, unnamed, no MAC; poll result maps `A2 → M` | `A2` listed under `M`; the view for `A2` shows `device_name = "phone"`; `mapped()` yields `(A2, M)` |
| L3 | `fah-rules` snapshot | assignments `Name("phone") → kids` and `Mac(M) → kids`; `active_at(now, named, mapped)` after L2 | `policy_for(A2) == kids` for each selector alone; `policy_for(A1) == default` |
| L4 | `fah-stats` snapshot | `save_snapshot` after L1, `boot` into a fresh `Stats` | `M` with name survives with zero addresses; `A1` absent |
| L5 | `fah-stats` registry | same as L1 with `M` **unnamed** | `M` removed together with `A1`; a later poll mapping `A2 → M` recreates `M` with the lease name only |
| L6 | `fah-api` | `PUT /clients/{A1}` name before rotation; after L2, `GET /clients?seen_within=24h` and `GET /devices/{M}` | the name is on `M`; `A2` carries it; `A1` is not listed; `GET /devices/{M}.addresses == [A2]` |
| L7 | dashboard | the clients page after L6 | one device row `"phone"` with `A2` under it, policy `kids` |

L1–L3 are the owner's stated scenario end to end: old address expires, device
remains, the rotated address maps to the same MAC, and both the name policy and
the MAC policy follow it.

Poll-planning and failure tests, against a fake `RouterOsSource` (a trait the
real REST client implements; the planner is pure and never touches the
network):

| # | Scenario | Asserts |
| --- | --- | --- |
| P1 | new IPv6 address `A` recorded; tick; the source returns `Err` | exactly one poll attempted; `A` still without MAC; `policy_for(A) == default`; `A` still pending; `counters.routeros_poll_failures == 1` |
| P2 | next tick after P1; the source returns the neighbour row `A → M` | exactly one poll attempted; `A` mapped to `M`; the snapshot published on this tick carries `A` under `M`'s assignments |
| P3 | 50 new addresses, both families, in one tick | exactly one poll, requesting all three tables; after it, none pending |
| P4 | IPv6-only pending | the poll requests `/ipv6/neighbor` only; IPv4-only pending requests `/ip/arp` and the leases only |
| P5 | a successful poll leaves `A` unmapped (absent from the tables) | `A` is no longer pending; the next tick issues **no** poll; the next full refresh does |
| P6 | nothing pending for 20 simulated minutes | polls issued: exactly 2, both full refreshes at 600 s and 1200 s; zero on every other tick |
| P7 | source hangs; `timeout_ms = 200` | the tick completes in about 200 ms; `refresh` still runs; the address stays pending |

## Verification on the device (2026-09-21)

Read-only: RouterOS CLI `print` over `ssh rb5009`, and REST `GET`s from the
dev box as the existing `monitor` user. Nothing on the RB5009 was changed.

| | |
| --- | --- |
| Device | RB5009UG+S+, RouterOS **7.21.5 (long-term)**, uptime 4 h at query time |
| `www-ssl` | port 8443, `address=192.168.10.0/24`, `certificate=router-localbox`; `www` and `api` services disabled |
| Groups | `monitor`: `policy=local,read,api,rest-api`, everything else `!`; the built-in `read` group is far wider (`reboot`, `sensitive`, `ssh`, …) and is not used |
| REST JSON | every value is a string (`"true"`, `"false"`, durations as `"7m39s"`); keys are hyphenated exactly as the CLI prints them; `.id` present |

### Fields relied on, as returned

| Endpoint | Keys used | Observed `status` values | Sample row (JSON, trimmed) |
| --- | --- | --- | --- |
| `/rest/ipv6/neighbor` | `address`, `mac-address`, `status`, `interface` | `permanent`, `reachable`, `stale`, `failed` | `{"address":"fd6c:7f32:8e91:0:e15e:5ead:96e5:9422","mac-address":"C8:7F:54:65:3F:DF","status":"reachable","interface":"BRIDGE"}` |
| `/rest/ip/arp` | `address`, `mac-address`, `status`, `complete`, `interface` | `reachable`, `stale`, `delay` | `{"address":"192.168.10.10","mac-address":"C8:7F:54:65:3F:DF","status":"reachable","complete":"true","dhcp":"false","interface":"BRIDGE"}` |
| `/rest/ip/dhcp-server/lease` | `address`, `mac-address`, `host-name`, `comment`, `status`, `dynamic` | `bound`, `waiting` | `{"address":"192.168.10.10","mac-address":"C8:7F:54:65:3F:DF","host-name":"bobdenaut","comment":"Liviu ASUS ROG","status":"bound","dynamic":"false"}` |

A `failed` neighbour row carries **no `mac-address` key at all**:
`{".id":"*1D","address":"2a02:2f04:520d:9100:9209:d0ff:fe11:2799","status":"failed",…}`.
The parser treats `mac-address` as optional and skips rows without one.
Leases list reservations while the device is offline (`status=waiting`,
`last-seen=1w4d…`), which is the source of a name for a device with no live
address.

### Temporary IPv6 addresses do map to a MAC

The neighbour table held 14 rows. Excluding link-local and the container's
own, the LAN rows and their devices:

| Address | MAC | Status | Kind |
| --- | --- | --- | --- |
| `fd6c:7f32:8e91:0:6663:c9f6:15a7:e924` | `78:ED:BC:44:AE:1D` | stale | RFC 4941 temporary: distinct random IID from the same MAC's other rows |
| `2a02:2f04:520d:9100:7310:1292:246:48d3` | `78:ED:BC:44:AE:1D` | reachable | temporary |
| `2a02:2f04:520d:9100:e65:a6a4:83b9:fef3` | `78:ED:BC:44:AE:1D` | stale | temporary |
| `fd6c:7f32:8e91:0:7f28:3e83:7561:526a` | `78:ED:BC:44:AE:1D` | permanent | temporary |
| `fd6c:7f32:8e91:0:e15e:5ead:96e5:9422` | `C8:7F:54:65:3F:DF` | reachable | stable random IID: the same IID appears under the GUA prefix |
| `2a02:2f04:520d:9100:e15e:5ead:96e5:9422` | `C8:7F:54:65:3F:DF` | reachable | stable |
| `fd6c:7f32:8e91:0:adaf:7476:41cf:4f55` | `C8:7F:54:65:3F:DF` | permanent | temporary |
| `fd6c:7f32:8e91:0:f286:20ff:fe8e:7c06` | `F0:86:20:8E:7C:06` | permanent | EUI-64, the TV |

`78:ED:BC:44:AE:1D` is `192.168.10.11` in ARP with no lease (static host);
`C8:7F:54:65:3F:DF` is lease `192.168.10.10`, comment `Liviu ASUS ROG`.

### Cross-check against what FAH actually sees

`GET /api/v1/clients?family=v6` on the deployed 0.4.1 held 729 items. Every
address seen in the last 2 h (`last_seen` after 09:31Z; queried ~11:32Z):

| FAH client | `last_seen` | `queries_24h` | In neighbour table | MAC |
| --- | --- | --- | --- | --- |
| `fd6c:7f32:8e91:0:6663:c9f6:15a7:e924` | 11:31:15Z | 1854 | yes, stale | `78:ED:BC:44:AE:1D` |
| `2a02:2f04:520d:9100:7310:1292:246:48d3` | 11:31:15Z | 1117 | yes, reachable | `78:ED:BC:44:AE:1D` |
| `2a02:2f04:520d:9100:e15e:5ead:96e5:9422` | 11:30:56Z | 1131 | yes, reachable | `C8:7F:54:65:3F:DF` |
| `fd6c:7f32:8e91:0:e15e:5ead:96e5:9422` | 11:30:23Z | 4719 | yes, reachable | `C8:7F:54:65:3F:DF` |

**4 of 4**, two devices, both temporary and stable addresses. Scope of the
claim: one snapshot, two active devices, an afternoon with 4 h router uptime.
Why it should hold in general: the router must resolve the client's MAC to
deliver the DNS reply, so a neighbour row exists whenever a query has just
transited, and the connector polls every 20 s while Linux keeps a neighbour
`stale` for minutes after its last packet. The `stale` row above is exactly
that state and still carries the MAC.

### The HTTPS proxy path

`/ipv6/firewall/nat` rule 10 (enabled) dst-nats `dst-port=443` from `BRIDGE`
to `[fd6c:7f32:8e91:1::2]:8444`, behind exemptions that already key on
`src-mac-address` (rules 8 and 9) — the router itself identifies these
clients by MAC. dst-nat rewrites the destination only, so the proxy's source
address is the client's own address, and the router resolves it to return
every reply packet, so the neighbour row exists for the life of the connection.

**Direct capture, 2026-09-21 ~11:45Z**, read-only. One IPv6 HTTPS download was
started from the dev box (`curl -6`, local address reported as
`2a02:2f04:520d:9100:e15e:5ead:96e5:9422`) and the two tables were read while
it ran:

```text
/ipv6/firewall/connection/print detail
  src-port=54634 dst-address=2a06:98c1:58::da dst-port=443
  reply-src-address=fd6c:7f32:8e91:1::2 reply-src-port=8444
  reply-dst-address=2a02:2f04:520d:9100:e15e:5ead:96e5:9422 reply-dst-port=54634
  tcp-state=established timeout=4m59s orig-packets=533 repl-bytes=3 403 148

/ipv6/neighbor/print detail where address=2a02:2f04:520d:9100:e15e:5ead:96e5:9422
  11 D  address=2a02:2f04:520d:9100:e15e:5ead:96e5:9422 interface=BRIDGE
        mac-address=C8:7F:54:65:3F:DF status="reachable"
```

The connection is steered to the proxy (`reply-src …:8444`), the address the
proxy sees as its client (`reply-dst-address`) is the client's own, and that
address maps to `C8:7F:54:65:3F:DF` in the same instant. This source is the
box's stable random-IID address; the temporary addresses of the other device
map the same way in the table above, and the mechanism does not distinguish
the two. FAH's `counters.http.pass` stood at 3628 since boot. Open item
closed.

## Costs

- Memory: 6 bytes per client record plus the device map, bounded by the two
  caps in §Device retention and small relative to the 4096-record registry;
  measured through `heap_bytes` and `GET /debug/memory`, not estimated here.
- CPU: table-sized JSON parses only when a poll runs — a handful a day plus
  one full refresh per 10 min; the registry walk is the same O(≤4096) Fix 1
  already pays.
- Runtime dependency: none new; `reqwest`/rustls are in the binary.
- Router: one user, one address. Full refreshes: 144 a day × 3 GETs =
  **432 GETs a day**, plus demand polls — on this LAN roughly 7–20 a day at
  1–3 GETs each — against 12 960 GETs a day for fixed 20 s polling. The
  ceiling stays one poll per tick while addresses are pending. Its memory log
  shows no per-request login lines for REST (0 lines for the `monitor` user in
  500, 2026-09-21).

## What would reverse it

- The container moved onto `BRIDGE` for another reason, and a probe container
  proving `RTM_GETNEIGH` works inside a RouterOS container — then the local
  table becomes the cheaper source and the REST client is the reversible part.
- A second router vendor on a deployment this project targets.

## Implementation order, after review

1. `fah-model`: `MacAddr`, `ClientSelector::Mac`, `matches` signature.
2. `fah-stats`: IP → MAC and MAC → name in the registry, device grouping,
   expiry exemption moved to the device, snapshot format extended.
3. `fah-rules`: `Mac` and MAC-keyed `Name` resolution in `active_at`;
   `parse_selector`; `validate_assignment`.
4. Binary: `[routeros]` config, the `RouterOsSource` trait and its REST
   client, the pure poll planner (pending set, family selection, full-refresh
   clock, one poll per tick) on the policy tick, the counter, tests P1–P7 with
   a fake source and the fixture rows from §Verification.
5. `fah-api` + dashboard: the surfaces above.
6. Docs listed under Surfaces, and the deploy runbook's proposed router
   commands.

Each step ships behind `url = ""`: until the connector is configured, every
device has no MAC and the system behaves exactly as at `6350fc1`.
