import type { PolicyStat } from '../../api/types';

/**
 * The traffic block every policy card carries, Default included.
 *
 * T8 — the bar is `blocked / queries` from `/stats.policies`, matched on the
 * policy id. A policy with no row there, or one with zero queries in the
 * window, draws an **empty track** and divides nothing: a new policy is the
 * normal case for both, and `0 / 0` is not a percentage.
 */
export function PolicyTraffic({ stat }: { stat: PolicyStat | null }) {
  const queries = stat?.queries ?? 0;
  const blocked = stat?.blocked ?? 0;
  const share = queries === 0 ? 0 : (blocked / queries) * 100;

  return (
    <div class="policy-traffic">
      <div class="policy-traffic-head">
        <span class="note">traffic, 24 h</span>
        <span class="mono num">
          {queries.toLocaleString()} queries · {blocked.toLocaleString()}{' '}
          blocked
        </span>
      </div>
      <div class="bar">
        <span
          class="policy-traffic-fill"
          style={{ width: `${String(Math.min(100, share))}%` }}
        />
      </div>
    </div>
  );
}

/**
 * `lists: null` is "every enabled list", which is what an omitted `lists`
 * means in the TOML too. An **empty array is the opposite**: `mask_for_list`
 * gives a `Some([])` policy no list at all, so a card folding `[]` into
 * "every enabled list" would claim full protection for a policy that blocks
 * nothing — it is rendered as its own state instead.
 */
export function ListChips({ lists }: { lists: string[] | null }) {
  return (
    <div class="policy-lists">
      <span class="note">lists</span>
      {lists === null ? (
        <span class="lchip">every enabled list</span>
      ) : lists.length === 0 ? (
        <span class="lchip lchip-warn">no lists — blocks nothing</span>
      ) : (
        lists.map((id) => (
          <span class="lchip" key={id}>
            {id}
          </span>
        ))
      )}
    </div>
  );
}
