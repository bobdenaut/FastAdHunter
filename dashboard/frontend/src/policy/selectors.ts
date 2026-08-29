/**
 * The client-selector matcher, mirroring the engine's own — `parse_selector` in
 * `crates/fah-rules/src/policy.rs` and `ClientSelector::matches` /
 * `specificity` in `crates/fah-model/src/policy.rs`.
 *
 * This is the one module in the dashboard that reimplements engine logic, and
 * it does so because the alternative is one `GET /clients/{ip}/policy` per row.
 * It is pure, it holds no clock, and every rule below is a test case. What it
 * decides is only ever a *label* — which selector a client inherited through.
 * The policy in force is always the API's own `policy` field, never this.
 */

export type Family = 'v4' | 'v6';

export interface IpAddress {
  family: Family;
  /** 4 bytes for v4, 16 for v6. */
  bytes: readonly number[];
}

export type Selector =
  | { kind: 'ip'; ip: IpAddress }
  | { kind: 'network'; ip: IpAddress; prefixLen: number }
  | { kind: 'name'; name: string };

/**
 * Rust's `Ipv4Addr::from_str`: exactly four dotted decimal octets, each 0–255,
 * and **no leading zeros** — `192.168.010.5` is not an address. Accepting it
 * would make the frontend match an assignment the engine never parses.
 */
function parseV4(spec: string): IpAddress | null {
  const parts = spec.split('.');
  if (parts.length !== 4) return null;
  const bytes: number[] = [];
  for (const part of parts) {
    if (part.length === 0 || part.length > 3) return null;
    if (!/^[0-9]+$/.test(part)) return null;
    if (part.length > 1 && part.startsWith('0')) return null;
    const value = Number(part);
    if (value > 255) return null;
    bytes.push(value);
  }
  return { family: 'v4', bytes };
}

/**
 * Rust's `Ipv6Addr::from_str`: hex groups of one to four digits, at most one
 * `::`, an optional trailing IPv4 form for the last 32 bits, and **no zone
 * id** — `fe80::1%eth0` does not parse.
 */
function parseV6(spec: string): IpAddress | null {
  if (spec.includes('%')) return null;
  const halves = spec.split('::');
  if (halves.length > 2) return null;

  const groups = (text: string, finalHalf: boolean): number[] | null => {
    if (text === '') return [];
    const out: number[] = [];
    const parts = text.split(':');
    for (let index = 0; index < parts.length; index += 1) {
      const part = parts[index] ?? '';
      if (part.includes('.')) {
        // The embedded IPv4 form is only legal as the final 32 bits **of the
        // whole address** — Rust rejects `1.2.3.4::`, where it sits in the
        // half a `::` elision follows, so the head takes it only when there
        // is no `::` at all.
        if (!finalHalf || index !== parts.length - 1) return null;
        const v4 = parseV4(part);
        if (v4 === null) return null;
        out.push(
          ((v4.bytes[0] ?? 0) << 8) | (v4.bytes[1] ?? 0),
          ((v4.bytes[2] ?? 0) << 8) | (v4.bytes[3] ?? 0),
        );
        continue;
      }
      if (part.length === 0 || part.length > 4) return null;
      if (!/^[0-9a-fA-F]+$/.test(part)) return null;
      out.push(Number.parseInt(part, 16));
    }
    return out;
  };

  const head = groups(halves[0] ?? '', halves.length === 1);
  if (head === null) return null;
  if (halves.length === 1) {
    if (head.length !== 8) return null;
    return { family: 'v6', bytes: toBytes(head) };
  }
  const tail = groups(halves[1] ?? '', true);
  if (tail === null) return null;
  // `::` must stand for at least one elided group.
  if (head.length + tail.length >= 8) return null;
  const middle = new Array<number>(8 - head.length - tail.length).fill(0);
  return { family: 'v6', bytes: toBytes([...head, ...middle, ...tail]) };
}

function toBytes(groups: readonly number[]): number[] {
  const bytes: number[] = [];
  for (const group of groups) {
    bytes.push((group >> 8) & 0xff, group & 0xff);
  }
  return bytes;
}

export function parseIp(spec: string): IpAddress | null {
  return spec.includes(':') ? parseV6(spec) : parseV4(spec);
}

/**
 * `192.168.1.50`, `192.168.1.0/24`, or a client name. A `/` **commits** the
 * value to being a prefix, so a malformed address cannot quietly become a name
 * that never matches anything.
 */
export function parseSelector(spec: string): Selector | null {
  const trimmed = spec.trim();
  if (trimmed === '') return null;

  const slash = trimmed.indexOf('/');
  if (slash !== -1) {
    const ip = parseIp(trimmed.slice(0, slash));
    if (ip === null) return null;
    const suffix = trimmed.slice(slash + 1);
    // Rust parses the length with `u8::from_str`, which takes a leading `+`.
    if (!/^\+?[0-9]+$/.test(suffix)) return null;
    const prefixLen = Number(suffix);
    // Rust parses the prefix as a `u8`, so anything above 255 fails to parse
    // before the family cap is even reached.
    if (prefixLen > 255) return null;
    if (prefixLen > (ip.family === 'v4' ? 32 : 128)) return null;
    return { kind: 'network', ip, prefixLen };
  }

  const ip = parseIp(trimmed);
  if (ip !== null) return { kind: 'ip', ip };
  return { kind: 'name', name: trimmed };
}

function equalAddresses(left: IpAddress, right: IpAddress): boolean {
  if (left.family !== right.family) return false;
  return left.bytes.every((byte, index) => byte === right.bytes[index]);
}

/** Whole bytes, then the masked partial byte. Mixed families never match — a
 *  v4 client is not in a v6 prefix, however the bits line up. */
export function networkContains(
  network: IpAddress,
  prefixLen: number,
  candidate: IpAddress,
): boolean {
  if (network.family !== candidate.family) return false;
  if (prefixLen > network.bytes.length * 8) return false;
  const whole = Math.floor(prefixLen / 8);
  for (let index = 0; index < whole; index += 1) {
    if (network.bytes[index] !== candidate.bytes[index]) return false;
  }
  const bits = prefixLen % 8;
  if (bits === 0) return true;
  const mask = (0xff << (8 - bits)) & 0xff;
  return (
    ((network.bytes[whole] ?? 0) & mask) === ((candidate.bytes[whole] ?? 0) & mask)
  );
}

/** `eq_ignore_ascii_case`: an explicit ASCII fold, never `toLocaleLowerCase`,
 *  which would fold a Turkish dotted `İ` the engine leaves alone. */
export function equalsAsciiCaseInsensitive(left: string, right: string): boolean {
  if (left.length !== right.length) return false;
  return asciiLower(left) === asciiLower(right);
}

export function asciiLower(value: string): string {
  let out = '';
  for (const character of value) {
    const code = character.charCodeAt(0);
    out +=
      code >= 65 && code <= 90 ? String.fromCharCode(code + 32) : character;
  }
  return out;
}

/** Whether this selector names the given client. A client with no name never
 *  matches a `Name` selector. */
export function matchesClient(
  selector: Selector,
  ip: string,
  name?: string | null,
): boolean {
  if (selector.kind === 'name') {
    return (
      name !== undefined &&
      name !== null &&
      equalsAsciiCaseInsensitive(name, selector.name)
    );
  }
  const address = parseIp(ip);
  if (address === null) return false;
  if (selector.kind === 'ip') return equalAddresses(selector.ip, address);
  return networkContains(selector.ip, selector.prefixLen, address);
}

/**
 * How specific a selector is, for resolving a client matched by more than one
 * assignment: a name is the most deliberate statement an operator can make,
 * then a single address, then the longest prefix.
 */
export function specificity(selector: Selector): number {
  if (selector.kind === 'name') return 1000;
  if (selector.kind === 'ip') return 900;
  return selector.prefixLen;
}
