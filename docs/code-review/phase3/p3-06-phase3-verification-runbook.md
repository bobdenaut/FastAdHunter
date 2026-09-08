# p3-06 — Step 4 on-device runbook

Owner-executed procedure for
[p3-06-phase3-verification-plan.md](../../../plan/wip/phase3/p3-06-phase3-verification-plan.md)
Step 4. Prepared 2026-09-08 on the dev box; **no command here has been run**.

## Summary

| | |
| --- | --- |
| Scope | the router writes and probe API calls Step 4 already declares — nothing new is decided here |
| Who runs it | the owner. The agent runs none of it ([CLAUDE.md](../../../CLAUDE.md) §The router is off limits) |
| Entries | `R0`–`R11`, each with command, resulting state, effect timing, restart requirement and rollback |
| Hard boundary | **RouterOS edits only for the four `veth3` containers.** Firewall, NAT, `/system`, production on `veth1` and `/kingston/fastadhunter/` are all out of scope |
| Owner only | **`R7`** (NAT rule) and **`R11`** (production soak deploy) — outside the boundary above, documented but never agent-run |
| Read first | `R0`, **already run 2026-09-08** — output in [p3-06-probe/device-state/](p3-06-probe/device-state/README.md). No probe container, no campaign tars, `veth3` free |
| First write | **`R4`** — upload the four tars. `R1` has nothing to fix and `R5` is a plain `add`, because no probe exists yet |
| Order that matters | `R4` → `R4b` → `R3` → `R5`. Directories and lists must exist before the container that names them |
| Blocked | `R11` until the 0.3.3 soak ends 2026-09-14. P3 has no entry at all — it cannot run until a publicly trusted h2 origin exists |
| Evidence behind it | [p3-06-phase3-verification-review.md](p3-06-phase3-verification-review.md) §Layer 3 and §Smoke session |

## The rule that bounds this document

**No RouterOS edit is permitted unless it concerns one of the four campaign
containers bound to `veth3`** — `fah-probe`, `fah-splicebench`, `fah-p4`,
`fah-certs` — or the mount lists, env list and image tars those four use.

Everything else on the router is out of scope: the firewall, the NAT chain,
`/ip` anything, `/system` anything, the production container on `veth1`,
`fah-env`, `fah-config`, `fah-data`, and `/kingston/fastadhunter/`.

Two entries in this runbook fall outside that boundary and are marked
**OWNER ONLY** — `R7` (a NAT rule) and `R11` (the production soak deploy).
They stay documented because Step 4 declares them, but no agent proposes,
prepares or runs them, and each needs a separate explicit decision from the
owner.

## Scope and conventions

Every action below is a router write or a probe API call **the owner runs**.
The agent runs none of them. Nothing here is a new decision: each entry maps to
an action already declared in
[p3-06-phase3-verification-plan.md](../../../plan/wip/phase3/p3-06-phase3-verification-plan.md)
Step 4 or in
[p3-06-testing-plan.md](../../../plan/wip/phase3/p3-06-testing-plan.md), and the
"why" stays in those documents.

**Four shell variables**, set once on bobdenaut before any `curl` below. They
are the only values this runbook cannot fill in for itself:

```sh
FAHKEY=$(cat /path/to/fah-probe-apikey)   # SFTP-copied from /kingston/fah-probe/config/apikey,
                                          # or read from the probe's first-boot log line
                                          # "generated API key — store it now"
MAC=192.168.10.__                         # the Mac's wired LAN address (egress allow-list, P1/P3 origin)
CLIENT=192.168.10.__                      # the intercepted test device (must hold a static lease)
DNSNAME=dns.____                          # the Private DNS hostname for R9
```

Everything else is concrete. Production owns `/kingston/fastadhunter/` and this
campaign never writes there; each of its four containers gets its own top-level
folder instead:

```text
/kingston/
  fastadhunter/   root/ config/ data/   PRODUCTION — never touched by this campaign
  fah-probe/      root/ config/ data/   the probe FAH instance, veth3
  fah-splicebench/ root/                one-shot bench
  fah-p4/         root/                 one-shot harness
  fah-certs/      root/                 one-shot bench
```

**Nothing in this campaign writes anywhere under `/kingston/fastadhunter/`.**
Each container owns one top-level folder named after it. A probe mounted on
`fah-config` would write its own CA, API key and TOML over the live resolver's.

| | production (live 2026-09-08) | campaign containers |
| --- | --- | --- |
| comment | `fastadhunter` | `fah-probe`, `fah-splicebench`, `fah-p4`, `fah-certs` |
| `interface` | `veth1` | `veth3` — **one at a time**, see below |
| `root-dir` | `/kingston/fastadhunter/root` | `/kingston/fah-probe/root`, `/kingston/fah-splicebench/root`, `/kingston/fah-p4/root`, `/kingston/fah-certs/root` |
| `mountlists` | `fah-config,fah-data` | `fahprobe-config,fahprobe-data` — **the FAH probe only** |
| `envlists` | `fah-env` | `fahprobe-env` — the FAH probe only |
| `workdir` | `/home/nonroot` | `/home/nonroot` |
| `cpu-list` | `cpu0,cpu1,cpu2,cpu3` | `""` — empty is **no restriction**, not "no CPUs". A pinned probe stops standing in for the thing it measures |
| `start-on-boot` | `yes` | `no` |

**One veth carries one container at a time** (confirmed on the device
2026-09-04), so the three one-shot containers cannot coexist with the running
probe. Each bench is a *stop the probe, add, start, read, remove, start the
probe again* sequence — budget the probe's downtime, not just the bench's
runtime.

`[find comment="…"]` is **exact equality**, so every command below is scoped by
comment and never by index. Every container needs its **own** comment:
`[find comment="…"]` on a shared value makes `stop`, `start` and `remove` hit
whichever matched first.

"Boot key" means the value is read once at startup — persisted immediately,
effective only after a restart
(`crates/fah-api/src/config_store.rs` `BOOT_KEYS`).

## Runbook

### R0 — read-only reconnaissance, run first, changes nothing

| Command | Reads |
| --- | --- |
| `/container/print detail` | **whether a `fah-probe` container exists at all**, plus its `envlists`, `mountlists`, `root-dir`, `interface`, `workdir`, `cpu-list` |
| `/container/envs/print detail` | whether `h1buf-env` still exists and what `fah-env` sets. Entries print as `list=… key=… value=…` |
| `/container/mounts/print detail` | the mount lists and their `src`. Production's are `fah-config` → `/kingston/fastadhunter/config` and `fah-data` → `/kingston/fastadhunter/data` |
| `/ip/firewall/nat/print chain=dstnat` | existing rules and their order — only if R7 is being run, which is optional and needs a fresh owner yes |
| `/ip/firewall/filter/print chain=forward` | whether the chain still ends without a final drop (routeros-traps §Reaching a container from the LAN) |
| `/ip/dhcp-server/lease/print where address=$CLIENT` | the static-lease precondition for every intercepted client (GAR §5.14) |
| `/log/print where topics~"container"` | the probe's start-up state and any boot error |

No rollback — nothing is written.

**R0 was run on 2026-09-08. Output and findings:
[p3-06-probe/device-state/](p3-06-probe/device-state/README.md)** — the
rollback reference, the state Step 4 must be able to return to.

What it settled, and what changes below because of it:

- **No `fah-probe` container exists.** One container only, `fastadhunter` 0.3.3
  on `veth1`. So **R5 is a plain `add`** — no `stop`, no `remove` — and
  **R1 has nothing to fix**: the probe's `/config` is written fresh at first
  boot, so R1 collapses into "do not write that key".
- **No campaign tars, no `fah-*` directories.** **R4 is the first write of the
  campaign**, not R1.
- **`veth3` is free** — it exists, comment `fah-test`, `172.17.0.4/24`, and
  carries no container.
- **Two mount lists, both production's.** The campaign adds two more, and
  **only the FAH probe needs them** — see R4b.
- **`h1buf-env` is gone.** Only `fah-env` remains, so `fahprobe-env` is built
  from scratch in R3.

Create every directory **before** the container that mounts it. A mount list
can exist while its target does not; the container then writes into the store
instead, where `/file/print` cannot see it and `container/remove` deletes it.
The check is direct — anything with a size under `/kingston/fah-probe/root/data`
means the mount is **not** attached; empty stubs dated at image-build time mean
it is. The `kingston/` prefix is not optional either: a path without it is the
internal NAND, ~15 MiB per extracted image on a 1 GiB partition shared with
RouterOS.

### R1 — `strategy = "fallback"` must never reach the probe config

**R0 turned this into a non-action.** There is no probe container and no probe
config, so there is no `strategy = "fallback"` to remove — the file is written
fresh at first boot and the key is simply never added. What remains is a check,
not an edit.

The trap it guards against is real and stays recorded: the tip build refuses to
load `strategy = "fallback"` (`fa9451a`; boot exits 1 with `"fallback" was
removed after 0.3.3; "adaptive" is the only strategy`). If a config carrying it
is ever restored from a campaign-1 backup, the probe will not boot, and a probe
that will not boot serves no API — so it must be fixed on the file, not over
the API. `POST /api/v1/config` is a deep merge and cannot delete a key; it can
only set the value to `adaptive`, which is also accepted at load.

| | |
| --- | --- |
| **Command (API route)** | `curl -sk -H "Authorization: Bearer $FAHKEY" -H 'content-type: application/json' -d '{"dns":{"upstreams":{"strategy":"adaptive"}}}' https://172.17.0.4:8443/api/v1/config` |
| **Command (file route)** | edit `/kingston/fah-probe/config/fastadhunter.toml` over SFTP and delete the `strategy` line under `[dns.upstreams]` |
| **Expected state** | after R5, `GET /api/v1/config` reports `dns.upstreams.strategy = "adaptive"` — the compiled-in default, with the key absent from the file |
| **Takes effect** | at the next container start — `dns.upstreams` is a boot key; the response carries `"restart_required": true` |
| **Restart required** | yes, and R5's image swap supplies it — no separate restart is needed if R1 is done before R5 |
| **Rollback** | none wanted: the previous value is what stops the probe booting. To return to the campaign-1 state, restore the old image *and* the old config together |

### R2 — the other three probe config keys

Values per [p3-06-testing-plan.md](../../../plan/wip/phase3/p3-06-testing-plan.md)
§The probe config. All three are boot keys.

| | |
| --- | --- |
| **Command** | `curl -sk -H "Authorization: Bearer $FAHKEY" -H 'content-type: application/json' -d '{"engine":{"mode":"dns+http+https"},"egress":{"allow_destinations":["'"$MAC"'/32"]},"https":{"interception":{"clients":[]}}}' https://172.17.0.4:8443/api/v1/config` |
| **Expected state** | `GET /api/v1/config` reports all three; the boot log then shows `HTTPS SNI listener bound` and a DoT line |
| **Takes effect** | next container start; response `"restart_required": true` |
| **Restart required** | yes — same restart as R1 |
| **Rollback** | re-`POST` the previous values read in R0, then restart |

`https.interception.clients` stays **empty here on purpose**. A listed client
with no CA installed is closed, not spliced (p3-04 L2), so clients are added
only in R8, after the CA is on the device.

### R3 — `fahprobe-env`

Without this the N axis does not exist: the probe inherits production's
`fah-env`, env beats file and API, and a P10 sweep silently measures N = 2 four
times. Editing `fah-env` is forbidden — it would move production's N mid-soak.

| | |
| --- | --- |
| **Command** | four adds, mirroring `fah-env` as read on 2026-09-08 — re-check against R0 before running: `/container/envs/add list=fahprobe-env key=FAH__RUNTIME__HTTP_RUNTIMES value=2`, `… key=MIMALLOC_ARENA_EAGER_COMMIT value=0`, `… key=MIMALLOC_PURGE_DECOMMITS value=1`, `… key=MIMALLOC_PURGE_DELAY value=0` |
| **Attach** | `/container/set [find comment="fah-probe"] envlists=fahprobe-env` |
| **Expected state** | `/container/print detail` shows `envlists=fahprobe-env`; after restart the boot log reads `http_runtimes=2` and `GET /api/v1/config` returns the same value |
| **Takes effect** | at container start; the env list is read when the container is created |
| **Restart required** | yes — stop and start the probe (R10's command pair) |
| **Rollback** | `/container/set [find comment="fah-probe"] envlists=fah-env` and restart; then `/container/envs/remove [find list="fahprobe-env"]` |

The alternative the plan allows is re-attaching phase 2.6's `h1buf-env`, if
R0 shows its `MIMALLOC_*` keys still match `fah-env`'s. Owner's choice; the
value is read back from `/config` after every restart either way.

**Verified on the dev box 2026-09-08.** The mechanism is proven — see [the review file](p3-06-phase3-verification-review.md) §Layer 3,
where `-e FAH__RUNTIME__HTTP_RUNTIMES=1` beat a file carrying `2`, in both the
boot log and `GET /api/v1/config`. What is owed here is the attachment, not the
mechanism.

### R4 — upload the four tip images

| | |
| --- | --- |
| **Command** | `scp fah-probe-arm64.tar fah-splicebench-arm64.tar fah-p4-arm64.tar fah-certs-arm64.tar bobdenaut:kingston/` (deploy-rb5009.md §2) |
| **Expected state** | `/file/print` lists all four tars under `kingston/` |
| **Takes effect** | on upload; nothing runs yet |
| **Restart required** | no |
| **Rollback** | `/file/remove [find name="fah-probe-arm64.tar"]`, and the same for `fah-splicebench-arm64.tar`, `fah-p4-arm64.tar`, `fah-certs-arm64.tar` |

Campaign 1's `a2d0802` images are retired and must not be reused for any
campaign-2 figure.

### R4b — mount lists, the FAH probe only

Only `Dockerfile.fahprobe` declares `VOLUME ["/config", "/data"]`. The three
one-shot containers need no mount: `fah-certs` writes its criterion output to
`CRITERION_HOME=/tmp/criterion`, `fah-p4` uses `TMPDIR=/tmp`, and
`fah-splicebench` writes nothing but stdout. Their results are read from the
container log, so losing `/tmp` on `remove` costs nothing.

| | |
| --- | --- |
| **Command** | `/container/mounts/add list=fahprobe-config src=/kingston/fah-probe/config dst=/config` and `/container/mounts/add list=fahprobe-data src=/kingston/fah-probe/data dst=/data` |
| **Precondition** | `/kingston/fah-probe/config` and `/kingston/fah-probe/data` exist — create them over SFTP first |
| **Expected state** | `/container/mounts/print detail` lists both alongside production's `fah-config` and `fah-data`, with `src` under `/kingston/fah-probe/` and never under `/kingston/fastadhunter/` |
| **Takes effect** | when a container that names them starts |
| **Restart required** | no — but a container already running does not pick them up |
| **Rollback** | `/container/mounts/remove [find list="fahprobe-config"]` and the same for `fahprobe-data` |

### R5 — add and start the campaign containers, **one at a time**

**All four bind `veth3`, and one veth carries one container at a time**
(confirmed on the device 2026-09-04). They can never be added as a batch. Only
one of the four may exist on `veth3` at any moment; the next is added only
after the previous one is removed.

The FAH probe is the long-lived one. The other three are one-shot: they run,
print to the container log, and exit. Each bench therefore costs the probe's
downtime as well as its own runtime.

**The cycle, per bench.** Repeat for `fah-splicebench`, then `fah-p4`, then
`fah-certs` — never two in flight:

```routeros
/container/stop   [find comment="fah-probe"]
/container/remove [find comment="fah-probe"]
/container/add    ... one bench, from the four below ...
/container/start  [find comment="<that bench>"]
/log/print where topics~"container"
/container/remove [find comment="<that bench>"]
/container/add    ... fah-probe again, from the four below ...
/container/start  [find comment="fah-probe"]
```

**The four `add` commands. Run exactly one, then finish its cycle.**

```routeros
/container/add file=kingston/fah-probe-arm64.tar interface=veth3 \
  root-dir=/kingston/fah-probe/root \
  mountlists=fahprobe-config,fahprobe-data envlists=fahprobe-env \
  cpu-list="" workdir=/home/nonroot logging=yes start-on-boot=no comment="fah-probe"
```

```routeros
/container/add file=kingston/fah-splicebench-arm64.tar interface=veth3 \
  root-dir=/kingston/fah-splicebench/root \
  cmd="--reps 1 --size-mib 8" \
  cpu-list="" workdir=/home/nonroot logging=yes start-on-boot=no comment="fah-splicebench"
```

```routeros
/container/add file=kingston/fah-p4-arm64.tar interface=veth3 \
  root-dir=/kingston/fah-p4/root \
  cpu-list="" workdir=/home/nonroot logging=yes start-on-boot=no comment="fah-p4"
```

```routeros
/container/add file=kingston/fah-certs-arm64.tar interface=veth3 \
  root-dir=/kingston/fah-certs/root \
  cpu-list="" workdir=/home/nonroot logging=yes start-on-boot=no comment="fah-certs"
```

| | |
| --- | --- |
| **Expected state** | `fah-probe`: `/log/print where topics~"container"` shows the listeners binding and `dropped privileges after binding uid=65532 gid=65532`, and `GET /health` answers `200`. The three one-shot containers print their result and exit — read it from the log, they leave no file |
| **Takes effect** | at `start` |
| **Restart required** | `fah-probe`'s start **is** the restart that applies R1, R2 and R3 |
| **Rollback** | `/container/stop [find comment="fah-probe"]` then `/container/remove [find comment="fah-probe"]`, and the same pair with `fah-splicebench`, `fah-p4`, `fah-certs`. For the probe, re-`add` from the campaign-1 tar with the campaign-1 config restored |

**If an `add` fails because `veth3` is busy, that is the rule working** — find
what still holds it with `/container/print detail` and remove it before
retrying. Do not add a second veth to work around it.


Three notes, all from [routeros-traps.md](../../routeros-traps.md):

- `logging=yes` is **not** the default, and without it there is no container
  log — which for the three one-shot containers is the *only* output channel.
- Only `fah-probe` carries `mountlists` and `envlists`. Giving a bench
  `mountlists=fah-config,fah-data` would point it at production's directories.
- Each `comment` is unique. `[find comment="…"]` is exact equality, so a shared
  comment makes `stop`, `start` and `remove` hit whichever matched first.

After the probe boots, confirm real content under `/kingston/fah-probe/config`.
A named mount is not a working mount: a failed one writes into the store
instead, where `container/remove` deletes it.

### R6 — the P10 restarts, one per arm

P10 is the only arm that sweeps N, and it runs before every other on-device
arm because it fixes the N later figures are taken at.

| | |
| --- | --- |
| **Command** | one of the four values, then a restart — see the block below |
| **Expected state** | boot log reads `http_runtimes=0`, `1`, `2` or `4` and `GET /api/v1/config` returns the same — `p10-domains.mjs --n <that value>` refuses to run if they disagree |
| **Takes effect** | at `start` |
| **Restart required** | yes, one per arm — four restarts for the four values |
| **Rollback** | set the value back to `2` and restart; that is also the correct end state once the sweep is read |

The read-back check is not optional. It is what stops four identical rows being
reported as a plateau that is an artefact of an unattached env list.

### R7 — dst-nat 443 (v4) — **OWNER ONLY, OUTSIDE THIS RUNBOOK'S BOUNDARY**

**No measured arm needs this rule.** SNI, P1, P2, P3, P4, P5, P6, P10, D11,
P8 and P9 all reach the probe directly at `172.17.0.4:8444`. R7 exists only for
**transparent interception of real device browsing** — the CA install
walkthrough, the ECH retry check and the pinned-app spot check. Skip it
otherwise.

**This is a firewall edit, not a `veth3` container edit, so it sits outside
this runbook's hard boundary.** No agent proposes, prepares or runs it. It is
recorded here only because plan Step 4 item 3 declares it, and it needs a
separate explicit decision from the owner every time.

Read `/ip/firewall/nat/print chain=dstnat` first. As read on 2026-09-08 the
chain ends at index 10 with no final drop, and nothing in it matches tcp/443
from the LAN — rules 9 and 10 steer port 80 to `172.17.0.2:8080`, and rules
5 and 6 are disabled. On that chain an append is safe and `place-before` is
not needed. Re-read before running: this is a property of the current rule
set, not a guarantee.

| | |
| --- | --- |
| **Command** | two rules, the same pair already established for production in [the review file](p3-06-phase3-verification-review.md), retargeted to the probe at `172.17.0.4`: `/ip/firewall/nat/add chain=dstnat action=accept protocol=tcp dst-port=443 dst-address-list=fah-http-skip comment="p3-06 https steer: leave local traffic alone"` then `/ip/firewall/nat/add chain=dstnat action=dst-nat protocol=tcp dst-port=443 in-interface-list=LAN src-address=!172.17.0.0/24 to-addresses=172.17.0.4 to-ports=8444 comment="p3-06 https steer"` |
| **Expected state** | one new rule, visible with a rising packet counter once a listed client browses; the probe's `listeners.https.connections` moves on `GET /api/v1/telemetry` |
| **Takes effect** | immediately, on new connections only — established flows keep their existing conntrack entry |
| **Restart required** | no |
| **Rollback** | `/ip/firewall/nat/remove [find comment~"p3-06 https steer"]` — both rules, matched on the shared comment prefix |

Three constraints carried from the plan, none of them new here:

- **`protocol=tcp` only.** UDP/443 stays unsteered so browsers fall back to TCP
  rather than black-holing HTTP/3.
- **Target is 8444**, the `[https.listen]` port. 8443 is the API's and p3-03
  rejects the collision at startup.
- **Steering all of `:443` closes no-SNI and ECH connections.** The container
  cannot recover their destination — `getsockopt(SO_ORIGINAL_DST)` returns
  `ENOENT` on a dst-nat'd flow, measured on-device 2026-08-31, because the NAT
  conntrack lives in the router's netns. Such a connection is closed, not
  forwarded, and the DNS layer is its only backstop. Confirm the deployed lists
  cover the domains that matter before enabling full mode.

The v6 half is a decision, not a command: either equivalent v6 steering, or an
explicit recorded decision that v6/443 stays unsteered this phase, naming the
traffic that leaves uncovered.

### R8 — list an intercepted client, after its CA install

Order matters and is an owner decision already taken (p3-04 L2): a listed
client with no CA is **closed, not spliced**, so listing it early turns every
HTTPS connection from that device into a closed socket and a `status 0` event
until the install lands.

| | |
| --- | --- |
| **Precondition** | the CA is installed on the device (dashboard login, download `fastadhunter-ca.crt`, install from Downloads — `GET …/ca/export` is authenticated, so not a bare URL), **and** `/ip/dhcp-server/lease/print where address=$CLIENT` shows a static lease |
| **Command** | `curl -sk -H "Authorization: Bearer $FAHKEY" -H 'content-type: application/json' -d '{"https":{"interception":{"clients":["'"$CLIENT"'"]}}}' https://172.17.0.4:8443/api/v1/config` |
| **Expected state** | `GET /api/v1/config` lists the client; after restart the boot log reads `HTTPS interception active for the listed clients`; the device sees `FastAdHunter CA` as issuer |
| **Takes effect** | next container start — `https` is a boot key |
| **Restart required** | yes: `/container/stop` + `/container/start` on `fah-probe` |
| **Rollback** | re-`POST` with `"clients": []` and restart; the device then splices and its own CA install is harmless |

### R9 — Private DNS hostname

| | |
| --- | --- |
| **Command** | `curl -sk -H "Authorization: Bearer $FAHKEY" -H 'content-type: application/json' -d '{"rules":["'"$DNSNAME"'$dnsrewrite=172.17.0.4"]}' https://172.17.0.4:8443/api/v1/rules/user`, then on the phone Settings → Network → Private DNS → hostname |
| **Expected state** | the hostname resolves to the probe over plain DNS (the bootstrap the phone uses while validating), and the SNI-minted leaf validates |
| **Takes effect** | immediately — `/rules/user` applies live, recompiles and atomically swaps |
| **Restart required** | no |
| **Rollback** | `PUT /api/v1/rules/user` with the previous rule set, and unset Private DNS on the phone |

Start by reading the container log for a certificate failure at boot: a
`DotListener::Closed` posture is surfaced but easy to miss. Record the tested
assumption either way — whether this device's Private DNS validation consults
the user CA store. Vendor behaviour varies, and a refusal means the
imported-real-certificate route is the remaining path.

### R10 — probe stop/start for the certificate-store checks

Used by the import-then-restart check and by every boot-key change above.

| | |
| --- | --- |
| **Command** | `/container/stop [find comment="fah-probe"]` then `/container/start [find comment="fah-probe"]` |
| **Expected state** | after an import, the acceptor serves the imported certificate and `GET /api/v1/certificates` reports `"imported"` |
| **Takes effect** | at `start` |
| **Restart required** | this is the restart |
| **Rollback** | none needed — the container returns to whatever its `/config` holds. To undo an import, re-import the previous pair and restart again |

`/container/stop` on the **production** container is never part of this
campaign. Everything in Step 4 item 7 is probe-only, propose-only for
production.

### R11 — the 24 h full-mode soak — **OWNER ONLY, OUTSIDE THIS RUNBOOK'S BOUNDARY**

**This touches the production container on `veth1`, not a `veth3` campaign
container, so it sits outside this runbook's hard boundary.** No agent
proposes, prepares or runs it. It is a deploy to the household's live resolver
and carries its own separate approval.

| | |
| --- | --- |
| **Blocked until** | the 0.3.3 soak ends **2026-09-14** — same container; audit F6 corrects the stale 2026-09-08 date |
| **Command** | the normal production deploy sequence, plus `engine.mode = "dns+http+https"` |
| **Expected state** | RAM ≤ 128 MB steady — budget in decimal MB, readings in MiB, compare like with like |
| **Takes effect** | at start |
| **Restart required** | yes |
| **Rollback** | redeploy 0.3.3 and set `engine.mode` back to `dns` |

Watch items a–f are listed in the plan and are readings, not commands.

### What this runbook deliberately does not contain

- **No `memory-high`.** It killed the live resolver once at `200M`; it is not
  proposed here at any value.
- **No CPU pinning.** All four carry `cpu-list=""` — empty is *no
  restriction*, not "no CPUs". Production's live value is
  `cpu0,cpu1,cpu2,cpu3`, which is the same thing spelled out. Pinning a probe
  to fewer cores would stop it standing in for the thing it measures.
- **No speculative firewall rule.** R0 records that `chain=forward` ends
  without a final drop, so LAN → container already works; if that still holds,
  R7 is the only firewall write in the campaign.
- **No P3 command.** P3 cannot run until a publicly trusted h2 origin exists
  under a public name (Let's Encrypt DNS-01 on the Mac). [The review file](p3-06-phase3-verification-review.md) §Layer 3 established
  that no local substitute exists: the release image reads
  `FAH_TEST_UPSTREAM_ROOT` only under `#[cfg(feature = "test-harness")]`.
  Writing a P3 step now would be writing a step that cannot be run.
