# Phase 3 — HTTPS + Certificates + Encrypted DNS Listeners

**Objective:** ROADMAP.md Phase 3: certificate machinery (generate CA, import
PEM/PFX, export CA, status — rustls + rcgen + x509-parser, no hand-rolled
crypto), SNI-level HTTPS filtering for everyone, full HTTPS interception for
managed clients (opt-in, per-client, never default), and DoT/DoH **listeners**
so Android Private DNS points at us. Operating mode `dns+http+https` becomes
real.

**Why this order:** certificate core first (everything else consumes it), its
API second (small, unblocks dashboard-side work), then SNI filtering (big
value, zero interception risk), then interception (the hard, sharp tool, on a
proven base), then encrypted DNS listeners (need the cert story for clients to
validate), verification last.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p3-01-cert-core.md` | CA generation, leaf minting + cache, PEM/PFX import, storage (heavy) | Opus | WAITING |
| 2 | `p3-02-certificates-api.md` | `/api/v1/certificates` per reserved namespace; API.md updated | Sonnet | WAITING |
| 3 | `p3-03-sni-filtering.md` | Blocked domains die at SNI — no decryption, works for every client | Sonnet | WAITING |
| 4 | `p3-04-tls-interception.md` | Opt-in per-client MITM feeding the Phase 2 HTTP pipeline (heavy) | Opus | WAITING |
| 5 | `p3-05-dot-doh-listeners.md` | DoT :853 + DoH listeners; Android Private DNS works | Sonnet | WAITING |
| 6 | `p3-06-phase3-verification.md` | TLS budgets, e2e, RB5009 dst-nat 443 + CA install walkthrough | Sonnet | WAITING |

**Definition of done:** any client gets SNI-level HTTPS blocking with zero
setup; a managed client with the CA installed gets full URL-level filtering
inside HTTPS; a phone with Private DNS set to the container resolves over DoT;
banking/pinned apps keep working (exclusions honored); budgets hold; SECURITY.md
promises verified (CA key never leaves `/config`, public-only export).

**Key risks:** certificate-pinned apps break under interception (mitigation:
interception is opt-in per client + exclusion list ships with known pinned
domains; SNI path is the default and breaks nothing); Android CA install
friction (mitigation: p3-06 walkthrough with screenshots; DoT needs no CA);
encrypted ClientHello (ECH) hides SNI on some traffic (documented limitation —
DNS layer still catches those domains).
