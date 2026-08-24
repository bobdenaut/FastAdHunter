#!/usr/bin/env python3
"""P2.6-10 / suite S1-N analysis. Recomputes every number from the raw JSON
written by collect.py -- nothing is carried forward from the run.

  python analyze.py runs/ > result.txt

Definitions, all pre-declared before the session:

  Per repetition r, every counter is a delta t1 - t0.
    forwards           latency.dns.forward.count
    forward_mean_ms    latency.dns.forward.sum_seconds / forwards * 1000
    cache_hits         latency.dns.cache_hit.count
    cache_hit_mean_ms  latency.dns.cache_hit.sum_seconds / cache_hits * 1000
    cache_misses       counters.dns.cache_misses
    attempts_total     sum over upstreams[] of attempts
    swr_c / swr_f      counters.swr.completed / .failed
    attempts_per_fwd   (attempts_total - (swr_c + swr_f)) / cache_misses
    forward_p99_ms     median over the perf rows strictly inside [t0, t1] of
                       latency.forward_p99 (a bucket bound, not interpolated)
    cache_hit_p99_ms   same for latency.cache_hit_p99
    qps_server         (pass + allow + block) delta / elapsed_seconds

  N for metric m, over K repetitions labelled A,B,A,B,...:
    mu    = mean of all K values (pooled, both labels)
    D     = { (a - b) / mu  for every a in A-group, b in B-group }  ((K/2)^2 pairs)
    N_m   = P95 of |d| over D, type-7 linear interpolation
            (equivalently: half-width of the symmetric interval [-N, +N]
             holding 95 % of the pairwise deltas, sign being meaningless in a
             null A/B)
    also reported: max|d| (the full band) and the label-group means.

  Control-arm drift: N computed the same way on cache_hit_p99_ms and on
  cache_hit_mean_ms. Session VOID if N_control > N_measured on the matching
  statistic (p99 vs p99, mean vs mean).

  Phase-effect check: least-squares slope of the metric against repetition
  index, expressed as % of mu per repetition. Flagged when |slope| * K > N_m --
  a monotone trend larger than the band it is supposed to sit inside.

  Resolution vs K: N recomputed from the first k repetitions only, k = 4, 6,
  8, ... K.

  Tier-3 decision, fixed order (spec S1-G2 / benchmarks S1-N), no judgement:
    1. attempts_total   if N <= 10 %  -> carries tier 3
    2. forward_p99_ms   if N <= 10 %  -> carries tier 3
    3. otherwise tier 3 is dropped, both N stated.
    Threshold = max(2 * N, 5 %) on the chosen metric.
"""

import json
import os
import sys

def q7(xs, q):
    """Type-7 quantile (the R/numpy default)."""
    if not xs:
        return float("nan")
    s = sorted(xs)
    if len(s) == 1:
        return s[0]
    h = (len(s) - 1) * q
    lo = int(h)
    hi = min(lo + 1, len(s) - 1)
    return s[lo] + (h - lo) * (s[hi] - s[lo])

def dig(obj, path, default=0):
    cur = obj
    for k in path:
        if not isinstance(cur, dict) or k not in cur:
            return default
        cur = cur[k]
    return cur

def load_rep(d):
    j = lambda n: json.load(open(os.path.join(d, n), encoding="utf-8"))
    meta, t0, t1, perf = j("meta.json"), j("t0-telemetry.json"), j("t1-telemetry.json"), j("perf.json")
    cfg = j("config.json")

    delta = lambda p: dig(t1, p) - dig(t0, p)
    forwards = delta(["latency", "dns", "forward", "count"])
    hits = delta(["latency", "dns", "cache_hit", "count"])
    misses = delta(["counters", "dns", "cache_misses"])
    att = (sum(u["attempts"] for u in t1.get("upstreams", []))
           - sum(u["attempts"] for u in t0.get("upstreams", [])))
    swr_c = delta(["counters", "swr", "completed"])
    swr_f = delta(["counters", "swr", "failed"])
    swr_e = delta(["counters", "swr", "enqueued"])
    swr_d = delta(["counters", "swr", "dropped"])
    queries = (delta(["counters", "dns", "pass"]) + delta(["counters", "dns", "allow"])
               + delta(["counters", "dns", "block"]))
    elapsed = meta["elapsed_seconds"]
    load_secs = meta.get("load_seconds", elapsed)

    # Perf rows are filtered to the load window, not to [T0, T1]: the post-load
    # drain exists only to let the 10 s telemetry poll republish the lagging
    # attempts/SWR counters, and its near-idle rows would drag the per-repetition
    # p99 toward an interval nobody loaded.
    lo = meta.get("load_t0_utc", meta["t0_utc"])
    hi = meta.get("load_t1_utc", meta["t1_utc"])
    rows = [i for i in perf.get("items", []) if lo <= i["ts"] <= hi]
    p99f = [dig(i, ["latency", "forward_p99"]) * 1000 for i in rows]
    p99c = [dig(i, ["latency", "cache_hit_p99"]) * 1000 for i in rows]

    return {
        "rep": meta["rep"], "label": meta["label"], "t0": meta["t0_utc"], "t1": meta["t1_utc"],
        "elapsed_s": elapsed,
        "version": meta["process_version_t1"],
        "config_fingerprint": json.dumps(cfg, sort_keys=True),
        "restarted": meta["uptime_t1"] < meta["uptime_t0"],
        "forwards": forwards,
        "forward_mean_ms": delta(["latency", "dns", "forward", "sum_seconds"]) / forwards * 1000 if forwards else float("nan"),
        "cache_hits": hits,
        "cache_hit_mean_ms": delta(["latency", "dns", "cache_hit", "sum_seconds"]) / hits * 1000 if hits else float("nan"),
        "cache_misses": misses,
        "attempts_total": att,
        "attempts_per_fwd": (att - (swr_c + swr_f)) / misses if misses else float("nan"),
        "swr_enqueued": swr_e, "swr_completed": swr_c, "swr_failed": swr_f, "swr_dropped": swr_d,
        "swr_attempts": swr_c + swr_f,
        "qps_server": queries / load_secs if load_secs else float("nan"),
        "forward_p99_ms": q7(p99f, 0.5) if p99f else float("nan"),
        "cache_hit_p99_ms": q7(p99c, 0.5) if p99c else float("nan"),
        "perf_rows": len(rows),
        "forward_p99_distinct": sorted(set(p99f)),
        "cache_hit_p99_distinct": sorted(set(p99c)),
        "cache_entries_t0": dig(t0, ["cache", "entries"]),
        "cache_fresh_t0": dig(t0, ["cache", "fresh"]),
        "cache_stale_t0": dig(t0, ["cache", "stale"]),
    }

def band(reps, key):
    vals = [r[key] for r in reps]
    if any(v != v for v in vals):
        return None
    mu = sum(vals) / len(vals)
    if mu == 0:
        return None
    A = [r[key] for r in reps if r["label"] == "A"]
    B = [r[key] for r in reps if r["label"] == "B"]
    if not A or not B:
        return None
    D = [abs(a - b) / mu for a in A for b in B]
    n = len(vals)
    xbar = (n - 1) / 2.0
    sxx = sum((i - xbar) ** 2 for i in range(n))
    sxy = sum((i - xbar) * (v - mu) for i, v in enumerate(vals))
    slope = (sxy / sxx / mu * 100) if sxx else 0.0
    return {
        "mu": mu, "vals": vals,
        "mean_A": sum(A) / len(A), "mean_B": sum(B) / len(B),
        "N_pct": q7(D, 0.95) * 100, "max_pct": max(D) * 100, "pairs": len(D),
        "slope_pct_per_rep": slope, "trend_pct_total": slope * (n - 1),
        "distinct": len(set(vals)),
    }

METRICS = ["attempts_total", "forward_p99_ms", "attempts_per_fwd", "forward_mean_ms",
           "cache_hit_p99_ms", "cache_hit_mean_ms", "qps_server", "swr_attempts"]

def main(root):
    dirs = sorted(d for d in os.listdir(root) if os.path.isdir(os.path.join(root, d))
                  and "-" in d)
    reps = [load_rep(os.path.join(root, d)) for d in dirs]
    warm = [r for r in reps if r["label"] == "W"]
    reps = [r for r in reps if r["label"] in ("A", "B")]
    reps.sort(key=lambda r: r["rep"])

    print("== repetitions (every one reported, none discarded) ==")
    hdr = ("rep lbl  t0(UTC)              elapsed  fwd      fwd_mean  fwd_p99  ch_p99  "
           "attempts  att/fwd  swr_att  qps")
    print(hdr)
    for r in reps:
        print("%3d %3s  %s %7.1f %8d %8.3f %8.3f %7.3f %9d %8.4f %8d %8.1f"
              % (r["rep"], r["label"], r["t0"], r["elapsed_s"], r["forwards"],
                 r["forward_mean_ms"], r["forward_p99_ms"], r["cache_hit_p99_ms"],
                 r["attempts_total"], r["attempts_per_fwd"], r["swr_attempts"],
                 r["qps_server"]))
    if warm:
        print("\ndiscarded warm-up repetitions (declared before the session): %s"
              % ", ".join("W%d" % w["rep"] for w in warm))

    print("\n== identity checks (both labels must run the same binary and config) ==")
    print("versions:        %s" % sorted({r["version"] for r in reps}))
    print("config identical: %s" % (len({r["config_fingerprint"] for r in reps}) == 1))
    print("alternation:      %s  (expected strict A/B/A/B...)"
          % "".join(r["label"] for r in reps))
    print("restart inside a repetition: %s" % any(r["restarted"] for r in reps))
    print("perf rows per repetition: %s" % [r["perf_rows"] for r in reps])
    print("forward_p99 distinct bucket bounds observed: %s"
          % sorted({v for r in reps for v in r["forward_p99_distinct"]}))
    print("cache_hit_p99 distinct bucket bounds observed: %s"
          % sorted({v for r in reps for v in r["cache_hit_p99_distinct"]}))
    print("cache state at T0 (entries/fresh/stale): %s"
          % [(r["cache_entries_t0"], r["cache_fresh_t0"], r["cache_stale_t0"]) for r in reps])

    print("\n== N per metric ==")
    print("metric              mean_A       mean_B       N%      max%    trend%   distinct")
    bands = {}
    for m in METRICS:
        b = band(reps, m)
        bands[m] = b
        if b is None:
            print("%-18s  <not populated>" % m)
            continue
        print("%-18s %12.4f %12.4f %7.2f %7.2f %8.2f %6d"
              % (m, b["mean_A"], b["mean_B"], b["N_pct"], b["max_pct"],
                 b["trend_pct_total"], b["distinct"]))

    print("\n== control arm ==")
    for meas, ctrl in (("forward_p99_ms", "cache_hit_p99_ms"),
                       ("forward_mean_ms", "cache_hit_mean_ms")):
        bm, bc = bands.get(meas), bands.get(ctrl)
        if not bm or not bc:
            print("%-16s vs %-18s : not computable" % (meas, ctrl))
            continue
        void = bc["N_pct"] > bm["N_pct"]
        print("%-16s N=%6.2f%%   %-18s N=%6.2f%%   -> %s"
              % (meas, bm["N_pct"], ctrl, bc["N_pct"],
                 "SESSION VOID (control moved more)" if void else "control quieter, session stands"))
        if bc["distinct"] == 1:
            print("    NOTE: control arm took a single value in every repetition -- it cannot"
                  " move, so it provides no drift protection at this resolution.")

    print("\n== phase-effect check (|trend| vs N) ==")
    for m in METRICS:
        b = bands.get(m)
        if not b:
            continue
        flag = "TREND EXCEEDS BAND" if abs(b["trend_pct_total"]) > b["N_pct"] else "ok"
        print("%-18s trend %+7.2f%% over the session, N %6.2f%%  -> %s"
              % (m, b["trend_pct_total"], b["N_pct"], flag))

    print("\n== resolution vs K ==")
    print(" k  " + "  ".join("%-16s" % m for m in ("attempts_total", "forward_p99_ms",
                                                   "attempts_per_fwd", "forward_mean_ms")))
    k = 4
    while k <= len(reps):
        row = []
        for m in ("attempts_total", "forward_p99_ms", "attempts_per_fwd", "forward_mean_ms"):
            b = band(reps[:k], m)
            row.append("%-16s" % ("n/a" if b is None else "%.2f%%" % b["N_pct"]))
        print("%2d  %s" % (k, "  ".join(row)))
        k += 2

    print("\n== S1-G2 tier 3, fixed order ==")
    chosen = None
    for m in ("attempts_total", "forward_p99_ms"):
        b = bands.get(m)
        if b is None:
            print("%-16s : not populated, skipped" % m)
            continue
        print("%-16s : N = %.2f %%  -> %s" % (m, b["N_pct"],
              "ADEQUATE (<= 10 %)" if b["N_pct"] <= 10.0 else "inadequate (> 10 %)"))
        if chosen is None and b["N_pct"] <= 10.0:
            chosen = m
    if chosen:
        n = bands[chosen]["N_pct"]
        print("DECISION: tier 3 carried by %s, threshold = max(2 x %.2f %%, 5 %%) = %.2f %%"
              % (chosen, n, max(2 * n, 5.0)))
    else:
        print("DECISION: tier 3 DROPPED -- N > 10 %% on both candidates. The healthy-path"
              " claim rests on S1-M (tier 1) and the tier 2 count invariants.")

if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "runs")
