import asyncio
import random
import time
import dns.asyncquery
import dns.message

SERVERS = [
    ("192.168.10.1", 53),
    ("2a02:2f04:5008:bb00::11", 53),
]

DOMAIN_FILE = "domains.txt"
WORKERS = 50
TYPES = ["A", "AAAA", "MX", "NS", "TXT", "SOA", "CNAME"]

queries = 0
errors = 0


# Fiecare worker primeste DOAR bucata lui de domenii (chunk)
async def worker(worker_id, chunk_domains):
    global queries, errors
    print(f"Worker {worker_id} a pornit cu {len(chunk_domains)} domenii.")
    rng = random.Random()
    num_domains = len(chunk_domains)

    if num_domains == 0:
        return

    while True:
        # Alege doar din bucata alocata acestui worker
        domain = chunk_domains[rng.randint(0, num_domains - 1)]
        qtype = rng.choice(TYPES)
        server_ip, port = rng.choice(SERVERS)

        msg = dns.message.make_query(domain, qtype)
        msg.use_edns(edns=0, payload=4096)

        try:
            await dns.asyncquery.udp(msg, server_ip, port=port, timeout=2.0)
        except Exception as e:
            errors += 1
            queries += 1
        await asyncio.sleep(0)

async def monitor():
    global queries, errors
    last = 0
    start = time.time()

    while True:
        await asyncio.sleep(1)
        q = queries
        e = errors

        delta = q - last
        last = q
        elapsed = time.time() - start

        print(
            f"{elapsed:8.1f}s | "
            f"total={q:15,d} | "
            f"qps={delta / 5:10,.0f} | "
            f"errors={e:,}"
        )

async def main():
    with open(DOMAIN_FILE, encoding="utf8") as f:
        domains = [x.strip() for x in f if x.strip()]

    total_domains = len(domains)
    print(f"Total Domains: {total_domains:,}")
    print(f"Workers: {WORKERS}")

    # Impartim lista in WORKERS bucati egale
    chunk_size = max(1, total_domains // WORKERS)
    chunks = [
        domains[i : i + chunk_size]
        for i in range(0, total_domains, chunk_size)
    ]

    print(f"Domenii / worker: ~{len(chunks[0]):,}\n")

    asyncio.create_task(monitor())

    tasks = []
    for i in range(min(WORKERS, len(chunks))):
        tasks.append(asyncio.create_task(worker(i, chunks[i])))

    await asyncio.gather(*tasks)


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\nTest oprit de utilizator.")