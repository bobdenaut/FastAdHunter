#!/usr/bin/env python3
"""Strictly sequential UDP DNS load generator for the P2.6-11 L.4 arms.

One query in flight at a time: send, wait for the answer, then send the next.
B.1's criterion counts queries dispatched after the penalty landed, and
concurrent in-flight queries also pay (design S1.3), which would blur the count.

Every query is logged with its wall-clock offset and its latency, so the cost of
the first attempts against a dead endpoint is visible per query rather than only
in aggregate.

  python seq.py --server 172.17.0.4 --port 53 --seconds 3600 --out l4a.jsonl
"""

import argparse
import json
import socket
import struct
import sys
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
    p.add_argument("--seconds", type=int, required=True)
    p.add_argument("--prefix", default="l4")
    p.add_argument("--timeout", type=float, default=5.0)
    p.add_argument("--out", required=True)
    a = p.parse_args()

    dst = (a.server, a.port)
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(a.timeout)

    start = time.time()
    deadline = start + a.seconds
    sent = answered = timedout = 0
    latencies = []

    with open(a.out, "w", encoding="utf-8") as log:
        while time.time() < deadline:
            qid = sent & 0xFFFF
            name = "q%d.%s.bench.invalid" % (sent, a.prefix)
            t0 = time.time()
            sock.sendto(query(qid, name), dst)
            sent += 1
            try:
                while True:
                    data, _ = sock.recvfrom(4096)
                    if len(data) >= 2 and struct.unpack(">H", data[:2])[0] == qid:
                        break
                t1 = time.time()
                ms = (t1 - t0) * 1000.0
                answered += 1
                latencies.append(ms)
                status = "ok"
            except socket.timeout:
                t1 = time.time()
                ms = (t1 - t0) * 1000.0
                timedout += 1
                status = "timeout"
            log.write(json.dumps({
                "n": sent - 1,
                "offset_s": round(t0 - start, 4),
                "ms": round(ms, 3),
                "status": status,
            }) + "\n")

    elapsed = time.time() - start
    latencies.sort()
    def pct(q):
        if not latencies:
            return None
        return round(latencies[min(len(latencies) - 1, int(len(latencies) * q))], 3)

    json.dump({
        "sent": sent,
        "answered": answered,
        "timeouts": timedout,
        "elapsed_s": round(elapsed, 2),
        "achieved_qps": round(sent / elapsed, 2),
        "p50_ms": pct(0.50),
        "p99_ms": pct(0.99),
        "max_ms": round(latencies[-1], 3) if latencies else None,
    }, sys.stdout, indent=1, sort_keys=True)
    print()

if __name__ == "__main__":
    main()
