import { describe, expect, it } from 'vitest';
import {
  asciiLower,
  equalsAsciiCaseInsensitive,
  matchesClient,
  networkContains,
  parseIp,
  parseSelector,
  specificity,
} from './selectors';

function selector(spec: string) {
  const parsed = parseSelector(spec);
  if (parsed === null) throw new Error(`did not parse: ${spec}`);
  return parsed;
}

describe('parsing an address the way Rust does', () => {
  it('takes four dotted decimal octets', () => {
    expect(parseIp('192.168.1.50')?.bytes).toEqual([192, 168, 1, 50]);
  });

  // `Ipv4Addr::from_str` has rejected leading zeros since 1.53. Accepting them
  // would make the UI claim a client matched an assignment the engine cannot
  // even parse.
  it('rejects a leading zero in an octet', () => {
    expect(parseIp('192.168.010.5')).toBeNull();
    expect(parseIp('192.168.0.5')).not.toBeNull();
  });

  it('rejects an octet above 255 and a wrong octet count', () => {
    expect(parseIp('192.168.1.256')).toBeNull();
    expect(parseIp('192.168.1')).toBeNull();
    expect(parseIp('192.168.1.5.6')).toBeNull();
  });

  it('expands `::` and reads the embedded v4 form', () => {
    expect(parseIp('::')?.bytes).toEqual(new Array(16).fill(0));
    expect(parseIp('::1')?.bytes).toEqual([...new Array(15).fill(0), 1]);
    expect(parseIp('2001:db8::1')?.bytes).toEqual([
      0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
    ]);
    expect(parseIp('::ffff:1.2.3.4')?.bytes.slice(10)).toEqual([
      0xff, 0xff, 1, 2, 3, 4,
    ]);
  });

  it('rejects two `::`, a zone id and a short group list', () => {
    expect(parseIp('1::2::3')).toBeNull();
    expect(parseIp('fe80::1%eth0')).toBeNull();
    expect(parseIp('2001:db8:1:2:3:4:5')).toBeNull();
  });

  // F10 — Rust takes the embedded v4 form only as the final 32 bits of the
  // whole address. `1.2.3.4::` puts it in the half a `::` follows, so Rust
  // rejects the address — and then reads the selector as a Name.
  it('takes the embedded v4 form only at the very end of the address', () => {
    expect(parseIp('1.2.3.4::')).toBeNull();
    expect(parseIp('64:ff9b::192.0.2.1')).not.toBeNull();
    expect(parseIp('1:2:3:4:5:6:1.2.3.4')).not.toBeNull();
    expect(parseIp('::1.2.3.4:5')).toBeNull();
    expect(parseSelector('1.2.3.4::')).toEqual({
      kind: 'name',
      name: '1.2.3.4::',
    });
  });
});

describe('parseSelector', () => {
  it('reads a bare address as an Ip selector', () => {
    expect(selector('192.168.1.50')).toEqual({
      kind: 'ip',
      ip: { family: 'v4', bytes: [192, 168, 1, 50] },
    });
  });

  it('reads a bare word as a Name selector', () => {
    expect(selector('tv')).toEqual({ kind: 'name', name: 'tv' });
  });

  // A `/` commits the value to being a prefix. This is the rule that stops a
  // typo becoming a name that silently never matches.
  it('does not turn a malformed address before a `/` into a name', () => {
    expect(parseSelector('192.168.1.999/24')).toBeNull();
    expect(parseSelector('not-an-address/24')).toBeNull();
    expect(parseSelector('192.168.1.0/')).toBeNull();
  });

  it('caps the prefix length per family', () => {
    expect(parseSelector('192.168.1.0/32')).not.toBeNull();
    expect(parseSelector('192.168.1.0/33')).toBeNull();
    expect(parseSelector('2001:db8::/128')).not.toBeNull();
    expect(parseSelector('2001:db8::/129')).toBeNull();
    expect(parseSelector('192.168.1.0/0')).not.toBeNull();
  });

  // F10 — Rust parses the length with `u8::from_str`, which takes a leading
  // `+`; the mirror must not reject a selector the engine compiles.
  it('takes a `+`-signed prefix length the way `u8::from_str` does', () => {
    expect(parseSelector('192.168.1.0/+24')).toEqual({
      kind: 'network',
      ip: { family: 'v4', bytes: [192, 168, 1, 0] },
      prefixLen: 24,
    });
    expect(parseSelector('192.168.1.0/+')).toBeNull();
  });

  it('rejects an empty spec', () => {
    expect(parseSelector('')).toBeNull();
    expect(parseSelector('   ')).toBeNull();
  });

  it('trims before deciding', () => {
    expect(selector('  192.168.1.50  ')).toEqual(selector('192.168.1.50'));
  });
});

describe('matching a client', () => {
  it('matches an address exactly', () => {
    expect(matchesClient(selector('192.168.1.50'), '192.168.1.50')).toBe(true);
    expect(matchesClient(selector('192.168.1.50'), '192.168.1.51')).toBe(false);
  });

  it('contains an address in a prefix, whole bytes then the partial one', () => {
    expect(matchesClient(selector('192.168.20.0/24'), '192.168.20.11')).toBe(true);
    expect(matchesClient(selector('192.168.20.0/24'), '192.168.21.11')).toBe(false);
    expect(matchesClient(selector('192.168.0.0/20'), '192.168.15.9')).toBe(true);
    expect(matchesClient(selector('192.168.0.0/20'), '192.168.16.9')).toBe(false);
  });

  it('matches everything at /0 and one address at /32', () => {
    expect(matchesClient(selector('0.0.0.0/0'), '10.9.8.7')).toBe(true);
    expect(matchesClient(selector('192.168.1.50/32'), '192.168.1.50')).toBe(true);
    expect(matchesClient(selector('192.168.1.50/32'), '192.168.1.51')).toBe(false);
    expect(matchesClient(selector('2001:db8::/128'), '2001:db8::')).toBe(true);
    expect(matchesClient(selector('2001:db8::/128'), '2001:db8::1')).toBe(false);
  });

  // The engine's own rule, and the reason `/0` cannot be used as a catch-all
  // across families.
  it('never matches across families', () => {
    expect(matchesClient(selector('::/0'), '192.168.1.50')).toBe(false);
    expect(matchesClient(selector('0.0.0.0/0'), '2001:db8::1')).toBe(false);
    expect(matchesClient(selector('192.168.1.50'), '::ffff:192.168.1.50')).toBe(
      false,
    );
  });

  it('folds a name the ASCII way', () => {
    expect(matchesClient(selector('tv'), '192.168.1.50', 'TV')).toBe(true);
    expect(matchesClient(selector('TV'), '192.168.1.50', 'tv')).toBe(true);
    expect(matchesClient(selector('tv'), '192.168.1.50', 'tvv')).toBe(false);
  });

  it('never matches a Name selector for an unnamed client', () => {
    expect(matchesClient(selector('tv'), '192.168.1.50')).toBe(false);
    expect(matchesClient(selector('tv'), '192.168.1.50', null)).toBe(false);
  });

  it('does not match when the client address itself does not parse', () => {
    expect(matchesClient(selector('192.168.20.0/24'), 'not-an-address')).toBe(
      false,
    );
  });
});

describe('specificity', () => {
  it('is name, then address, then prefix length', () => {
    expect(specificity(selector('tv'))).toBe(1000);
    expect(specificity(selector('192.168.1.50'))).toBe(900);
    expect(specificity(selector('192.168.20.0/24'))).toBe(24);
    expect(specificity(selector('0.0.0.0/0'))).toBe(0);
  });
});

describe('the ASCII fold itself', () => {
  it('folds only A–Z', () => {
    expect(asciiLower('TV-Ätest')).toBe('tv-Ätest');
    expect(equalsAsciiCaseInsensitive('İ', 'i')).toBe(false);
    expect(equalsAsciiCaseInsensitive('ABC', 'abc')).toBe(true);
  });
});

describe('networkContains directly', () => {
  it('refuses a prefix longer than the address', () => {
    const v4 = parseIp('192.168.1.0');
    expect(v4).not.toBeNull();
    expect(networkContains(v4!, 33, v4!)).toBe(false);
  });
});
