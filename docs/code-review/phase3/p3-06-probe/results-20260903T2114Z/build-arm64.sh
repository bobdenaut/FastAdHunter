#!/usr/bin/env bash
# arm64 image builds for the p3-06 campaign, run from the repo root on bobdenaut
# (Git Bash). Follows the header of each Dockerfile: buildx for linux/arm64 into a
# docker tar, then the skopeo docker-archive conversion RouterOS needs
# (docs/routeros-traps.md: legacy docker-archive, never OCI layout).
# Tars land in the repo root (gitignored *.tar); logs beside this script.
set -u
cd "$(dirname "$0")/../../../../.." || exit 1
OUT=docs/code-review/phase3/p3-06-probe/results-20260903T2114Z
TIP=$(git rev-parse --short=7 HEAD)
DIRTY=$(git status --porcelain --untracked-files=no | grep -vc phase2.6)
echo "tip=$TIP dirty_non_phase2.6=$DIRTY started=$(date -u +%FT%TZ)"
for spec in "fahprobe fah-probe" "p4 fah-p4" "splicebench fah-splicebench"; do
  set -- $spec
  df=$1
  name=$2
  echo "== $name build start $(date -u +%T)"
  docker buildx build --platform linux/arm64 \
    -f "docs/code-review/phase3/p3-06-probe/Dockerfile.$df" \
    -t "$name:$TIP" -o "type=docker,dest=$name-$TIP-arm64.tar" . \
    > "$OUT/build-$name.log" 2>&1
  echo "   build exit=$? $(date -u +%T)"
  echo "-- manifest layers:"
  tar -xOf "$name-$TIP-arm64.tar" manifest.json 2>/dev/null | head -c 300
  echo
  echo "-- skopeo convert"
  MSYS2_ARG_CONV_EXCL='*' docker run --rm -v 'E:\FastAdHunter:/work' quay.io/skopeo/stable copy \
    --insecure-policy "oci-archive:/work/$name-$TIP-arm64.tar" \
    "docker-archive:/work/$name-$TIP-rosready.tar:$name:$TIP" \
    > "$OUT/convert-$name.log" 2>&1
  echo "   convert exit=$?"
  ls -la "$name-$TIP-rosready.tar" 2>&1
done
echo "finished=$(date -u +%FT%TZ)"
