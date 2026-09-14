# p3-11 — Security suite in the shipped configuration

## Summary

The five security arms p3-11 owes already existed in
`crates/fastadhunter/tests/security_phase3.rs`. What did not exist was evidence
for two of them in the configuration that ships — `clients: []`. The API route
walk booted with a client listed, so the shipped surface was never walked; and
the "not masked" arm is an interception property with no splice-side
counterpart. Both closed here. The suite is 9/9. No production code changed.

## Decisions

- The route walk runs **twice**, `clients: []` and `clients: ["127.0.0.1"]`,
  rather than being switched over. The property is credential-scoped, not
  mode-scoped; dropping the interception arm to gain the shipped one would have
  traded coverage rather than added it.
- `bad_upstream_cert_is_not_masked` stays interception-only. Under `clients: []`
  the binary never verifies the origin, so `526`, minting and
  verification-before-minting have no meaning on that path.
- The splice-side property is a different claim and gets its own test: the
  origin's certificate reaches the client unchanged. Proved by **which client
  succeeds**, not by comparing bytes.
- PERFORMANCE.md rows 58, 59, 63 and the `dns+http+https` half of row 45 are not
  fillable on a dev box. The file's own rule (line 141) is that splice figures do
  not convert from loopback.

## Bugs found

None. Two coverage gaps, both closed.

## Measurements

| Arm | Test | `clients` | Before | Now |
| --- | --- | --- | --- | --- |
| CA key unreachable on every route | `ca_key_unreachable_via_every_route` | `[]` | not covered | 212 requests, 70 routes x 3 credentials, 0 leaks |
| the same, interception on | `ca_key_unreachable_via_every_route_with_a_client_listed` | `["127.0.0.1"]` | covered | unchanged, 212 requests |
| bad upstream certificate not masked | `bad_upstream_cert_is_not_masked` | `["127.0.0.1"]` | covered | unchanged; interception-only by nature |
| bad origin relayed untouched | `a_bad_origin_certificate_is_relayed_untouched_when_interception_is_off` | `[]` | not covered | **new** |
| non-listed client never minted a leaf | `non_listed_client_is_never_minted_a_leaf` | `["127.0.0.2"]` | covered | unchanged |
| exports carry no private material | `exports_contain_no_private_material` | `[]` | covered | unchanged |
| interception off implies byte-identical splice | `splice_is_byte_identical_when_interception_is_off` | `[]` | covered | unchanged |

Falsification, then reverted: flipping the new test's `clients` to
`["127.0.0.1"]` fails at its first assertion — "a client trusting the origin's
own certificate must complete the handshake through the splice". It fails
because interception terminates the connection and refuses an origin it cannot
verify, so the client never sees the origin's certificate at all. That is the
same behaviour `bad_upstream_cert_is_not_masked` pins from the other side.

Suite: 9 passed, 0 failed, 10.6 s. Windows dev box, 2026-09-14.

## Files changed

| File | Change |
| --- | --- |
| `crates/fastadhunter/tests/security_phase3.rs` | route walk extracted into `walk_every_route_for_key_material`, called by two tests; one new test for the splice side of "not masked" |

## Remaining TODOs

- `dns_query_is_the_only_new_unauthenticated_route` prints "0 of 18 answered
  200": the dev box has no web root, so its two static-path assertions are
  vacuous (X5). The on-device curl walk is the closing evidence, and it waits on
  the deploy decision.
- **README drift, not edited here — needs the owner's go.** Line 1 and line 444
  present per-client interception as part of `dns+http+https`, but the mode opens
  the listener and the Interception Document's `clients` list decides
  interception, empty since 2026-09-13. Line 37 still points at branch
  `phase3-06` and a 24 h soak. Line 45 says 0.3.3 since 2026-09-09; production
  runs 0.3.4 since 2026-09-11.
- **PERFORMANCE.md rows 60, 61 and 62** — interception handshake overhead,
  intercepted h2 relay, minted-leaf cache hit rate — read "TBD, must be measured
  during verification", which reads as owed work. Under the 2026-09-13 decision
  they cannot be filled at all. A wording change needs the owner's go.
