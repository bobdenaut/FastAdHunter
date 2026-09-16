import { describe, expect, it } from 'vitest';
import { addressKey, compareAddressesAsc, isV6 } from './address';

/**
 * The two properties the Clients table's order depends on: an address sorts
 * numerically rather than lexicographically, and the two families do not
 * interleave.
 */
describe('address ordering', () => {
  const asc = (ips: string[]) => [...ips].sort(compareAddressesAsc);

  it('tells the families apart', () => {
    expect(isV6('192.168.10.1')).toBe(false);
    expect(isV6('fd6c:7f32:8e91:1::2')).toBe(true);
  });

  it('orders IPv4 numerically, not as text', () => {
    // The failure this exists for: `.9` must not follow `.10`.
    expect(asc(['192.168.10.100', '192.168.10.10', '192.168.10.9'])).toEqual([
      '192.168.10.9',
      '192.168.10.10',
      '192.168.10.100',
    ]);
  });

  it('orders across octets, not just the last', () => {
    expect(asc(['192.168.20.11', '192.168.10.50', '10.0.0.1'])).toEqual([
      '10.0.0.1',
      '192.168.10.50',
      '192.168.20.11',
    ]);
  });

  it('expands `::` so a compressed address sorts as its full form', () => {
    expect(addressKey('fd6c::1')).toBe(addressKey('fd6c:0:0:0:0:0:0:1'));
    expect(addressKey('::1')).toBe(addressKey('0:0:0:0:0:0:0:1'));
  });

  it('orders IPv6 groups numerically', () => {
    expect(asc(['fd6c::100', 'fd6c::10', 'fd6c::9'])).toEqual([
      'fd6c::9',
      'fd6c::10',
      'fd6c::100',
    ]);
  });

  it('keeps the families contiguous', () => {
    const sorted = asc(['192.168.10.1', 'fd6c::2', '10.0.0.1', 'fd6c::1']);
    expect(sorted).toEqual(['10.0.0.1', '192.168.10.1', 'fd6c::1', 'fd6c::2']);
  });

  it('does not throw on an address it cannot parse', () => {
    expect(() => addressKey('not-an-address')).not.toThrow();
    expect(addressKey('192.168.1')).toBe('v0:192.168.1');
  });

  it('sorts a malformed address first under the ascending order', () => {
    expect(asc(['10.0.0.1', 'fd6c::1', '192.168.1'])).toEqual([
      '192.168.1',
      '10.0.0.1',
      'fd6c::1',
    ]);
  });
});
