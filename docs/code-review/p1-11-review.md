# Code Review — p1-11 RB5009 Deployment and Soak

**Scope:** `docs/deploy-rb5009.md` · on-device deployment to a live MikroTik
RB5009 (RouterOS 7, arm64) · **Date:** 2026-07-19 · **Status:** deployment
validated, budgets measured at 1.19M rules; 24h soak NOT run; six defects
found, none fixed yet.

## Overall assessment

The engine works on the target hardware and beats every measured budget with
large margin — the matcher answers a 1.19M-rule lookup in ~40 µs and the whole
process holds 49.6 MB RSS. Phase 1's functional goal ("a known ad domain
blocked, real counters in the API") is met on the device.

What the device disproved was never the engine: it was the *deployment
contract*. Four documented assumptions turned out to be wrong on RouterOS
(image format, healthcheck, privileged ports, container DNS), and one shipped
API silently loses data across restart. None of these are visible from a
development machine running Docker, which is exactly why this task exists.

No code was changed. The only repository change is `docs/deploy-rb5009.md`.

---

## 1. What was implemented

`docs/deploy-rb5009.md` — a 35-line Phase 0 stub with two open questions,
replaced with a complete, device-verified deployment guide:

| Section | Content |
| ------- | ------- |
| §0 | Prerequisites: device-mode, ext4 SSD, buildx + QEMU |
| §1 | arm64 build **plus archive-layout check and skopeo conversion** |
| §2 | Tarball upload |
| §3.1 | veth/bridge, **including the IPv6 requirement** |
| §3.2 | Firewall — *change nothing up front*, symptom→fix table only |
| §3.3 | Storage, mounts, **`resolv.conf` mount requirement** |
| §4 | Container creation, first boot, **port-53 redirect (required)** |
| §5 | DHCP cutover **+ IPv6 bypass warning** |
| §6 | 11-point verification checklist |
| §7 | Liveness via `/system/scheduler` (RouterOS ignores `HEALTHCHECK`) |
| §8 | Rollback, escalating in four stages |
| §9 | 24h soak procedure with metric→budget mapping |
| — | Troubleshooting table |

Corrections to previously documented claims, each forced by the device:

1. **Build command was wrong.** The stub's `-o type=tar` produces a filesystem
   tarball, which RouterOS rejects outright.
2. **`HEALTHCHECK` is ignored by RouterOS** — Phase 0 left this as an open
   question. Answered: its runtime does not implement Docker healthcheck
   semantics, so the image's `--healthcheck` self-probe never runs on-device.
   Liveness needs a RouterOS scheduler script.
3. **Port 53 cannot be bound** (see §3, defect 4).
4. **`/etc/resolv.conf` is not populated** by RouterOS (see §3, defect 5).

---

## 2. Deployment settings (MikroTik)

Verified working configuration. `kingston` is the ext4 USB SSD; substitute
your own disk slot, subnets and IPv6 prefix.

### 2.1 Image build (build host)

```sh
docker buildx build --platform linux/arm64 \
  -t fastadhunter:0.1.0 -o type=docker,dest=fastadhunter-arm64.tar .

# REQUIRED CHECK — containerd image store emits OCI layout, which RouterOS
# cannot import (it hangs at status=extracting, silently, forever).
tar -tf fastadhunter-arm64.tar | head -5
#   blobs/ oci-layout index.json  -> OCI, must convert
#   <hash>.tar entries            -> legacy, ready

docker run --rm -v "$PWD:/work" quay.io/skopeo/stable copy --insecure-policy \
  oci-archive:/work/fastadhunter-arm64.tar \
  docker-archive:/work/fastadhunter-rosready.tar:fastadhunter:0.1.0
```

Cross-building arm64 on amd64 also needs QEMU registered
(`docker run --privileged --rm tonistiigi/binfmt --install arm64`) — Docker
Desktop does **not** always ship this; check `docker buildx inspect` lists
`linux/arm64` before starting a 30-minute build.

### 2.2 Network

```routeros
/interface/veth/add name=veth2 \
  address=172.17.0.3/24,2a02:xxxx:xxxx:xxxx::11/64 \
  gateway=172.17.0.1 gateway6=2a02:xxxx:xxxx:xxxx::1
/interface/bridge/port/add bridge=CONTAINERS interface=veth2
```

**IPv6 is not optional if the LAN has IPv6.** See defect 2.

One veth per container — an existing container owns its own veth and cannot
share it.

### 2.3 Container config, mounts, env

```routeros
/container/config/set tmpdir=kingston/pull ram-high=256M

/container/mounts/add name=fah-config src=kingston/fastadhunter/config      dst=/config
/container/mounts/add name=fah-data   src=kingston/fastadhunter/data        dst=/data
/container/mounts/add name=fah-resolv src=kingston/fastadhunter/resolv.conf dst=/etc/resolv.conf

/container/envs/add name=ENV_FAH key=FAH__DNS__LISTEN__PORT value=5353
```

`kingston/fastadhunter/resolv.conf` must exist before first start:

```text
nameserver 1.1.1.1
nameserver 9.9.9.9
options timeout:2 attempts:2
```

`ram-high=256M` mirrors the PERFORMANCE.md hard ceiling. Do **not** set `0`
(unlimited) — the RB5009 shares 1 GB with RouterOS itself.

### 2.4 Container

```routeros
/container/add \
  file=kingston/fastadhunter-rosready.tar \
  interface=veth2 \
  root-dir=kingston/images/fastadhunter \
  mounts=fah-config,fah-data,fah-resolv \
  envlist=ENV_FAH \
  dns=1.1.1.1 \
  hostname=fastadhunter \
  logging=yes \
  start-on-boot=yes \
  comment="fastadhunter"
```

`logging=yes` must be set from the CLI — ticking the WinBox checkbox does not
reliably persist, and without it the application's stdout never reaches
`/log/print`, including the one-time API key.

Confirm `OS`/`Arch` populate as `linux`/`arm64` after import. If they stay
blank, the archive layout is wrong (§2.1).

### 2.5 Traffic redirect

If the router already redirects LAN DNS to a container, the cutover is a value
change on the **existing** rules, not new ones:

```routeros
/ip/firewall/nat/set [find comment="Redirect catre DNS filter(docker)"] \
  to-addresses=172.17.0.3 to-ports=5353
```

`to-ports=5353` is what absorbs the privileged-port problem (defect 4) — no new
rule, no root container, no capability. Rollback is the same two values back.

On a router with no existing redirect, two `dstnat` rules are required. Read
the chain first: `/ip/firewall/filter/add` appends to the end, behind any final
drop, where it has no effect.

---

## 3. Defects

### 1. HIGH — list mutations via the API are never persisted

`POST /api/v1/lists` returns `201 Created`, the list downloads, compiles and
blocks live traffic — and disappears on restart, silently.

`ConfigStore::apply_patch` is the only persisting path, and it has exactly one
caller: `post_config`. `create_list` ([routes.rs:264](../../crates/fah-api/src/routes.rs#L264))
calls `state.rules.add_list(&config)`, mutating only the in-memory
`ListManager`, and touches `state.config` solely to *read*
`refresh_hours_default`.

| Endpoint | In-memory | Persisted |
| -------- | --------- | --------- |
| `POST /api/v1/lists` | yes | **no** |
| `PATCH /api/v1/lists/{id}` | yes | **no** |
| `DELETE /api/v1/lists/{id}` | yes | **no** |
| `PUT /api/v1/rules/user` | yes | yes (`/data` cache) |
| `POST /api/v1/config` | yes | yes |

What makes it worse: list *content* **is** cached to `/data`
([lifecycle/mod.rs:489](../../crates/fah-rules/src/lifecycle/mod.rs#L489)), so
the failure is asymmetric. On boot `ListManager::new` reads the config, sees
only the originally configured lists, and loads those from cache — leaving the
downloaded content of API-added lists orphaned on disk.

**Observed:** two lists added via the API (1,136,202 rules) served traffic
correctly, then vanished on restart, dropping the matcher from 1,192,068 to
55,866 rules. No error, no warning.

**Severity rationale:** with no dashboard until Phase 3, this API is the only
supported way to manage filters. A user configures blocking, verifies it works,
reboots the router, and is unprotected with no indication anything changed.

**Fix direction:** `create_list`/`patch_list`/`delete_list` must route through
`ConfigStore` so the config file is rewritten, ideally sharing one path with
`post_config` so persistence cannot be forgotten again. Needs a regression test
that restarts a `ListManager` against the same `/config` and `/data`.

### 2. MEDIUM — no IPv4 fallback when IPv6 is broken

The list fetcher resolves a hostname, gets an AAAA, attempts IPv6, and fails
after ~2 s with no fallback to the A record.

**Observed, same container, same second:**

| Host | Records | Result |
| ---- | ------- | ------ |
| `raw.githubusercontent.com` | A only | ok, 80,874 rules |
| `small.oisd.nl` | A + AAAA | failed |
| `1.1.1.1` (literal) | — | ok |

Half-configured IPv6 is common on home networks. The failure mode is
pathological: DNS forwarding keeps working (upstreams are IP literals), the
container looks healthy, `/health` returns 200 — and the ruleset stays at 0, so
the ad blocker blocks nothing while appearing fine.

This also recurred *after* the veth's IPv6 was fixed and a manual refresh had
succeeded, suggesting a race between container start and RouterOS establishing
the v6 route. Whichever it is, the client should not depend on it.

**Fix direction:** happy-eyeballs (RFC 8305) or explicit A-record fallback in
the `reqwest` client.

### 3. MEDIUM — fetch errors discard their cause

```text
fetch https://small.oisd.nl failed: error sending request for url (https://small.oisd.nl/)
```

This is reqwest's outer error with the `source()` chain dropped. DNS failure,
connection refused, timeout and TLS rejection are indistinguishable. On a
distroless image with no shell, this log line is the *only* diagnostic
available on-device.

Diagnosing defect 2 required adding a probe list with an IP-literal URL via the
API to bisect the failure — work that a one-line cause would have made
unnecessary.

**Fix direction:** walk `source()` and include the underlying error.

### 4. DESIGN / ADR — port 53 cannot be bound on RouterOS

RouterOS honours the image's `USER nonroot` (uid 65532) and does **not** set
`net.ipv4.ip_unprivileged_port_start=0` as Docker does. Binding 53 fails with
`EACCES`; the container starts, compiles its ruleset, then exits 1:

```text
INFO  fastadhunter starting
INFO  ruleset compiled from cache rules=0
ERROR fastadhunter failed to start error=Permission denied (os error 13)
```

RouterOS exposes no `cap-add`, so `CAP_NET_BIND_SERVICE` is unavailable.

SECURITY.md currently claims FastAdHunter "runs as non-root; requires no
capabilities beyond binding its ports" — unsatisfiable on this platform. This
affects every RouterOS deployment, not one router.

Competitive note: `adguard/adguardhome` ships with `user=[]` (root) on all
architectures, which is why it binds 53 with no user-side configuration.

**Options:** (a) `setcap cap_net_bind_service=+ep` on the binary — keeps
non-root, but needs verification that BuildKit preserves the xattr through
`COPY --from` *and* that RouterOS preserves it on import; (b) a root-running
RouterOS image variant, matching the category norm; (c) document the redirect
requirement, which is the worst outcome for a product whose competitor needs
none. Needs an ADR and a SECURITY.md correction either way.

### 5. DEPLOYMENT — RouterOS does not populate `/etc/resolv.conf`

RouterOS accepts `dns=` on the container and displays it in
`/container/print detail`, but does not write it into the container's
`/etc/resolv.conf` — which the distroless image ships as a 0-byte file. With no
nameserver, every list download fails after ~5 s. DNS forwarding is unaffected,
so the container appears healthy.

Worked around with a single-file mount (§2.3). A baked-in default
`resolv.conf` in the image would make the container work out of the box, at the
cost of hardcoding a resolver — worth considering alongside defect 2, since
both stem from the list fetcher depending on the *system* resolver while the
process is itself a fully configured DNS resolver.

### 6. LOW — status and id defects

- **Stale status after boot.** Post-restart, `oisd-basic` reported
  `status=failed, rules=0, last_refresh=null` while the matcher was actively
  serving 55,866 rules loaded from that list's `/data` cache. Status does not
  reflect the ruleset in use.
- **`derive_id` collides.** `raw.githubusercontent.com/StevenBlack/hosts/.../hosts`
  and `.../1Hosts/master/Xtra/hosts.txt` both derive to `hosts`, producing
  `409 conflict`. Two unrelated lists cannot coexist without an explicit id.
- **Undocumented `id` field.** `POST /api/v1/lists` accepts `id`, which is what
  works around the collision, but API.md documents only
  `{url, enabled, refresh_hours}`.

---

## 4. Measured results

RB5009, RouterOS 7, arm64 container, 1,192,068 rules (oisd-basic + StevenBlack
hosts + 1Hosts Xtra).

| Budget (PERFORMANCE.md) | Target | Measured | |
| ----------------------- | ------ | -------- | - |
| RAM steady-state @1M | ≤128 MB | **49.6 MB** | pass |
| Compiled ruleset @1M | ≤40 MB | **37.7 MB** @1.19M · 33.2 MB @1M | pass |
| Verdict + cache hit p99 | <1 ms | **0.034 ms** | pass |
| Blocked query p99 | <1 ms | **0.042 ms** | pass |
| Container image size | ≤30 MB | **12 MB** | pass |
| Startup to serving @1M | 1–3 s | 153 ms **@55k only** | **not validated** |
| Sustained throughput | ≥10k QPS | not measured | **not validated** |

Cost per rule: **33.2 bytes**. Memory was flat across a 2-minute observation at
full ruleset — no growth.

Functional verification: `doubleclick.net`, `ads.pubmatic.com` and
`googlesyndication.com` all returned `0.0.0.0`; `example.com` and `github.com`
resolved normally; query log, stats aggregates, Prometheus metrics, TLS and
API-key auth all confirmed working against the live device.

## 5. Not done

- **24h soak not run.** Requires the cutover, which requires defect 1 fixed
  first — otherwise a router reboot mid-soak silently drops the ruleset to
  whatever is in the config file and invalidates the run.
- **Startup @1M not measured.** Blocked by defect 1; the lists must survive a
  restart. Workaround: add them via `POST /api/v1/config`, which persists.
- **Throughput not measured.** Needs a real load generator (`dnsperf`,
  `flamethrower`) on the LAN.

p1-11 should stay open until the soak runs.
