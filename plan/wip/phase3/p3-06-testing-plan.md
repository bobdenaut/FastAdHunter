# p3-06 — what must be measured, and the scripts that measure it

Second-attempt declaration for the probe-side arms of
`p3-06-phase3-verification-plan.md` Step 4.5 and Runbook 5 / 7. The binding
pre-declaration is the review file §Pre-declaration (P1–P7 plus the MA-6…MA-11
changes). Every place this plan departs from it is listed under §Declaration
deltas and is recorded there — owner approval, `.md` edit — **before** the arm
runs. A figure taken under an unrecorded declaration is diagnostic only.
Results never land in this file: they go to
`docs/code-review/phase3/p3-06-testing-results.md` (§Scripts, Results).

Scope: probe container `fah-probe` on `veth3` (`172.17.0.4`), running the
`phase3-06` tip build (hash recorded per run), driven from bobdenaut
(`192.168.10.10`) over the LAN. No router steering, no device tied to the
probe, production on `veth1` untouched. The router writes this plan needs — a
probe restart after boot keys, a bench container add/remove — are proposed to
the owner with the exact command, never run.

**Hard rule — no test process is named `fastadhunter`.** `/tool/profile
cpu=all` keys on process name and sums two containers running the same binary
(`docs/routeros-traps.md`); the live resolver and a test build must be
separable in every read. Every image uploaded for this task renames its
binary (§Scripts, naming table) — the FAH probe instance included. The binary
never reads its own name (no `current_exe` / `argv[0]` use), so the rename
changes nothing else.

## Measurements

| # | Question | Row / budget (PERFORMANCE.md §Budgets) | Runs on | Needs |
| --- | --- | --- | --- | --- |
| SNI | a blocked domain closes at SNI, before any certificate | gate — the everyone path of the definition of done | probe, LAN client | a domain the probe's lists block |
| P1-LAN | splice throughput on the deployment path — the shipped 16/16 build, then the sweep's pick | ≥ 100 MiB/s steady state (gigabit is 119 MiB/s); single connection must land in P1-control's range | probe; LAN client, LAN origin; 64 MiB, one connection, 5 runs, median + range; 8-connection aggregate arm with `/tool/profile` share (§Choosing `SPLICE_BUF`) | origin under a **publicly resolvable** name; `egress.allow_destinations`; the second LAN endpoint (below) |
| P1-control | what the same LAN path carries without the probe | control for P1-LAN, no budget; without it P1-LAN has no ceiling to read against | LAN client → LAN origin, direct | the second LAN endpoint — **unsolved, owner decision** (§Traps) |
| P1-loopback | CPU per relayed byte vs buffer size — the `SPLICE_BUF` sweep | diagnostic; picks the buffer (§Choosing `SPLICE_BUF`). Reads the buffer sensitivity of the whole in-device loop — client, proxy and origin in one process — and stands for CPU per byte **only if the run is shown CPU-bound** (`/tool/profile` during it); the LAN aggregate arm's `/tool/profile` share is the confirmatory CPU reading | bench container, the `splicebench` example (`crates/fah-http/examples/splicebench.rs`, shipped `TlsServer` / `TlsProxy` relay via `TlsProxy::with_splice_buffers`) on in-device loopback, up / down sizes as runtime parameters, interleaved | **one** image; stdout to the container log, no mount |
| P2 | handshake cost: direct vs spliced vs intercepted | **two rows**: "SNI verdict + splice added latency" = spliced p50 − direct p50 (handshake and first-byte columns both recorded); "intercepted p50 ≤ 2 × spliced p50" on the handshake column | probe, one fixed public origin (name, TLS version, ALPN recorded), 200 rounds, three arms interleaved per round, min / p50 / p99 | CA PEM on the client; **all three arms from one host** — a bridged VM on a **wired** link with **two verified same-family LAN IPv4 source addresses**, one listed and one not (§Traps); never v4 vs v6, never one arm per machine, **no source-IP workaround on bobdenaut**. **Execution location: `p2-handshake.mjs` runs on that VM**; bobdenaut only launches it over ssh and copies the results back — every socket of all three arms binds a VM address, none originates on bobdenaut. Precondition the script proves before any round: both addresses on the VM's interface, each observed by the probe (`/api/v1/clients` after one DNS query bound to each source), exactly one of them in `https.interception.clients` — else `INVALID`. Path proof per row is the served issuer (§Invalidity rules); `/telemetry` + `/certificates` before and after each arm are **supporting evidence only**: `minted_total` delta recorded, **not** required to be +1 — the origin's leaf may already be cached and no API reads per-host state; `blocked = 0` |
| P3 | intercepted h2 relay: throughput **and** per-session RSS under a 64-stream stall | ≥ 50 MiB/s (throughput arm). RSS: the reported quantity is the **session RSS delta** = max `process_rss` over the stall window − `process_rss` before the session, per run; gate statistic = the **max delta over 3 runs**, read against the ≈ 5.5 MiB worst case CONFIGURATION.md `[https] max_connections` states. Raw `process_rss` is never called "per-session RSS". P3 is the **sole authority** for that ceiling — a passing soak says nothing about it (MA-10) | probe, public h2 origin, 8 MiB per stream, 3 runs. **Warm-up first:** one small request to the same origin through the terminate leg, so the leaf mint and the first upstream connection are cached before "before" is read — otherwise they land in the delta. **Stall barrier:** the window opens only after all 64 streams are open **simultaneously**, each has received `:status 200` **and** a first DATA chunk, and the client has then stopped reading; `process_rss` sampled every second for the settle time, max taken; "after" follows the last close. **No-stall control — a separate matched run**, never the same session: its own warm-up, "before", window and "after"; same origin, same 64 streams **fully drained**, same sampling; 3 control runs interleaved with the 3 stall runs (S/C/S/C/S/C). The control delta is the allocator's band (mimalloc purge band alone is ± 6 MB — a single delta cannot resolve a 5.5 MiB question). **Report the raw stall delta and the control delta separately, never combined.** **Attribution:** `RESOLVED` when the smallest stall delta exceeds the largest control delta (disjoint ranges); otherwise **`UNRESOLVED`** — the stall delta sits inside the control / allocator band, is reported as such and is **not** read against 5.5 MiB as a session cost. A raw process RSS delta is never called "session-only memory". Throughput is a separate arm: one unstalled 8 MiB stream, 3 runs, median MiB/s | listed client; the barrier met, else no RSS figure; **nothing else enters the probe during the window** — `listeners.https.connections` delta = warm-up + 1 and the DNS query counters flat across it, read from `/telemetry` before / after |
| P4 | DoT / DoH added latency vs UDP, p50 — in-engine, reused connection | TBD — this arm sets the row | **in-device**: the `encrypted_latency` harness cross-compiled beside the release binary, 3 interleaved rounds × 2 000 per transport, handshakes excluded (MA-8) | a second image (test binary + release binary); owner container add / remove |
| P4-LAN | the same quantity as the LAN sees it | diagnostic only — the LAN hop and one handshake per batch are in the figure, which is why it is not the row | probe, LAN client, blocked domain, 3 × 2 000, one connection per transport per batch | a domain the probe's lists block; the DoT hostname |
| P5 | cold `prewarm` per first-sight host (whole path incl. eviction scan) | < 1 ms | probe, first-sight hosts **over DoT**, handshake only, no query. **Shipped path, verified** (`fah-dns/src/dot.rs`, `fah-certs/src/leaf.rs`): with a CA in the probe's store and an SNI in the hello, the listener pre-warms on `spawn_blocking` **before** the handshake — first sight mints (`minted_total` +1), a repeat finds the leaf fresh (no mint) — and the handshake serves the minted leaf (issuer = FastAdHunter CA). No CA in the store ⇒ nothing is minted, the fallback API certificate is served, `unwarmed_misses` counts. **State the script configures, not assumes:** CA generated on the probe; client trust irrelevant; host count **below** `LEAF_CACHE_CAPACITY` (512) minus what is already cached — 256 — so the repeat pass evicts nothing. Per host one first-sight and one repeat handshake; figure = median(first-sight) − median(repeat), an **incremental p50 estimate** of mint + insert, not a mint duration: a difference of medians, and the `spawn_blocking` hop is paid by both arms and cancels. On-device `certs_mint` (criterion) is the diagnostic beside it | host list (syntactically valid names, need not resolve); CA on the probe; `minted_total`, `unwarmed_misses`, `evictions` before / after; served issuer per row |
| P6 | CA generate / API-pair import | < 100 ms / < 50 ms on **`time_starttransfer − time_appconnect`** — **client-observed request-processing time excluding the TLS handshake** (request transit, server work, first-byte transit; supersedes MA-9's "server side" label); `time_total` beside it as the handshake-inclusive diagnostic | probe API, 5 each, min / median | a real pair on disk for the import arm; archive headroom (§Environment) |
| P8-probe | CPU the probe burns under a stage's load | diagnostic, **undeclared until recorded**. P8 proper is the soak deploy against the 0.3.1 `dns+http` container (MA-11), not the probe | router, read-only `/tool/profile cpu=all` | the naming rule (§Scope); an idle baseline before every read; the load sized to outlast the profile window |
| P9-probe | boot-to-serving with the phase-3 listeners | diagnostic. P9 proper is the soak deploy's container log against the 0.3.1 container (MA-6); the probe's list set is its own, not production's — record its parsed count beside the reading | `/log print where topics~"container"` | — |
| Runbook 7 | certificate-store checks | pass / fail, all mandatory | split below | — |

**Runbook 7 split.** Script side: no key material over the API — `ca/export`
in both formats and `/config` searched for the CA key's base64 payload, plus
the security suite's traversal list with `curl -sk` against `:8443` (X5).
Owner side, propose only: import-then-restart (`/container/stop` + `start` on
`fah-probe` is a router write) with `openssl s_client` and `source ==
"imported"` after; `0600` on every private key via `sftp` `ls -l` after first
boot, import and `ca/generate` ×2; the archive cap — the **ninth** generate
answers `409` `archive_full` with the live fingerprint unchanged.

**Device-only, no script can stand in:** Android CA trust and Private DNS
(Runbook 2, 3), the pinned-app check (Runbook 4), the ECH browser retry.

**Out of this plan, still required before the phase row flips:** Runbook 1
(dst-nat 443 v4 + the v6 decision), P7 — the 24 h soak on the production
container (RAM ≤ 128 MB steady; the minted-leaf hit-rate row ≥ 90 % from the
`leaf_cache` counters, the only evidence since D12 was struck; watch items
a–f), and P8 / P9 proper on the soak deploy.

## Choosing `SPLICE_BUF` — 16 KiB or 64 KiB

"16 or 64" is the wrong question. Both device-loopback figures (338.7 vs
456.3 MiB/s) sit 3–4 × above the 119 MiB/s NIC, so on the wire any size
reaches line rate on one connection. What the buffer buys is **CPU per
relayed byte** — fewer read / write syscalls per TLS record — and what it
costs is **memory per session**. Two points cannot find the knee, and the two
directions are not symmetric: the splice is
`copy_bidirectional_with_sizes(client, upstream, SPLICE_BUF, SPLICE_BUF)`
(`fah-http/src/https.rs`), downloads fill upstream → client, requests barely
fill client → upstream. The sizes are runtime arguments to tokio already, so
the bench can sweep them; production keeps a const (or two).

Prediction to falsify: 32 KiB down captures most of the 16 → 64 gain (TLS
records are ≤ 16 KiB; a LAN receive queue holds 2–4 of them at line rate),
128 ≈ 64, and `up` does not matter for a download.

Procedure:

1. **Sweep on device loopback, one image** (P1-loopback). The bench takes
   `up` / `down` as parameters. Matrix: `up = 16 KiB`, `down ∈ {16, 32, 64,
   128} KiB`, plus `64 / 64` as the symmetry control; steady state, 64 MiB,
   5 reps each, arms interleaved A/B/A/B within one run. No container swaps,
   no rebuild per point. `/tool/profile cpu=all` is read during the run and
   the `fah-splicebench` share recorded: the arm counts as CPU-bound, and its
   MiB/s as a CPU-per-byte proxy, only if that share shows it; otherwise the
   sweep reports "throughput of the loop" and the CPU axis comes from step 4
   alone.
2. **Pick the knee — among candidates inside the step 3 budget only.** Per
   candidate the statistic is the **median of its 5 repetitions**; **best** is
   the highest candidate median; the pick is the **smallest in-budget
   candidate whose median ≥ 0.9 × best**. Min–max per candidate is reported
   beside the median: where the pick's range overlaps best's, the difference
   is not established, which only strengthens the smaller pick. `up` stays 16
   unless `64 / 64`'s median beats `16 / 64`'s by more than 10 %. Candidates
   outside the budget are still measured (the evidence for step 3's separate
   decision), never picked. Five repetitions per candidate, as declared —
   owner decision 2026-09-03, not to be expanded.
3. **Memory budget, `max_connections = 1024` fixed.** `max_connections` is a
   product capacity property; it does not move during the sweep or the
   selection, and it is never lowered to make room for a buffer. The budget
   is `(up + down) × 1024`, worst case, declared by the owner **before** the
   sweep runs (the pre-declaration rule); until another figure is declared it
   is today's 32 MiB (`16 + 16`). Worst case per candidate at 1024:

   | `up + down` (KiB) | worst case |
   | --- | --- |
   | 16 + 16 | 32 MiB |
   | 16 + 32 | 48 MiB |
   | 16 + 64 | 80 MiB |
   | 64 + 64 | 128 MiB |
   | 16 + 128 | 144 MiB |

   A candidate above the budget is **rejected**, whatever its CPU gain. At
   the 32 MiB budget only `16 / 16` fits — the sweep then yields evidence for
   a **separate owner decision** (raise the buffer budget, or change
   `max_connections` with its own justification), recorded as a decision
   item in the review file, never taken inside this procedure.
4. **Confirm on the LAN with the real binary** (P1-LAN): the shipped
   `16 / 16` first, then the pick if one passed step 3 — one extra image at
   most, container replace by the owner, P1-control re-read before and after
   so drift shows.
   Single connection must land in P1-control's range for both. The
   8-connection aggregate arm reads `/tool/profile` share for each build at
   the same aggregate rate (P8-probe, idle baseline first, naming rule
   §Scope): that CPU delta is the number that justifies a change, because
   wire speed will tie. A build that misses P1-control's range is a
   **finding** (veth hop, conntrack, splice loop), attributed before any
   tuning.
5. The const changes only through its own gates plus a dev-box D6 / D7 rerun;
   each build gets one row in the review §Measurements with all three axes.

Rejected: an adaptive buffer (start at 16 KiB, grow on a full read). Hot-path
state and a branch per read for a gain the sweep prices first — principle 16.

## Scripts

**One script per measurement**, all under `docs/code-review/phase3/p3-06-probe/`,
Node, no dependencies. Shared code lives in `lib.mjs` only: config and flag
parsing (`--probe`, `--key`, `--out` on every script), the bearer API call,
percentiles, the `valid` / `INVALID` / `degraded` reporting and the results
directory. Each script owns its preconditions and its §Invalidity rule; none
knows about another, so a change to one measurement touches one file.

| Script | Covers | Key inputs | Emits |
| --- | --- | --- | --- |
| `lib.mjs` | shared — nothing measured here | — | — |
| `p0-sni.mjs` | SNI gate | blocked domain | `sni.json` |
| `p1-lan.mjs` | P1-LAN; `--direct` runs P1-control against the same origin; `--connections 8` the aggregate arm (§Choosing `SPLICE_BUF` step 4) | origin name, runs, bytes, connections | `p1.json` |
| `p1-origin.mjs` | the P1 origin as its own process, on the second LAN endpoint | payload size, cert paths | serves N MiB over TLS on `:443` |
| `Dockerfile.splicebench` | P1-loopback, the whole sweep — the `splicebench` example, matrix / repetitions / interleaving / pick rule as declared in §Choosing `SPLICE_BUF` | `up` / `down` sizes as runtime parameters, one image; `--budget-mib` (default 32) | per-candidate rows and the pick line in the container log |
| `p2-handshake.mjs` | P2, three arms interleaved — **runs on the wired bridged VM**, launched from bobdenaut over ssh | public origin, CA PEM, rounds, listed / unlisted source address | `p2.json`, copied back to the results directory |
| `p3-h2stall.mjs` | P3, throughput and RSS | CA PEM, h2 origin + path, streams | `p3.json` |
| `Dockerfile.p4` | P4, in-device | `encrypted_latency` test binary + release `fastadhunter`, `aarch64-unknown-linux-musl` | harness output in the container log |
| `p4-lan.mjs` | P4-LAN | blocked domain, DoT hostname, queries, rounds | `p4-lan.json` |
| `p5-mint.mjs` | P5, first-sight and repeat arms over DoT | host list, seed | `p5.json` |
| `p6-certs-time.mjs` | P6, generate and import arms, both columns | real pair path | `p6.json` |
| `p7-store.mjs` | Runbook 7 script side | traversal list | `certs.json` |
| `Dockerfile.fahprobe` | the probe FAH instance every LAN arm drives; the `SPLICE_BUF` pick is a second build of it | release `fastadhunter`, renamed | image whose process is `fah-probe` |

Every script writes `<name>.json` with `valid`, appends `raw.jsonl` and
`run.log`, and snapshots the probe's `/config` once per run directory.

**Binary names on the router** (the hard rule in §Scope). Linux `comm`
truncates at 15 characters, so every name is ≤ 15; the rename is a `COPY …
/<name>` plus `ENTRYPOINT` in the Dockerfile, nothing in the code.

| Image | Process name | Never |
| --- | --- | --- |
| `Dockerfile.fahprobe` (probe instance, both buffer builds) | `fah-probe` | `fastadhunter` |
| `Dockerfile.splicebench` | `fah-splicebench` | the criterion hash name |
| `Dockerfile.p4` — harness plus the binary it spawns | `fah-p4` spawning `/fah-probe` (`FAH_E2E_BINARY=/fah-probe`) | `fastadhunter` |

Verification before the first `/tool/profile` read of a session: `/container/
print detail` shows the test container's `cmd` / entrypoint under its own
name, and `/tool/profile cpu=all` lists `fastadhunter` **and** the test name
as separate rows while both run. The live resolver's row is the idle baseline
for every attribution.

Nothing listed above exists in the repo yet; the Dockerfiles the first
attempt used must be committed beside the scripts or their figures cannot be
reproduced. Every in-device image follows the `Dockerfile.probe` pattern —
single layer, legacy docker-archive, never OCI layout
(`docs/routeros-traps.md`).

Results: raw output stays in the `results-<ts>/` directory under
`p3-06-probe/`, committed with the run. **Figures are recorded in
`docs/code-review/phase3/p3-06-testing-results.md`** (root CLAUDE.md rule 19:
measurements live under `docs/code-review/`). That file has **one section per
measurement ID, two tables each**:

- **Runs** — one row per run: date, tip hash, device / workload / corpus,
  idle check, validity / attribution, the declaration delta number if the run
  followed one, raw directory.
- **Figures** — shaped for the measurement (arms × min / p50 / p99,
  candidates × median / min / max, runs × stall delta / control delta), the
  **gate line as its last row**. Diagnostics go in this table, never in
  prose.

An `INVALID` or `degraded` run gets a Runs row and no Figures row. The review
file §Measurements links to the section. **This plan carries no results and
is not edited after a run** — §State below is the last result-shaped content
it will hold. A figure that is in neither place is not a result.

No script may print a figure it did not verify it could produce — see
§Invalidity rules.

## Invalidity rules

Every script checks its own preconditions and prints `INVALID` with the reason
instead of a number when they fail. Each rule exists because its absence
produced a wrong figure.

| Measurement | Its script must verify before reporting |
| --- | --- |
| P1 | a spliced sample completed; the origin arm is labelled `loopback_origin`, never "direct" or "control", and is excluded from the gate line — the control is P1-control |
| P1-loopback | the `/tool/profile` share during the run is recorded; without it, or with a share that does not show the loop CPU-bound, the figure is labelled "throughput of the loop", never CPU per byte |
| P2 | the identity precondition (§Measurements): two same-family IPv4 addresses on the VM, both observed by the probe, exactly one listed — else `INVALID` before the first round. Then **both**, per row: every spliced row's served issuer ≠ FastAdHunter CA **and** every intercepted row's served issuer = FastAdHunter CA. Either condition alone proves nothing about the other arm. Certificate provenance is authoritative; counters are supporting — `minted_total` is recorded, never asserted. `requests > connections` is **not** a rule — with one request per fresh connection it is false by construction on both legs |
| P3 | the warm-up ran before "before"; the stall barrier (§Measurements) was met — all 64 open at once, each with `:status 200` and a first DATA chunk — before the window opened; the window was exclusive (`listeners.https.connections` delta = warm-up + 1, DNS counters flat); the 3 matched no-stall control runs ran, interleaved with the stall runs, each with its own warm-up / before / window / after. Any of these missing ⇒ no RSS figure. Stall and control deltas printed separately; the attribution label (`RESOLVED` / `UNRESOLVED`) printed with them |
| P4 / P4-LAN | the figure is labelled by which of the two it is; replies matched by 16-bit transaction ID; DoT reassembled by its 2-byte length prefix; unanswered and unmatched counts reported |
| P5 | `minted_total` moved by exactly the host count; `evictions` delta 0 across the repeat pass; every row's served issuer = FastAdHunter CA (the fallback certificate on any row ⇒ no CA in the store ⇒ `INVALID`); the repeat arm ran; `unwarmed_misses` delta reported |
| P6 | every generate and import returned `200`; the archive count read first |
| all | origin failure rate within budget, else the result is `degraded`, not a gate; tip hash and idle baseline recorded |

## Gate statistic vs diagnostics

One statistic per measurement decides; everything else is reported beside it
and decides nothing. A script prints the gate line from the gate statistic
only.

| Measurement | Gate statistic (authoritative) | Diagnostics (reported, never gate) |
| --- | --- | --- |
| SNI | every attempt closed before any certificate — boolean | close latency |
| P1-LAN | median of 5 runs ≥ 100 MiB/s **and** inside P1-control's min–max | min / max, aggregate-arm MiB/s, `/tool/profile` share |
| P1-control | none — it is the band | min / p50 / max |
| P1-loopback | none — selection statistic is the per-candidate median (§Choosing step 2), from the `splicebench` example's per-candidate rows | min / max, `/tool/profile` share |
| P2 | handshake column: intercepted **p50** ≤ 2 × spliced **p50**; row value: spliced p50 − direct p50 (handshake and first-byte); p50 = median over 200 rounds | min, p99, per-round raw, first-byte ratio, counters |
| P3 throughput | median of 3 runs ≥ 50 MiB/s | per-run MiB/s |
| P3 RSS | **max over 3 runs** of the stall delta, read against ≈ 5.5 MiB **only when attribution is `RESOLVED`** (smallest stall delta > largest control delta); above it is a finding, not a fail. `UNRESOLVED` ⇒ both deltas reported, no reading against 5.5 MiB | per-run stall delta, per-run control delta, the per-second series |
| P4 | per transport p50 − UDP p50 (sets the row) | p99, per-round, handshake time |
| P4-LAN | none — diagnostic | p50 / p99 from `p4-lan.mjs` (send → matched reply, one connection per transport per batch), batch wall-clock mean |
| P5 | median(first-sight) − median(repeat) < 1 ms | per-host raw, on-device `certs_mint`, `unwarmed_misses` / `evictions` deltas |
| P6 | median of 5 on `time_starttransfer − time_appconnect`: generate < 100 ms, import < 50 ms | min, `time_total` |
| P8-probe, P9-probe | none — diagnostic | as declared |
| Runbook 7 | each check pass / fail | — |

## Environment the scripts assume

Probe config, set once via `POST /api/v1/config`. All three keys are **boot**
keys (`restart_required: true`): set all three, then one restart —
`/container/stop` + `start` on `fah-probe`, a router write the owner runs.

| Key | Value | Why |
| --- | --- | --- |
| `engine.mode` | `dns+http+https` | default is `dns`; no HTTPS or DoT listener otherwise |
| `egress.allow_destinations` | the LAN origin's address | empty refuses every private destination, judged on the **resolved** address |
| `https.interception.clients` | the listed client only | anything not listed is spliced |

Plus: a CA generated on the probe and exported to the driving host — and
**re-exported after every P6 run**, since each generate replaces it; a publicly
resolvable name for the LAN origin (`<dashed-ip>.nip.io` works — see §Traps);
`ca-archive/` **empty** before P6, so that P6's five generates plus four more
reach the ninth for the archive-cap check (`fah_certs::MAX_ARCHIVES` = 8;
`api-archive/` likewise before the import arm).

## Running from the laptop — the agent drives

Every script except `p2-handshake.mjs` runs on bobdenaut (`192.168.10.10`,
Windows 11, Node) from the repo checkout, invoked by the agent, with `--out
docs/code-review/phase3/p3-06-probe/results-<ts>`. **`p2-handshake.mjs` runs
on the wired bridged VM** (Node, the CA PEM and the API key file copied
there): bobdenaut launches it over ssh, and its results directory is copied
back into the run's results directory on bobdenaut afterwards (`scp` — rule
18, ask first). The VM's virtual NIC hop is in all three arms alike, so it
cancels in the ratio and in spliced − direct; the absolute direct figure is
diagnostic. What the agent may do on its own, what it proposes, and what it
never does:

| Action | Who |
| --- | --- |
| run a script, read probe API / router read-only prints, write results under `p3-06-probe/` | agent |
| launch `p2-handshake.mjs` on the VM over ssh; copy its results back | agent (the copy after a yes — `scp`) |
| add / remove a local firewall rule (elevated PowerShell) | agent **after an explicit yes** per rule, or owner |
| restart the probe, add / remove a bench container, `scp` an image | owner — agent proposes the exact command (root CLAUDE.md: router off limits; rule 18 for `scp`) |
| commit results | owner's yes per changeset |

**Before every script**, recorded in `run.log`:

1. `GET /health` on the probe answers `200`; `engine.mode` from `/config`
   contains `https` for the HTTPS and DoT arms.
2. `Get-NetConnectionProfile` — the LAN adapter's `NetworkCategory` is
   `Private`. On `Public`, Windows drops unsolicited inbound silently and the
   first-run `node.exe` "Windows Security Alert" is suppressed, so the origin
   arm fails with no local error.
3. `tasklist` shows no browser or video player (measurement-traps: a
   tip-only figure on a non-idle box has no control to cancel the load).
4. Before the origin starts: `Get-NetTCPConnection -LocalPort 443 -State
   Listen` is empty — an existing listener on `:443` makes the origin arm
   `INVALID` with `EADDRINUSE`, not a wrong number.
5. Before P2, on the VM: `ip -4 addr` shows both LAN addresses on the
   bridged interface; nothing else runs on the VM.

**Local firewall.** Inbound only where the laptop *accepts* a connection; every
other arm is outbound and needs no rule (Windows allows outbound by default;
UDP 53 replies are stateful).

| Measurement | Laptop accepts from | Rule |
| --- | --- | --- |
| P1-LAN, origin on the laptop | the probe, `172.17.0.4`, TCP `443` | inbound allow, scoped to that remote address |
| P1-control, origin on the laptop | the client host, TCP `443` | inbound allow, scoped to that host |
| P1 with the origin on the second endpoint | nothing on the laptop | the same two rules on **that** host's firewall instead |
| SNI, P2, P3, P4-LAN, P5, P6, Runbook 7 | nothing | none |

Add before the run, remove after, never without `-RemoteAddress` — an
unscoped `:443` allow on a laptop outlives the session:

```powershell
New-NetFirewallRule -DisplayName "fah-p1-origin" -Direction Inbound -Protocol TCP -LocalPort 443 -RemoteAddress 172.17.0.4 -Action Allow -Profile Any
Get-NetFirewallRule -DisplayName "fah-p1-origin" | Get-NetFirewallAddressFilter
Remove-NetFirewallRule -DisplayName "fah-p1-origin"
```

Do not answer the "Windows Security Alert" dialog with *Allow*: it writes a
permanent, unscoped `node.exe` rule for every port. The rule above pre-empts
the dialog. Never `New-NetIPAddress` / `netsh … add address` on the laptop
(§Traps).

## Traps

Each cost a run or a retraction in the first attempt.

| Trap | Effect | Avoidance |
| --- | --- | --- |
| Probe and production both run a binary named `fastadhunter` | `/tool/profile` sums them; a 84.5 % core reading was attributed to the probe and was not | hard rule in §Scope: every uploaded image renames its binary (§Scripts naming table); idle baseline before every attribution |
| A listed client has no spliced path | "spliced" arm silently measures the terminate leg | separate the arms by client address and check the issuer |
| Spliced and intercepted arms from two different machines | the ratio compares two TLS stacks, not two legs | all three arms from one host carrying two verified same-family IPv4 addresses: a bridged VM with static addresses (no DHCP trap). Never v4 vs v6: the probe listens on v4 only and interception matches the client address, so the "arm" would change the path |
| A bridged VM on Wi-Fi | most access points drop frames carrying a second MAC; the VM "has" its addresses and the probe never sees them | wired link for the VM's bridge; the P2 identity precondition catches it, the wire fixes it |
| `New-NetIPAddress` / `netsh … add address` for a second source IP | drops the adapter's DHCP lease and default route | do not on bobdenaut; a second address costs nothing on a VM |
| No second LAN endpoint | P1-control impossible; the P1 origin sits on the driving host and `loopback_origin` never crosses the NIC | **owner decision**: any second LAN machine, or a VM with a bridged NIC on bobdenaut (own DHCP lease, no adapter edit). One endpoint serves P1-control, the P1 origin and, if it carries two addresses, P2's arm split |
| The proxy resolves upstream names through the container's stub resolver, not its own engine | a `$dnsrewrite` rule never applies to the splice path; `.test` names give `resolve_failures` | use a publicly resolvable name |
| `dnsrewrite` to an IPv4 answers AAAA with `::` | upstream connect fails instantly | v6 rewrite as well, or a name that resolves for real |
| `/tool/fetch` as a throughput control | 20 s to `disk1`, 1 s to `kingston`; measures the write target, 1 s resolution | not a control |
| A figure printed before its stage checked preconditions | two P3 "results" (4.0 MiB, then 0.0 MiB for 64 "stalled" streams) were quoted before any stream was shown to carry a byte | §Invalidity rules; a stage prints `INVALID`, never a number it cannot stand behind |
| Results left in a `results-*` directory on the laptop | the first attempt's figures exist nowhere the review can cite | commit the directory with the run; §Measurements cites it |
| Criterion needs a writable cwd | bench aborts before printing | `workdir=/data` with a mount |
| Stage runs longer than the profile window | `/tool/profile` samples an idle box | size the load to outlast the profile |

## Declaration deltas

Recorded in the review file §Pre-declaration "Declaration changes" before the
arm runs; the original block stays unedited.

1. **P1** — declared: 16 vs 64 KiB as two real-binary builds on the LAN.
   Now: a four-point asymmetric sweep on device loopback picks the size
   (§Choosing `SPLICE_BUF`), and the LAN carries the shipped build plus the
   pick only, with an 8-connection aggregate arm for the CPU axis.
2. **P1** — the origin sits on a LAN endpoint the client does not share;
   `loopback_origin` is not a control; P1-control added.
3. **P5** — DoT path with the first-sight − repeat difference method (declared:
   `minted_total` delta / wall time, which carries the TLS RTT per mint).
4. **P8-probe, P9-probe** — added as diagnostics beside P8 / P9 proper.
5. **P2 proof** — declared (Runbook 5): telemetry proofs including
   `requests > connections`. Now: the served issuer per row is authoritative
   for both arms; counters are supporting; `requests > connections` dropped
   as false by construction for one request per fresh connection.
6. **P5 state and count** — declared: 512 hosts, `minted_total` delta / wall
   time. Now: CA in the probe's store as a configured precondition, 256 hosts
   (below `LEAF_CACHE_CAPACITY` so the repeat pass evicts nothing), the
   incremental p50 estimate (first-sight − repeat medians), issuer per row.
7. **P6 label** — MA-9's "server side" becomes "client-observed
   request-processing time excluding the TLS handshake"; same quantity,
   honest name.
8. **P3 RSS quantity** — declared: "RSS from `/api/v1/debug/memory`
   before / during / after". Now: the stall delta (max over the stall
   window − before), max over 3 runs as the statistic, a warm-up before
   "before", 3 matched no-stall control runs interleaved with the stall runs
   for the allocator band, an exclusive window proven by counters, and an
   attribution label — `RESOLVED` only when the stall and control ranges are
   disjoint, otherwise `UNRESOLVED` and no reading against 5.5 MiB.
9. **P2 identities and location** — declared: "two client addresses, or two
   passes with a restart between", run from the LAN client. Now: the wired
   bridged VM with two verified addresses only, `p2-handshake.mjs` executing
   on the VM (bobdenaut launches and collects); the two-pass alternative is
   withdrawn (it loses interleaving and a restart clears the leaf cache
   between arms).
10. **P4 image uid (2026-09-03)** — declared: the probe-image convention,
    `USER 0:0`. Now: `Dockerfile.p4` runs as `65532:65532` with `/tmp` owned
    by that uid (`TMPDIR=/tmp`). Reason: the harness creates its config /
    data volumes with `tempfile` (0700, owner = the harness uid) and the
    spawned binary drops to 65532 before its first-boot writes, so a root
    harness produces a predictable `EACCES` and no figure. Started as 65532
    the binary performs no drop and binds ephemeral loopback ports, which
    need no privilege; the measured quantity (per-query latency, in-engine)
    never includes the drop. Recorded before the first P4 run.
11. **Origin-failure-rate budget, P1-LAN, P2 and the P3 throughput arm
    (2026-09-03)** — declared: §Invalidity rules row "all", "origin failure
    rate within budget, else `degraded`", no number. Now: the budget is
    **2 %** of a stage's samples (`p1-lan.mjs` runs, `p2-handshake.mjs`
    rows per arm, `p3-h2stall.mjs` throughput-arm streams; `--max-fail-pct`
    default 2). Zero completed samples ⇒ `INVALID` (the P1 row's "a spliced
    sample completed"); failures at or under 2 % ⇒ `valid`, figures from
    the completed samples; above ⇒ `degraded`, not a gate. With 5 runs
    (P1-LAN) or 3 (P3 throughput) one incomplete run is already above the
    budget; P2's 200 rows per arm tolerate 4. P2's issuer rules stay
    `INVALID` conditions, outside this budget. Gate statistics, quantities
    and counts unchanged. Recorded before the first P1-LAN, P2 or P3 run.
12. **P4 DoH protocol (2026-09-03)** — declared: the `encrypted_latency`
    harness "DoH keep-alive", the same quantity as D13, whose client
    negotiated HTTP/1.1 because the workspace `reqwest` carried no `http2`
    feature (p3-05 review). Now: the DoH arm runs over **h2** — `reqwest`
    dev-dependency feature `http2`, every response asserted `HTTP/2.0` —
    the protocol P4-LAN (`kdig +https`, `p4-lan.mjs` over `node:http2`) and
    real DoH clients speak, so the two DoH columns describe one protocol
    (smoke findings F17 / F26). D13's h1 seed is not the comparator for the
    DoH column; UDP and DoT columns, gate statistic and counts unchanged.
    Recorded before the first P4 run.
13. **SNI invalidity rules and gate term (2026-09-03)** — declared: gate
    "every attempt closed before any certificate — boolean", no §Invalidity
    row for SNI. Now, as `p0-sni.mjs` implements it: (a) the allowed name
    must reach ServerHello through the probe, else `INVALID` — a listener
    that closes everything proves nothing about blocked rows; (b)
    `listeners.https.blocked` must move by at least the blocked-attempt
    count (attempts × 2 when `https.sni.no_sni = "block"`, × 1 when
    `"pass"`), else `INVALID` — a close caused by a resolve failure is not
    an SNI verdict (smoke F18); (c) the gate boolean covers the blocked and
    the no-SNI attempts: every one `closed_silent` or `alert`, none reaching
    ServerHello. Under `no_sni = "pass"` the no-SNI close is the listener
    having nothing to splice to, not a rule verdict; it stays in the gate
    because a certificate served on a hello without SNI is a finding under
    either setting. Close latency stays diagnostic. Recorded before the
    first SNI run.
14. **P3 origin (2026-09-03)** — declared: "public h2 origin". Now: an h2
    origin on the second LAN endpoint under a **public name with a publicly
    trusted certificate** (Let's Encrypt DNS-01 — the release probe verifies
    upstreams against `webpki-roots` only, so a local CA is refused with
    `UnknownIssuer`), served by `smoke/h2-origin.mjs --address 0.0.0.0
    --bytes 8` (`/8mib` = 8 388 608 bytes with `content-length`, `/` for the
    warm-up, HEAD answered headers-only), the endpoint's address in
    `egress.allow_destinations`. Reasons: no third-party 64 × 8 MiB burst,
    and LAN bandwidth — against a WAN origin the ≥ 50 MiB/s throughput gate
    reads the internet link, not the relay. Preflight from bobdenaut before
    the arm: `smoke/h2-preflight.mjs --host <name> --path /8mib --bytes 8`
    — one HEAD and one GET over h2, `PASS` only when ALPN is h2, both answer
    200 with `content-length: 8388608` and the GET body is 8 388 608 bytes
    (no curl on bobdenaut speaks h2, so `curl -I --http2` is not the tool) —
    output saved to `results-<ts>/p3-origin-preflight.log`. Barrier,
    exclusive-window proof, gate statistics and counts unchanged.
    Recorded before the first P3 run.
15. **P1 firewall scoping (2026-09-05)** — declared: §Running from the laptop,
    the Local firewall table, an inbound allow on TCP 443 for the P1 origin
    scoped to the probe, `-RemoteAddress 172.17.0.4`. Now: scoped to
    **`192.168.10.1`**. Reason: `/ip/firewall/nat` srcnat rule 1 is
    `action=masquerade src-address=172.17.0.0/24` with **no** `out-interface`
    restriction, so traffic from the probe to a LAN host is masqueraded and
    arrives from the router's LAN address, not from `172.17.0.4`. The declared
    rule matches nothing and Windows drops the connection silently — the arm
    fails with no local error, the same shape §Running item 2 warns about for a
    `Public` adapter. Verified on the device 2026-09-04 (`/ip/firewall/nat
    print`). Unaffected: the reverse direction (LAN → container is not matched
    by rule 1, so the probe still sees real client addresses and P2's identity
    precondition stands), and the P1-control rule, which is LAN → LAN and never
    crosses the container subnet. Gate statistics, quantities and counts
    unchanged. Recorded before the first P1-LAN or P1-control run.

**Frozen at approval (2026-09-03).** The scripts implement this plan as
written. A methodology change discovered while writing them is a new numbered
delta here and in the review file, recorded **before** the arm runs — the
script follows the plan, never the reverse.

## State at the end of the first attempt

Nothing below is in the review file; the raw output is not in the repo.

| Item | State |
| --- | --- |
| `SPLICE_BUF` | **provisional**: 16 KiB (P1-loopback: 16 KiB 338.7 MiB/s vs 64 KiB 456.3 MiB/s steady state; loopback only). Decided by §Choosing `SPLICE_BUF`: loopback sweep picks, LAN confirms |
| P2 | PASS, 1.13× against ≤ 2× — counts once §Measurements carries min / p50 / p99 for both columns, the spliced − direct row, the telemetry proofs, the origin's name / TLS version / ALPN and the tip hash |
| P1-LAN | preliminary; no reproducible ceiling, gate undecided, control outstanding |
| P3 | **BLOCKED** — one h2 stream of N answers through the terminate leg; HTTP/1.1 to the same URL through the same probe returns the full 8 MiB. Attribution owed before a rerun: reproduce on the dev box — 64 streams × 8 MiB on one h2 session through the p3-04 interception harness's h2 origin (its carry-over test covers 8 parallel small responses only). Reproduces ⇒ a p3-04 finding in the review file; does not ⇒ origin- or harness-side, rerun against another h2 origin |
| P4, P4-LAN, P5, P6, Runbook 7 | not run |
