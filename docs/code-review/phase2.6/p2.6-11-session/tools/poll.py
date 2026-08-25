#!/usr/bin/env python3
"""Fine-grained telemetry sampler for the P2.6-11 L.4 arms.

The probe records perf every 30 s, which cannot resolve the first penalty — and
the first penalty is the whole of L.4b. This polls /api/v1/telemetry on a short
interval and writes one compact JSON line per sample, so the transition
penalties 0 -> 1 can be read together with the dead endpoint's attempt count at
that instant.

  python poll.py --base https://172.17.0.4:8443 --token <t> --insecure \
      --interval 0.25 --seconds 60 --out l4a-first60.jsonl
"""

import argparse
import json
import ssl
import sys
import time
import urllib.request

def get(base, token, path, insecure):
    ctx = ssl._create_unverified_context() if insecure else None
    req = urllib.request.Request(base.rstrip("/") + path,
                                 headers={"Authorization": "Bearer " + token})
    with urllib.request.urlopen(req, timeout=10, context=ctx) as r:
        return json.loads(r.read().decode())

def sample(telemetry):
    return {
        "uptime_s": telemetry.get("process", {}).get("uptime_seconds"),
        "upstreams": [
            {
                "address": u.get("address"),
                "state": u.get("state"),
                "attempts": u.get("attempts"),
                "failures": u.get("failures"),
                "penalties": u.get("penalties"),
                "probes": u.get("probes"),
                "penalty_round": u.get("penalty_round"),
            }
            for u in telemetry.get("upstreams", [])
        ],
    }

def main():
    p = argparse.ArgumentParser()
    p.add_argument("--base", required=True)
    p.add_argument("--token", required=True)
    p.add_argument("--interval", type=float, default=0.25)
    p.add_argument("--seconds", type=int, required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--insecure", action="store_true", default=True)
    a = p.parse_args()

    start = time.time()
    deadline = start + a.seconds
    taken = failed = 0

    with open(a.out, "w", encoding="utf-8") as log:
        while time.time() < deadline:
            tick = time.time()
            try:
                row = sample(get(a.base, a.token, "/api/v1/telemetry", a.insecure))
                row["offset_s"] = round(tick - start, 4)
                taken += 1
            except Exception as err:
                row = {"offset_s": round(tick - start, 4), "error": str(err)}
                failed += 1
            log.write(json.dumps(row, sort_keys=True) + "\n")
            log.flush()
            rest = a.interval - (time.time() - tick)
            if rest > 0:
                time.sleep(rest)

    json.dump({"samples": taken, "errors": failed,
               "elapsed_s": round(time.time() - start, 2)},
              sys.stdout, indent=1, sort_keys=True)
    print()

if __name__ == "__main__":
    main()
