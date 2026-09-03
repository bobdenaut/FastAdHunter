# p3-06 probe smoke — findings (smoke-20260903T1844Z, bobdenaut)

Clean rerun of the smoke plan on the tree as committed after
smoke-20260903T1303Z (F1–F13) and smoke-20260903T1557Z (F14–F22): the probe
scripts, the smoke plan and two production defects (fah-dns DoT/TCP framing,
fah-http h2 connection window) were fixed and committed before this session.
Functional / negative-path / container coverage only. **Nothing here is a
measurement**; no number below is a result, a budget or a comparison.

Tip: `92e3f4a9b930a152a03186f790650a71233e24a7`, recorded by every `run.log` as
`92e3f4a9b930-dirty` — the owner's two uncommitted `docs/code-review/phase2.6`
files, out of scope for this session and approved to run over.

Numbering continues from F22.

## F23 — p3-h2stall.mjs — 1.2
- classification: environment
- command: `node p3-h2stall.mjs --probe 127.0.0.1 --api-port 8443 --key smoke-config/apikey --out <root>/layer1-b --origin 127-0-0-1.nip.io --path /8mib --warmup-path / --ca smoke-ca.pem --port 8444 --streams 64 --runs 1 --settle 5 --throughput-runs 1`
- expected: "`p3.json`: throughput arm `ok: true`, 8 MiB received" (smoke plan §1.2 row `p3`)
- observed:
  `[p3] args {... "path":"C:/Program Files/Git/8mib","warmup-path":"C:/Program Files/Git/" ...}`
  `[p3] throughput run 1: FAILED bytes=0 error=ERR_HTTP2_STREAM_ERROR`
  `[p3] INVALID: throughput arm: no stream completed (the P3 BLOCKED state?)`
- files: (overwritten by the corrected rerun in the same directory: smoke-20260903T1844Z/layer1-b/p3.json, smoke-20260903T1844Z/layer1-b/run.log)
- repro: run any `--path /...` flag from Git Bash / MSYS on this box without `MSYS2_ARG_CONV_EXCL='*'`
- note: not a script defect and not a probe defect — MSYS path conversion rewrites a bare `/8mib` argument into a Windows path before Node ever sees it, so the probe requested a path the origin does not serve. Recorded because the script's `INVALID` reason then names **the P3 BLOCKED state** as the suspected cause, which is exactly the wrong conclusion to draw from a mistyped or shell-mangled path; the campaign drives these scripts from this laptop (testing-plan §Running), so the same reason line could be read as a reproduction of [[F7]]. Prefixing the command with `MSYS2_ARG_CONV_EXCL='*'` (or running it from PowerShell) makes the row pass unchanged — it did, in the same directory, immediately after.

## F24 — p3-h2stall.mjs / smoke plan Layer 3 row `p3` — 3.3
- classification: environment
- command: `node -e '...h2.connect("https://speed.cloudflare.com")...'` (the smoke plan's own preflight one-liner), path `/__down?bytes=8388608`
- expected: "`alpn h2` and `bytes` = 8 388 608 are required" (smoke plan Layer 3, row `p3-h2stall.mjs` RSS rows)
- observed:
  `speed.cloudflare.com /__down?bytes=8388608 alpn false stream-error ERR_HTTP2_ERROR`
  `speed.cloudflare.com /__down?bytes=8388608 error ERR_HTTP2_ERROR`
  `echo | openssl s_client -connect speed.cloudflare.com:443 -servername speed.cloudflare.com -alpn h2` prints no `ALPN protocol:` line at all; the handshake completes (`issuer=C=US, O=Google Trust Services, CN=WE1`, `verify return:1`)
  same one-liner against `cloudflare.com /cdn-cgi/trace`: `alpn h2 status 200 content-length 231 bytes 231`
- files: smoke-20260903T1844Z/SMOKE-REPORT.md §Layer 3 (preflight table)
- repro: `node ./h2pre.mjs speed.cloudflare.com "/__down?bytes=8388608"` from this box, or the openssl line above
- note: reappearance of the same condition the previous session recorded against this origin — the failure is host-specific, not a box-wide h2 or TLS failure, since `cloudflare.com`, `cdn.jsdelivr.net`, `mirror.nl.leaseweb.net` and `sabnzbd.org` all negotiate `alpn h2` from the same shell minutes apart. `speedtest.london.linode.com` and `test.rebex.net` fail identically (`alpn false`, `ERR_HTTP2_ERROR`), so more than one host is affected. Seven origins were preflighted before one met the plan's condition; the rows were run against `sabnzbd.org/tests/internetspeed/5MB.bin` (`alpn h2`, 5 242 880 bytes = exactly 5 MiB, no `content-length` — `p3-h2stall.mjs:253` skips the length check when the header is absent), with the owner's explicit approval for the outbound burst. The campaign still owes a P3 origin it can rely on from the device, and `speed.cloudflare.com` is not it from this network.

## F25 — Dockerfile.p4 / encrypted_latency harness — 3.1
- classification: probe-behaviour
- command: `docker run --rm fah-p4:smoke`
- expected: the DoT column sits in the same range as UDP and DoH — the condition the tip's `fix(fah-dns): frame each TCP/DoT reply in one write and set TCP_NODELAY` (`3883e55`) was committed to establish after [[F16]] / [[F19]]
- observed:
  `round 1: p50 udp=53us dot=44024us doh=80us; DoT handshake 547.961µs`
  `round 2: p50 udp=47us dot=44003us doh=99us; DoT handshake 612.705µs`
  `round 3: p50 udp=54us dot=44004us doh=78us; DoT handshake 599.71µs`
  `udp        n=6000 min=25us p50=53us p90=62us p99=115us max=306us`
  `dot        n=6000 min=67us p50=44011us p90=44330us p99=48101us max=61796us`
  `doh-post   n=6000 min=70us p50=87us p90=130us p99=360us max=1230us`
  `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 266.38s`
- files: smoke-20260903T1844Z/layer3-p4-run.log, smoke-20260903T1844Z/build-images.log
- repro: `docker buildx build --platform linux/amd64 --load -f docs/code-review/phase3/p3-06-probe/Dockerfile.p4 -t fah-p4:smoke . && docker run --rm fah-p4:smoke`
- note: **not a measurement and not a budget claim** — the shape is the finding. [[F16]] / [[F19]] are **improved but not resolved**. The smoke plan's three structural conditions for this row all hold (`FAH_E2E_BINARY override active: /fah-probe`, three transports at 2 000 queries per round, no `EACCES`), so the row itself passes; but the DoT per-query column still sits three orders of magnitude above UDP and DoH on the same connection, stable across all three rounds, while the DoT *handshake* stays sub-millisecond. The one thing the fix did change is the size of the step: last session's run on the pre-fix tree showed `dot p50 = 88003us` with a floor of `40723us`; this run shows `p50 = 44011us` with `min = 67us`. That is one delayed-ACK interval instead of two — consistent with the commit removing one of two short writes per reply, leaving one. The `min = 67us` is new and matters: at least one reply in 6 000 took the fast path, so the listener is not unconditionally slow, which a pure framing bug would be. Whatever remains is a second write-coalescing site the commit did not cover. P4 cannot be read off this harness until it is explained.
- fixed: `145bc17` — the remaining step was the harness's own DoT client, not `fah-dns`: `time_dot` wrote the length prefix and the query as two `write_all` calls without `TCP_NODELAY`, the mirror of the server-side defect `3883e55` removed. One framed write (`common::framed_query`) plus `set_nodelay(true)`, shared with the e2e DoT client. In-container rerun `smoke-20260903T1949Z/layer3-p4-run.log`: the DoT p50 sits with UDP in all three rounds, no step. [[F16]] and [[F19]] close with it.

## F26 — Dockerfile.p4 / encrypted_latency harness — 3.1
- classification: probe-behaviour
- command: `docker run --rm fah-p4:smoke`
- expected: P4 measures "DoT / DoH added latency vs UDP" (testing-plan §Scripts row `Dockerfile.p4`), comparable with the `p4-lan.mjs` DoH arm, which speaks h2
- observed:
  `pooled over 3 rounds; DoT handshakes (excluded from per-query figures) [547.961µs, 612.705µs, 599.71µs]; DoH over Some(HTTP/1.1)`
- files: smoke-20260903T1844Z/layer3-p4-run.log
- repro: `docker run --rm fah-p4:smoke`, read the `pooled over 3 rounds` line
- note: [[F17]] reappears verbatim and unfixed — the in-device harness still negotiates **HTTP/1.1** for its DoH arm while `p4-lan.mjs` drives DoH over h2 (`layer1-a/p4-lan.json` records `"protocol":"doh over h2"`). The two DoH columns describe different protocols, so P4 and P4-LAN remain non-comparable on that transport even after the labelling rule in §Invalidity rules is applied. Nothing in the tip's two commits touched ALPN on the harness client, so this is expected to persist; recorded so it is not read as new.
- fixed: `145bc17` — `reqwest` dev-dependency feature `http2` (the workspace definition carries `default-features = false`, so h2 was never on); `time_doh` asserts `HTTP/2.0` on every response. Testing-plan delta 12 declares the h2 arm; D13's HTTP/1.1 seed is not the comparator. In-container rerun `smoke-20260903T1949Z/layer3-p4-run.log`: `DoH over Some(HTTP/2.0)`, assertion held on 6 000 responses. [[F17]] closes with it.

## F27 — smoke plan Layer 3, `docker run` line for `Dockerfile.fahprobe` — 3.3
- classification: environment
- command: `docker run --rm -v "$PWD/smoke-config:/config" -v "$PWD/smoke-data:/data" -p 8443:8443 -p 8444:8444 -p 8853:853 -p 5300:53/udp fah-probe:smoke` (the smoke plan's line, run from Git Bash)
- expected: the probe boots on the mounted store — "Then Layer 1 again against it (`--probe 127.0.0.1`, same ports). Passes when every Layer 1 row passes unchanged"
- observed:
  `INFO fastadhunter: fastadhunter starting config_path=/config/fastadhunter.toml data_dir=/data mode=Dns`
  `WARN fah_dns::upstream: all upstreams failed — answers now depend on cached entries upstreams=2 suppressed=0 error=upstream timed out`
  `docker inspect --format '{{range .Mounts}}...'`:
  `bind src=E:\FastAdHunter\smoke3-config;C dst=\Program Files\Git\config`
  `bind src=E:\FastAdHunter\smoke3-data;C dst=\Program Files\Git\data`
  `volume src=/var/lib/docker/volumes/c902b5ca…/_data dst=/config`
  `volume src=/var/lib/docker/volumes/49fcf413…/_data dst=/data`
  host-side mount directory after boot: `fastadhunter.toml` only — no `apikey`, no `api-cert.pem`
- files: smoke-20260903T1844Z/layer3-fahprobe-boot.log
- repro: run the plan's `docker run -v "…:/config"` line from Git Bash / MSYS without `MSYS2_ARG_CONV_EXCL='*'`
- note: second instance of the same shell trap as [[F23]], and the more dangerous one. MSYS rewrites the `:` in the `-v host:container` spec, so Docker parses `…smoke3-config;C` as the source and `\Program Files\Git\config` as the destination; `/config` and `/data` then fall back to **anonymous volumes**, the probe never sees the mounted TOML, and it boots on compiled-in defaults — `mode=Dns` instead of `dns+http+https`, two default upstreams instead of the configured one. Nothing errors: the container starts, answers `/health`, and every https-dependent Layer 1 row would then fail against it for a reason that has nothing to do with the image. Worth a line in the plan's Layer 3 block, since that block's commands are written in POSIX shell and this box's default shell is Git Bash. Prefixing `MSYS2_ARG_CONV_EXCL='*'` and using a native `E:\…` source path boots it correctly (`mode=DnsHttpHttps`, `list refreshed … active=62948`, `apikey` written into the mount) — it did, immediately after.

## F28 — p4-lan.mjs against the fah-probe image — 3.3
- classification: probe-behaviour
- command: `node p4-lan.mjs --probe 127.0.0.1 --api-port 8443 --key smoke3-config/apikey --out <root>/layer3-l1a --domain ads.smoke.test --dot-host dns.smoke.test --dns-port 5300 --dot-port 8853 --queries 200 --rounds 1`
- expected: "all three transports `n = 200`, `unanswered = 0`, `unmatched = 0`, `first_answer.answers` = `["0.0.0.0"]`, dot `served_issuer` = `FastAdHunter CA`" — and, this session, the DoT column in the same range as UDP and DoH
- observed:
  `[p4-lan] round 1 udp p50=0.347 p99=0.543 ms n=200 unanswered=0 unmatched=0 wall=74.963ms hs=-`
  `[p4-lan] round 1 dot p50=0.33 p99=0.626 ms n=200 unanswered=0 unmatched=0 wall=76.451ms hs=13.832`
  `[p4-lan] round 1 doh p50=0.483 p99=1.083 ms n=200 unanswered=0 unmatched=0 wall=148.485ms hs=5.139`
  `[p4-lan] GATE {"statistic":"none — diagnostic","dot_p50_minus_udp_p50_ms":-0.017,"doh_p50_minus_udp_p50_ms":0.136}`
  `[p4-lan] == p4-lan done (valid)`
- files: smoke-20260903T1844Z/layer3-l1a/p4-lan.json, smoke-20260903T1844Z/layer3-l1a/run.log
- repro: build and run the `fah-probe` image as in the smoke plan Layer 3, seed a user rule and a CA, then run the command above against the mapped ports
- note: **the row passes, and [[F19]] is fixed** — recorded because it changes what [[F25]] means. F19 was the same script against the same containerised probe showing `dot p50=43.904` against `udp p50=0.368`; on this tip the step is gone and the DoT column sits below UDP. So `3883e55` did resolve the Linux DoT write path for this client. But the `Dockerfile.p4` harness, running against a probe built from the same commit in the same session, still shows a ~44 ms DoT step (F25). Two clients, one probe, one tree: the remaining step therefore tracks **the harness's DoT client**, not `fah-dns`. That is F19's attribution reversed and [[F16]]'s original guess restored — recorded here rather than by editing either entry.

## F29 — p3-h2stall.mjs RSS arm against the fah-probe image — 3.3
- classification: probe-behaviour
- command: `node p3-h2stall.mjs --probe 127.0.0.1 --api-port 8443 --key smoke3-config/apikey --out <root>/layer3-p3 --origin sabnzbd.org --path /tests/internetspeed/5MB.bin --warmup-path / --ca smoke3-ca.pem --port 8444 --bytes 5 --streams 64 --runs 1 --settle 5 --throughput-runs 1`
- expected: "Pass: throughput arm `ok: true`; RSS arm `barrier: met` on the control run, samples present, `attribution` printed" (smoke plan Layer 3, `p3-h2stall.mjs` RSS rows)
- observed:
  `[p3] throughput run 1: 11.348 MiB/s bytes=5242880`
  `[p3] stall run 1: barrier MET (64/64 streams with :status 200 + first DATA, met)`
  `[p3] stall run 1: before=50.04 max=70.28 after=68.09 MiB delta=20.246 MiB drained=0/64 exclusive=true`
  `[p3] control run 1: barrier MET (64/64 streams with :status 200 + first DATA, met)`
  `[p3] control run 1: before=68.11 max=73.3 after=65.71 MiB delta=5.195 MiB drained=64/64 exclusive=true`
  `[p3] GATE {"throughput":{"statistic":"median of runs >= 50 MiB/s","median_mib_s":11.348,"pass":false},"rss":{"statistic":"max stall delta over runs vs ~5.5 MiB, read only when RESOLVED","attribution":"RESOLVED","max_stall_delta_mib":20.246,"reading_against_5_5_mib":"above — a finding, not a fail"}}`
  `[p3] == p3 done (valid)`
- files: smoke-20260903T1844Z/layer3-p3/p3.json, smoke-20260903T1844Z/layer3-p3/run.log, smoke-20260903T1844Z/layer3-p3/raw.jsonl, smoke-20260903T1844Z/layer3-p3-preflight.log
- repro: the command above, against the `fah-probe:smoke` image with `127.0.0.1` and the bridge gateway listed in `https.interception.clients`
- note: **not a measurement** — one run, `--streams 64` at `--bytes 5` over a WAN origin on a busy dev box, and the plan forbids reading a number off this layer. Two structural things are worth the owner's eye. First, **[[F7]]'s stall barrier is met**: `64/64 streams with :status 200 + first DATA`, where the p3-04 BLOCKED state and the previous two smoke sessions had 5/64 or could not run the arm at all. `84d34be fix(fah-http): size the intercepted h2 connection window for 64 stalled streams` does what it was committed to do, and the P3 RSS arm has now produced a figure somewhere for the first time — `attribution: RESOLVED`, exclusive window proven on both runs (`connections_delta: 2`, `dns_flat: true`), issuer `FastAdHunter CA`, `alpn h2`, `authorized: true`. Second, the script's own gate line flags the stall delta as `"above — a finding, not a fail"` against its ~5.5 MiB reference, with the matched control run an order of magnitude below it. That is the script doing its job, not a result: it says the arm is now capable of producing the P3 figure and that the first thing it produced wants explaining. The real reading belongs to the campaign, on the device, against a P3 origin the plan trusts — which, per [[F24]], it does not yet have.
