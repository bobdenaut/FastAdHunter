/**
 * `ok` / `degraded` / `failed` / `rejected` — the vocabulary of
 * `POST /api/v1/lists/refresh` and `GET /health`. Same rule as the verdict
 * pill: the word travels with the colour.
 *
 * `degraded` is amber and not red on purpose: under the adaptive strategy it
 * means no endpoint is currently healthy, and the box is still answering
 * (API.md §GET /health).
 */
export type Status = 'ok' | 'degraded' | 'failed' | 'rejected' | 'penalized' | 'probing';

const TONE: Record<Status, string> = {
  ok: 'good',
  degraded: 'warn',
  failed: 'bad',
  rejected: 'warn',
  penalized: 'warn',
  probing: 'special',
};

export function StatusPill({ status }: { status: Status }) {
  return <span class={`pill ${TONE[status]}`}>{status}</span>;
}
