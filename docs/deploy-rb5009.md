# Deploying to RB5009 (RouterOS Container)

End-to-end deployment of FastAdHunter as a RouterOS container on a MikroTik
RB5009, from image build to LAN-wide DNS filtering, plus the soak procedure
that validates the [PERFORMANCE.md](../PERFORMANCE.md) budgets on-device.

**Target topology used throughout:**

| Thing | Value |
| ----- | ----- |
| LAN bridge / subnet | `bridge` · `192.168.10.0/24`, router at `192.168.10.1` |
| Container bridge | `containers` · `172.17.0.0/24`, router at `172.17.0.1` |
| Container address | `172.17.0.3` |
| DNS service | `172.17.0.3:53` (UDP + TCP) |
| API | `https://172.17.0.3:8443` |
| Persistent storage | external USB SSD, mounted by RouterOS as `kingston` |

Substitute your own addresses consistently; nothing below depends on these
specific values.

## 0. Prerequisites

- RouterOS 7.x with the **`container` package** installed and
  `/system/device-mode` container support enabled. Enabling device-mode
  requires a physical power-cycle or reset-button press within the confirmation
  window — do this before anything else, it needs physical access to the
  router:

  ```routeros
  /system/device-mode/update container=yes
  # then power-cycle / press reset when prompted, and re-check:
  /system/device-mode/print
  ```

- An **external USB SSD** formatted and mounted. RB5009 internal flash is too
  small and too write-limited for query-log segments. Verify:

  ```routeros
  /disk/print
  ```

  Expect a `kingston` (or similar) slot. The rest of this guide writes to
  `kingston/fastadhunter/`.

- A build host with Docker + `buildx`. Cross-building `arm64` on an `amd64`
  host needs QEMU registered (Docker Desktop ships it; on plain Linux run
  `docker run --privileged --rm tonistiigi/binfmt --install all`).

> **Do not skip the SSD.** With `/data` on internal flash, query-log flushes
> (default every 5 s) will wear it out and the 500 MB retention cap will not
> fit.

## 1. Build the arm64 image

RouterOS imports the **legacy docker-archive** layout: layer tarballs named
`<hash>.tar` at the top level, with `manifest.json` referencing them by that
relative path. Two other formats look plausible and both fail:

- `-o type=tar` — a flat filesystem tarball, not an image. Rejected outright.
- **OCI layout** — `oci-layout` + `index.json` + `blobs/sha256/…`. RouterOS
  cannot follow the `blobs/` indirection and **hangs at `status=extracting`
  indefinitely**, with `OS` and `Arch` left blank. It does not report an error.

Build:

```sh
docker buildx build \
  --platform linux/arm64 \
  -t fastadhunter:0.1.0 \
  -o type=docker,dest=fastadhunter-arm64.tar \
  .
```

**Then check which layout you actually got** — this is not optional. On Docker
with the containerd image store enabled (the default on recent Docker Desktop),
both `buildx -o type=docker` *and* `docker save` emit OCI layout:

```sh
tar -tf fastadhunter-arm64.tar | head -5
```

- Flat `<hash>.tar` entries → legacy layout, ready to upload.
- `blobs/`, `oci-layout`, `index.json` → OCI layout, **convert first**:

```sh
docker run --rm -v "$PWD:/work" quay.io/skopeo/stable copy --insecure-policy \
  oci-archive:/work/fastadhunter-arm64.tar \
  docker-archive:/work/fastadhunter-rosready.tar:fastadhunter:0.1.0
```

Upload the converted file. It is larger (legacy layers are uncompressed —
roughly 12 MB versus 4.7 MB here), which is still inside the ≤ 30 MB budget:

```sh
ls -lh fastadhunter-rosready.tar
```

Alternatively, turning off the containerd image store in Docker Desktop's
settings makes `docker save` emit the legacy layout directly, with no
conversion step.

### Alternative: pull from a registry instead of uploading a tarball

```sh
docker buildx build --platform linux/arm64 \
  -t <registry>/fastadhunter:0.1.0 --push .
```

Then on the router, set `registry-url` and use `remote-image=` instead of
`file=` in step 4. The tarball route avoids needing the router to reach a
registry and is the recommended path for a first deployment.

## 2. Upload the tarball to the router

From the build host:

```sh
scp fastadhunter-arm64.tar admin@192.168.10.1:kingston/
```

Or, if the tarball is reachable over HTTP, from RouterOS:

```routeros
/tool/fetch url="http://<build-host>:8000/fastadhunter-arm64.tar" \
  dst-path=kingston/fastadhunter-arm64.tar
```

## 3. Network and storage setup

### 3.1 Container network

```routeros
/interface/veth/add name=veth2 \
  address=172.17.0.3/24,<your-v6-prefix>::11/64 \
  gateway=172.17.0.1 gateway6=<your-v6-prefix>::1

/interface/bridge/add name=containers
/interface/bridge/port/add bridge=containers interface=veth2
/ip/address/add address=172.17.0.1/24 interface=containers
```

> **If the LAN has IPv6, configure it on the veth — or omit IPv6 entirely.**
> A veth with only IPv4 still picks up an IPv6 address from RA on the bridge,
> but has no route out. The container then resolves a dual-stack host, gets an
> AAAA, tries IPv6, and fails after ~2 s. The symptom is maddening: DNS
> forwarding works perfectly (upstreams are IP literals), IPv4-only list hosts
> download fine, and only list URLs whose host has an AAAA record fail —
> leaving `rules=0` and an ad blocker that silently blocks nothing.
> Confirmed example: `raw.githubusercontent.com` (IPv4-only) downloads;
> `small.oisd.nl` (has AAAA) fails, on the same container, in the same second.

> **One veth per container.** If the router already runs another container
> (a previous DNS blocker, for instance), it owns its veth and you cannot
> share it — give FastAdHunter its own veth on a free address in the same
> subnet. Check what exists first:
>
> ```routeros
> /container/print detail
> /interface/veth/print detail
> /interface/bridge/port/print
> ```
>
> Two resolvers can then run side by side, each binding `:53` on its own
> address, with DHCP deciding which one the LAN actually uses. That makes the
> old one a ready-made rollback target (§8) — but stop it before the soak, or
> its RSS competes for the same shared 1 GB and the memory numbers mean
> nothing.

### 3.2 Firewall — change nothing up front

On a stock RouterOS firewall **no rule changes are needed**, and you should not
add any speculatively:

- **Egress** (rule-list downloads, upstream DNS) is normally already covered by
  the existing blanket `chain=srcnat action=masquerade out-interface=<WAN>`
  rule, which matches any source address including `172.17.0.0/24`.
- **LAN → container** is traffic between two local bridges. The default forward
  chain drops traffic *from WAN that was not dst-natted*; it does not block
  inter-bridge local traffic.
- **WAN → container** is already blocked by that same default drop, and no
  port-forward points at the container. SECURITY.md's "never expose 53 or 8443
  to the WAN" is satisfied by not creating one.

Verify empirically instead, after the container is running:

| Symptom | Rule to add — only if it actually fails |
| ------- | --------------------------------------- |
| Ruleset stays at 0 rules, lists never download | `/ip/firewall/nat/add chain=srcnat action=masquerade src-address=172.17.0.0/24` |
| LAN clients time out querying `172.17.0.3:53` | `/ip/firewall/filter/add chain=forward action=accept src-address=<LAN>/24 dst-address=172.17.0.3 protocol=udp dst-port=53` (and the same for `tcp`) |

> **Never paste a filter rule in blind.** `/ip/firewall/filter/add` appends to
> the end of the chain — behind any final drop, where it has no effect. Rules
> are evaluated in order, so a rule that is genuinely needed must be *placed*
> (`place-before=<id>`), and placement depends on the chain you already have.
> Read the chain first, decide the position, then add.

Before assuming a firewall cause, check the chain you actually have:

```routeros
/ip/firewall/filter/print where chain=forward
/ip/firewall/nat/print where chain=srcnat
```

If your forward chain ends in `action=accept` rather than a drop, that is a
pre-existing property of your firewall, not something this deployment
introduces — but it does mean the container is only as protected as the rest of
your LAN.

### 3.3 Storage

```routeros
/container/config/set tmpdir=kingston/fastadhunter/tmp \
  ram-high=256M

/container/mounts/add name=fah-config \
  src=kingston/fastadhunter/config dst=/config
/container/mounts/add name=fah-data \
  src=kingston/fastadhunter/data   dst=/data
```

Confirm both attached — `/container/print detail` must list them.

**No `resolv.conf` mount is needed.** Earlier builds required one: RouterOS
accepts a `dns=` setting on the container and shows it in
`/container/print detail`, but does **not** write it into `/etc/resolv.conf`,
which the distroless image ships as a 0-byte file. With no nameserver, every
list download failed after ~5 s with a useless `error sending request for url`,
while DNS *forwarding* kept working (upstreams are IP literals) — so the
container looked healthy while downloading nothing.

FastAdHunter now resolves its own list sources through the servers in
`[[dns.upstreams.servers]]` and never consults `/etc/resolv.conf`
(ARCHITECTURE.md §Dependency Layering → Ports). If you are upgrading and still
have the mount, drop it:

```routeros
/container/mounts/remove [find name=fah-resolv]
```

`ram-high=256M` mirrors the PERFORMANCE.md hard ceiling — the container is
throttled rather than allowed to starve RouterOS of the shared 1 GB.

`/config` is small and worth backing up (TOML, API key, TLS cert). `/data` is
bulky and fully regenerable (cached lists, query-log segments, stats
snapshots) — see [CONFIGURATION.md](../CONFIGURATION.md) §Volumes.

## 4. Create and start the container

```routeros
/container/add \
  file=kingston/fastadhunter-arm64.tar \
  interface=veth2 \
  root-dir=kingston/fastadhunter/root \
  mounts=fah-config,fah-data \
  logging=yes \
  start-on-boot=yes \
  comment="fastadhunter"
```

Importing the tarball takes a while on RB5009 hardware. Wait for the status to
leave `extracting`:

```routeros
/container/print detail
```

Then start it:

```routeros
/container/start [find comment="fastadhunter"]
/container/print detail
```

Expect `status=running`.

### First-boot output

With `logging=yes`, the container's stdout lands in RouterOS's log:

```routeros
/log/print where topics~"container"
```

> **Set `logging=yes` explicitly.** Ticking the box in WinBox's Container
> dialog does not always persist — check `/container/print detail` for
> `logging=yes` and set it from the CLI if absent
> (`/container/set [find comment="fastadhunter"] logging=yes`). Without it you
> get RouterOS runtime lines but none of the application's stdout, which is
> where every first-boot diagnosis lives — including the one-time API key.

On an empty `/config` FastAdHunter generates its config, API key and
self-signed TLS certificate ([CONFIGURATION.md](../CONFIGURATION.md) §First
boot). **The API key is printed exactly once** — copy it now:

```text
generated API key — store it now; it is not shown again
```

If you miss it, the key file is at `kingston/fastadhunter/config/apikey` on the
SSD; rotate it via `POST /api/v1/config/apikey/rotate` if it may have been
exposed in the log.

### Port 53 needs a redirect — this is required, not optional

**Confirmed on RouterOS 7 / RB5009:** the container cannot bind port 53
directly. RouterOS honours the image's `USER nonroot` (uid 65532) and does
**not** set `net.ipv4.ip_unprivileged_port_start=0` the way Docker does, so
binding a port below 1024 fails with `EACCES`. The container starts, compiles
its ruleset, then exits 1:

```text
INFO  fastadhunter starting config_path=/config/fastadhunter.toml
INFO  ruleset compiled from cache rules=0
ERROR fastadhunter failed to start error=Permission denied (os error 13)
```

Note what this log rules out: `/config` and `/data` are both writable, or
those first two lines would not appear. A permission error here is the port,
not the mounts.

RouterOS exposes no `cap-add`, so `CAP_NET_BIND_SERVICE` is unavailable, and
running the container as root contradicts SECURITY.md. Move the listener to an
unprivileged port and redirect instead:

1. Stop the container.
2. Add to the container's `Envlist` (cleaner than editing the TOML on the SSD):

   ```text
   FAH__DNS__LISTEN__PORT=5353
   ```

   Env overrides beat the file per CONFIGURATION.md §Precedence.
   `dns.listen.port` is boot-class, so this takes effect on restart.
3. Redirect 53 → 5353 on the router so clients still use the standard port:

   ```routeros
   /ip/firewall/nat/add chain=dstnat action=dst-nat \
     dst-address=172.17.0.3 protocol=udp dst-port=53 to-ports=5353 \
     comment="fastadhunter dns udp"
   /ip/firewall/nat/add chain=dstnat action=dst-nat \
     dst-address=172.17.0.3 protocol=tcp dst-port=53 to-ports=5353 \
     comment="fastadhunter dns tcp"
   ```

4. Start the container again.

Record which path was needed in the completion note — if the workaround is
required, that is a deployment fact worth an issue against the image.

## 5. Point the LAN at FastAdHunter

Hand the container's address to clients via DHCP:

```routeros
/ip/dhcp-server/network/set [find address=192.168.10.0/24] \
  dns-server=172.17.0.3
```

Clients pick this up on their next lease renewal; force it by reconnecting, or
shorten the lease time temporarily.

> RouterOS's own DNS service (`/ip/dns`) is unaffected — it lives on the
> router's LAN address, not on `172.17.0.3`, so the two do not collide. Leave
> it configured as a fallback you can revert to (see §8).

### IPv6 bypass — check this before believing the deployment failed

FastAdHunter listens on IPv4 only: `[dns.listen] address` defaults to
`0.0.0.0`, which is the IPv4 wildcard and does **not** accept IPv6. The DHCP
setting above therefore only steers IPv4 resolution.

If the LAN also runs IPv6 and advertises DNS servers over RA or DHCPv6, dual
-stack clients will prefer those and resolve **without ever reaching
FastAdHunter**. Every check in §6 passes, `/api/v1/stats` shows traffic from
the hosts that are IPv4-only, and ads still appear on the phone. Nothing is
broken — the queries simply never arrive.

```routeros
/ipv6/nd/print detail          # is RA advertising DNS servers?
/ipv6/dhcp-server/print
```

Options, in order of preference:

1. Stop advertising IPv6 DNS servers, so clients fall back to the IPv4
   resolver they were handed. Simplest, and keeps IPv6 connectivity intact.
2. Disable IPv6 on the LAN for the duration of the soak. Blunt, but removes
   the variable entirely while validating budgets.
3. Leave it, and accept that soak numbers cover IPv4 traffic only — record
   this in the completion note, because it biases QPS and cache-hit figures
   downward against PERFORMANCE.md budgets.

Serving DNS *over* IPv6 (binding `::`) is not in Phase 1 scope. Note that the
container's veth may hold a globally routable address, so binding `::` would
publish the listener to the internet with no NAT in front of it — that needs
a firewall review first, not just a config edit.

## 6. On-device verification checklist

Work through these in order; each one is a gate for the next.

| # | Check | Command | Expected |
| - | ----- | ------- | -------- |
| 1 | Container running | `/container/print detail` | `status=running` |
| 2 | First boot wrote to the SSD | `/file/print where name~"fastadhunter/config"` | `fastadhunter.toml`, `apikey`, `api-cert.pem`, `api-key.pem` |
| 3 | Ruleset compiled | `/log/print where message~"ruleset compiled"` | non-zero rule count |
| 4 | Listeners bound | `/log/print where message~"DNS listeners bound"` | udp + tcp addresses |
| 5 | Resolves from the router | `/tool/dns-lookup name=example.com server=172.17.0.3` | an address |
| 6 | Resolves from a LAN client | `nslookup example.com 172.17.0.3` | an address |
| 7 | Known ad domain blocked | `nslookup doubleclick.net 172.17.0.3` | `0.0.0.0` (default `null_ip` blocking mode) |
| 8 | Health endpoint | see below | `200` |
| 9 | Stats show real counters | see below | non-zero `total`/`blocked` |
| 10 | Metrics scrape | see below | Prometheus text |
| 11 | Phone browses with ads blocked | manual | ad slots empty |

From a LAN client (`--insecure` because the certificate is self-signed —
export the public cert via the API if you want to pin it):

```sh
# 8 — health (no key needed; api.metrics_public defaults to true)
curl --insecure https://172.17.0.3:8443/health

# 9 — stats
curl --insecure -H "Authorization: Bearer $FAH_KEY" \
  https://172.17.0.3:8443/api/v1/stats

# 10 — metrics
curl --insecure https://172.17.0.3:8443/metrics | head -40

# recent queries, to confirm the block in check 7 was logged
curl --insecure -H "Authorization: Bearer $FAH_KEY" \
  "https://172.17.0.3:8443/api/v1/queries?limit=20"
```

Endpoint shapes are in [API.md](../API.md).

## 7. Liveness on RouterOS

**RouterOS ignores the image's `HEALTHCHECK`.** Its container runtime does not
implement Docker healthcheck semantics, so the `--healthcheck` self-probe baked
into the image never runs on-device. Liveness needs a RouterOS-side poll.

This scheduler script restarts the container if `/health` stops answering
twice in a row:

```routeros
/system/scheduler/add name=fah-liveness interval=1m on-event={
  :global fahFails
  :if ([:typeof $fahFails] = "nothing") do={ :set fahFails 0 }
  :do {
    /tool/fetch url="https://172.17.0.3:8443/health" check-certificate=no \
      output=none as-value
    :set fahFails 0
  } on-error={
    :set fahFails ($fahFails + 1)
    :if ($fahFails >= 2) do={
      :log warning "fastadhunter unhealthy - restarting container"
      /container/stop [find comment="fastadhunter"]
      :delay 5s
      /container/start [find comment="fastadhunter"]
      :set fahFails 0
    }
  }
}
```

Restarts logged by this script count as watchdog restarts for the soak
acceptance criteria — the target is **zero**.

## 8. Rollback

Fastest path back to a working LAN, in order of escalation:

1. **Revert DNS only** (LAN keeps working, filtering off):

   ```routeros
   /ip/dhcp-server/network/set [find address=192.168.10.0/24] \
     dns-server=192.168.10.1
   ```

2. **Stop the container:**

   ```routeros
   /system/scheduler/disable fah-liveness
   /container/stop [find comment="fastadhunter"]
   ```

3. **Remove it entirely** (keeps `/config` + `/data` on the SSD, so a
   re-add resumes with the same key, cert and cached lists):

   ```routeros
   /container/remove [find comment="fastadhunter"]
   ```

4. **Full reset** — additionally delete `kingston/fastadhunter/config` and
   `kingston/fastadhunter/data`. The next start behaves as a first boot and
   prints a **new** API key.

To downgrade rather than remove: keep the previous tarball on the SSD, remove
the container, and re-add with `file=` pointing at the old image. `/config` and
`/data` survive, so the rollback is a container swap, not a data migration.

## 9. 24-hour soak

**Goal:** real household traffic for ≥ 24 h with no crashes, no watchdog
restarts, and RSS ≤ 128 MB steady-state ([PERFORMANCE.md](../PERFORMANCE.md)
§Budgets).

### Collection

Sample `/metrics` every 5 minutes from any always-on LAN host:

```sh
mkdir -p soak
while true; do
  ts=$(date -u +%Y%m%dT%H%M%SZ)
  curl -s --insecure https://172.17.0.3:8443/metrics > "soak/metrics-$ts.txt"
  curl -s --insecure -H "Authorization: Bearer $FAH_KEY" \
    https://172.17.0.3:8443/api/v1/stats > "soak/stats-$ts.json"
  sleep 300
done
```

If a Prometheus instance is available, scrape `172.17.0.3:8443/metrics`
instead — the retention and querying are worth it for a 24 h window.

Also snapshot the RouterOS view periodically, since it accounts for memory
differently than the process does:

```routeros
/container/print detail
/system/resource/print
```

### What to record

| Budget | Metric to read |
| ------ | -------------- |
| RAM steady-state ≤ 128 MB | `process_resident_memory_bytes` (max over the window) |
| RAM hard ceiling 256 MB | RouterOS `ram-high` never throttling |
| Compiled ruleset ≤ 40 MB | `fastadhunter_ruleset_heap_bytes` |
| Cache-hit / blocked p99 < 1 ms | `fastadhunter_query_duration_seconds` histogram, by `verdict` |
| Throughput | `rate(fastadhunter_queries_total[5m])`, peak |
| No dropped events | `fastadhunter_events_dropped_total` stays 0 |
| Upstream health | `fastadhunter_upstream_failures_total`, `..._consecutive_failures` |
| Cache effectiveness | `fastadhunter_cache_hits_total` / (hits + misses) |
| No crashes / restarts | RouterOS log; `fah-liveness` never fired |

Household traffic will not approach the 10 000 QPS sustained-throughput
budget — that number belongs to the load benches from
`p1-10-benches-integration`. The soak validates **steady-state memory,
stability, and latency under real query mix**, not peak throughput.

### Acceptance

- ≥ 24 h continuous uptime, `status=running` throughout.
- Zero watchdog restarts, zero crash entries in the RouterOS log.
- Peak `process_resident_memory_bytes` ≤ 128 MB, and flat — a rising trend
  across the window is a leak and fails the soak even below the cap
  (CLAUDE.md hard rule 4: memory must not grow with traffic or uptime).
- p99 in-engine latency < 1 ms for cache-hit and blocked verdicts.

Record the actual numbers in the task's completion note. Anything the device
disproves gets an issue — or an ADR in [docs/decisions/](decisions/) if it is
design-level rather than a bug.

## Troubleshooting

| Symptom | Likely cause |
| ------- | ------------ |
| `status=extracting` forever, `OS`/`Arch` blank | Tarball is OCI layout, not legacy docker-archive — convert with skopeo (§1). RouterOS reports no error, it just hangs |
| `container add` rejects the file outright | Filesystem tarball (`-o type=tar`) rather than an image archive (§1) |
| Container exits immediately, bind error in log | Port 53 as non-root — see §4 |
| Container runs, no config written to SSD | Mounts wrong or SSD not writable — check `/container/mounts/print` and `/disk/print` |
| Lists never download, ruleset stays 0 | No egress — check the masquerade rule and that the container's `gateway=172.17.0.1` matches the bridge address |
| LAN clients time out on DNS | DHCP not renewed yet, or the `forward` accept rules are missing |
| API unreachable but DNS works | `api.address` bound narrower than `0.0.0.0`, or the 8443 forward rule is missing |
| Ads still shown on the phone | Client using DoH/DoT to bypass the LAN resolver, or a hardcoded resolver — check `/api/v1/queries` for whether the domain reached FastAdHunter at all |
