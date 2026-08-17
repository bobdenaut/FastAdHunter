# Binding port 53 without a privileged runtime

DNS must listen on port 53, which is privileged on Linux. The container runs as
non-root (uid 65532, distroless `:nonroot`), and SECURITY.md claims it "requires
no capabilities beyond binding its ports" — a claim that is true under Docker
only by accident: Docker sets `net.ipv4.ip_unprivileged_port_start=0`, so 53 is
not privileged there at all.

RouterOS does not set that sysctl and exposes no `cap-add`. On the RB5009 the
process honours `USER nonroot`, compiles its ruleset, then dies binding 53 with
`EACCES` (p1-11, [code-review](../code-review/phase1/p1-11-review.md) defect 4). This
affects every RouterOS deployment. Falling back to a high port plus a dst-nat
redirect works, but closes the simplest topology — give the container a LAN IP
and hand it out over DHCP — for everyone, while the obvious competitor
(`adguard/adguardhome`, which ships as root) needs no router configuration at
all.

A process cannot escalate, so this is a build-time choice, not something the
binary can detect and recover from: by the time a bind returns `EACCES`, being
non-root is already irreversible. Two strategies are viable.

**A — non-root image, `setcap cap_net_bind_service=+ep` on the binary.** The
process is never privileged, not even briefly, and no privilege-dropping code
exists to get wrong. It depends on the `security.capability` xattr surviving two
separate gates: BuildKit's `COPY --from` out of the builder stage, and
RouterOS's own image import. If either strips it, there is no recovery path —
the container simply cannot serve DNS.

**B — root image, drop to uid 65532 immediately after bind.** Works on every
platform regardless of xattr handling, sysctls or `cap-add` support. This is
what bind9, unbound and dnsmasq all do. The process is root only for the window
between `execve` and `setuid`, before a single query is accepted. Costs a `libc`
dependency and an `unsafe` block whose ordering (`setgroups` → `setgid` →
`setuid`, then verify the drop is irreversible) is security-critical, and image
scanners will flag the image as root-by-default regardless of what it does at
runtime.

We ship one image, not both. Two distributions would push a platform question
onto users that neither variant requires them to understand.

The runtime drop is worth implementing either way, since it is correct in both:

```text
bind sockets
if euid == 0 { setgroups([]); setgid(gid); setuid(uid); verify irreversible }
serve
```

Under A that branch never executes; under B it always does. It also keeps a
`USER root` image behaving correctly for anyone who overrides the user back to
non-root on Docker.

## Decision

**B — root image, drop to uid 65532 immediately after bind.** Decided
2026-07-19 by measurement, not preference: A was the better outcome and was
tested first.

Gate 1 passed. BuildKit preserves the `security.capability` xattr through
`COPY --from`, verified twice — with a standalone probe, and by decoding the
xattr out of the shipped layer itself (`magic 0x02000001`,
`VFS_CAP_REVISION_2` with the effective bit set, permitted mask `0x00000400` =
`CAP_NET_BIND_SERVICE`).

Gate 2 failed. That image, imported on the RB5009 and started with no port
override, still died binding 53:

```text
INFO  ruleset compiled from cache rules=55866
ERROR fastadhunter failed to start error=Permission denied (os error 13)
```

The capability was demonstrably present in the archive, so RouterOS either
strips it on import or lands the rootfs somewhere file capabilities are
ignored. The two are not distinguished — the image is distroless and has no
shell to inspect from — and the distinction does not change the outcome.

SECURITY.md's "requires no capabilities beyond binding its ports" is wrong as
written and must be corrected to describe the bind-then-drop sequence.

## Revisit criteria

Reopen if MikroTik begins preserving file capabilities on import (which would
make A viable where B was chosen), if RouterOS gains `cap-add`, or if the
distroless base changes its uid/gid convention.
