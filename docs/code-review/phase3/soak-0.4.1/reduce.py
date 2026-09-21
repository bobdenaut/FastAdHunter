#!/usr/bin/env python3
"""Recompute every derived figure of the 0.4.1 soak from pulls/ alone. Prints markdown.

Usage:
  python reduce.py [section ...] [--compare FILE --compare-boot ISO] [--days N]

Sections (default all):
  pulls      what landed, errors, gaps in the hourly pulls
  perf       the merged history/perf series since boot: rows, coverage, gaps
  residual   THE question: residual_bytes per UTC day, quiet-night and same-hour
             floors, 12 h floors, linear fit in MiB/day, day-over-day deltas
  peak       process_peak_rss per day and every time it moved
  accounted  ruleset/cache/aggregates/clients at first and last row, cache bounds
  router     container memory-current, router free-memory, cpu-load, temperature
  upstreams  per-upstream attempts/failures/penalties, non-healthy rows
  counters   first vs last telemetry: listeners, dns, http, swr, faults, cpu ms
  traffic    queries/blocked/qps and concurrent connections per day
  problems   RouterOS error/critical/warning log lines seen since boot
  verdict    the numbers the reading rests on, with the thresholds spelled out

--compare FILE --compare-boot ISO   run the residual table and fit on another
  history/perf dump (the 0.3.4 series, pulled from the router while retention
  keeps it) so both slopes come from the same code.
--days N   only the first N days since boot (a read at h24 passes 1, h72 passes 3).
--warmup-hours H   rows closer than H hours to boot are left out of the residual
  and peak floors (default 1: the boot sample precedes the first list refresh).

Every figure is MiB unless the header says otherwise. Day = UTC calendar day.
"""

import argparse
import glob
import json
import math
import os
import sys
from collections import defaultdict
from datetime import datetime, timedelta, timezone

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "pulls")
BOOT = datetime(2026, 9, 21, 7, 14, 50, tzinfo=timezone.utc)
MiB = 1 << 20
QUIET = (0, 5)
SAME_HOUR = (7, 8)
CLIMB_MIB_PER_DAY = 1.0
FLAT_MIB_PER_DAY = 0.5
FLAT_SPREAD_MIB = 1.5


def ts(s):
    return datetime.strptime(s, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)


def load(path):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, json.JSONDecodeError):
        return None


def mib(b):
    return None if b is None else b / MiB


def f1(x):
    return "—" if x is None or (isinstance(x, float) and math.isnan(x)) else "%.1f" % x


def f2(x):
    return "—" if x is None or (isinstance(x, float) and math.isnan(x)) else "%.2f" % x


def fit(points):
    n = len(points)
    if n < 2:
        return float("nan"), float("nan"), n
    mx = sum(x for x, _ in points) / n
    my = sum(y for _, y in points) / n
    sxx = sum((x - mx) ** 2 for x, _ in points)
    if sxx == 0:
        return float("nan"), float("nan"), n
    b = sum((x - mx) * (y - my) for x, y in points) / sxx
    a = my - b * mx
    syy = sum((y - my) ** 2 for _, y in points)
    ss = sum((y - (a + b * x)) ** 2 for x, y in points)
    return b, (1 - ss / syy if syy else float("nan")), n


def table(header, rows):
    print("| " + " | ".join(header) + " |")
    print("|" + "|".join(" --- " for _ in header) + "|")
    for r in rows:
        print("| " + " | ".join(str(c) for c in r) + " |")
    print()


def h1(title):
    print("## " + title)
    print()


# ---------------------------------------------------------------- load

def load_pulls(days):
    pulls = []
    for d in sorted(glob.glob(os.path.join(ROOT, "*"))):
        meta = load(os.path.join(d, "meta.json"))
        if not meta or not os.path.isdir(d):
            continue
        t = ts(meta["pull_utc"])
        if days and t > BOOT + timedelta(days=days):
            continue
        pulls.append(dict(
            name=os.path.basename(d), dir=d, t=t, h=(t - BOOT).total_seconds() / 3600, meta=meta,
            tel=load(os.path.join(d, "telemetry.json")),
            mem=load(os.path.join(d, "debug-memory.json")),
            plog=load(os.path.join(d, "routeros-log-problems.json")) or [],
            errors=meta.get("errors") or {}))
    return pulls


def merge_perf(pulls, days):
    rows = {}
    for pu in pulls:
        hp = load(os.path.join(pu["dir"], "history-perf.json"))
        for it in (hp or {}).get("items", []):
            t = ts(it["ts"])
            if t >= BOOT and (not days or t <= BOOT + timedelta(days=days)):
                rows[t] = it
    return [rows[k] for k in sorted(rows)]


def residual_of(it):
    m = it.get("memory")
    return mib(m["residual_bytes"]) if m else None


def by_day(rows, key):
    days = defaultdict(list)
    for it in rows:
        v = key(it)
        if v is not None:
            days[it["ts"][:10]].append((ts(it["ts"]), v))
    return dict(sorted(days.items()))


# ---------------------------------------------------------------- sections

def sec_pulls(pulls, rows):
    h1("Pulls")
    if not pulls:
        print("no pulls under %s\n" % ROOT)
        return
    table(["pulls", "first", "last", "hours since boot", "with errors", "daily-tier pulls"], [[
        len(pulls), pulls[0]["name"], pulls[-1]["name"], f1(pulls[-1]["h"]),
        sum(1 for p in pulls if p["errors"]),
        sum(1 for p in pulls if p["meta"].get("tiers", {}).get("daily"))]])
    bad = [(p["name"], ", ".join(sorted(p["errors"]))) for p in pulls if p["errors"]]
    if bad:
        table(["pull", "errors"], bad)
    gaps = [(a["name"], b["name"], f1((b["t"] - a["t"]).total_seconds() / 3600))
            for a, b in zip(pulls, pulls[1:]) if (b["t"] - a["t"]) > timedelta(hours=1.5)]
    if gaps:
        table(["gap after", "next pull", "hours"], gaps)


def sec_perf(rows):
    h1("history/perf since boot")
    if not rows:
        print("no rows\n")
        return
    first, last = ts(rows[0]["ts"]), ts(rows[-1]["ts"])
    span_h = (last - first).total_seconds() / 3600
    expected = span_h * 10
    table(["rows", "first", "last", "hours", "coverage vs 360 s"], [[
        len(rows), rows[0]["ts"], rows[-1]["ts"], f1(span_h),
        "%.0f%%" % (100 * len(rows) / expected) if expected else "—"]])
    gaps = [(a["ts"], b["ts"], f1((ts(b["ts"]) - ts(a["ts"])).total_seconds() / 60))
            for a, b in zip(rows, rows[1:]) if ts(b["ts"]) - ts(a["ts"]) > timedelta(minutes=15)]
    if gaps:
        table(["gap after", "next row", "minutes"], gaps)
    days = by_day(rows, lambda it: 1)
    table(["day", "rows"], [(d, len(v)) for d, v in days.items()])


def residual_table(rows, boot, label):
    days = by_day(rows, residual_of)
    if not days:
        print("no residual_bytes in %s (build without memory breakdown?)\n" % label)
        return None
    out, mins = [], []
    for i, (d, pts) in enumerate(days.items()):
        vals = [v for _, v in pts]
        quiet = [v for t, v in pts if QUIET[0] <= t.hour < QUIET[1]]
        same = [v for t, v in pts if SAME_HOUR[0] <= t.hour < SAME_HOUR[1]]
        rss = [mib(it["rss_bytes"]) for it in rows if it["ts"][:10] == d]
        day_idx = (pts[0][0].replace(hour=0, minute=0, second=0) - boot.replace(hour=0, minute=0, second=0)).days
        mins.append((day_idx, min(vals)))
        prev = out[-1][2] if out else None
        out.append([d, len(vals), min(vals), sorted(vals)[len(vals) // 2], max(vals),
                    min(quiet) if quiet else None, min(same) if same else None,
                    min(rss), max(rss), (min(vals) - prev) if prev is not None else None])
    table(["day", "n", "res min", "res p50", "res max", "quiet 00–05Z min", "same-hour 07–08Z min",
           "rss min", "rss max", "Δ min vs prev"],
          [[r[0], r[1]] + [f2(x) if isinstance(x, float) else ("—" if x is None else x) for x in r[2:]] for r in out])
    b, r2, n = fit(mins)
    last3 = fit(mins[-3:]) if len(mins) >= 3 else (float("nan"), float("nan"), len(mins))
    halves = defaultdict(list)
    for it in rows:
        v = residual_of(it)
        if v is not None:
            halves[int((ts(it["ts"]) - boot).total_seconds() // 43200)].append(v)
    hb, hr2, hn = fit([(k, min(v)) for k, v in sorted(halves.items())])
    table(["%s fit" % label, "MiB/day", "R²", "points"], [
        ["daily minimum, all days", f2(b), f2(r2), n],
        ["daily minimum, last 3 days", f2(last3[0]), f2(last3[1]), last3[2]],
        ["12 h floors (slope ×2 = per day)", f2(hb * 2 if not math.isnan(hb) else hb), f2(hr2), hn]])
    return dict(mins=mins, slope=b, r2=r2, last3=last3[0], half_slope=hb * 2 if not math.isnan(hb) else hb)


def sec_residual(rows, compare, compare_boot, warmup):
    h1("Residual per day")
    print("Rows in the first %.1f h after boot are excluded from every floor: the boot sample sits "
          "below the first list refresh and is not a floor.\n" % warmup)
    res = residual_table(rows, BOOT, "0.4.1")
    if compare:
        cmp_rows = [it for it in (load(compare) or {}).get("items", []) if ts(it["ts"]) >= compare_boot]
        cmp_rows.sort(key=lambda it: it["ts"])
        print("### Comparison series: %s (%d rows since %s)\n" % (compare, len(cmp_rows), compare_boot.isoformat()))
        residual_table(cmp_rows, compare_boot, "compare")
    return res


def sec_peak(rows):
    h1("Peak RSS")
    days = by_day(rows, lambda it: mib(it["peak_rss"]))
    table(["day", "peak max", "rss max"],
          [(d, f2(max(v for _, v in pts)), f2(max(mib(it["rss_bytes"]) for it in rows if it["ts"][:10] == d)))
           for d, pts in days.items()])
    moves, prev = [], None
    for it in rows:
        pk = mib(it["peak_rss"])
        if prev is not None and pk > prev + 0.05:
            moves.append((it["ts"], f2(prev), f2(pk), f2(mib(it["rss_bytes"])), f2(residual_of(it))))
        prev = pk
    if moves:
        table(["peak moved at", "from", "to", "rss then", "residual then"], moves)
    else:
        print("peak never moved after the first row\n")


def sec_accounted(rows):
    h1("Accounted breakdown")
    if not rows or not rows[0].get("memory"):
        print("no memory breakdown\n")
        return
    keys = ["ruleset_bytes", "cache_estimated_bytes", "stats_aggregates_bytes", "stats_clients_bytes",
            "accounted_bytes", "residual_bytes"]
    a, b = rows[0]["memory"], rows[-1]["memory"]
    table(["field", "first row", "last row", "Δ"],
          [(k, f2(mib(a[k])), f2(mib(b[k])), f2(mib(b[k] - a[k]))) for k in keys])
    days = by_day(rows, lambda it: it["cache"]["entries"])
    table(["day", "cache entries min", "max", "cache MiB max", "capacity", "max MiB"],
          [(d, min(v for _, v in pts), max(v for _, v in pts),
            f2(max(mib(it["cache"]["bytes"]) for it in rows if it["ts"][:10] == d)),
            rows[-1]["cache"]["capacity"], f1(mib(rows[-1]["cache"]["max_bytes"]))) for d, pts in days.items()])


def sec_router(pulls):
    h1("RouterOS view")
    good = [p for p in pulls if p["meta"].get("container_memory_current") is not None]
    if not good:
        print("no RouterOS data\n")
        return None
    days = defaultdict(list)
    for p in good:
        days[p["meta"]["pull_utc"][:10]].append(p)
    out, mins = [], []
    for i, (d, ps) in enumerate(sorted(days.items())):
        cm = [mib(p["meta"]["container_memory_current"]) for p in ps]
        free = [mib(p["meta"]["router_free_memory"]) for p in ps if p["meta"].get("router_free_memory") is not None]
        cpu = [p["meta"]["router_cpu_load"] for p in ps if p["meta"].get("router_cpu_load") is not None]
        temp = [p["meta"]["router_cpu_temperature"] for p in ps if p["meta"].get("router_cpu_temperature") is not None]
        cusage = [p["meta"]["container_cpu_usage"] for p in ps if p["meta"].get("container_cpu_usage") is not None]
        mins.append((i, min(cm)))
        out.append([d, len(ps), f2(min(cm)), f2(max(cm)), f1(min(free)) if free else "—",
                    max(cpu) if cpu else "—", max(cusage) if cusage else "—",
                    f1(max(temp)) if temp else "—"])
    table(["day", "pulls", "container min", "container max", "router free min", "cpu-load max %",
           "container cpu max %", "cpu temp max °C"], out)
    b, r2, n = fit(mins)
    table(["container memory-current fit", "MiB/day", "R²", "points"], [["daily minimum", f2(b), f2(r2), n]])
    return dict(slope=b, r2=r2)


def sec_upstreams(rows, pulls):
    h1("Upstreams")
    if not rows:
        return
    first = {u["address"]: u for u in rows[0].get("upstreams", [])}
    last = {u["address"]: u for u in rows[-1].get("upstreams", [])}
    unhealthy = defaultdict(int)
    for it in rows:
        for u in it.get("upstreams", []):
            if u.get("state") != "healthy":
                unhealthy[u["address"]] += 1
    out = []
    for addr, u in last.items():
        f = first.get(addr, {})
        out.append([addr, u.get("protocol"), u.get("state"), u.get("attempts"),
                    u.get("attempts", 0) - f.get("attempts", 0), u.get("failures"),
                    u.get("failures", 0) - f.get("failures", 0), u.get("penalties"),
                    u.get("penalized_seconds_total"), f2(1000 * u.get("rtt", {}).get("p50", 0)),
                    f2(1000 * u.get("rtt", {}).get("p99", 0)), unhealthy.get(addr, 0)])
    table(["upstream", "proto", "state (last)", "attempts", "Δ attempts", "failures", "Δ failures",
           "penalties", "penalized s", "rtt p50 ms", "rtt p99 ms", "rows not healthy"], out)


def sec_counters(pulls):
    h1("Counters, first vs last pull")
    good = [p for p in pulls if p["tel"] and p["mem"]]
    if len(good) < 1:
        print("no telemetry\n")
        return None
    a, b = good[0], good[-1]

    def g(p, path):
        o = p
        for k in path.split("."):
            o = (o or {}).get(k) if isinstance(o, dict) else None
        return o

    paths = [
        ("https connections", "tel.listeners.https.connections"),
        ("https requests", "tel.listeners.https.requests"),
        ("https blocked", "tel.listeners.https.blocked"),
        ("https non_tls", "tel.listeners.https.non_tls"),
        ("https hello_timeouts", "tel.listeners.https.hello_timeouts"),
        ("https refused_destination", "tel.listeners.https.refused_destination"),
        ("https upstream_failures", "tel.listeners.https.upstream_failures"),
        ("http connections", "tel.listeners.http.connections"),
        ("http upstream_failures", "tel.listeners.http.upstream_failures"),
        ("dns pass", "tel.counters.dns.pass"),
        ("dns block", "tel.counters.dns.block"),
        ("dns cache_hits", "tel.counters.dns.cache_hits"),
        ("dns cache_misses", "tel.counters.dns.cache_misses"),
        ("dns cache_stale", "tel.counters.dns.cache_stale"),
        ("dns udp inflight shed", "tel.counters.dns_udp_inflight.shed"),
        ("dns tcp peak", "tel.counters.dns_tcp_connections.peak"),
        ("dot peak", "tel.counters.dns_dot_connections.peak"),
        ("swr dropped", "tel.counters.swr.dropped"),
        ("swr failed", "tel.counters.swr.failed"),
        ("cache_cleanup runs", "tel.counters.cache_cleanup.runs"),
        ("tasks_died", "tel.counters.tasks_died"),
        ("events_dropped", "tel.counters.events_dropped"),
        ("major_page_faults", "mem.major_page_faults"),
        ("minor_page_faults", "mem.minor_page_faults"),
        ("cpu_user_ms", "mem.cpu_user_ms"),
        ("cpu_system_ms", "mem.cpu_system_ms"),
    ]
    out, flags = [], {}
    for label, path in paths:
        va, vb = g(a, path), g(b, path)
        delta = (vb - va) if isinstance(va, (int, float)) and isinstance(vb, (int, float)) else None
        flags[label] = delta
        out.append([label, va, vb, delta if delta is not None else "—"])
    table(["counter", a["name"], b["name"], "Δ"], out)
    hours = max(b["h"] - a["h"], 1e-9)
    cpu = (flags.get("cpu_user_ms") or 0) + (flags.get("cpu_system_ms") or 0)
    table(["cpu over the window", "value"], [
        ["hours", f1(hours)], ["cpu ms total (user+sys)", cpu],
        ["cpu % of one core", f2(cpu / (hours * 3600 * 1000) * 100)]])
    return flags


def sec_traffic(rows):
    h1("Traffic per day")
    days = by_day(rows, lambda it: it["queries_delta"])
    out = []
    for d, pts in days.items():
        its = [it for it in rows if it["ts"][:10] == d]
        q = [it["qps"] for it in its]
        out.append([d, sum(v for _, v in pts), sum(it["blocked_delta"] for it in its),
                    f2(sum(q) / len(q)), f2(max(q)),
                    max(it["concurrent_connections"]["https"] for it in its),
                    max(it["concurrent_connections"]["http"] for it in its),
                    its[-1]["minor_page_faults"] - its[0]["minor_page_faults"],
                    f1(mib(its[-1]["allocator_committed_bytes"]))])
    table(["day", "queries", "blocked", "qps mean", "qps max", "https conc max", "http conc max",
           "minor faults Δ", "allocator committed (monotone)"], out)


def sec_problems(pulls):
    h1("RouterOS problem log since boot")
    seen = {}
    for p in pulls:
        for e in p["plog"]:
            when = e.get("time", "")
            try:
                t = datetime.strptime(when, "%Y-%m-%d %H:%M:%S").replace(tzinfo=timezone(timedelta(hours=3)))
            except ValueError:
                continue
            if t < BOOT:
                continue
            seen[(when, e.get("message"))] = e
    counts = defaultdict(int)
    for e in seen.values():
        counts[",".join(e.get("topics", []))] += 1
    table(["topics", "lines"], sorted(counts.items()))
    tail = sorted(seen.values(), key=lambda e: e["time"])[-10:]
    table(["time (router local)", "topics", "message"],
          [(e["time"], ",".join(e.get("topics", [])), e.get("message", "")[:100]) for e in tail])


def sec_verdict(res, router, flags, rows):
    h1("Verdict inputs")
    if not res:
        print("no residual series\n")
        return
    mins = res["mins"]
    deltas = [b - a for (_, a), (_, b) in zip(mins, mins[1:])]
    rising = sum(1 for d in deltas if d > 0)
    spread = (max(v for _, v in mins[-3:]) - min(v for _, v in mins[-3:])) if len(mins) >= 3 else float("nan")
    table(["input", "value", "threshold"], [
        ["days with a daily minimum", len(mins), "≥ 4 for a slope"],
        ["daily-min slope, all days (MiB/day)", f2(res["slope"]), "climbing ≥ %.1f, flat < %.1f" % (CLIMB_MIB_PER_DAY, FLAT_MIB_PER_DAY)],
        ["R² of that fit", f2(res["r2"]), "≥ 0.6 to trust the slope"],
        ["daily-min slope, last 3 days", f2(res["last3"]), "same"],
        ["12 h floor slope (MiB/day)", f2(res["half_slope"]), "same"],
        ["day-over-day minima rising", "%d of %d" % (rising, len(deltas)), "climbing if most"],
        ["spread of the last 3 daily minima", f2(spread), "flat if < %.1f" % FLAT_SPREAD_MIB],
        ["container memory-current slope (MiB/day)", f2(router["slope"]) if router else "—", "should agree in sign"],
        ["peak_rss last (MiB)", f2(mib(rows[-1]["peak_rss"])) if rows else "—", "t0 was 92.97; note each move"],
        ["tasks_died Δ", (flags or {}).get("tasks_died", "—"), "0"],
        ["events_dropped Δ", (flags or {}).get("events_dropped", "—"), "0"],
        ["major_page_faults Δ", (flags or {}).get("major_page_faults", "—"), "~0 after boot"],
        ["https non_tls Δ", (flags or {}).get("https non_tls", "—"), "0 with the laptop exempt"],
    ])
    reading = "UNCLEAR"
    if len(mins) >= 4 and not math.isnan(res["slope"]):
        if res["slope"] >= CLIMB_MIB_PER_DAY and res["r2"] >= 0.6 and rising >= len(deltas) / 2:
            reading = "CLIMBING"
        elif abs(res["slope"]) < FLAT_MIB_PER_DAY or (not math.isnan(spread) and spread < FLAT_SPREAD_MIB):
            reading = "FLAT"
    print("Suggested reading: **%s** — the 0.3.4 reference rose 1.3–8.2 MiB/day on same-hour floors "
          "and doubled its daily minimum in nine days. A reading is a proposal; the owner decides.\n" % reading)


# ---------------------------------------------------------------- main

def main():
    sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser()
    ap.add_argument("sections", nargs="*")
    ap.add_argument("--compare", default="")
    ap.add_argument("--compare-boot", default="2026-09-11T22:02:48Z")
    ap.add_argument("--days", type=float, default=0)
    ap.add_argument("--warmup-hours", type=float, default=1.0)
    a = ap.parse_args()
    order = ["pulls", "perf", "residual", "peak", "accounted", "router", "upstreams", "counters",
             "traffic", "problems", "verdict"]
    want = a.sections or order
    unknown = [s for s in want if s not in order]
    if unknown:
        sys.exit("unknown section(s): %s" % ", ".join(unknown))

    pulls = load_pulls(a.days)
    rows = merge_perf(pulls, a.days)
    print("# Soak 0.4.1 — reduce at %s, boot %s, %d pulls, %d perf rows%s\n" % (
        datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), BOOT.strftime("%Y-%m-%dT%H:%M:%SZ"),
        len(pulls), len(rows), ", first %s days" % a.days if a.days else ""))

    res = router = flags = None
    settled = [it for it in rows if ts(it["ts"]) >= BOOT + timedelta(hours=a.warmup_hours)]
    if "pulls" in want:
        sec_pulls(pulls, rows)
    if "perf" in want:
        sec_perf(rows)
    if "residual" in want or "verdict" in want:
        res = sec_residual(settled, a.compare, ts(a.compare_boot), a.warmup_hours) if settled else None
    if "peak" in want and settled:
        sec_peak(settled)
    if "accounted" in want and rows:
        sec_accounted(rows)
    if "router" in want or "verdict" in want:
        router = sec_router(pulls)
    if "upstreams" in want and rows:
        sec_upstreams(rows, pulls)
    if "counters" in want or "verdict" in want:
        flags = sec_counters(pulls)
    if "traffic" in want and rows:
        sec_traffic(rows)
    if "problems" in want:
        sec_problems(pulls)
    if "verdict" in want:
        sec_verdict(res, router, flags, rows)


if __name__ == "__main__":
    main()
