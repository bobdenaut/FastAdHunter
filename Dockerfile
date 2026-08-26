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
#
# A C compiler *is* required, for mimalloc (see `crates/fastadhunter/src/allocator.rs`). Both halves of that are
# already in this stage — see the `apk add` below.

# The web UI (`/web`), built on the *build host's* architecture. Its output is
# static files with no architecture, and without --platform=$BUILDPLATFORM the
# arm64 build would run the whole Node toolchain under QEMU for nothing.
#
# Node is a build-time dependency only: nothing from this stage reaches the
# runtime image except the emitted files.
#
# The bundle carries no version string: the top bar reads `version` from
# `GET /health` at runtime, so nothing here needs a build argument.
FROM --platform=$BUILDPLATFORM node:22.21.1-alpine AS frontend

WORKDIR /app

# Manifest first, so `npm ci` caches across every source-only edit.
COPY dashboard/frontend/package.json dashboard/frontend/package-lock.json ./
RUN npm ci

COPY dashboard/frontend/ ./

# `npm run build` typechecks, builds, writes the `.gz`/`.br` siblings and
# enforces the 150 KB gzip budget. A bundle over budget fails the image build,
# not merely a local check.
RUN npm run build && mkdir -p /web && cp -R dist/. /web/

# Keep this tag's Rust version in sync with rust-toolchain.toml.
FROM rust:1.96.0-alpine AS builder

# Load-bearing, not vestigial: `libmimalloc-sys` compiles mimalloc's C via the
# `cc` crate (see `crates/fastadhunter/src/allocator.rs`), and needs libc headers to do it. The compiler itself
# (`/usr/bin/gcc`) already ships in the rust:*-alpine base image; `musl-dev` is
# what supplies the headers and `libc.a` alongside it. Remove this and the build
# fails in the build script, not at link time.
RUN apk add --no-cache musl-dev

WORKDIR /build
# Scoped rather than `COPY . .`, so an edit under `dashboard/` cannot invalidate
# this stage's layer cache and force a full Rust rebuild. These four entries are
# everything `cargo build --locked -p fastadhunter` reads: the workspace
# manifest and lockfile, the toolchain pin, and both `members` globs
# (`crates/*`, `tui-monitor`).
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY tui-monitor ./tui-monitor

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
# Image content, never a volume (VOLUME below stays /config + /data): the UI and
# the API version and deploy as one artifact, so a rollback can never pair an
# older binary with a newer UI. Left root-owned — the serving uid only reads it.
COPY --from=frontend /web /web

# Root at entry, by design (ADR-0004): the process binds port 53 — which
# RouterOS permits no other way, having no `cap-add`, no lowered
# `net.ipv4.ip_unprivileged_port_start`, and no respect for the
# `CAP_NET_BIND_SERVICE` file capability on import — then permanently drops to
# uid/gid 65532 before answering a single query, as bind9 and unbound do.
#
# This must be stated explicitly. The `:nonroot` base tag carries its own
# `USER 65532`, so *omitting* a `USER` line here silently inherits it and the
# bind fails with EACCES. The base tag stays `:nonroot` deliberately — 65532
# is the uid the binary drops to and the owner `/config` and `/data` are
# seeded with below — so this line and that tag have to disagree on purpose.
#
# Image scanners will flag this as root-by-default: true of the entrypoint,
# false of the serving process.
USER 0:0

VOLUME ["/config", "/data"]
EXPOSE 53/udp 53/tcp 8443/tcp

# No shell in this image — the container self-execs the binary to probe
# itself (SECURITY.md: "self-probe; no shell tools").
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["/fastadhunter", "--healthcheck"]

ENTRYPOINT ["/fastadhunter"]
