# P5-02 — Certificate and Browser Spike

**Phase:** 5 · **Depends on:** p5-01 · **Model:** Opus

## Goal

Establish, from real devices, what a household browser does when it opens this
box's dashboard — and fix the certificate so the session design in `p5-04` rests
on measured behaviour instead of an assumption. Small and decisive: evidence,
one SAN decision, one migration rule.

## Context

`crates/fah-api/src/tls.rs:18` generates a self-signed certificate whose SANs are
`fastadhunter`, `localhost` and `127.0.0.1`. Nothing covers the address a
household member actually types. SECURITY.md §TLS promises "a one-time warning
for the self-signed certificate"; a browser opening `https://192.168.88.1:8443/`
gets a **name-mismatch** error instead, which is a different and less forgiving
interstitial.

This matters because everything in `p5-04` rides on it: `__Host-` requires
`Secure`, `Secure` requires the browser to treat the origin as HTTPS, and the
plan forbids an HTTP fallback under any framing. If a phone will not persist a
`Secure` cookie for this origin, the auth design does not work and it is better
to know that before it is built.

**This is not Phase 3.** Phase 3 delivers real certificate machinery — generate
CA, import PEM/PFX, export CA, status. This task must not build a second,
competing certificate architecture. It picks the smallest change that makes the
Phase 5 dashboard usable and that Phase 3 can later supersede without a rewrite.

## Scope

- **Evidence, on real devices over the LAN.** At minimum one desktop browser and
  one phone browser, each recorded with name and version:
  - what the interstitial says on first visit, by address form — IP literal,
    `fastadhunter`, and any mDNS/DNS name the household actually uses;
  - what it takes to proceed, and whether that has to be repeated;
  - whether the exception survives a browser restart;
  - **whether a `Secure`, `__Host-`-prefixed cookie is set and persists** on that
    origin after the exception is accepted. This is the load-bearing reading —
    a warning that can be clicked past is tolerable, a cookie that will not
    persist is not.
- **SAN decision and the code change that implements it.** Options, cheapest
  first: the configured `[api] address` when it is a literal; the box's detected
  non-loopback interface addresses at generation time; an explicit
  operator-supplied SAN list. Pick one, state why, implement it. Every config key
  ships with a working compiled-in default — the household must not have to
  hand-edit TOML to make the dashboard reachable.
- **Regeneration migration.** Changing the SAN set means a new certificate, which
  **voids every exception already accepted** on every household device. Decide
  and document: when regeneration triggers, whether an existing `/config`
  certificate is left alone, and what the operator sees. A silent regeneration
  that makes every phone warn again is worse than the mismatch it fixes.
- **The recommendation**, written for SECURITY.md and the deployment notes: what
  a household should do — accept once, install the box certificate on devices, or
  a trusted name and certificate — with the measured cost of each.

## Acceptance criteria

- Browser behaviour recorded for at least one desktop and one phone, each with
  device, browser and version, per address form.
- A `Secure` `__Host-`-prefixed cookie demonstrated to set **and persist** on the
  chosen origin on the phone. If it does not, that is the finding, and `p5-04`
  does not start until the path is settled.
- SAN mechanism implemented, with a test asserting the generated certificate
  carries the expected names.
- Regeneration behaviour defined and tested: an existing `/config` certificate is
  not silently replaced.
- The recommendation drafted for SECURITY.md and the deployment notes.
- No HTTP fallback introduced, proposed, or left as an option.
- Gates green, `request_coverage.rs` included.

## Out of scope

Phase 3's CA machinery — generating a CA, importing PEM/PFX, exporting a CA,
certificate status endpoints. Any UI. Any change to the auth design itself
(`p5-04`). Anything that makes a warning less visible rather than less necessary.

## Suggested prompt

> Read SECURITY.md §TLS, `crates/fah-api/src/tls.rs`, and
> plan/open/phase5/p5-02-cert-browser-spike.md. Measure what desktop and phone
> browsers do against this box over the LAN for each address form, and whether a
> `Secure` `__Host-` cookie persists. Then pick and implement the smallest SAN
> mechanism that makes the dashboard reachable without hand-edited config, define
> the regeneration migration, and draft the SECURITY.md recommendation for
> approval. Do not build Phase 3's CA machinery and do not add an HTTP fallback.
