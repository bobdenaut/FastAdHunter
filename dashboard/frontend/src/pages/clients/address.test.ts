import { describe, expect, it } from 'vitest';
import { addressKey, compareAddressesDesc, isV6 } from './address';

/**
 * The two properties the Clients table's order depends on: an address sorts
 * numerically rather than lexicographically, and the two families do not
 * interleave.
 */
describe('address ordering', () => {
  const desc = (ips: string[]) => [...ips].sort(compareAddressesDesc);

  it('tells the families apart', () => {
    expect(isV6('192.168.10.1')).toBe(false);
    expect(isV6('fd6c:7f32:8e91:1::2')).toBe(true);
  });

  it('orders IPv4 numerically, not as text', () => {
    // The failure this exists for: `.9` must not follow `.10`.
    expect(desc(['192.168.10.9', '192.168.10.10', '192.168.10.100'])).toEqual([
      '192.168.10.100',
      '192.168.10.10',
      '192.168.10.9',
    ]);
  });

  it('orders across octets, not just the last', () => {
    expect(desc(['192.168.20.11', '192.168.10.50', '10.0.0.1'])).toEqual([
      '192.168.20.11',
      '192.168.10.50',
      '10.0.0.1',
    ]);
  });

  it('expands `::` so a compressed address sorts as its full form', () => {
    expect(addressKey('fd6c::1')).toBe(addressKey('fd6c:0:0:0:0:0:0:1'));
    expect(addressKey('::1')).toBe(addressKey('0:0:0:0:0:0:0:1'));
  });

  it('orders IPv6 groups numerically', () => {
    expect(desc(['fd6c::9', 'fd6c::10', 'fd6c::100'])).toEqual([
      'fd6c::100',
      'fd6c::10',
      'fd6c::9',
    ]);
  });

  it('keeps the families contiguous', () => {
    const sorted = desc(['192.168.10.1', 'fd6c::2', '10.0.0.1', 'fd6c::1']);
    expect(sorted).toEqual(['fd6c::2', 'fd6c::1', '192.168.10.1', '10.0.0.1']);
  });

  /** A row that arrived is a row that gets drawn — malformed input sorts last
   *  rather than throwing. */
  it('does not throw on an address it cannot parse', () => {
    expect(() => addressKey('not-an-address')).not.toThrow();
    expect(addressKey('192.168.1')).toBe('v0:192.168.1');
  });

  it('sorts a malformed address last under the descending order', () => {
    expect(desc(['192.168.1', '10.0.0.1', 'fd6c::1'])).toEqual([
      'fd6c::1',
      '10.0.0.1',
      '192.168.1',
    ]);
  });
});
