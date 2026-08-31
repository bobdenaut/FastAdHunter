# RouterOS / RB5009 — operational facts and traps

Things about the deployment target that cost time to learn and are recorded
nowhere else. Not a deploy guide — that is
[deploy-rb5009.md](deploy-rb5009.md). Every line here is something that was
wrong once.

> **Agents never change anything on this router.** Read-only queries are fine;
> every write is proposed and the owner runs it. See the Working agreement in
> the root [CLAUDE.md](../CLAUDE.md).

## Topology

Verified against the device 2026-08-02 (`/interface/veth/print detail`,
`/container/print detail`, `/container/mounts/print detail`).

| Interface | Address | What |
| --- | --- | --- |
| `veth1` | `172.17.0.2` | **FastAdHunter**, DNS on 53, API on 8443 |
| `veth3` | `172.17.0.4` | `fah-test` — the scratch interface for a probe container beside the live one. **Enabled**; it shows no `R` flag while nothing is attached, so no enable command is needed |

Gateway is `172.17.0.1`; each veth also carries an IPv6 in
`fd6c:7f32:8e91:1::/64` (static ULA) plus a global address by SLAAC:
`CONTAINERS` holds `::1/64 from-pool=ipv6-pool` with `advertise=yes` and an
`/ipv6/nd` entry (`advertise-dns=no`), so the container derives
`<prefix>:6c29:acff:fed8:a1f1` itself and follows every delegation rotation
without a restart (verified 2026-08-22: ping 0.5 ms, neighbor entry with the
container MAC; prefix lifetimes `valid 10m / preferred 5m`).
`/interface/veth address=` cannot take a pool address — never hardcode a
global there; an `/ipv6/address` on `veth1` itself lands on the bridge (slave
port) and never reaches the container. `srcnat masquerade
fd6c:7f32:8e91:1::/64 → DIGI` stays for ULA-sourced flows (and the ≤10 min
gap after a rotation); global-sourced traffic leaves un-NATed.

**Router-wide memory graphs are not a FAH measurement.** RouterOS itself and any
other container share the same 1 GB budget, so attribute per-container with
`memory-current` before blaming FAH — and check what is actually running rather
than assuming, since the container set changes. A stale `ENV_FAH` list
(`FAH__DNS__LISTEN__PORT`) is unused; the live one is `fah-env` (the
`MIMALLOC_*` keys).

## Container configuration

| Field | Verdict |
| --- | --- |
| `comment` | **exactly `fastadhunter`**, no version — `[find comment="fastadhunter"]` is exact equality and breaks if a version is appended |
| `DNS` | **is** written to `/etc/resolv.conf`, once, at container start (7.21.5 — older builds did not). Empty falls back to whatever `/ip/dns servers` holds *at that instant*, and it is never refreshed afterwards. Harmless here: FAH resolves through its own upstreams and never reads the file |
| `Tmpfs` | harmful — `/data` on tmpfs loses cache mtimes, which brings back the double compile at boot |
| `Auto Restart Interval` | **empty = nothing restarts FAH if it dies**; `Start On Boot` only covers router boot |
| `Workdir` | `/home/nonroot`, inherited from the `:nonroot` base, harmless (all FAH paths are absolute) |
| `Entrypoint`/`Cmd` | only real use is `--healthcheck` to debug a bricked config — there is no shell in distroless |
| image `HEALTHCHECK` | **inert on this device.** RouterOS 7.21.5 has no health field in `/container/print detail`, before or after deploy, and no reaction to an unhealthy container. The image's `HEALTHCHECK` runs for nothing here; the binary's `--healthcheck` only helps when something calls it. Verified p2.5-09 V3b (F3), 2026-08-23 |
| `memory-high=200M` | **KILLS FAH.** Do not propose it as a "safe falsifiable check"; it took down the live resolver once |
| `root-dir` | `/kingston/fastadhunter/root`. A path without the `kingston/` prefix is the internal NAND — ~15 MiB per extracted image on a 1 GiB partition shared with RouterOS |
| `logging=yes` | **not the default.** Without it there is no container log, which is the only debugging channel and the authority on FAH's start time |
| `envlists` | `fah-env` (the `MIMALLOC_*` keys). `ENV_FAH` is a stale list and is not attached |
| `interface` | `veth1` |

**A named mount is not a working mount.** RouterOS accepts `mounts=` and reports
them in `/container/print detail` without guaranteeing the container writes
through them. A silently ineffective mount looks healthy and loses every write on
`container/remove` — which once looked exactly like a list-persistence code bug.
When on-device state seems not to persist, check the *instance*, not the code:
`/container/mounts/print detail` cross-checked against the running container's
`mounts=`.

The **direct** check is better: `/config` and `/data` exist inside the image as
empty 777 stubs, so a failed mount does not error — FAH just writes into the
store instead, where `/file/print` cannot see it and `container/remove` deletes
it. **Anything with a size under `kingston/fastadhunter/root/data` means the
mounts are not attached.** Empty stubs dated at image-build time mean they are.

## Inside a container store

`/file/print` shows a store as one opaque entry (`type=container store`) and
never enumerates it. **SFTP does** — browse `kingston/fastadhunter/root/` with
WinSCP to see the extracted rootfs. Not seeing a Linux tree in `/file/print` is
not evidence that extraction failed.

| Entry | Origin |
| --- | --- |
| `fastadhunter` (~9.5 MB) | the static musl binary — this is the whole application |
| `etc`, `home/nonroot`, `tmp`, `root`, `var` | the distroless base |
| `config`, `data` | mount-point stubs from the Dockerfile, 777, empty when the mounts work |
| `bin`, `boot`, `dev`, `lib`, `proc`, `run`, `sbin`, `sys`, `usr` | **RouterOS's**, not ours — the runtime's mount-point skeleton, all stamped with the RouterOS build date |

Timestamps read cleanly: the skeleton carries the RouterOS build date, the binary
and the stubs carry the image build date, `home` carries the extraction, and
`etc` is touched at every container start when the runtime writes `hosts`.
Everything is owned by uid 0 even though FAH runs as 65532 — privileges are
dropped after binding, which the start-up log states.

## Commands that are not where you expect

- **`/disk/print` is top-level** — there is no `/system/disk`.
- `/system/resource/print`'s `free-hdd-space` is the internal NAND, not the
  kingston SSD.
- **`/system/resource/print` uptime is the ROUTER's, not the container's.** The
  container log is authoritative for FAH's start time.
- **There is no `/system/scheduler`** on this box — the `fah-liveness` script in
  the deploy doc was never applied, so nothing restarts FAH if it dies.
  Continuous `uptime_seconds` remains direct proof of process life.
- **`/tool/netwatch` holds a DNS failover** (`comment="change DNS if needed"`):
  every 1 min it resolves `www.google.com` against `172.17.0.2`, and on failure
  points `/ip/dns servers` at public resolvers, restoring `172.17.0.2` on
  recovery. It does not restart the container. Two consequences: **"nobody
  complained" is not evidence FAH stayed up**, and clients resolve unfiltered
  for the length of every redeploy. The probe survives blocking only because
  `null_ip` returns a valid A record — under NXDOMAIN blocking, a list carrying
  the probe host would read as FAH being dead.
- **QPS is `/interface monitor-traffic veth1` → `tx-packets-per-second`**
  (`tx` = into the container). Firewall counters also work but this is the
  answer.

## Build and deploy pipeline

RouterOS has no `docker` CLI — never `docker load`.

1. `docker buildx build --platform linux/arm64 … -o type=docker,dest=fah-raw-<ver>.tar`
   emits **OCI layout**, which RouterOS cannot import.
2. Convert with skopeo to a legacy **docker-archive** (flat, ~12 MB).
3. Copy the tar (see the deploy-copy note in agent memory for the scp target).
4. `/container` add reusing the existing settings: `interface=veth1`,
   `root-dir=/kingston/fastadhunter/root`, `mountlists=fah-config,fah-data`,
   `envlists=fah-env`, `workdir=/home/nonroot`, `start-on-boot=yes`,
   `logging=yes`, `comment="fastadhunter"`.

**buildx needs QEMU re-registered after any Docker Desktop restart**
(`docker run --privileged --rm tonistiigi/binfmt --install arm64`) **and then the
builder container restarted** (`docker restart buildx_buildkit_fah-builder0`).
`buildx inspect --bootstrap` alone does not refresh the platform list, and
recreating the builder loses the build cache.

## On-device measurement needs its own container

**There is no container shell** — distroless/static, and RouterOS exposes no
`docker exec`. The obvious plan, "scp a bench binary to the router and run it in
the container", does not work. Budget a measurement as *build and ship a second
container*, not *copy a file*:

1. Cross-compile to `aarch64-unknown-linux-musl`, pack as a single-layer OCI tar.
2. `scp <name>-arm64.tar bobdenaut:kingston/` (see [deploy-rb5009.md](deploy-rb5009.md) §2).
3. `/container/mounts/add` if it needs `/data` — read-only is usually enough,
   the cached list copies live there.
4. `/container/add file=… interface=veth3 root-dir=… mounts=… cmd=… envlist=…`
5. `/container/start`, read `/log print where topics~"container"`,
   `/container/remove`.

The production container is never stopped; the probe exits on its own. **Steps
2–5 are router writes — an agent never runs them.** Propose the exact commands
and the owner executes them, and read `/container print detail` +
`/container/mounts print` first, because the mount names, `root-dir` and veth are
deployment-specific.

**Container CPU affinity exists**: the `cpu-list` field on `/container`
(verified 7.21.5, 2026-08-24). Empty means *no restriction*, not "no CPUs" —
threads are schedulable on all four cores and compete with RouterOS's own
`networking`, `bridging` and `firewall` tasks. Production carries `cpu-list=""`,
so **do not pin a probe**: it would stop standing in for the thing it measures.
Where the x86 reference was core-pinned and the comparison needs it, take 3 runs
and report the median instead.

`/tool/profile cpu=all` attributes CPU per process per core, which `cpu-load`
alone cannot. It keys on process name, so two containers running the same binary
are not separated.

**A second container can be added from the same image tar.** RouterOS extracts
it into the new `root-dir` and auto-suffixes the derived name (`…-0.2.19` →
`…-0.2.19-2`), so there is no duplicate-name collision. Give it its own
`root-dir`, its own mount lists, and a `comment` that is **not** `fastadhunter`
— `[find comment="fastadhunter"]` is exact equality and would match both.

**A mount list can exist while its target directory does not.** Creating
`/container/mounts` entries does not create the directories they point at; the
container then starts, silently writes into the container store instead, and
loses everything on `container/remove`. Create the tree first and confirm real
content appears under it after boot.

## API access

- Auth is `Authorization: Bearer <key>` — **not** `X-API-Key`.
- `/health` is at the **root**; everything else under `/api/v1/`.
- The config file is `/config/fastadhunter.toml`, **not** `config.toml`.
- `/api/v1/history/perf` returns **`items`**, not `samples`, and needs explicit
  `from`/`to` to return anything useful.
- Engine counters, latency, upstreams, cache and memory come from one call:
  `/api/v1/telemetry`. There is no per-query HTTP endpoint — individual events
  are only on `WS /api/v1/events`.
- **Latency percentiles are not on `/telemetry`.** It carries `{count,
  sum_seconds}` per stage, from which only a mean is derivable. `forward_p50`,
  `forward_p99`, `cache_hit_*` and `block_*` live only on
  `/api/v1/history/perf`, per sample interval, and there is no Prometheus
  surface exposing raw buckets.
- **`upstreams[].attempts` and `counters.swr.*` lag by up to 10 s** — they are
  republished by the binary's telemetry poll, not read live, while
  `latency.*` and `counters.dns.*` are current. Differencing two snapshots taken
  while traffic is flowing undercounts them. Drain quietly past one poll before
  the closing snapshot.

## Reaching a container from the LAN

Verified 2026-08-24. `172.17.0.0/24` is a connected route via `CONTAINERS`, and
`chain=forward` **ends without a final drop**, so LAN → container and
container → LAN both fall through to the default accept. **No firewall rule is
needed to drive a probe container from a LAN host** — and none should be added
speculatively, since `add` appends behind whatever is already there.

Read the chain before concluding this still holds; it is a property of the
current rule set, not a guarantee.

A generator that uses **one UDP socket** keeps the router at a single conntrack
entry for the whole stream and gets fastpathed after the first packet. One that
opens a fresh source port per query pays conntrack setup at the query rate.

## `SO_ORIGINAL_DST` does not work from the container

**Measured 2026-08-31.** A transparent proxy usually recovers the pre-dst-nat
destination with `getsockopt(SO_ORIGINAL_DST)`. It does **not** work here — not
because RouterOS lacks the option, but because the dst-nat and its conntrack run
in the **router's** network namespace, while the container's socket sees only the
post-NAT flow arriving across the veth.

Test: a probe container on `veth3`, a temporary `dstnat` rule redirecting
`dst-port=4443` → `172.17.0.4:4443`, and a LAN host (`192.168.10.10`) aiming at
`1.1.1.1:4443`. The probe's `getsockopt(SO_ORIGINAL_DST)` returned **`ENOENT`
(errno 2)**, with `accept_local=172.17.0.4:4443` confirming the redirect fired.
`ENOENT`, **not** `ENOPROTOOPT`: the option is supported, but the container's
netns holds no conntrack record of the router-side NAT, so the original
destination is unrecoverable from inside.

**Consequence.** Any transparent interception in the container must derive the
destination from an L7 claim, never the socket: HTTP reads the `Host` header
(`crates/fah-http/src/claim.rs`), and Phase 3's HTTPS path must read SNI. A
no-SNI / ECH TLS connection therefore has no recoverable destination and cannot
be forwarded — a hard transport limit, not a policy choice. The probe was a
throwaway libc `getsockopt(SO_ORIGINAL_DST)` listener in a scratch container on
`veth3`, not kept in-tree.

## IPv6 — verify before acting

These entries were written across several sessions and partly supersede each
other. **Re-read the live config before changing anything here.**

- Two `/ipv6/firewall/nat` dstnat rules catch *all* IPv6 :53 from BRIDGE
  regardless of destination, targeting `fd6c:7f32:8e91:1::2/128` — the
  container's ULA on `veth1`. Confirmed 2026-08-09.
- IPv6 **is** filtered in practice: 22 % of real traffic, and the dual-stack
  listener served ~22.4 M queries under load.
- RA advertises `dns=fd6c:7f32:8e91:1::2`, the same ULA, so client DNS survives
  a delegation rotation. `preferred-lifetime=5m` / `valid-lifetime=10m` /
  `ra-lifetime=10m` are short on purpose: clients abandon a dead prefix in
  minutes rather than hours, which is what makes a rotating ISP survivable.
- `/ipv6/firewall/filter` chain=input has **no final drop**: 8 accepts then
  implicit accept, while the router holds a global address. Router services are
  reachable from the internet over IPv6. A general input drop was deliberately
  not proposed without seeing the WireGuard config.

**No DIGI global is hardcoded anywhere in the config.** Verified 2026-08-09 by
grepping `2a02:` over a full `/export hide-sensitive`: the single hit is the
`log-wan-ip` scheduler's comment, which is stored state rather than
configuration. Keep it that way — every place that needs the LAN prefix derives
it (`from-pool` on the address, `prefix-address-lists` on the firewall list),
and everything else uses the ULA or link-local.

What no router config can fix: clients hold global addresses from the delegated
prefix, so a rotation kills their established connections regardless. The short
RA lifetimes above are the only mitigation available.

**The delegation rotates, so no rule may hardcode a global v6 prefix.** Eight
distinct `/56`s observed, three of them in one working session:
`2a02:2f04:5100:e700`, `…5303:6800`, `…520a:3d00`, `…540c:7900`, `…5407:c600`,
`…5204:a100`, `…5300:3500`, `…5204:500`.

**The cause is a PPPoE redial**, not DHCPv6 lease policy. The `never` (infinite)
DHCPv6 lifetimes are therefore not self-contradictory: the lease is not
expiring, the session under it is. Diagnose a rotation by
`/interface/pppoe-client/monitor` uptime — not by the DHCPv6 client, and **not
by the public IPv4**.

**The WAN IPv6 address and the delegated prefix rotate together; the public IPv4
is independent of both.** Two redials, opposite outcomes: 2026-08-09 11:36
turned over all three (IPv4 at 11:36:21, the v6 pair at 11:36:25 — IPCP first on
the fresh session, DHCPv6 four seconds behind, session uptime `2m27s` shortly
after), while 2026-08-10 11:36 returned the **same** IPv4 `5.12.68.56` beside a
new WAN v6 and a new `…5204:500::/56`. An unchanged public IPv4 is no evidence
that the prefix held.

- The BRIDGE address is `::1/64` `from-pool=ipv6-pool` — offset pinned, prefix
  followed. A prefix pinned in the address goes `I` invalid at the next
  rotation and the LAN silently loses its global.
- A firewall rule that needs the LAN prefix takes it from an address list the
  DHCPv6 client maintains (`/ipv6/dhcp-client set prefix-address-lists=…`),
  never a literal. A stale literal in a *skip* rule fails dangerously: the skip
  stops matching and the rule below it acts on traffic it was written to leave
  alone.
- Global IPv6 reachability is not implied by a bound client and an active
  default route. Measured 2026-08-09: `/ping 2606:4700:4700::1111` from the
  router, 100 % loss, with both present. Test it, do not infer it.

## Logging — what is instrumented, and its traps

**`/log print` is a unified view over every logging action, not the memory
buffer.** A topic routed only to disk still appears there — `dhcp` has one rule,
to `netlog`, and its lines show up in `/log print` all the same. A quiet
`/log print` means nothing was logged, not that it went somewhere else.

Topics go to a `netlog` action on the Kingston (`kingston/net-log.*.txt`,
8 × 5000 lines) rather than to memory or internal flash: memory is 500 lines
shared with the `[CONTAINER]` topic that carries FastAdHunter's own output, and
internal flash means wear.

### What to search for

| Query | Returns |
| --- | --- |
| `/log print where message~"IPv6 global"` | v6 reachability, `UP` / `DOWN`, on transition |
| `/log print where message~"IPv4 global"` | v4 reachability, same |
| `/log print where message~"WAN change"` | `[old] => [new]` for `v4=` public address, `v6=` WAN global, `pd=` delegated prefix |
| `/log print where topics~"script"` | all of the above together — netwatch and `log-wan-ip` |
| `/log print where topics~"ppp"` | session up/down **with the disconnect reason** |
| `/log print where topics~"container"` | FastAdHunter's own output |

Prefixes `[DHCP]`, `[ROUTE]`, `[LINK]`, `[PPPOE]`, `[PPP]` tag those lines in the
file on disk.

**`- administrator request` on a `terminating…` line is what separates your own
redial from the ISP dropping the session.** Without it a burst of reconnects
reads as upstream instability. Measured 2026-08-10 over a 24.8 h window: five
session establishments, every disconnect administrator-requested, none
ISP-initiated.

**The memory buffer and the disk file do not retain the same span**, so a
question older than the buffer needs the file. The disk actions filter
`!debug,!packet`; the memory buffer does not, and `pppoe,ppp` packet lines flood
it. Measured in the same window: the buffer reached back to 11:43 while the file
still held 10:53 — and the evicted 50 minutes were exactly the `terminating…`
lines that answered the question.

### Reading the file needs `scp`, not `/file get`

**`/file get … contents` returns an empty string above ~64 KiB, with no error.**
Measured: a 65.1 KiB file gives `:len` of `0`. With `disk-lines-per-file=5000`
each file reaches ~500 KiB, so this is the normal state, not an edge case — and
it fails silently, which reads as "logging stopped".

```powershell
scp rb5009:kingston/net-log.0.txt .
Select-String "WAN change|IPv6 global|\[PPP\]" net-log.0.txt
```

Under 64 KiB, `:put [/file get …]` does work, but it emits bare `\n` — 54 LF
against 1 CR in a measured sample — so a terminal staircases each line from where
the previous one ended. The file is not corrupt. `/log print` is unaffected
because RouterOS formats that output itself.

- **`topics=dhcp` matches `dhcp,debug,packet` too**, and one LAN client's lease
  renewal is ~20 lines of option dumps. The rule is `dhcp,!debug,!packet`;
  without the negations the DHCPv6 prefix events are buried within hours.
- `warning` has two rules, `netlog` and `memory`, and **`/log print` renders one
  entry per matching rule** — so every warning appears there twice while the disk
  file holds one copy. Do not count occurrences in `/log print`. The second rule
  is redundant, since `/log print` already shows disk-routed entries; removing it
  removes the duplication and loses nothing.
- Netwatch (`v6-global`, `v4-global`) fires its scripts on **transition only**;
  verified by an entry sitting `down` across probe intervals without emitting a
  second line.
- **A scheduled script's `:global` state does not survive between runs** —
  `/system/script/environment/print` is empty after a run that set one. A script
  that logs "only on change" therefore logs on *every* run, ~8,600 entries/day
  at a 1-minute interval, which rolls the whole disk log in under a day.
  `log-wan-ip` keeps its previous reading in the **scheduler's comment**, which
  survives runs and reboots; it writes to the scheduler rather than to itself so
  nothing modifies a script mid-execution. Silence between runs is the proof it
  persists.
- `/ip/cloud`'s `public-address` lags by up to `ddns-update-interval` (1d) and is
  measurably wrong: it read `5.12.207.102` while the PPPoE interface held
  `5.12.68.56/32`. The interface address is authoritative; the IPv4 public
  address rotates too, not only the v6 delegation.
