import type { Assignment, Client, Policy } from '../api/types';
import { matchesClient, parseSelector } from './selectors';

/**
 * Direct versus inherited, and why. Pure: two responses in, a label out. It
 * holds no clock and evaluates no schedule window — every "in force" statement
 * here is read off `client.policy`, which is the server's own answer.
 *
 * The branch names are the plan's §7.3 and the tests pin them, because the
 * *order* is the correctness property: a `Name` selector outranks an `Ip` one,
 * so an open direct window can still lose, and testing the schedule first
 * would print "window shut now" over a window that is wide open.
 */

export const DEFAULT_POLICY = 'default';

export type Branch =
  | '0'
  | '1'
  | '2a'
  | '2a-prime'
  | '2b'
  | '2c'
  | '3'
  | '4-via'
  | '4-inherited';

export interface Classification {
  branch: Branch;
  /** `assignment_source`'s, taken verbatim — the one signal Rust computes. */
  style: 'solid' | 'dashed';
  /** What the chip prints. */
  policy: string;
  /** `client.policy` — the policy in force at the instant of the read. */
  inForce: string;
  /** The words beside the chip. Empty when the branch proves nothing. */
  note: string;
  tone: 'normal' | 'warn';
  /** The direct assignment's schedule text, on branches 1 and 2b. */
  schedule: string | null;
  /** The name a deciding name assignment matched, on branch 2a. */
  nameWitness: string | null;
  /** The single inherited selector, on branch 4 and on 2b when one exists. */
  via: string | null;
}

/** `days` is echoed as configured; the `-` becomes an en dash for display
 *  only, and the value sent back on a `PATCH` is the string the API gave. */
function displayDays(days: string): string {
  return days.split('-').join('–');
}

/** §7.4, one formatter for Clients, Policies and the Rule Tester so the three
 *  cannot word a schedule differently. */
export function scheduleText(assignment: Assignment): string {
  const days = assignment.days;
  const window =
    assignment.start !== undefined && assignment.end !== undefined
      ? `${assignment.start} → ${assignment.end}`
      : null;
  if (days === undefined) {
    return window === null ? 'no schedule — always' : `daily · ${window}`;
  }
  return window === null
    ? `${displayDays(days)} · all day`
    : `${displayDays(days)} · ${window}`;
}

export function hasSchedule(assignment: Assignment): boolean {
  return (
    assignment.days !== undefined ||
    assignment.start !== undefined ||
    assignment.end !== undefined
  );
}

interface Owned {
  policy: Policy;
  assignment: Assignment;
}

/**
 * The **raw string equality** the Rust uses — `direct_assignment` and
 * `PolicyResolver` both compare `assignment.client == ip.to_string()`
 * (`routes.rs`). Parsed-selector equality would be a second, subtly different
 * rule, and the point of matching Rust's comparison is that the two can then
 * disagree only when the responses describe different instants.
 *
 * The API keeps at most one such assignment, but a hand-edited TOML can hold
 * several and Rust takes the first — so this takes the first too, in item
 * order then array order.
 */
function findDirect(ip: string, policies: readonly Policy[]): Owned | null {
  for (const policy of policies) {
    for (const assignment of policy.assignments) {
      if (assignment.client === ip) return { policy, assignment };
    }
  }
  return null;
}

function assignmentsOf(id: string, policies: readonly Policy[]): Assignment[] {
  return policies.find((policy) => policy.id === id)?.assignments ?? [];
}

/** Every assignment owned by `id` whose selector parses as a `Name` matching
 *  this client. `default` is never among the configured items, so it has none. */
function namedMembers(
  client: Client,
  id: string,
  policies: readonly Policy[],
): Assignment[] {
  return assignmentsOf(id, policies).filter((assignment) => {
    const selector = parseSelector(assignment.client);
    return (
      selector !== null &&
      selector.kind === 'name' &&
      matchesClient(selector, client.ip, client.name)
    );
  });
}

/** Every assignment owned by `id` that matches this client by any selector. */
function candidates(
  client: Client,
  id: string,
  policies: readonly Policy[],
): Assignment[] {
  return assignmentsOf(id, policies).filter((assignment) => {
    const selector = parseSelector(assignment.client);
    return selector !== null && matchesClient(selector, client.ip, client.name);
  });
}

/** D2's two spellings, plus the residual `Ip` case a non-canonical spelling
 *  can produce. The selector is printed as configured, never re-spelled. */
function viaText(assignment: Assignment): string {
  const selector = parseSelector(assignment.client);
  const spec = assignment.client.trim();
  return selector !== null && selector.kind === 'name'
    ? `via name ${spec}`
    : `via ${spec}`;
}

function soleVia(
  client: Client,
  id: string,
  policies: readonly Policy[],
): string | null {
  const matching = candidates(client, id, policies);
  const only = matching[0];
  return matching.length === 1 && only !== undefined ? viaText(only) : null;
}

function base(
  branch: Branch,
  style: 'solid' | 'dashed',
  policy: string,
  inForce: string,
  note: string,
  tone: 'normal' | 'warn',
): Classification {
  return {
    branch,
    style,
    policy,
    inForce,
    note,
    tone,
    schedule: null,
    nameWitness: null,
    via: null,
  };
}

/**
 * §7.3, branch for branch. The chip style is `assignment_source`'s; the
 * procedure only locates the assignment behind it so the note can name a
 * schedule or a reason. A branch that cannot prove *why* says nothing.
 */
export function classifyAssignment(
  client: Client,
  policies: readonly Policy[],
): Classification {
  const inForce = client.policy;

  if (client.assignment_source === 'direct') {
    const direct = findDirect(client.ip, policies);

    // Branch 0. The two responses describe different instants — a mutation
    // landed between them. Claim nothing.
    if (direct === null) {
      return base('0', 'solid', inForce, inForce, '', 'normal');
    }

    if (direct.policy.id === inForce) {
      const schedule = scheduleText(direct.assignment);
      return {
        ...base(
          '1',
          'solid',
          inForce,
          inForce,
          `${schedule} · in force`,
          'normal',
        ),
        schedule,
      };
    }

    const named = namedMembers(client, inForce, policies);
    const scheduleless = named.find((member) => !hasSchedule(member));

    // 2a — proven: a schedule-less name assignment on the in-force policy is
    // active by construction and outranks any address assignment, open or not.
    if (scheduleless !== undefined) {
      const witness = client.name ?? scheduleless.client;
      return {
        ...base(
          '2a',
          'solid',
          direct.policy.id,
          inForce,
          `not in force — the name assignment on "${witness}" decides (${inForce})`,
          'warn',
        ),
        nameWitness: witness,
      };
    }

    // 2a' — a *scheduled* name assignment proves nothing without a clock: it
    // may be open and deciding, or shut while something else lands the client
    // on the same policy. State only what `client.policy` already proves.
    if (named.length > 0) {
      return base(
        '2a-prime',
        'solid',
        direct.policy.id,
        inForce,
        `not in force — ${inForce} in force`,
        'warn',
      );
    }

    // 2b — sound only because `named(P)` is empty: nothing but a `Name` could
    // outrank an open direct window, and an open name assignment deciding
    // would have made its owner the in-force policy.
    if (hasSchedule(direct.assignment)) {
      const schedule = scheduleText(direct.assignment);
      return {
        ...base(
          '2b',
          'solid',
          direct.policy.id,
          inForce,
          `${schedule} · window shut now — ${inForce} in force`,
          'warn',
        ),
        schedule,
        via:
          inForce === DEFAULT_POLICY
            ? null
            : soleVia(client, inForce, policies),
      };
    }

    // 2c — a schedule-less direct assignment with no name route to the
    // in-force policy should be deciding. It is not, so the responses
    // disagree. No reason is claimed.
    return base(
      '2c',
      'solid',
      direct.policy.id,
      inForce,
      `not in force — ${inForce} in force`,
      'warn',
    );
  }

  // `assignment_source` absent. Any string-equal assignment the lookup might
  // find is ignored: the same disagreement, and the same silence.
  if (inForce === DEFAULT_POLICY) {
    return base(
      '3',
      'dashed',
      DEFAULT_POLICY,
      inForce,
      'inherited · no assignment',
      'normal',
    );
  }

  const via = soleVia(client, inForce, policies);
  return via === null
    ? base('4-inherited', 'dashed', inForce, inForce, 'inherited', 'normal')
    : { ...base('4-via', 'dashed', inForce, inForce, via, 'normal'), via };
}

/** The Rule Tester's policy mode, where assignments do not enter into it. */
export const CHOSEN_POLICY_REASON =
  'you chose this policy — assignments are ignored';

/**
 * §8.4's "why that policy" sentence, from the same classification the Clients
 * chip uses. Nothing here is a second reading of the responses.
 */
export function whyPolicyApplies(classification: Classification): string {
  const { inForce } = classification;
  switch (classification.branch) {
    case '1':
      return `assignment on this address · ${classification.schedule ?? ''} · in force now`;
    case '2a':
      return `the assignment on this address is not deciding — the name assignment on "${classification.nameWitness ?? ''}" decides (${inForce})`;
    case '2b': {
      const head = `assignment on this address · ${classification.schedule ?? ''} · window shut — ${inForce} in force`;
      return classification.via === null
        ? head
        : `${head} · ${classification.via}`;
    }
    case '3':
      return 'no assignment covers this address';
    case '4-via':
      return classification.via ?? 'inherited';
    case '4-inherited':
      return 'inherited';
    default:
      return `not in force — ${inForce} in force`;
  }
}
