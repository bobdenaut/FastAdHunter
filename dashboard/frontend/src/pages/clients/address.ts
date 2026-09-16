/**
 * Address-family classification and ordering for the Clients table.
 *
 * **Lexicographic order is wrong for addresses**, and visibly so: it puts
 * `192.168.10.9` after `192.168.10.10` and sorts a compressed `fd6c::1` against
 * an expanded one as different strings. Both are ordered here on a padded
 * numeric key instead, so the table reads the way a subnet does.
 */

/**
 * A colon is the discriminator, not a library: `GET /clients` echoes what the
 * engine parsed as an `IpAddr`, so the string is always one of the two forms
 * and never a hostname. A zone suffix (`%eth0`) never reaches this page — the
 * registry keys on the address alone.
 */
export function isV6(ip: string): boolean {
  return ip.includes(':');
}

/**
 * A fixed-width key both families sort on. IPv4 becomes four 3-digit groups,
 * IPv6 eight 4-digit ones, and the `v4`/`v6` prefix keeps the two families
 * contiguous rather than interleaved by coincidence of digits.
 *
 * An address that parses as neither takes the `v0` prefix, so under the
 * table's ascending order it sorts **first**, under its own literal text,
 * rather than throwing — a row that arrived is a row that gets drawn.
 */
export function addressKey(ip: string): string {
  if (isV6(ip)) {
    const [head = '', tail = ''] = ip.split('::', 2);
    const left = head === '' ? [] : head.split(':');
    const right = tail === '' ? [] : tail.split(':');
    const gap = 8 - left.length - right.length;
    if (ip.includes('::') ? gap < 0 : left.length !== 8) return `v0:${ip}`;
    const groups = ip.includes('::')
      ? [...left, ...Array(gap).fill('0'), ...right]
      : left;
    return `v6:${groups.map((group) => group.padStart(4, '0')).join(':')}`;
  }
  const octets = ip.split('.');
  if (octets.length !== 4) return `v0:${ip}`;
  return `v4:${octets.map((octet) => octet.padStart(3, '0')).join('.')}`;
}

/**
 * The table's own order: two [`addressKey`] results ascending, compared as
 * plain code units rather than `localeCompare` — the keys are ASCII by
 * construction and a locale has no business in their order. It is also the
 * tie-break under every other column, ascending whichever way that column
 * points, so equal counts keep a stable place.
 *
 * **Keys, not addresses.** A comparator that built its own would build one per
 * comparison, 2·n·log₂n of them, and an IPv6 key is eight `padStart`s and a
 * `join`. The table builds n once and sorts those — measured at 4096 IPv6
 * rows, 48.4 ms became 2.9 ms
 * (`docs/code-review/phase5/adhoc-clients-column-sort-review.md`).
 */
export function compareKeys(left: string, right: string): number {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}
