# IPv6 privacy-address rotation vs address-exact client identity

Non-task investigation, captured during the 0.3.1 re-soak. Read-only; no code
changed, nothing deployed, the RB5009 was not modified.

## Summary

Every per-device control FastAdHunter offers is keyed to an exact IP address —
`ClientSelector::Ip`, `PUT /api/v1/clients/{ip}/policy`, `PUT /api/v1/clients/{ip}`.
On this LAN 6–7 devices run RFC 4941 privacy extensions and replace their IPv6
address about once a day. A policy or name set on such an address stops applying
roughly 24 h later, silently, and the device falls back to `default`.

The mechanism is confirmed in code and the rotation is confirmed in live data.
**No lapse has occurred yet**: no policy exists (`active_assignments: 0`), and the
only named IPv6 client sits on a stable EUI-64 address. The fault is latent and
fires on the first policy assigned to a phone or tablet — the parental-control
case the feature exists for.

## Decisions

- `ClientSelector::Network` is **rejected** as a device identity here. The ULA
  `/64` holds every device in the house; the GUA `/64` survives ~1 day.
- **No `/64` fallback** in `PolicySet::resolve` when no exact address matches — it
  would silently widen a one-device policy to the whole LAN, the inverse of the
  failure the schedule-fallback rule at `fah-rules/src/policy.rs:229-238` avoids.
- Correction to the opening premise: subnet assignment is **not** missing from the
  dashboard (see Finding 4).
- Nothing built. Direction is the owner's call (see Remaining TODOs).

## Findings

### 1. HIGH (latent) — address-exact identity lapses silently on rotation

| | |
| --- | --- |
| Applies to | any client using RFC 4941 temporary addresses |
| Live impact today | none — 0 assignments configured, only v6 name is EUI-64 |
| Fires when | a policy or name is set on a rotating address |
| Time to failure | ~24 h (`TEMP_PREFERRED_LIFETIME`) |
| Signal to operator | none |

Chain:

| Step | Location |
| --- | --- |
| Exact equality, no prefix or identity fallback | `fah-model/src/policy.rs:131` |
| First match wins, else `PolicyId::DEFAULT` | `fah-rules/src/policy.rs:285` |
| `Name` resolved to addresses at snapshot build, from the registry's named set | `fah-rules/src/policy.rs:259-262` |
| Rotated address is unnamed on arrival, so it is not in that set | `fah-stats/src/client_registry.rs:115` |
| `assignment` merely goes absent, no lapse field | `fah-api/src/wire.rs:864-867` |

Naming the device does not survive rotation either — a name is itself keyed to one
address.

### 2. MEDIUM — registry eviction protects the wrong entry

`client_registry.rs:81-85` evicts the least-recently-seen **unnamed** client first,
sparing named devices. Correct against a spoof flood. Against rotation it preserves
yesterday's dead address and lets today's live one compete for eviction as an
unnamed entry.

### 3. LOW — registry is 89% dead addresses

434 of 486 entries not seen in 48 h. Bounded and self-correcting: ~13 new v6
addresses/day against the 4096 cap reaches full around **May 2027**, and
unnamed-first eviction then removes exactly the right entries. No action.

### 4. INFO — subnet assignment already exists in the dashboard

| Path | Selector supported |
| --- | --- |
| Policies page → policy dialog, free-text row | address, CIDR, name |
| `POST` / `PATCH /api/v1/policies` (`AssignmentResponse.client: String`) | address, CIDR, name |
| Clients page → assign dialog | address only |
| `PUT /api/v1/clients/{ip}/policy` | address only |

`parse_selector` (`fah-rules/src/policy.rs:460`) and `validate_assignment`
(`fah-config/src/lib.rs:344`) both accept CIDR, and `policy-dialog.tsx:363`
documents `192.168.20.0/24` in the field help. No TOML editing is required today.
This does not help — see Decisions.

**Verdict: PASS WITH DEFERRED FINDINGS.** No active breakage, no code changed.
Finding 1 is a design limit awaiting an owner decision, not a regression.

## Measurements

Device: RB5009, FastAdHunter 0.3.1, during the 7-day 0.3.1 soak.
Corpus: `GET /api/v1/clients` (486 items, 96,370 bytes) and `GET /api/v1/policies`,
pulled 2026-09-01T~09:05Z. Read-only, no `history/perf`. Corroborated against
[phase2/soak-0.2.10/api-v1-clients.json](../phase2/soak-0.2.10/api-v1-clients.json)
(2026-08-02, 72 items), which shows the same signature at 1/6 the scale.

### Registry composition

| Metric | Value |
| --- | --- |
| Clients total | 486 |
| IPv6 / IPv4 | 470 / 16 |
| Not seen in 48 h | 434 (89%) |
| Oldest `first_seen` | 2026-07-27T11:35Z (37 days) |
| Distinct IPv6 `/64` prefixes | 34 |
| Policies configured | 0 (`active_assignments: 0`) |

### ULA prefix `fd6c:7f32:8e91:0::/64` — the LAN

| Metric | Value |
| --- | --- |
| Addresses | 240 |
| Random IID / EUI-64 | 236 / 4 |
| Median active span | 10.7 h |
| Span under 26 h | 222 of 240 |
| New addresses per day | 6.5, steady across all 37 days |
| Active in last 24 h | 14 |
| Implied privacy-address devices | 6–7 (6.5 rotations/day ÷ ~1 per device per day) |

### The two populations do not overlap

All 14 ULA addresses active in the last 24 h:

| `queries_24h` | span (h) | `first_seen` | address |
| --- | --- | --- | --- |
| 25125 | 6.9 | 2026-08-31T07:15 | `fd6c:7f32:8e91:0:7d86:ff1e:3d2f:fd2d` |
| 24917 | 12.0 | 2026-08-31T14:58 | `fd6c:7f32:8e91:0:3178:c3c0:408b:a370` |
| 23080 | 19.4 | 2026-08-31T04:59 | `fd6c:7f32:8e91:0:787c:196b:306f:5b26` |
| 3363 | 3.2 | 2026-09-01T05:49 | `fd6c:7f32:8e91:0:c126:aaa6:8cfe:630f` |
| 2200 | 10.7 | 2026-08-31T04:51 | `fd6c:7f32:8e91:0:44c1:fa60:4e97:404c` |
| 1829 | 8.5 | 2026-09-01T00:24 | `fd6c:7f32:8e91:0:dcfa:cb86:c1e8:5e26` |
| 1680 | 8.6 | 2026-08-31T06:59 | `fd6c:7f32:8e91:0:2562:b354:3718:4225` |
| 1326 | 8.5 | 2026-08-31T15:53 | `fd6c:7f32:8e91:0:276b:6283:e9b3:5644` |
| 1150 | 8.4 | 2026-08-31T15:53 | `fd6c:7f32:8e91:0:873a:ba71:aaab:388` |
| 1068 | 11.7 | 2026-08-31T07:40 | `fd6c:7f32:8e91:0:296d:e4ef:b592:772a` |
| 677 | 0.1 | 2026-09-01T04:13 | `fd6c:7f32:8e91:0:e97e:6761:9cdb:5c10` |
| 485 | 3.8 | 2026-09-01T00:25 | `fd6c:7f32:8e91:0:51f4:5953:9096:9da4` |
| 137 | 11.8 | 2026-08-31T07:34 | `fd6c:7f32:8e91:0:ace9:e895:207a:6c04` |
| 123 | **840.6** | 2026-07-27T20:49 | `fd6c:7f32:8e91:0:f286:20ff:fe8e:7c06` (EUI-64) |

Thirteen random-IID addresses, spans 0.1–19.4 h, every one first seen inside 28 h.
One EUI-64 address, span 35 days. Two populations, no overlap — RFC 4941 beside
stable SLAAC in one snapshot.

### GUA prefix churn — the ISP delegation rotates too

| Metric | Value |
| --- | --- |
| Distinct `2a02:2f04:*::/64` prefixes in 37 days | 33 |
| Implied delegation lifetime | ~1.1 days |
| Prefixes with traffic in last 24 h | 1 (`2a02:2f04:5000:a400::/64`, 13 addresses) |
| Largest prefix | 27 addresses (`2a02:2f04:5304:cb00::/64`) |

A GUA `/64` assignment therefore expires about as fast as the address it was meant
to outlive.

### Named clients and their exposure

| Name | Address | Rotates? |
| --- | --- | --- |
| Deco M5 parter | `192.168.10.12` | no — IPv4 |
| Deco M5 etaj | `192.168.10.13` | no — IPv4 |
| Comunicator centrala | `192.168.10.15` | no — IPv4 |
| TV LG WebOS (ipv4) | `192.168.10.16` | no — IPv4 |
| CONTAINER interface | `172.17.0.1` | no — docker |
| TV LG WebOS (ipv6) | `fd6c:7f32:8e91:0:f286:20ff:fe8e:7c06` | no — EUI-64, stable 840 h |

Zero named clients at risk today. The TV shows per-device control works fine on a
device that keeps a stable address.

## Files inspected

No files changed.

| File | Why |
| --- | --- |
| `crates/fah-model/src/policy.rs` | `ClientSelector::matches`, `specificity` |
| `crates/fah-rules/src/policy.rs` | `PolicySet::resolve`, `ActivePolicies::policy_for`, `parse_selector` |
| `crates/fah-config/src/lib.rs` | `validate_assignment` |
| `crates/fah-config/src/schema/policy.rs` | `AssignmentConfig` |
| `crates/fah-stats/src/client_registry.rs` | capacity, name-aware eviction |
| `crates/fah-api/src/routes.rs`, `wire.rs` | client and policy endpoints |
| `dashboard/frontend/src/pages/policies/policy-dialog.tsx` | assignment editor |
| `dashboard/frontend/src/pages/clients/assign-dialog.tsx` | per-client assignment |

## Remaining TODOs

Owner decision required. Ranked cheapest first; none started.

| # | Option | Cost | Fixes the lapse? |
| --- | --- | --- | --- |
| 1 | Dashboard flags a name or assignment whose address has not been seen in N hours. Uses existing `last_seen`; no backend change, no hot-path cost. | small, dashboard only | no — removes the silence, not the lapse |
| 2 | Give the controlled device a stable identity (reserved address, or privacy extensions off on that device). Router-side; commands to be proposed, never applied here. | zero code | yes, per device, by operator action |
| 3 | Identity by MAC/DUID instead of source IP. Needs neighbour-table lookup; contradicts [CONTEXT.md §Client](../../../CONTEXT.md); drags `ClientSelector::Name` and `$client=` rules onto the new identity. Off the hot path — registry and snapshot rebuild only. | large, needs an ADR first | yes |

Option 1 is worth shipping whichever of 2 or 3 is chosen. Rejected: `/64`
assignment, and a `/64` fallback in `resolve` — see Decisions.

## Solution (2026-09-07)

Architecture pass over the findings above. Read-only; nothing changed, nothing
deployed. Two separate problems, three fixes.

### Additional evidence

| Fact | Source |
| --- | --- |
| RA on `BRIDGE` carries `advertise-dns=yes dns=fd6c:7f32:8e91:1::2`; `other-configuration=no`; no `/ipv6/dhcp-server`, no options | `/ipv6/nd/print detail`, `/ipv6/dhcp-server/print`, 2026-09-07 |
| RDNSS is therefore the **only** IPv6 DNS source; every dual-stack device sends DNS over IPv6 from an RFC 4941 temporary address | above + [routeros-traps.md](../../routeros-traps.md) §IPv6 |
| ISP `/56` lease shows `never`, yet 33 GUA `/64` in 37 days: re-delegated on reconnect. ULA is the only stable v6 prefix | `/ipv6/dhcp-client/print` + §GUA prefix churn |
| Container sits on `veth1` in the `CONTAINERS` bridge, routed from `BRIDGE`. Its neighbour table holds the gateway only | [routeros-traps.md](../../routeros-traps.md) §Container network |
| Registry: cap 4096, no time expiry, no delete endpoint, persisted in the stats snapshot; `GET /api/v1/clients` filters on `family` only | `client_registry.rs:16,79-105`, `routes.rs:58-81,388` |
| Growth ~19 addresses/day: 486 on 2026-09-01, ~600 on 2026-09-07 | dashboard |

**Correction to Option 3 above:** "neighbour-table lookup" returns nothing on this
deployment. The container is not L2-adjacent to LAN clients. See Fix 3.

### Fix 1 — registry bloat (product, small, ship regardless)

| Item | Where | Hot path | Memory |
| --- | --- | --- | --- |
| Idle expiry of **unnamed** entries, compiled-in default 7 d, run on the 20 s policy ticker (`main.rs:812`) or the snapshot tick | `fah-stats` registry + binary wiring | none | shrinks |
| `GET /api/v1/clients?seen_within=<duration>`; dashboard default 24 h with a "show all" toggle | `fah-api` routes/wire, `clients` page | none | none |
| Option 1 above: flag a name or assignment whose address is unseen for N h | dashboard only | none | none |

Makes Finding 2 moot: dead addresses leave by age, not by cap. Named entries never
expire. Until this ships the ~580 stale v6 rows stay listed even after Fix 2.

### Fix 2 — identity lapse on this LAN, now (router, zero code)

Stop advertising IPv6 DNS. Clients fall back to the DHCPv4 DNS server, so every
query arrives from a stable IPv4 address (reservations already cover the named
devices). AAAA still resolves, IPv6 connectivity is untouched, filtering coverage
is unchanged because verdicts are by name, not transport. The two `/ipv6/firewall/nat`
`:53` dstnat rules stay as the safety net for hard-coded v6 resolvers.

Proposed command, to be run by the owner, not by an agent:

```routeros
/ipv6/nd/set [find interface=BRIDGE] advertise-dns=no
```

- What it does: drops the RDNSS option from Router Advertisements on `BRIDGE`.
  `dns=` stays stored but unused.
- When it takes effect: next RA, within `ra-interval` (30 s–2 m). Clients keep
  the learned server until the RDNSS lifetime expires (expected ≤ `ra-lifetime`,
  10 m); stragglers drop it on Wi-Fi reconnect.
- Revert: `/ipv6/nd/set [find interface=BRIDGE] advertise-dns=yes`.
- Verify (read-only): `/ipv6/nd/print detail` shows `advertise-dns=no`; on a
  Windows client `ipconfig /all` lists only `192.168.10.x` under DNS Servers;
  `GET /api/v1/clients?family=ipv6` shows no `last_seen` newer than the change
  after ~15 min, save for hard-coded v6 resolvers caught by dstnat.

Trade-offs: IPv6-only hosts lose DNS (none on this LAN). Per-operator choice, not
a product fix. Does not help Phase 3/4: a proxied TCP connection still carries a
temporary GUA as source.

### Fix 3 — durable identity (product, large, ADR first, later phase)

Identity = MAC. `ClientSelector::Mac`, names keyed by MAC, MAC resolved to its
current IP set at snapshot build, the same pattern `Name` uses today
(`fah-rules/src/policy.rs:353`). Hot path untouched.

Two ways to obtain the MAC. Owner picks one; **B is recommended**.

| | A — RouterOS REST connector | B — local neighbour table |
| --- | --- | --- |
| Container placement | stays on `CONTAINERS` (routed) | moved onto `BRIDGE` (same L2 as clients) |
| MAC source | poll `/rest/ip/arp` + `/rest/ipv6/neighbor` with a read-only RouterOS user | kernel neighbour table of the container's own namespace, read via rtnetlink `RTM_GETNEIGH` (what `ip neigh` shows) |
| Product cost | HTTP client in the binary, credentials in config, MikroTik-specific | netlink dump parsing, no credentials, works on any Linux host |
| Router cost | enable REST, create user | one-time re-plumb (below) |
| Precedent | none | Pi-hole, AdGuard Home |

Way B router work, one-time, owner-run, none of it proposed as commands yet:

- `veth1` becomes a port of `BRIDGE`; container gets an address in
  `192.168.10.0/24` plus the LAN ULA `fd6c:7f32:8e91::/64`.
- Repoint: DHCPv4 network `dns=`, RA `dns=`, both `/ipv6/firewall/nat` `:53`
  dstnat targets, the `/ip/dns` watchdog script, every firewall rule naming
  `172.17.0.2`. Docs: [routeros-traps.md](../../routeros-traps.md),
  [deploy-rb5009.md](../../deploy-rb5009.md).
- Trade-off: LAN reaches the container at L2, the router firewall no longer sits
  between them. Ports 53 and 8443 are LAN-facing already, so the loss is small.

Way B product work:

| Item | Hot path |
| --- | --- |
| Background task in the binary: neighbour dump on the 20 s policy ticker, or on demand when the registry first sees an IP | none |
| Registry stores IP → MAC once learned. Kernel garbage-collects idle entries after 60 s once the table exceeds 128, so the registry remembers, never re-reads | none |
| `ClientSelector::Mac`; names keyed by MAC; snapshot build resolves MAC → live IP set, same shape as `Name` today | none |
| Applies to IPv4 too: one row per device on the clients page | none |
| Policy gap per rotation: first reply + one poll, ≤ 20 s on `default` | — |

Caveats:

- Unverified: RouterOS containers permitting `NETLINK_ROUTE` reads. Test is a
  probe container on `veth3` running `ip neigh`; starting it is owner-run.
- Phone MAC randomisation is stable per SSID by default. iOS 18 offers a
  "rotating" private address; controlled devices must use "fixed".

Changes [CONTEXT.md §Client](../../../CONTEXT.md). Required before per-client
HTTPS interception opt-in ([SECURITY.md](../../../SECURITY.md)): an opt-in keyed
to a rotating address is a worse failure than a lapsed policy.

### Decisions held and added

- `/64` selector and `/64` fallback in `resolve` stay rejected.
- No interface-identifier correlation: temporary IIDs are random, RFC 7217 stable
  IIDs differ per prefix. Dead end.
- Fix 1 and Fix 2 are independent; Fix 2 is the owner's call and needs no code.
