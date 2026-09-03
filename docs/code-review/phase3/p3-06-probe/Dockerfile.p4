# syntax=docker/dockerfile:1
#
# Dockerfile.p4 — P4: DoT / DoH added latency vs UDP, in-device (plan
# §Measurements row P4, MA-8). The `encrypted_latency` harness
# (crates/fastadhunter/tests/encrypted_latency.rs, #[ignore], 3 interleaved
# rounds x 2 000 per transport, handshakes excluded) cross-compiled beside the
# release binary, both in one image; results in the container log.
#
# Names (plan §Scripts naming table): the harness runs as /fah-p4 and spawns
# /fah-probe (FAH_E2E_BINARY=/fah-probe), never `fastadhunter`. The harness
# boots the binary on loopback with ephemeral ports and an in-process mock
# upstream; it listens on nothing reachable and needs no mount.
#
# Two builds, two target dirs, so the measured binary is the plain release
# build. The harness is a test target, and fastadhunter's dev-dependency on
# fah-api enables `test-harness`, which fah-api refuses to compile into a
# release profile (compile_error! on `not(debug_assertions)`). The harness
# is therefore built in the release profile with debug assertions forced on
# through CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true: optimized timing loop,
# no manifest change, the feature never touches /fah-probe.
#
# UID — this image runs as 65532:65532 (declaration delta 10, recorded
# before the first P4 run). The harness creates its config / data volumes
# with tempfile (0700, owner = the harness uid) and the spawned binary is
# the uid it would drop to anyway; started as 65532 it performs no drop and
# binds ephemeral loopback ports, which need no privilege. /tmp is owned by
# that uid and TMPDIR points at it. The probe convention (USER 0:0 in
# Dockerfile.probe / Dockerfile.httpprobe) does not apply: those entrypoints
# never spawn a privilege-dropping child.
#
# Build from the repo root:
#
#   docker buildx build --platform linux/arm64 \
#     -f docs/code-review/phase3/p3-06-probe/Dockerfile.p4 \
#     -t fah-p4:<tip> -o type=docker,dest=fah-p4-<tip>-arm64.tar .
#   # then the skopeo docker-archive conversion as in Dockerfile.fahprobe
#
# Router side — proposed, owner runs:
#   /container/add file=kingston/fah-p4-<tip>-rosready.tar interface=veth3 \
#     root-dir=/kingston/p4/root logging=yes start-on-boot=no comment="fah-p4"
#   /container/start [find comment="fah-p4"]
#   /log print where topics~"container"
#   /container/remove [find comment="fah-p4"]

FROM rust:1.96.0-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY tui-monitor ./tui-monitor

RUN cargo build --release --locked -p fastadhunter

RUN CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo test --release --locked -p fastadhunter \
        --test encrypted_latency --no-run --target-dir /build/target-harness \
    && mkdir -p /out /seed/tmp \
    && cp "$(find /build/target-harness/release/deps -maxdepth 1 -type f -name 'encrypted_latency-*' ! -name '*.d' | head -n 1)" /out/fah-p4 \
    && test -x /out/fah-p4

FROM gcr.io/distroless/static-debian12:nonroot

COPY --from=builder /build/target/release/fastadhunter /fah-probe
COPY --from=builder /out/fah-p4 /fah-p4
COPY --from=builder --chown=65532:65532 /seed/tmp /tmp

ENV FAH_E2E_BINARY=/fah-probe
ENV TMPDIR=/tmp

USER 65532:65532

ENTRYPOINT ["/fah-p4", "--ignored", "--nocapture", "--test-threads=1", "--exact", "per_query_latency_udp_vs_dot_vs_doh"]
