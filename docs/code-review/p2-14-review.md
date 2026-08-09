# p2-14 — IPv6 HTTP Interception

**Task:** [`plan/wip/phase2/p2-14-ipv6-http-interception.md`](../../plan/wip/phase2/p2-14-ipv6-http-interception.md)
· **Closed on-device 2026-08-09** on `0.2.14` · **Zero code changed**

## Summary

Phase 2's last functional gap: a LAN client reaching a dual-stack origin over
IPv6 bypassed the proxy entirely. The task was framed as a hypothesis — the
listener is already dual-stack (`address = "::"`, `IPV6_V6ONLY` off), so the
whole gap should be RouterOS rules and no code. **The hypothesis held.**

Four rules closed it. The acceptance table's first row flipped, per-client
scoping works over IPv6, and the IPv4 path is untouched.

## Decisions

- **The delegated prefix is never hardcoded.** `p2-08`'s draft named a `/56`
  that had rotated three times since; the rules take it from an address list the
  DHCPv6 client maintains.
- `in-interface-list=LAN` rather than the draft's `in-interface=BRIDGE`, which
  is narrower than the live IPv4 rule and leaves `CONTAINERS` uncovered.
- **The rotation-lag criterion is carried, not met.** Its failure mode is a
  LAN-to-LAN request briefly proxied, not a loss of filtering, so it does not
  block closure.

## Bugs found

None. Two things that looked like defects and were not:

1. A first test rule, `||httpforever.com^*/blocked-test`, did not fire. The
   syntax was wrong, not the matcher: `^` already consumes the separator, so the
   pattern demanded two slashes. `||httpforever.com/blocked-a` blocks correctly.
2. `/log print` looking empty while the disk log was 65 KiB — `/file get …
   contents` returns an empty string above ~64 KiB with no error. Recorded in
   [`routeros-traps.md`](../routeros-traps.md).

## Measurements

### The acceptance table

Measured in `0.2.10-soak-baseline.md` §Known gap, re-run 2026-08-09:

| Request | Connected to | Reached the proxy |
| --- | --- | --- |
| `httpforever.com` (default) | `2606:4700:3031::6815:4d2` | **yes** (was **no**) |
| `httpforever.com` forced `-4` | `104.21.4.210` | yes |
| `http.badssl.com` (IPv4-only) | `104.154.89.105` | yes |

Confirmed by `counters.http.pass`, which moved by exactly the number of requests
made.

### Blocking over IPv6

A URL-tier rule in `user-rules`, both anchor forms:

| Rule | Result |
| --- | --- |
| `\|\|httpforever.com/blocked-a` (domain anchor) | **403**, 630 B |
| `/blocked-b` (anywhere anchor) | **403**, 613 B |
| origin's own response for a missing path | 404, 162 B |

`counters.http.block` 0 → 2, `pass` unchanged. The block response is FAH's, not
the origin's.

### Per-client scoping — the strongest evidence

One machine, one URL, one moment. Only the client's address family differs:

| Path | Client in `$client=<lan>/64` | Result |
| --- | --- | --- |
| IPv6 | yes | **403**, 630 B — blocked |
| IPv4 | no | **404**, 162 B — passed |

Client identity is therefore resolved from the real peer address, IPv6 addresses
canonicalise correctly, and they match a CIDR term. Coincidence is excluded by
construction: nothing else about the two requests differs.

### LAN-to-LAN is skipped, and provably so

Per-rule counters, `/ipv6/firewall/nat/print stats`:

| # | Rule | Packets |
| --- | --- | ---: |
| 3 | skip ULA / link-local | 0 |
| 4 | **skip delegated prefix** | **5** |
| 5 | **dst-nat to the proxy** | **43** |

The skip rule is **actively firing**, not merely unexercised — which an absent
HTTP response could not have distinguished.

### The dynamic list survives a rotation

`fah-lan6` held `2a02:2f04:5300:3500::/56`, matching the pool exactly, and
re-populated itself at the 13:31 delegation change without intervention.

### Side effect worth recording

Three `PUT /rules/user` calls recompiled the full ruleset and raised
`process_peak_rss` **117.73 → 173.63 MiB** — close to the ~180 MB `p2-12`
attributed to a refresh compile. The step landed in `/history/perf` at
21:39:55Z, giving `p2-13`'s monotonicity criterion its first real empirical
point: a 2.85 s event captured by a 360 s sampler.

## Files changed

None. Four RouterOS rules, applied by the owner:

```routeros
/ipv6/dhcp-client set [find interface=DIGI] prefix-address-lists=fah-lan6
/ipv6/firewall/address-list/add list=fah-http-skip6 address=fd6c:7f32:8e91::/48
/ipv6/firewall/address-list/add list=fah-http-skip6 address=fe80::/10
/ipv6/firewall/address-list/add list=fah-http-skip6 address=::1/128
# two dstnat accepts (fah-http-skip6, fah-lan6), then the dst-nat to :8080
```

`user-rules` was `["||bing.com^"]` before the test rules and is restored to it.

## Remaining TODOs

- **The rotation lag is unmeasured.** How long `fah-lan6` trails a live
  delegation change decides whether LAN-to-LAN v6 `:80` is briefly proxied. If
  it is not small, the second skip wants `2000::/3` arriving on a LAN interface
  instead of the exact delegation.
- HTTPS over IPv6 is Phase 3's, and inherits what this task settled about client
  identity.
