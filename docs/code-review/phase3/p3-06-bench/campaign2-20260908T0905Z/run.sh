#!/usr/bin/env bash
# Campaign 2 dev-box bench session. A = main 857865d, B = phase3-06 tip.
# A/B/A/B interleaving per the pre-declaration; pinned arms use affinity 4 + High.
set -u
A=/e/FastAdHunter-main857
B=/e/FastAdHunter
OUT=$(cd "$(dirname "$0")" && pwd)

pinned() {                      # pinned <logfile> <cargo args...>
  local log="$1"; shift
  local win; win=$(cygpath -w "$log")
  local args
  args=$(printf "'%s'," "$@"); args="${args%,}"
  powershell -NoProfile -Command "
    \$p = Start-Process -FilePath 'cargo' -ArgumentList $args -PassThru -NoNewWindow \
      -RedirectStandardOutput '$win' -RedirectStandardError '$win.err'
    \$p.ProcessorAffinity = 4
    \$p.PriorityClass = 'High'
    \$p.WaitForExit()
    exit \$p.ExitCode" >/dev/null 2>&1
  echo "  pinned -> $(basename "$log") (exit $?)"
}

plain() {                       # plain <logfile> <cargo args...>
  local log="$1"; shift
  cargo "$@" > "$log" 2> "$log.err"
  echo "  plain  -> $(basename "$log") (exit $?)"
}

round() {                       # round <side> <dir> <r>
  local side="$1" dir="$2" r="$3"
  echo "== $side r$r =="
  cd "$dir" || return 1
  plain  "$OUT/D1D2-$side-r$r.txt" bench -p fah-http  --bench proxy   -- '^http_'
  pinned "$OUT/D3-$side-r$r.txt"   bench -p fah-dns   --bench cache   -- cache_hit_in_engine_latency
  pinned "$OUT/D4-$side-r$r.txt"   bench -p fastadhunter --bench pipeline \
          --config 'profile.bench.debug-assertions=true' -- 'blocked_query|forwarded_query_overhead'
  pinned "$OUT/D5-$side-r$r.txt"   bench -p fah-rules --bench matcher -- matcher_lookup
}

echo "session start $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "A = $(git -C $A rev-parse --short HEAD)   B = $(git -C $B rev-parse --short HEAD)"
git -C "$B" status --short | head -20

for r in 1 2; do
  round A "$A" "$r"
  round B "$B" "$r"
done

echo "== tip-only arms =="
cd "$B" || exit 1
plain  "$OUT/D6D7-tip.txt"  bench -p fah-http  --bench proxy     -- https_sni_splice
plain  "$OUT/D8D9D10-tip.txt" bench -p fah-http --bench intercept
pinned "$OUT/D11D12-tip.txt" bench -p fah-certs --bench certs

echo "session end $(date -u +%Y-%m-%dT%H:%M:%SZ)"
