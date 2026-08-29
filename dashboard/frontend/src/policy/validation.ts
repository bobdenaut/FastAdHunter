/**
 * Mirrors of the config validator and of the `422` message the rules endpoint
 * builds, so a form can reject before the API does.
 *
 * **Form validation is a courtesy, never the guarantee.** Every path that uses
 * one of these still renders the API's own `422` when the server disagrees —
 * these exist to answer while the operator is typing, not to replace the
 * server's answer.
 */

const RESERVED_POLICY_ID = 'default';

/** `routes.rs` `validate_policy_id`: lowercase letters, digits, `.`, `_` and
 *  `-`; not empty; not starting with `.`; and `default` is reserved. */
export function validatePolicyId(id: string): string | null {
  if (id === RESERVED_POLICY_ID) {
    return '"default" names the implicit policy every unassigned client already gets and cannot be redefined';
  }
  const alphabetOk = id.length > 0 && /^[a-z0-9._-]+$/.test(id);
  if (alphabetOk && !id.startsWith('.')) return null;
  return `invalid policy id "${id}": use lowercase letters, digits, '.', '_' or '-', and do not start with '.'`;
}

const DAY_NAMES = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'] as const;

/**
 * `crates/fah-config/src/schema/policy.rs` `parse_days`, quirks included: the
 * long forms are accepted by prefix (`monday`), and a range splits on the
 * **first** `-` only, so `mon-fri-sat` reads as `mon` to `fri-sat`, which
 * itself resolves to Friday by the same prefix rule.
 */
function parseDayName(name: string): string | null {
  const folded = name.trim().toLowerCase();
  const known = DAY_NAMES.some(
    (short) => folded.startsWith(short) && folded.length <= 9,
  );
  return known
    ? null
    : `unknown day "${folded}" (use sun..sat, or "daily")`;
}

/** `daily`/`all`, comma lists, inclusive ranges, wrapping (`fri-mon`) and the
 *  long day forms. Returns the message the config validator would give. */
export function parseDays(spec: string): string | null {
  const trimmed = spec.trim();
  if (trimmed.toLowerCase() === 'daily' || trimmed.toLowerCase() === 'all') {
    return null;
  }
  for (const raw of trimmed.split(',')) {
    const part = raw.trim();
    if (part === '') return 'empty entry in days';
    const dash = part.indexOf('-');
    if (dash === -1) {
      const failure = parseDayName(part);
      if (failure !== null) return failure;
      continue;
    }
    const from = parseDayName(part.slice(0, dash));
    if (from !== null) return from;
    const to = parseDayName(part.slice(dash + 1));
    if (to !== null) return to;
  }
  return null;
}

/** `HH:MM`, with `24:00` accepted as an end bound — `00:00` would wrap the
 *  window to the previous day, so midnight is otherwise unwritable. */
export function parseTimeOfDay(spec: string): string | null {
  const trimmed = spec.trim();
  const colon = trimmed.indexOf(':');
  if (colon === -1) return `time "${trimmed}" must be HH:MM`;
  const hoursText = trimmed.slice(0, colon).trim();
  const minutesText = trimmed.slice(colon + 1).trim();
  if (!/^\+?[0-9]+$/.test(hoursText)) {
    return `time "${trimmed}" has a non-numeric hour`;
  }
  if (!/^\+?[0-9]+$/.test(minutesText)) {
    return `time "${trimmed}" has a non-numeric minute`;
  }
  const hours = Number(hoursText);
  const minutes = Number(minutesText);
  if (hours > 65535) return `time "${trimmed}" has a non-numeric hour`;
  if (minutes > 65535) return `time "${trimmed}" has a non-numeric minute`;
  if (minutes > 59 || hours > 24 || (hours === 24 && minutes > 0)) {
    return `time "${trimmed}" is not a time of day (00:00-24:00)`;
  }
  return null;
}

/** `crates/fah-config/src/lib.rs`: a half-open window would silently never
 *  close, so the two bounds are set together or not at all. */
export function bothOrNeither(start: string, end: string): string | null {
  const hasStart = start.trim() !== '';
  const hasEnd = end.trim() !== '';
  if (hasStart === hasEnd) return null;
  return 'a schedule needs both start and end (a half-open window would silently never close)';
}

/* ------------------------------------------------ the `422` message parser */

export interface RuleLineError {
  line: number;
  /** Everything after `line N: ` — the API's own words for that line. */
  detail: string;
}

export interface UserRulesError {
  /** In the order the API reported them. Bounded: the server caps the reported
   *  lines at 100 and states the remainder separately. */
  lines: RuleLineError[];
  /** The `and N more invalid line(s)` remainder; `0` when absent. */
  more: number;
  /** The envelope's message verbatim, for the banner. */
  raw: string;
}

const ENTRY = /^line (\d+): invalid rule syntax: "/;
const TAIL = /^and (\d+) more invalid line\(s\)$/;

/**
 * `validate_user_rules` joins `line N: invalid rule syntax: {content:?}` with
 * `; `, so the parser consumes that grammar **strictly**: each entry's quoted
 * rule is walked as a Rust debug string (a `\` escapes the next character), so
 * a `; ` — or a whole `line N: …` that happens to sit *inside* the quotes —
 * is content, never a new entry.
 *
 * **Total, and it never fabricates a line number.** A message that does not
 * parse as the grammar end to end yields zero anchors and the raw text; the
 * failure mode is a visible degradation to an unanchored banner, never a
 * callout pointing at the wrong line.
 */
export function parseUserRulesError(message: string): UserRulesError {
  return parseStrict(message) ?? { lines: [], more: 0, raw: message };
}

function parseStrict(message: string): UserRulesError | null {
  const lines: RuleLineError[] = [];
  let more = 0;
  let at = 0;

  while (at < message.length) {
    const rest = message.slice(at);

    const tail = TAIL.exec(rest);
    if (tail !== null) {
      const count = Number(tail[1]);
      if (!Number.isSafeInteger(count) || count <= 0) return null;
      more = count;
      at = message.length;
      break;
    }

    const entry = ENTRY.exec(rest);
    if (entry === null) return null;
    const line = Number(entry[1]);
    if (!Number.isSafeInteger(line) || line <= 0) return null;

    let closed = -1;
    for (let cursor = at + entry[0].length; cursor < message.length; cursor += 1) {
      const character = message[cursor];
      if (character === '\\') {
        cursor += 1;
        continue;
      }
      if (character === '"') {
        closed = cursor;
        break;
      }
    }
    if (closed === -1) return null;

    lines.push({
      line,
      detail: message.slice(at + `line ${entry[1]}: `.length, closed + 1),
    });

    at = closed + 1;
    if (at === message.length) break;
    if (!message.startsWith('; ', at)) return null;
    at += 2;
    if (at === message.length) return null;
  }

  return lines.length === 0 && more === 0 ? null : { lines, more, raw: message };
}

/** T2 — the `1 invalid` figure: distinct anchored lines plus the stated
 *  remainder, which is the only honest count when the server truncated. */
export function invalidLineCount(parsed: UserRulesError): number {
  return new Set(parsed.lines.map((entry) => entry.line)).size + parsed.more;
}
