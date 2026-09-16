#!/usr/bin/env python3
"""0.3.4 seven-day soak collector. One invocation == one pull.

Writes raw JSON/text only, one directory per pull, so the result tree alone is
enough to recompute every derived number later.

Tiers keep the tree small without losing fidelity:
  every pull   telemetry, debug/memory, health, cache, history/perf
               (incremental window), history/summary, lists, RouterOS resource
               + container detail + warning/error log
  daily        clients, stats, history/top, config, full container log

The RouterOS side is read-only (`print` only) and reached over passwordless
SSH; `memory-current` from `/container/print detail` is the outside-the-process
view of RSS that the API cannot see.

/api/v1/history/perf is the full-fidelity series (60 s rows, 30-day retention),
so the hourly incremental pull is a safety copy: even if the collector stops,
the rows survive on the device until retention prunes them.

Usage:
  python collect-soak.py --base https://host:8443 --token <KEY> --out pulls/
  python collect-soak.py ... --tag t0        # force every tier (baseline)
"""

import argparse
import datetime as dt
import json
import os
import re
import ssl
import subprocess
import sys
import urllib.error
import urllib.request

CONTAINER_ENTRY = re.compile(r"^ {0,4}\d+ +[A-Z]+ ", re.M)
OWN_CONTAINER = re.compile(r'name="fastadhunter[^"]*"')
MEMORY_CURRENT = re.compile(r"memory-current=(\S+)")


def own_container_memory(text):
    """`memory-current` of the fastadhunter container, or None.

    `/container/print detail` lists every container on the router, so the last
    `memory-current=` in the output belongs to whatever printed last, not
    necessarily to us. Entries start at a line-initial index, which is how the
    text is split back into one block per container.
    """
    starts = [m.start() for m in CONTAINER_ENTRY.finditer(text)]
    for i, start in enumerate(starts):
        end = starts[i + 1] if i + 1 < len(starts) else len(text)
        entry = text[start:end]
        if OWN_CONTAINER.search(entry):
            found = MEMORY_CURRENT.search(entry)
            return found.group(1) if found else None
    return None

JSON_ENDPOINTS = {
    "telemetry": "/api/v1/telemetry",
    "debug-memory": "/api/v1/debug/memory",
    "health": "/health",
    "cache": "/api/v1/cache",
    "history-summary": "/api/v1/history/summary",
    "lists": "/api/v1/lists",
    "clients": "/api/v1/clients",
    "stats": "/api/v1/stats",
    "history-top": "/api/v1/history/top",
    "config": "/api/v1/config",
    "policies": "/api/v1/policies",
}

EVERY = ["telemetry", "debug-memory", "health", "cache", "history-summary", "lists"]
DAILY = ["clients", "stats", "history-top", "config", "policies"]

ROUTEROS_EVERY = {
    "routeros-resource.txt": "/system/resource/print",
    "routeros-container.txt": "/container/print detail",
    "routeros-log-problems.txt": '/log print where topics~"error" || topics~"critical" || topics~"warning"',
}
ROUTEROS_DAILY = {
    "routeros-log-container.txt": '/log print where topics~"container"',
}


def iso(t):
    return t.strftime("%Y-%m-%dT%H:%M:%SZ")


def stamp(t):
    return t.strftime("%Y%m%dT%H%M%SZ")


def fetch(base, token, path, insecure, timeout=30):
    ctx = ssl._create_unverified_context() if insecure else None
    req = urllib.request.Request(base.rstrip("/") + path,
                                 headers={"Authorization": "Bearer " + token})
    with urllib.request.urlopen(req, timeout=timeout, context=ctx) as r:
        return r.read().decode("utf-8", "replace")


def ssh(host, command, timeout=45):
    proc = subprocess.run(
        ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=15", host, command],
        capture_output=True, text=True, timeout=timeout)
    if proc.returncode != 0:
        raise RuntimeError("ssh rc=%d: %s" % (proc.returncode, (proc.stderr or "").strip()[:200]))
    noisy = ("** WARNING: connection is not using a post-quantum",
             '** This session may be vulnerable to "store now',
             "** The server may need to be upgraded")
    return "\n".join(l for l in proc.stdout.splitlines()
                     if not l.startswith(noisy)) + "\n"


def save_text(outdir, name, text):
    with open(os.path.join(outdir, name), "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def save_json(outdir, name, text):
    try:
        obj = json.loads(text)
    except json.JSONDecodeError:
        save_text(outdir, name.replace(".json", ".raw.txt"), text)
        return False
    with open(os.path.join(outdir, name), "w", encoding="utf-8", newline="\n") as f:
        json.dump(obj, f, indent=1, sort_keys=True)
    return True


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--base", required=True)
    p.add_argument("--token", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--tag", default="")
    p.add_argument("--perf-hours", type=float, default=1.2,
                   help="width of the incremental history/perf window")
    p.add_argument("--ssh-host", default="",
                   help="RouterOS host for read-only print queries; empty skips them")
    p.add_argument("--insecure", action="store_true")
    a = p.parse_args()

    now = dt.datetime.now(dt.timezone.utc)
    name = stamp(now) + ("-" + a.tag if a.tag else "")
    outdir = os.path.join(a.out, name)
    os.makedirs(outdir, exist_ok=True)

    wanted = list(EVERY)
    daily = a.tag == "t0" or now.hour == 0
    if daily:
        wanted += DAILY

    errors = {}
    for key in wanted:
        try:
            ok = save_json(outdir, key + ".json",
                           fetch(a.base, a.token, JSON_ENDPOINTS[key], a.insecure))
            if not ok:
                errors[key] = "non-json payload (dashboard catch-all?)"
        except (urllib.error.URLError, OSError, TimeoutError) as exc:
            errors[key] = repr(exc)

    frm = now - dt.timedelta(hours=a.perf_hours)
    path = "/api/v1/history/perf?from=%s&to=%s" % (iso(frm), iso(now + dt.timedelta(minutes=2)))
    try:
        if not save_json(outdir, "history-perf.json",
                         fetch(a.base, a.token, path, a.insecure, 120)):
            errors["history-perf"] = "non-json payload (dashboard catch-all?)"
    except (urllib.error.URLError, OSError, TimeoutError) as exc:
        errors["history-perf"] = repr(exc)

    if a.ssh_host:
        queries = dict(ROUTEROS_EVERY)
        if daily:
            queries.update(ROUTEROS_DAILY)
        for name_, cmd in queries.items():
            try:
                save_text(outdir, name_, ssh(a.ssh_host, cmd))
            except (RuntimeError, OSError, subprocess.SubprocessError) as exc:
                errors[name_] = repr(exc)

    meta = {"pull_utc": iso(now), "tag": a.tag, "base": a.base,
            "ssh_host": a.ssh_host,
            "tiers": {"every": True, "daily": daily},
            "errors": errors}
    try:
        tel = json.load(open(os.path.join(outdir, "telemetry.json"), encoding="utf-8"))
        meta["version"] = tel.get("process", {}).get("version")
        meta["uptime_seconds"] = tel.get("process", {}).get("uptime_seconds")
        meta["process_rss"] = tel.get("memory", {}).get("process_rss")
        meta["process_peak_rss"] = tel.get("memory", {}).get("process_peak_rss")
    except (OSError, json.JSONDecodeError, KeyError):
        pass
    try:
        text = open(os.path.join(outdir, "routeros-container.txt"), encoding="utf-8").read()
        current = own_container_memory(text)
        if current is None:
            errors["routeros-container"] = "no fastadhunter container in /container/print detail"
        else:
            meta["container_memory_current"] = current
    except OSError as exc:
        errors["routeros-container"] = repr(exc)
    with open(os.path.join(outdir, "meta.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump(meta, f, indent=1, sort_keys=True)

    print("%s uptime=%s rss=%s peak=%s container=%s errors=%s" % (
        name, meta.get("uptime_seconds"), meta.get("process_rss"),
        meta.get("process_peak_rss"), meta.get("container_memory_current"),
        sorted(errors) or 0), flush=True)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
