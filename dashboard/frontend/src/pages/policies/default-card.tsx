import type { PolicyStat } from '../../api/types';
import { Card } from '../../components/card';
import { ListChips, PolicyTraffic } from './policy-card';

/**
 * The Default card is **synthetic**. `GET /policies` returns the *configured*
 * policies and `default` is never among them — it is implicit and reserved,
 * and the config validator refuses a policy defined with that id.
 *
 * So its copy is fixed, it carries no Edit and no Delete, and its list subset
 * is "every enabled list" by definition. Its only live figure is traffic, from
 * `/stats.policies` where `policy === "default"`.
 */
export function DefaultCard({ stat }: { stat: PolicyStat | null }) {
  return (
    <Card
      title={
        <>
          Default{' '}
          <span class="note mono policy-id">default</span>
        </>
      }
      tools={<span class="pill neutral">reserved</span>}
      className="policy-card"
    >
      <ListChips lists={null} />
      <p class="note policy-note">
        Everything not covered by another assignment, and every client whose
        schedule window is currently shut. Cannot be deleted or renamed.
      </p>
      <PolicyTraffic stat={stat} />
    </Card>
  );
}
