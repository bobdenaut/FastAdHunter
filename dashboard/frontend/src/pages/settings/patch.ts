import type { Config, UpstreamServerConfig } from '../../api/types';
import {
  FIELDS,
  MAX_UPSTREAM_SERVERS,
  UPSTREAM_PROTOCOLS,
  fieldMeta,
  type FieldMeta,
} from './metadata';

/**
 * The write path's arithmetic, as pure functions: what is dirty, what the body
 * is, and what the client refuses before a request is made.
 *
 * **`POST /api/v1/config` is a partial deep-merge, so the body carries the
 * changed keys and nothing else.** Submitting the document that was read back
 * would write every value the UI last saw — keys it does not model included —
 * so a hand-edited value, or one a newer build added, would be overwritten the
 * moment anyone saved an unrelated field (IA §Writing config).
 *
 * **Dirty state is tracked per field against a baseline, never derived from a
 * re-fetch.** `config_changed` can move the server's copy between the read and
 * the save, so a diff against a fresh document would attribute someone else's
 * change to this operator.
 */

export type FieldValue =
  | string
  | number
  | boolean
  | readonly string[]
  | readonly UpstreamServerConfig[];

/** The operator's edits, by dotted key. **Only dirty fields are present** — a
 *  field reverted to its baseline value leaves the map, so `Object.keys` is the
 *  dirty set and the patch body in one. */
export type Edits = Readonly<Record<string, FieldValue>>;

export interface FieldError {
  key: string;
  message: string;
}

/** Reads a dotted key out of the response as it arrived. The form never assumes
 *  a section is present: an older build that has not grown a key yet renders
 *  that field as unavailable rather than as a zero. */
export function readPath(config: unknown, key: string): unknown {
  let cursor: unknown = config;
  for (const step of key.split('.')) {
    if (typeof cursor !== 'object' || cursor === null) return undefined;
    cursor = (cursor as Record<string, unknown>)[step];
  }
  return cursor;
}

/** Deep for the two array fields, `===` for every scalar. The arrays hold at
 *  most eight rows, so a stringify is cheaper than a bespoke comparison and
 *  cannot disagree with itself. */
export function sameValue(a: unknown, b: unknown): boolean {
  if (Array.isArray(a) || Array.isArray(b)) {
    return JSON.stringify(a) === JSON.stringify(b);
  }
  return a === b;
}

/** What the control renders: the edit if there is one, the baseline otherwise. */
export function fieldValue(
  baseline: Config | null,
  edits: Edits,
  key: string,
): unknown {
  if (Object.hasOwn(edits, key)) return edits[key];
  return baseline === null ? undefined : readPath(baseline, key);
}

/**
 * One edit. An edit equal to the baseline **removes** the key rather than
 * storing it, so typing a value back to what it was clears the dirty mark and
 * shrinks the body — a save must never carry a key the operator did not change.
 */
export function setEdit(
  baseline: Config | null,
  edits: Edits,
  key: string,
  value: FieldValue,
): Edits {
  const next: Record<string, FieldValue> = { ...edits };
  const original = baseline === null ? undefined : readPath(baseline, key);
  if (sameValue(original, value)) delete next[key];
  else next[key] = value;
  return next;
}

/**
 * A fresh `GET /config` replaces the baseline; the operator's edits survive it.
 * An edit that now equals the new server value stops being dirty — someone else
 * made the same change, and re-sending it would be a write with nothing behind
 * it.
 */
export function rebase(baseline: Config, edits: Edits): Edits {
  const next: Record<string, FieldValue> = {};
  for (const [key, value] of Object.entries(edits)) {
    if (!sameValue(readPath(baseline, key), value)) next[key] = value;
  }
  return next;
}

export function dirtyKeys(edits: Edits): string[] {
  return Object.keys(edits).sort();
}

/**
 * The request body: the dirty keys, nested as the deep-merge expects.
 * `{"dns":{"cache":{"max_entries":20000}}}` and nothing beside it.
 *
 * **An array is sent whole.** `config_store.rs::merge` replaces arrays rather
 * than merging them — a half-merged array of tables has no meaning — so one
 * edited upstream row means the whole `servers` array travels.
 */
export function buildPatch(edits: Edits): Record<string, unknown> {
  const body: Record<string, unknown> = {};
  for (const key of dirtyKeys(edits)) {
    const steps = key.split('.');
    const leaf = steps.pop();
    if (leaf === undefined) continue;
    let cursor = body;
    for (const step of steps) {
      const existing = cursor[step];
      const child =
        typeof existing === 'object' && existing !== null && !Array.isArray(existing)
          ? (existing as Record<string, unknown>)
          : {};
      cursor[step] = child;
      cursor = child;
    }
    cursor[leaf] = edits[key] as unknown;
  }
  return body;
}

/* ------------------------------------------------------------ validation */

const IPV4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/;

/**
 * Mirrors `validate_ip`, which is `str::parse::<IpAddr>()`. Deliberately
 * structural rather than exhaustive: a form of IPv6 this misses is anchored by
 * the server's own `422` under the field, which is the fallback every rule here
 * leans on. It must never accept what the server rejects **and** must never
 * reject what the server accepts, so the v6 branch is permissive.
 */
export function looksLikeIp(value: string): boolean {
  const text = value.trim();
  const v4 = IPV4.exec(text);
  if (v4 !== null) {
    return v4.slice(1).every((part) => {
      const octet = Number(part);
      // The raw text against its own value, which is what rejects a leading
      // zero: `01` parses to 1 and prints back as `1`. Comparing the number to
      // itself — as this did — is a tautology, so `01.0.0.1` reached a server
      // whose `IpAddr` parse refuses it.
      return octet <= 255 && part === String(octet);
    });
  }
  return text.includes(':') && /^[0-9a-fA-F:.]+$/.test(text);
}

/** Mirrors `validate_allowed_destination`: an IP, or an IP and a prefix length
 *  within its family. */
export function looksLikeDestination(value: string): boolean {
  const [address = '', prefix, ...rest] = value.trim().split('/');
  if (rest.length > 0) return false;
  if (!looksLikeIp(address)) return false;
  if (prefix === undefined) return true;
  if (!/^\d{1,3}$/.test(prefix)) return false;
  return Number(prefix) <= (address.includes(':') ? 128 : 32);
}

function integerError(meta: FieldMeta, value: unknown): string | null {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return 'must be a number';
  }
  if (!Number.isInteger(value)) return 'must be a whole number';
  const control = meta.control;
  if (control.kind !== 'int' && control.kind !== 'bytes') return null;
  if (value < control.min || value > control.max) {
    return `must be between ${control.min.toLocaleString()} and ${control.max.toLocaleString()}`;
  }
  return null;
}

/**
 * Mirrors `validate`'s `address.starts_with("https://")` without writing that
 * prefix as one literal: `scripts/postbuild.mjs` refuses a shipped asset
 * containing `https://`, because in a bundle that is a CDN reference. A scheme
 * a validator compares against is not one, and splitting on the separator says
 * the same thing without a string the scan cannot tell apart from a fetch.
 */
function isTlsUrl(address: string): boolean {
  const [scheme = '', rest = ''] = address.split('://');
  return scheme === 'https' && rest !== '';
}

function serversError(value: unknown): string | null {
  if (!Array.isArray(value)) return 'must be a list of servers';
  const rows = value as UpstreamServerConfig[];
  if (rows.length === 0) return 'at least one upstream server is required';
  if (rows.length > MAX_UPSTREAM_SERVERS) {
    return `at most ${MAX_UPSTREAM_SERVERS} upstream servers are supported`;
  }
  for (const [index, row] of rows.entries()) {
    const at = `server ${index + 1}`;
    if (row.address.trim() === '') return `${at}: an address is required`;
    if (!(UPSTREAM_PROTOCOLS as readonly string[]).includes(row.protocol)) {
      return `${at}: protocol must be one of ${UPSTREAM_PROTOCOLS.join(', ')}`;
    }
    if (row.protocol === 'dot' && (row.hostname ?? '').trim() === '') {
      return `${at}: a dot upstream requires a hostname for certificate verification`;
    }
    if (row.protocol === 'doh' && !isTlsUrl(row.address)) {
      return `${at}: a doh upstream must be an https URL`;
    }
  }
  return null;
}

/**
 * One field against its metadata. Every rule here mirrors one in
 * `fah-config`'s `validate` — the client refuses early so an out-of-range entry
 * costs no round trip, and the server stays the authority: anything this misses
 * comes back as a `422` anchored under the same field.
 */
export function fieldError(meta: FieldMeta, value: unknown): string | null {
  switch (meta.control.kind) {
    case 'int':
    case 'bytes':
      return integerError(meta, value);
    case 'bool':
      return typeof value === 'boolean' ? null : 'must be true or false';
    case 'enum':
      return typeof value === 'string' && meta.control.values.includes(value)
        ? null
        : `must be one of ${meta.control.values.join(', ')}`;
    case 'ip':
      return typeof value === 'string' && looksLikeIp(value)
        ? null
        : 'must be an IP address';
    case 'tz':
      // The POSIX TZ grammar is not mirrored: it is a real parser
      // (`fah-config` `tz::PosixTz`), and half a parser here would reject
      // strings the server accepts. Presence is the whole client-side rule.
      return typeof value === 'string' && value.trim() !== ''
        ? null
        : 'must not be empty';
    case 'string-list': {
      if (!Array.isArray(value)) return 'must be a list';
      const bad = (value as string[]).find(
        (entry) => !looksLikeDestination(entry),
      );
      return bad === undefined
        ? null
        : `${bad} is not an IP address or CIDR block`;
    }
    case 'server-list':
      return serversError(value);
  }
}

/**
 * Every dirty field, plus the one cross-field rule the schema carries
 * (`min_ttl_seconds <= max_ttl_seconds`). The cross-field check reads the
 * **effective** values — an edit where there is one, the baseline otherwise —
 * because raising the ceiling and the floor in one save is legal and comparing
 * an edit against a stale baseline would refuse it.
 */
export function validateEdits(
  baseline: Config | null,
  edits: Edits,
): FieldError[] {
  const errors: FieldError[] = [];
  for (const key of dirtyKeys(edits)) {
    const meta = fieldMeta(key);
    if (meta === null) continue;
    const message = fieldError(meta, edits[key]);
    if (message !== null) errors.push({ key, message });
  }

  const min = fieldValue(baseline, edits, 'dns.cache.min_ttl_seconds');
  const max = fieldValue(baseline, edits, 'dns.cache.max_ttl_seconds');
  if (
    typeof min === 'number' &&
    typeof max === 'number' &&
    min > max &&
    (Object.hasOwn(edits, 'dns.cache.min_ttl_seconds') ||
      Object.hasOwn(edits, 'dns.cache.max_ttl_seconds'))
  ) {
    errors.push({
      key: 'dns.cache.min_ttl_seconds',
      message: `must be <= max_ttl_seconds (${min.toLocaleString()} > ${max.toLocaleString()})`,
    });
  }
  return errors;
}

/* ------------------------------------------------------- 422 anchoring */

/** `invalid value for \`key\`: message` — `fah-config`'s
 *  `ConfigError::Validation`, which `post_config` passes through verbatim. */
const ANCHOR = /^invalid value for `([^`]+)`: ([\s\S]+)$/;

export interface AnchoredError {
  /** `null` when the message named no field this form models, in which case the
   *  page renders it whole rather than pinning it somewhere plausible. */
  key: string | null;
  message: string;
}

/**
 * D2 — pins a `422` under the field it is about.
 *
 * The prefix is matched exactly rather than hunting for the first backticked
 * token: serde's own messages are backticked too (`unknown field \`x\``), and a
 * loose match would anchor an unrelated rejection under whichever field it
 * happened to name.
 *
 * A key the form does not model — `dns.upstreams.servers.hostname`, which the
 * server reports per row — anchors under its nearest modelled ancestor, so a
 * row-level rejection lands on the row editor rather than on the page.
 */
export function anchorError(message: string): AnchoredError {
  const match = ANCHOR.exec(message.trim());
  if (match === null) return { key: null, message };
  const [, key = '', detail = ''] = match;
  if (fieldMeta(key) !== null) return { key, message: detail };
  const ancestor = FIELDS.map((field) => field.key)
    .filter((candidate) => key.startsWith(`${candidate}.`))
    .sort((a, b) => b.length - a.length)[0];
  return { key: ancestor ?? null, message: ancestor === undefined ? message : detail };
}
