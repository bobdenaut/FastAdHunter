/**
 * `ok` / `degraded` / `failed` / `rejected` / `never` — the five values of
 * `GET /api/v1/lists`' `last_status` — plus `penalized` / `probing` from
 * `/telemetry`'s upstream state and `disabled`, which is not a status at all
 * but the artboards' pill for `enabled === false`. Same rule as the verdict
 * pill: the word travels with the colour.
 *
 * `degraded` is amber and not red on purpose. On `/health` it means no endpoint
 * is currently healthy while the box still answers; on a list it means the
 * fetch succeeded and most of the body failed to parse. Neither is a milder
 * `ok`, and neither is an outage.
 */
export type Status =
  | 'ok'
  | 'degraded'
  | 'failed'
  | 'rejected'
  | 'never'
  | 'disabled'
  | 'penalized'
  | 'probing';

const TONE: Record<Status, string> = {
  ok: 'good',
  degraded: 'warn',
  failed: 'bad',
  rejected: 'warn',
  never: 'neutral',
  disabled: 'neutral',
  penalized: 'warn',
  probing: 'special',
};

export function StatusPill({ status }: { status: Status }) {
  return <span class={`pill ${TONE[status]}`}>{status}</span>;
}
