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

> **A `manifest.json` in the listing does not mean legacy.** buildx writes an
> OCI archive that *also* carries `manifest.json` for `docker load`
> compatibility — but its `Layers` are `blobs/sha256/<hash>` paths, which is
> exactly the indirection RouterOS cannot follow. Judge by where the layers
> live, not by which files are present:
>
> ```sh
> tar -xOf fastadhunter-arm64.tar manifest.json
> ```
>
> `"Layers":["blobs/sha256/…"]` → still OCI, convert. `"Layers":["<hash>.tar"]`
> → genuinely legacy.

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

**The deploy copy:**

```sh
scp fastadhunter-arm64-<ver>.tar bobdenaut:kingston/
```

That is the whole command — no port, no username, no key path. They live in
the build host's `~/.ssh/config`, which is also what WinSCP reads:

```text
Host bobdenaut rb5009
    HostName 192.168.10.1
    Port 2202              # RouterOS SSH is NOT on 22 here
    User bobdenaut
    IdentityFile ~/.ssh/fah_rb5009
    IdentitiesOnly yes
```

### One-time key setup

RouterOS accepts ed25519. Generate a **dedicated deploy key** rather than
reusing a personal one, so revoking it later costs nothing:

```sh
ssh-keygen -t ed25519 -f ~/.ssh/fah_rb5009 -N "" -C "fah-deploy@$(hostname)"
```

Upload the **public** half — the only time a password is typed — then import
it. Note the **trailing colon**: without it `scp` reads
`user@host` as a local filename and silently makes a local copy instead of
transferring anything:

```sh
scp -P 2202 ~/.ssh/fah_rb5009.pub bobdenaut@192.168.10.1:
```

```routeros
/user/ssh-keys/import public-key-file=fah_rb5009.pub user=bobdenaut
/user/ssh-keys/print
```

Two things that make this look broken when it is not:

- **A password prompt after the import means the key was not offered**, not
  that it was rejected. `fah_rb5009` is not one of the default names OpenSSH
  tries, so a raw `scp -P 2202 … bobdenaut@192.168.10.1:` falls back to
  password. Use the config alias, or pass `-i` explicitly.
- On Windows, `ssh.exe` checks the private key's **ACLs**, not POSIX bits, and
  refuses a world-readable key with *UNPROTECTED PRIVATE KEY FILE*:

  ```powershell
  icacls $env:USERPROFILE\.ssh\fah_rb5009 /inheritance:r /grant:r "$env:USERNAME:(R)"
  ```

Check the SSH port and that the input chain admits LAN traffic before assuming
a firewall problem — on a stock chain ending in
`drop in-interface-list=!LAN`, LAN traffic already falls through to the
implicit accept and **no rule needs adding**
([read the chain first](../CLAUDE.md)):

```routeros
/ip/service/print
/ip/firewall/filter/print where chain=input
```

### Alternatives

If the tarball is reachable over HTTP, pull it from RouterOS instead:

```routeros
/tool/fetch url="http://<build-host>:8000/fastadhunter-arm64.tar" \
  dst-path=kingston/fastadhunter-arm64.tar
```

An SMB share of the SSD (`/disk` `smb-sharing=yes`) also works and was the
original path here, but **it is not recommended**: the share exposes
`kingston/fastadhunter/config/`, which holds the API key and the API server's
TLS **private key** — and from Phase 3, the interception CA's private key.
`scp` needs no such exposure. Turning it off requires the container stopped,
since RouterOS must unmount the filesystem to change the flag:

```routeros
/container/stop [find comment="fastadhunter"]
/disk/set [find slot=kingston] smb-sharing=no media-sharing=no
/container/start [find comment="fastadhunter"]
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
  file=kingston/fastadhunter-arm64-0.2.4.tar \
  interface=veth2 \
  root-dir=kingston/fastadhunter/root \
  mounts=fah-config,fah-data \
  logging=yes \
  start-on-boot=yes \
  comment="fastadhunter"
```

**Keep the comment exactly `fastadhunter`, with no version in it.** Every
`[find comment="fastadhunter"]` below — and in §7 and §8 — uses `=`, which in
RouterOS is exact equality, not a substring match. A comment of
`"fastadhunter 0.2.4"` makes all of them match nothing and fail *silently*: the
start, the watchdog's stop/start pair, and the rollback all become no-ops. The
version is already recorded where it cannot drift — RouterOS derives `name=`
from the tarball filename and `repo=` from the image tag, so
`/container/print detail` shows `name="fastadhunter-arm64-0.2.4.tar"` and
`repo="docker.io/library/fastadhunter:0.2.4"` on its own. (Use `~` instead of
`=` only if you have inherited a container whose comment already carries a
version.)

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

### Replacing an existing resolver — keep both, switch with a script

If the router already runs another DNS filter (AdGuard Home, Pi-hole) and
redirects `:53` to it, clients never see the container's address: the
redirect decides which resolver answers. Cutover is then a set of value
changes, not a DHCP change — and the old resolver stays running as a
one-command rollback.

Find every rule that names the current resolver before touching anything;
there are usually more than expected:

```routeros
/ip/firewall/nat/print detail where dst-port=53
/ip/firewall/filter/print detail where dst-port=53
/ip/dns/print
```

A typical setup has six: two `dstnat` redirects, two `srcnat` accepts that
exempt VPN clients from masquerade (these match the *post-dstnat* address,
which is why they break if only the redirects are changed), and two `forward`
accepts. Save one script per direction:

```routeros
/system/script/add name=dns-fah source={
  /ip/firewall/filter/set [find comment~"DNS filter redirect"] dst-address=172.17.0.3
  /ip/firewall/nat/set [find comment="Redirect catre DNS filter(docker)"] to-addresses=172.17.0.3
  /ip/firewall/nat/set [find comment~"No NAT WG DNS"] dst-address=172.17.0.3
  /ip/dns/set servers=172.17.0.3
  /ip/dns/cache/flush
}

/system/script/add name=dns-adguard source={
  /ip/firewall/nat/set [find comment="Redirect catre DNS filter(docker)"] to-addresses=172.17.0.2
  /ip/firewall/nat/set [find comment~"No NAT WG DNS"] dst-address=172.17.0.2
  /ip/firewall/filter/set [find comment~"DNS filter redirect"] dst-address=172.17.0.2
  /ip/dns/set servers=172.17.0.2
  /ip/dns/cache/flush
}
```

Match on comments rather than rule numbers — numbers shift when rules are
added. Adjust the comment strings to whatever the existing rules use.

**The two scripts order their statements differently on purpose.** Switching
*to* FastAdHunter opens the forward path before the redirect starts using it;
switching *back* stops the redirect before the path closes. Reverse either one
and traffic is briefly forwarded to an address the filter chain has not
accepted yet — which a default `drop` at the end of `forward` will swallow.

Once FastAdHunter serves production, every redeploy becomes:

```routeros
/system/script/run dns-adguard
/system/scheduler/disable fah-liveness
# ... container remove / add / start (§4) ...
/system/scheduler/enable fah-liveness
/system/script/run dns-fah
```

Disabling the scheduler is not optional — the watchdog (§7) restarts the
container on two failed health checks and will fight a deploy in progress.

**Both bracketing steps are conditional, and a redeploy is the wrong moment to
discover that.** Check first:

```routeros
/system/scheduler/print      # is fah-liveness actually there?
/container/print detail      # is the old resolver actually running?
```

If `/system/scheduler/print` is empty, §7 was never applied — there is no
watchdog to fight, so drop both scheduler lines (and consider adding §7 *after*
the redeploy, never before, since a restarter plus a `container/remove` is a
race).

If the old resolver shows `status=stopped`, `dns-adguard` is worse than
skipping it: it aims the redirect *and* `/ip/dns` at an address nothing answers,
so DNS breaks either way and now you must remember to run `dns-fah` to get back.
Skipping leaves the redirect already on FastAdHunter's address, so the LAN
recovers by itself the moment the new container starts. The cost is a DNS gap
for the length of the `extracting` phase — minutes on RB5009, largely absorbed
by client resolver caches on a household LAN.

> **Do not replace the redirect with `servers=172.17.0.3,172.17.0.2` and let
> RouterOS fail over on its own.** It works, and it is tempting because the
> failover is automatic — but then every query reaches FastAdHunter from the
> router's address instead of the client's. Per-client statistics and the
> `$client` rules the parser already classifies as `ClientScoped` (Phase 2,
> Policies) become impossible to apply. The redirect is what preserves client
> identity.

### IPv6 — establish where it resolves before trusting any measurement

FastAdHunter binds `[dns.listen] address`, **`::` by default** — one dual-stack
socket that accepts IPv4 and IPv6 alike (`IPV6_V6ONLY` is turned off
explicitly, and v4 clients are still reported canonically rather than as
`::ffff:…` mapped addresses). So the listener is not the constraint it was
before; earlier builds bound `0.0.0.0` and could not answer IPv6 at all.

**Steering is a separate question, and on this deployment IPv6 does reach
FastAdHunter.** The DHCP setting and the `dstnat` redirect above cover IPv4
only, so it is tempting to conclude IPv6 resolves elsewhere. Measured on the
live router, that conclusion is false: `GET /api/v1/clients` shows **19 IPv6
clients against 13 IPv4**, and excluding synthetic load generators the IPv6
clients account for 12,953 of 59,177 household queries in 24 h — 22 % of real
traffic — filtered at a 55 % block rate. IPv6 clients also reach the TCP
listener (client disconnects in the router log carry `2a02:…` source
addresses).

Measurements confirm that IPv6 DNS traffic reaches FastAdHunter on this
deployment. The exact steering mechanism (for example, IPv6 NAT, Router
Advertisement configuration, or another routing mechanism) should be verified
explicitly rather than assumed.

So do not assume either answer. The commands below are how you find out which.
The point of this section is unchanged: **establish where IPv6 resolves before
trusting any measurement**, because if part of it bypasses FastAdHunter then the
§6 checks and the §9 soak silently describe only a fraction of the LAN.

That is not a deployment fault and this guide does not prescribe a firewall for
it. What deployment needs is the answer to one question: **do IPv6 queries
reach FastAdHunter, another resolver, or nothing?** Each answer changes what
the §6 checks and the §9 soak actually prove. Read all five before concluding
anything — the answer is never in one of them alone:

```routeros
/ipv6/nd/print detail          # is RA advertising a DNS server, and which?
/ipv6/dhcp-server/print
/ipv6/dhcp-server/option/print
/ipv6/address/print            # does the advertised address exist on this router?
/ipv6/firewall/nat/print       # is IPv6 :53 redirected somewhere first?
```

A real deployment produced a combination no single command reveals: RA
advertising `dns=` an address belonging to an **expired prefix delegation**,
*and* a `dstnat` capturing every IPv6 `:53` from the bridge and sending it to a
container address that no longer answered. The redirect was masking the stale
RA — clients queried a dead address, the router hijacked the packet anyway, and
IPv6 resolution worked or failed entirely on the redirect's target.

Three things that guide made harder than it needed to be:

- **A rule comment names whoever was there when it was written.** Resolve the
  target instead: `/ipv6/neighbor/print where address=<target>` plus a `ping`.
  `status="failed"` and 100% loss mean nothing answers there, whatever the
  comment claims.
- **Reading one table tells you nothing.** A `filter` chain with no `:53` drop
  looks like an open path to external resolvers until `nat` shows a redirect
  catching that traffic first. Symmetrically, a redirect proves nothing until
  its target is verified alive.
- **Hardcoded global addresses expire.** Where a prefix comes from a dynamic
  delegation, an address literal written into `dns=`, a `dstnat` target, or a
  static interface address survives the delegation that made it valid. Prefer
  link-local or pool-derived addresses; when a literal is unavoidable, expect
  to re-verify it after any prefix change.

If IPv6 resolves somewhere other than FastAdHunter, the soak measures IPv4
traffic only. Record that in the completion note — it biases QPS and cache-hit
figures downward against PERFORMANCE.md budgets.

Serving DNS *over* IPv6 is supported: set `[dns.listen] address = "::"` and
one dual-stack socket serves both stacks (CONFIGURATION.md — `IPV6_V6ONLY`
is cleared explicitly, so this does not depend on the host's `bindv6only`
sysctl; IPv4 clients keep their plain addresses in stats and the query log).
The cutover is then:

1. `/interface/veth/print detail` — confirm the veth's IPv6 address (the
   reference deployment: `2a02:2f04:5008:bb00::11/64` on veth2).
2. Edit `/config/fastadhunter.toml`: `[dns.listen] address = "::"`, restart
   the container, and verify both binds from a LAN client:
   `nslookup example.com <veth-IPv4>` and `nslookup example.com <veth-IPv6>`.
3. Retarget whatever steers IPv6 `:53` (the reference deployment: two
   `/ipv6/firewall/nat` dstnat rules — move `to-address` to the veth's IPv6).
4. Watch the dstnat counters and `GET /api/v1/queries`: IPv6-sourced clients
   appear under their own IPv6 addresses.

Confirm first whether the veth address is globally routable, since nothing
NATs in front of it — LAN-side firewalling is the operator's, not the
guide's.

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
