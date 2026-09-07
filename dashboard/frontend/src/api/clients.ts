import { request } from './core';
import type {
  Client,
  ClientFamily,
  ClientPolicyBody,
  ClientPolicyResponse,
  ClientsResponse,
} from './types';

export const CLIENTS_PATH = '/api/v1/clients';

/**
 * The Dashboard's Top-clients card reads this rather than `/stats.top_clients`:
 * the artboards draw a blocked figure beside the query count, and only this
 * response carries `blocked_24h`. Bounded by the number of observed addresses.
 *
 * It is also the Clients page's whole policy column. `p5-03` put `policy` and
 * `assignment_source` on every item for exactly that reason — there is no
 * per-row read of `GET /clients/{ip}/policy` anywhere in this application, and
 * no accessor for it, because a household with forty observed clients would
 * pay forty extra requests per page load.
 *
 * `family` narrows the list to one address family on the server; absent lists
 * both. The Clients page reads IPv4 by default, the Top-clients card reads all.
 */
export function getClients(
  signal?: AbortSignal,
  family?: ClientFamily,
): Promise<ClientsResponse> {
  return request<ClientsResponse>(
    family === undefined ? CLIENTS_PATH : `${CLIENTS_PATH}?family=${family}`,
    {
      ...(signal === undefined ? {} : { signal }),
    },
  );
}

function clientPath(ip: string): string {
  return `${CLIENTS_PATH}/${encodeURIComponent(ip)}`;
}

/**
 * `null` clears the name. Live in milliseconds — a rename republishes the
 * client → policy snapshot and recompiles nothing.
 *
 * `404` when the address aged out of the registry between the read and the
 * write: the API knows clients only by traffic.
 */
export function setClientName(
  ip: string,
  name: string | null,
  signal?: AbortSignal,
): Promise<Client> {
  return request<Client>(clientPath(ip), {
    method: 'PUT',
    body: { name },
    ...(signal === undefined ? {} : { signal }),
  });
}

/**
 * One assignment per address: the handler strips this address from every
 * policy before adding it to the named one, so saving replaces whatever
 * assignment it already had, in whichever policy held it.
 *
 * `Recompile::No` — live in milliseconds, which is why it neither confirms nor
 * blocks. `404` names a policy that does not exist.
 */
export function setClientPolicy(
  ip: string,
  body: ClientPolicyBody,
  signal?: AbortSignal,
): Promise<ClientPolicyResponse> {
  return request<ClientPolicyResponse>(`${clientPath(ip)}/policy`, {
    method: 'PUT',
    body,
    ...(signal === undefined ? {} : { signal }),
  });
}

/** `204`, or `404` when nothing was assigned to this address. Also live. */
export function clearClientPolicy(
  ip: string,
  signal?: AbortSignal,
): Promise<void> {
  return request<void>(`${clientPath(ip)}/policy`, {
    method: 'DELETE',
    ...(signal === undefined ? {} : { signal }),
  });
}
