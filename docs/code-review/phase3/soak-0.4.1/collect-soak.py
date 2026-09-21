#!/usr/bin/env python3
"""0.4.1 seven-day soak collector. One invocation == one pull, one directory under --out.

Everything is raw JSON, one file per source, so the pull tree alone is enough to
recompute every derived number later (reduce.py).

API tier (Bearer key, https):
  every pull   telemetry (counters, listeners, upstreams, lists, ruleset),
               debug/memory (rss, peak, accounted, residual, faults, cpu ms),
               health, cache, stats, history/summary,
               history/perf incremental window (--perf-hours, default 1.2 h),
               lists
  daily 00Z    clients, history/top, config, policies, certificates,
               interception, rules/user
  --tag t0     every tier at once (baseline or backfill; pair with --perf-hours)

RouterOS tier (passwordless ssh, read-only, one session per pull):
  every pull   routeros-resource.json      /system/resource/get      cpu-load, free-memory, uptime
               routeros-cpu.json           /system/resource/cpu      per-core load/irq/disk
               routeros-health.json        /system/health            cpu-temperature
               routeros-container.json     /container/print          memory-current (bytes), cpu-usage
               routeros-log-problems.json  /log topics error|critical|warning
  daily        routeros-log-container.json /log topics container
  RouterOS serialises the whole batch with `:serialize to=json` (7.13+; the
  device runs 7.21.5). `memory-current` is the cgroup view of the container,
  the outside-the-process RSS the API cannot see.

meta.json carries the pull time, tiers, errors and the headline numbers.

Usage:
  python collect-soak.py --base https://host:8443 --token <KEY> --out pulls/ --ssh-host bobdenaut
  python collect-soak.py ... --tag t0 --perf-hours 12    # baseline with the series since boot
"""

import argparse
import datetime as dt
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request

JSON_ENDPOINTS = {
    "telemetry": "/api/v1/telemetry",
    "debug-memory": "/api/v1/debug/memory",
    "health": "/health",
    "cache": "/api/v1/cache",
    "stats": "/api/v1/stats",
    "history-summary": "/api/v1/history/summary",
    "lists": "/api/v1/lists",
    "clients": "/api/v1/clients",
    "history-top": "/api/v1/history/top",
    "config": "/api/v1/config",
    "policies": "/api/v1/policies",
    "certificates": "/api/v1/certificates",
    "interception": "/api/v1/interception",
    "rules-user": "/api/v1/rules/user",
}

EVERY = ["telemetry", "debug-memory", "health", "cache", "stats", "history-summary", "lists"]
DAILY = ["clients", "history-top", "config", "policies", "certificates", "interception", "rules-user"]

ROUTEROS_EVERY = {
    "resource": "/system/resource/get",
    "cpu": "/system/resource/cpu/print as-value",
    "health": "/system/health/print as-value",
    "container": "/container/print as-value",
    "log-problems": '/log print as-value where topics~"error" || topics~"critical" || topics~"warning"',
}
ROUTEROS_DAILY = {
    "log-container": '/log print as-value where topics~"container"',
}

SSH_NOISE = ("** WARNING: connection is not using a post-quantum",
             '** This session may be vulnerable to "store now',
             "** The server may need to be upgraded")


def iso(t):
    return t.strftime("%Y-%m-%dT%H:%M:%SZ")


def stamp(t):
    return t.strftime("%Y%m%dT%H%M%SZ")


def fetch(base, token, path, timeout=30):
    req = urllib.request.Request(base.rstrip("/") + path,
                                 headers={"Authorization": "Bearer " + token})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read().decode("utf-8", "replace")


def routeros_batch(host, queries, timeout=60):
    """One ssh session; RouterOS answers every query as one JSON object keyed like `queries`."""
    body = "; ".join("%s=[%s]" % (key.replace("-", ""), cmd) for key, cmd in queries.items())
    command = ":put [:serialize to=json value={%s}]" % body
    proc = subprocess.run(
        ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=15", host, command],
        capture_output=True, text=True, timeout=timeout)
    if proc.returncode != 0:
        raise RuntimeError("ssh rc=%d: %s" % (proc.returncode, (proc.stderr or "").strip()[:200]))
    text = "\n".join(l for l in proc.stdout.splitlines() if not l.startswith(SSH_NOISE))
    obj = json.loads(text)
    return {key: obj[key.replace("-", "")] for key in queries}


def write_json(outdir, name, obj):
    with open(os.path.join(outdir, name), "w", encoding="utf-8", newline="\n") as f:
        json.dump(obj, f, indent=1, sort_keys=True)


def save_json(outdir, name, text):
    try:
        obj = json.loads(text)
    except json.JSONDecodeError:
        with open(os.path.join(outdir, name.replace(".json", ".raw.txt")), "w",
                  encoding="utf-8", newline="\n") as f:
            f.write(text)
        return False
    write_json(outdir, name, obj)
    return True


def own_container(containers):
    for c in containers:
        if str(c.get("name", "")).startswith("fastadhunter"):
            return c
    return None


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--base", required=True)
    p.add_argument("--token", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--tag", default="")
    p.add_argument("--perf-hours", type=float, default=1.2,
                   help="width of the incremental history/perf window")
    p.add_argument("--ssh-host", default="",
                   help="RouterOS host for read-only queries; empty skips the tier")
    a = p.parse_args()

    now = dt.datetime.now(dt.timezone.utc)
    name = stamp(now) + ("-" + a.tag if a.tag else "")
    outdir = os.path.join(a.out, name)
    os.makedirs(outdir, exist_ok=True)

    daily = a.tag == "t0" or now.hour == 0
    wanted = EVERY + (DAILY if daily else [])

    errors = {}
    for key in wanted:
        try:
            if not save_json(outdir, key + ".json", fetch(a.base, a.token, JSON_ENDPOINTS[key])):
                errors[key] = "non-json payload"
        except (urllib.error.URLError, OSError, TimeoutError) as exc:
            errors[key] = repr(exc)

    frm = now - dt.timedelta(hours=a.perf_hours)
    path = "/api/v1/history/perf?from=%s&to=%s" % (iso(frm), iso(now + dt.timedelta(minutes=2)))
    try:
        if not save_json(outdir, "history-perf.json", fetch(a.base, a.token, path, 120)):
            errors["history-perf"] = "non-json payload"
    except (urllib.error.URLError, OSError, TimeoutError) as exc:
        errors["history-perf"] = repr(exc)

    routeros = {}
    if a.ssh_host:
        queries = dict(ROUTEROS_EVERY)
        if daily:
            queries.update(ROUTEROS_DAILY)
        try:
            routeros = routeros_batch(a.ssh_host, queries)
            for key, obj in routeros.items():
                write_json(outdir, "routeros-%s.json" % key, obj)
        except (RuntimeError, OSError, ValueError, subprocess.SubprocessError) as exc:
            errors["routeros"] = repr(exc)

    meta = {"pull_utc": iso(now), "tag": a.tag, "base": a.base, "ssh_host": a.ssh_host,
            "tiers": {"every": True, "daily": daily}, "perf_from": iso(frm), "errors": errors}

    def pick(file, *keys):
        try:
            obj = json.load(open(os.path.join(outdir, file), encoding="utf-8"))
            for k in keys:
                obj = obj[k]
            return obj
        except (OSError, json.JSONDecodeError, KeyError, TypeError):
            return None

    meta["version"] = pick("health.json", "version")
    meta["uptime_seconds"] = pick("health.json", "uptime_seconds")
    for k in ("process_rss", "process_peak_rss", "accounted_bytes", "residual_bytes",
              "major_page_faults", "cpu_user_ms", "cpu_system_ms"):
        meta[k] = pick("debug-memory.json", k)

    c = own_container(routeros.get("container") or [])
    if routeros and c is None:
        errors["routeros-container"] = "no fastadhunter container in /container/print"
    meta["container_memory_current"] = c.get("memory-current") if c else None
    meta["container_cpu_usage"] = c.get("cpu-usage") if c else None
    res = routeros.get("resource") or {}
    meta["router_free_memory"] = res.get("free-memory")
    meta["router_cpu_load"] = res.get("cpu-load")
    meta["router_uptime"] = res.get("uptime")
    temps = [h.get("value") for h in (routeros.get("health") or []) if h.get("name") == "cpu-temperature"]
    meta["router_cpu_temperature"] = temps[0] if temps else None
    write_json(outdir, "meta.json", meta)

    print("%s uptime=%s rss=%s residual=%s peak=%s container=%s router_free=%s cpu_load=%s temp=%s errors=%s" % (
        name, meta["uptime_seconds"], meta["process_rss"], meta["residual_bytes"],
        meta["process_peak_rss"], meta["container_memory_current"], meta["router_free_memory"],
        meta["router_cpu_load"], meta["router_cpu_temperature"], sorted(errors) or 0), flush=True)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
