# Deploying to RB5009 (RouterOS Container)

Stub — Phase 0 scope is the image itself; full on-device deployment and soak
testing is a Phase 1 task (`plan/open/phase1/p1-11-rb5009-deploy.md`).

## Prerequisites

- RouterOS with the `container` package enabled and a `veth`/bridge set up
  for the container's network namespace (RouterOS containers run isolated
  from the host network by default).
- An `arm64` image built via `docker buildx` (see [Dockerfile](../Dockerfile)),
  either pushed to a registry RouterOS can pull from, or exported as a tarball
  for `container-add file=`.

## Steps (to be completed in Phase 1)

1. Build and push the multi-arch image, or export the `arm64` layer as a
   tarball: `docker buildx build --platform linux/arm64 -o type=tar,dest=fastadhunter-arm64.tar .`
2. Transfer the tarball to the router (`/file` upload or `fetch`) if not
   pulling from a registry.
3. Create the `veth` interface and bridge port for the container.
4. `/container/mounts` for `/config` and `/data` pointing at persistent
   RouterOS storage.
5. `/container/add` with the image, mounts, `veth`, and root-dir; start it.
6. Confirm health: `/container/print` shows `running`, and the container's
   own healthcheck (self-exec, no shell available on RouterOS either) reports
   healthy.

## Open questions for Phase 1

- Exact RouterOS storage layout for `/data` (query log + cached lists) given
  the RB5009's shared 1GB RAM/flash budget (PERFORMANCE.md).
- Whether `container-add` on this RouterOS version supports the same
  `HEALTHCHECK` semantics as Docker, or whether liveness needs a RouterOS
  scheduler script polling `--healthcheck` separately.
