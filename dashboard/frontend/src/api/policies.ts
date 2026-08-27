import { request } from './core';
import type {
  CreatePolicyBody,
  PatchPolicyBody,
  PoliciesResponse,
  Policy,
} from './types';

export const POLICIES_PATH = '/api/v1/policies';

export function getPolicies(signal?: AbortSignal): Promise<PoliciesResponse> {
  return request<PoliciesResponse>(POLICIES_PATH, {
    ...(signal === undefined ? {} : { signal }),
  });
}

/**
 * `201`, and **it recompiles**: a new policy adds a bit to every rule's mask,
 * so the handler awaits the whole rebuild inline (`routes.rs` `create_policy`
 * → `Recompile::Yes`). The request is held open for seconds on the RB5009 and
 * is never aborted — the caller blocks rather than cancels.
 *
 * `409` when the id exists, `422` from the config validator (the 16-policy
 * ceiling, a list that is not in `[[rules.lists]]`, a malformed schedule).
 */
export function createPolicy(
  body: CreatePolicyBody,
  signal?: AbortSignal,
): Promise<Policy> {
  return request<Policy>(POLICIES_PATH, {
    method: 'POST',
    body,
    ...(signal === undefined ? {} : { signal }),
  });
}

/**
 * Only the changed fields are sent. A `lists` change recompiles and everything
 * else is live in milliseconds, so sending an unchanged subset back would make
 * an assignment edit indistinguishable from a subset edit in the request log
 * even though neither would rebuild.
 */
export function patchPolicy(
  id: string,
  body: PatchPolicyBody,
  signal?: AbortSignal,
): Promise<Policy> {
  return request<Policy>(`${POLICIES_PATH}/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body,
    ...(signal === undefined ? {} : { signal }),
  });
}

/** `204`, and it recompiles. */
export function deletePolicy(id: string, signal?: AbortSignal): Promise<void> {
  return request<void>(`${POLICIES_PATH}/${encodeURIComponent(id)}`, {
    method: 'DELETE',
    ...(signal === undefined ? {} : { signal }),
  });
}
