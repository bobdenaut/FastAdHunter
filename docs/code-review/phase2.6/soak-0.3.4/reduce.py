"""Recompute every derived figure in README.md from pulls/ alone.

Usage: python reduce.py [section ...]
Sections: pulls perf memory floor peak container upstream http service lists
Default: all.
"""

import json, math, os, re, sys
from collections import Counter, defaultdict
from datetime import datetime, timezone

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "pulls")
BOOT = datetime(2026, 9, 11, 22, 2, 48, tzinfo=timezone.utc)
MiB = 1 << 20


def ts(s):
    return datetime.strptime(s, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)


def load(path):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return None


def txt(path):
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            return f.read()
    except Exception:
        return ""


def ros_field(body, key):
    m = re.search(r"^\s*" + re.escape(key) + r":\s*(\S+)", body, re.M)
    return m.group(1) if m else None


def mib(value):
    if not value:
        return None
    m = re.match(r"([\d.]+)(KiB|MiB|GiB)", value)
    if not m:
        return None
    return float(m.group(1)) * {"KiB": 1 / 1024, "MiB": 1, "GiB": 1024}[m.group(2)]


def ros_uptime(value):
    n = {k: int(v) for v, k in re.findall(r"(\d+)([wdhms])", value or "")}
    return (n.get("w", 0) * 604800 + n.get("d", 0) * 86400 + n.get("h", 0) * 3600
            + n.get("m", 0) * 60 + n.get("s", 0))


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


def stats(values):
    v = sorted(values)
    mu = sum(v) / len(v)
    sd = math.sqrt(sum((x - mu) ** 2 for x in v) / (len(v) - 1)) if len(v) > 1 else 0.0
    return dict(n=len(v), mean=mu, sd=sd, p50=v[len(v) // 2], min=v[0], max=v[-1])


# ---------------------------------------------------------------- load

dirs = sorted(d for d in os.listdir(ROOT) if os.path.isdir(os.path.join(ROOT, d)))
pulls = []
for d in dirs:
    p = os.path.join(ROOT, d)
    meta = load(os.path.join(p, "meta.json"))
    tel = load(os.path.join(p, "telemetry.json"))
    if not meta:
        continue
    res = txt(os.path.join(p, "routeros-resource.txt"))
    cont = txt(os.path.join(p, "routeros-container.txt"))
    names = re.findall(r';;; (\S+)', cont)
    own = re.search(r'name="fastadhunter-[^"]*"(?:.|\n)*?memory-current=(\S+)', cont)
    pulls.append(dict(
        name=d, dir=p, meta=meta, tel=tel, t=ts(meta["pull_utc"]),
        h=(ts(meta["pull_utc"]) - BOOT).total_seconds() / 3600,
        uptime=meta["uptime_seconds"], version=meta["version"],
        rss=meta["process_rss"] / MiB, peak=meta["process_peak_rss"] / MiB,
        cmem_meta=mib(meta.get("container_memory_current")),
        cmem_own=mib(own.group(1)) if own else None,
        containers=names,
        free=mib(ros_field(res, "free-memory")),
        freehdd=mib(ros_field(res, "free-hdd-space")),
        wsect=int(ros_field(res, "write-sect-since-reboot") or 0),
        ros_up=ros_uptime(ros_field(res, "uptime")),
        memhigh=(re.search(r"memory-high=(\S+)", cont) or [None, None])[1],
        errors=meta.get("errors") or {},
        clog=txt(os.path.join(p, "routeros-log-container.txt")),
        plog=txt(os.path.join(p, "routeros-log-problems.txt")),
    ))

rows = {}
for pu in pulls:
    hp = load(os.path.join(pu["dir"], "history-perf.json"))
    for it in (hp or {}).get("items", []):
        if ts(it["ts"]) >= BOOT:
            rows[it["ts"]] = it
series = [rows[k] for k in sorted(rows)]
for r in series:
    r["_h"] = (ts(r["ts"]) - BOOT).total_seconds() / 3600

RSS = lambda r: r["rss_bytes"] / MiB
RES = lambda r: r["memory"]["residual_bytes"] / MiB
ACC = lambda r: r["memory"]["accounted_bytes"] / MiB
END = series[-1]["_h"] + 1e-3


def bucket_min(key, lo, hi, width):
    b = {}
    for r in series:
        if lo <= r["_h"] < hi:
            k = int(r["_h"] // width)
            b[k] = min(b.get(k, 1e18), key(r))
    return sorted(b.items())


want = set(sys.argv[1:])
run = lambda s: not want or s in want

# ---------------------------------------------------------------- sections

if run("pulls"):
    print("== PULLS ==")
    print("n=%d  %s .. %s  span=%.2f h  uptime_last=%.2f h" % (
        len(pulls), pulls[0]["name"], pulls[-1]["name"],
        (pulls[-1]["t"] - pulls[0]["t"]).total_seconds() / 3600, pulls[-1]["uptime"] / 3600))
    print("versions=%s  uptime_monotonic=%s  memory_high=%s" % (
        sorted({p["version"] for p in pulls}),
        all(pulls[i]["uptime"] < pulls[i + 1]["uptime"] for i in range(len(pulls) - 1)),
        sorted({p["memhigh"] for p in pulls})))
    print("meta.errors non-empty: %d" % sum(1 for p in pulls if p["errors"]))
    gaps = [(pulls[i + 1]["t"] - pulls[i]["t"]).total_seconds() for i in range(len(pulls) - 1)]
    print("cadence=%s" % Counter(gaps).most_common(6))
    extra = [p["name"] for p in pulls if len(p["containers"]) > 1]
    print("pulls with >1 container: %s" % extra)
    for p in pulls:
        if p["cmem_own"] is not None and p["cmem_meta"] is not None and abs(p["cmem_own"] - p["cmem_meta"]) > 0.05:
            print("  meta mis-parse %s: meta=%.1f own=%.1f containers=%s" % (
                p["name"], p["cmem_meta"], p["cmem_own"], p["containers"]))
    warn = [l for p in pulls for l in p["clog"].splitlines() if re.search(r"WARN|ERROR", l)]
    print("container log WARN/ERROR lines: %d" % len(warn))
    newest = Counter(([l for l in p["plog"].splitlines() if l.strip()] or ["-"])[-1].strip()[:90] for p in pulls)
    print("newest RouterOS problem line: %s" % newest.most_common(2))

if run("perf"):
    print("\n== PERF SERIES ==")
    d = [(ts(series[i + 1]["ts"]) - ts(series[i]["ts"])).total_seconds() for i in range(len(series) - 1)]
    span = (ts(series[-1]["ts"]) - ts(series[0]["ts"])).total_seconds()
    print("rows=%d expected=%.0f  %s .. %s  last_h=%.2f" % (
        len(series), span / 360 + 1, series[0]["ts"], series[-1]["ts"], series[-1]["_h"]))
    print("intervals=%s" % Counter(d).most_common(6))

if run("memory"):
    print("\n== ACCOUNTED ==")
    acc = sorted(ACC(r) for r in series)
    print("accounted MiB min=%.2f p50=%.2f p95=%.2f max=%.2f" % (
        acc[0], acc[len(acc) // 2], acc[int(.95 * (len(acc) - 1))], acc[-1]))
    for k in ("ruleset_bytes", "cache_estimated_bytes", "stats_clients_bytes", "stats_aggregates_bytes"):
        v = [r["memory"][k] / MiB for r in series]
        print("  %-24s first=%.2f last=%.2f min=%.2f max=%.2f" % (k, v[0], v[-1], min(v), max(v)))
    print("rss_file %.2f -> %.2f MiB   rss_anon min=%.2f max=%.2f" % (
        series[0]["rss_file_bytes"] / MiB, series[-1]["rss_file_bytes"] / MiB,
        min(r["rss_anon_bytes"] / MiB for r in series), max(r["rss_anon_bytes"] / MiB for r in series)))

if run("floor"):
    print("\n== FLOOR (hourly minima, least squares) ==")
    for lo in (4, 24, 48, 60, 72, 84, 96):
        b, r2, n = fit([(float(h), v) for h, v in bucket_min(RSS, lo, END, 1)])
        b2, r22, _ = fit([(float(h), v) for h, v in bucket_min(RES, lo, END, 1)])
        print("  h%-3d-end  RSS %+.2f MiB/day R2=%.3f | residual %+.2f MiB/day R2=%.3f | n=%d" % (
            lo, b * 24, r2, b2 * 24, r22, n))
    print("  12 h floors (RSS / residual):")
    for i in range(int(END // 12) + 1):
        sub = [r for r in series if 12 * i <= r["_h"] < 12 * (i + 1)]
        if sub:
            print("    h%-3d-%-3d  %.1f / %.1f  (n=%d)" % (
                12 * i, 12 * (i + 1), min(map(RSS, sub)), min(map(RES, sub)), len(sub)))
    print("  6 h RSS minima: %s" % " ".join("%.1f" % v for _, v in bucket_min(RSS, 0, END, 6)))
    print("  6 h residual minima: %s" % " ".join("%.1f" % v for _, v in bucket_min(RES, 0, END, 6)))

if run("diurnal"):
    print("\n== FLOOR BY FIXED HOUR-OF-DAY WINDOW (removes the daily cycle) ==")
    for lo_h, hi_h, label in ((1, 6, "quiet 01-06 UTC"), (12, 17, "busy 12-17 UTC")):
        by_day = defaultdict(list)
        for r in series:
            if lo_h <= ts(r["ts"]).hour < hi_h:
                by_day[ts(r["ts"]).date()].append(r)
        print("  %s" % label)
        prev = None
        for day in sorted(by_day):
            sub = by_day[day]
            rss, res = min(map(RSS, sub)), min(map(RES, sub))
            step = "" if prev is None else "  %+.1f" % (rss - prev)
            print("    %s  rss=%.1f  residual=%.1f  n=%-3d%s" % (day, rss, res, len(sub), step))
            prev = rss

if run("peak"):
    print("\n== PEAK RSS / ALLOCATOR COMMITTED STEPS ==")
    pk = cm = None
    for r in series:
        v, c = r["peak_rss"] / MiB, r["allocator_committed_bytes"] / MiB
        if pk is None or v > pk + 0.01:
            print("  peak      %s h%-6.1f %.2f MiB  bodies=%d" % (r["ts"], r["_h"], v, r["list_fetch"]["bodies"]))
            pk = v
        if cm is None or c > cm + 0.5:
            print("  committed %s h%-6.1f %.1f MiB  bodies=%d rss=%.1f" % (
                r["ts"], r["_h"], c, r["list_fetch"]["bodies"], RSS(r)))
            cm = c
    print("  committed monotone non-decreasing: %s" % all(
        series[i + 1]["allocator_committed_bytes"] >= series[i]["allocator_committed_bytes"]
        for i in range(len(series) - 1)))

if run("container"):
    print("\n== CONTAINER / ROUTER ==")
    clean = [p for p in pulls if len(p["containers"]) == 1]
    off = [(p["h"], p["cmem_meta"] - p["rss"]) for p in clean]
    s = stats([o for _, o in off])
    print("offset (own container only) n=%(n)d mean=%(mean).2f sd=%(sd).2f p50=%(p50).2f min=%(min).2f max=%(max).2f" % s)
    b, r2, n = fit(off)
    print("offset trend %+.2f MiB/day R2=%.3f n=%d" % (b * 24, r2, n))
    for lbl, sl in (("first24", off[:24]), ("last24", off[-24:])):
        q = stats([o for _, o in sl])
        print("  %-8s mean=%.2f sd=%.2f" % (lbl, q["mean"], q["sd"]))
    print("container memory-current first=%.1f last=%.1f max=%.1f MiB" % (
        clean[0]["cmem_meta"], clean[-1]["cmem_meta"], max(p["cmem_meta"] for p in clean)))
    print("router free-memory first=%.1f last=%.1f min=%.1f MiB" % (
        pulls[0]["free"], pulls[-1]["free"], min(p["free"] for p in pulls)))
    print("free-hdd %.1f -> %.1f MiB | write-sect %d -> %d (+%d) | router uptime %.1f h" % (
        pulls[0]["freehdd"], pulls[-1]["freehdd"], pulls[0]["wsect"], pulls[-1]["wsect"],
        pulls[-1]["wsect"] - pulls[0]["wsect"], pulls[-1]["ros_up"] / 3600))

if run("upstream"):
    print("\n== UPSTREAMS ==")
    for u in pulls[-1]["tel"]["upstreams"]:
        print("  %-22s att=%-6d fail=%-2d runs=%s pen=%d pensec=%d probes=%d/%d state=%s p50=%.3f mean=%.2f ms" % (
            u["address"], u["attempts"], u["failures"], u["failure_runs"], u["penalties"],
            u["penalized_seconds_total"], u["probe_successes"], u["probes"], u["state"],
            u["rtt"]["p50"], 1000 * u["rtt"]["sum_seconds"] / max(1, u["rtt"]["count"])))
    print("  p50 across pulls: %s   p99: %s" % (
        dict(Counter(p["tel"]["upstreams"][0]["rtt"]["p50"] for p in pulls if p["tel"])),
        dict(Counter(p["tel"]["upstreams"][0]["rtt"]["p99"] for p in pulls if p["tel"]))))
    print("  events:")
    prev = None
    for r in series:
        if not r.get("upstreams"):
            continue
        u = r["upstreams"][0]
        cur = (u["failures"], u["penalties"], u["penalized_seconds_total"], u["probes"], tuple(u["failure_runs"]))
        if prev and cur != prev:
            print("    %s h%-6.1f fail %d->%d pen %d->%d pensec %d->%d probes %d->%d runs %s" % (
                r["ts"], r["_h"], prev[0], cur[0], prev[1], cur[1], prev[2], cur[2], prev[3], cur[3], cur[4]))
        prev = cur

if run("http"):
    print("\n== HTTP ==")
    deltas = []
    for i in range(1, len(pulls)):
        a, b = pulls[i - 1]["tel"], pulls[i]["tel"]
        if not a or not b:
            continue
        deltas.append((pulls[i]["t"], (b["counters"]["http"]["response_bytes"] - a["counters"]["http"]["response_bytes"]) / 1e6,
                       b["counters"]["http"]["pass"] - a["counters"]["http"]["pass"], pulls[i]["rss"]))
    for t, mb, ps, rss in sorted(deltas, key=lambda x: -x[1])[:6]:
        print("  %s  %8.1f MB  pass+%-5d rss_at_pull=%.1f" % (t.strftime("%m-%d %H:%M"), mb, ps, rss))
    print("  total %.1f MB  max concurrent http=%d https=%d" % (
        sum(x[1] for x in deltas),
        max(r["concurrent_connections"]["http"] for r in series),
        max(r["concurrent_connections"]["https"] for r in series)))
    xs = [x[1] for x in deltas]
    ys = []
    for i in range(1, len(pulls)):
        ys.append((pulls[i]["tel"]["memory"]["residual_bytes"] - pulls[i - 1]["tel"]["memory"]["residual_bytes"]) / MiB)
    mx, my = sum(xs) / len(xs), sum(ys) / len(ys)
    num = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
    den = math.sqrt(sum((x - mx) ** 2 for x in xs) * sum((y - my) ** 2 for y in ys))
    print("  pearson(d_http_MB, d_residual_MiB)=%.3f n=%d" % (num / den, len(xs)))
    print("  hourly RSS deltas >= 8 MiB (max per hour):")
    byh = defaultdict(list)
    for r in series:
        byh[int(r["_h"])].append(r)
    seq = [(h, max(map(RSS, byh[h])), max(x["concurrent_connections"]["http"] for x in byh[h]), byh[h][0]["ts"])
           for h in sorted(byh)]
    for i in range(1, len(seq)):
        d = seq[i][1] - seq[i - 1][1]
        if abs(d) >= 8:
            print("    h%-4d %s rss_max=%.1f delta=%+.1f conc_http=%d" % (
                seq[i][0], seq[i][3][:16], seq[i][1], d, seq[i][2]))

if run("service"):
    print("\n== SERVICE ==")
    tel = pulls[-1]["tel"]
    c, dns, upt = tel["counters"], tel["counters"]["dns"], tel["process"]["uptime_seconds"]
    q = dns["block"] + dns["pass"] + dns["allow"]
    print("  queries=%d over %.1f h = %.3f qps" % (q, upt / 3600, q / upt))
    print("  blocked=%d (%.2f%%)  pass=%d  allow=%d" % (dns["block"], 100 * dns["block"] / q, dns["pass"], dns["allow"]))
    print("  cache hits=%d misses=%d -> %.2f%% of non-blocked; stale=%d (%.2f%% of hits)" % (
        dns["cache_hits"], dns["cache_misses"], 100 * dns["cache_hits"] / (dns["cache_hits"] + dns["cache_misses"]),
        dns["cache_stale"], 100 * dns["cache_stale"] / dns["cache_hits"]))
    print("  answers=%s  swr=%s" % (dns["answers"], c["swr"]))
    print("  cache=%s" % {k: tel["cache"][k] for k in ("entries", "capacity", "bytes", "evictions", "expired")})
    print("  cleanup=%s" % c["cache_cleanup"])
    print("  tcp=%s udp=%s events_dropped=%d" % (c["dns_tcp_connections"], c["dns_udp_inflight"], c["events_dropped"]))
    print("  http=%s" % c["http"])
    for k, v in tel["latency"]["dns"].items():
        print("  latency dns %-10s n=%-7d mean=%.4f ms" % (k, v["count"], 1000 * v["sum_seconds"] / max(1, v["count"])))
    for k, v in tel["latency"]["http"].items():
        if v["count"]:
            print("  latency http %-9s n=%-7d mean=%.1f ms" % (k, v["count"], 1000 * v["sum_seconds"] / v["count"]))
    m = tel["memory"]
    print("  cpu user=%ds sys=%ds -> %.3f%% of one core" % (
        m["cpu_user_ms"] // 1000, m["cpu_system_ms"] // 1000,
        100 * (m["cpu_user_ms"] + m["cpu_system_ms"]) / 1000 / upt))
    att = sum(u["attempts"] for u in tel["upstreams"])
    print("  reconcile: attempts %d - (misses %d + swr.completed %d) = %d" % (
        att, dns["cache_misses"], c["swr"]["completed"], att - dns["cache_misses"] - c["swr"]["completed"]))
    print("  reconcile: cache_stale %d - (enqueued %d + dedup %d) = %d" % (
        dns["cache_stale"], c["swr"]["enqueued"], c["swr"]["deduplicated"],
        dns["cache_stale"] - c["swr"]["enqueued"] - c["swr"]["deduplicated"]))
    hod = defaultdict(list)
    for r in series:
        hod[ts(r["ts"]).hour].append(r["qps"])
    med = [(h, round(sorted(v)[len(v) // 2], 2)) for h, v in sorted(hod.items())]
    print("  qps median by hour-of-day: %.2f .. %.2f ; max sample %.2f" % (
        min(x[1] for x in med), max(x[1] for x in med), max(r["qps"] for r in series)))

if run("lists"):
    print("\n== LISTS / RULESET / CLIENTS ==")
    f, l = pulls[0]["tel"], pulls[-1]["tel"]
    print("  rules %d -> %d (+%d); ruleset_bytes %.2f -> %.2f MiB" % (
        f["ruleset"]["rules"], l["ruleset"]["rules"], l["ruleset"]["rules"] - f["ruleset"]["rules"],
        f["memory"]["ruleset_bytes"] / MiB, l["memory"]["ruleset_bytes"] / MiB))
    print("  bodies %d -> %d ; fetched %.1f MB ; not_modified %d" % (
        f["counters"]["lists"]["bodies"], l["counters"]["lists"]["bodies"],
        l["counters"]["lists"]["bytes_fetched"] / 1e6, l["counters"]["lists"]["not_modified"]))
    prev = None
    for p in pulls:
        b = p["tel"]["counters"]["lists"]["bodies"]
        if prev is not None and b != prev:
            print("    %s bodies %d->%d rss=%.1f" % (p["name"], prev, b, p["rss"]))
        prev = b
    for p in pulls:
        cl = load(os.path.join(p["dir"], "clients.json"))
        if cl:
            print("  %s clients=%d" % (p["name"], len(cl.get("items", cl if isinstance(cl, list) else []))))
    cfg = [json.dumps(load(os.path.join(p["dir"], "config.json")), sort_keys=True) for p in pulls]
    cfg = [c for c in cfg if c != "null"]
    print("  config pulls=%d distinct=%d" % (len(cfg), len(set(cfg))))
