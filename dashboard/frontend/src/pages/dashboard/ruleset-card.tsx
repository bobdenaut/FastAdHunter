import type { Telemetry } from '../../api/types';
import { Card } from '../../components/card';
import { Figure } from '../../components/figure';
import { RefreshCluster } from '../../components/refresh-cluster';
import type { RefreshRegistry } from '../../refresh/registry';

/**
 * Three figures from `/telemetry.ruleset` and a fourth the artboard draws that
 * only `GET /lists` can supply — the count of enabled lists (R12). That is what
 * put `/lists` on this page's read set.
 *
 * The refresh cluster is deviation **X3**, bound to `lists` for the same reason
 * as the one on Top clients.
 */
export function RulesetCard({
  telemetry,
  enabledLists,
  registry,
  className,
}: {
  telemetry: Telemetry | null;
  enabledLists: number | null;
  registry: RefreshRegistry;
  className?: string;
}) {
  const ruleset = telemetry?.ruleset;

  return (
    <Card
      title="Ruleset"
      secondary="atomic swap — the hot path never waits on a compile"
      tools={<RefreshCluster registry={registry} endpoint="lists" />}
      bodyClass="figure-row"
      className={className}
    >
      <Figure
        value={ruleset === undefined ? '—' : ruleset.rules.toLocaleString()}
        label="compiled rules"
      />
      <Figure
        value={
          ruleset === undefined
            ? '—'
            : ruleset.duplicates_removed.toLocaleString()
        }
        label="duplicates removed"
      />
      <Figure
        value={
          ruleset === undefined
            ? '—'
            : `${ruleset.compile_duration_seconds.toFixed(2)} s`
        }
        label="last compile duration"
      />
      <Figure
        value={enabledLists === null ? '—' : enabledLists.toLocaleString()}
        label="enabled lists"
      />
    </Card>
  );
}
