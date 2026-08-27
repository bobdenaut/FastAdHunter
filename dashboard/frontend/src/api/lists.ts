import { request } from './core';
import type {
  AddListRequest,
  ListItem,
  ListsResponse,
  PatchListRequest,
  RefreshAllResponse,
} from './types';

export const LISTS_PATH = '/api/v1/lists';

export function getLists(signal?: AbortSignal): Promise<ListsResponse> {
  return request<ListsResponse>(LISTS_PATH, {
    ...(signal === undefined ? {} : { signal }),
  });
}

/** `409` on a derived-id collision, and on a source another list already
 *  holds. The envelope names the other list in both cases. */
export function addList(
  body: AddListRequest,
  signal?: AbortSignal,
): Promise<ListItem> {
  return request<ListItem>(LISTS_PATH, {
    method: 'POST',
    body,
    ...(signal === undefined ? {} : { signal }),
  });
}

/** A partial update: an absent `refresh_hours` leaves the interval alone,
 *  `null` clears the per-list override back to the configured default. */
export function patchList(
  id: string,
  body: PatchListRequest,
  signal?: AbortSignal,
): Promise<ListItem> {
  return request<ListItem>(`${LISTS_PATH}/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body,
    ...(signal === undefined ? {} : { signal }),
  });
}

/**
 * Removes the cached copy before it drops the list, and answers `500` with the
 * list kept if it cannot — so a success means the content-gate baseline is
 * gone. That is what makes delete-and-re-add the recovery for a `rejected`
 * list, and what makes a `500` here a reason to stop rather than retry.
 */
export function deleteList(id: string, signal?: AbortSignal): Promise<void> {
  return request<void>(`${LISTS_PATH}/${encodeURIComponent(id)}`, {
    method: 'DELETE',
    ...(signal === undefined ? {} : { signal }),
  });
}

/** `202 Accepted`. The outcome arrives as a `list_refreshed` event, not in
 *  this response — the two must not be presented as one act. */
export function refreshList(id: string, signal?: AbortSignal): Promise<void> {
  return request<void>(`${LISTS_PATH}/${encodeURIComponent(id)}/refresh`, {
    method: 'POST',
    ...(signal === undefined ? {} : { signal }),
  });
}

/** Synchronous and blocking: one pass over every enabled list, one recompile,
 *  and the per-list outcome in the body. */
export function refreshAllLists(
  signal?: AbortSignal,
): Promise<RefreshAllResponse> {
  return request<RefreshAllResponse>(`${LISTS_PATH}/refresh`, {
    method: 'POST',
    ...(signal === undefined ? {} : { signal }),
  });
}
