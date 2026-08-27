import { describe, expect, it } from 'vitest';
import type { Assignment, Client, Policy } from '../api/types';
import {
  CHOSEN_POLICY_REASON,
  classifyAssignment,
  hasSchedule,
  scheduleText,
  whyPolicyApplies,
} from './assignment';

function client(overrides: Partial<Client> & Pick<Client, 'ip'>): Client {
  return {
    name: null,
    first_seen: '2026-08-27T09:00:00Z',
    last_seen: '2026-08-27T10:00:00Z',
    queries_24h: 10,
    blocked_24h: 1,
    policy: 'default',
    ...overrides,
  };
}

function policy(id: string, assignments: Assignment[]): Policy {
  return {
    id,
    name: id,
    lists: null,
    blocking_mode: null,
    assignments,
  };
}

describe('schedule text', () => {
  it('covers all four days/window combinations', () => {
    expect(scheduleText({ client: 'x' })).toBe('no schedule — always');
    expect(
      scheduleText({ client: 'x', start: '21:00', end: '07:00' }),
    ).toBe('daily · 21:00 → 07:00');
    expect(scheduleText({ client: 'x', days: 'mon-fri' })).toBe(
      'mon–fri · all day',
    );
    expect(
      scheduleText({
        client: 'x',
        days: 'mon-fri',
        start: '21:00',
        end: '07:00',
      }),
    ).toBe('mon–fri · 21:00 → 07:00');
  });

  it('renders every `-` in a day list as an en dash, display only', () => {
    expect(scheduleText({ client: 'x', days: 'fri-mon' })).toContain('fri–mon');
    expect(scheduleText({ client: 'x', days: 'sat,sun' })).toContain('sat,sun');
  });

  it('reports a schedule when any one of the three fields is set', () => {
    expect(hasSchedule({ client: 'x' })).toBe(false);
    expect(hasSchedule({ client: 'x', days: 'daily' })).toBe(true);
    expect(hasSchedule({ client: 'x', start: '21:00' })).toBe(true);
    expect(hasSchedule({ client: 'x', end: '07:00' })).toBe(true);
  });
});

describe('branch 1 — a direct assignment that is deciding', () => {
  it('is solid, names its schedule and says it is in force', () => {
    const result = classifyAssignment(
      client({
        ip: '192.168.10.50',
        name: 'tv',
        policy: 'kids',
        assignment_source: 'direct',
      }),
      [
        policy('kids', [
          { client: '192.168.10.50', days: 'mon-fri', start: '21:00', end: '07:00' },
        ]),
      ],
    );
    expect(result.branch).toBe('1');
    expect(result.style).toBe('solid');
    expect(result.policy).toBe('kids');
    expect(result.note).toBe('mon–fri · 21:00 → 07:00 · in force');
    expect(whyPolicyApplies(result)).toBe(
      'assignment on this address · mon–fri · 21:00 → 07:00 · in force now',
    );
  });
});

describe('R1 — 2a fires before 2b', () => {
  // The whole point of the ordering. The direct window is wide open; a
  // schedule-less name assignment on the in-force policy outranks it. Testing
  // the schedule first would print "window shut now" over an open window.
  it('attributes an open, overridden window to the name assignment', () => {
    const result = classifyAssignment(
      client({
        ip: '192.168.10.50',
        name: 'tv',
        policy: 'guest',
        assignment_source: 'direct',
      }),
      [
        policy('kids', [
          { client: '192.168.10.50', days: 'mon-fri', start: '21:00', end: '07:00' },
        ]),
        policy('guest', [{ client: 'tv' }]),
      ],
    );
    expect(result.branch).toBe('2a');
    expect(result.policy).toBe('kids');
    expect(result.note).toBe(
      'not in force — the name assignment on "tv" decides (guest)',
    );
    expect(result.note).not.toContain('window shut');
    expect(whyPolicyApplies(result)).toBe(
      'the assignment on this address is not deciding — the name assignment on "tv" decides (guest)',
    );
  });

  it("2a' refuses to attribute when every name member is scheduled", () => {
    const result = classifyAssignment(
      client({
        ip: '192.168.10.50',
        name: 'tv',
        policy: 'guest',
        assignment_source: 'direct',
      }),
      [
        policy('kids', [
          { client: '192.168.10.50', days: 'mon-fri', start: '21:00', end: '07:00' },
        ]),
        policy('guest', [{ client: 'tv', days: 'sat-sun' }]),
      ],
    );
    expect(result.branch).toBe('2a-prime');
    expect(result.policy).toBe('kids');
    expect(result.note).toBe('not in force — guest in force');
    expect(result.note).not.toContain('name assignment');
    expect(result.note).not.toContain('window shut');
  });
});

describe('branch 2b — the shut window', () => {
  it('names the shut window and the policy actually in force (X4)', () => {
    const result = classifyAssignment(
      client({
        ip: '192.168.10.22',
        policy: 'default',
        assignment_source: 'direct',
      }),
      [policy('kids', [{ client: '192.168.10.22', days: 'sat-sun' }])],
    );
    expect(result.branch).toBe('2b');
    expect(result.style).toBe('solid');
    expect(result.policy).toBe('kids');
    expect(result.tone).toBe('warn');
    expect(result.note).toBe(
      'sat–sun · all day · window shut now — default in force',
    );
    expect(result.via).toBeNull();
  });

  it('appends the single inherited selector when the fallback is not default', () => {
    const result = classifyAssignment(
      client({
        ip: '192.168.20.11',
        policy: 'guest',
        assignment_source: 'direct',
      }),
      [
        policy('kids', [
          { client: '192.168.20.11', start: '21:00', end: '07:00' },
        ]),
        policy('guest', [{ client: '192.168.20.0/24' }]),
      ],
    );
    expect(result.branch).toBe('2b');
    expect(result.via).toBe('via 192.168.20.0/24');
    expect(whyPolicyApplies(result)).toBe(
      'assignment on this address · daily · 21:00 → 07:00 · window shut — guest in force · via 192.168.20.0/24',
    );
    // The chip note stays the artboard's sentence; only the "why" appends.
    expect(result.note).not.toContain('via ');
  });
});

describe('branch 2c — no claim', () => {
  it('says only what the in-force policy already proves', () => {
    const result = classifyAssignment(
      client({
        ip: '192.168.10.50',
        policy: 'guest',
        assignment_source: 'direct',
      }),
      [policy('kids', [{ client: '192.168.10.50' }]), policy('guest', [])],
    );
    expect(result.branch).toBe('2c');
    expect(result.policy).toBe('kids');
    expect(result.note).toBe('not in force — guest in force');
    expect(whyPolicyApplies(result)).toBe('not in force — guest in force');
  });
});

describe('branch 0 — the two responses disagree', () => {
  it('claims nothing when `direct` is declared and no assignment matches', () => {
    const result = classifyAssignment(
      client({
        ip: '192.168.10.50',
        policy: 'kids',
        assignment_source: 'direct',
      }),
      [policy('kids', [{ client: '192.168.20.0/24' }])],
    );
    expect(result.branch).toBe('0');
    expect(result.style).toBe('solid');
    expect(result.policy).toBe('kids');
    expect(result.note).toBe('');
    expect(whyPolicyApplies(result)).toBe('not in force — kids in force');
  });

  // The reverse shape: a string-equal assignment exists but the API did not
  // call it direct. It is ignored and the row falls to branches 3/4.
  it('ignores a string-equal assignment when the source is absent', () => {
    const result = classifyAssignment(
      client({ ip: '192.168.10.50', policy: 'default' }),
      [policy('kids', [{ client: '192.168.10.50' }])],
    );
    expect(result.branch).toBe('3');
    expect(result.style).toBe('dashed');
    expect(result.note).toBe('inherited · no assignment');
  });
});

describe('the direct lookup is string equality, exactly as Rust compares', () => {
  it('does not match a non-canonical spelling of the same address', () => {
    const result = classifyAssignment(
      client({
        ip: '192.168.10.5',
        policy: 'kids',
        assignment_source: 'direct',
      }),
      [policy('kids', [{ client: '192.168.010.5' }])],
    );
    expect(result.branch).toBe('0');
  });

  it('takes the first match in item order then array order', () => {
    const result = classifyAssignment(
      client({
        ip: '192.168.10.5',
        policy: 'guest',
        assignment_source: 'direct',
      }),
      [
        policy('kids', [{ client: '192.168.10.5' }]),
        policy('guest', [{ client: '192.168.10.5' }]),
      ],
    );
    // The first is `kids`, which is not the in-force policy, so this is a
    // branch-2 shape rather than branch 1.
    expect(result.policy).toBe('kids');
    expect(result.branch).toBe('2c');
  });
});

describe('branches 3 and 4 — inherited', () => {
  it('3 — nothing is assigned', () => {
    const result = classifyAssignment(
      client({ ip: '192.168.10.15', name: 'liviu-phone' }),
      [],
    );
    expect(result.branch).toBe('3');
    expect(result.style).toBe('dashed');
    expect(result.policy).toBe('default');
    expect(whyPolicyApplies(result)).toBe('no assignment covers this address');
  });

  it('4 — one subnet candidate names its selector', () => {
    const result = classifyAssignment(
      client({ ip: '192.168.20.11', policy: 'guest' }),
      [policy('guest', [{ client: '192.168.20.0/24' }])],
    );
    expect(result.branch).toBe('4-via');
    expect(result.style).toBe('dashed');
    expect(result.note).toBe('via 192.168.20.0/24');
  });

  it('4 — one name candidate uses the name spelling', () => {
    const result = classifyAssignment(
      client({ ip: '192.168.10.50', name: 'TV', policy: 'kids' }),
      [policy('kids', [{ client: 'tv' }])],
    );
    expect(result.branch).toBe('4-via');
    expect(result.note).toBe('via name tv');
  });

  // D2: the row names a selector only when exactly one matches. Two matching
  // assignments have no correct single answer, so it claims none.
  it('4 — two candidates claim nothing', () => {
    const result = classifyAssignment(
      client({ ip: '192.168.20.11', name: 'tv', policy: 'guest' }),
      [policy('guest', [{ client: '192.168.20.0/24' }, { client: 'tv' }])],
    );
    expect(result.branch).toBe('4-inherited');
    expect(result.note).toBe('inherited');
    expect(whyPolicyApplies(result)).toBe('inherited');
  });

  it('4 — no candidate at all still claims nothing', () => {
    const result = classifyAssignment(
      client({ ip: '192.168.20.11', policy: 'guest' }),
      [policy('guest', [{ client: '10.0.0.0/8' }])],
    );
    expect(result.branch).toBe('4-inherited');
    expect(result.note).toBe('inherited');
  });
});

describe('the policy-mode reason', () => {
  it('is a constant, because the request itself is the reason', () => {
    expect(CHOSEN_POLICY_REASON).toBe(
      'you chose this policy — assignments are ignored',
    );
  });
});
