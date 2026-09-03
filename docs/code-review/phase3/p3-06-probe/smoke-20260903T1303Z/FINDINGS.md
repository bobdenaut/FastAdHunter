# p3-06 probe smoke — findings (smoke-20260903T1303Z, bobdenaut)

Functional / negative-path / container smoke of the probe scripts. Nothing
here is a measurement; no number below is a result.

## F1 — smoke plan Layer 1 §1.1 boot config — 1.1
- classification: environment
- command: `node p0-sni.mjs --probe 127.0.0.1 --api-port 8443 --key smoke-config/apikey --out <root>/layer1 --port 8444 --blocked ads.smoke.test --allowed example.com`
- expected: "`sni.json` `valid: true`; blocked and no-SNI rows `closed_silent` or `alert`, allowed rows `server_hello`; `telemetry_delta.blocked >= 5`"
- observed:
  `2026-09-03T13:04:52.208082Z  WARN fah_dns::upstream: all upstreams failed — answers now depend on cached entries upstreams=1 suppressed=0 error=upstream timed out`
  `[sni] allowed  #1 closed_silent bytes=0 close=807.624ms`
  `[sni] listeners.https delta {"connections":15,"requests":15,"blocked":5,...,"resolve_failures":5,...}`
  `[sni] INVALID: allowed name did not reach ServerHello through the probe; the listener closing everything proves nothing about blocked rows`
- files: smoke-20260903T1303Z/layer1/sni.json, smoke-20260903T1303Z/layer1/run.log, smoke-20260903T1303Z/fah-boot.log
- repro: `powershell -c "Test-NetConnection -ComputerName 1.1.1.1 -Port 53 -InformationLevel Quiet"` returns False on this box
- note: the smoke plan's boot TOML pins `[[dns.upstreams.servers]] address = "1.1.1.1:53"`, which this LAN does not allow out; every arm needing a public origin resolves nothing. Re-run below used `192.168.10.1:53` (the LAN resolver, reachable) as the upstream — a boot-config deviation, not a script or methodology change.

## F2 — smoke plan Layer 1 §1.2 row p1 — 1.2
- classification: plan-gap
- command: `node p1-lan.mjs --probe 127.0.0.1 --api-port 8443 --key smoke-config/apikey --out <root>/layer1 --origin 127-0-0-1.nip.io --origin-cert p1.crt --bytes 8 --port 8444`
- expected: "spliced arm on `:443`: `p1.json` `valid: true`, `served_issuer` = the origin's CN"
- observed:
  `[p1] run 1: ok=0/1 steady=- MiB/s total=- aggregate=- hs=nullms`
  `[p1] listeners.https delta {"connections":5,"requests":5,"blocked":0,"refused_claim":0,"refused_destination":0,"resolve_failures":0,"upstream_failures":0,"upstream_cert_failures":5,"non_http":0,"non_tls":0,"hello_timeouts":0,"dropped_events":0}`
  `[p1] INVALID: no spliced sample completed 8 MiB on every connection`
- files: smoke-20260903T1303Z/layer1/p1.json, smoke-20260903T1303Z/layer1/run.log
- repro: boot with the plan's smoke TOML (`[https.interception] clients = ["127.0.0.1"]`), origin on `127.0.0.1:443` with a self-signed cert, run the command above
- note: the smoke TOML lists `127.0.0.1` in `https.interception.clients` (p3 and p5 need it), so the only client address the dev box has is intercepted, never spliced — the probe terminates TLS and rejects the self-signed origin (`upstream_cert_failures`); the row's spliced arm needs an unlisted client, which loopback-only cannot provide at the same time as the p3/p5 rows.

## F3 — p1-lan.mjs — 1.2
- classification: script-bug
- command: same as F2
- expected: the plan's P1 invalidity rule — "a run whose byte count differs from `--bytes` MiB is INVALID" — reported with its cause
- observed:
  `{"run":1,"connections":1,"ok":0,"bytes_total":0,"steady_mib_s":null,...,"errors":[],"tls":null}`
- files: smoke-20260903T1303Z/layer1/p1.json, smoke-20260903T1303Z/layer1/raw.jsonl
- repro: point `p1-lan.mjs` at a listener that accepts the TCP connection and closes it cleanly with zero bytes
- note: `pull()` resolves with `error: null` on the `end` event, so a clean zero-byte close leaves `errors: []` and `tls: null` in the row — `ok: 0` with no per-connection reason; only the `listeners.https` delta explains the failure, and that is absent for `--direct`.

## F4 — p2-handshake.mjs (Linux arm) — 1.2
- classification: environment
- command: not run — `node p2-handshake.mjs --probe <dev-box LAN IP> --listed <A> --unlisted <B> --origin example.com --ca smoke-ca.pem --rounds 10 --dns-port 5300 --port 8444` from WSL / the VM
- expected: "`p2.json` `valid: true`; intercepted `served_issuer` = `FastAdHunter CA`, spliced and direct != ; `gate.ratio` present"
- observed:
  `  NAME              STATE           VERSION`
  `* Ubuntu            Stopped         2`
  `  docker-desktop    Running         2`
- files: (no result file; Windows negative path is smoke-20260903T1303Z/layer1/p2.json)
- repro: `wsl.exe -l -v` — only a WSL2 (NAT) distribution is present; the smoke TOML binds every listener to `127.0.0.1`, which a WSL2 guest cannot reach
- note: reaching the binary from WSL2 would need the listeners rebound to `0.0.0.0`, a second address added on the guest interface and an inbound Windows Firewall rule — all outside this smoke run; the Windows negative path was run instead and printed `INVALID: identity precondition: both --listed and --unlisted must be on this host's interfaces (found none)` after `ip -4 -o addr: {"error":"spawnSync ip ENOENT"}`, exactly as the plan predicts.

## F5 — p3-h2stall.mjs — 1.2
- classification: script-bug
- command: `node p3-h2stall.mjs --probe 127.0.0.1 --api-port 8443 --key smoke-config/apikey --out <root>/layer1 --origin 127-0-0-1.nip.io --path /8mib --warmup-path / --ca smoke-ca.pem --port 8444 --streams 64 --runs 1 --settle 5 --throughput-runs 1`
- expected: "`p3.json`: throughput arm `ok: true`, 8 MiB received; RSS arm reaches `barrier: met` ... `attribution` printed" — or, on any precondition failure, an `INVALID` line and a result file (plan §Invalidity rules: "Every script checks its own preconditions and prints `INVALID` with the reason instead of a number")
- observed:
  `2026-09-03T13:15:23.704Z [p3] local address toward the probe: 127.0.0.1; https.interception.clients = ["127.0.0.1"]`
  `node:internal/modules/run_main:107`
  `Error: Client network socket disconnected before secure TLS connection was established`
  `    at TLSSocket.onConnectEnd (node:internal/tls/wrap:1819:19)`
  `  code: 'ECONNRESET',`
  `  host: '127.0.0.1',`
  `  port: 8444,`
- files: no `p3.json` written; smoke-20260903T1303Z/layer1/run.log ends at the interception-clients line
- repro: point `p3-h2stall.mjs` at a probe that resets the intercepted TLS connection (see F6) — the first `connectSession()` rejects and nothing catches it
- note: `connectSession()`'s TLS error is an unhandled rejection, so the script dies with a Node stack trace instead of the `INVALID` + result file every other script writes; on the router this loses the run and the evidence with it.

## F6 — smoke plan Layer 1 §1.2, p1/p3 origin-certificate one-liner — 1.2
- classification: plan-gap
- command: `openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout p1.key -out p1.crt -days 3 -subj "/CN=127-0-0-1.nip.io" -addext "subjectAltName=DNS:127-0-0-1.nip.io"`
- expected: "needs an h2 origin the binary trusts: ... run it instead of the release one with `FAH_TEST_UPSTREAM_ROOT=<origin-cert.der>`"
- observed:
  `2026-09-03T13:17:08.229962Z DEBUG fah_http::intercept: upstream certificate not verified; closing before our handshake peer=127.0.0.1:61187 host=127-0-0-1.nip.io address=127.0.0.1:443 error=invalid peer certificate: Other(OtherError(CaUsedAsEndEntity))`
  `X509v3 Basic Constraints: critical` / `CA:TRUE`
- files: smoke-20260903T1303Z/fah-harness-debug.log
- repro: `openssl x509 -in p1.crt -noout -text | grep -A1 'Basic Constraints'`
- note: `openssl req -x509` marks the self-signed certificate `CA:TRUE`, which rustls rejects as an end-entity certificate; the splice arm (p1) never validates it and passes, the interception arm (p3) cannot. The e2e harness's `self_signed_origin()` uses rcgen defaults (`CA:FALSE`, EKU serverAuth). The plan's one-liner needs `-addext "basicConstraints=critical,CA:FALSE" -addext "extendedKeyUsage=serverAuth"` for every interception arm.

## F7 — p3-h2stall.mjs / fah-http interception — 1.2
- classification: probe-behaviour
- command: `node p3-h2stall.mjs --probe 127.0.0.1 --api-port 8443 --key smoke-config/apikey --out <root>/layer1 --origin 127-0-0-1.nip.io --path /8mib --warmup-path / --ca smoke-ca.pem --port 8444 --streams 64 --runs 1 --settle 5 --throughput-runs 1`
- expected: "RSS arm reaches `barrier: met` on both a stall and a control run, samples present, `attribution` printed"
- observed:
  `[p3] throughput run 1: 157.882 MiB/s bytes=8388608`
  `[p3] stall run 1: warm-up 200 3B issuer=FastAdHunter CA`
  `[p3] stall run 1: barrier NOT MET (5/64 streams with :status 200 + first DATA, timeout)`
  `[p3] control run 1: barrier MET (64/64 streams with :status 200 + first DATA, met)`
  `[p3] control run 1: before=0 max=0 after=0 MiB delta=0 MiB drained=64/64 exclusive=true`
  `[p3] INVALID: RSS arm: 1 run(s) produced no figure: stall#1 barrier not met: 5/64 streams answered`
  per-stream dump, stall run 1 (`p3.json` `rss_runs[0].streams`, 64 entries):
  `5 x status=200 first_data=>0 ended=false error=closed`
  `59 x status=200 first_data=null ended=true error=null`
  probe log, 62 identical lines in the first stall attempt:
  `DEBUG fah_http::intercept: intercepted upstream request failed peer=127.0.0.1:54410 host=127-0-0-1.nip.io error=http2 error`
- files: smoke-20260903T1303Z/layer1/p3.json, smoke-20260903T1303Z/layer1/run.log, smoke-20260903T1303Z/fah-harness-debug.log
- repro: h2 origin on `127.0.0.1:443` serving `/8mib` (8 MiB) and `/`, dev-profile `--features test-harness` binary with `FAH_TEST_UPSTREAM_ROOT=p3.der`, `127.0.0.1` listed in `https.interception.clients`, then the command above
- note: this is the dev-box reproduction the P3 BLOCKED state owes. The matched control run (same 64 streams, drained instead of stalled) meets the barrier 64/64 and proves the setup; only the stall run fails. Every one of the 59 failing streams receives `:status 200` and is then ended by the probe with **zero DATA frames** and no error — the intercepted relay answers the header and closes the body once the client stops reading. No fix attempted in `fah-http`. The origin's own limits were ruled out first: with Node's default `maxSessionMemory` (10 MB) only 2/64 streams answered and the probe logged 62 `error=http2 error` lines; raising the origin to `maxSessionMemory: 4096` and `maxConcurrentStreams: 256` moved the control arm to 64/64 and left the stall arm at 5/64.

## F8 — /api/v1/debug/memory on Windows — 1.2
- classification: environment
- command: `curl -sk -H "Authorization: Bearer $(cat smoke-config/apikey)" https://127.0.0.1:8443/api/v1/debug/memory`
- expected: the P3 RSS arm reads `process_rss` before / during / after the window and prints a delta
- observed:
  `{"ruleset_bytes":2016003,...,"process_rss":null,"process_peak_rss":null,"major_page_faults":null,"minor_page_faults":null,"process_rss_anon":null,"process_rss_file":null,"allocator_committed_bytes":103092224,...}`
  `[p3] control run 1: before=0 max=0 after=0 MiB delta=0 MiB drained=64/64 exclusive=true`
- files: smoke-20260903T1303Z/layer1/p3.json, smoke-20260903T1303Z/layer1/run.log
- repro: run the curl above against the probe on this dev box
- note: `process_rss` is a Linux `/proc` read, so it is `null` on Windows and `p3-h2stall.mjs` renders it as `0`; even a stall run that met the barrier could not produce an RSS figure here. The RSS arm is only measurable in-container / on the router. The script does not flag the `null` — it prints `0 MiB` deltas as if they were samples.

## F9 — lib.mjs `snapshotConfig()` / smoke plan Layer 2 `--out <root>/layer2` — 2.6
- classification: plan-gap
- command: `node p1-lan.mjs --probe 127.0.0.1 --api-port 8443 --key smoke-config/apikey --out <root>/layer2 --origin 127-0-0-1.nip.io --origin-cert p1.crt --bytes 8 --port 8444` after `allow_destinations = []` + restart
- expected: "`INVALID: origin address … is not in egress.allow_destinations`"
- observed (shared `layer2` directory, a config.json from an earlier row already present):
  `2026-09-03T15:06:23.980Z [p1] egress.allow_destinations = ["127.0.0.0/8"]`
  `2026-09-03T15:06:23.980Z [p1] arm=spliced target=127.0.0.1:8444 sni=127-0-0-1.nip.io expected=8 MiB x 1 conn x 5 runs`
  `2026-09-03T15:06:23.997Z [p1] INVALID: no spliced sample completed 8 MiB on every connection`
  same command, `--out <root>/layer2/r6-egress` (fresh directory):
  `2026-09-03T15:06:24.779Z [p1] egress.allow_destinations = []`
  `2026-09-03T15:06:24.779Z [p1] INVALID: origin address 127.0.0.1 is not in egress.allow_destinations: the probe refuses the destination (plan §Environment)`
- files: smoke-20260903T1303Z/layer2/run.log, smoke-20260903T1303Z/layer2/r6-egress/p1.json
- repro: run any config-dependent row twice into one `--out` directory with a probe config change in between
- note: `snapshotConfig()` returns the existing `config.json` instead of re-reading `/api/v1/config` ("snapshots the probe's /config once per run directory", plan §Scripts). Every Layer 2 row that changes the probe config and reuses `--out <root>/layer2` therefore reads the previous row's config and its precondition fires on stale state — here the egress rule was skipped and the row failed for the wrong reason. Every config-changing row needs its own `--out` directory, or `config.json` deleted first. The same applies to the campaign: two runs into one `results-<ts>/` after a `SPLICE_BUF` rebuild would carry the first run's config.

## F10 — p6-certs-time.mjs archive-cap precondition — 2.14
- classification: script-bug
- command: `node p6-certs-time.mjs --probe 127.0.0.1 --key smoke-config2/apikey --out <root>/layer2/r14b-409 --skip-host-checks --generate 9 --import 0 --cert p1.crt --key-file p1.key` against a fresh store
- expected: "with the counts omitted, the ninth call itself `INVALID: ca/generate #9 answered 409`"
- observed:
  `[p6] ca/generate #8: 200 starttransfer-appconnect=1.846 ms total=3.316 ms`
  `[p6] ca/generate #9: 200 starttransfer-appconnect=1.552 ms total=3.032 ms`
  `[p6] == p6 done (degraded)`
  `ls smoke-config2/ca-archive | wc -l` -> `8`
  and, with the count supplied, the precheck still fires:
  `[p6] INVALID: ca-archive holds 0; 9 generates would pass the cap of 8`
- files: smoke-20260903T1303Z/layer2/r14b-409/p6.json, smoke-20260903T1303Z/layer2/r14-cap/p6.json
- repro: fresh `/config`, then 9 `POST /api/v1/certificates/ca/generate` calls; count `ca-archive/`
- note: off by one. The first generate on an empty store archives nothing, so N generates produce N-1 archives: 9 generates fill the cap exactly (8) and all answer 200. The `--ca-archive-count 0 --generate 9` precheck refuses a run the API would have accepted; conversely the plan's "ninth call 409" never happens. Cap is reached at generate #10 from an empty store.

## F11 — smoke plan Layer 2, row "barrier not met (`p3`)" — 2.11
- classification: plan-gap
- command: `node p3-h2stall.mjs --probe 127.0.0.1 --key smoke-config2/apikey --out <root>/layer2/r11-tiny --skip-host-checks --origin 127-0-0-1.nip.io --path / --warmup-path / --ca smoke-ca2.pem --port 8444 --streams 64 --runs 1 --settle 5 --throughput-runs 1`
- expected: "RSS arm `INVALID: … barrier not met` or `streams answered` count in the reason; throughput arm unaffected"
- observed:
  `[p3] throughput run 1: 0.073 MiB/s bytes=3`
  `[p3] degraded: throughput stream bytes 3 differ from --bytes 8388608`
  `[p3] stall run 1: barrier MET (64/64 streams with :status 200 + first DATA, met)`
  `[p3] control run 1: barrier MET (64/64 streams with :status 200 + first DATA, met)`
  `[p3] GATE (degraded, not a gate) {... "attribution":"UNRESOLVED","max_stall_delta_mib":0 ...}`
  `[p3] == p3 done (degraded)`
- files: smoke-20260903T1303Z/layer2/r11-tiny/p3.json, smoke-20260903T1303Z/layer2/r11-tiny/run.log
- repro: the command above against a tiny (3-byte) resource
- note: the row's premise — "a tiny resource: streams end before the window" — is backwards. The barrier only asks for `:status 200` plus a first DATA chunk, and a 3-byte body delivers both immediately on all 64 streams, so the barrier is *met*, not missed; the run degrades on the throughput byte check instead. Read together with [[F7]] this narrows the P3 failure: 64 streams alone are fine, 64 x 8 MiB is not. A row that actually misses the barrier needs a resource that never sends DATA (a slow or header-only origin), not a small one.

## F12 — smoke plan Layer 2, row "busy host (all, Windows)", `--allow-busy` half — 2.4
- classification: environment
- command: not run — `node p0-sni.mjs ... --allow-busy` with a browser open
- expected: "with `--allow-busy` the result carries `status: \"degraded\"`"
- observed (the `INVALID` half, twice, unprompted, with a browser that had reopened itself):
  `2026-09-03T13:10:38.825Z [p1] idle check: FAIL chrome (recorded; each stage decides)`
  `2026-09-03T13:10:38.856Z [p1] INVALID: host not idle: chrome running (plan §Running item 3); --allow-busy to proceed degraded`
  `2026-09-03T15:03:58.398Z [p5] idle check: FAIL chrome (recorded; each stage decides)`
  `2026-09-03T15:03:58.406Z [p5] INVALID: host not idle: chrome running (plan §Running item 3); --allow-busy to proceed degraded`
- files: smoke-20260903T1303Z/layer1/run.log, smoke-20260903T1303Z/layer1/p1.json, smoke-20260903T1303Z/layer1/p5.json
- repro: start any process named in `lib.mjs` `BUSY_PROCESSES` and run any script
- note: the owner asked mid-session that the browser not be closed or started by the agent, so the `--allow-busy` -> `degraded` half was left unrun; every remaining row after that point used `--skip-host-checks`, which logs `host checks skipped (win32)` and records no idle state. The `INVALID` half is proven by the two firings above.

## F13 — smoke plan Layer 2, row "both addresses unlisted (`p2`, Linux)" — 2.9
- classification: environment
- command: not run — `node p2-handshake.mjs --listed <A> --unlisted <B> ...` with `clients = []`
- expected: "`INVALID: … exactly one of the two addresses must be in https.interception.clients`"
- observed: no Linux host can reach this probe — see [[F4]]
- files: (none)
- repro: `wsl.exe -l -v`
- note: same blocker as F4; the rule is unexercised on this dev box. The Windows identity precondition fires first and hides it, so the branch is only reachable from the VM.
