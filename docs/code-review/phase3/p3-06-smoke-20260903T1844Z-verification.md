# p3-06 smoke session 20260903T1844Z — independent verification

Read-only verification of `docs/code-review/phase3/p3-06-probe/smoke-20260903T1844Z/`
against the frozen `plan/wip/phase3/p3-06-testing-plan.md`, the smoke plan and
the raw evidence. Only `node --check` was executed (12 files, all pass). No
number below is a result.

## Summary

- Scripts implement the frozen plan: gate statistics, result-file names, delta
  11 at P1-LAN / P2 / P3 throughput, per-stage preconditions and INVALID
  reporting all match. Deviations are additions, not omissions: `p0-sni.mjs`
  asserts two invalidity rules and one gate term the plan never declared.
- Both `.md` files are accurate on every `valid` / `INVALID` / `degraded`
  verdict checked against its result file, with one label error (Layer 1 `p3`).
- Evidence gaps: the container-boot lines quoted for `Dockerfile.fahprobe` and
  F27 are in no file; the seven-origin preflight table and F24's
  `speed.cloudflare.com` observations exist only in the report text; the
  "previous session recorded it" claim has no file behind it.
- Both files state numbers as comparisons (verdict table, F25, F28, F29),
  against their own rule.
- Nothing script-side blocks the campaign. P3 (no trusted 8 MiB h2 origin),
  P4 (harness DoT / DoH, F25 / F26) and P2 (no bridged VM, F15) are blocked by
  environment or harness items the owner holds; SNI needs a recorded delta.
- Verdict: PASS WITH DEFERRED FINDINGS.

## Check A — scripts vs the frozen plan

| Script | Matches plan | Deviations |
| --- | --- | --- |
| `lib.mjs` | mostly | Holds flag parsing, bearer API, `timedRequest`, percentiles, `Run` reporting, results dir, config snapshot, host checks (record only, line 335). Two things outside the declared boundary: the `engine.mode` INVALID decision at line 317 (a precondition decision, opt-in via `needsHttps`), and two single-stage helpers — `dnsParse` (only `p4-lan.mjs`) and `localAddressToward` (only `p3-h2stall.mjs`). No measurement logic. |
| `p0-sni.mjs` | gate yes; rules widened | Gate boolean matches (line 203). `sni.json`, `raw.jsonl`, `run.log`, `valid` present. Asserts three things the frozen plan does not declare: allowed name must reach ServerHello else INVALID (line 165), `listeners.https.blocked` must move by at least the attempt count else INVALID (line 172, the F18 fix), and no-SNI rows are folded into the gate boolean (line 208) although the plan's SNI gate names the blocked domain only — with `no_sni = "pass"` a no-SNI close is a listener property (`https.rs:286`, nothing to splice to), not a verdict. No SNI row exists in §Invalidity rules. |
| `p1-lan.mjs` | yes | Gate = median ≥ 100 and inside the P1-control band from `--control` (lines 174–183, `pass: null` without the band, matching the plan's "no ceiling"). Delta 11: zero completed ⇒ INVALID (line 158), > 2 % ⇒ degraded (line 162). Arms `spliced` / `p1_control` / `aggregate` write `p1.json` / `p1-control.json` / `p1-aggregate.json`. Egress-list precondition (line 68). Nit: line 164 checks `authorized` on connection 0 only (`row.tls = conns[0].tls`), so the aggregate arm never sees a failed verification on connections 1–7. Undeclared but harmless: degraded without `--origin-cert` (line 57), degraded without `--profile-share` (line 187). |
| `p1-origin.mjs` | yes | Serves N MiB then closes; no HTTP; `:443` default; no assertions. |
| `p2-handshake.mjs` | yes | Identity precondition in the plan's order (lines 92–105); both issuer rules (198–199); delta 11 (196, 200–203); gate intercepted p50 ≤ 2 × spliced p50 (227–233); spliced − direct row value, both columns (211–214); `blocked ≠ 0` ⇒ degraded (204). Nit: no `ALPNProtocols` on `tls.connect` (line 138), so the recorded origin ALPN is always `null` — the plan asks for the origin's ALPN recorded. |
| `p3-h2stall.mjs` | yes | Warm-up before "before" (161–165); barrier = every stream `:status 200` + first DATA, reading stopped (109, 177); exclusive window = connections delta 2 and DNS flat (210–212); S/C interleaving with own warm-up / before / window / after (288–291); attribution RESOLVED iff min stall > max control (296); reading against 5.5 MiB only when RESOLVED (308); throughput median ≥ 50 (283); delta 11 (277–279); listed-client precondition (68); issuer rules (164, 275). `p3.json`. |
| `p4-lan.mjs` | yes | Labelled P4-LAN (line 210), 16-bit ID matching (`Matcher`), DoT 2-byte reassembly (106–114), unanswered / unmatched reported, gate `none — diagnostic` (226). Undeclared: a 98 % answered budget ⇒ degraded (line 223) — delta 11 applied to a stage the plan does not name. Harmless (no gate). |
| `p5-mint.mjs` | yes | CA present, DoT listening, headroom below capacity (40–43); first-sight then repeat; `minted_total` = hosts, `evictions` 0 across the repeat pass, repeat minted 0, issuer = CA on every row, `unwarmed_misses` reported (100–104, 118); gate median − median < 1 ms (122). `p5.json`. |
| `p6-certs-time.mjs` | yes | Columns `starttransfer − appconnect` and `time_total` (60–67); every call 200 else INVALID (78); archive counts first, cap arithmetic (52–58, F10 fix); re-export after generate (94–99); gate medians < 100 / < 50 (118–125). Undeclared: `api_certificate.source == "imported"` asserted immediately after import (line 100); the plan places that check owner-side after restart. Harmless. `p6.json`. |
| `p7-store.mjs` | yes | `--ca-key` mandatory and parsed (41–57); PEM / DER export and `/config` searched for marker, base64 and DER payloads (84–122); traversal list identical to `security_phase3.rs::static_and_traversal_paths` (20 paths, with and without bearer); status recorded, never asserted (F20 fix); gate every check pass (150). `certs.json`. |
| `smoke/h2-origin.mjs` | n/a (smoke only) | `/`, `/8mib`, `/stall` as the smoke plan describes; not a stage. |

## Check B — the two `.md` files vs the raw evidence

| Claim | Evidence | Verdict |
| --- | --- | --- |
| SMOKE-REPORT Layer 3, `Dockerfile.fahprobe` row: reason lines `fastadhunter starting config_path=/config/fastadhunter.toml data_dir=/data mode=DnsHttpHttps` and `list refreshed list=oisd-basic active=62948` | `layer3-fahprobe-boot.log` starts at `DNS listeners bound`, has no `starting` line, and holds `all upstreams failed` and `scheduled list refresh failed list=oisd-basic` instead. The `starting … mode=DnsHttpHttps` / `active=62948` lines exist only in the host-binary logs `fah-boot-*.log` (`config_path=smoke-config/…`). The good boot is proven indirectly by `layer3-l1a/run.log` `engine.mode = dns+http+https`. | not in the cited file |
| FINDINGS F27 observed: `mode=Dns` boot line, `docker inspect` bind / volume lines, host-side mount listing; files: `layer3-fahprobe-boot.log` | None of those lines is in that file or in any other file in the session directory. | no file evidence |
| SMOKE-REPORT "Full preflight output is in `layer3-p3-preflight.log`"; the seven-origin preflight table; F24 observed lines for `speed.cloudflare.com`, the `openssl s_client` output, `cloudflare.com /cdn-cgi/trace` | The file holds two lines, both `sabnzbd.org`. The other six table rows and every F24 observed line are in no file. | not in the cited file |
| F24 note and SMOKE-REPORT: the `speed.cloudflare.com` failure is "the same condition the previous session recorded against this origin" | No file under `smoke-20260903T1303Z/` or `smoke-20260903T1557Z/` mentions `speed.cloudflare.com` (the only `cloudflare` hit in `docs/code-review/phase3/` is the ECH origin in the review file). | "reappears" unsupported by files |
| F24: "Seven origins were preflighted"; repro `node ./h2pre.mjs …` | The note names ten hosts (adds `cdn.jsdelivr.net`, `mirror.nl.leaseweb.net`, `test.rebex.net`); the saved file is `h2-preflight.mjs`, no `h2pre.mjs` exists. | internally inconsistent |
| SMOKE-REPORT Layer 1 `p3` row: "valid for this layer"; tally "8 valid" | `layer1-b/p3.json` lines 38–39: `"valid": false`, `"status": "INVALID"` (the RSS arm's exit writes the whole file INVALID; the throughput row inside it is `ok: true`). The smoke plan calls this INVALID "the correct outcome". | label contradicts file |
| SMOKE-REPORT Layer 1 `p3` RSS reason line, quoted as ending "…on-device only" | `layer1-b/run.log` line 35 continues `; control#1 process_rss is null on this probe …`. A prefix quoted as the whole line, no ellipsis. | not verbatim |
| SMOKE-REPORT Layer 2 "no API key": `INVALID: no API key: --key <key or file> or FAH_PROBE_KEY` | `layer2/r1-nokey/run.log`: `INVALID: no API key: --key <key\|file> or FAH_PROBE_KEY` (`lib.mjs:308`). | not verbatim |
| SMOKE-REPORT Layer 2 tally "21 rows fired exactly the INVALID or degraded" | The table has 22 such rows (no key, `/health`, six `engine.mode`, busy, `--allow-busy`, allowed, egress, bytes, identity, not-listed, no-CA, headroom, cap, cap-409, unreadable, not-a-key, failure-rate). Every one of the 22 has its reason line verbatim in `layer2/<row>/run.log`. | count off by one |
| SMOKE-REPORT Layer 3 splicebench: `docker top` shows `/fah-splicebench --reps 3 --size-mib 64` for the `--reps 1 --size-mib 8` run | `layer3-splicebench-run.log` carries five `rep=1` splice lines and no `rep=2`; `layer3-splicebench-top.log` shows the default-argument command line. The top log is from a different container run. Process name proven; run identity not. | evidence from another run |
| SMOKE-REPORT Layer 1 `p6`: `ca-after-p6.pem` "differs from `smoke-ca.pem`" (Layer 3 likewise) | `cmp`: repo-root `smoke-ca.pem` (re-exported after p6, as the smoke plan instructs) is byte-identical to `layer1-a/ca-after-p6.pem`; same for `smoke3-ca.pem` vs `layer3-l1a/`. The CA change is proven by the before / after fingerprints in `layer1-a/run.log` lines 100 and 105 instead. | unverifiable as written |
| F23 files: "(overwritten by the corrected rerun in the same directory …)" | `run.log` is append-only: the mangled-path attempt is at `layer1-b/run.log` lines 15–23 and the corrected run at 25–35. Only `p3.json` was overwritten. | field wrong, evidence exists |
| Both files: "no number below is a result, a budget or a comparison" | Verdict table row 3 compares `dot p50=…us` against `udp p50=…us` and says the DoT column "now sits with UDP and DoH"; F25 compares this session's DoT p50 / min with last session's and derives "one delayed-ACK interval instead of two"; F28 quotes F19's DoT and UDP p50 and says the column "sits below UDP"; F29 says the control delta is "an order of magnitude below". | numbers used as comparisons |
| SMOKE-REPORT §Deviations is complete | Not listed: (a) Layer 3 `p3` at `--bytes 5` where the smoke plan requires `bytes` = 8 388 608 (the preflight text mentions it, §Deviations item 4 does not); (b) Layer 3 container `egress.allow_destinations` = `127.0.0.0/8`, `172.16.0.0/12` and the origin on user bridge `fah-smoke-net` (`layer3-l1a/config.json:66–69`) — items 5–6 cover upstream and clients only; (c) Layer 2 rows marked "(all)" (no key, `/health`, busy) run for `p0` only; (d) a report file was written although smoke plan §Report says none is — same as the two previous sessions. | four omissions |

Verified without discrepancy (not tabled): every Layer 1, Layer 2 and Layer 3
script row's reason line and result-file status; `Dockerfile.p4` lines
(override, rounds, no `EACCES`, test result, `docker top` uid 65532 and the
`/fah-p4` to `/fah-probe` spawn); splicebench five splice / one origin / pick /
five counters lines; `layer3-p3/run.log` and `p3.json` for F29 (barrier MET on
both runs, `RESOLVED`, `connections_delta: 2`, `dns_flat: true`, issuer, `h2`,
`authorized`); F25 / F26 lines in `layer3-p4-run.log`; F28 lines in
`layer3-l1a/run.log`; `tip=92e3f4a9b930-dirty` on every `run.log` header
with `HEAD` = `92e3f4a9…` and exactly the two `phase2.6` files modified; idle
check PASS on every Layer 1 / 3 log; F18 fixed (`r4b-allowbusy` INVALID line),
F20 fixed (`layer3-l1a` p7 with `spa_shell` rows and `failing: []`), F19 fixed
for `p4-lan.mjs`, F7 cleared (`84d34be` in history, barrier met), F15 / F21 /
F22 unchanged (`Dockerfile.fahprobe:68` still plain `--release`), F16 / F17
quotes match the 1557Z file. All seven findings carry the seven fields the
previous sessions use; classifications fit.

143 claims checked, 14 discrepancies.

## Check C — campaign readiness

- **P3 — blocked (both arms).** No campaign origin: the plan requires `alpn h2`
  and exactly 8 MiB, proven from the driving host and recorded; this session
  has one 5 MiB origin on a one-off approval and no file behind the preflight
  (F24, Check B rows 3–4). Owner / agent step: choose, preflight from the
  device's network, save the one-liner output as a file. Item 3 of "Still
  needed" is a real blocker.
- **P4 — blocked (in-device row only).** F25 (DoT step in the
  `encrypted_latency` harness client, per F28's attribution) and F26 (harness
  DoH over HTTP/1.1) sit on the row P4 exists to set; both are code / harness
  triage, not smoke work. Item 4 is a real blocker for P4. P4-LAN is not
  affected (`p4-lan` valid in Layer 1 and Layer 3).
- **P2 — blocked by environment.** The three-arm run has never executed
  anywhere (F15 unchanged); the wired bridged VM with two verified addresses
  is an owner-supplied precondition. Owner step, but the P2 stage cannot start
  without it.
- **SNI — doc step before the stage runs.** `p0-sni.mjs` asserts two INVALID
  rules and one gate term the frozen plan lacks (V6). The plan's own rule —
  "the script follows the plan, never the reverse" — wants a numbered delta or
  an SNI row in §Invalidity rules, an `.md` edit the owner approves.
- **Owner steps, not blockers:** item 1 clean commit (every `run.log` is
  `-dirty`; the plan forbids starting so); item 2 boot keys + restart (router
  write, propose only); item 5 p3-04 note for F7; item 6 smoke-plan
  `MSYS2_ARG_CONV_EXCL` lines — recommended, because the campaign drives from
  this Git Bash box and the mangled-path INVALID reads as an F7 reproduction
  (F23).
- **Not blocking:** P1 (both arms valid in Layer 1; the Layer 3 control is a
  Docker Desktop limit, F21), P5, P6, P7 (clean in both layers). Layer 2's
  "barrier not met" rule, never runnable on this box, was exercised on the
  pre-fix tree in session 1303Z (F7: `barrier NOT MET (5/64 …)`), so the code
  path is covered.
- **Attention, not a blocker:** the Layer 3 P3 RSS gate line reads "above — a
  finding, not a fail" on x86 Docker Desktop. Not a result, but the campaign's
  first on-device P3 RSS reading may become a p3-04 finding; the review file
  should have a place for it.

## Findings

| id | severity | file:line | what | why it matters | fix before campaign |
| --- | --- | --- | --- | --- | --- |
| V1 | major | `smoke-20260903T1844Z/SMOKE-REPORT.md` Layer 3 `Dockerfile.fahprobe` row; `FINDINGS.md` F27 | The quoted container-boot lines and F27's whole observed block are in no file; `layer3-fahprobe-boot.log` is a 12-line partial capture of one boot with `all upstreams failed` | The Layer 3 image pass condition and an environment finding rest on text only; the owner cannot re-derive either from the directory | no (recapture `docker logs` for both boots when the image is next run) |
| V2 | major | `SMOKE-REPORT.md` §P3 public-origin preflight; `FINDINGS.md` F24 | "Full preflight output is in `layer3-p3-preflight.log`" is false (two `sabnzbd.org` lines); six origins, the `openssl` output and the "previous session recorded it" claim have no file | The P3 origin decision — a campaign blocker — has no reproducible evidence, and F24's "reappearance" is unsupported by F1–F22 | yes — the campaign origin's preflight must be saved as a file before P3 |
| V3 | minor | `SMOKE-REPORT.md` Layer 1 `p3` row and tally | Row labelled "valid for this layer", counted among "8 valid"; `layer1-b/p3.json` is `INVALID` | A row's result contradicts its file; the plan itself expects INVALID here | no |
| V4 | minor | `SMOKE-REPORT.md` §Verdict row 3; `FINDINGS.md` F25, F28, F29 notes | Numbers stated as comparisons (cross-session DoT p50 / min, DoT vs UDP columns, stall vs control delta) | Breaks the files' own rule and the smoke plan's "no number is cited or compared"; invites reading smoke output as a measurement | no (strip to shape statements when the files are next edited) |
| V5 | minor | `SMOKE-REPORT.md` §Deviations | Omits `--bytes 5` vs the required 8 MiB, the widened `egress.allow_destinations` and bridge origin, the "(all)" rows run for `p0` only, and the report file itself | §Deviations is the owner's checklist for what departed from the smoke plan | no |
| V6 | minor | `p0-sni.mjs:165,172,208` vs `p3-06-testing-plan.md` §Invalidity rules / §Gate statistic | Two INVALID rules and the no-SNI gate term are undeclared in the frozen plan (no SNI row); with `no_sni = "pass"` the no-SNI close is not a verdict | The plan says a script never leads the plan; the SNI gate as printed differs from the gate as declared | yes — numbered delta or §Invalidity SNI row (owner `.md` edit) before the SNI stage |
| V7 | nitpick | `lib.mjs:317`, `lib.mjs:228`, `lib.mjs:248` | An INVALID decision (`engine.mode`) and two single-stage helpers (`dnsParse`, `localAddressToward`) live in shared code | Outside the declared lib boundary; harmless today | no |
| V8 | nitpick | `p4-lan.mjs:223` | 98 % answered budget ⇒ degraded, undeclared for P4-LAN | Delta 11 names P1-LAN, P2, P3 throughput only; no gate here, so no effect | no |
| V9 | nitpick | `p2-handshake.mjs:138` | No ALPN offered, so the plan's "ALPN recorded" is always `null` | The origin descriptor the plan wants is incomplete | no (offer `['http/1.1']` or record "none offered") |
| V10 | nitpick | `p1-lan.mjs:146,164` | `authorized` checked on connection 0 only in the aggregate arm | A failed verification on connections 1–7 goes unnoticed; the aggregate arm is diagnostic | no |
| V11 | nitpick | `SMOKE-REPORT.md` Layer 2 row 1 and tally; Layer 1 `p3`, `p6`; Layer 3 splicebench; `FINDINGS.md` F23 files, F24 repro / count | Quoting slips: `<key or file>` vs `<key\|file>`; RSS reason truncated without marker; 21 vs 22; `docker top` from another run; "differs from `smoke-ca.pem`" no longer true on disk; F23 says the lines were overwritten though `run.log` keeps them; F24 seven vs ten origins, `h2pre.mjs` vs `h2-preflight.mjs` | Verbatim quoting is the whole basis of a smoke report | no |
| V12 | nitpick | `p6-certs-time.mjs:100` | Asserts `api_certificate.source == "imported"` right after import; the plan puts that check owner-side after restart | Undeclared assertion, harmless | no |

PASS WITH DEFERRED FINDINGS
