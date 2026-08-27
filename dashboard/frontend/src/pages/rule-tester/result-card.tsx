import type { RuleTestResult } from '../../api/types';
import { Card } from '../../components/card';
import { VerdictPill } from '../../components/verdict-pill';
import { Icon } from '../../shell/icon';

/** `lifecycle/mod.rs` names the user document `user-rules` on the wire; the
 *  page it comes from calls it "your custom rules", and so does this. */
export const USER_RULES_LIST = 'user-rules';

export interface TestRecord {
  domain: string;
  qtype: string;
  /** What was actually sent as `client`, or `null` in policy mode. */
  sentClient: string | null;
  /** The name typed, when it was resolved to an address. */
  resolvedFrom: string | null;
  /** The policy asked for, in policy mode. */
  sentPolicy: string | null;
  result: RuleTestResult;
  /** Why the deciding policy applies — §8.5 T13, the only derived field here. */
  why: string;
  /** A name that has never been observed: the engine had no address to select
   *  a policy from, so the policy half of the answer is not an answer. */
  partial: boolean;
}

export function ResultCard({ record }: { record: TestRecord }) {
  const { result } = record;
  const subject =
    record.sentPolicy !== null
      ? `under policy ${record.sentPolicy}`
      : `for ${record.sentClient ?? 'the default policy'}`;

  return (
    <Card
      title="Result"
      secondary="the real engine, not a text search"
      className="tester-result"
    >
      {record.partial && (
        <div class="banner" role="status">
          <Icon name="warning" size={16} className="warning" />
          <div>
            <b>Partial answer.</b> The engine selects a policy from an{' '}
            <i>address</i>, and this box has never seen a client named{' '}
            <span class="mono">{record.sentClient}</span>. The verdict, the rule
            and the list are real — they account for any{' '}
            <span class="mono">$client</span> rule naming it — but{' '}
            <b>deciding policy reads default whatever is assigned</b>, because
            no address was given to resolve.
          </div>
        </div>
      )}

      <div class="tester-verdict">
        <span class={`verdict-slab ${result.verdict}`}>{result.verdict}</span>
        <span class="tester-verdict-line">
          <span class="mono">{record.domain}</span> would be{' '}
          {result.verdict === 'block'
            ? 'blocked'
            : result.verdict === 'allow'
              ? 'allowed by an exception'
              : 'passed'}{' '}
          {subject}
        </span>
      </div>

      <div class="kv">
        <span>matching rule</span>
        <span class="mono">{result.rule ?? 'no rule matched'}</span>
        <span>from list</span>
        <span class={result.list === null ? undefined : 'mono'}>
          {result.list === null
            ? '—'
            : result.list === USER_RULES_LIST
              ? 'your custom rules'
              : result.list}
        </span>
        <span>deciding policy</span>
        <span class="mono">{result.policy}</span>
        <span>why that policy</span>
        <span>{record.why}</span>
        {record.resolvedFrom !== null && (
          <>
            <span>tested as</span>
            <span class="mono">
              {record.sentClient} ({record.resolvedFrom})
            </span>
          </>
        )}
      </div>
    </Card>
  );
}

export function VerdictBadge({ verdict }: { verdict: RuleTestResult['verdict'] }) {
  return <VerdictPill verdict={verdict} />;
}
