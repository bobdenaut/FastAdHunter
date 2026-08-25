#!/usr/bin/env python3
"""P2.6-10 / suite S1-N collector. One invocation == one repetition.

Snapshots /api/v1/telemetry before and after a fixed-duration load, then pulls
the /api/v1/history/perf rows covering the window. Writes raw JSON only --
every derived number is recomputed by analyze.py, so the result directory alone
is enough to reproduce N.

Usage:
  python collect.py --base https://172.17.0.4:8443 --token <KEY> \
      --label A --rep 1 --seconds 600 --out runs/ \
      --load "docker run --rm ... <generator command>"

--load is run as a subprocess for the whole measured window; its stdout and
stderr are captured verbatim (the generator, not the server, is the authority
on offered and achieved QPS).
"""

import argparse
import datetime as dt
import json
import os
import ssl
import subprocess
import sys
import time
import urllib.request

def iso(t):
    return dt.datetime.fromtimestamp(t, dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

def get(base, token, path, insecure):
    ctx = ssl._create_unverified_context() if insecure else None
    req = urllib.request.Request(base.rstrip("/") + path,
                                 headers={"Authorization": "Bearer " + token})
    with urllib.request.urlopen(req, timeout=30, context=ctx) as r:
        return json.loads(r.read().decode())

def save(outdir, name, obj):
    with open(os.path.join(outdir, name), "w", encoding="utf-8") as f:
        json.dump(obj, f, indent=1, sort_keys=True)

def main():
    p = argparse.ArgumentParser()
    p.add_argument("--base", required=True)
    p.add_argument("--token", required=True)
    p.add_argument("--label", required=True, choices=["A", "B", "W"])
    p.add_argument("--rep", required=True, type=int)
    p.add_argument("--seconds", required=True, type=int)
    p.add_argument("--out", required=True)
    p.add_argument("--load", required=True)
    p.add_argument("--settle", type=int, default=60)
    p.add_argument("--drain", type=int, default=30)
    p.add_argument("--insecure", action="store_true", default=True)
    a = p.parse_args()

    outdir = os.path.join(a.out, "rep%02d-%s" % (a.rep, a.label))
    os.makedirs(outdir, exist_ok=True)

    print("[rep %02d %s] settle %ds" % (a.rep, a.label, a.settle), flush=True)
    time.sleep(a.settle)

    cfg = get(a.base, a.token, "/api/v1/config", a.insecure)
    save(outdir, "config.json", cfg)

    t0_wall = time.time()
    t0 = get(a.base, a.token, "/api/v1/telemetry", a.insecure)
    save(outdir, "t0-telemetry.json", t0)

    print("[rep %02d %s] load %ds" % (a.rep, a.label, a.seconds), flush=True)
    load_t0 = time.time()
    proc = subprocess.run(a.load, shell=True, capture_output=True, text=True,
                          timeout=a.seconds + 300)
    load_t1 = time.time()

    # `upstreams[].attempts` and `counters.swr.*` are republished into the
    # metrics registry by the binary's 10 s telemetry poll, so they lag live
    # traffic. Drain quietly past at least one tick before T1, or the attempts
    # delta carries up to (10 s x QPS) of sampling artefact that would read as
    # harness noise on the metric the S1-G2 fixed order picks first.
    print("[rep %02d %s] drain %ds" % (a.rep, a.label, a.drain), flush=True)
    time.sleep(a.drain)

    t1 = get(a.base, a.token, "/api/v1/telemetry", a.insecure)
    t1_wall = time.time()
    save(outdir, "t1-telemetry.json", t1)

    # Widen by one sample interval on each side so no row inside the window is
    # clipped; analyze.py filters strictly on ts.
    perf = get(a.base, a.token,
               "/api/v1/history/perf?from=%s&to=%s" % (iso(t0_wall - 120), iso(t1_wall + 120)),
               a.insecure)
    save(outdir, "perf.json", perf)

    save(outdir, "meta.json", {
        "label": a.label, "rep": a.rep,
        "t0_utc": iso(t0_wall), "t1_utc": iso(t1_wall),
        "load_t0_utc": iso(load_t0), "load_t1_utc": iso(load_t1),
        "load_seconds": load_t1 - load_t0,
        "drain_seconds": a.drain,
        "elapsed_seconds": t1_wall - t0_wall,
        "load_command": a.load,
        "load_returncode": proc.returncode,
        "process_version_t0": t0.get("process", {}).get("version"),
        "process_version_t1": t1.get("process", {}).get("version"),
        "uptime_t0": t0.get("process", {}).get("uptime_seconds"),
        "uptime_t1": t1.get("process", {}).get("uptime_seconds"),
    })
    with open(os.path.join(outdir, "load-stdout.txt"), "w", encoding="utf-8") as f:
        f.write(proc.stdout or "")
    with open(os.path.join(outdir, "load-stderr.txt"), "w", encoding="utf-8") as f:
        f.write(proc.stderr or "")

    print("[rep %02d %s] done -> %s (load rc=%d)" % (a.rep, a.label, outdir, proc.returncode),
          flush=True)
    return 0

if __name__ == "__main__":
    sys.exit(main())
