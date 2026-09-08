#!/usr/bin/env bash
# D8 attribution: where does the spliced arm's extra time go?
# Same quantity D8 measures, but through the shipped binary, phase by phase.
# W = curl's timing marks, cumulative from start of the request.
set -u
N=${1:-30}
FMT='%{time_connect} %{time_appconnect} %{time_starttransfer} %{time_total}\n'

run() {                # run <label> <curl args...>
  local label="$1"; shift
  local c=() a=() s=() t=()
  for _ in $(seq 1 "$N"); do
    read -r x y z w < <(curl -sk --max-time 5 -o /dev/null -w "$FMT" "$@" 2>/dev/null)
    [ -z "${w:-}" ] && continue
    c+=("$x"); a+=("$y"); s+=("$z"); t+=("$w")
  done
  python - "$label" "${c[*]}" "${a[*]}" "${s[*]}" "${t[*]}" <<'PY'
import sys, statistics
label = sys.argv[1]
cols = [[float(v) * 1000 for v in col.split()] for col in sys.argv[2:6]]
if not cols[0]:
    print(f'{label:28s} no samples'); raise SystemExit
med = [statistics.median(c) for c in cols]
print(f'{label:28s} n={len(cols[0]):3d}  connect={med[0]:7.3f}  tls={med[1]:7.3f}  '
      f'firstbyte={med[2]:7.3f}  total={med[3]:7.3f}   '
      f'[tls-connect={med[1]-med[0]:6.3f}  fb-tls={med[2]-med[1]:6.3f}  total-fb={med[3]-med[2]:6.3f}] ms')
PY
}

echo "phase medians, ms, cumulative from request start; N=$N"
run "direct to origin"   --resolve 127-0-0-2.nip.io:443:127.0.0.2  -H 'Connection: close' https://127-0-0-2.nip.io/
run "through splice"     --connect-to 127-0-0-2.nip.io:443:127.0.0.1:8444 -H 'Connection: close' https://127-0-0-2.nip.io/
run "direct keep-alive"  --resolve 127-0-0-2.nip.io:443:127.0.0.2  https://127-0-0-2.nip.io/
run "splice keep-alive"  --connect-to 127-0-0-2.nip.io:443:127.0.0.1:8444 https://127-0-0-2.nip.io/
