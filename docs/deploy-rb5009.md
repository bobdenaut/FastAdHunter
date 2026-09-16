# Deploying to RB5009 (RouterOS Container)

Everything needed to take FastAdHunter from nothing to a filtering resolver on
a MikroTik RB5009: build, upload, network, container, configuration, the LAN
cutover, the optional HTTP and HTTPS layers, client setup, verification and
rollback. Work through it in order — each section assumes the previous ones.

Already running a deployment and only upgrading it? Use
[routeros-stepbystep-install.md](routeros-stepbystep-install.md) instead: it
covers the 0.4.0 upgrade on the reference router, with that box's current state
and real rule indices.

**Topology used throughout:**

| Thing | Value |
| ----- | ----- |
| LAN bridge / subnet | `BRIDGE` · `192.168.10.0/24`, router at `192.168.10.1` |
| Container bridge | `CONTAINERS` · `172.17.0.0/24`, router at `172.17.0.1` |
| Container address | `172.17.0.2`, `fd6c:7f32:8e91:1::2` |
| DNS service | `172.17.0.2:53` (UDP + TCP) |
| API | `https://fah-api.localbox.ro:8443` — resolves through FastAdHunter itself, so fall back to `https://172.17.0.2:8443` whenever DNS is the thing that is broken |
| Persistent storage | external USB SSD, mounted by RouterOS as `kingston` |

Interface names are **case-sensitive** on RouterOS. Substitute your own
addresses consistently; nothing below depends on these specific values.

## 0. Prerequisites

- RouterOS 7.x with the **`container` package** installed and
  `/system/device-mode` container support enabled. Enabling device-mode needs a
  physical power-cycle or reset-button press inside the confirmation window —
  do this first, it cannot be done remotely:

  ```routeros
  /system/device-mode/update container=yes
  # power-cycle / press reset when prompted, then:
  /system/device-mode/print
  ```

- An **external USB SSD**, formatted and mounted (`/disk/print` should show a
  `kingston`-like slot). RB5009 internal flash is too small and too
  write-limited for query-log segments: with `/data` on flash, the 5-second
  flushes wear it out and the 500 MB retention cap does not fit.

- A build host with Docker + `buildx`. Cross-building arm64 on amd64 needs QEMU
  registered — Docker Desktop ships it; on plain Linux run
  `docker run --privileged --rm tonistiigi/binfmt --install all`.

## 1. Build the arm64 image

RouterOS imports the **legacy docker-archive** layout: layer tarballs named
`<hash>.tar` at the top level, with `manifest.json` referencing them by that
relative path. Two other formats look plausible and both fail:

- `-o type=tar` — a flat filesystem tarball, not an image. Rejected outright.
- **OCI layout** — `oci-layout` + `index.json` + `blobs/sha256/…`. RouterOS
  cannot follow the `blobs/` indirection and **hangs at `status=extracting`
  forever**, with `OS` and `Arch` blank. It reports no error.

```sh
docker buildx build --platform linux/arm64 \
  -t fastadhunter:0.4.0 \
  -o type=docker,dest=fastadhunter-arm64-0.4.0-raw.tar .
```

**Then check which layout you got.** With the containerd image store enabled
(the default on recent Docker Desktop) both `buildx -o type=docker` *and*
`docker save` emit OCI.

A `manifest.json` in the listing does **not** mean legacy: buildx writes an OCI
archive that also carries one for `docker load` compatibility. Judge by where
the layers live:

```sh
tar -xOf fastadhunter-arm64-0.4.0-raw.tar manifest.json
```

`"Layers":["<hash>.tar"]` is legacy and ready to upload.
`"Layers":["blobs/sha256/…"]` is OCI — convert:

```sh
docker run --rm -v "$PWD:/work" quay.io/skopeo/stable copy --insecure-policy \
  oci-archive:/work/fastadhunter-arm64-0.4.0-raw.tar \
  docker-archive:/work/fastadhunter-arm64-0.4.0.tar:fastadhunter:0.4.0
```

The converted file is larger — legacy layers are uncompressed, ~16 MB against
~7 MB — and still inside the ≤ 30 MB budget. On Windows, Git Bash mangles the
`-v` paths: use a drive-letter source and `MSYS_NO_PATHCONV=1`.

Turning the containerd image store off in Docker Desktop's settings makes
`docker save` emit legacy directly, with no conversion step.

**Alternative — registry instead of a tarball.** Build with
`-t <registry>/fastadhunter:0.4.0 --push`, then set `registry-url` on the
router and use `remote-image=` instead of `file=` in §4. The tarball route
needs no registry reachable from the router and is the recommended first
deployment.

## 2. Upload the tarball

```sh
scp fastadhunter-arm64-0.4.0.tar bobdenaut:kingston/
```

That is the whole command — no port, no username, no key path. They live in the
build host's `~/.ssh/config`, which WinSCP reads too:

```text
Host bobdenaut rb5009
    HostName 192.168.10.1
    Port 2202              # RouterOS SSH is NOT on 22 here
    User bobdenaut
    IdentityFile ~/.ssh/fah_rb5009
    IdentitiesOnly yes
```

### One-time key setup

RouterOS accepts ed25519. Generate a **dedicated deploy key** so revoking it
later costs nothing:

```sh
ssh-keygen -t ed25519 -f ~/.ssh/fah_rb5009 -N "" -C "fah-deploy@$(hostname)"
scp -P 2202 ~/.ssh/fah_rb5009.pub bobdenaut@192.168.10.1:
```

Note the **trailing colon** — without it `scp` reads `user@host` as a local
filename and silently makes a local copy. Then import:

```routeros
/user/ssh-keys/import public-key-file=fah_rb5009.pub user=bobdenaut
/user/ssh-keys/print
```

Two things that look broken and are not:

- **A password prompt after the import means the key was not offered**, not
  rejected. `fah_rb5009` is not a name OpenSSH tries by default, so a raw
  `scp -P 2202 …` falls back to password. Use the config alias or pass `-i`.
- On Windows, `ssh.exe` checks the private key's **ACLs**, not POSIX bits, and
  refuses a world-readable key with *UNPROTECTED PRIVATE KEY FILE*:

  ```powershell
  icacls $env:USERPROFILE\.ssh\fah_rb5009 /inheritance:r /grant:r "$env:USERNAME:(R)"
  ```

Check `/ip/service/print` and the input chain before assuming a firewall
problem. On a stock chain ending in `drop in-interface-list=!LAN`, LAN traffic
already falls through and **no rule needs adding**.

### Alternatives

`/tool/fetch url="http://<build-host>:8000/…" dst-path=kingston/…` pulls the
tarball from RouterOS instead.

An SMB share of the SSD also works, but **it is not recommended**: the share
exposes `kingston/fastadhunter/config/`, which
holds the API key, the API server's TLS **private key** and — from Phase 3 —
the interception CA's private key. Turning it off needs the container stopped,
since RouterOS must unmount the filesystem to change the flag:

```routeros
/container/stop [find comment="fastadhunter"]
/disk/set [find slot=kingston] smb-sharing=no media-sharing=no
/container/start [find comment="fastadhunter"]
```

## 3. Network and storage

### 3.1 Container network

```routeros
/interface/veth/add name=veth1 \
  address=172.17.0.2/24,fd6c:7f32:8e91:1::2/64 \
  gateway=172.17.0.1 gateway6=fd6c:7f32:8e91:1::1 \
  comment="fastadhunter"

/interface/bridge/add name=CONTAINERS
/interface/bridge/port/add bridge=CONTAINERS interface=veth1
/ip/address/add address=172.17.0.1/24 interface=CONTAINERS
/ipv6/address/add address=fd6c:7f32:8e91:1::1/64 interface=CONTAINERS advertise=no

/interface/list/member/add list=LAN interface=CONTAINERS
```

**The `LAN` list membership is load-bearing.** Every steering rule in §5b and
§5c matches `in-interface-list=LAN`; leave `CONTAINERS` out of the list and
they all match nothing while looking perfectly correct.

> **If the LAN has IPv6, configure it on the veth — or omit IPv6 entirely.** A
> veth with only IPv4 still picks up an IPv6 address from RA on the bridge but
> has no route out. The container then resolves a dual-stack host, gets an
> AAAA, tries IPv6 and fails after ~2 s. The symptom is maddening: DNS
> forwarding works perfectly (upstreams are IP literals), IPv4-only list hosts
> download fine, and only list URLs whose host has an AAAA record fail —
> leaving `rules=0` and an ad blocker that silently blocks nothing. Confirmed:
> `raw.githubusercontent.com` downloads, `small.oisd.nl` fails, same container,
> same second.

> **One veth per container.** A router already running another container owns
> its veth and you cannot share it — give FastAdHunter its own on a free
> address in the same subnet (`/container/print detail`,
> `/interface/veth/print`, `/interface/bridge/port/print`). Two resolvers can
> then run side by side, each binding `:53` on its own address, with the
> redirect deciding which one the LAN uses. That makes the old one a ready-made
> rollback target (§8) — but stop it before a soak, or its RSS competes for the
> same shared 1 GB and the memory numbers mean nothing.

### 3.2 Firewall — change nothing up front

On a stock RouterOS firewall **no rule changes are needed** for DNS, and you
should not add any speculatively:

- **Egress** (list downloads, upstream DNS) is normally already covered by the
  blanket `chain=srcnat action=masquerade out-interface-list=WAN` rule, which
  matches any source including `172.17.0.0/24`.
- **LAN → container** is traffic between two local bridges. The default forward
  chain drops WAN traffic that was not dst-natted; it does not block local
  inter-bridge traffic.
- **WAN → container** is already blocked by that same drop, and no port-forward
  points at the container. SECURITY.md's "never expose 53 or 8443 to the WAN"
  is satisfied by not creating one.

Verify empirically instead, once the container runs:

| Symptom | Rule to add — only if it actually fails |
| ------- | --------------------------------------- |
| Ruleset stays at 0 rules, lists never download | `/ip/firewall/nat/add chain=srcnat action=masquerade src-address=172.17.0.0/24` |
| LAN clients time out on `172.17.0.2:53` | `/ip/firewall/filter/add chain=forward action=accept src-address=<LAN>/24 dst-address=172.17.0.2 protocol=udp dst-port=53` (and the same for `tcp`) |

> **Never paste a filter rule in blind.** `/ip/firewall/filter/add` appends to
> the end of the chain — behind any final drop, where it has no effect. A rule
> that is genuinely needed must be *placed* (`place-before=…`), and placement
> depends on the chain you already have. Read
> `/ip/firewall/filter/print where chain=forward` first, decide the position,
> then add.

A forward chain ending in `accept` rather than a drop is a pre-existing
property of your firewall, not something this deployment introduces — but it
does mean the container is only as protected as the rest of the LAN.

### 3.3 Storage

```routeros
/container/config/set tmpdir=/kingston/pull

/container/mounts/add list=fah-config \
  src=/kingston/fastadhunter/config dst=/config
/container/mounts/add list=fah-data \
  src=/kingston/fastadhunter/data   dst=/data
```

`tmpdir` must be on the SSD. Extracting a 16 MB image into NAND-backed storage
is how the box runs out of space.

`memory-high` (`ram-high` on older releases) throttles the container rather
than letting it starve RouterOS of the shared 1 GB. The reference deployment
runs it **unlimited** deliberately: a value set too low OOM-kills the live
resolver, and the process is already bounded by design.

`/config` is small and worth backing up (TOML, API key, TLS certificate pair).
`/data` is bulky and fully regenerable (cached lists, query-log segments, stats
snapshots) — [CONFIGURATION.md](../CONFIGURATION.md) §Volumes.

**No `resolv.conf` mount is needed.** FastAdHunter resolves its own list
sources through `[[dns.upstreams.servers]]` and never reads the file. 7.21.5
writes it once at container start — from `dns=` when set, otherwise from
whatever `/ip/dns servers` holds at that instant — and never refreshes it, so
its contents are a snapshot of that moment. Confusing to read, harmless to
have. Carrying such a mount from an older deployment, drop it:
`/container/mounts/remove [find list=fah-resolv]`.

## 4. Create and start the container

```routeros
/container/envs/add list=fah-env key="MIMALLOC_ARENA_EAGER_COMMIT" value="0"
/container/envs/add list=fah-env key="MIMALLOC_PURGE_DECOMMITS" value="1"
/container/envs/add list=fah-env key="MIMALLOC_PURGE_DELAY" value="0"

/container/add \
  file=kingston/fastadhunter-arm64-0.4.0.tar \
  interface=veth1 \
  root-dir=/kingston/fastadhunter/root \
  mountlists=fah-config,fah-data \
  envlists=fah-env \
  logging=yes \
  start-on-boot=yes \
  comment="fastadhunter"
```

The three `MIMALLOC_*` keys return freed pages to the kernel instead of holding
them, which is what keeps RSS flat across list refreshes.

**Keep the comment exactly `fastadhunter`, with no version in it.** Every
`[find comment="fastadhunter"]` below — and in §7 and §8 — uses `=`, which is
exact equality in RouterOS, not a substring match. A comment of
`"fastadhunter 0.4.0"` makes all of them match nothing and fail *silently*: the
start, the watchdog's stop/start pair and the rollback all become no-ops. The
version is already recorded where it cannot drift — RouterOS derives `name=`
from the image tag and shows `tag="docker.io/library/fastadhunter:0.4.0"` in
`/container/print detail`. Use `~` only if you inherited a container whose
comment already carries a version.

Importing takes a while on RB5009 hardware. Wait for the status to leave
`extracting` (`/container/print detail`), then:

```routeros
/container/start [find comment="fastadhunter"]
```

Expect `status=running`.

### First-boot output

```routeros
/log/print where topics~"container"
```

> **Set `logging=yes` explicitly.** Ticking the box in WinBox's Container
> dialog does not always persist — check `/container/print detail` and set it
> from the CLI if absent. Without it you get RouterOS runtime lines but none of
> the application's stdout, which is where every first-boot diagnosis lives,
> including the one-time API key.

On an empty `/config` FastAdHunter generates its config, API key and
self-signed TLS certificate ([CONFIGURATION.md](../CONFIGURATION.md) §First
boot). **The API key is printed exactly once.** If you miss it, the file is at
`kingston/fastadhunter/config/apikey`; rotate via
`POST /api/v1/config/apikey/rotate` if it may have been exposed in the log.

## 4b. Configure it

An empty `/config` makes the container fully functional with zero
configuration: it writes `fastadhunter.toml` with the shipped defaults, an API
key, a self-signed TLS certificate for the API, a dashboard password and a
session secret. **Two of those are printed once and never again** — the API key
and the dashboard password. Copy both out of the log now
([CONFIGURATION.md](../CONFIGURATION.md) §First boot).

Lost the password? Delete `/config/auth-hash` and restart; a new one is printed
([SECURITY.md](../SECURITY.md) §Password recovery). Lost the API key? Rotate it
with `POST /api/v1/config/apikey/rotate`.

The dashboard is the same origin as the API — open
`https://fah-api.localbox.ro:8443/` and log in with that password.

**Change configuration through the API, not by editing the file.** The TOML
lives inside a mounted volume and the running process owns it; a hand edit is
overwritten on the next write-back and cannot be verified:

```sh
curl -sk -X POST -H "Authorization: Bearer $FAH_KEY" -H 'content-type: application/json' \
  -d '{"dns":{"blocking":{"mode":"null_ip"}}}' \
  https://fah-api.localbox.ro:8443/api/v1/config

curl -sk -H "Authorization: Bearer $FAH_KEY" \
  https://fah-api.localbox.ro:8443/api/v1/config
```

Every key is validated and written back. Keys are **boot-class** or
**runtime-class**: a runtime key takes effect immediately, a boot key answers
`restart_required: true` and needs a container restart. `engine.mode`,
listener addresses and ports are boot-class. Which is which:
[CONFIGURATION.md](../CONFIGURATION.md).

The default rule lists ship enabled, so the ruleset compiles on first boot with
no action. Check and drive them with:

```sh
curl -sk -H "Authorization: Bearer $FAH_KEY" https://fah-api.localbox.ro:8443/api/v1/lists
curl -sk -X POST -H "Authorization: Bearer $FAH_KEY" \
  https://fah-api.localbox.ro:8443/api/v1/lists/refresh
```

A ruleset stuck at 0 rules is an egress problem, not a configuration one — §3.2
and the troubleshooting table.

### If you want DoT or DoH

Both need a certificate clients already trust. Android's Private DNS validates
against the system store and **refuses a private CA outright**, so the
self-signed certificate from first boot will not do: you need a public name and
a publicly-issued certificate, dropped into `/config` as `api-cert.pem` and
`api-key.pem`. How the reference deployment obtained and renews one is in
[public-certificate.md](public-certificate.md).

Then enable them — boot-class, so restart after:

```sh
curl -sk -X POST -H "Authorization: Bearer $FAH_KEY" -H 'content-type: application/json' \
  -d '{"dns":{"listen":{"dot_enabled":true,"dot_port":853,"doh_enabled":true}}}' \
  https://fah-api.localbox.ro:8443/api/v1/config
```

DoH rides the API listener at `https://<api-host>:8443/dns-query` and is the
one unauthenticated route there. It binds wherever `api.address` binds — set to
`0.0.0.0` that is IPv4 only. DoT gets its own listener on `:853`, dual-stack.

Client setup for both is §5d.

## 5. Point the LAN at FastAdHunter

```routeros
/ip/dhcp-server/network/set [find address=192.168.10.0/24] \
  dns-server=172.17.0.2
```

Clients pick this up on their next lease renewal. RouterOS's own `/ip/dns`
service is unaffected — it lives on the router's LAN address, not on
`172.17.0.2` — so leave it configured as a fallback to revert to (§8).

### What a client can still do instead

Handing out the address covers the clients that ask. It does not cover the ones
that do not. Three escape routes, three different closures:

| Route | Closed by | State |
| --- | --- | --- |
| Plain DNS to a public resolver, UDP/TCP 53 | firewall drop on egress `:53` | rules below |
| DoT to a public resolver, TCP 853 | firewall drop on egress `:853` | rules below |
| DoH to a public resolver, TCP 443 | not a firewall matter — ordinary HTTPS to an ordinary-looking host, with no port to filter on. It closes at the **SNI**, once §5c's steering is on and the resolver's hostname is on a blocklist | needs §5c plus a list entry |

```routeros
/ip/firewall/filter/add chain=forward action=drop protocol=udp \
  src-address=192.168.10.0/24 out-interface-list=WAN dst-port=53 \
  comment="Block direct external DNS UDP"
/ip/firewall/filter/add chain=forward action=drop protocol=tcp \
  src-address=192.168.10.0/24 out-interface-list=WAN dst-port=53 \
  comment="Block direct external DNS TCP"
/ip/firewall/filter/add chain=forward action=drop protocol=tcp \
  src-address=192.168.10.0/24 out-interface-list=WAN dst-port=853 \
  comment="Block direct external DoT"
```

`out-interface-list=WAN` is what makes them safe: LAN clients must still reach
the container, which answers on `:53` and on `:853` as its own DoT listener.
Placement follows §5c's rule — ahead of `fasttrack-connection`, or an
already-open flow keeps passing.

**Reaching our own `:853` — check it, do not assume it.** Those rules are
egress-only, so they never touch a LAN client talking to the container. What
they also never do is *allow* it. On the reference chain there are explicit
accepts for the container on `:53` and nothing for `:853`; the last rule drops
WAN traffic only, so a LAN client reaching `:853` falls off the end and takes
RouterOS's default, which is accept. That is not something to rely on silently
— a later rule appended to the chain changes it without touching anything named
`fastadhunter`. Confirm the DoT listener answers from a LAN host before setting
Private DNS on a phone, and add an explicit accept beside the `:53` pair if the
chain ever grows a terminal drop.

**Until `:443` is steered, DoH is open and nothing here closes it.** Say so out
loud when someone reports that a phone still shows ads.

### Replacing an existing resolver — keep both, switch with a script

If the router already redirects `:53` to another filter (AdGuard Home,
Pi-hole), clients never see the container's address: the redirect decides which
resolver answers. Cutover is then a set of value changes, not a DHCP change,
and the old resolver stays running as a one-command rollback.

Find every rule naming the current resolver first — there are usually more than
expected:

```routeros
/ip/firewall/nat/print detail where dst-port=53
/ip/firewall/filter/print detail where dst-port=53
/ip/dns/print
```

A typical setup has six: two `dstnat` redirects, two `srcnat` accepts that
exempt VPN clients from masquerade (these match the *post-dstnat* address,
which is why they break if only the redirects change), and two `forward`
accepts. Save one script per direction, matching on **comments as they actually
read on your router** — read them, do not copy the strings below blind:

```routeros
/system/script/add name=dns-fah source={
  /ip/firewall/filter/set [find comment~"DNS filter redirect"] dst-address=172.17.0.2
  /ip/firewall/nat/set [find comment~"DNS filter redirect"] to-addresses=172.17.0.2
  /ip/firewall/nat/set [find comment~"No NAT WG DNS"] dst-address=172.17.0.2
  /ip/dns/set servers=172.17.0.2
  /ip/dns/cache/flush
}

/system/script/add name=dns-old source={
  /ip/firewall/nat/set [find comment~"DNS filter redirect"] to-addresses=<old>
  /ip/firewall/nat/set [find comment~"No NAT WG DNS"] dst-address=<old>
  /ip/firewall/filter/set [find comment~"DNS filter redirect"] dst-address=<old>
  /ip/dns/set servers=<old>
  /ip/dns/cache/flush
}
```

**The two scripts order their statements differently on purpose.** Switching
*to* FastAdHunter opens the forward path before the redirect starts using it;
switching back stops the redirect before the path closes. Reverse either and
traffic is briefly forwarded to an address the filter chain has not accepted
yet — which a default `drop` at the end of `forward` swallows.

A redeploy then becomes:

```routeros
/system/script/run dns-old
# ... container remove / add / start (§4) ...
/system/script/run dns-fah
```

**If §7's watchdog is installed**, bracket that with
`/system/scheduler/disable fah-liveness` and `enable` — it restarts the
container on two failed health checks and will fight a deploy in progress.
Check first: `/system/scheduler/print`. It is not installed on the reference
router, so there is nothing to fight there.

Check the old resolver too (`/container/print detail`). If it shows
`status=stopped`, running `dns-old` is worse than skipping it: it aims the
redirect *and* `/ip/dns` at an address nothing answers, so DNS breaks either
way and you must remember to run `dns-fah` to recover. Skipping leaves the
redirect already on FastAdHunter's address, so the LAN recovers by itself the
moment the new container starts — at the cost of a DNS gap for the length of
the `extracting` phase, largely absorbed by client resolver caches.

> **Do not replace the redirect with `servers=172.17.0.2,<old>` and let
> RouterOS fail over on its own.** It works, and the automatic failover is
> tempting — but then every query reaches FastAdHunter from the *router's*
> address instead of the client's. Per-client statistics and `$client` rules
> (Policies) become impossible to apply. The redirect is what preserves client
> identity.

### IPv6 — establish where it resolves before trusting any measurement

FastAdHunter binds `[dns.listen] address`, **`::` by default** — one dual-stack
socket serving both families (`IPV6_V6ONLY` is cleared explicitly, and v4
clients are still reported canonically rather than as `::ffff:…`). The listener
is not the constraint.

Steering is a separate question, and the answer changes what §6 and §9 prove:
**if part of the LAN bypasses FastAdHunter, they silently describe a fraction
of it.** On the reference deployment IPv6 *does* reach the container — two
`/ipv6/firewall/nat` dstnat rules force every v6 `:53` to it regardless of
destination, which is stricter than the v4 half — and RA on `BRIDGE`
advertises `dns=fd6c:7f32:8e91:1::2`.

Do not assume either answer elsewhere. Read all five; the answer is never in
one alone:

```routeros
/ipv6/nd/print detail          # is RA advertising a DNS server, and which?
/ipv6/dhcp-server/print
/ipv6/dhcp-server/option/print
/ipv6/address/print            # does the advertised address exist here?
/ipv6/firewall/nat/print       # is v6 :53 redirected somewhere first?
```

The combination that no single command reveals: RA advertising an address from
an **expired prefix delegation**, *and* a `dstnat` capturing every v6 `:53` and
sending it to a container address that no longer answers. The redirect masks
the stale RA — clients query a dead address, the router hijacks the packet
anyway, and v6 resolution lives or dies on the redirect's target. Three rules
follow:

- **A rule comment names whoever was there when it was written.** Resolve the
  target instead: `/ipv6/neighbor/print where address=<target>` plus a ping.
  `status="failed"` and 100 % loss mean nothing answers there.
- **One table tells you nothing.** A `filter` chain with no `:53` drop looks
  like an open path until `nat` shows a redirect catching it first — and a
  redirect proves nothing until its target is verified alive.
- **Hardcoded global addresses expire.** Where a prefix comes from a dynamic
  delegation, a literal in `dns=`, a `dstnat` target or a static interface
  address outlives the delegation that made it valid. Prefer link-local or
  ULA; the reference deployment steers to `fd6c:7f32:8e91:1::2` for exactly
  this reason, and keeps the delegated `/56` in the dynamic `fah-lan6` list
  rather than writing it anywhere.

If IPv6 resolves somewhere else, record it — the soak then measures IPv4 only,
which biases QPS and cache-hit figures downward against the budgets.

## 5b. HTTP (Phase 2, `dns+http`)

Optional and independent of DNS: skip it and the deployment is a DNS-only
resolver. Reversible by removing two firewall rules.

### Turn on the HTTP engine

`engine.mode` is boot-class. Set it through the API and restart:

```sh
curl -sk -X POST -H "Authorization: Bearer $FAH_KEY" -H 'content-type: application/json' \
  -d '{"engine":{"mode":"dns+http"}}' https://fah-api.localbox.ro:8443/api/v1/config
```

Without the API, add a **new** envlist — never the key to `fah-env`, which
other containers share:

```routeros
/container/envs/add list=fah-mode key="FAH__ENGINE__MODE" value="dns+http"
/container/set [find comment="fastadhunter"] envlists=fah-env,fah-mode
```

Confirm before touching the firewall:

```routeros
/log/print where message~"HTTP listener bound"
```

`addr=[::]:8080` — the default `[http.listen] port` is **8080**, not 80. The
router dst-nats 80 to it, so the listener needs no privilege after the ADR-0004
drop.

### The redirect

The skip rule must precede the dst-nat; appending in this order does that. The
`dstnat` chain has no final drop and the DNS redirects only match port 53, so
no `place-before` is needed.

```routeros
/ip/firewall/address-list/add list=fah-http-skip address=192.168.0.0/16
/ip/firewall/address-list/add list=fah-http-skip address=10.0.0.0/8
/ip/firewall/address-list/add list=fah-http-skip address=172.16.0.0/12
/ip/firewall/address-list/add list=fah-http-skip address=169.254.0.0/16
/ip/firewall/address-list/add list=fah-http-skip address=127.0.0.0/8

/ip/firewall/nat/add chain=dstnat action=accept protocol=tcp dst-port=80 \
  dst-address-list=fah-http-skip \
  comment="fastadhunter http: leave local traffic alone"

/ip/firewall/nat/add chain=dstnat action=dst-nat protocol=tcp dst-port=80 \
  in-interface-list=LAN src-address=!172.17.0.0/24 \
  to-addresses=172.17.0.2 to-ports=8080 \
  comment="fastadhunter http"
```

Three parts are load-bearing, and §5c reuses all three:

- **The skip list is not optional.** The egress guard refuses private
  destinations by design (`[egress] allow_destinations = []`), so LAN-to-LAN
  HTTP must never enter the proxy or every local web UI stops loading.
- **`src-address=!172.17.0.0/24`** keeps the proxy's own outbound fetches from
  being redirected into itself. `CONTAINERS` is in the `LAN` list, so without
  it the container steers its own traffic back to itself.
- **`to-ports=8080`** must match `[http.listen] port`.

**The forward chain needs nothing** on the reference deployment: its only
catch-all drop is `in-interface-list=WAN`, so LAN → container falls through to
the default accept. Read your own chain — a deployment with a final LAN drop
needs an accept for `172.17.0.2:8080`, mirroring the DNS one.

### IPv6

Mirror the v4 steering with address lists `fah-http-skip6` and `fah-lan6` — the
latter dynamic, fed by the DHCPv6 delegation — two dstnat accepts, then the
dst-nat to `:8080`. The rules, the evidence behind them and the one open
criterion (how far `fah-lan6` lags a live delegation change) are in
[`p2-14-review.md`](code-review/phase2/p2-14-review.md).

The proxy still fetches origins over **IPv4 regardless** — `resolve_host`
returns A before AAAA deliberately.

### Verify

```routeros
/ip/firewall/nat/print stats where comment~"fastadhunter"
```

A packet count above zero on the dst-nat rule is the only proof traffic is
arriving; everything else can look healthy while nothing does. Then:

```sh
curl -s -o /dev/null -w "%{remote_ip}\n" http://neverssl.com/
curl -sk -H "Authorization: Bearer $FAH_KEY" \
  "https://fah-api.localbox.ro:8443/api/v1/telemetry" | grep -o '"http":{[^}]*}'
```

`counters.http` must have moved. Subscribe to `WS /api/v1/events` while the
`curl` runs for the individual requests.

**Two results that look like failures and are not:**

- **Blocked ad domains never appear in the HTTP log.** DNS blocking preempts
  HTTP: the domain resolves to `0.0.0.0`, the client never opens a socket.
- **A low `kind=http` count is the web being HTTPS.** Phase 2 filters only the
  unencrypted remainder; the nat counter distinguishes "no traffic" from "not
  intercepted".

## 5c. HTTPS (Phase 3, `dns+http+https`)

Optional and independent of §5b: skip it and HTTPS goes straight out, filtered
by DNS alone. **Read §The no-SNI warning before steering** — this is the one
step in this guide that can break sites the DNS layer never touched.

**Order matters, in one place.** Prove the listener, reject QUIC, add the skip
and the dst-nat, verify. The QUIC reject goes first because a browser that
already prefers HTTP/3 walks past a TCP-only steer: land the dst-nat first and
the first verification reads as "nothing arrives", when what is happening is
that everything left over UDP.

### Turn on the HTTPS engine

Same mechanism as §5b, different value:

```sh
curl -sk -X POST -H "Authorization: Bearer $FAH_KEY" -H 'content-type: application/json' \
  -d '{"engine":{"mode":"dns+http+https"}}' https://fah-api.localbox.ro:8443/api/v1/config
```

It answers `restart_required: true`.

### Prove the listener before steering

```routeros
/log/print where message~"HTTPS SNI listener bound"
```

`addr=[::]:8444` — the default `[https.listen] port` is **8444**. Then, from a
LAN host, prove the splice serves the origin's own certificate:

```sh
openssl s_client -connect 172.17.0.2:8444 -servername neverssl.com </dev/null \
  | openssl x509 -noout -subject
```

It must print neverssl's subject, and `WS /api/v1/events` must show a
`kind: https-sni` item. If either is missing, do not touch the firewall.

### Reject QUIC

**Place the reject ahead of `fasttrack-connection`, not merely ahead of
`accept established,related`.** On the default RouterOS chain the latter is not
enough: `FastTrack` sits *before* the accept rule, and a fasttracked connection
leaves the filter path altogether after its first packet, so a reject landing
behind it never sees an established QUIC flow again. Read the chain
(`/ip/firewall/filter/print where chain=forward`) and place before whichever of
the two comes first.

```routeros
/ip/firewall/filter/add chain=forward action=reject reject-with=icmp-admin-prohibited \
  protocol=udp dst-port=443 in-interface-list=LAN out-interface-list=WAN \
  place-before=[find comment="FastTrack"] comment="fastadhunter: no QUIC, force TCP"
/ipv6/firewall/filter/add chain=forward action=drop protocol=udp dst-port=443 \
  in-interface-list=LAN out-interface-list=WAN \
  place-before=[find comment="fasttrack IPv6"] comment="fastadhunter v6: no QUIC, force TCP"
```

Comments differ between the two chains on a stock RouterOS — `FastTrack` and
`fasttrack IPv6`. A chain with no fasttrack rule keeps the original placement:
match `comment~"accept established"` instead. If neither comment exists on the
router in front of you, use the numeric index rather than guessing at a regex.
The mechanism is in [routeros-traps.md](routeros-traps.md) §FastTrack takes a
connection out of the filter chain.

The rule is narrow — UDP/443, LAN to WAN — so sitting ahead of `FastTrack`
costs nothing else. Already-open QUIC flows do **not** stop: they are already
fasttracked, and clients reconnect or the entries expire.

### The redirect

Same shape and the same reasoning as §5b's, reusing its `fah-http-skip` and
`fah-http-skip6` lists — apply that section first.

```routeros
/ip/firewall/nat/add chain=dstnat action=accept protocol=tcp dst-port=443 \
  dst-address-list=fah-http-skip \
  comment="fastadhunter https: leave local traffic alone"
/ip/firewall/nat/add chain=dstnat action=dst-nat protocol=tcp dst-port=443 \
  in-interface-list=LAN src-address=!172.17.0.0/24 \
  to-addresses=172.17.0.2 to-ports=8444 \
  comment="fastadhunter https"
```

`protocol=tcp` only — QUIC is not intercepted (a p3-03 non-goal), which is what
the reject above exists for. Browsers fall back to TCP only when QUIC fails;
left open it is a bypass (measured 2026-09-11,
[p3-06-n3-alert-ab.md](code-review/phase3/p3-06-n3-alert-ab.md)).

New connections steer immediately; established flows finish on their conntrack
entry, so a client whose flows predate the rule must reconnect
([routeros-traps.md](routeros-traps.md) §Steering one client).

### IPv6 — decide, and record which

Mirroring §5b's v6 steering extends SNI filtering to v6 traffic; leaving it
unsteered means v6 HTTPS is covered by the DNS layer alone. Either is
defensible, but **write down which you chose** — the two behave differently and
nothing on the box reports the difference.

```routeros
/ipv6/firewall/nat/add chain=dstnat action=accept protocol=tcp dst-port=443 \
  dst-address-list=fah-http-skip6 comment="fastadhunter https v6: leave local traffic alone"
/ipv6/firewall/nat/add chain=dstnat action=accept protocol=tcp dst-port=443 \
  dst-address-list=fah-lan6 comment="fastadhunter https v6: leave the delegated LAN prefix alone"
/ipv6/firewall/nat/add chain=dstnat action=dst-nat to-address=fd6c:7f32:8e91:1::2/128 \
  to-ports=8444 protocol=tcp dst-address=!fd6c:7f32:8e91:1::2/128 \
  in-interface-list=LAN dst-port=443 comment="fastadhunter https v6"
```

**Consequence either way:** interception identity is IP or CIDR, and a phone's
v6 source address rotates. A v6-steered connection from a listed device is
therefore **spliced, not intercepted** — interception rides v4 only.

### The no-SNI / ECH warning

Once `:443` is steered, a TCP connection carrying **no plaintext SNI** is
**closed**, not forwarded: the container cannot recover the destination.
`getsockopt(SO_ORIGINAL_DST)` returns `ENOENT` through RouterOS dst-nat
(measured 2026-08-31, [routeros-traps.md](routeros-traps.md)). That covers
legacy clients, IP-literal HTTPS and ECH hellos the browser does not retry.
`[https.sni] no_sni` decides only how the closure is **reported**, never
whether it happens.

Confirm the deployed lists cover what the household relies on before steering,
and keep the rollback line at hand for the first hour.

### Interception and the CA

Splicing needs no client setup at all; that is the "any client, zero setup"
half of Phase 3. Interception is opt-in per client and needs the CA installed
on each listed device **first** — a listed device without the CA sees its
connections close (`PUT /api/v1/interception`, CONFIGURATION.md §Interception
Document).

Export with `GET /api/v1/certificates/ca/export`; the per-device walkthrough
lives in the p3-06 review file's runbook.

> **Generating a CA changes DoT.** `dot_tls` falls back to the API certificate
> pair only while the store holds no CA; with one present, `MintingResolver`
> mints a private leaf for the SNI the client sent, and Android Private DNS —
> which always sends SNI and validates against the system store — stops
> working on every device. Interception ships disabled, so the shipped state is
> the working one.

### Verify

```routeros
/ip/firewall/nat/print stats where comment~"fastadhunter https"
```

Packet counts above zero on the dst-nat rule are the only proof traffic
arrives. Then, from an **unlisted** device with nothing installed:

```sh
curl -sv https://<a domain the deployed lists block>/
```

The TLS connection must fail before any certificate — no `subject:` or
`issuer:` line, a reset or unexpected EOF rather than a certificate error.
`GET /api/v1/telemetry` shows `listeners.https.blocked` +1 and `connections`
+1, with one `WS /api/v1/events` item carrying `kind: "https-sni"`,
`verdict: "block"`. Control: the same `curl` to an allowed domain returns the
**origin's own** certificate, with a public issuer that is never
`FastAdHunter CA`, and a `https-sni` `pass` item.

## 5d. Client setup for DoT and DoH

Nothing here is required. Plain DNS already works through DHCP and RA, and a
client that configures nothing is still filtered. DoT and DoH are for the
devices that would otherwise carry their own encrypted DNS straight past the
router — the escape routes in §5.

All of it needs §4b's public certificate first, and all of it is **LAN-only**:
the names resolve to private addresses, so a device off the network gets no
answer. Reaching them from outside means a dst-nat on `:853` or a VPN, and that
is a separate decision.

**Android — DoT.** Settings → Network & internet → Private DNS → *Private DNS
provider hostname*, then the hostname pointed at the container
(`fah-dot.localbox.ro` on the reference deployment). Android fixes the port at
853 and validates against the system store; nothing is installed on the phone.
Off the LAN it fails closed, so it has to go back to Automatic when the device
leaves — or reach the router over a VPN.

**Firefox — DoH.** Settings → Privacy & Security → DNS over HTTPS → *Max
Protection*, Provider: Custom, `https://fah-api.localbox.ro:8443/dns-query`.
Max Protection makes Firefox fail rather than quietly fall back to system DNS,
which is what you want — a silent fallback hides an endpoint that stopped
answering.

**Chrome and Edge — DoH.** Settings → Privacy and security → Security → *Use
secure DNS* → With: Custom, same URL. Chrome falls back to system DNS when the
custom resolver fails and gives no indication it has done so, so verify rather
than trusting the toggle: `chrome://net-internals/#dns`, or block a known-bad
domain and watch the live feed.

**Verifying any of them.** A configured client appears in
`WS /api/v1/events` with `transport` of `dot` or `doh` rather than `udp`. From
a terminal:

```sh
kdig @fah-dot.localbox.ro +tls doubleclick.net
curl -s -H 'accept: application/dns-message' \
  'https://fah-api.localbox.ro:8443/dns-query?dns=…' | xxd | head -3
```

**iOS has no built-in UI** for either; it needs a configuration profile.

## 6. On-device verification checklist

Each is a gate for the next.

| # | Check | Command | Expected |
| - | ----- | ------- | -------- |
| 1 | Container running | `/container/print detail` | `status=running` |
| 2 | First boot wrote to the SSD | `/file/print where name~"fastadhunter/config"` | `fastadhunter.toml`, `apikey`, `api-cert.pem`, `api-key.pem` |
| 3 | Ruleset compiled | `/log/print where message~"ruleset compiled"` | non-zero rule count |
| 4 | Listeners bound | `/log/print where message~"listener bound"` | udp + tcp, plus DoT/DoH and HTTPS in full mode |
| 5 | Resolves from the router | `/tool/dns-lookup name=example.com server=172.17.0.2` | an address |
| 6 | Resolves from a LAN client | `nslookup example.com 172.17.0.2` | an address |
| 7 | Known ad domain blocked | `nslookup doubleclick.net 172.17.0.2` | `0.0.0.0` (default `null_ip`) |
| 8 | Health endpoint | `curl -k https://fah-api.localbox.ro:8443/health` | `200`, reporting the version you deployed |
| 9 | Stats show real counters | `GET /api/v1/stats` | non-zero `total` / `blocked` |
| 10 | Engine telemetry | `GET /api/v1/telemetry` | populated `ruleset`, `counters`, `upstreams` |
| 11 | Phone browses with ads blocked | manual | ad slots empty |

Checks 9 and 10 need `-H "Authorization: Bearer $FAH_KEY"` and `--insecure`
while the certificate is self-signed. Endpoint shapes:
[API.md](../API.md). The HTTP and HTTPS steering have their own verification in
§5b and §5c; this table covers the DNS path and the API.

## 7. Liveness on RouterOS

**RouterOS ignores the image's `HEALTHCHECK`.** Its container runtime does not
implement Docker healthcheck semantics, so the `--healthcheck` self-probe baked
into the image never runs on-device. Liveness needs a RouterOS-side poll.

Optional, and **not installed on the reference router.** If you add it,
remember §5's redeploy bracket.

```routeros
/system/scheduler/add name=fah-liveness interval=1m on-event={
  :global fahFails
  :if ([:typeof $fahFails] = "nothing") do={ :set fahFails 0 }
  :do {
    /tool/fetch url="https://fah-api.localbox.ro:8443/health" check-certificate=no \
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

Restarts logged by this script count as watchdog restarts for a soak's
acceptance criteria — the target is **zero**.

## 8. Rollback

Firewall first, since that is what restores service:

```routeros
/ip/firewall/nat/remove [find comment="fastadhunter https"]
/ip/firewall/nat/remove [find comment="fastadhunter https: leave local traffic alone"]
/ipv6/firewall/nat/remove [find comment~"fastadhunter https v6"]
/ip/firewall/filter/remove [find comment="fastadhunter: no QUIC, force TCP"]
/ipv6/firewall/filter/remove [find comment="fastadhunter v6: no QUIC, force TCP"]
```

**Exact comments for the NAT pair, not `comment~"fastadhunter http"`** — that
regex also matches the https rules, so the shorter line tears down HTTPS
steering while claiming to roll back HTTP. And **do not forget the QUIC
rejects**: they live in `filter`, not `nat`, and forgetting them is a quiet
failure. HTTPS works again over TCP so nothing looks broken, while HTTP/3 stays
refused for the whole household with no fastadhunter rule left in NAT to
explain why.

The HTTP pair is the same shape:

```routeros
/ip/firewall/nat/remove [find comment="fastadhunter http"]
/ip/firewall/nat/remove [find comment="fastadhunter http: leave local traffic alone"]
```

Then, in order of escalation:

1. **Revert DNS only** — LAN keeps working, filtering off:

   ```routeros
   /ip/dhcp-server/network/set [find address=192.168.10.0/24] dns-server=192.168.10.1
   ```

2. **Stop the container** (`/system/scheduler/disable fah-liveness` first, if
   §7 is installed):

   ```routeros
   /container/stop [find comment="fastadhunter"]
   ```

3. **Remove it** — keeps `/config` and `/data` on the SSD, so a re-add resumes
   with the same key, certificate and cached lists:

   ```routeros
   /container/remove [find comment="fastadhunter"]
   ```

4. **Full reset** — additionally delete `kingston/fastadhunter/config` and
   `…/data`. The next start behaves as a first boot and prints a **new** API
   key.

To downgrade rather than remove: keep the previous tarball on the SSD, and
re-add with `file=` pointing at the old image. `/config` and `/data` survive,
so a rollback is a container swap, not a data migration. Giving the new
container its own `root-dir` lets both exist at once, which makes the swap a
stop and a start.

## 9. Soak

**Goal:** real household traffic with no crashes, no watchdog restarts, and RSS
≤ 128 MB steady-state ([PERFORMANCE.md](../PERFORMANCE.md) §Budgets).

A soak is a **measurement procedure, not a release gate.** Run it when you want
the number — and not on a build with a known defect, where it measures the
defect rather than the deployment.

### Collection

Sample `/api/v1/telemetry` every 5 minutes from any always-on LAN host. One
call covers ruleset, counters, latency totals, upstreams, cache and memory:

```sh
mkdir -p soak
while true; do
  ts=$(date -u +%Y%m%dT%H%M%SZ)
  curl -s --insecure -H "Authorization: Bearer $FAH_KEY" \
    https://fah-api.localbox.ro:8443/api/v1/telemetry > "soak/telemetry-$ts.json"
  sleep 300
done
```

The engine also persists its own series — `GET /api/v1/history/perf` returns
per-sample RSS, latency percentiles and cache figures across the whole window,
so the loop is a cross-check rather than the only record. Snapshot
`/container/print detail` and `/system/resource/print` periodically too, since
RouterOS accounts for memory differently than the process does.

### What to read, and what passes

| Budget | Metric | Passes when |
| ------ | ------ | ----------- |
| RAM steady-state ≤ 128 MB | `process_resident_memory_bytes`, max over the window | under the cap **and flat** — a rising trend is a leak and fails even below the cap (hard rule 4) |
| Compiled ruleset ≤ 40 MB | `memory_component_bytes{component="ruleset"}` | under the cap |
| Cache-hit / blocked p99 < 1 ms | `query_duration_seconds` by `verdict` | under 1 ms |
| No dropped events | `events_dropped_total` | stays 0 |
| Upstream health | `upstream_failures_total`, `…_consecutive_failures` | no endpoint parked |
| No crashes / restarts | RouterOS log | zero, and `fah-liveness` never fired |

Household traffic will not approach the 10 000 QPS budget — that belongs to the
load benches. A soak validates steady-state memory, stability and latency under
a real query mix, not peak throughput.

Record the actual numbers in the task's review file under
[code-review/](code-review/), with the corpus, workload and device. Anything the
device disproves gets an issue, or an ADR in [decisions/](decisions/) if it is
design-level rather than a bug.

## Troubleshooting

| Symptom | Likely cause |
| ------- | ------------ |
| `status=extracting` forever, `OS`/`Arch` blank | Tarball is OCI layout, not legacy docker-archive — convert with skopeo (§1). RouterOS reports no error, it just hangs |
| `container add` rejects the file outright | Filesystem tarball (`-o type=tar`) rather than an image archive (§1) |
| Container exits immediately, bind error in log | Port 53 as non-root — see §4 |
| Container runs, no config written to SSD | Mounts wrong or SSD not writable — `/container/mounts/print` and `/disk/print` |
| Lists never download, ruleset stays 0 | No egress — check the masquerade rule, and that `gateway=172.17.0.1` matches the bridge address. If only *some* lists fail, it is the veth IPv6 trap in §3.1 |
| LAN clients time out on DNS | DHCP not renewed yet, or the `forward` accepts are missing |
| API unreachable but DNS works | `api.address` bound narrower than `0.0.0.0` |
| Steering rules exist but counters stay 0 | `CONTAINERS` is not in the `LAN` interface list (§3.1), or the flows predate the rule and clients have not reconnected |
| Ads still shown on the phone | Client using DoH/DoT to bypass the LAN resolver — watch `WS /api/v1/events` for whether the domain arrives at all. §5 names each escape route; DoH stays open until §5c's `:443` steering is on |
