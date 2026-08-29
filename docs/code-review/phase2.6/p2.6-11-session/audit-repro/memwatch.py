import json, time, urllib.request, ssl, sys, os

KEY = "<API_KEY_REDACTED>"
URL = "https://172.17.0.2:8443/api/v1/debug/memory"
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "memwatch.jsonl")
DURATION_S = 8 * 3600
PERIOD_S = 30

ctx = ssl.create_default_context()
ctx.check_hostname = False
ctx.verify_mode = ssl.CERT_NONE

t_end = time.time() + DURATION_S
n = err = 0
with open(OUT, "a", encoding="ascii") as f:
    while time.time() < t_end:
        t0 = time.time()
        try:
            req = urllib.request.Request(URL, headers={"Authorization": "Bearer " + KEY})
            with urllib.request.urlopen(req, timeout=10, context=ctx) as r:
                d = json.loads(r.read())
            d["ts"] = round(t0, 1)
            f.write(json.dumps(d, separators=(",", ":")) + "\n")
            f.flush()
            n += 1
        except Exception as e:
            err += 1
            f.write(json.dumps({"ts": round(t0, 1), "error": str(e)[:120]}) + "\n")
            f.flush()
        time.sleep(max(0.0, PERIOD_S - (time.time() - t0)))
print(f"memwatch done: {n} samples, {err} errors -> {OUT}")
