#!/usr/bin/env python3
"""
test_aleator.py — DNS traffic generator for FastAdHunter soak / verification.

Supersedes plan/closed/phase1/test_aleator.py (kept there as the historical
Phase 1 soak artifact). Two profiles, one script:

  real   — models N household devices as realistically as practical:
             * Zipfian *shared* domain popularity (everyone hits the same
               few CDNs/telemetry hosts -> realistic cross-client cache
               sharing and hit ratio);
             * realistic query-type mix (mostly A/AAAA, some HTTPS/type-65,
               a little PTR, rare MX/TXT/NS/SOA);
             * per-device think time between page visits + a burst of
               overlapping lookups per visit (a page pulling third-party
               resources), with intra-burst jitter;
             * happy-eyeballs A+AAAA pairing.
           This is deliberately *low* aggregate QPS — that is what a real
           house looks like. Turn --think-mean down for a busy evening.

  stress — the adversarial worst-case hammer (max rate, uniform types over
           all domains, big EDNS payload) that pushes the cache *byte*
           ceiling. Use this for the byte-cap check in p1.5-07.

Runs multiprocess (one asyncio event loop per client core, --procs) so the
generator itself is not the bottleneck — a single-core asyncio client can
silently cap the load and make the router look idle. Client-observed latency
is tracked with a fixed-bucket histogram (bounded memory) and reported as
p50 / p99 / p99.9, merged across all processes.

Requires: dnspython  (pip install dnspython)

Examples:
  # realistic ~25-device household, run until Ctrl-C
  python test_aleator.py --profile real

  # busier evening, 40 devices, 1 hour, only the v4 resolver
  python test_aleator.py --profile real --clients 40 --think-mean 6 \
      --duration 3600 --server 192.168.10.1

  # adversarial byte-cap hammer for the p1.5-07 cache-cap check
  python test_aleator.py --profile stress --workers 64 --duration 1800
"""

import argparse
import asyncio
import bisect
import os
import random
import time
from multiprocessing import Array, Process

import dns.asyncquery
import dns.exception
import dns.message
import dns.rdatatype
import dns.reversename

# ---------------------------------------------------------------------------
# Configuration constants
# ---------------------------------------------------------------------------

DEFAULT_SERVERS = ["192.168.10.1", "2a02:2f04:5008:bb00::11"]  # v4 + v6, port 53
DEFAULT_DOMAINS = "domains.txt"

# Fixed seed so the *popularity ranking* of domains is identical across every
# process/device -> the same handful of hosts are globally "popular", which is
# what produces realistic shared-cache behaviour. Per-query randomness still
# uses each process's own (entropy-seeded) RNG.
POPULARITY_SEED = 1234
ZIPF_ALPHA = 1.1

# Realistic client-side query-type mix (weights need not sum to 1).
REAL_TYPES = [
    ("A", 0.45),
    ("AAAA", 0.40),
    ("HTTPS", 0.10),  # type 65 — modern browsers query this heavily
    ("PTR", 0.02),
    ("MX", 0.008),
    ("TXT", 0.008),
    ("NS", 0.007),
    ("SOA", 0.007),
]
REAL_TYPE_NAMES = [t for t, _ in REAL_TYPES]
REAL_TYPE_W = [w for _, w in REAL_TYPES]

# Adversarial hammer: uniform over these (overweights big TXT/SOA on purpose).
STRESS_TYPES = ["A", "AAAA", "MX", "NS", "TXT", "SOA", "CNAME"]

# Latency histogram: fixed upper edges in ms; an implicit +inf overflow bucket
# catches timeouts and anything slower than the last edge. Bounded memory.
LAT_BUCKETS_MS = [0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000]
NB = len(LAT_BUCKETS_MS) + 1  # + overflow bucket

# Pre-resolve rdata types once (avoid per-query from_text cost / handle old
# dnspython that lacks HTTPS).
def _rdtype(name):
    try:
        return dns.rdatatype.from_text(name)
    except Exception:
        return dns.rdatatype.A


TYPE_RD = {name: _rdtype(name) for name in set(REAL_TYPE_NAMES + STRESS_TYPES + ["PTR"])}

FLUSH_SECS = 0.3  # how often each process publishes its counters to the parent


# ---------------------------------------------------------------------------
# Small helpers
# ---------------------------------------------------------------------------

class Local:
    """Per-process counters (single-threaded asyncio -> no lock needed)."""

    __slots__ = ("q", "err", "to", "hist")

    def __init__(self):
        self.q = 0
        self.err = 0
        self.to = 0
        self.hist = [0] * NB


def bucket_of(ms):
    for i, edge in enumerate(LAT_BUCKETS_MS):
        if ms <= edge:
            return i
    return len(LAT_BUCKETS_MS)  # overflow


def pct_label(agg, frac):
    total = sum(agg)
    if total == 0:
        return "-"
    target = total * frac
    c = 0
    for i, cnt in enumerate(agg):
        c += cnt
        if c >= target:
            if i < len(LAT_BUCKETS_MS):
                return f"<={LAT_BUCKETS_MS[i]:g}ms"
            return ">2000ms"
    return ">2000ms"


def parse_server(s):
    """Accept 'host', 'host:port', or '[v6]:port'. Default port 53."""
    if s.startswith("["):
        host, _, rest = s[1:].partition("]")
        port = int(rest[1:]) if rest.startswith(":") else 53
        return host, port
    if s.count(":") == 1:  # ipv4:port  (v6 literals have >1 colon)
        host, port = s.rsplit(":", 1)
        return host, int(port)
    return s, 53  # bare ipv4 or bare v6 literal


def server_weights(servers, v4_share):
    """Split load across families: v4_share to v4 resolvers, rest to v6."""
    v4 = [s for s in servers if ":" not in s[0]]
    v6 = [s for s in servers if ":" in s[0]]
    weights = []
    for ip, _ in servers:
        if ":" in ip:
            weights.append((1 - v4_share) / len(v6) if v6 else 0.0)
        else:
            weights.append(v4_share / len(v4) if v4 else 0.0)
    tot = sum(weights) or 1.0
    return [w / tot for w in weights]


def build_zipf_cdf(n):
    weights = [1.0 / ((i + 1) ** ZIPF_ALPHA) for i in range(n)]
    total = sum(weights)
    cdf, acc = [], 0.0
    for w in weights:
        acc += w / total
        cdf.append(acc)
    return cdf


def load_domains(path):
    with open(path, encoding="utf8") as f:
        domains = [x.strip() for x in f if x.strip()]
    # Shuffle deterministically so rank (= popularity) is consistent across
    # every process, without depending on the file's ordering.
    random.Random(POPULARITY_SEED).shuffle(domains)
    return domains


async def sleep_or_stop(stop, secs):
    """Sleep, but wake immediately if the stop event fires. True if stopped."""
    if secs <= 0:
        return stop.is_set()
    try:
        await asyncio.wait_for(stop.wait(), timeout=secs)
        return True
    except asyncio.TimeoutError:
        return False


# ---------------------------------------------------------------------------
# The actual query
# ---------------------------------------------------------------------------

async def one_query(server, name, qtype, payload, timeout, local):
    ip, port = server
    msg = dns.message.make_query(name, TYPE_RD.get(qtype, dns.rdatatype.A))
    msg.use_edns(edns=0, payload=payload)
    t0 = time.perf_counter()
    try:
        await dns.asyncquery.udp(msg, ip, port=port, timeout=timeout)
        dt_ms = (time.perf_counter() - t0) * 1000.0
        local.q += 1
        local.hist[bucket_of(dt_ms)] += 1
    except (asyncio.TimeoutError, dns.exception.Timeout):
        local.q += 1
        local.to += 1
        local.err += 1
        local.hist[NB - 1] += 1  # timeout -> overflow bucket (counts in p99)
    except Exception:
        local.q += 1
        local.err += 1


def pick_query(rng, domains, cdf):
    """Return (name, qtype) for the 'real' profile."""
    qtype = rng.choices(REAL_TYPE_NAMES, weights=REAL_TYPE_W, k=1)[0]
    if qtype == "PTR":
        ip = f"{rng.randint(1, 223)}.{rng.randint(0, 255)}." \
             f"{rng.randint(0, 255)}.{rng.randint(1, 254)}"
        return dns.reversename.from_address(ip), "PTR"
    idx = bisect.bisect_left(cdf, rng.random())
    if idx >= len(domains):
        idx = len(domains) - 1
    return domains[idx], qtype


# ---------------------------------------------------------------------------
# Behaviours
# ---------------------------------------------------------------------------

async def device(args, server, domains, cdf, rng, local, stop):
    """One household device: idle, then a burst of overlapping lookups."""
    payload = 1232  # modern EDNS default (DNS flag-day recommendation)
    while not stop.is_set():
        # Think time between page visits (exponential arrivals).
        if await sleep_or_stop(stop, rng.expovariate(1.0 / args.think_mean)):
            break
        burst = rng.randint(args.burst_min, args.burst_max)
        pending = []
        for _ in range(burst):
            if stop.is_set():
                break
            name, qtype = pick_query(rng, domains, cdf)
            pending.append(
                asyncio.create_task(one_query(server, name, qtype, payload,
                                              args.timeout, local))
            )
            # Happy-eyeballs: a real client fires A and AAAA together.
            if qtype in ("A", "AAAA") and rng.random() < args.happy_eyeballs:
                sib = "AAAA" if qtype == "A" else "A"
                pending.append(
                    asyncio.create_task(one_query(server, name, sib, payload,
                                                  args.timeout, local))
                )
            # Intra-burst jitter — a page fires requests over a few hundred ms,
            # overlapping rather than all at the same instant.
            if await sleep_or_stop(stop, rng.uniform(args.intra_min, args.intra_max)):
                break
        if pending:
            await asyncio.gather(*pending, return_exceptions=True)


async def stress_worker(args, servers, weights, domains, rng, local, stop):
    """Adversarial worst case: max rate, uniform types over all domains."""
    payload = 4096  # let big TXT/SOA responses through -> max byte pressure
    n = len(domains)
    while not stop.is_set():
        name = domains[rng.randrange(n)]
        qtype = rng.choice(STRESS_TYPES)
        server = rng.choices(servers, weights=weights, k=1)[0]
        await one_query(server, name, qtype, payload, args.timeout, local)
        await asyncio.sleep(0)  # yield, but never idle


async def flusher(pid, local, s_q, s_err, s_to, s_hist, stop):
    base = pid * NB

    def publish():
        s_q[pid] = local.q
        s_err[pid] = local.err
        s_to[pid] = local.to
        for i in range(NB):
            s_hist[base + i] = local.hist[i]

    while not await sleep_or_stop(stop, FLUSH_SECS):
        publish()
    publish()  # final


# ---------------------------------------------------------------------------
# Per-process entry point
# ---------------------------------------------------------------------------

async def process_main(pid, args, servers, weights, units, s_q, s_err, s_to, s_hist):
    domains = load_domains(args.domains)
    cdf = build_zipf_cdf(len(domains)) if args.profile == "real" else None
    rng = random.Random()  # entropy-seeded; per-process query randomness
    local = Local()
    stop = asyncio.Event()

    if args.duration > 0:
        asyncio.get_event_loop().call_later(args.duration, stop.set)

    tasks = [asyncio.create_task(flusher(pid, local, s_q, s_err, s_to, s_hist, stop))]
    if args.profile == "real":
        for _ in range(units):
            srv = rng.choices(servers, weights=weights, k=1)[0]  # per-device resolver
            tasks.append(asyncio.create_task(
                device(args, srv, domains, cdf, rng, local, stop)))
    else:
        for _ in range(units):
            tasks.append(asyncio.create_task(
                stress_worker(args, servers, weights, domains, rng, local, stop)))

    await asyncio.gather(*tasks, return_exceptions=True)


def run_process(pid, args, servers, weights, units, s_q, s_err, s_to, s_hist):
    try:
        asyncio.run(process_main(pid, args, servers, weights, units,
                                 s_q, s_err, s_to, s_hist))
    except KeyboardInterrupt:
        pass


# ---------------------------------------------------------------------------
# Parent: spawn, monitor, summarise
# ---------------------------------------------------------------------------

def units_for(pid, total, procs):
    return total // procs + (1 if pid < total % procs else 0)


def monitor(args, procs, children, s_q, s_err, s_to, s_hist):
    start = time.time()
    last_q, last_t = 0, start
    print(f"{'time':>8} | {'total':>14} | {'qps':>9} | "
          f"{'errors':>12} | latency (client-observed)")
    try:
        while any(p.is_alive() for p in children):
            time.sleep(1.0)
            tq, te, tt = sum(s_q), sum(s_err), sum(s_to)
            now = time.time()
            dt = now - last_t
            qps = (tq - last_q) / dt if dt > 0 else 0.0
            agg = [0] * NB
            for pid in range(procs):
                b = pid * NB
                for i in range(NB):
                    agg[i] += s_hist[b + i]
            print(f"{now - start:8.1f}s | {tq:14,d} | {qps:9,.0f} | "
                  f"{te:8,d} (to={tt:,}) | "
                  f"p50={pct_label(agg, .50)} p99={pct_label(agg, .99)} "
                  f"p99.9={pct_label(agg, .999)}")
            last_q, last_t = tq, now
    except KeyboardInterrupt:
        print("\nStopping (SIGINT)...")
        for p in children:
            p.terminate()

    for p in children:
        p.join()

    tq, te, tt = sum(s_q), sum(s_err), sum(s_to)
    agg = [0] * NB
    for pid in range(procs):
        b = pid * NB
        for i in range(NB):
            agg[i] += s_hist[b + i]
    elapsed = time.time() - start
    print("\n" + "=" * 62)
    print(f"  profile        : {args.profile}")
    print(f"  duration       : {elapsed:,.1f}s")
    print(f"  total queries  : {tq:,}")
    print(f"  avg qps        : {tq / elapsed if elapsed else 0:,.0f}")
    print(f"  errors         : {te:,} "
          f"({100 * te / tq if tq else 0:.3f}%)  timeouts={tt:,}")
    print(f"  latency  p50   : {pct_label(agg, .50)}")
    print(f"           p99   : {pct_label(agg, .99)}")
    print(f"           p99.9 : {pct_label(agg, .999)}")
    print("=" * 62)


def parse_args():
    p = argparse.ArgumentParser(
        description="Realistic (or adversarial) DNS traffic generator.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    p.add_argument("--profile", choices=["real", "stress"], default="real")
    p.add_argument("--server", action="append", metavar="HOST[:PORT]",
                   help="Resolver to hit (repeatable). Default: v4 + v6 router.")
    p.add_argument("--domains", default=DEFAULT_DOMAINS)
    p.add_argument("--procs", type=int, default=0,
                   help="Client processes (0 = CPU count).")
    p.add_argument("--duration", type=float, default=0,
                   help="Seconds to run (0 = until Ctrl-C).")
    p.add_argument("--timeout", type=float, default=2.0)
    p.add_argument("--v4-share", type=float, default=0.6,
                   help="Fraction of devices/queries sent to v4 resolvers.")
    # real profile knobs
    p.add_argument("--clients", type=int, default=25,
                   help="[real] household devices to model.")
    p.add_argument("--think-mean", type=float, default=20.0,
                   help="[real] mean idle seconds between page visits.")
    p.add_argument("--burst-min", type=int, default=5,
                   help="[real] min lookups per page visit.")
    p.add_argument("--burst-max", type=int, default=40,
                   help="[real] max lookups per page visit.")
    p.add_argument("--intra-min", type=float, default=0.005,
                   help="[real] min seconds between lookups within a burst.")
    p.add_argument("--intra-max", type=float, default=0.06,
                   help="[real] max seconds between lookups within a burst.")
    p.add_argument("--happy-eyeballs", type=float, default=0.7,
                   help="[real] prob. an A/AAAA also fires its sibling.")
    # stress profile knobs
    p.add_argument("--workers", type=int, default=50,
                   help="[stress] total max-rate workers.")
    return p.parse_args()


def main():
    args = parse_args()
    raw = args.server if args.server else DEFAULT_SERVERS
    servers = [parse_server(s) for s in raw]
    weights = server_weights(servers, args.v4_share)
    procs = args.procs if args.procs > 0 else (os.cpu_count() or 1)
    total_units = args.clients if args.profile == "real" else args.workers
    procs = max(1, min(procs, total_units))

    print(f"FastAdHunter traffic generator - profile={args.profile}")
    print(f"  servers : {', '.join(f'{ip}:{port}' for ip, port in servers)}")
    print(f"  procs   : {procs}")
    print(f"  units   : {total_units} "
          f"{'devices' if args.profile == 'real' else 'workers'} "
          f"(~{total_units / procs:.1f}/proc)")
    if args.profile == "real":
        print(f"  model   : think~{args.think_mean}s, burst {args.burst_min}-"
              f"{args.burst_max}, happy-eyeballs {args.happy_eyeballs}")
    print(f"  duration: {'until Ctrl-C' if args.duration <= 0 else str(args.duration) + 's'}\n")

    s_q = Array("Q", procs, lock=False)
    s_err = Array("Q", procs, lock=False)
    s_to = Array("Q", procs, lock=False)
    s_hist = Array("Q", procs * NB, lock=False)

    children = []
    for pid in range(procs):
        units = units_for(pid, total_units, procs)
        p = Process(target=run_process,
                    args=(pid, args, servers, weights, units,
                          s_q, s_err, s_to, s_hist))
        p.start()
        children.append(p)

    monitor(args, procs, children, s_q, s_err, s_to, s_hist)


if __name__ == "__main__":
    main()
