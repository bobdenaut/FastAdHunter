# Task — HTTP allocation domain: branch, implement, A/B on the probe

Written 2026-09-06 evening at the end of the buffer A/B sitting
([resoak-0.3.1-H1_MAX_BUF-diagnosis.md](resoak-0.3.1-H1_MAX_BUF-diagnosis.md)
§RB5009 buffer A/B). Self-contained prompt for a fresh session. Every "go"
below is a separate owner decision; nothing here is a permission.

## Prompt

```text
Role: implement the HTTP allocation domain from
docs/code-review/phase2.6/allocation-domains-proposal.md on a new branch and
A/B it on the RB5009 probe container. Read-only against production and the
router; the owner runs every router command. Owner rules, absolute: NO router
change by the agent (read-only queries via `ssh bobdenaut '<cmd>'` are fine
and need no ask); NO commit, tag or push without a go for that exact
changeset; NO .md edit without a go per edit (this file included); scp needs
permission; production and probe APIs only via GET with the key in
E:/FastAdHunter/.vscode/production.key read into a shell variable, never
printed. No comments in Rust code (hook rejects them). Answer in English,
briefly, step by step, one action then wait for the owner's output when the
step is his.

Read first: root CLAUDE.md and plan/CLAUDE.md (load by themselves), then
allocation-domains-proposal.md §Summary, §Proposed topology, §Ownership rule,
§Validation plan (step 1 FAIL and step 1b), then
resoak-0.3.1-H1_MAX_BUF-diagnosis.md §RB5009 buffer A/B, then
docs/routeros-traps.md §Build and deploy pipeline and §On-device measurement,
docs/measurement-traps.md §Memory.

Established 2026-09-06, do not re-derive:
- Production = 0.3.1 = db2f9b2, container comment "fastadhunter", veth1
  172.17.0.2, one tokio runtime, 4 workers, mimalloc, envlist fah-env
  (MIMALLOC_ARENA_EAGER_COMMIT=0, MIMALLOC_PURGE_DECOMMITS=1,
  MIMALLOC_PURGE_DELAY=0). The 0.3.1 soak is treated by the owner as
  invalidated for G2 (verification waves and dev-box dry runs went through it).
- main = 64be513 = db2f9b2 + 12 commits; crate code differs only in
  fah-dns/src/upstream/mod.rs (RTT accounting) and fah-stats/src/aggregates.rs
  (hit rate). HTTP path identical to 0.3.1.
- 4eddc39 (tip of phase3-06) = per-listener concurrent_connections high-water
  in the perf sample (fah-http ConnectionGauge; fah-model
  ConcurrentConnections {http, https}; fah-stats reader; fah-api ?fields=;
  API.md). Cherry-pick onto main conflicts in crates/fah-http/src/lib.rs and
  server.rs (Phase 3 reshaped them) and needs tls_server.rs dropped (absent
  on main, https stays 0); the other files apply clean.
- Buffer A/B result: no cap below 256 KiB passes the owner's rule (<= +10 %
  CPU); 128 KiB halves the step for +14..20 % CPU during transfers. Owner
  decision: keep hyper's 408 KiB default, fix the RETURN of memory after a
  burst with the allocation domain, not the height. On an idle process every
  408 arm held ~100 % of its +56..+69 MiB step for 15 min; production returns
  in ~45 min only through DNS churn cycling the owners (F29). The domain's
  own step 1b showed a single-thread HTTP runtime returning a burst within one
  30 s sample on the dev box. That return time is the metric this A/B decides.
- At ~90 MiB/s the proxy costs 11.7..13.3 ms CPU per MiB, i.e. 1.2 cores
  average and more during 8-parallel waves. One single-thread HTTP runtime
  cannot carry that; N = max(1, cores/2) = 2 on the RB5009.
- 0.3.1 HTTP server today (crates/fah-http/src/server.rs): Server::bind
  (one TcpListener via bind_tcp, Arc<Semaphore>(max_connections)),
  serve(&mut self, Arc<Proxy>) spawns accept_loop on the caller's runtime,
  accept_loop acquires a permit, accepts, tokio::spawn(serve_connection) per
  connection, shutdown() aborts the acceptor only. main.rs: run() builds one
  new_multi_thread runtime (enable_all) and block_on's Engine::start; start()
  binds dns, http, api; http.serve(proxy) once; shutdown() calls
  dns/http/api shutdown. Proxy (proxy.rs) owns the hyper-util Client (pool
  idle 60 s, 8 idle per host) built inside Proxy::new; rules/policies/events
  are Arcs and a Sender. Cargo: tokio features macros, rt-multi-thread,
  signal; socket2 0.6 is a workspace dep. hyper 1.10.1, hyper-util 0.1.20.
- Owner design decisions (2026-09-06): (1) branch from main at commit
  64be5138c79294622dd644787f8f521e0263a0b7 (short 64be513; verify with
  `git rev-parse main` before branching, and stop if main has moved),
  meaningful name — use alloc-domains/http; (2) hand-off = one acceptor plus
  channel, NOT SO_REUSEPORT (unix-only; gates run on the Windows dev box);
  (3) max_connections = ONE shared semaphore across the N runtimes; (4) each
  HTTP runtime is a current_thread runtime on its own std thread — never one
  runtime with N workers (F36: intra-runtime migration recreates the
  retention); (5) 4eddc39 is taken, first, as its own commit.
- Probe container on the router: comment "fah-h1buf", veth3 172.17.0.4/24 on
  bridge CONTAINERS, root-dir /kingston/fah-h1buf/root, mountlists
  h1buf-config (kingston/fah-h1buf/config: live 0.3.1 TOML + probe-only
  [egress] allow_destinations=["192.168.10.10/32"], allow_ip_literal_hosts=
  true, production apikey) and h1buf-data (kingston/fah-h1buf/data, lists
  cached), envlist h1buf-env (fah-env keys + FAH_BENCH_H1_CLIENT_BUF_KIB=408,
  FAH_BENCH_H1_UPSTREAM_BUF_KIB=408, harmless to the new builds). STOPPED,
  not removed. Tar fastadhunter-h1buf-db2f9b2-rosready.tar still on kingston/.
  One veth carries one container at a time: a new probe image means the owner
  removes fah-h1buf and adds the new container on veth3 reusing the mount
  lists and the envlist (/container/add file=... interface=veth3
  root-dir=/kingston/<name>/root mountlists=h1buf-config,h1buf-data
  envlists=h1buf-env workdir=/home/nonroot logging=yes start-on-boot=no
  comment="<name>"). /container/envs keys are addressed by list=, not name=.
- Router facts: free memory 658 MiB of 1024 (5 d uptime), container
  memory-high unlimited, RouterOS 7.21.5. NAT rule 10 redirects LAN port 80
  to production except src 172.17.0.0/24, so the probe's own fetches bypass
  production and the harness (dst port 8080) is never redirected. Any HTTP
  to port 80 from a LAN host — including Docker containers on the dev box —
  goes through PRODUCTION: never run a dev-box HTTP dry run against a WAN
  origin without knowing that.
- Harness: E:/fah-diag/tools/h1buf-ab.sh (dev box, Git Bash, curl + jq).
  Per arm: SETTLE=180 N=5 ARM=<label> KEY_FILE=E:/FastAdHunter/.vscode/
  production.key bash E:/fah-diag/tools/h1buf-ab.sh. Defaults PROBE=
  172.17.0.4, PORT=8080, API=https://$PROBE:8443 (-k), ORIGIN=
  cachefly.cachefly.net, SINGLE=/100mb.test, PAR=/10mb.test, TAIL=1 (+3 min
  and +15 min samples). Output E:/fah-diag/out/h1buf/<ARM>-<stamp>/
  {summary.txt,runs.txt,samples.jsonl}. It reads cpu_user_ms/cpu_system_ms
  from /api/v1/debug/memory, present only in the bench builds (fah-model
  ProcessStats + fastadhunter process.rs getrusage + fah-api MemoryResponse;
  patch in worktree E:/FastAdHunter-var-h1buf031, uncommitted). Carry those
  two fields into both builds below or the CPU column reads null.
- Build pipeline: docker buildx --platform linux/amd64 -t <tag> --load . for
  a dev-box smoke; --platform linux/arm64 -o type=docker,dest=<raw>.tar for
  the router, then skopeo (docker run --rm -v "<dir>:/work"
  quay.io/skopeo/stable copy --insecure-policy oci-archive:/work/<raw>.tar
  docker-archive:/work/<name>-rosready.tar:fastadhunter:<tag>). ~15 MB.
  Docker Desktop must be running; arm64 under QEMU takes ~15 min. Dev-box
  smoke containers cannot reach UDP 53; use DoT upstreams (address 1.1.1.1,
  protocol "dot", hostname cloudflare-dns.com) in a throwaway config copy.

Step 1 — branch and the gauge (stop before the commit).
 1a git branch alloc-domains/http 64be5138c79294622dd644787f8f521e0263a0b7
    (= main on 2026-09-06); git switch alloc-domains/http.
 1b git cherry-pick 4eddc39; resolve: apply the gauge to the 0.3.1 accept
    loop in server.rs (field, bind, serve, accessor, accept_loop param,
    enter() before spawn, guard held with the permit), lib.rs mod + pub use,
    drop tls_server.rs, keep https = 0 in ConcurrentConnections.
 1c cargo fmt --all -- --check; cargo clippy --workspace --all-targets
    --message-format=short -- -D warnings; cargo test --all-features
    --workspace. Green, then STOP: report and wait for the commit go
    ("feat(fah-http): per-listener concurrent_connections high-water mark",
    one commit, the cherry-pick only).

Step 2 — the HTTP allocation domain (stop before the commit).
 2a Config: [runtime] http_runtimes: usize, default max(1,
    available_parallelism/2), 0 = legacy shared runtime (keeps the A arm in
    the same image; decide later whether 0 stays a product option). Every
    key ships with a compiled-in default; no hand-edited TOML in production.
 2b fah-http: HttpServerGroup (name per CONTEXT.md, propose "allocation
    domain" as the term, CONTEXT.md entry is its own go). One acceptor task
    on the base runtime: acquire the SHARED semaphore permit, accept,
    set_nodelay, TcpStream::into_std, send (stream, peer, permit) to runtime
    k by round-robin over N bounded channels. Runtime k = std::thread named
    fah-http-k running tokio Builder::new_current_thread().enable_all();
    inside it a receiver loop: TcpStream::from_std, spawn_local or spawn on
    that runtime proxy_k.serve_connection(stream, peer) holding the permit.
    Each runtime owns its own Proxy (own hyper-util Client and pool); shared
    Arcs for resolver, rules, policies, counters (one ProxyCounters so
    ProxyStats and the gauge stay global) and a cloned events Sender.
    Ownership rule: a block is freed on the thread that allocated it; the
    only cross-domain drops left are the accepted socket state (tiny, per
    accept), resolver replies and events (already so today) — list them in
    the code-review file, not in code comments.
 2c Shutdown: shutdown signal -> acceptor stops -> each runtime stops taking
    from its channel, drains open connections with a bounded wait (<= 5 s),
    shutdown_timeout on the runtime, main joins the threads. Must finish
    inside RouterOS stop-time=10s; a SIGTERM-with-open-connections test.
 2d Tests: group with N=2 on port 0 serves and balances; N=0 behaves as
    0.3.1; shutdown drains; shared max_connections holds across N; existing
    server.rs and proxy tests untouched or ported. Gates as 1c. STOP for the
    commit go ("feat(fah-http): HTTP allocation domain — N single-thread
    runtimes behind one acceptor").

Step 3 — images (permission before scp).
 3a Tag both arms: A = this branch with http_runtimes=0 (legacy path), B =
    http_runtimes=2. One image serves both if 2a keeps the 0 switch; the
    arm is then a TOML line in the probe's config (owner edits
    kingston/fah-h1buf/config/fastadhunter.toml, or two config dirs) plus a
    restart. If the switch is dropped, two images and container swaps.
 3b amd64 --load, dev-box smoke (log line naming N, /api/v1/debug/memory
    with cpu_* fields, one proxied fetch), then arm64 raw tar, skopeo,
    ls -la. Ask permission, then scp <rosready>.tar bobdenaut:kingston/.
 3c Propose the exact /container commands (remove fah-h1buf, add the new
    container on veth3 reusing the mount lists and envlist, start, /log
    print where topics~"container") and STOP; the owner runs them and pastes
    the log. Verify N in the start log.

Step 4 — A/B on the probe, one sitting, same hour.
 4a Arms in order: A (N=0), B (N=2), A control, then B repeat if the return
    time is not unambiguous. Each arm is an owner-run restart of the probe;
    settle 180 s; run the harness with ARM=dom-A-a / dom-B / dom-A-b /
    dom-B-b. Nothing else touches the probe between arms.
 4b Decisive metric: RSS at +3 min and +15 min relative to peak and to the
    cache-start floor (~43 MiB). A is expected to hold ~100 % of the step for
    15 min (today's four 408 arms did); B passes on memory if it returns to
    within the purge band (measurement-traps §Memory, ±6 MB) of the floor by
    +3 min. Also report: peak step, CPU s and ms/MiB (band today 10.55–11.95 s
    for 900 MiB), 8-parallel MiB/s (86–90) and p95 (0.80–0.83), single-stream
    median (noisy on WAN, secondary), minor faults.
 4c Verdict per allocation-domains-proposal.md §Scope: MEMORY (return time),
    DNS (probe serves none; skip, note it), HTTP (B within 10 % of A on
    8-parallel and p95), CPU (reported; the owner's +10 % bar applies),
    SHUTDOWN (2c test plus /container/stop with open connections, no SIGKILL
    in /log), 7 DAYS (a later soak, not this sitting).
 4d Write the results into this file §Results (go), then, only if B passes:
    ADR, CONTEXT.md entry, CONFIGURATION.md [runtime] section, API.md if the
    perf sample changes — each its own go. If B fails: numbers here, branch
    kept, proposal closed with the reason.

Stop points: after 1c, after 2d, before scp in 3b, before every router
command in 3c, before every .md edit.
```

## Results

Run 2026-09-06 19:02–20:36Z, one sitting. Branch `alloc-domains/http` =
`64be513` + `0ed7d1e` (gauge cherry-pick) + `14e0bdd` (allocation domain) +
`b254977` (`cpu_user_ms` / `cpu_system_ms` on `/api/v1/debug/memory`), not
pushed. One image for both arms, tag `fastadhunter:alloc-b254977`, tar
`fastadhunter-alloc-b254977-rosready.tar` on `kingston/`. Probe container
`fah-alloc` on `veth3` (172.17.0.4), root-dir `/kingston/fah-alloc/root`, mount
lists `h1buf-config` / `h1buf-data`, envlist `h1buf-env` (mimalloc keys plus
`FAH__RUNTIME__HTTP_RUNTIMES`, the arm switch; the two `FAH_BENCH_H1_*` keys
removed). `fah-h1buf` removed first. Production untouched.

### What was built

- `[runtime] http_runtimes` (fah-config): default `max(1, cores/2)`, `0` =
  the 0.3.1 shared-runtime path. Env override `FAH__RUNTIME__HTTP_RUNTIMES`.
- `fah_http::Server::serve_domains(NonZeroUsize, drain, factory)`: one
  acceptor task on the base runtime takes the shared `max_connections` permit,
  accepts, `set_nodelay`, `into_std`, round-robins the socket + permit + gauge
  guard over N bounded channels (32 deep). Domain k = std thread `fah-http-k`
  running a `current_thread` runtime; `from_std`, spawn into a `JoinSet`. Each
  domain builds its own `Proxy` (own hyper-util client and pool) on its own
  thread; resolver, rules, policies and one `ProxyCounters` are shared `Arc`s,
  the events `Sender` is cloned.
- Shutdown: `Server::shutdown(&mut self)` aborts the acceptor, flips a `watch`,
  each domain closes its inbox, serves what was queued, drains open
  connections for at most `HTTP_DRAIN_TIMEOUT` = 5 s, aborts the rest,
  `shutdown_timeout(1 s)` on the runtime; the binary joins the threads.
- Cross-domain drops left (ownership rule): the accepted socket state (std
  stream, peer, permit, gauge guard; base → domain, per accept), resolver
  replies (DNS runtime → domain, as in 0.3.1), events (domain → fan-out on the
  base runtime, as in 0.3.1), shared `Arc`s at shutdown only.
- Tests: two domains built on their own threads and served in strict
  round-robin; one `max_connections` ceiling across domains; shutdown answers
  an in-flight request; drain bounded, leftover connection closed. Dev-box
  smoke (amd64, `docker stop` with a hanging connection): 6.5 s, exit 0.

### Arms

Harness `E:/fah-diag/tools/h1buf-ab.sh`, N=5, WAN origin
`cachefly.cachefly.net`, 900 MiB per arm (5 × 100 MiB single, 5 × 8 × 10 MiB
parallel), RSS from `/api/v1/debug/memory` `process_rss`. Every arm is an
owner-run envlist change plus restart; transfers began ~186 s after the start
except A-a (harness started 3 min after the start with a 30 s settle, and its
floor is the one non-cache-start figure). Raw series
`E:/fah-diag/out/h1buf/dom-*`.

| arm | start UTC | floor | peak | end | +3 min | +15 min | step held | CPU s | ms/MiB | par8 med MiB/s | p95 s | single med | minflt |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| A-a (N=0) | 19:02 | 37.57 | 98.86 | 98.86 | 97.48 | 97.42 | +59.9 | 12.18 | 13.5 | 89.69 | 0.789 | 92.8 | 6765 |
| B-a (N=2) | 19:23 | 43.11 | 65.80 | 63.27 | 60.63 | 60.63 | +17.5 | 9.69 | 10.8 | 85.42 | 0.819 | 93.0 | 7044 |
| A-b (N=0) | 19:57 | 43.79 | 102.32 | 102.32 | 100.27 | 100.13 | +56.3 | 11.66 | 13.0 | 84.61 | 0.825 | 71.6 | 4140 |
| B-b (N=2) | 20:19 | 43.84 | 65.73 | 58.37 | 57.55 | 58.15 | +14.3 | 8.91 | 9.9 | 87.24 | 0.852 | 96.1 | 7970 |

Extra runs, not predeclared (`E:/fah-diag/tools/spikes.sh`: 3 waves of
3 × 8 × 10 MiB, RSS every 10 s for 3 min after each):

- B-a, from the warm 60.63 level: wave 1 end 57.03; wave 2 end 66.52, +120 s
  63.88; wave 3 end 58.98, +120 s 57.68, +180 s 57.70. One 1 MiB request after
  that: 57.01. The −2.6 / −1.3 steps at +120 s are the upstream pool reaper
  closing idle connections (60 s idle timeout, checked at 60–120 s).
- A-a, one wave from 97.43: 104.10, flat through +100 s (series cut to switch
  arms).

Shutdown under N=2 with connections in flight: three 100 MiB fetches at
1 MiB/s each through the probe, `/container/stop` at 20:35:54Z, both domains
logged `drain timed out; aborting the remaining connections` (open=1, open=2)
at +5.0 s, `exited with status 0` at +5.5 s. No kill. One client recorded the
cut (43 MB of 100, curl exit 18).

### Reading

- Memory: B's peak step is a third of A's (+22 vs +58..+61) and its held
  residue a quarter (+14..+17 vs +56..+60), stable from +3 to +15 min; three
  further waves neither ratchet it nor return it while the process is idle.
  The predeclared absolute return-band criterion (floor ±6 MB by +3 min) was
  not met, but repeated A/B evidence showed a large reduction in retained
  memory, no per-burst accumulation, and materially faster reclamation
  behavior. Observed behavior is
  consistent with deferred allocator reclamation on the owning HTTP runtime,
  with reclamation triggered by subsequent allocation activity. A single
  small request does not trigger it (−0.7); a wave does (wave 1 ended 3.6
  below its pre-wave level, wave 3 4.9 below).
- CPU: B −17..−24 % against the same-hour A pair (8.91 / 9.69 s vs
  11.66 / 12.18 s for 900 MiB).
- HTTP: B par8 and p95 inside the A pair's spread (84.6–89.7 MiB/s,
  0.789–0.825 s); single-stream is WAN noise (A-b's 71.6 median).
- DNS: the probe serves none; not measured.
- SHUTDOWN: pass at the library, dev-box and router level.
- 7 DAYS: open; needs a soak.

Supersedes nothing in the buffer A/B; the four 408 arms there and the two A
arms here agree (+56..+69 held). Superseded by any run on the same device with
a warm-floor start or with DNS traffic on the probe, neither of which this
sitting had.

State at the end: probe stopped, envlist at `2`, tar on `kingston/`. Docs
(ADR, CONTEXT.md, CONFIGURATION.md `[runtime]`, API.md) and the review file
are separate go's.
