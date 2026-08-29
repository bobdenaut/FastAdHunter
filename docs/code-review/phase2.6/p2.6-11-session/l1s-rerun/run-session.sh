#!/bin/sh
set -eu
SP="C:/Users/liviu/AppData/Local/Temp/claude/e--FastAdHunter/285521c3-817e-4400-8079-2b03ae41928c/scratchpad/l1s"
TOOLS="e:/FastAdHunter/docs/code-review/phase2.6/p2.6-11-session/tools"
GEN="python $TOOLS/gen.py --server 127.0.0.1 --port 25353 --qfile $SP/config/l1s-fwd.txt --ctlfile $SP/config/l1s-ctl.txt --qps 500 --ctl-qps 1000"
BASE="https://127.0.0.1:28443"

start_fah() {
    strategy="$1"
    docker rm -f fah-l1s >/dev/null 2>&1 || true
    docker run -d --name fah-l1s --network fah-l1s-net --ip 172.30.1.20 \
        -v "$SP/config:/config" -v "$SP/data:/data" \
        -e FAH__DNS__UPSTREAMS__STRATEGY="$strategy" \
        -p 25353:53/udp -p 28443:8443 \
        fastadhunter:c042840 >/dev/null
    sleep 8
    KEY=$(cat "$SP/data/../config/apikey" 2>/dev/null || true)
    if [ -z "$KEY" ]; then
        KEY=$(docker logs fah-l1s 2>&1 | sed -n 's/.*api_key=\([0-9a-f]*\).*/\1/p' | head -1)
    fi
    echo "$KEY" > "$SP/key.txt"
    for i in $(seq 1 30); do
        if curl -sk --max-time 3 "$BASE/health" | grep -q '"ok"'; then break; fi
        sleep 2
    done
    curl -sk --max-time 5 -H "Authorization: Bearer $KEY" "$BASE/api/v1/config" | grep -o '"strategy": *"[a-z]*"' || true
}

rep() {
    label="$1"; num="$2"; secs="$3"
    KEY=$(cat "$SP/key.txt")
    python "$TOOLS/collect.py" --base "$BASE" --token "$KEY" \
        --label "$label" --rep "$num" --seconds "$secs" \
        --out "$SP/out" --load "$GEN --seconds $secs"
}

echo "=== arm F (fallback) ==="
start_fah fallback
rep W 0 120
rep B 1 300
rep B 2 300
rep B 3 300

echo "=== arm A (adaptive) ==="
start_fah adaptive
rep W 9 120
rep A 4 300
rep A 5 300
rep A 6 300

docker rm -f fah-l1s >/dev/null 2>&1 || true
echo "SESSION DONE"
