import re, glob, json, sys

NAME_TIME = re.compile(r'^(\S+)\s+(time|thrpt):\s+\[(.+?)\]\s*$')
BARE_NAME = re.compile(r'^(\S+)\s*$')
IND_VAL   = re.compile(r'^\s+(time|thrpt):\s+\[(.+?)\]\s*$')

def parse(path):
    arms, name, seen = {}, None, set()
    for raw in open(path, encoding='utf-8', errors='replace'):
        line = raw.rstrip('\n')
        if line.strip().startswith('change:'):
            seen.add((name, 'change')); continue
        m = NAME_TIME.match(line)
        if m:
            name = m.group(1)
            arms.setdefault(name, {}).setdefault(m.group(2), m.group(3))
            continue
        m = IND_VAL.match(line)
        if m and name:
            if (name, 'change') in seen:
                continue
            arms.setdefault(name, {}).setdefault(m.group(1), m.group(2))
            continue
        m = BARE_NAME.match(line)
        if m and '/' in m.group(1):
            name = m.group(1); seen.discard((name, 'change'))
    return arms

def mid(v):
    if not v: return None
    parts = v.split()
    return ' '.join(parts[len(parts)//2 - 1: len(parts)//2 + 1]) if len(parts) == 6 else v

out = {}
for f in sorted(glob.glob('D*.txt')):
    out[f] = parse(f)
json.dump(out, open('parsed.json','w'), indent=1)
for f, arms in out.items():
    print('##', f)
    for k, v in arms.items():
        print(f'    {k:52s} {mid(v.get("time","")):22s} {mid(v.get("thrpt","")) or ""}')
