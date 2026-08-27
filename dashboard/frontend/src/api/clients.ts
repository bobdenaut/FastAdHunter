import { request } from './core';
import type { ClientsResponse } from './types';

export const CLIENTS_PATH = '/api/v1/clients';

/**
 * The Dashboard's Top-clients card reads this rather than `/stats.top_clients`:
 * the artboards draw a blocked figure beside the query count, and only this
 * response carries `blocked_24h`. Bounded by the number of observed addresses.
 */
export function getClients(signal?: AbortSignal): Promise<ClientsResponse> {
  return request<ClientsResponse>(CLIENTS_PATH, {
    ...(signal === undefined ? {} : { signal }),
  });
}
