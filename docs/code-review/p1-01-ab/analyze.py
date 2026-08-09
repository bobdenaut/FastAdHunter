import re, sys, statistics as st

def load(path):
    runs, cur = [], None
    for line in open(path):
        m = re.match(r'=== iter=(\d+) arm=(\w+) mode=(\w+) ===', line)
        if m:
            cur = {'iter': int(m.group(1)), 'arm': m.group(2)}
            runs.append(cur); continue
        m = re.match(r'\[ab\] (\w+)=([\d.]+)', line)
        if m and cur is not None:
            cur[m.group(1)] = float(m.group(2))
    return runs

def report(path, keys, drop_first):
    runs = load(path)
    print(f"\n===== {path.split(chr(92))[-1]}  (n={len(runs)//2} pairs, first {drop_first} dropped as warm-up) =====")
    print(f"{'metric':<26}{'before med':>12}{'after med':>12}{'delta':>10}{'before min':>12}{'after min':>12}{'delta':>10}")
    for k in keys:
        b = [r[k] for r in runs if r['arm']=='before' and k in r][drop_first:]
        a = [r[k] for r in runs if r['arm']=='after'  and k in r][drop_first:]
        if not b or not a: continue
        bm, am = st.median(b), st.median(a)
        bn, an = min(b), min(a)
        scale = 1/1048576 if 'bytes' in k else 1.0
        unit = ' MiB' if 'bytes' in k else ''
        print(f"{k:<26}{bm*scale:>12.2f}{am*scale:>12.2f}{(am-bm)/bm*100:>9.1f}%{bn*scale:>12.2f}{an*scale:>12.2f}{(an-bn)/bn*100:>9.1f}%")

report(sys.argv[1], ['control_ms','parse_total_ms','parse_hosts_ms','parse_adblock_ms','parse_plain_ms','add_ms','build_ms','compile_total_ms','peak_working_set_bytes'], 2)
report(sys.argv[2], ['boot_ms','peak_working_set_bytes'], 2)
