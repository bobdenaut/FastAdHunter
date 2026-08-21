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

Measured in `docs/code-review/phase2/0.2.10-soak-baseline.md` §Known gap, when IPv6 was
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

Five distinct `/56` delegations observed inside a week, three of them inside one
working session:

| When | Delegated prefix |
| --- | --- |
| `0.2.10-soak-baseline.md`'s draft rules | `2a02:2f04:5100:e700` |
| Previously pinned in the BRIDGE address | `2a02:2f04:5303:6800` |
| 2026-08-09 11:0x | `2a02:2f04:520a:3d00` |
| 2026-08-09 11:09 | `2a02:2f04:540c:7900` |
| 2026-08-09 11:36 | `2a02:2f04:5407:c600` |

**The mechanism is a PPPoE redial, not a DHCPv6 lease policy.** At the 11:36
rotation the WAN IPv4 changed at 11:36:21 and the IPv6 address and delegation at
11:36:25 — IPCP first on the fresh PPP session, DHCPv6 four seconds behind it.
Session uptime read `2m27s` shortly after. So all three values turn over
together, on every redial, and the `never` (infinite) DHCPv6 lifetimes are not
contradictory: the lease is not expiring, the session under it is.

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

Four stages with a checkpoint between 2 and 3. **The order is mandatory, not
stylistic** — see the checkpoint for what applying step 4 early does.

**Steps 1–3 are applied. Step 4 is not**, and waits on both gates in
§Sequencing.

#### Step 1 — the delegated prefix maintains its own list

```routeros
/ipv6/dhcp-client set [find interface=DIGI] prefix-address-lists=fah-lan6
```

#### Step 2 — checkpoint

```routeros
/ipv6/firewall/address-list/print where list="fah-lan6"
```

Must show a `D` (dynamic) entry carrying the **current** delegation. RouterOS
populates it on the next DHCPv6 renewal, so it can lag; a rebind forces it, and
is safe while global v6 is already down because v6 DNS interception rides on the
ULA `fd6c:…`, which a rebind does not touch:

```routeros
/ipv6/dhcp-client release [find interface=DIGI]
```

**Do not run step 4 while that list is empty.** An empty list means
`dst-address-list=fah-lan6` matches nothing, the second skip never fires, and the
redirect below it sends **LAN-to-LAN IPv6 `:80` into the proxy**.

#### Step 3 — static locals

```routeros
/ipv6/firewall/address-list/add list=fah-http-skip6 address=fd6c:7f32:8e91::/48
/ipv6/firewall/address-list/add list=fah-http-skip6 address=fe80::/10
/ipv6/firewall/address-list/add list=fah-http-skip6 address=::1/128
```

#### Step 4 — the NAT rules, in this order

`in-interface-list=LAN` matches the IPv4 rule already live (`/ip/firewall/nat`
index 10); `p2-08`'s draft used `in-interface=BRIDGE`, which is narrower and
leaves `CONTAINERS` uncovered.

```routeros
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

They append after the two DNS dstnat rules, which is correct — different port,
no interaction. Effective immediately. `to-address` is `veth1`'s container
address, confirmed against `/interface/veth/print`.

#### Rollback

```routeros
/ipv6/firewall/nat/remove [find comment~"fastadhunter http v6"]
```

## Criteria

**All met 2026-08-09, with zero code changed** — the hypothesis held. Evidence in
[`p2-14-review.md`](../../../docs/code-review/phase2/p2-14-review.md).

- [x] A dual-stack origin reached over IPv6 is proxied, and a blocked URL over
      IPv6 returns the type-aware block response rather than the origin's body.
      `httpforever.com` over `2606:4700:3031::6815:4d2` reached the proxy
      (`http.pass` +2); a blocked path returned **403 / 630 B** against the
      origin's own 404 / 162 B, for both the domain and the anywhere anchor.
- [x] Per-client policy applies over IPv6 — a `$client`-scoped block fires for a
      client identified by its v6 address, not just its v4 one. **The strongest
      test in the set**: one machine, one URL, one moment, only the address
      family differing — `$client=<lan>/64` gave **403 over IPv6 and 404 over
      IPv4**. Coincidence is excluded.
- [x] The IPv4 path is unchanged: rows 2 and 3 of the table still reach the
      proxy.
- [x] `fah-lan6` is populated by the DHCPv6 client and **follows a rotation**.
      It re-populated itself at the 13:31 delegation change without intervention.
- [x] LAN-to-LAN v6 HTTP is not redirected. Proven by per-rule counters rather
      than by an absent response: the skip rule shows **5 packets accepted**
      while the redirect shows 43 — so the skip is actively firing, not merely
      unexercised.
- [ ] **The rotation gap is bounded.** A redial gives clients the new prefix at
      the moment `fah-lan6` still holds the old one, and in that window
      LAN-to-LAN v6 `:80` is redirected into the proxy. **Not measured** — the
      one observed rotation was a deliberate reboot, not a live redial, so the
      lag was never exposed. Carried to §Left open rather than blocking closure:
      the failure it describes is a LAN-to-LAN request briefly proxied, not a
      loss of filtering.
- [x] Gates green if any code changed; nothing to run — none did.

## Sequencing

Both gates cleared 2026-08-09:

1. **DIGI's IPv6 is fixed** — `IPv6 global UP` logged at 21:10:07;
   `traceroute6` now completes in 7 hops where it previously died at the third.
2. **The 0.2.13 soak has ended**, verified in
   [`soak-0.2.13-report.md`](../../../docs/code-review/phase2/soak-0.2.13-report.md).

Gate 2 shares a window with the `p2-13` deploy.

## Out of scope

- IPv6 egress from the container to origins. The proxy reaches origins over v4
  by design and that is not a gap this task closes.
- The ISP's prefix-delegation behaviour. The infinite (`never`) lifetimes on a
  rotating prefix are a DIGI-side defect; this task only has to survive it.
- HTTPS over IPv6 — Phase 3 owns the interception path, and it inherits whatever
  this task settles about client identity.
