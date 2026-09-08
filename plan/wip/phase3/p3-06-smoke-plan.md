# p3-06 — smoke plan for the probe scripts (campaign 2, before any RB5009 run)

Functional, negative-path and container-level coverage for every file campaign
2 runs from `docs/code-review/phase3/p3-06-probe/` (testing plan §Scripts; the
campaign-1 diagnostics listed there as history are not smoked), on the dev box
only. **Nothing here is
a measurement**: no number from a smoke run is cited, budgeted or copied into
`p3-06-testing-results-2.md`. [p3-06-testing-plan.md](p3-06-testing-plan.md) is
untouched by this file; the scripts are exercised, not changed — a script bug
found here is fixed under its own review, methodology stays frozen.

**Why campaign 2 smokes again.** The scripts last ran against a build without
allocation domains, without `runtime.http_runtimes`, and against a probe config
whose `strategy = "fallback"` the tip build now rejects at load — and without
`oha` in the arm set. Four things must be proved before any arm64 image: the
scripts still work, they record N, they fail loudly on the two new
boot-refusal paths, and `oha`'s `--connect-to` carries SNI from the URL rather
than from the socket target.

**Stop rule.** No arm64 image, no probe config change, no router command, no
commit until the owner has reviewed the outputs of all four layers. The owner
runs `git` and the router; the agent runs the layers below on bobdenaut and
reports.

Output root: `docs/code-review/phase3/p3-06-probe/smoke-<ts>/` (one per
session, `<ts>` = `YYYYMMDDTHHMMZ`), untracked until the owner decides. Every
script gets `--out <root>/<layer>` so `run.log`, `raw.jsonl` and `config.json`
land beside each `<name>.json`. Numbers inside are not results.

## Layer 0 — the two new boot paths

Both are one-command checks against the release binary; both must fail the way
the plan says they fail, before anything else runs.

| Check | How | Expected |
| --- | --- | --- |
| removed strategy | add `strategy = "fallback"` under `[dns.upstreams]` in the smoke TOML, boot | boot fails with the `REMOVED_FALLBACK` message naming `dns.upstreams.strategy`. **This is the shape the probe will hit on the router** if its stored config is not fixed first |
| N is readable | boot with `[runtime] http_runtimes = 1`, then `GET /api/v1/config` | `runtime.http_runtimes` reads `1`. Every HTTPS script must echo it into `run.log`; a script that cannot read it refuses to run an HTTPS arm (`lib.mjs` change) |
| N default on this box | boot with no `[runtime]` key | reads `max(1, cores / 2)` — **16** on bobdenaut's 32 cores, not 2. Recorded so nobody reads a dev-box HTTPS figure as an RB5009 one |
| `oha` pinned | `oha --version` on bobdenaut **and** on the Mac | both read **1.16.0**. A mismatch between the two hosts would put the spliced and intercepted TLS rate arms on two different clients — the campaign-1 trap in a new costume |
| **`--connect-to` preserves SNI** | with the local probe up and posture A, `oha -n 1 -c 1 --connect-to 127-0-0-1.nip.io:443:127.0.0.1:8444 --insecure https://127-0-0-1.nip.io/` while a rule blocks that name | the probe must close it **as an SNI verdict** — `listeners.https.blocked` moves — proving the ClientHello carried `127-0-0-1.nip.io` and not the socket's target. Then repeat with an allowed name and confirm ServerHello. **A prerequisite, not a smoke nicety:** every `oha` TLS arm depends on it, and if SNI followed the connect target instead, all of them would silently measure the wrong name |

The `--connect-to` result is written into the report verbatim. Until it reads
as above, no `oha` arm carries a figure (testing plan §Load generators).

## Layer 1 — local FastAdHunter, same scripts

### 1.1 Boot

```sh
cargo build --release --locked -p fastadhunter
mkdir smoke-config smoke-data
```

`smoke-config/fastadhunter.toml` — ports chosen not to collide with the dev
box's resolver; every script has a flag for each. Note `[runtime]` is now
pinned and `[dns.upstreams]` carries no `strategy`:

```toml
[engine]
mode = "dns+http+https"

[runtime]
http_runtimes = 2             # pinned: the RB5009's value, not this box's 16

[dns.listen]
address = "127.0.0.1"
port = 5300
dot_port = 8853

[[dns.upstreams.servers]]
address = "192.168.10.1:53"   # the LAN resolver; this LAN blocks DNS straight out
protocol = "udp"

[http.listen]
address = "127.0.0.1"
port = 8080

[https.listen]
address = "127.0.0.1"
port = 8444

[https.interception]
clients = []                  # posture A; posture B = ["127.0.0.1"], see below

[egress]
allow_destinations = ["127.0.0.0/8"]

[api]
address = "127.0.0.1"
port = 8443
tls = true

[log]
level = "info"
format = "text"
```

```sh
./target/release/fastadhunter --config smoke-config/fastadhunter.toml --data smoke-data
```

First boot writes `smoke-config/apikey` — that file is `--key` for every
script. Then, once: a rule set with a blockable domain
(`||ads.smoke.test^` via `PUT /api/v1/rules/user`), a CA
(`POST /api/v1/certificates/ca/generate`), and the CA exported to
`smoke-ca.pem` (`GET …/ca/export?format=pem`). All four calls are
`curl -sk -H "Authorization: Bearer $(cat smoke-config/apikey)"` against
`https://127.0.0.1:8443`.

Common flags for every call below: `--probe 127.0.0.1 --api-port 8443 --key
smoke-config/apikey --out <root>/layer1`. A browser open on the dev box makes
every script `INVALID` by design (plan §Running item 3); close it or pass
`--allow-busy` and accept `degraded`.

**Two client postures.** Loopback has one client address, `127.0.0.1`. The
spliced arms need it *unlisted*, the terminate-leg arms need it *listed*, so
the box boots twice: **posture A** (`clients = []`) for `p0`, `p1`, `p4-lan`,
`p6`, `p7`; **posture B** (`clients = ["127.0.0.1"]`) for `p3` and `p5`. Every
restart is a config change: give each posture its own `--out` directory
(`layer1-a`, `layer1-b`). `lib.mjs` reads the live config on every run and,
when it differs from the directory's `config.json`, writes a dated copy and
labels the run `degraded` — a row with that label ran in the wrong directory,
not against the wrong probe.

**Three N postures, on top of the client ones.** `http_runtimes ∈ {0, 1, 2}`,
each its own `--out` suffix (`-n0`, `-n1`, `-n2`). Only `p0`, `p1` and `p3`
need all three — they are the arms whose path the domains changed. `N = 0` is
the pre-merge shared-runtime path and must still work; `N = 1` puts every
session on one domain thread, which is where a hand-off deadlock would show;
`N = 2` is the device's value. The rest run at `N = 2` only.

### 1.2 Per script — command and what "works" means

| Script | Command (beyond the common flags) | Passes when |
| --- | --- | --- |
| `p0-sni.mjs` | `--port 8444 --blocked ads.smoke.test --allowed example.com`, ×3 N postures | `sni.json` `valid: true`; blocked and no-SNI rows `closed_silent` or `alert`, allowed rows `server_hello`; `telemetry_delta.blocked ≥ 5`; `run.log` carries the N |
| `p1-origin.mjs` + `p1-lan.mjs` | posture A. Origin certificate needs `basicConstraints=critical,CA:FALSE` and `extendedKeyUsage=serverAuth` — without them rustls rejects it as `CaUsedAsEndEntity`. Then `node p1-origin.mjs --cert p1.crt --key-file p1.key --port 4443 --bytes 8`; client `--origin 127-0-0-1.nip.io --origin-port 4443 --origin-cert p1.crt --bytes 8 --port 8444`. The probe splices to `:443` by construction, so on the dev box the spliced arm reaches the origin only if the origin listens on 443 (elevated shell) — else expect `INVALID` with `refused_destination` / `upstream_failures` and count that as the negative path; `--direct` with `--origin-port 4443` must pass | `p1-control.json` `valid: true`, 5 runs, `steady_mib_s` present; spliced arm on `:443`: `p1.json` `valid: true`, `served_issuer` = the origin's CN; `--connections 8` writes `p1-aggregate.json`. **The relative gate line is present and reads against the control** — the absolute MiB/s is a diagnostic column (testing-plan delta 1) |
| `p2-handshake.mjs` | Runs on the **Mac** — its dress rehearsal, replacing campaign 1's bridged VM. Dev-box side: a third posture with every listener on `0.0.0.0` and one Mac address in `clients`; inbound rules on bobdenaut scoped to the Mac's two addresses for TCP 8443, 8444, 8853 and UDP 5300 (owner adds, removes afterwards). On the Mac: `--probe <dev-box LAN IP> --listed <A> --unlisted <B> --origin example.com --ca smoke-ca.pem --rounds 10 --dns-port 5300 --port 8444`. **The Darwin branch of the identity precondition is what this row exists to prove** — `ifconfig`, not `ip` | `p2.json` `valid: true`; intercepted `served_issuer` = `FastAdHunter CA`, spliced and direct ≠; `gate.ratio` present. On the Windows dev box directly the precondition must print `INVALID` (no `ip`, no `ifconfig`) — record that as the negative path |
| `p3-h2stall.mjs` | posture B, ×3 N postures for the throughput arm. Needs an h2 origin the binary trusts: build the harness binary (`cargo build -p fastadhunter --features test-harness`), run it instead of the release one with `FAH_TEST_UPSTREAM_ROOT=<origin-cert.der>`; origin `smoke/h2-origin.mjs --cert p3.crt --key-file p3.key --port 443 --bytes 8` (elevated); client `--origin 127-0-0-1.nip.io --path /8mib --warmup-path / --ca smoke-ca.pem --port 8444 --streams 64 --runs 1 --settle 5 --throughput-runs 1` | throughput arm `ok: true`, 8 MiB received, at each N. RSS arm **on Windows**: `INVALID: process_rss is null on this probe` is the correct outcome — the kernel reading exists in-container only. **The stall barrier is the regression check for p3-04 S2** (campaign 1's "BLOCKED" state, control 64/64 / stall 5/64, fixed by the 4 MiB connection window): at each N the stall run must report `barrier MET` 64/64; a barrier not met on the domain lane is a new p3-04 finding, not a reproduction of the old one |
| `p4-lan.mjs` | `--domain ads.smoke.test --dot-host dns.smoke.test --dns-port 5300 --dot-port 8853 --queries 200 --rounds 1` | `p4-lan.json` `valid: true`; all three transports `n = 200`, `unanswered = 0`, `unmatched = 0`, `first_answer.answers` = `["0.0.0.0"]`, dot `served_issuer` = `FastAdHunter CA`; the DoH arm asserted `HTTP/2.0` |
| `p5-mint.mjs` | `--dot-port 8853 --hosts 16` | `p5.json` `valid: true`; `counters_delta.minted_total = 16`, `evictions = 0`, both arms' `served_issuer` = `FastAdHunter CA` |
| `p6-certs-time.mjs` | `--generate 2 --import 2 --cert p1.crt --key-file p1.key --ca-archive-count 0 --api-archive-count 0` (fresh `smoke-config`, so both archives are empty) | `p6.json` `valid: true`; four rows `status: 200`; `api_certificate_after.source = "imported"`; `ca-after-p6.pem` written and differs from `smoke-ca.pem` |
| `p7-store.mjs` | `--ca-key smoke-config/ca-key.pem` | `certs.json` `valid: true`, `gate.pass: true`, every traversal row `pass: true` = `leaks: []`. Status is diagnostic only: a build that ships `/web` answers `200 text/html` (the SPA shell) for every unmatched route including `/config/ca-key.pem` — the suite asserts needles, never status. An `other_200` on a sensitive path is logged for a human look |
| `p10-domains.mjs` (+ `oha`) | **new, and the largest smoke item.** `--arm close`, `--arm mixed` (HTTP half), `--arm transfers`, and both TLS connection-rate arms — all `oha`-driven, `--duration 20 --concurrency 8`, at `http_runtimes ∈ {0, 1, 2}` | `p10-N<n>.json` `valid: true`; per arm: requests/s, p50 / p95 / p99 from `oha`'s `--output-format json`, **cores from `cpu_user_ms + cpu_system_ms` deltas** (not `/tool/profile` — it does not exist here), ΔRSS against the arm-local floor, 502 count. `oha_version: "1.16.0"` and the `--worker-threads` value present in every file. The N read back from `/config` matches the intended N — the invalidity rule that stops four identical rows being reported as a plateau. **TIME_WAIT check:** `netstat -an \| findstr TIME_WAIT` shows no pile-up on bobdenaut, else the run measures Windows port exhaustion (~130 conn/s) and not the probe. On the TLS arms, the `openssl s_client` issuer sample before and after is in `run.log` |
| `p10-connrate.mjs` | **keep-alive arm only** — 20 requests per connection, last with `Connection: close`, plaintext and TLS. This is the shape `oha` cannot express | requests/s **and connections/s** both present (the field that makes the phase-2.6 comparison possible), p50 / p95 / p99, per-connection request count = 20 |
| `p10-dnsload.mjs` | the DNS half of the mixed arm: `--qps 300 --duration 20` against `:5300`, 50 % cached / 20 % blocked / 30 % uncached | `n` matches qps × duration, `unmatched = 0`, `timeouts` reported, the blocked share answering `0.0.0.0`. Run **concurrently with** the `oha` HTTP half, which is what makes it the mixed arm rather than two arms |

After `p6`, re-export the CA (`smoke-ca.pem` is stale — each generate replaces
it) before re-running `p2`, `p3` or `p5`.

## Layer 2 — negative paths, one per rule

Each row is one command; the pass condition is the printed `INVALID` reason or
the `degraded` label, never a number. Restore the state afterwards.

| Rule (script) | How to force it | Expected |
| --- | --- | --- |
| removed strategy (boot) | `strategy = "fallback"` in the TOML | boot refused, `REMOVED_FALLBACK`, no listener opens |
| N unreadable (all HTTPS) | point `--probe` at a build whose `/config` omits `runtime` | `INVALID: could not read runtime.http_runtimes` |
| N mismatch (`p10`) | ask for `--n 4` while the config says 2 | `INVALID: probe reports http_runtimes=2, arm declared 4` |
| no API key (all) | omit `--key`, unset `FAH_PROBE_KEY` | `INVALID: no API key` before any request |
| `/health` unreachable (all) | `--probe 127.0.0.9` | `INVALID: probe /health answered 0` |
| `engine.mode` without https | `mode = "dns"`, restart | `INVALID: engine.mode dns carries no https listener`; `p6` and `p7` still run |
| busy host (all, Windows) | open a browser | `INVALID: host not idle: … --allow-busy to proceed degraded`; with `--allow-busy` the result carries `status: "degraded"` |
| allowed name does not open (`p0`) | `--allowed ads.smoke.test` | `INVALID: allowed name did not reach ServerHello` |
| origin outside `egress.allow_destinations` (`p1`) | `allow_destinations = []`, restart | `INVALID: origin address … is not in egress.allow_destinations` |
| origin byte count wrong (`p1`) | client `--bytes 16` against an 8 MiB origin, `--direct` | `INVALID: no p1_control sample completed` |
| control missing (`p1`) | run `p1-lan.mjs` with no prior `--direct` result in the run directory | `INVALID: no P1-control median in this session` — the relative gate has nothing to read against (delta 1) |
| identity precondition (`p2`) | run on the Windows dev box | `INVALID: identity precondition: both --listed and --unlisted must be on this host's interfaces` |
| both addresses unlisted (`p2`, Mac) | `clients = []` | `INVALID: … exactly one of the two addresses must be in https.interception.clients` |
| this host not listed (`p3`) | `clients = []`, restart | `INVALID: this host (127.0.0.1) is not in https.interception.clients` |
| barrier not met (`p3`) | `--arm rss --path /stall` against `smoke/h2-origin.mjs` (`/stall` answers `:status 200` and never sends DATA, so no stream reaches its first chunk; a tiny body would *meet* the barrier) | RSS arm `INVALID: … barrier not met: 0/64 streams answered` after `--barrier-timeout`. **Not runnable on this dev box**: on Windows `process_rss is null` fires first, and the release `fah-probe` image cannot trust a local origin. Record as not run |
| no CA in the store (`p5`) | fresh `smoke-config` without `ca/generate` | `INVALID: no CA in the probe store` |
| cache headroom (`p5`) | `--hosts 600` | `INVALID: leaf_cache.size … exceeds capacity 512` |
| archive cap (`p6`) | fresh store, `--generate 10 --ca-archive-count 0` | `INVALID: ca-archive holds 0; 10 generates would add 9 (the first on an empty store archives nothing) and pass the cap of 8` before any call; with the counts omitted, the **tenth** call itself `INVALID: ca/generate #10 answered 409` |
| `--ca-key` unreadable / not a key (`p7`) | `--ca-key nope.pem`, then `--ca-key p1.crt` | `INVALID: --ca-key unreadable`, then `INVALID: --ca-key is not a private key` |
| failure-rate budget (`p1`, `p2`, `p3`) | kill `p1-origin.mjs` after run 3 of 5 | `p1.json` `status: "degraded"`, `degraded_reasons` names `2 of 5 runs incomplete`, figures from 3 runs |
| toy origin (`p10`) | point the load at a server with a listen backlog of 5 | 502s in the result and a `degraded` label — the phase-2.6 rig-2 failure, reproduced deliberately so it is recognised on the device |
| `oha` version drift (`p10`) | temporarily shim a different `oha` on `PATH` | `INVALID: oha 1.16.0 required, found <x>` before the arm starts |
| `--worker-threads` unset (`p10`) | omit it from the arm definition | `INVALID: --worker-threads must be set explicitly` — never a silent default of 24 |
| issuer sample missing on a TLS arm (`p10`) | disable the `openssl s_client` step | the arm completes but is labelled a diagnostic, not a figure, with the reason named |
| `oha` used for the keep-alive arm (`p10`) | pass `--arm keepalive --driver oha` | refused: `keepalive is driven by p10-connrate.mjs; oha has no requests-per-connection control`. The tool cannot be swapped in for an arm the plan excludes it from |
| dirty checkout (all) | run with an uncommitted tracked change | `run.log` line `tip=<hash>-dirty`; the campaign must not start in this state |

## Layer 3 — the four images, x86 Docker on the dev box

Same Dockerfiles, `--platform linux/amd64 --load`, run locally, **all rebuilt
at the tip** — campaign 1's `a2d0802` images are retired. Proves the
container-only parts — uid, `/tmp`, harness spawn, distroless entrypoints, the
renamed binaries — before any arm64 build.

| Image | Run | Passes when |
| --- | --- | --- |
| `Dockerfile.p4` | `docker run --rm fah-p4:smoke` | the log shows the harness booting `/fah-probe` (its `FAH_E2E_BINARY override active` line), three transports with `2000` queries per round, the DoH arm reporting `HTTP/2.0`, and no `EACCES` / `Permission denied`. `docker inspect` shows `User: 65532:65532` |
| `Dockerfile.splicebench` | `docker run --rm fah-splicebench:smoke --reps 1 --size-mib 8` | five `rep=1 arm=splice` lines, one `loopback_origin` line, the candidate table, a `pick` line and five `counters` lines; `docker top` shows `/fah-splicebench` |
| `Dockerfile.certs` | `docker run --rm fah-certs:smoke` | criterion runs `certs_mint` at **default** warm-up and measurement time and prints a point estimate; `docker top` shows `/fah-certs` |
| `Dockerfile.fahprobe` | `smoke-config-l3/` = the Layer 1 TOML with every listen `address` set to `0.0.0.0` (a `127.0.0.1` bind is unreachable through `-p`), same ports; then `docker run --rm -v "$PWD/smoke-config-l3:/config" -v "$PWD/smoke-data-l3:/data" -p 8443:8443 -p 8444:8444 -p 8853:8853 -p 5300:5300/udp fah-probe:smoke` | Layer 1 again against it (`--probe 127.0.0.1`, same ports), every row unchanged, `docker top` shows `/fah-probe`. **`process_rss` is non-null here**, so this is where the P3 RSS arm becomes exercisable |

Then, in-container, two things the loopback build cannot show:

- **`process_rss` and the P3 RSS arm.** Only against a **publicly trusted** h2
  origin: the image is a plain release build, so `FAH_TEST_UPSTREAM_ROOT` is
  inert and a local origin fails `UnknownIssuer`. Prove the origin first with
  `smoke/h2-preflight.mjs` — `alpn h2` and exactly 8 388 608 bytes with
  `content-length` — and record origin, path and that line in the report. Then
  run with `127.0.0.1` listed. Pass: throughput arm `ok: true`; RSS arm
  `barrier: met` on the control run, samples present, `attribution` printed. A
  stall run that misses the barrier reproduces the P3 finding in-container:
  capture the per-stream dump, do not work around it.
- **`p10-domains.mjs` against the container**, `http_runtimes` supplied as
  `-e FAH__RUNTIME__HTTP_RUNTIMES=<n>` — the same env-over-file precedence the
  router will use. This is the dress rehearsal for `fahprobe-env`: it proves
  the var reaches the binary and that `p10` reads it back from `/config`.

Two rows differ from Layer 1 and are recorded as not run: **`p1-lan.mjs
--direct` (P1-control)** — Docker Desktop on Windows does not route the bridge
subnet to the host, so the host reaches a bridge-side origin only through the
published port; the arm has real LAN addresses on the RB5009. And **Docker
Desktop's UDP port relay wedges under load**, so keep `p4-lan` at
`--queries 200` and read no timing from this layer.

## Report

One table per layer in the session's chat report; no report file is written —
the owner reads `run.log` and the `<name>.json` files directly. Per row:
script, command, `valid` / `INVALID` / `degraded`, the reason line, and any
script bug found. A script bug is fixed in the script, re-run, and listed;
nothing in `p3-06-testing-plan.md` changes.

When all four layers pass, the report ends with what is still needed before
the campaign starts:

1. a clean commit (owner);
2. the probe's config fixed — `strategy` removed, `engine.mode`,
   `egress.allow_destinations`, `https.interception.clients` — plus a restart
   (owner, router write);
3. `fahprobe-env` created and attached, or `h1buf-env` re-attached (owner,
   router write) — without it the N axis does not exist;
4. the four tip images uploaded (owner);
5. the Mac's preconditions met (wired, Node, **`oha` 1.16.0**, ssh, alias, AC
   power);
6. the P3 public-trusted origin name, still outstanding;
7. nothing on the h2 stall — p3-04 S2 is filed and fixed; the Layer 1 and
   Layer 3 stall runs must have read `barrier MET` at each N.
