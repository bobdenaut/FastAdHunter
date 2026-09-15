# Phase 4 is parked: HTML rewriting has nothing to rewrite

The architecture was designed to grow DNS → HTTP → HTTPS → HTML without
changing shape, and Phase 4 was the last step: a streaming `lol_html` rewriter
and cosmetic rules. It is **parked on 2026-09-15**, by owner decision, because
the traffic it would act on does not exist on this deployment.

Rewriting HTML requires the response body. An HTTPS body requires terminating
TLS, which is interception. Interception has been off since 2026-09-13 — the
Interception Document's `clients` list is empty — and the owner reaffirmed that
on 2026-09-15 with the operational reason rather than a preference: switching it
on means installing the CA on televisions, WiFi routers and appliances, many of
which ignore the user trust store or pin their own chain, and whose failure mode
is not a certificate warning but an application that quietly stops working.

So Phase 4 could only act on plain HTTP. Two measurements say what that is
worth, and they are independent of each other.

**Plain HTTP is under 1 % of what the box decides.** Over 3.3 days of household
traffic on the deployed 0.3.4 (soak pull `20260915T060002Z`): `http.pass` 2 900
and `http.refused` 283, against `dns.block` 343 260 and `dns.pass` 78 951. The
334 MB those 2 900 requests carried is firmware updates, certificate checks and
appliances — not pages anyone reads. No rewriter improves a browsing experience
on that path, because no browsing happens there.

**The deployed corpus is 0.06 % URL rules.** Measured 2026-09-15 by compiling
the sixteen configured list URLs through the real parser
(`cargo run --release -p fah-rules --example urlbench`): **1 182 029 DNS rules,
720 URL rules, 39 inactive**. The 720 come from two lists only — the AdGuard DNS
filter (530) and Hostlists Registry filter 50 (190); the other fourteen carry
none. All sixteen sources are DNS blocklists by design: `big.oisd.nl`, eleven
AdGuard *Hostlists Registry* filters — the DNS-filter registry, not the
browser-filter one — three hagezi `dns-blocklists`, and a phishdestroy
`hosts.txt`. There is no EasyList and no EasyPrivacy.

So `http.block` is **0** for two reasons at once: DNS got there first, and what
remains for the URL engine is 720 ad-path rules meeting 2 900 firmware and
certificate-check requests. The 554 µs URL-verdict figure in README comes from a
full EasyList + EasyPrivacy bench corpus, which is orders of magnitude more URL
rules than this deployment loads.

The second measurement matters more than it first appears: interception would
decrypt every HTTPS session in order to apply those 720 rules. As configured, it
buys almost nothing.

## Considered options

- **Ship Phase 4 for plain HTTP only.** Technically sound and architecturally
  free — the rewriter sits behind the same pipeline. It would run against 2 900
  non-browser requests per 3.3 days. Cost without a beneficiary.
- **Turn interception on so Phase 4 has a body to rewrite.** This reverses a
  decision taken for operational risk on a household network that includes
  devices nobody can debug. It would also need URL-path lists to be worth
  anything, which are not deployed.
- **Leave Phase 4 `not started` indefinitely.** The state the repository was in.
  It reads as work still coming, which is worse than a recorded decision: a phase
  that will not be built should say so, with its reason, rather than sit open and
  quietly age.

## What this costs and what it does not

`lol_html` leaves the effective technology stack; the `lol_html` half of the
deferred capacity microbench (rustls AES-GCM · lol_html ms/MiB on the RB5009)
parks with it, the AES-GCM half having run on 2026-09-15. Cosmetic rules are not
delivered, which was never something a router could do well anyway — a router
cannot see a page's DOM, and README has said so from the start.

Nothing architectural changes. The Rule Engine's two typed entry points, the one
compiled index and the atomic swap are untouched, and the HTTP pipeline keeps
streaming bodies without parsing them. A later decision restores Phase 4 exactly
where it was.

## What would reverse it

**Both** of these, not either: interception switched on for real clients, *and*
URL-path lists loaded so there is something to enforce inside the decrypted
stream. Either one alone leaves Phase 4 acting on nothing, which is the state
this decision records.

Mechanically, the four tasks in `plan/open/phase4` are marked `PARKED`.
`plan/CLAUDE.md` already allows a phase to close with a parked task in it, so
when Phase 3 closes the selector walks Phase 4 through `wip` to `closed` on its
own — the closure is a recorded consequence rather than an ad-hoc folder move.
