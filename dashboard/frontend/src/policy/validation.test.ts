import { describe, expect, it } from 'vitest';
import {
  bothOrNeither,
  invalidLineCount,
  parseDays,
  parseTimeOfDay,
  parseUserRulesError,
  validatePolicyId,
} from './validation';

describe('policy ids', () => {
  it('accepts the alphabet the API accepts', () => {
    expect(validatePolicyId('kids')).toBeNull();
    expect(validatePolicyId('guest-wifi')).toBeNull();
    expect(validatePolicyId('a.b_c-1')).toBeNull();
  });

  it('reserves `default`', () => {
    expect(validatePolicyId('default')).toContain('implicit policy');
  });

  it('rejects an empty id, uppercase, spaces and a leading dot', () => {
    expect(validatePolicyId('')).not.toBeNull();
    expect(validatePolicyId('Kids')).not.toBeNull();
    expect(validatePolicyId('two words')).not.toBeNull();
    expect(validatePolicyId('.hidden')).not.toBeNull();
  });
});

describe('parseDays, against the schema validator own vectors', () => {
  it('takes `daily` and `all`', () => {
    expect(parseDays('daily')).toBeNull();
    expect(parseDays('all')).toBeNull();
    expect(parseDays('DAILY')).toBeNull();
  });

  it('takes comma lists and inclusive, wrapping ranges', () => {
    expect(parseDays('sat,sun')).toBeNull();
    expect(parseDays('mon-fri')).toBeNull();
    expect(parseDays('fri-mon')).toBeNull();
  });

  it('takes the long forms, by prefix', () => {
    expect(parseDays('Monday, Wednesday')).toBeNull();
    expect(parseDays('monday-friday')).toBeNull();
  });

  it('rejects an unknown day and an empty entry', () => {
    expect(parseDays('funday')).toContain('unknown day');
    expect(parseDays('mon,,fri')).toBe('empty entry in days');
    expect(parseDays('')).toBe('empty entry in days');
  });

  // Rust splits on the FIRST `-` and then resolves by prefix, so this is
  // accepted upstream. Mirrored rather than "fixed": a form stricter than the
  // API rejects a value the API would take.
  it('mirrors the first-dash split quirk', () => {
    expect(parseDays('mon-fri-sat')).toBeNull();
  });
});

describe('parseTimeOfDay', () => {
  it('takes HH:MM and the 24:00 end bound', () => {
    expect(parseTimeOfDay('00:00')).toBeNull();
    expect(parseTimeOfDay('21:00')).toBeNull();
    expect(parseTimeOfDay('24:00')).toBeNull();
  });

  it('rejects a missing colon, a non-numeric part and an out-of-range value', () => {
    expect(parseTimeOfDay('2100')).toContain('must be HH:MM');
    expect(parseTimeOfDay('ab:00')).toContain('non-numeric hour');
    expect(parseTimeOfDay('21:cd')).toContain('non-numeric minute');
    expect(parseTimeOfDay('24:01')).toContain('not a time of day');
    expect(parseTimeOfDay('25:00')).toContain('not a time of day');
    expect(parseTimeOfDay('21:60')).toContain('not a time of day');
  });
});

describe('bothOrNeither', () => {
  it('accepts both set and neither set', () => {
    expect(bothOrNeither('21:00', '07:00')).toBeNull();
    expect(bothOrNeither('', '')).toBeNull();
    expect(bothOrNeither('  ', '')).toBeNull();
  });

  it('rejects a half-open window', () => {
    expect(bothOrNeither('21:00', '')).toContain('both start and end');
    expect(bothOrNeither('', '07:00')).toContain('both start and end');
  });
});

describe('the 422 message parser', () => {
  it('anchors every entry to its line', () => {
    const parsed = parseUserRulesError(
      'line 7: invalid rule syntax: "@@||^"; line 12: invalid rule syntax: "|||"',
    );
    expect(parsed.lines).toEqual([
      { line: 7, detail: 'invalid rule syntax: "@@||^"' },
      { line: 12, detail: 'invalid rule syntax: "|||"' },
    ]);
    expect(parsed.more).toBe(0);
    expect(invalidLineCount(parsed)).toBe(2);
  });

  // The reason it scans for the anchor instead of splitting on `; `: the
  // quoted rule text is arbitrary and can contain the separator.
  it('keeps a quoted rule that contains the joiner intact', () => {
    const parsed = parseUserRulesError(
      'line 3: invalid rule syntax: "||a.example^; b"; line 9: invalid rule syntax: "x"',
    );
    expect(parsed.lines).toHaveLength(2);
    expect(parsed.lines[0]).toEqual({
      line: 3,
      detail: 'invalid rule syntax: "||a.example^; b"',
    });
    expect(parsed.lines[1]?.line).toBe(9);
  });

  it('reads the stated truncation as a remainder, not as an anchor', () => {
    const parsed = parseUserRulesError(
      'line 7: invalid rule syntax: "@@||^"; and 3 more invalid line(s)',
    );
    expect(parsed.lines).toHaveLength(1);
    expect(parsed.lines[0]?.detail).toBe('invalid rule syntax: "@@||^"');
    expect(parsed.more).toBe(3);
    expect(invalidLineCount(parsed)).toBe(4);
  });

  it('caps at what the server sent — 100 anchors plus a remainder', () => {
    const entries: string[] = [];
    for (let line = 1; line <= 100; line += 1) {
      entries.push(`line ${String(line)}: invalid rule syntax: "x"`);
    }
    entries.push('and 42 more invalid line(s)');
    const parsed = parseUserRulesError(entries.join('; '));
    expect(parsed.lines).toHaveLength(100);
    expect(parsed.more).toBe(42);
    expect(invalidLineCount(parsed)).toBe(142);
  });

  // The property the whole design rests on: a format change degrades to an
  // unanchored banner, never to a callout on the wrong line.
  it('yields zero anchors and no fabricated number for an unparseable message', () => {
    const parsed = parseUserRulesError('the rules document was rejected');
    expect(parsed.lines).toEqual([]);
    expect(parsed.more).toBe(0);
    expect(parsed.raw).toBe('the rules document was rejected');
    expect(invalidLineCount(parsed)).toBe(0);
  });

  it('reads a message that is only the remainder', () => {
    const parsed = parseUserRulesError('and 5 more invalid line(s)');
    expect(parsed.lines).toEqual([]);
    expect(parsed.more).toBe(5);
  });

  it('counts distinct lines, not entries', () => {
    const parsed = parseUserRulesError(
      'line 7: invalid rule syntax: "a"; line 7: invalid rule syntax: "b"',
    );
    expect(parsed.lines).toHaveLength(2);
    expect(invalidLineCount(parsed)).toBe(1);
  });

  // F11 — the quoted rule text is arbitrary. An anchor-shaped string *inside*
  // the quotes is content, never a second anchor: the quoted span is walked as
  // a Rust debug string rather than scanned past.
  it('does not mint an anchor from an anchor-shaped rule text', () => {
    const parsed = parseUserRulesError(
      'line 2: invalid rule syntax: "line 5: invalid rule syntax: \\"x\\""',
    );
    expect(parsed.lines).toEqual([
      {
        line: 2,
        detail: 'invalid rule syntax: "line 5: invalid rule syntax: \\"x\\""',
      },
    ]);
    expect(parsed.more).toBe(0);
  });

  it('does not read a remainder out of a rule text that spells one', () => {
    const parsed = parseUserRulesError(
      'line 1: invalid rule syntax: "x; and 9 more invalid line(s)"',
    );
    expect(parsed.lines).toHaveLength(1);
    expect(parsed.more).toBe(0);
  });

  it('keeps escaped quotes inside the rule text', () => {
    const parsed = parseUserRulesError(
      'line 4: invalid rule syntax: "say \\"hi\\""',
    );
    expect(parsed.lines).toEqual([
      { line: 4, detail: 'invalid rule syntax: "say \\"hi\\""' },
    ]);
  });

  it('degrades an unterminated quote to zero anchors', () => {
    const parsed = parseUserRulesError('line 4: invalid rule syntax: "broken');
    expect(parsed.lines).toEqual([]);
    expect(parsed.more).toBe(0);
    expect(parsed.raw).toBe('line 4: invalid rule syntax: "broken');
  });
});
