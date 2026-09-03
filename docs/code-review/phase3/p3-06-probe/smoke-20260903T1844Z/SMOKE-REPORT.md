# p3-06 probe smoke — report (smoke-20260903T1844Z, bobdenaut)

Tip `92e3f4a9b930a152a03186f790650a71233e24a7`, recorded by every `run.log` as
`92e3f4a9b930-dirty`: the owner's two uncommitted `docs/code-review/phase2.6`
files were present all session and approved to run over. Nothing else in the
tree was dirty at the start.

**Nothing in this run is a measurement.** No number below or in any
`<name>.json` under this directory is a result, a budget or a comparison, and
none may be cited in `p3-06-testing-results.md`. Pass conditions here are
structural — `valid` / `INVALID` / `degraded`, reasons, counters, issuers.

Clean rerun of [p3-06-smoke-plan.md](../../../../plan/wip/phase3/p3-06-smoke-plan.md)
on the tree as committed after smoke-20260903T1303Z (F1–F13) and
smoke-20260903T1557Z (F14–F22). Findings continue at F23 in
[FINDINGS.md](FINDINGS.md).

Host: Windows 11 IoT Enterprise LTSC 2024, Node 26.7.0, Docker Desktop, Git
Bash. Chrome was closed for Layers 1 and 3 (idle check `PASS`) and reopened by
the owner for Layer 2 row 4 only.

## Layer 1 — local FastAdHunter, same scripts

Common flags: `--probe 127.0.0.1 --api-port 8443 --key smoke-config/apikey`.
Posture A (`clients = []`) writes to `<root>/layer1-a`; posture B
(`clients = ["127.0.0.1"]`) writes to `<root>/layer1-b`.

| Script | Command beyond the common flags | Result | Reason line | Finding |
| --- | --- | --- | --- | --- |
| `p0-sni.mjs` | `--port 8444 --blocked ads.smoke.test --allowed example.com` | valid | `GATE {"statistic":"every blocked and no-SNI attempt closed before a certificate (boolean)","blocked_closed":true,"no_sni_closed":true,"allowed_opened":true,"pass":true}`; `listeners.https delta {…"blocked":5…}` | — |
| `p1-lan.mjs` (P1-control) | `--direct --origin 127-0-0-1.nip.io --origin-port 4443 --origin-cert p1.crt --bytes 8 --port 8444` | valid | `GATE {"statistic":"none — this arm is the band","band":{"min":208.937,"p50":1151.576,"max":1331.78}}`; 5 runs, `steady_mib_s` present | — |
| `p1-lan.mjs` (spliced) | `--origin 127-0-0-1.nip.io --origin-cert p1.crt --bytes 8 --port 8444` | valid | `p1.json` `"served_issuer": "127-0-0-1.nip.io"` = the origin's CN; `== p1 done (valid)` | — |
| `p1-lan.mjs` (aggregate) | as above plus `--connections 8` | degraded | `degraded: aggregate arm without a /tool/profile share (--profile-share): throughput of the loop only`; `p1-aggregate.json` written | — |
| `p2-handshake.mjs` (Windows negative path) | `--out <root>/layer1-a-p2win --listed 192.168.10.10 --unlisted 192.168.10.11 --origin example.com --ca smoke-ca.pem --rounds 10 --dns-port 5300 --port 8444` | INVALID (the expected negative path) | `ip -4 -o addr: {"error":"spawnSync ip ENOENT"}` then `INVALID: identity precondition: both --listed and --unlisted must be on this host's interfaces (found none)` | — |
| `p2-handshake.mjs` (three-arm Linux run) | — | not run | no bridged VM was made available; the owner chose the Windows negative path for this session | F15 (previous session), unchanged |
| `p3-h2stall.mjs` | posture B, harness binary with `FAH_TEST_UPSTREAM_ROOT=E:/FastAdHunter/p3.der`, local `smoke/h2-origin.mjs` on `:443`; `--origin 127-0-0-1.nip.io --path /8mib --warmup-path / --ca smoke-ca.pem --port 8444 --streams 64 --runs 1 --settle 5 --throughput-runs 1` | valid for this layer | throughput `[p3] throughput run 1: 163.436 MiB/s bytes=8388608`, `ok: true`; RSS arm `INVALID: RSS arm: 2 run(s) produced no figure: stall#1 process_rss is null on this probe (kernel reading unavailable, e.g. Windows): the RSS arm is measurable in-container / on-device only` — the plan's stated correct outcome on Windows | F23 (first attempt, shell-mangled `--path`) |
| `p4-lan.mjs` | `--domain ads.smoke.test --dot-host dns.smoke.test --dns-port 5300 --dot-port 8853 --queries 200 --rounds 1` | valid | all three transports `n=200 unanswered=0 unmatched=0`, `first_answer.answers ["0.0.0.0"]`, dot `served_issuer ["FastAdHunter CA"]` | — |
| `p5-mint.mjs` | posture B, `--dot-port 8853 --hosts 16` | valid | `leaf_cache delta … "minted_total":16 … "evictions":0`; both arms `issuers=FastAdHunter CA` | — |
| `p6-certs-time.mjs` | `--generate 2 --import 2 --cert p1.crt --key-file p1.key --ca-archive-count 0 --api-archive-count 0` | valid | four rows `200`; `api_certificate={"source":"imported"}`; `ca-after-p6.pem` written and differs from `smoke-ca.pem` | — |
| `p7-store.mjs` | `--ca-key smoke-config/ca-key.pem` | valid | `GATE {"statistic":"every check pass","pass":true,"failing":[]}`; `served:{"spa_shell":0,"other_200":0,"rejected":40}` — this host build ships no `/web` | — |

Layer 1: **8 valid, 1 degraded, 1 INVALID-by-design (p2 on Windows), 1 not run.**

## Layer 2 — negative paths, one per rule

State restored after every config-changing row. Each row writes to
`<root>/layer2/<row>`.

| Rule (script) | Command beyond the common flags | Result | Reason line | Finding |
| --- | --- | --- | --- | --- |
| no API key (`p0`) | `--key` omitted, `FAH_PROBE_KEY` unset | INVALID (expected) | `INVALID: no API key: --key <key or file> or FAH_PROBE_KEY` — printed before any request | — |
| `/health` unreachable (`p0`) | `--probe 127.0.0.9` | INVALID (expected) | `GET /health -> 0 Error: connect ECONNREFUSED 127.0.0.9:8443` then `INVALID: probe /health answered 0` | — |
| `engine.mode` without https — `p0` | `mode = "dns"`, restart | INVALID (expected) | `INVALID: engine.mode dns carries no https listener` | — |
| … `p1-lan` | same restart | INVALID (expected) | `INVALID: engine.mode dns carries no https listener` | — |
| … `p2-handshake` | same restart | INVALID (expected) | `INVALID: engine.mode dns carries no https listener` | — |
| … `p3-h2stall` | same restart | INVALID (expected) | `INVALID: engine.mode dns carries no https listener` | — |
| … `p4-lan` | same restart | INVALID (expected) | `INVALID: engine.mode dns carries no https listener` | — |
| … `p5-mint` | same restart | INVALID (expected) | `INVALID: engine.mode dns carries no https listener` | — |
| … `p6` still runs | same restart | valid | four rows `200`, `== p6 done (valid)` | — |
| … `p7` still runs | same restart | valid | `GATE {"statistic":"every check pass","pass":true,"failing":[]}` | — |
| busy host, no flag (`p0`) | Chrome open (owner) | INVALID (expected) | `idle check: FAIL chrome` then `INVALID: host not idle: chrome running (plan §Running item 3); --allow-busy to proceed degraded` | — |
| busy host, `--allow-busy` (`p0`) | `--allow-busy`, seeded store | degraded (expected) | `degraded: host not idle: chrome`; `GATE (degraded, not a gate) {…"pass":true}`; `== sni done (degraded)` | — |
| allowed name does not open (`p0`) | `--allowed ads.smoke.test` | INVALID (expected) | `INVALID: allowed name did not reach ServerHello through the probe; the listener closing everything proves nothing about blocked rows` | — |
| origin outside `egress.allow_destinations` (`p1`) | `allow_destinations = []`, restart | INVALID (expected) | `INVALID: origin address 127.0.0.1 is not in egress.allow_destinations: the probe refuses the destination (plan §Environment)` | — |
| origin byte count wrong (`p1`) | `--direct --origin-port 4443 --bytes 16` against an 8 MiB origin | INVALID (expected) | `errors=[{"index":0,"error":"closed_early","bytes":8388608}]` then `INVALID: no p1_control sample completed 16 MiB on every connection` | — |
| identity precondition (`p2`) | run on the Windows dev box | INVALID (expected) | `INVALID: identity precondition: both --listed and --unlisted must be on this host's interfaces (found none)` | — |
| both addresses unlisted (`p2`, Linux) | — | not run | no bridged VM; the plan marks this row Linux-only | F15 (previous session), unchanged |
| this host not listed (`p3`) | `clients = []`, restart | INVALID (expected) | `local address toward the probe: 127.0.0.1; https.interception.clients = []` then `INVALID: this host (127.0.0.1) is not in https.interception.clients: the terminate leg is not reachable from here` | — |
| barrier not met (`p3`) | — | not run | the plan's own text: "**Not runnable on this dev box** (F8 + F22)". Still true on this tip: `/stall` lives only in `smoke/h2-origin.mjs`, a locally-minted origin the release `fah-probe` image cannot trust, and the public origin used in Layer 3 serves no stalling path | F22 (previous session), unchanged |
| no CA in the store (`p5`) | fresh `smoke2-config`, no `ca/generate` | INVALID (expected) | `certificates before: ca.present=false` then `INVALID: no CA in the probe store: nothing would be minted (POST /api/v1/certificates/ca/generate first)` | — |
| cache headroom (`p5`) | `--hosts 600` | INVALID (expected) | `INVALID: leaf_cache.size 0 + 600 hosts exceeds capacity 512: the repeat pass would evict` | — |
| archive cap, declared (`p6`) | fresh store, `--generate 10 --ca-archive-count 0` | INVALID (expected) | `INVALID: ca-archive holds 0; 10 generates would add 9 (the first on an empty store archives nothing) and pass the cap of 8` — before any call | — |
| archive cap, counts omitted (`p6`) | fresh store, `--generate 10` | INVALID (expected) | nine calls `200`, then `ca/generate #10: 409` and `INVALID: ca/generate #10 answered 409: {"error":{"code":"conflict","message":"archive_full: ca-archive already holds 8 retired pairs; move some out of /config before replacing this pair"}}` — F10's arithmetic confirmed | — |
| `--ca-key` unreadable (`p7`) | `--ca-key nope.pem` | INVALID (expected) | `INVALID: --ca-key unreadable: ENOENT: no such file or directory, open 'E:\FastAdHunter\nope.pem'` | — |
| `--ca-key` not a key (`p7`) | `--ca-key p1.crt` | INVALID (expected) | `INVALID: --ca-key is not a private key: error:1E08010C:DECODER routines::unsupported` | — |
| failure-rate budget (`p1`) | spliced arm, `--bytes 512` against a 512 MiB origin, origin killed mid-run | degraded (expected) | `degraded: 2 of 5 runs incomplete (40% over 2%); figures from the 3 completed runs`; `== p1 done (degraded)` | — |
| dirty checkout (all) | the owner's two `phase2.6` files | as expected | every `run.log` header carries `tip=92e3f4a9b930-dirty` | — |

Layer 2: **21 rows fired exactly the INVALID or degraded the plan predicts,
2 valid (`p6` and `p7` under `mode = "dns"`), 2 not run.** No row failed to
fire, and no row was blocked by a script defect.

`--bytes 512` on the failure-rate row is a deviation from nothing the plan
fixes: the plan says only "kill `p1-origin.mjs` after run 3 of 5", and at the
plan's 8 MiB a loopback run finishes in about 20 ms, far too fast to interrupt.
The first attempt at 8 MiB hit the byte-count rule instead, because the
origin's payload size is fixed when the origin starts.

## Layer 3 — the three images, x86 Docker on the dev box

All three built at this tip with `--platform linux/amd64 --load`; build log in
`build-images.log`.

| Image | Command | Result | Reason line | Finding |
| --- | --- | --- | --- | --- |
| `Dockerfile.p4` | `docker run --rm fah-p4:smoke` | valid (row passes) | `FAH_E2E_BINARY override active: /fah-probe`; `3 rounds x 2000 sequential queries per transport`; no `EACCES` or `Permission denied`; `test result: ok. 1 passed; 0 failed`. `docker inspect` shows `User: 65532:65532`; `docker top` shows `/fah-p4 …` spawning `/fah-probe …` | F25, F26 |
| `Dockerfile.splicebench` | `docker run --rm fah-splicebench:smoke --reps 1 --size-mib 8` | valid | five `rep=1 arm=splice` lines, one `rep=1 arm=loopback_origin` line, the candidate table, `pick: none — no in-budget candidate reaches 0.9 x best 6741.4`, five `counters …` lines; `docker top` shows `/fah-splicebench --reps 3 --size-mib 64` | — |
| `Dockerfile.fahprobe` | `docker run -d … -v "E:\FastAdHunter\smoke3-config:/config" -v "E:\FastAdHunter\smoke3-data:/data" -p 8443:8443 -p 8444:8444 -p 8853:853 -p 5300:53/udp fah-probe:smoke` | valid | `fastadhunter starting config_path=/config/fastadhunter.toml data_dir=/data mode=DnsHttpHttps`; `dropped privileges after binding uid=65532 gid=65532`; `list refreshed list=oisd-basic active=62948`; `docker top` shows `/fah-probe` | F27 |

Layer 1 rows re-run against the `fah-probe` image, key `smoke3-config/apikey`,
writing to `<root>/layer3-l1a`, `<root>/layer3-l1b` and `<root>/layer3-p3`:

| Script | Command beyond the common flags | Result | Reason line | Finding |
| --- | --- | --- | --- | --- |
| `p0-sni.mjs` | `--port 8444 --blocked ads.smoke.test --allowed example.com` | valid | `listeners.https delta {…"blocked":5…}`; `GATE {…"blocked_closed":true,"no_sni_closed":true,"allowed_opened":true,"pass":true}` | — |
| `p1-lan.mjs` (spliced) | origin container at `172.28.0.3` on a user bridge; `--origin 172-28-0-3.nip.io --origin-cert p3c.crt --bytes 8 --port 8444` | valid | 5 runs `ok=1/1`; `listeners.https delta {"connections":5,"requests":5,…"upstream_cert_failures":0…}`; `== p1 done (valid)` | — |
| `p1-lan.mjs --direct` (P1-control) | — | not run | the plan's own row: "Docker Desktop on Windows does not route the bridge subnet to the host … Record as not run; the arm has real LAN addresses on the RB5009" | F21 (previous session), unchanged |
| `p4-lan.mjs` | `--domain ads.smoke.test --dot-host dns.smoke.test --dns-port 5300 --dot-port 8853 --queries 200 --rounds 1` | valid | three transports `n=200 unanswered=0 unmatched=0`; `GATE {"statistic":"none — diagnostic","dot_p50_minus_udp_p50_ms":-0.017,"doh_p50_minus_udp_p50_ms":0.136}` — **F19 fixed**, the DoT column now sits with UDP and DoH | F28 |
| `p5-mint.mjs` | `--dot-port 8853 --hosts 16` | valid | `"minted_total":16 … "evictions":0`; both arms `issuers=FastAdHunter CA` | — |
| `p6-certs-time.mjs` | `--generate 2 --import 2 --cert p1.crt --key-file p1.key --ca-archive-count 0 --api-archive-count 0` | valid | four rows `200`; `api_certificate={"source":"imported"}` | — |
| `p7-store.mjs` | `--ca-key smoke3-config/ca-key.pem` | valid | `PASS traversal list against the API listener {"paths":20,"requests":40,"served":{"spa_shell":24,"other_200":0,"rejected":16},"failing":[]}`; `GATE {…"pass":true,"failing":[]}` — **F20 fixed**: this image ships `/web`, 24 requests hit the SPA shell, and the predicate now asserts needles rather than status | — |
| `p3-h2stall.mjs` (throughput and RSS) | `--origin sabnzbd.org --path /tests/internetspeed/5MB.bin --warmup-path / --ca smoke3-ca.pem --port 8444 --bytes 5 --streams 64 --runs 1 --settle 5 --throughput-runs 1` | valid | `throughput run 1: 11.348 MiB/s bytes=5242880`; `stall run 1: barrier MET (64/64 streams with :status 200 + first DATA, met)`; `control run 1: barrier MET (64/64 …)`; `"attribution":"RESOLVED"` — **F7's 64-stream stall barrier is met for the first time** | F24, F29 |

### P3 public-origin preflight (required by the plan, recorded here)

`speed.cloudflare.com` would not negotiate ALPN h2 from this box — the same
condition the previous session recorded, filed again as F24. Seven origins
were preflighted with the plan's own one-liner:

| Origin | Path | Preflight line |
| --- | --- | --- |
| speed.cloudflare.com | `/__down?bytes=8388608` | `alpn false stream-error ERR_HTTP2_ERROR` |
| cloudflare.com | `/cdn-cgi/trace` | `alpn h2 status 200 content-length 231 bytes 231` |
| speedtest.london.linode.com | `/100MB-london.bin` | `alpn false stream-error ERR_HTTP2_ERROR` |
| ash-speed.hetzner.com | `/100MB.bin` | `error ERR_SSL_TLSV1_ALERT_NO_APPLICATION_PROTOCOL` |
| scaleway.testdebit.info | `/10M.iso` | `alpn h2 status 200 content-length 10000000 bytes 10000000` — decimal MB, not an integer MiB |
| cachefly.cachefly.net | `/10mb.test` | `alpn h2 status 200 content-length undefined bytes 7` |
| **sabnzbd.org** | **`/tests/internetspeed/5MB.bin`** | **`alpn h2 status 200 content-length undefined bytes 5242880`** — exactly 5 MiB |

`sabnzbd.org` was used, at `--bytes 5`, with the owner's explicit approval for
the 64-stream outbound burst. `p3-h2stall.mjs:253` skips the length check when
`content-length` is absent, so the missing header does not weaken the row; the
byte count was verified in the preflight above and again per stream in the run.
Full preflight output is in `layer3-p3-preflight.log`.

Layer 3: **10 valid, 0 INVALID, 0 degraded, 0 BLOCKED BY SCRIPT, 1 not run.**

## Deviations

1. **Git Bash path conversion.** Every command carrying a bare `/path`
   argument, and both `docker run -v host:container` specs, had to be prefixed
   with `MSYS2_ARG_CONV_EXCL='*'`. Without it MSYS rewrites the argument before
   Node or Docker sees it — silently, and in the Docker case with the container
   still starting and answering `/health` on the wrong config. F23 and F27.
2. **Failure-rate row at `--bytes 512`**, not the plan's 8 MiB — see the Layer 2
   note. No methodology, threshold or statistic changed; the row is a negative
   path and nothing in it is measured.
3. **`p2-handshake.mjs` three-arm run not attempted.** The owner confirmed no
   bridged VM this session; the Windows negative path ran in its place.
4. **The Layer 3 `p3` origin is `sabnzbd.org`, not `speed.cloudflare.com`** —
   see the preflight table and F24.
5. **The container's upstream is `127.0.0.11:53`** (Docker's embedded
   resolver), not the LAN resolver: this LAN blocks outbound DNS and
   `192.168.10.1:53` is not reachable from the bridge. This is a scratch config
   authored for this session; no committed probe config or Dockerfile was
   changed.
6. **The Layer 3 `clients` list.** For the `p3` rows the container's config
   lists `["127.0.0.1", "172.28.0.1"]` — `127.0.0.1` because that is the local
   address `p3-h2stall.mjs` sees toward a published port, `172.28.0.1` because
   that is the peer address the probe actually sees. The plan names only
   `127.0.0.1`.

## Artefacts

All untracked, under `docs/code-review/phase3/p3-06-probe/smoke-20260903T1844Z/`:

- `layer1-a/`, `layer1-b/`, `layer1-a-p2win/` — Layer 1, both postures
- `layer2/<row>/` — one directory per Layer 2 row
- `layer3-l1a/`, `layer3-l1b/`, `layer3-p3/` — Layer 1 rows against the image
- `layer3-p4-run.log`, `layer3-splicebench-run.log`, `layer3-fahprobe-boot.log`
- `layer3-p4-top.log`, `layer3-splicebench-top.log`, `layer3-fahprobe-top.log`
- `layer3-p3-preflight.log`, `build-images.log`, `fah-boot-*.log`
- `FINDINGS.md` (F23–F29), this file

Repo-root scratch state, all covered by `.gitignore`: `smoke-config/`,
`smoke-data/`, `smoke2-config/`, `smoke2-data/`, `smoke3-config/`,
`smoke3-data/`, `smoke-ca.pem`, `smoke3-ca.pem`, `p1.crt` / `p1.key`,
`p3.crt` / `p3.key` / `p3.der`, `p3c.crt` / `p3c.key`. The plan's preflight
one-liner, saved as a file to run it repeatedly, is kept here as
`h2-preflight.mjs`; the repo root was left with no unignored litter. Docker
leftovers: images `fah-p4:smoke`, `fah-splicebench:smoke`, `fah-probe:smoke`;
containers `fah-probe-smoke` and `fah-origin-smoke`, both **stopped** but not
removed so `docker logs` still works; network `fah-smoke-net`.

## Verdict on the four fixes this rerun was meant to check

| Fix | Verdict | Evidence |
| --- | --- | --- |
| `p0-sni.mjs` blocked-side guard (F18) | **fixed** | Layer 2 row 4b against the unseeded store: `INVALID: blocked rows closed without a rule verdict: listeners.https.blocked moved by 0, expected at least 5 (resolve_failures 5, refused_destination 0); --blocked is not blocked by the probe's ruleset`. Last session the same shape returned `valid: true` with `gate.pass: true`. |
| `p7-store.mjs` needle-not-status predicate (F20) | **fixed** | Layer 3 `p7`: `served:{"spa_shell":24,"other_200":0,"rejected":16}`, `failing:[]`, on an image that ships `/web`. Last session: 20 of 40 rows `pass: false`. |
| `3883e55` fah-dns DoT/TCP framing (F16, F19) | **fixed for `p4-lan.mjs`, not for the `Dockerfile.p4` harness** | Layer 3 `p4-lan`: `dot_p50_minus_udp_p50_ms: -0.017`. Same tree, same session, `Dockerfile.p4`: `dot p50=44011us` against `udp p50=53us`. F25 and F28. |
| `84d34be` fah-http h2 connection window (F7) | **fixed** | Layer 3 `p3`: `barrier MET (64/64 streams with :status 200 + first DATA)` on both the stall and the control run, `attribution: RESOLVED`. F29. |

## Still needed before the campaign

1. **A clean commit** (owner). The tree carries the owner's two uncommitted
   `phase2.6` files, so every `run.log` reads `…-dirty`; the campaign must not
   start in that state — the plan's own Layer 2 row says so.
2. **The probe's three boot keys plus restart** (owner, router write).
3. **A P3 origin the campaign can rely on.** `speed.cloudflare.com` does not
   negotiate ALPN h2 from this network (F24), and the release `fah-probe` image
   cannot trust a locally-minted one (F22, unchanged). `sabnzbd.org` carried
   this smoke run on a one-off approval; it is not a campaign origin.
4. **Triage `Dockerfile.p4`'s DoT column** (F25) and its HTTP/1.1 DoH arm
   (F26). Both land on the row P4 exists to measure, and F28 now places the
   remaining DoT step in the harness's own client rather than in `fah-dns`.
5. **A p3-04 note that F7 is reproduced and cleared** (F29). The barrier is met
   in-container on this tip, so the P3 BLOCKED state has a resolution to
   record, and the RSS arm has produced a figure for the first time anywhere.
6. **Two lines in the smoke plan for this box's shell** (F23, F27) — the
   `MSYS2_ARG_CONV_EXCL` prefix for bare `/path` flags and for `docker -v`
   specs. Owner's call; no `.md` outside this directory was touched.
