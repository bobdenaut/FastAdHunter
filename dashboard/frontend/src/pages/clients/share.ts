import type { Client } from '../../api/types';

export function blockedShare(client: Client): number {
  return client.queries_24h === 0
    ? 0
    : (client.blocked_24h / client.queries_24h) * 100;
}
