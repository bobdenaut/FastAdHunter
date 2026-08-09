# P2-14 — IPv6 HTTP Interception

**Phase:** 2 · **Depends on:** [`p2-08`](p2-08-phase2-verification.md) (which
found the gap and wrote the first draft of the rules) · **Model:** Opus

## Goal

Close Phase 2's last functional gap: a LAN client reaching a dual-stack origin
over IPv6 bypasses the proxy entirely.

**This is expected to be a verification task, not a code task.** The listener is
already dual-stack — `[http.listen] address = "::"`, one socket with
`IPV6_V6ONLY` off — and the missing piece is RouterOS rules. But the v6 path has
never been exercised end to end, so "no code needed" is the hypothesis under
test, not a finding to assume.

## Blocked — the upstream has no working IPv6

**Do not start this task until global IPv6 works from the router.** Verified
2026-08-09 on the RB5009:

```text
/ping 2606:4700:4700::1111 count=4   →   sent=4 received=0 packet-loss=100%
```

That is the router itself, not a client, and it holds an active default route
`::/0 → DIGI` plus a global WAN address. Packets leave and nothing returns —
upstream routing, acknowledged by the ISP.

Until it is fixed, the acceptance table below **cannot distinguish "the proxy
was bypassed" from "IPv6 does not work"**, so a run of it proves nothing.

### Preconditions to check before starting

1. `/ping 2606:4700:4700::1111 count=4` from the router succeeds.
2. A LAN client has a global address from the **currently delegated** prefix and
   can reach a global v6 destination.
3. `/ipv6/pool/print` and `/ipv6/address/print` agree on the prefix, and the
   BRIDGE address is `G`, not `I`.

## What is already known

Measured in `docs/code-review/0.2.10-soak-baseline.md` §Known gap, when IPv6 was
still working:

| Request | Connected to | Reached the proxy |
| --- | --- | --- |
| `httpforever.com` (default) | `2606:4700:3031::6815:4d2` | **no** |
| `httpforever.com` forced `-4` | `172.67.132.115` | yes |
| `http.badssl.com` (IPv4-only host) | `104.154.89.105` | yes |

DNS already has IPv6 coverage (two `/ipv6/firewall/nat` dstnat rules forcing all
IPv6 `:53`), so the gap is specific to HTTP.

**The container does not need global IPv6.** `resolve_host` returns A before
AAAA and the proxy takes the first address passing the egress check, so clients
reach the proxy over v6 while the proxy reaches origins over v4.

## The delegated prefix is not stable — do not hardcode it

Four distinct `/56` delegations observed inside a week, one of them a change
during a single working session:

| When | Delegated prefix |
| --- | --- |
| `0.2.10-soak-baseline.md`'s draft rules | `2a02:2f04:5100:e700` |
| Previously pinned in the BRIDGE address | `2a02:2f04:5303:6800` |
| 2026-08-09, early | `2a02:2f04:520a:3d00` |
| 2026-08-09, minutes later | `2a02:2f04:540c:7900` |

**`p2-08`'s draft skip rule names the first of those**, so applying it verbatim
skips a prefix that no longer exists.

This fails dangerously rather than harmlessly: a skip rule whose address no
longer matches stops accepting, and the dst-nat rule below it then redirects
LAN-to-LAN v6 HTTP into the proxy. The rules below therefore take the delegated
prefix from a **dynamically maintained address list**, never a literal.

The BRIDGE address is already rotation-proof: it is configured as `::1/64`
`from-pool=ipv6-pool`, so only the offset is pinned and the prefix follows the
pool. It tracked two rotations without going invalid.

## Work

1. **The owner applies the rules below.** **Propose, do not run** (root
   CLAUDE.md §The router is off limits).
2. Re-run the three-request table and require the first row to flip to **yes**,
   with rows 2 and 3 unchanged.
3. Confirm the request appears as `kind=http` on the events socket with the
   client identified by its **v6** address.
4. Only if 2 or 3 fails: find and fix the code path. Candidates, in order —
   client-identity canonicalisation for a v6 peer, the egress guard's judgement
   of a v6 resolved address, and `Host`-header port stripping for a bracketed v6
   literal.

### Rules to apply

Have the delegated prefix maintain its own address list, so a rotation cannot
stale the skip rule:

```routeros
/ipv6/dhcp-client set [find interface=DIGI] prefix-address-lists=fah-lan6
```

Static locals, then the two skips, then the redirect. `in-interface-list=LAN`
matches the IPv4 rule already live (`/ip/firewall/nat` index 10); `p2-08`'s
draft used `in-interface=BRIDGE`, which is narrower than the v4 rule and would
leave `CONTAINERS` uncovered:

```routeros
/ipv6/firewall/address-list/add list=fah-http-skip6 address=fd6c:7f32:8e91::/48
/ipv6/firewall/address-list/add list=fah-http-skip6 address=fe80::/10
/ipv6/firewall/address-list/add list=fah-http-skip6 address=::1/128

/ipv6/firewall/nat/add chain=dstnat action=accept protocol=tcp dst-port=80 \
  dst-address-list=fah-http-skip6 \
  comment="fastadhunter http v6: leave local traffic alone"

/ipv6/firewall/nat/add chain=dstnat action=accept protocol=tcp dst-port=80 \
  dst-address-list=fah-lan6 \
  comment="fastadhunter http v6: leave the delegated LAN prefix alone"

/ipv6/firewall/nat/add chain=dstnat action=dst-nat protocol=tcp dst-port=80 \
  in-interface-list=LAN dst-address=!fd6c:7f32:8e91:1::2/128 \
  to-address=fd6c:7f32:8e91:1::2/128 to-ports=8080 \
  comment="fastadhunter http v6"
```

`to-address` is `veth1`'s container address, confirmed against
`/interface/veth/print`.

## Criteria

- [ ] A dual-stack origin reached over IPv6 is proxied, and a blocked URL over
      IPv6 returns the type-aware block response rather than the origin's body.
- [ ] Per-client policy applies over IPv6 — a `$client`-scoped block fires for a
      client identified by its v6 address, not just its v4 one.
- [ ] The IPv4 path is unchanged: rows 2 and 3 of the table still reach the
      proxy.
- [ ] `fah-lan6` is populated by the DHCPv6 client and **follows a rotation**.
      Check it before and after the prefix next changes; a static-looking entry
      means `prefix-address-lists` did not take and the skip is one rotation
      away from failing.
- [ ] LAN-to-LAN v6 HTTP is not redirected — the router's own `:80` and one
      client-to-client request stay off the proxy.
- [ ] Gates green if any code changed; nothing to run if none did.

## Sequencing

Two independent gates, both of which must clear:

1. **DIGI's IPv6 is fixed** — see the preconditions above.
2. **The 0.2.13 soak has ended.** Applying the rules starts routing v6 HTTP into
   the proxy and changes the HTTP arm's traffic while it is being measured.

Gate 2 shares a window with the `p2-13` deploy.

## Out of scope

- IPv6 egress from the container to origins. The proxy reaches origins over v4
  by design and that is not a gap this task closes.
- The ISP's prefix-delegation behaviour. The infinite (`never`) lifetimes on a
  rotating prefix are a DIGI-side defect; this task only has to survive it.
- HTTPS over IPv6 — Phase 3 owns the interception path, and it inherits whatever
  this task settles about client identity.
