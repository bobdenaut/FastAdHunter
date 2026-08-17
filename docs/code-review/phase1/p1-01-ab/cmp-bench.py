import re, sys

UNIT = {'ps': 1e-3, 'ns': 1.0, 'us': 1e3, 'us2': 1e3, 'ms': 1e6, 's': 1e9}


def unit(tok):
    """Criterion's µs survives the console as mojibake; anything non-ascii
    before the trailing 's' is a micro sign."""
    if tok in UNIT:
        return UNIT[tok]
    if tok.endswith('s') and not tok[:-1].isascii():
        return 1e3
    raise KeyError(tok)
THRPT = {'elem/s': 1.0, 'Kelem/s': 1e3, 'Melem/s': 1e6, 'Gelem/s': 1e9}


def parse(path):
    """bench id -> (median_ns, lo_ns, hi_ns, thrpt_median or None)."""
    out, cur = {}, None
    for line in open(path, encoding='utf-8', errors='replace'):
        line = line.rstrip()
        m = re.match(r'^(\S+/\S+)\s*$', line) or re.match(r'^(\S+/\S+)\s+time:', line)
        if m:
            cur = m.group(1)
        m = re.search(r'time:\s+\[([\d.]+) (\S+) ([\d.]+) (\S+) ([\d.]+) (\S+)\]', line)
        if m and cur:
            lo = float(m.group(1)) * unit(m.group(2))
            md = float(m.group(3)) * unit(m.group(4))
            hi = float(m.group(5)) * unit(m.group(6))
            out[cur] = [md, lo, hi, None]
        m = re.search(r'thrpt:\s+\[[\d.]+ \S+ ([\d.]+) (\S+) [\d.]+ \S+\]', line)
        if m and cur and cur in out:
            out[cur][3] = float(m.group(1)) * THRPT.get(m.group(2), 1.0)
        m = re.match(r'peak_working_set_bytes=(\d+)', line)
        if m:
            out['__peak__'] = [float(m.group(1)), 0, 0, None]
    return out


def fmt(ns):
    for unit, scale in (('s', 1e9), ('ms', 1e6), ('us2', 1e3)):
        if ns >= scale:
            return f'{ns / scale:.4g} {unit}'
    return f'{ns:.4g} ns'


before, after = parse(sys.argv[1]), parse(sys.argv[2])
print(f"{'bench':<48}{'before':>13}{'after':>13}{'delta':>9}   {'CI width (after)':>16}")
for key in sorted(set(before) | set(after)):
    if key not in before or key not in after:
        print(f'{key:<48}{"(missing in one arm)":>35}')
        continue
    b, a = before[key], after[key]
    if key == '__peak__':
        print(f'{"peak working set":<48}{b[0]/2**20:>10.2f} MiB{a[0]/2**20:>10.2f} MiB'
              f'{(a[0]-b[0])/b[0]*100:>8.1f}%')
        continue
    ci = (a[2] - a[1]) / a[0] * 100
    flag = '  <- noise' if abs(ci) > abs((a[0] - b[0]) / b[0] * 100) else ''
    print(f'{key:<48}{fmt(b[0]):>13}{fmt(a[0]):>13}'
          f'{(a[0]-b[0])/b[0]*100:>8.1f}%{ci:>15.1f}%{flag}')
    if a[3] and b[3]:
        print(f'{"  +- thrpt":<48}{b[3]/1e3:>9.1f} Ke/s{a[3]/1e3:>9.1f} Ke/s'
              f'{(a[3]-b[3])/b[3]*100:>8.1f}%')
