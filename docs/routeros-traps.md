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
| `veth2` | `172.17.0.3` | **postgres** (`postgres:17-alpine`), unrelated to FAH |
| `veth3` | `172.17.0.4` | `test` — the scratch interface for trying a temporary FAH build beside the live one; currently disabled, no container attached |

Gateway is `172.17.0.1`; each veth also carries an IPv6 in
`fd6c:7f32:8e91:1::/64`.

**Another container shares the box.** `postgres` runs `start-on-boot=yes` with
`memory-high=unlimited` and holds ~49 MiB. It is not FAH's, but it is on the same
1 GB budget, so router-wide memory graphs are **not** a FAH measurement —
attribute per-cgroup with `memory-current` before blaming FAH. A stale `ENV_FAH`
list (`FAH__DNS__LISTEN__PORT`) is likewise unused; the live one is `fah-env`
(the `MIMALLOC_*` keys).

## Container configuration

| Field | Verdict |
| --- | --- |
| `comment` | **exactly `fastadhunter`**, no version — `[find comment="fastadhunter"]` is exact equality and breaks if a version is appended |
| `DNS` | **is** written to `/etc/resolv.conf`, once, at container start (7.21.5 — older builds did not). Empty falls back to whatever `/ip/dns servers` holds *at that instant*, and it is never refreshed afterwards. Harmless here: FAH resolves through its own upstreams and never reads the file |
| `Tmpfs` | harmful — `/data` on tmpfs loses cache mtimes, which brings back the double compile at boot |
| `Auto Restart Interval` | **empty = nothing restarts FAH if it dies**; `Start On Boot` only covers router boot |
| `Workdir` | `/home/nonroot`, inherited from the `:nonroot` base, harmless (all FAH paths are absolute) |
| `Entrypoint`/`Cmd` | only real use is `--healthcheck` to debug a bricked config — there is no shell in distroless |
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

Unknown until checked: whether the installed RouterOS can pin container CPU
affinity. If it cannot, take 3 runs and report the median — the x86 references
it is compared against were core-pinned.

## API access

- Auth is `Authorization: Bearer <key>` — **not** `X-API-Key`.
- `/health` and `/metrics` are at the **root**; everything else under `/api/v1/`.
- The config file is `/config/fastadhunter.toml`, **not** `config.toml`.
- `/api/v1/history/perf` returns **`items`**, not `samples`, and needs explicit
  `from`/`to` to return anything useful.
- Engine counters, latency, upstreams, cache and memory come from one call:
  `/api/v1/telemetry`. There is no per-query HTTP endpoint — individual events
  are only on `WS /api/v1/events`.

## IPv6 — verify before acting

These entries were written across several sessions and partly supersede each
other. **Re-read the live config before changing anything here.**

- Two `/ipv6/firewall/nat` dstnat rules catch *all* IPv6 :53 from BRIDGE
  regardless of destination. They were retargeted off a stale SLAAC ULA on
  2026-07-19; AdGuard stopped around the same date, so the current target needs
  confirming.
- IPv6 **is** filtered in practice: 22 % of real traffic, and the dual-stack
  listener served ~22.4 M queries under load.
- RA advertises `dns=2a02:2f04:530a:3800::1`, a prefix that exists nowhere on the
  router (expired DIGI delegation). Harmless **only because the dstnat masks
  it** — delete that dstnat believing it dead and IPv6 DNS drops.
- `/ipv6/firewall/filter` chain=input has **no final drop**: 8 accepts then
  implicit accept, while the router holds a global address. Router services are
  reachable from the internet over IPv6. A general input drop was deliberately
  not proposed without seeing the WireGuard config.

Pattern behind all of it: globals from an expired DIGI delegation hardcoded in
several places. Prefer link-local or pool-derived addresses on this router.

**The delegation rotates, so no rule may hardcode a global v6 prefix.** Four
distinct `/56`s observed inside a week, one of them a change during a single
working session: `2a02:2f04:5100:e700`, `…5303:6800`, `…520a:3d00`,
`…540c:7900`. The WAN address moves with it, and DIGI advertises both with a
`never` (infinite) lifetime while replacing them — which is what leaves clients
holding addresses from prefixes that no longer route.

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

## Logging — what is instrumented, and three traps

`/log print` reads the **memory** buffer only. A topic routed to a disk action
is invisible there and looks like it is not logging at all.

Topics go to a `netlog` action on the Kingston (`kingston/net-log.*.txt`,
8 × 5000 lines) rather than to memory or internal flash: memory is 500 lines
shared with the `[CONTAINER]` topic that carries FastAdHunter's own output, and
internal flash means wear.

- **`topics=dhcp` matches `dhcp,debug,packet` too**, and one LAN client's lease
  renewal is ~20 lines of option dumps. The rule is `dhcp,!debug,!packet`;
  without the negations the DHCPv6 prefix events are buried within hours.
- `warning` is logged twice on purpose — once to `netlog` for retention, once to
  `memory` so `/log print where message~"…"` works.
- Netwatch (`v6-global`, `v4-global`) fires its scripts on **transition only**;
  verified by an entry sitting `down` across probe intervals without emitting a
  second line. The disk action can drop an identical message repeated inside the
  same second, so treat that file as detection, not as a complete audit trail.
