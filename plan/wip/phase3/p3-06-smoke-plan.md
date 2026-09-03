# p3-06 — smoke plan for the probe scripts (before any RB5009 run)

Functional, negative-path and container-level coverage for every file under
`docs/code-review/phase3/p3-06-probe/`, on the dev box only. **Nothing here is
a measurement**: no number from a smoke run is cited, budgeted or copied into
`p3-06-testing-results.md`. The frozen
[p3-06-testing-plan.md](p3-06-testing-plan.md) is untouched by this file; the
scripts are exercised, not changed — a script bug found here is fixed under
its own review, methodology stays frozen.

**Stop rule.** No arm64 image, no probe config change, no router command, no
commit until the owner has reviewed the outputs of all three layers. The
owner runs `git` and the router; the agent runs the layers below on
bobdenaut and reports.

Output root: `docs/code-review/phase3/p3-06-probe/smoke-<ts>/` (one per
session, `<ts>` = `YYYYMMDDTHHMMZ`), untracked until the owner decides.
Every script gets `--out <root>/<layer>` so `run.log`, `raw.jsonl` and
`config.json` land beside each `<name>.json`. Numbers inside are not results.

## Layer 1 — local FastAdHunter, same scripts

### 1.1 Boot

Build and run the release binary on loopback with all three engines:

```sh
cargo build --release --locked -p fastadhunter
mkdir smoke-config smoke-data
```

`smoke-config/fastadhunter.toml` (ports chosen not to collide with the dev
box's resolver; every script has a flag for each):

```toml
[engine]
mode = "dns+http+https"

[dns.listen]
address = "127.0.0.1"
port = 5300
dot_port = 8853

[[dns.upstreams.servers]]
address = "192.168.10.1:53"   # the LAN resolver; this LAN blocks DNS straight out (F1)
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
script. Then, once:

```sh
# a domain every DNS / SNI arm can block
curl -sk -H "Authorization: Bearer $(cat smoke-config/apikey)" -X PUT \
  https://127.0.0.1:8443/api/v1/rules/user -H 'content-type: application/json' \
  -d '{"rules":["||ads.smoke.test^"]}'
# a CA, exported for the listed-client arms
curl -sk -H "Authorization: Bearer $(cat smoke-config/apikey)" -X POST \
  https://127.0.0.1:8443/api/v1/certificates/ca/generate -H 'content-type: application/json' \
  -d '{"confirm":true}'
curl -sk -H "Authorization: Bearer $(cat smoke-config/apikey)" \
  "https://127.0.0.1:8443/api/v1/certificates/ca/export?format=pem" -o smoke-ca.pem
```

Common flags for every call below: `--probe 127.0.0.1 --api-port 8443 --key
smoke-config/apikey --out <root>/layer1`. A browser open on the dev box makes
every script `INVALID` by design (plan §Running item 3); close it or pass
`--allow-busy` and accept `degraded`.

**Two client postures (F2).** Loopback has one client address, `127.0.0.1`.
The spliced arms need it *unlisted*, the terminate-leg arms need it *listed*,
so the box boots twice: **posture A** (`clients = []`) for `p0`, `p1`,
`p4-lan`, `p6`, `p7`; **posture B** (`clients = ["127.0.0.1"]`) for `p3` and
`p5`. Every restart is a probe config change: give each posture its own
`--out` directory (`<root>/layer1-a`, `<root>/layer1-b`). `lib.mjs` reads the
live config on every run and, when it differs from the directory's
`config.json`, writes a dated copy and labels the run `degraded` (F9) — a
row that shows that label ran in the wrong directory, not against the wrong
probe.

### 1.2 Per script — command and what "works" means

| Script | Command (beyond the common flags) | Passes when |
| --- | --- | --- |
| `p0-sni.mjs` | `--port 8444 --blocked ads.smoke.test --allowed example.com` | `sni.json` `valid: true`; blocked and no-SNI rows `closed_silent` or `alert`, allowed rows `server_hello`; `telemetry_delta.blocked ≥ 5` |
| `p1-origin.mjs` + `p1-lan.mjs` | posture A. Origin: `openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout p1.key -out p1.crt -days 3 -subj "/CN=127-0-0-1.nip.io" -addext "subjectAltName=DNS:127-0-0-1.nip.io" -addext "basicConstraints=critical,CA:FALSE" -addext "extendedKeyUsage=serverAuth"` (the last two are what the interception leg needs — without them rustls rejects the certificate as `CaUsedAsEndEntity`, F6), then `node p1-origin.mjs --cert p1.crt --key-file p1.key --port 4443 --bytes 8`; client: `--origin 127-0-0-1.nip.io --origin-port 4443 --origin-cert p1.crt --bytes 8 --port 8444` — the probe splices to `:443` by construction, so on the dev box the spliced arm reaches the origin only if the origin listens on 443 (elevated shell) — else expect `INVALID` with `refused_destination` / `upstream_failures` in the telemetry delta and count that as the negative path; `--direct` with `--origin-port 4443` must pass | `p1-control.json` `valid: true`, 5 runs, `steady_mib_s` present; spliced arm on `:443`: `p1.json` `valid: true`, `served_issuer` = the origin's CN; `--connections 8` writes `p1-aggregate.json` |
| `p2-handshake.mjs` | Linux only (`ip addr`), and WSL2 cannot reach a loopback-bound probe (F4). Run it from the **wired bridged VM the campaign will use** — its dress rehearsal: a third posture with every listener on `0.0.0.0` and one VM address in `clients`; on the dev box, inbound firewall rules scoped to the VM's two addresses for TCP 8443, 8444, 8853 and UDP 5300 (owner adds, removes afterwards); on the VM `--probe <dev-box LAN IP> --listed <A> --unlisted <B> --origin example.com --ca smoke-ca.pem --rounds 10 --dns-port 5300 --port 8444`. No VM available ⇒ record the row as not run | `p2.json` `valid: true`; intercepted `served_issuer` = `FastAdHunter CA`, spliced and direct ≠; `gate.ratio` present. On the Windows dev box directly the identity precondition must print `INVALID` (no `ip`) — record that as the negative path |
| `p3-h2stall.mjs` | posture B. Needs an h2 origin the binary trusts: build the dev-profile harness binary `cargo build -p fastadhunter --features test-harness`, run it instead of the release one with `FAH_TEST_UPSTREAM_ROOT=<origin-cert.der>`; origin: `node docs/code-review/phase3/p3-06-probe/smoke/h2-origin.mjs --cert p3.crt --key-file p3.key --port 443 --bytes 8` (elevated; certificate with the CA:FALSE + EKU lines above, DER via `openssl x509 -in p3.crt -outform der -out p3.der`); client: `--origin 127-0-0-1.nip.io --path /8mib --warmup-path / --ca smoke-ca.pem --port 8444 --streams 64 --runs 1 --settle 5 --throughput-runs 1` | `p3.json`: throughput arm `ok: true`, 8 MiB received. RSS arm **on Windows**: `INVALID: process_rss is null on this probe` is the correct outcome (F8 — the kernel reading exists in-container only; Layer 3's `fah-probe` image is where the arm can reach `barrier: met`, samples and `attribution`). **The barrier itself is the dev-box reproduction the P3 BLOCKED state owes** (plan §State; F7 reproduced it: control 64/64, stall 5/64, the other 59 streams answered `:status 200` then ended with zero DATA) — a p3-04 finding to be filed before any device run |
| `p4-lan.mjs` | `--domain ads.smoke.test --dot-host dns.smoke.test --dns-port 5300 --dot-port 8853 --queries 200 --rounds 1` | `p4-lan.json` `valid: true`; all three transports `n = 200`, `unanswered = 0`, `unmatched = 0`, `first_answer.answers` = `["0.0.0.0"]`, dot `served_issuer` = `FastAdHunter CA` |
| `p5-mint.mjs` | `--dot-port 8853 --hosts 16` | `p5.json` `valid: true`; `counters_delta.minted_total = 16`, `evictions = 0`, both arms' `served_issuer` = `FastAdHunter CA` |
| `p6-certs-time.mjs` | `--generate 2 --import 2 --cert p1.crt --key-file p1.key --ca-archive-count 0 --api-archive-count 0` (fresh `smoke-config`, so both archives are empty) | `p6.json` `valid: true`; four rows `status: 200`; `api_certificate_after.source = "imported"`; `ca-after-p6.pem` written and differs from `smoke-ca.pem` |
| `p7-store.mjs` | `--ca-key smoke-config/ca-key.pem` | `certs.json` `valid: true`, `gate.pass: true`, every traversal row `pass: true`, `/config/*` and `/data/*` never `200` |

After `p6`, re-export the CA (`smoke-ca.pem` is stale — each generate
replaces it) before re-running `p2`, `p3` or `p5`.

## Layer 2 — negative paths, one per rule

Each row is one command; the pass condition is the printed `INVALID` reason
or the `degraded` label, never a number. Restore the state afterwards.

| Rule (script) | How to force it | Expected |
| --- | --- | --- |
| no API key (all) | omit `--key`, unset `FAH_PROBE_KEY` | `INVALID: no API key` before any request |
| `/health` unreachable (all) | `--probe 127.0.0.9` | `INVALID: probe /health answered 0` |
| `engine.mode` without https (`p0`, `p1`, `p2`, `p3`, `p4-lan`, `p5`) | `mode = "dns"` in the smoke TOML, restart | `INVALID: engine.mode dns carries no https listener`; `p6` and `p7` still run |
| busy host (all, Windows) | open a browser | `INVALID: host not idle: … --allow-busy to proceed degraded`; with `--allow-busy` the result carries `status: "degraded"` |
| allowed name does not open (`p0`) | `--allowed ads.smoke.test` (a blocked name as the allowed one) | `INVALID: allowed name did not reach ServerHello` |
| origin outside `egress.allow_destinations` (`p1`) | `allow_destinations = []`, restart | `INVALID: origin address … is not in egress.allow_destinations` |
| origin byte count wrong (`p1`) | client `--bytes 16` against an 8 MiB origin, `--direct` | `INVALID: no p1_control sample completed` |
| identity precondition (`p2`) | run on the Windows dev box | `INVALID: identity precondition: both --listed and --unlisted must be on this host's interfaces` |
| both addresses unlisted (`p2`, Linux) | `clients = []` | `INVALID: … exactly one of the two addresses must be in https.interception.clients` |
| this host not listed (`p3`) | `clients = []`, restart | `INVALID: this host (127.0.0.1) is not in https.interception.clients` |
| barrier not met (`p3`) | `--arm rss --path /stall` against `smoke/h2-origin.mjs` (`/stall` answers `:status 200` and never sends DATA, so no stream reaches its first chunk; a tiny body would *meet* the barrier, F11) | RSS arm `INVALID: … barrier not met: 0/64 streams answered` after `--barrier-timeout`; on Windows the `process_rss is null` rule fires first, so run this row in Layer 3's `fah-probe` image |
| no CA in the store (`p5`) | fresh `smoke-config` without `ca/generate` | `INVALID: no CA in the probe store` |
| cache headroom (`p5`) | `--hosts 600` | `INVALID: leaf_cache.size … exceeds capacity 512` |
| archive cap (`p6`) | fresh store, `--generate 10 --ca-archive-count 0` | `INVALID: ca-archive holds 0; 10 generates would add 9 (the first on an empty store archives nothing) and pass the cap of 8` before any call; with the counts omitted, the **tenth** call itself `INVALID: ca/generate #10 answered 409` (F10: from an empty store N generates make N − 1 archives; nine fill the cap and all answer 200) |
| `--ca-key` unreadable / not a key (`p7`) | `--ca-key nope.pem`, then `--ca-key p1.crt` | `INVALID: --ca-key unreadable`, then `INVALID: --ca-key is not a private key` |
| failure-rate budget (`p1`, `p2`, `p3`, delta 11) | kill `p1-origin.mjs` after run 3 of 5 | `p1.json` `status: "degraded"`, `degraded_reasons` names `2 of 5 runs incomplete`, figures from 3 runs |
| dirty checkout (all) | run with an uncommitted tracked change | `run.log` line `tip=<hash>-dirty`; the campaign must not start in this state |

## Layer 3 — the three images, x86 Docker on the dev box

Same Dockerfiles, `--platform linux/amd64 --load`, run locally. Proves the
container-only parts — uid, `/tmp`, harness spawn, distroless entrypoints,
the renamed binaries — before any arm64 build.

```sh
docker buildx build --platform linux/amd64 --load \
  -f docs/code-review/phase3/p3-06-probe/Dockerfile.p4 -t fah-p4:smoke .
docker run --rm fah-p4:smoke
```

Passes when the log shows the harness booting `/fah-probe` (its
`FAH_E2E_BINARY override active` line), three transports with `2000`
queries per round, and no `EACCES` / `Permission denied`. `docker inspect`
shows `User: 65532:65532`.

```sh
docker buildx build --platform linux/amd64 --load \
  -f docs/code-review/phase3/p3-06-probe/Dockerfile.splicebench -t fah-splicebench:smoke .
docker run --rm fah-splicebench:smoke --reps 1 --size-mib 8
```

Passes when five `rep=1 arm=splice` lines, one `loopback_origin` line, the
candidate table, a `pick` line and five `counters` lines print; `ps` inside
is not possible (distroless) — `docker top` shows the process as
`/fah-splicebench`.

```sh
docker buildx build --platform linux/amd64 --load \
  -f docs/code-review/phase3/p3-06-probe/Dockerfile.fahprobe -t fah-probe:smoke .
docker run --rm -v "$PWD/smoke-config:/config" -v "$PWD/smoke-data:/data" \
  -p 8443:8443 -p 8444:8444 -p 8853:853 -p 5300:53/udp fah-probe:smoke
```

Then Layer 1 again against it (`--probe 127.0.0.1`, same ports). Passes when
every Layer 1 row passes unchanged and `docker top` shows `/fah-probe`.
Docker Desktop's UDP port relay is known to wedge under load
(`docs/project-state.md`): keep `p4-lan` at `--queries 200`, and read no
timing from this layer.

## Report

One table per layer in the session's chat report; no report file is
written — the owner reads `run.log` and the `<name>.json` files directly.
Per row: script, command, `valid` /
`INVALID` / `degraded`, the reason line, and any script bug found. A script
bug is fixed in the script, re-run, and listed; nothing in
`p3-06-testing-plan.md` changes. When all three layers pass, the report
ends with the two things still needed before the campaign: a clean commit
(owner) and the probe's three boot keys plus restart (owner, router write).
