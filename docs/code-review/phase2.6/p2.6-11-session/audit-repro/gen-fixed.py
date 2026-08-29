#!/usr/bin/env python3
"""Open-loop UDP DNS load generator for the P2.6-10 dry run.

Fixed offered rate, two streams (forward = unique names, control = a small
pre-warmed cacheable set). Reports offered vs achieved rate and client-side
latency; the generator, not the server, is the authority on QPS.

  python gen.py --server 127.0.0.1 --port 5353 --qfile q.txt --ctlfile ctl.txt \
      --qps 2000 --ctl-qps 200 --seconds 120
"""

import argparse
import json
import socket
import struct
import sys
import threading
import time

def encode_name(name):
    out = b""
    for label in name.strip(".").split("."):
        b = label.encode("ascii")
        out += bytes([len(b)]) + b
    return out + b"\x00"

def query(qid, name, qtype=1):
    return (struct.pack(">HHHHHH", qid, 0x0100, 1, 0, 0, 0)
            + encode_name(name) + struct.pack(">HH", qtype, 1))

def main():
    p = argparse.ArgumentParser()
    p.add_argument("--server", required=True)
    p.add_argument("--port", type=int, default=53)
    p.add_argument("--qfile", required=True)
    p.add_argument("--ctlfile", required=True)
    p.add_argument("--qps", type=int, required=True)
    p.add_argument("--ctl-qps", type=int, required=True)
    p.add_argument("--seconds", type=int, required=True)
    a = p.parse_args()

    fwd = [l.strip() for l in open(a.qfile, encoding="ascii") if l.strip()]
    ctl = [l.strip() for l in open(a.ctlfile, encoding="ascii") if l.strip()]
    dst = (a.server, a.port)

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 4 << 20)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 8 << 20)
    sock.settimeout(0.5)

    sent_at = [0.0] * 65536
    stats = {"recv": 0, "lat": []}
    stop = threading.Event()

    def receiver():
        while not stop.is_set():
            try:
                data, _ = sock.recvfrom(2048)
            except (socket.timeout, OSError):
                continue
            if len(data) < 2:
                continue
            qid = struct.unpack(">H", data[:2])[0]
            t = sent_at[qid]
            if t:
                stats["lat"].append(time.perf_counter() - t)
            stats["recv"] += 1

    rx = threading.Thread(target=receiver, daemon=True)
    rx.start()

    total_rate = a.qps + a.ctl_qps
    burst = max(1, total_rate // 200)          # 5 ms pacing granularity
    interval = burst / float(total_rate)

    qid = 0
    n_fwd = 0
    n_ctl = 0
    ctl_acc = 0
    sent = 0
    errors = 0
    t_start = time.perf_counter()
    deadline = t_start + a.seconds
    next_send = t_start
    while True:
        now = time.perf_counter()
        if now >= deadline:
            break
        if now < next_send:
            time.sleep(min(next_send - now, 0.002))
            continue
        for _ in range(burst):
            # Bresenham interleave: exact ctl_qps/total_rate control fraction
            # for any integer ratio, with independent per-stream counters so
            # both name sets are covered fully regardless of the ratio's
            # parity (the old shared-counter modulo aliased even splits onto
            # half of each set).
            ctl_acc += a.ctl_qps
            if ctl_acc >= total_rate:
                ctl_acc -= total_rate
                name = ctl[n_ctl % len(ctl)]
                n_ctl += 1
            else:
                name = fwd[n_fwd % len(fwd)]
                n_fwd += 1
            qid = (qid + 1) & 0xFFFF
            sent_at[qid] = time.perf_counter()
            try:
                sock.sendto(query(qid, name), dst)
                sent += 1
            except OSError:
                errors += 1
        next_send += interval

    elapsed_send = time.perf_counter() - t_start
    time.sleep(2.0)                            # drain in-flight answers
    stop.set()
    rx.join(timeout=3)

    lat = sorted(stats["lat"])
    def q(x):
        return lat[min(len(lat) - 1, int(x * len(lat)))] * 1000 if lat else float("nan")

    out = {
        "offered_qps": total_rate,
        "sent": sent,
        "send_errors": errors,
        "received": stats["recv"],
        "lost": sent - stats["recv"],
        "elapsed_send_seconds": round(elapsed_send, 3),
        "achieved_send_qps": round(sent / elapsed_send, 1),
        "achieved_answer_qps": round(stats["recv"] / elapsed_send, 1),
        "client_p50_ms": round(q(0.50), 3),
        "client_p99_ms": round(q(0.99), 3),
        "client_max_ms": round(lat[-1] * 1000, 3) if lat else float("nan"),
        "latency_samples": len(lat),
    }
    print(json.dumps(out, indent=1))
    return 0

if __name__ == "__main__":
    sys.exit(main())
