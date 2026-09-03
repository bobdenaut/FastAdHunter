# p3-06 probe smoke — report (smoke-20260903T1303Z, bobdenaut)

Companion to [FINDINGS.md](FINDINGS.md), which carries only the 13 issues.
This file carries the per-row outcome of every row in
`plan/wip/phase3/p3-06-smoke-plan.md`, including the rows that passed.

**Nothing here is a measurement.** Pass conditions are structural (valid /
INVALID / degraded, reasons, counters, issuers). Reason lines are quoted
verbatim as evidence; no number in them is a result, a budget or a
comparison, and none may be copied into `p3-06-testing-results.md`.

Tip on every row: `68b7f36f9817-dirty` (smoke plan Layer 2 last row — the
campaign must not start in this state).

## Layer 1 — local FastAdHunter, same scripts

| Script | Command beyond the common flags | Result | Reason line | Finding |
| --- | --- | --- | --- | --- |
| `p0-sni.mjs` | `--port 8444 --blocked ads.smoke.test --allowed example.com` | INVALID, then **valid** | `INVALID: allowed name did not reach ServerHello through the probe` → after the upstream deviation: `GATE {…"blocked_closed":true,"no_sni_closed":true,"allowed_opened":true,"pass":true}` | F1 |
| `p1-origin.mjs` | `--cert p1.crt --key-file p1.key --port 4443 --bytes 8` | **valid** | `p1-origin listening on 0.0.0.0:4443, 8 MiB per connection, cert p1.crt` | — |
| `p1-origin.mjs` (port precondition) | second instance while the first held `:4443` | INVALID as designed | `INVALID: listen failed: EADDRINUSE (an existing listener on :4443? plan §Running: Get-NetTCPConnection -LocalPort 4443 -State Listen must be empty)` | — |
| `p1-lan.mjs` P1-control | `--origin-port 4443 --origin-cert p1.crt --bytes 8 --direct` | **valid** | `GATE {"statistic":"none — this arm is the band",…}`; 5 runs, `steady_mib_s` present on each | — |
| `p1-lan.mjs` spliced | `--origin-cert p1.crt --bytes 8 --port 8444` | INVALID, then **valid** with `clients = []` | `INVALID: no spliced sample completed 8 MiB on every connection` (`upstream_cert_failures` 5) → `served_issuer: "127-0-0-1.nip.io"` = the origin's CN, `inside_band: true` | F2, F3 |
| `p1-lan.mjs` aggregate | `--connections 8` | degraded | `degraded: aggregate arm without a /tool/profile share (--profile-share): throughput of the loop only`; `p1-aggregate.json` written | — |
| `p2-handshake.mjs` Linux arm | — | **not run** | no Linux host can reach a loopback-bound probe | F4 |
| `p2-handshake.mjs` Windows negative path | `--listed 127.0.0.1 --unlisted 127.0.0.2 --rounds 10` | INVALID as the plan predicts | `ip -4 -o addr: {"error":"spawnSync ip ENOENT"}` then `INVALID: identity precondition: both --listed and --unlisted must be on this host's interfaces (found none)` | — |
| `p3-h2stall.mjs` throughput arm | `--path /8mib --warmup-path / --streams 64 --runs 1 --settle 5 --throughput-runs 1` | **ok** | `throughput run 1: … bytes=8388608`, `"pass":true` | — |
| `p3-h2stall.mjs` RSS arm | same | crash, then INVALID | `INVALID: RSS arm: 1 run(s) produced no figure: stall#1 barrier not met: 5/64 streams answered` | F5, F6, F7, F8 |
| `p4-lan.mjs` | `--domain ads.smoke.test --dot-host dns.smoke.test --dns-port 5300 --dot-port 8853 --queries 200 --rounds 1` | **valid** | all three transports `n=200 unanswered=0 unmatched=0`; `first_answer {"rcode":0,"answers":["0.0.0.0"]}`; dot `served_issuer ["FastAdHunter CA"]` | — |
| `p5-mint.mjs` | `--dot-port 8853 --hosts 16` | **valid** | `leaf_cache delta whole run {…"minted_total":16,…"evictions":0…}`; both arms `issuers=FastAdHunter CA`; `"pass":true` | — |
| `p6-certs-time.mjs` | `--generate 2 --import 2 --cert p1.crt --key-file p1.key --ca-archive-count 0 --api-archive-count 0` | **valid** | four rows `200`; `api_certificate={"source":"imported"}`; `ca-after-p6.pem` written, differs from `smoke-ca.pem` | — |
| `p7-store.mjs` | `--ca-key smoke-config/ca-key.pem` | **valid** | `GATE {"statistic":"every check pass","pass":true,"failing":[]}`; 4 checks PASS, 20 paths / 40 requests, no `/config/*` or `/data/*` `200` | — |

## Layer 2 — negative paths, one per rule

Rows are the smoke plan's table order. `--skip-host-checks` from row 2.4
onward (see F12).

| Rule (script) | Result | Reason line | Finding |
| --- | --- | --- | --- |
| no API key (all) | **pass** | `INVALID: no API key: --key <key\|file> or FAH_PROBE_KEY` — before any request | — |
| `/health` unreachable (all) | **pass** | `GET /health -> 0 Error: connect ECONNREFUSED 127.0.0.9:8443` then `INVALID: probe /health answered 0` | — |
| `engine.mode` without https | **pass** | `INVALID: engine.mode dns carries no https listener`; `p6` and `p7` still ran `valid` | — |
| busy host — `INVALID` half | **pass** | `INVALID: host not idle: chrome running (plan §Running item 3); --allow-busy to proceed degraded` | F12 |
| busy host — `--allow-busy` → `degraded` half | **not run** | browser off limits by owner instruction | F12 |
| allowed name does not open (`p0`) | **pass** | `INVALID: allowed name did not reach ServerHello through the probe; the listener closing everything proves nothing about blocked rows` | — |
| origin outside `egress.allow_destinations` (`p1`) | **pass** with a fresh `--out` | `INVALID: origin address 127.0.0.1 is not in egress.allow_destinations: the probe refuses the destination (plan §Environment)` | F9 |
| origin byte count wrong (`p1`) | **pass** | `INVALID: no p1_control sample completed 16 MiB on every connection` | — |
| identity precondition (`p2`, Windows) | **pass** | `INVALID: identity precondition: both --listed and --unlisted must be on this host's interfaces (found none)` | — |
| both addresses unlisted (`p2`, Linux) | **not run** | no Linux host | F13 |
| this host not listed (`p3`) | **pass** | `INVALID: this host (127.0.0.1) is not in https.interception.clients: the terminate leg is not reachable from here` | — |
| barrier not met (`p3`) | **row premise wrong** | `stall run 1: barrier MET (64/64 streams with :status 200 + first DATA, met)`; run ended `degraded` on the byte check instead | F11 |
| no CA in the store (`p5`) | **pass** | `INVALID: no CA in the probe store: nothing would be minted (POST /api/v1/certificates/ca/generate first)` | — |
| cache headroom (`p5`) | **pass** | `INVALID: leaf_cache.size 0 + 600 hosts exceeds capacity 512: the repeat pass would evict` | — |
| archive cap (`p6`) — precheck half | fires, but off by one | `INVALID: ca-archive holds 0; 9 generates would pass the cap of 8` — before any call | F10 |
| archive cap (`p6`) — 409 half | **row premise wrong** | `ca/generate #9: 200`; `ca-archive` holds 8 | F10 |
| `--ca-key` unreadable / not a key (`p7`) | **pass** | `INVALID: --ca-key unreadable: ENOENT…`, then `INVALID: --ca-key is not a private key: error:1E08010C:DECODER routines::unsupported` | — |
| failure-rate budget (`p1`, delta 11) | **pass** | `degraded: 5 of 10 runs incomplete (50% over 2%); figures from the 5 completed runs`; `== p1 done (degraded)` | — |
| dirty checkout (all) | **pass** | `tip=68b7f36f9817-dirty` on every row | — |

## Layer 3 — the three images

**Not run.** The owner stopped the session after the JavaScript rows. No
`Dockerfile.p4`, `Dockerfile.splicebench` or `Dockerfile.fahprobe` build was
attempted, x86 or arm64; the uid, `/tmp`, harness-spawn, distroless-entrypoint
and binary-rename checks are all still open.

## Deviations from the smoke plan

Each one was forced by an environment or plan defect, and each is recorded in
FINDINGS.md. Methodology, workloads, gate thresholds, statistics and the
candidate matrix are untouched.

| Deviation | Rows affected | Why | Finding |
| --- | --- | --- | --- |
| upstream `1.1.1.1:53` → `192.168.10.1:53` in the boot TOML | every row after 1.1 | this LAN blocks DNS straight out; nothing public resolved | F1 |
| `https.interception.clients` toggled `["127.0.0.1"]` ↔ `[]` between rows | p1 spliced, p3, Layer 2 rows 2.6 / 2.10 / 2.11 | loopback has one client address; p1 needs it unlisted, p3 and p5 need it listed | F2 |
| second origin certificate `p3.crt` with `CA:FALSE` + EKU serverAuth | p3 | the plan's `openssl req -x509` one-liner is `CA:TRUE`, which rustls rejects as an end-entity certificate | F6 |
| smoke h2 origin raised to `maxSessionMemory: 4096`, `maxConcurrentStreams: 256` | p3 | ruling the origin's own limits out before attributing the barrier failure to the probe | F7 |
| per-row `--out` subdirectories under `layer2` | every config-changing Layer 2 row | the shared directory's cached `config.json` made preconditions fire on stale state | F9 |
| `--skip-host-checks` from row 2.4 onward | Layer 2 rows after 2.4 | owner asked that the browser not be closed or started by the agent | F12 |
| second config store `smoke-config2` / `smoke-data2` | Layer 2 rows 2.12, 2.14b, 2.11 | those rows need a store with no CA and an empty `ca-archive` | — |
| failure-rate row run at 10 runs × 1024 MiB instead of 5 × 8 MiB | Layer 2 failure-rate row | 8 MiB runs finish in ~13 ms each, too fast to kill the origin mid-run | — |

## Artefacts

| Path | Holds |
| --- | --- |
| `layer1/` | `sni.json`, `p1-control.json`, `p1.json`, `p1-aggregate.json`, `p2.json`, `p3.json`, `p4-lan.json`, `p5.json`, `p6.json`, `certs.json`, `ca-after-p6.pem`, `h2-origin.mjs`, `run.log`, `raw.jsonl`, `config.json` |
| `layer1-f1/` | the F1 run's artefacts, kept so the first `p0` failure survives the re-run |
| `layer1-f2/` | the F2 run's `p1.json`, kept for the same reason |
| `layer2/` | shared-directory rows plus one subdirectory per config-changing row (`r3-mode`, `r6-egress`, `r10-notlisted`, `r11-tiny`, `r12-noca`, `r13-headroom`, `r14-cap`, `r14b-409`, `r15a`, `r15b`, `r16-failrate`, `r16b-failrate`) |
| `layer3/` | empty — Layer 3 not run |
| `fah-*.log` | probe stdout per boot: `fah-boot`, `fah-boot2`, `fah-boot-l2`, `fah-boot-dnsonly`, `fah-boot-noclients`, `fah-boot-fresh`, `fah-harness-boot`, `fah-harness-debug`, `fah-harness-r11` |
| `p1-origin-*.log`, `h2-origin-*.log`, `r16*-stdout.log` | origin and background-run stdout |

## Still needed before the campaign

1. **Owner** — a clean commit of the scripts and Dockerfiles. Every row above
   ran at `68b7f36f9817-dirty`, so no run here is reproducible.
2. **Owner** — the probe's three boot keys plus a restart on the router
   (router write; the agent proposes, never runs).
3. Layer 3, whenever the owner wants it — the three x86 images are unrun.
