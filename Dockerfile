# syntax=docker/dockerfile:1
#
# Multi-arch (amd64 + arm64) static-musl build of `fastadhunter`, packaged in
# a distroless image (SECURITY.md §Container hardening: no shell, no package
# manager, non-root). Deployment target is RouterOS/RB5009 (arm64).
#
# Build (single platform, loads into local `docker images`):
#   docker buildx build --platform linux/amd64 -t fastadhunter:dev --load .
#
# Build (multi-arch, requires a registry to push to — see docs/deploy-rb5009.md
# and the one-time buildx builder setup below):
#   docker buildx build --platform linux/amd64,linux/arm64 \
#     -t <registry>/fastadhunter:0.1.0 --push .
#
# One-time buildx setup (per host):
#   docker buildx create --name fah-builder --use
#   docker buildx inspect --bootstrap
# On Linux hosts without built-in cross-arch emulation, also register QEMU:
#   docker run --privileged --rm tonistiigi/binfmt --install all
# (Docker Desktop on Windows/macOS ships this already.)
#
# No target flag or per-arch toolchain is needed: Alpine's default libc is
# musl, so `cargo build --release` produces a static musl binary natively for
# whichever architecture buildx is running this stage under (native on the
# host arch, QEMU-emulated otherwise) — same Dockerfile, both platforms.

# Keep this tag's Rust version in sync with rust-toolchain.toml.
FROM rust:1.96.0-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /build
COPY . .

RUN cargo build --release --locked -p fastadhunter

# Empty dirs, owned by the nonroot uid/gid, to seed /config and /data below.
# A Docker named volume with nothing in it yet is initialized from whatever
# already exists at that path in the image — without this, Docker creates
# the mountpoint owned by root and the nonroot binary can't write its
# first-boot config there.
RUN mkdir -p /seed/config /seed/data

FROM gcr.io/distroless/static-debian12:nonroot

COPY --from=builder /build/target/release/fastadhunter /fastadhunter
COPY --from=builder --chown=65532:65532 /seed/config /config
COPY --from=builder --chown=65532:65532 /seed/data /data

# Non-root user baked into the `:nonroot` distroless variant (uid/gid 65532).
USER nonroot:nonroot

VOLUME ["/config", "/data"]
EXPOSE 53/udp 53/tcp 8443/tcp

# No shell in this image — the container self-execs the binary to probe
# itself (SECURITY.md: "self-probe; no shell tools").
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["/fastadhunter", "--healthcheck"]

ENTRYPOINT ["/fastadhunter"]
