"""Household-like light DNS workload against the dev-box repro container.

Mix calibrated to fah-next pull-4 lifetime counters (326 951 queries over
315 123 s ~= 1.04 QPS): block 70.9 %, fresh hit 12.0 %, stale hit 14.9 %,
miss 2.2 %. Stale hits are produced by a warm pool re-queried at ~800 s per
name (TTL floor 600 s), which also reproduces the ~0.15/s SWR refresh rate.
"""

import random
import socket
import struct
import sys
import threading
import time

SERVER = ("127.0.0.1", 15353)
QPS = 1.2
SECONDS = 24 * 3600

AD_BASES = [
    "doubleclick.net", "googlesyndication.com", "googleadservices.com",
    "google-analytics.com", "adservice.google.com", "adnxs.com",
    "criteo.com", "taboola.com", "outbrain.com", "scorecardresearch.com",
    "moatads.com", "adsafeprotected.com", "amazon-adsystem.com",
    "casalemedia.com", "pubmatic.com", "rubiconproject.com",
    "openx.net", "smartadserver.com", "quantserve.com", "krxd.net",
]
HOT = [
    "google.com", "www.google.com", "youtube.com", "www.youtube.com",
    "wikipedia.org", "en.wikipedia.org", "github.com", "api.github.com",
    "cloudflare.com", "www.cloudflare.com", "microsoft.com",
    "www.microsoft.com", "apple.com", "www.apple.com", "amazon.com",
    "www.amazon.com", "netflix.com", "www.netflix.com", "reddit.com",
    "www.reddit.com", "stackoverflow.com", "mozilla.org", "www.mozilla.org",
    "debian.org", "kernel.org", "archlinux.org", "python.org",
    "www.python.org", "rust-lang.org", "www.rust-lang.org",
]
WARM_BASES = [
    "example.com", "example.org", "example.net", "iana.org", "ietf.org",
    "w3.org", "gnu.org", "fsf.org", "apache.org", "nginx.org",
    "openssl.org", "curl.se", "sqlite.org", "postgresql.org", "mariadb.org",
    "docker.com", "kubernetes.io", "golang.org", "nodejs.org", "npmjs.com",
    "pypi.org", "crates.io", "docs.rs", "gitlab.com", "bitbucket.org",
    "sourceforge.net", "eff.org", "letsencrypt.org", "digicert.com",
    "quad9.net", "one.one.one.one", "dns.google", "opendns.com",
    "ubuntu.com", "fedoraproject.org", "opensuse.org", "alpinelinux.org",
    "freebsd.org", "openbsd.org", "netbsd.org", "gentoo.org", "slackware.com",
    "x.org", "wayland.freedesktop.org", "gnome.org", "kde.org",
    "libreoffice.org", "videolan.org", "blender.org", "gimp.org",
]
WARM = WARM_BASES + ["www." + b for b in WARM_BASES] + [
    "mail." + b for b in WARM_BASES[:50]
]
WARM = WARM[:150]

def encode_name(name):
    out = b""
    for label in name.strip(".").split("."):
        b = label.encode("ascii")
        out += bytes([len(b)]) + b
    return out + b"\x00"

def query(qid, name):
    return (struct.pack(">HHHHHH", qid, 0x0100, 1, 0, 0, 0)
            + encode_name(name) + struct.pack(">HH", 1, 1))

def main():
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(0.5)
    stop = threading.Event()

    def drain():
        while not stop.is_set():
            try:
                sock.recvfrom(2048)
            except (socket.timeout, OSError):
                continue

    threading.Thread(target=drain, daemon=True).start()

    rng = random.Random(20260829)
    qid = 0
    n_miss = 0
    hot_i = 0
    warm_i = 0
    sent = 0
    t_start = time.time()
    deadline = t_start + SECONDS
    next_send = t_start
    while time.time() < deadline:
        now = time.time()
        if now < next_send:
            time.sleep(min(next_send - now, 0.05))
            continue
        r = rng.random()
        if r < 0.709:
            base = rng.choice(AD_BASES)
            name = f"sub{rng.randrange(50)}.{base}"
        elif r < 0.829:
            name = HOT[hot_i % len(HOT)]
            hot_i += 1
        elif r < 0.978:
            name = WARM[warm_i % len(WARM)]
            warm_i += 1
        else:
            n_miss += 1
            name = f"miss{n_miss:06d}.fah-repro-miss.com"
        qid = (qid + 1) & 0xFFFF
        try:
            sock.sendto(query(qid, name), SERVER)
            sent += 1
        except OSError:
            pass
        next_send += 1.0 / QPS
        if sent % 3600 == 0:
            print(f"sent {sent} at {time.strftime('%H:%M:%S')}", flush=True)
    stop.set()
    print(f"workload done: {sent} sent")
    return 0

if __name__ == "__main__":
    sys.exit(main())
