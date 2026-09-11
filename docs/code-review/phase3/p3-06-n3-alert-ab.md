# N3 follow-up — does the client TLS alert differ by cause? A/B on the OnePlus 15

## Summary

- Question from [p3-06-testing-results-2.md](p3-06-testing-results-2.md) §N3:
  can the engine tell *application refuses our leaf* from *client never trusted
  our CA* by the alert it receives? p3-08 assumed the second sends `unknown_ca`.
- Instrumentation, this branch: three per-alert counters on
  `listeners.https` (`alert_bad_certificate`, `alert_certificate_unknown`,
  `alert_access_denied`, summing to `client_cert_rejections`) and the alert
  name on the accept-side `debug!` lines, including alerts outside the
  rejection set.
- Same phone, same apps, same hosts, CA installed (B) then removed (A):
  **the alert did not change with the cause.** It changed with the app's TLS
  stack.
- Chromium without the CA sends `certificate_unknown`, the alert Spotify sends
  with the CA installed. Firefox without the CA sends `unknown_ca`.
- Decision: 525 semantics, classifier, Interception Document and policy
  unchanged. Diagnosis moves into the engine: fah-stats keeps a per-client
  account of the terminate leg (`intercepted.completed` / `rejected` with last
  times, on `GET /api/v1/clients`), the HTTPS listener gains
  `handshakes_completed`, and the rejection view shows the account beside its
  rows. UDP 443 is refused for intercepted clients, mandatory (§Clean rerun).
- Scope: one device — OnePlus 15, OxygenOS, Android. Superseded by a second
  stack disagreeing.
- Status: **technical experiment, concluded 2026-09-11, not promoted.** The
  code is on `phase3-06`; nothing was deployed; the trial setup on the router
  was torn down the same evening.

## Decisions

- Keep `rejection_status` as is; `unknown_ca` stays status 0 and uncounted —
  Firefox showed it is a real, correctly handled stack on this phone.
- Keep the three counters; they replaced a DEBUG re-run and carry the alert
  identity through `GET /api/v1/telemetry` (API.md).
- Both accept-side log lines return to `debug!`: at this phone's rate the
  INFO lines filled RouterOS's 500-line memory buffer in about four minutes.
- The engine states it, not the page: the per-client account lives in
  fah-stats' bounded client registry (cap 4096, fed by the event channel, zero
  hot-path cost) and the view reads it once on mount and on Retry — the
  dashboard does not poll. A page-ring variant was built first and dropped.
- The view says "completed intercepted handshakes on record" — never "CA
  installed" or "client trusts CA". Exclude is unchanged.
- No per-client exclusion, no gating of Exclude, no alert name on the event:
  the alert is stack identity and would be read as cause.
- UDP 443 is refused for intercepted clients (SECURITY.md, deploy-rb5009.md
  §5c); the runbook and review premise that said the opposite are corrected.

## Method

| Item | Value |
| --- | --- |
| Device | OnePlus 15, OxygenOS, 192.168.10.11 + `2a02:2f04:5400:cc00::/64`, static lease, owner-operated |
| Probe | `fah-probe` on `veth3`, tip `4596e1e` plus the instrumentation, `fah-probe-4596e1e-n3ab-rosready.tar` 16 886 784 B, sha256 `7111cb1a…aae122`, started 2026-09-11 15:44:00 local |
| Steer | R7 v4 (`src-address=192.168.10.11`) and v6 (`p3-06-probe-client`) in place throughout |
| Reads | `GET /api/v1/telemetry` `listeners.https`; `/log print where message~"rejected our certificate" or message~"outside the rejection set"` |
| Order | B first (CA installed): Chrome `example.com`, Spotify. Remove CA, force-stop browsers, restart phone. A: Chrome and Firefox `www.bbc.com`, Spotify |
| Not run | UniCredit — a second app in the same state, no discriminating power, two document writes |

Three traps met, all owner-side, none a defect:

| Trap | What happened | Cure |
| --- | --- | --- |
| Browser session cache | After CA removal Chrome still loaded pages on the open connection | Force-stop the browser, use a host not visited that day |
| v6 rotation on restart | The phone restart produced a new temporary address `…:ae0a:b03b:ab28:deb3`, absent from the steer list — v6 would have bypassed the probe | Re-read `/ipv6/neighbor` by MAC, add to `p3-06-probe-client` before measuring |
| "Issued by FastAdHunter CA" on the warning page | Read as "still trusted" | The viewer shows what was presented; the fatal alert in the probe log is what proves distrust |

## Measurements

Counters, `listeners.https`, since the container start:

| Uptime | Point | connections | rejections | bad_certificate | certificate_unknown | access_denied |
| --- | --- | --- | --- | --- | --- | --- |
| 47 s | baseline | 0 | 0 | 0 | 0 | 0 |
| 107 s | B, background apps | 23 | 16 | 0 | 13 | 3 |
| 227 s | B, after Spotify | 400 | 392 | 0 | 389 | 3 |
| 1218 s | A, after browsers | 1435 | 1082 | 2 | 1027 | 53 |
| 1372 s | A, after Spotify | 1583 | 1229 | 3 | 1168 | 58 |

Alert per host, from the probe log (the memory buffer rolls, so B is the
15:45–15:47 read and A the 16:00–16:06 reads):

| Stack | Host | B, CA installed | A, CA absent |
| --- | --- | --- | --- |
| Chromium (Chrome / Brave) | `example.com`, `www.bbc.com` | completed, issuer FastAdHunter CA | `CertificateUnknown` ×2 |
| Firefox | `www.bbc.com`, Mozilla and uBlock filter hosts | not run | `UnknownCA` ×4 bbc, ×100+ filter hosts, status 0 |
| Spotify | `login5`, `spclient.wg`, `gew1-spclient`, `links.tospotify`, `i.scdn.co`, `*.spotifycdn.com` | `CertificateUnknown`, all hosts, 372 lines | `CertificateUnknown`, all hosts, 68 lines |
| Google Play services | `notifications-pa`, `firebaseinstallations` | `CertificateUnknown` ×18 | `CertificateUnknown` ×317 |
| Facebook | `z-m-gateway.facebook.com` | `AccessDenied` ×3 | `AccessDenied` ×33, `CertificateUnknown` ×14, `graph.facebook.com` `BadCertificate` ×1 |
| OxygenOS | `weather-server.allawnos.com`, `*.heytapmobile.com` | `CertificateUnknown` ×5 | — |

The decisive row is Chromium: missing CA on this device produces exactly the
alert Spotify produces with the CA present. No classifier split separates them.

### Clean rerun, same day, bypasses closed

The first run's browser observations were corrupted by two bypasses (below), so
the A/B was repeated with every phone flow verified to pass through the probe:
zero un-steered TCP 443 flows on either family, zero UDP 443 flows, watched
over 20 s on the router's connection table before measuring. Engine readings
were unaffected in the first run — they come only from flows that reached it —
but the rerun removes the doubt.

| Step | Phone shows | Engine sees |
| --- | --- | --- |
| B, CA installed, Chrome `www.bbc.com` | page loads, issuer FastAdHunter CA | no alert line; event stream: requests inside intercepted sessions from both the v4 and the current v6 address, `ichef.bbci.co.uk` among them, 854 in 150 s |
| B, Spotify | does not play | `CertificateUnknown` ×216 in 2 min |
| A, CA removed, browsers force-stopped, Chrome `www.bbc.com` | warning page, certificate details still say FastAdHunter CA | `CertificateUnknown` ×3 |
| A, Spotify | does not play | `CertificateUnknown` on all six hosts |
| A, 20 s event-stream sample | — | 27 `https` events, all 525, **zero** carrying a request |

Same result as the first run. Same browser, same host, same wire path, one
variable changed, one alert either way.

**Two bypasses found, both about the steer, both product-relevant:**

| Bypass | Evidence | Fix applied for the test |
| --- | --- | --- |
| Android v6 temporary-address rotation. The phone rotated three times in one afternoon (restart, airplane mode); an address-list steer misses every new address, and its flows go straight to the origin | connection table: every v6 TCP 443 flow from the current address `…:500c…` replied from Cloudflare, Google, Microsoft, Akamai — not from the probe. "Spotify works" with the CA installed was this | v6 dst-nat matches `src-mac-address=78:ED:BC:44:AE:1D` instead of the list. The `I` flag on the rule after `set` was the transient one already recorded |
| HTTP/3. The steer is tcp-only; the runbook's premise "UDP/443 stays unsteered so browsers fall back to TCP" is backwards — browsers fall back only when QUIC fails. Left open, Chrome and Cronet-based apps reach the origin over UDP 443 and get the real certificate | connection table: assured UDP 443 flows from the phone to Google (`2a00:1450…`), 207 packets; "issued by the original CA" after CA removal in the first run was this | `forward` reject (v4) / drop (v6) of UDP 443 by the phone's MAC. Existing flows keep their NAT decision and pass the "accept established" rules, so the phone's connections had to be cut (airplane mode) before the rules took effect |

Production's steer is tcp-only too: **interception is bypassable over HTTP/3
for every listed client until UDP 443 is refused for them.** Decision
2026-09-11: mandatory operational rule (SECURITY.md, deploy docs).

**One misreading of mine, corrected:** the HTTPS listener's `requests` counts
SNI verdicts per connection plus requests inside intercepted sessions
(API.md); it is not a completed-handshake count, and the rises quoted from it
during the clean run were SNI judgements. Completed handshakes were proven by
the event stream instead (`https` items carrying a request). A
`handshakes_completed` listener counter is added so that this number exists.

The first run's "successes in B: 8 of 400" was derived as connections minus
rejections and is withdrawn on the same grounds; the event stream is the
evidence.

## Files changed

| File | Change |
| --- | --- |
| `crates/fah-http/src/intercept.rs` | `alert_counter`; `handshakes_completed` after `accept`; the alert named on the non-rejection path too; both lines `debug!` |
| `crates/fah-http/src/proxy.rs`, `crates/fah-model/src/engine.rs` | four counters, `#[serde(default)]` |
| `crates/fah-model/src/request_event.rs` | `CLIENT_CERT_REJECTED` / `UPSTREAM_CERT_FAILURE` move here — one home, fah-http and fah-stats read them |
| `crates/fah-stats/src/client_registry.rs`, `stats.rs` | `InterceptedHandshakes` on the client record; `record_https` classifies 525 → rejected, request inside → completed |
| `crates/fastadhunter/src/main.rs`, `adapters.rs` | `Event::Https` routed to `record_https`; the account mapped into the API port |
| `crates/fah-api/src/ports.rs`, `wire.rs`, `routes.rs` | `intercepted` object on `GET /api/v1/clients` |
| `crates/fah-http/tests/interception.rs`, `crates/fah-api/tests/api.rs`, fah-stats tests | per-alert and completed assertions; JSON shape; classification |
| `API.md`, `CONTEXT.md` | the counters, the `intercepted` object, the alert names the stack |
| `SECURITY.md`, `docs/deploy-rb5009.md`, `docs/routeros-traps.md` | UDP 443 refused for intercepted clients; Android user-store limit; steering-one-client traps |
| `dashboard/frontend/src/pages/live-feed/rejections.ts` | `summarizeClients` over the API's client list |
| `dashboard/frontend/src/pages/live-feed/rejections-view.tsx` | reads `/api/v1/clients` on mount and Retry; `ClientsSummary` above the rows; footer line |
| `dashboard/frontend/src/api/types.ts` | `intercepted` on `Client` |
| `p3-06-testing-results-2.md`, `p3-08-…-review.md`, `p3-09-…-review.md`, runbook, verification review | point here; the UDP 443 premise corrected |

Gates: `cargo fmt`, `clippy -D warnings`, `cargo test --all-features
--workspace` green; vitest 58 files / 1047 tests; bundle 137 074 B gzip
(89.2 % of 153 600, +701 B).

## Remaining TODOs

- Not run on the probe: the trial setup was torn down the same evening (probe
  container, `kingston/fah-probe`, image tars removed; `veth3` remains).
  Verified by the local suites only. `fah-probe-4596e1e-n3ab2-rosready.tar`
  (16 889 344 B, sha256 `26b11df3…c9fe1`) stays on the dev box for a rebuild.
- `docs/dashboard/information-architecture.md` sub-view paragraph: describe
  the per-client line (owner approval pending).
- A second stack — iOS, Windows Schannel — would extend the scope of the
  "alert names the stack" claim; nothing here predicts them.
