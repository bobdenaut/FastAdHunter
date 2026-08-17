# Code Review — p1-11 RB5009 Deployment and Soak

**Scope:** `docs/deploy-rb5009.md` · on-device deployment to a live MikroTik
RB5009 (RouterOS 7, arm64) · **Date:** 2026-07-19 · **Status:** deployment
validated, budgets measured at 1.19M rules; 24h soak NOT run. Six defects
found: 1, 3 and 4 fixed and verified on the device; 2 fixed pending a device
check; 5 worked around; 6 open.

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

Four of the six have since been fixed and three of those re-verified against the
live RB5009 — list persistence survives a restart, the privileged-port question
is settled as [ADR-0004](../../decisions/0004-privileged-port-binding.md), and the
fetch failures now name their cause. Defect 2's fix awaits a device check;
defects 5 and 6 remain open. Deployment findings are in
`docs/deploy-rb5009.md`.

Worth recording: **two of the six were initially misdiagnosed**, and both
misdiagnoses were downstream of defect 3. Reasoning from a stripped error
message produced a confident, wrong root cause for defect 2 and a fix direction
that would have been a no-op. Fixing the diagnostics first is what made the
rest cheap.

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
/interface/veth/add name=veth1 \
  address=172.17.0.2/24,2a02:xxxx:xxxx:xxxx::11/64 \
  gateway=172.17.0.1 gateway6=2a02:xxxx:xxxx:xxxx::1
/interface/bridge/port/add bridge=CONTAINERS interface=veth1
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
  interface=veth1 \
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
  to-addresses=172.17.0.2 to-ports=5353
```

`to-ports=5353` is what absorbs the privileged-port problem (defect 4) — no new
rule, no root container, no capability. Rollback is the same two values back.

On a router with no existing redirect, two `dstnat` rules are required. Read
the chain first: `/ip/firewall/filter/add` appends to the end, behind any final
drop, where it has no effect.

---

## 3. Defects

### 1. HIGH — list mutations via the API are never persisted — **FIXED**

`POST /api/v1/lists` returns `201 Created`, the list downloads, compiles and
blocks live traffic — and disappears on restart, silently.

`ConfigStore::apply_patch` is the only persisting path, and it has exactly one
caller: `post_config`. `create_list` ([routes.rs:264](../../../crates/fah-api/src/routes.rs#L264))
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
([lifecycle/mod.rs:489](../../../crates/fah-rules/src/lifecycle/mod.rs#L489)), so
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

#### Persistence fix as applied (2026-07-19)

All three handlers now read the current list set, write the resulting set back
through `ConfigStore::apply_patch`, and only then mutate the `ListManager`.

- **Persist before mutating.** The durable record is the step allowed to fail.
  A failed write returns `500` and the mutation does not happen, so the file
  and the engine cannot disagree — the reverse order would need a rollback that
  can itself fail.
- **One shared path.** `persist_lists` goes through `apply_patch`, the same
  validated + atomic write `post_config` uses; `merge` already replaces arrays
  wholesale, which is the right semantics for `[[rules.lists]]`.
- **`AppState::list_mutations`.** Read-modify-write across three steps needs
  serializing, or two concurrent writers each persist a set omitting the
  other's change. Admin-plane only — nothing on the DNS hot path takes it.

Regression test: `list_mutations_are_persisted_to_the_config_file`
([tests/api.rs](../../../crates/fah-api/tests/api.rs)) — reparses the TOML from
disk (what a restart actually sees) after create/patch/delete, and asserts a
rejected duplicate and a 404 delete leave the file untouched. Verified to fail
against the unfixed code (0 lists persisted, expected 2).

**Known side effect, accepted:** `apply_patch` writes the *effective* config,
so env-var overrides get baked into the file — on the RB5009 the first list
mutation writes `port = 5353` into `fastadhunter.toml`. This is pre-existing
behaviour of `POST /api/v1/config` ("the file always reflects the running
intent", CONFIGURATION.md), now reachable via list edits too. Harmless here
since the value matches the deployment, but removing `FAH__DNS__LISTEN__PORT`
later would no longer revert the port. Separating file-sourced from effective
config is a larger design question — not taken on here.

### 2. MEDIUM — list fetches fail in name resolution — **FIXED, pending device check**

> The original title was "no IPv4 fallback when IPv6 is broken". That diagnosis
> was wrong in both halves — there is a fallback, and IPv6 was never the
> problem. The original text is kept below, followed by the correction and the
> measured root cause, because how it was misread is the useful part.

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

#### Correction (2026-07-19): the above diagnosis does not hold

Happy-eyeballs is **already enabled**. `reqwest` builds on hyper-util's
`HttpConnector`, whose `happy_eyeballs_timeout` defaults to `Some(300ms)`
(`hyper-util-0.1.20/src/client/legacy/connect/http.rs:231`); `ConnectingTcp::new`
splits the resolved addresses by family and races the second family after that
delay. A fallback path exists whenever the resolver returns both an A and an
AAAA, so "attempts IPv6, no fallback to the A record" cannot be what happened —
and adding happy-eyeballs would have been a no-op patch.

The failure is therefore more likely *before* the connect, in name resolution,
where there is no fallback to speak of. Leading unconfirmed hypothesis: this is
a static **musl** binary and `reqwest`'s default resolver is `GaiResolver`, i.e.
musl's `getaddrinfo`, which queries A and AAAA in parallel and fails the whole
lookup if one query goes unanswered rather than returning the family that did
resolve. That would fit the observed table exactly — the A-only host resolved,
the A+AAAA host did not, and an IP literal needed no resolution at all.

**Why this was not caught the first time:** defect 3. The only evidence on the
device was reqwest's outer message, which is identical for a DNS failure and a
connect failure, so "IPv6 was attempted" was inferred rather than observed.

#### Root cause, measured on the device (2026-07-19)

The very first boot carrying defect 3's fix named it outright:

```text
fetch https://small.oisd.nl failed: error sending request for url (…):
  client error (Connect): dns error:
  failed to lookup address information: Try again
```

`Try again` is **`EAI_AGAIN`** from `getaddrinfo`. The connection was never
attempted — the hostname never resolved. This confirms the hypothesis above and
kills the original entry: IPv6 was never reached, so no amount of fallback
logic would have helped.

The `~2 s` in the original report corroborates it independently: that is the
`timeout:2` in the container's mounted `resolv.conf`, a *resolver* timeout. A
refused connection returns immediately and an unreachable route fails on a
different clock entirely.

**Fix as applied:** the `hickory-dns` feature on `reqwest`, replacing musl's
`getaddrinfo` with the pure-Rust resolver, plus an explicit `.hickory_dns(true)`
in `ListManager::new`. The call is feature-gated, so dropping the feature later
breaks the build instead of silently restoring the old behaviour. It reuses the
`hickory-proto` already in-tree for DNS upstreams — one added crate, not a
second DNS stack.

**Verified on hardware (2026-07-19).** With the hickory resolver, the boot
scheduler refreshed every remote list without a word — successful refreshes log
nothing — where the previous image failed `small.oisd.nl` at the same point
with `dns error: … Try again`. The only failure in that run was a local-file
list pointing at a path that genuinely did not exist:

```text
WARN scheduled list refresh failed list=custom
     error=read local list "/data/lists/custom.txt": No such file or directory (os error 2)
```

which is defect 3's fix demonstrating itself on an unrelated code path.

#### What this cost, and why

Defect 3 is the reason this took two attempts. Yesterday the only evidence was
reqwest's outer message, which reads identically for a DNS failure and a
connect failure, so "IPv6 was attempted" was *inferred* and written up as
observed. The inference then survived into a fix direction (happy-eyeballs)
that would have changed nothing, since hyper-util already does it by default.
Fixing the diagnostics first turned a day of bisecting into one log line.

### 3. MEDIUM — fetch errors discard their cause — **FIXED**

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

#### Error-chain fix as applied (2026-07-19)

`fah_common::error_chain(&dyn Error)` walks the `source()` chain and joins it,
skipping any layer whose own `Display` already interpolates its source (so
`LifecycleError::Fetch`'s `"fetch {url} failed: {source}"` does not stutter).
Applied at the three places an error stops being typed: the recorded
`RefreshResult::Failed` status, the scheduler's warning, and the API's
manual-refresh warning.

```text
before  fetch http://…/ failed: error sending request for url (http://…/)
after   fetch http://…/ failed: error sending request for url (http://…/):
        client error (Connect): tcp connect error: …refused it. (os error 10061)
```

`error_chain` lives in `fah-common` (L1) since `fah-rules` and `fah-api` both
need it; both edges point downward. Covered by unit tests in `fah-common` and
an assertion in `kill_the_network_keeps_previous_ruleset_and_surfaces_failure`
that the refused connection is named.

**Still missing:** `GET /api/v1/lists` maps `RefreshResult::Failed(_)` to the
bare string `"failed"` and drops the message, so this diagnostic is reachable
only through the container log. Exposing it would mean a new field in API.md's
list object — worth doing, not done here.

### 4. DESIGN / ADR — port 53 cannot be bound on RouterOS — **RESOLVED**

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

**Resolved (2026-07-19)** as [ADR-0004](../../decisions/0004-privileged-port-binding.md),
by measurement rather than preference. Option (c) was rejected outright: it
closes the LAN-IP + DHCP topology for every user, not only those who already
have a redirect. Option (a) — non-root plus `setcap` — was the better outcome
and was tested first. BuildKit preserves the capability xattr (verified by
decoding it out of the shipped layer); **RouterOS does not honour it**, and the
container died with the same `EACCES`. So option B: root at entry, bind 53,
drop to uid 65532 before serving.

Validated on the device:

```text
DNS listeners bound udp=0.0.0.0:53 tcp=0.0.0.0:53
dropped privileges after binding uid=65532 gid=65532
generated API key — store it now; it is not shown again
API listening url=https://0.0.0.0:8443
```

The order is the guarantee. `fah_dns::Server::bind` was split from
`Server::serve` for exactly this: previously `bind` spawned its listener tasks,
so any drop afterwards left a window where queries were answered as root. The
sockets now sit idle until the drop completes. The API key being written after
the drop confirms `/config` is writable as the service user.

**Not yet exercised:** the `chown` *adopt* branch. `/config` was already owned
by 65532 on this run, so the skip branch ran. A first boot onto a root-owned
volume remains untested.

### 5. DEPLOYMENT — RouterOS does not populate `/etc/resolv.conf` — **FIXED (2026-07-19), pending device check**

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

#### Fix as applied (2026-07-19)

Took the last sentence at its word rather than baking in a `resolv.conf`. The
fetcher now resolves list hosts through `[[dns.upstreams.servers]]` — the same
servers already answering client queries — so `/etc/resolv.conf` is never
consulted and the mount is gone from the deployment guide. This also removes
the hardcoded-resolver privacy question a baked-in file would have created:
list downloads go wherever the operator's upstreams point, encrypted if those
are DoT/DoH.

**Layering.** `fah-rules` is L2 and `fah-dns` is L3, so the fetcher cannot
import the DNS engine. `fah-rules` declares a `HostResolver` port; the binary
implements it over `UpstreamPool` in `adapters.rs`, next to the existing
`StatsSource`/`TelemetrySource` adapters. Arrows still point down only, and the
new port is documented in ARCHITECTURE.md §Dependency Layering.

**Two deliberate choices:**

- *Not through the pipeline.* `UpstreamPool::resolve_host` skips the Rule
  Engine and the cache. Routing it through the pipeline would let a blocklist
  block the host serving its own next copy — a self-inflicted, unrecoverable
  state short of hand-editing the config.
- *Either address family is enough.* `A` and `AAAA` go out concurrently and a
  failure or empty answer on one is not fatal. That is defect 2's root cause
  (musl's all-or-nothing `getaddrinfo`) fixed structurally rather than by
  swapping resolver libraries, and it directly covers the veth IPv6 trap in
  §3.1 of the deployment guide.

`ListManager::new` still uses the system resolver and is what tests and benches
use; the binary calls `ListManager::with_resolver`. Regression test
`list_downloads_go_through_the_injected_resolver` fetches from a `.invalid`
hostname — unresolvable by any system resolver — and was confirmed to fail
against `ListManager::new` before being kept.

### 6. LOW — status and id defects — **FIXED (2026-07-19), pending device check**

- **Stale status after boot.** Post-restart, `oisd-basic` reported
  `status=failed, rules=0, last_refresh=null` while the matcher was actively
  serving 55,866 rules loaded from that list's `/data` cache. Status does not
  reflect the ruleset in use.
- **`derive_id` collides.** `raw.githubusercontent.com/StevenBlack/hosts/.../hosts`
  and `.../1Hosts/master/Xtra/hosts.txt` both derive to `hosts`, producing
  `409 conflict`. Two unrelated lists cannot coexist without an explicit id.
- **Undocumented `id` field.** `POST /api/v1/lists` accepts `id`, which is what
  works around the collision, but API.md documents only
  `{url, enabled, refresh_hours}`. *(Documented 2026-07-19.)*
- **Nothing rejects two lists sharing a URL.** Only ids are checked for
  uniqueness, so `POST` without an `id` on a URL that is already configured
  under a different id succeeds and the same list is fetched, cached and
  compiled twice — double bandwidth, double heap, duplicate rules in the
  matcher. Hit accidentally on 2026-07-19: the shipped default carries
  `https://small.oisd.nl` as `oisd-basic`, and adding that URL without an `id`
  derived `small.oisd.nl` and was accepted. At ~30 MB per large list this is
  not a trivial waste on a 1 GB router. Either reject a duplicate URL with 409,
  or return the existing entry.

#### Fixes as applied (2026-07-19)

**Status.** The root cause was one field doing two jobs: `rules_total` was read
out of `last_result`, so any refresh failure reported zero rules for a list
whose rules were still compiled and still blocking — the API said "unprotected"
at exactly the moment protection was in fact intact. `ListStatus` now carries a
separate `compiled: Option<RefreshStats>` — what the list contributes to the
ruleset that is *serving* — and `last_result` keeps meaning "what the last
fetch did". `GET /api/v1/lists` reads the counts from the former and
`last_status` from the latter, so `"failed"` alongside a non-zero `rules_total`
is now the expected shape and is documented in API.md.

`compiled` is written by a single new `ListManager::swap_in`, which publishes
the matcher and records the per-list counts in the same step. All five compile
sites (boot, refresh, add, remove, user rules) go through it, so the reported
numbers cannot drift from the matcher actually installed; two of those sites
previously discarded the stats entirely. A list that drops out of a compile
(disabled, removed, cache file gone) has `compiled` cleared rather than keeping
a number it no longer earns.

**Duplicate URL.** `POST /api/v1/lists` now rejects a source already held by
another list with `409` naming that list, regardless of the `id` offered. The
check sits in the same `list_mutations`-guarded read-then-write section as the
id check, so it cannot race a concurrent add.

**Derived-id collision.** Left as-is — an explicit `id` is the right answer and
inventing a suffix would silently create near-identical ids. The `409` now says
the id was *derived* and points at the `id` field, so the caller is not left
wondering why a name they never chose is taken.

**Not changed:** boot-from-cache still reports `last_status: "ok"` with
`last_refresh: null`. That pair reads as "loaded from cache, not yet refreshed
this run", which is accurate; the counts, which were the misleading part, now
come from elsewhere.

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
| Startup to serving @1M | 1–3 s | **2.99 s** @1.19M | pass, no margin |
| Sustained throughput | ≥10k QPS | not measured | **not validated** |

Cost per rule: **33.2 bytes**. Memory was flat across a 2-minute observation at
full ruleset — no growth.

**Startup, measured 2026-07-19** (boot to API listening, 1,188,420 rules
compiled from `/data` cache):

```text
11:32:46.739  fastadhunter starting
11:32:49.721  ruleset compiled from cache rules=1188420
11:32:49.726  API listening
```

2.99 s against a 1–3 s budget: inside it, with no margin at all. Essentially
all of it is compiling the cached lists — the bind, drop and API start take
5 ms between them. This is startup-only work, but it is also how long a router
reboot leaves the LAN without filtering, so the budget is the right one to hold
and the current number does not survive a larger ruleset. Worth profiling
before Phase 2 adds to it.

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
