import { request } from './core';
import type { RuleTestBody, RuleTestResult, UserRules } from './types';

export const USER_RULES_PATH = '/api/v1/rules/user';
export const RULES_TEST_PATH = '/api/v1/rules/test';

export function getUserRules(signal?: AbortSignal): Promise<UserRules> {
  return request<UserRules>(USER_RULES_PATH, {
    ...(signal === undefined ? {} : { signal }),
  });
}

/**
 * All-or-nothing, and **it recompiles**: `set_user_rules` runs a full compile
 * and atomic swap inline, so this holds the connection open for the same
 * duration a list refresh does. Never aborted — cancelling mid-compile can
 * leave the persisted document ahead of the live matcher with the client
 * unable to tell whether the write landed.
 *
 * `422` on any unparseable line, with per-line messages joined by `; `. The
 * response's `rules` is authoritative: exact-duplicate rule lines are dropped,
 * so it can be shorter than what was sent.
 */
export function putUserRules(
  rules: readonly string[],
  signal?: AbortSignal,
): Promise<UserRules> {
  return request<UserRules>(USER_RULES_PATH, {
    method: 'PUT',
    body: { rules },
    ...(signal === undefined ? {} : { signal }),
  });
}

/** Reads the running matcher. No recompile, no write, nothing cached. */
export function testRule(
  body: RuleTestBody,
  signal?: AbortSignal,
): Promise<RuleTestResult> {
  return request<RuleTestResult>(RULES_TEST_PATH, {
    method: 'POST',
    body,
    ...(signal === undefined ? {} : { signal }),
  });
}
