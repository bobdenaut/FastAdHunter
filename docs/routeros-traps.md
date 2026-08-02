# RouterOS / RB5009 — operational facts and traps

Things about the deployment target that cost time to learn and are recorded
nowhere else. Not a deploy guide — that is
[deploy-rb5009.md](deploy-rb5009.md). Every line here is something that was
wrong once.

> **Agents never change anything on this router.** Read-only queries are fine;
> every write is proposed and the owner runs it. See the Working agreement in
> the root [CLAUDE.md](../CLAUDE.md).

## Topology

| Interface | Address | What |
| --- | --- | --- |
| `veth1` | `172.17.0.2` | AdGuard Home — **stopped since ~2026-07-19**, resolves nothing |
| `veth2` | `172.17.0.3` | FastAdHunter, DNS on 53, API on 8443 |

FastAdHunter has been the LAN's **only** resolver since AdGuard stopped, so
household RSS readings are soak-grade evidence. Rollback to AdGuard is no longer
one command — the container must be started first, and its existence verified.

## Container configuration

| Field | Verdict |
| --- | --- |
| `comment` | **exactly `fastadhunter`**, no version — `[find comment="fastadhunter"]` is exact equality and breaks if a version is appended |
| `DNS` | **no-op trap** — accepted and displayed, never written to `/etc/resolv.conf` |
| `Tmpfs` | harmful — `/data` on tmpfs loses cache mtimes, which brings back the double compile at boot |
| `Auto Restart Interval` | **empty = nothing restarts FAH if it dies**; `Start On Boot` only covers router boot |
| `Workdir` | `/home/nonroot`, inherited from the `:nonroot` base, harmless (all FAH paths are absolute) |
| `Entrypoint`/`Cmd` | only real use is `--healthcheck` to debug a bricked config — there is no shell in distroless |
| `memory-high=200M` | **KILLS FAH.** Do not propose it as a "safe falsifiable check"; it took down the live resolver once |

**A named mount is not a working mount.** RouterOS accepts `mounts=` and reports
them in `/container/print detail` without guaranteeing the container writes
through them. A silently ineffective mount looks healthy and loses every write on
`container/remove` — which once looked exactly like a list-persistence code bug.
When on-device state seems not to persist, check the *instance*, not the code:
`/container/mounts/print detail` cross-checked against the running container's
`mounts=`.

## Commands that are not where you expect

- **`/disk/print` is top-level** — there is no `/system/disk`.
- `/system/resource/print`'s `free-hdd-space` is the internal NAND, not the
  kingston SSD.
- **`/system/resource/print` uptime is the ROUTER's, not the container's.** The
  container log is authoritative for FAH's start time.
- **There is no `/system/scheduler`** on this box — the `fah-liveness` script in
  the deploy doc was never applied. Production runs with no liveness net; the
  upside is that continuous `uptime_seconds` is direct proof of process life.
- **QPS is `/interface monitor-traffic veth1` → `tx-packets-per-second`**
  (`tx` = into the container). Firewall counters also work but this is the
  answer.

## Build and deploy pipeline

RouterOS has no `docker` CLI — never `docker load`.

1. `docker buildx build --platform linux/arm64 … -o type=docker,dest=fah-raw-<ver>.tar`
   emits **OCI layout**, which RouterOS cannot import.
2. Convert with skopeo to a legacy **docker-archive** (flat, ~12 MB).
3. Copy the tar (see the deploy-copy note in agent memory for the scp target).
4. `/container` add reusing the existing settings: `interface=veth2`,
   `root-dir=kingston/fastadhunter/root`, `mounts=fah-config,fah-data`,
   `workdir=/home/nonroot`, `start-on-boot=yes`.

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
- The query-log endpoint is `/api/v1/queries` — there is no `querylog`.

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
