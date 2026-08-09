import re, sys, statistics as st

runs, cur = [], None
for line in open(sys.argv[1], encoding='utf-8', errors='replace'):
    m = re.match(r'=== iter=(\d+) arm=(\w+) mode=(\w+) ===', line)
    if m:
        cur = {'iter': int(m.group(1)), 'arm': m.group(2), 'mode': m.group(3)}
        runs.append(cur)
        continue
    m = re.match(r'\[ab\] (\w+)=([\d.]+)', line)
    if m and cur is not None:
        cur[m.group(1)] = float(m.group(2))

DROP = 2  # warm-up iterations


def series(arm, mode, key):
    v = [r[key] for r in runs if r['arm'] == arm and r['mode'] == mode and key in r]
    return v[DROP:]


ROWS = [
    ('phases', 'parse_total_ms', 'parse total', 'ms'),
    ('phases', 'parse_hosts_ms', '  hosts (3 lists)', 'ms'),
    ('phases', 'parse_adblock_ms', '  adblock (12 lists)', 'ms'),
    ('phases', 'parse_plain_ms', '  plain-domain (1 list)', 'ms'),
    ('phases', 'add_ms', 'add_parsed_list', 'ms'),
    ('phases', 'build_ms', 'build', 'ms'),
    ('phases', 'compile_total_ms', 'compile total', 'ms'),
    ('phases', 'control_ms', 'control (FNV over corpus)', 'ms'),
    ('phases', 'peak_working_set_bytes', 'peak working set (phases)', 'MiB'),
    ('boot', 'boot_ms', 'BOOT (startup/refresh total)', 'ms'),
    ('boot', 'peak_working_set_bytes', 'peak working set (boot)', 'MiB'),
]

hdr = (f"{'metric':<30}{'before (a)':>12}{'after (cur)':>13}{'layout (b)':>12}"
       f"{'after vs a':>12}{'layout vs a':>13}{'spread a':>10}")
print(hdr)
print('-' * len(hdr))
for mode, key, label, unit in ROWS:
    a, c, b = series('a', mode, key), series('cur', mode, key), series('b', mode, key)
    if not (a and c and b):
        continue
    sc = 1 / 2**20 if unit == 'MiB' else 1.0
    ma, mc, mb = st.median(a) * sc, st.median(c) * sc, st.median(b) * sc
    spread = (max(a) - min(a)) / st.median(a) * 100
    print(f'{label:<30}{ma:>10.2f} {unit:<1}{mc:>11.2f} {unit:<1}{mb:>10.2f} {unit:<1}'
          f'{(mc-ma)/ma*100:>11.1f}%{(mb-ma)/ma*100:>12.1f}%{spread:>9.1f}%')

print()
for mode in ('phases', 'boot'):
    for key in ('compiled_rules', 'compiled_url_rules', 'compiled_heap_bytes',
                'parsed_active', 'parsed_url', 'parsed_inactive', 'parse_errors'):
        vals = {arm: sorted({r[key] for r in runs if r['arm'] == arm and r['mode'] == mode and key in r})
                for arm in ('a', 'cur', 'b')}
        if not vals['a']:
            continue
        same = vals['a'] == vals['cur'] == vals['b']
        mark = 'identical' if same else 'DIFFERS'
        print(f'{mode:<8}{key:<22}a={vals["a"]} cur={vals["cur"]} b={vals["b"]}  -> {mark}')
    break
