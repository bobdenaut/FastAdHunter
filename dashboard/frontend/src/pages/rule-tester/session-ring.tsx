import { Card } from '../../components/card';
import { VerdictPill } from '../../components/verdict-pill';
import { USER_RULES_LIST, type TestRecord } from './result-card';

/**
 * The previous tests, **bounded at ten and session-only**. Nothing persists it
 * and nothing polls it: the ring dies with the page, which is phase constraint
 * 7 applied to the one place on these four screens where state could otherwise
 * accumulate with use.
 */
export const RING_LIMIT = 10;

/** Newest first, oldest dropped. Pure so the bound is a tested property rather
 *  than a hope about a `slice` written inline. */
export function pushRecord(
  ring: readonly TestRecord[],
  record: TestRecord,
): TestRecord[] {
  return [record, ...ring].slice(0, RING_LIMIT);
}

export function SessionRing({ ring }: { ring: readonly TestRecord[] }) {
  return (
    <Card title="Previous tests" secondary="this session only">
      {ring.length === 0 ? (
        <p class="note" style={{ margin: 0 }}>
          Nothing yet. The last {RING_LIMIT} tests appear here and are forgotten
          when the page is left.
        </p>
      ) : (
        <div class="ring">
          {ring.map((record, index) => (
            <div class="ring-row" key={index}>
              <VerdictPill verdict={record.result.verdict} />
              <span class="mono">{record.domain}</span>
              {record.sentPolicy !== null ? (
                <>
                  <span class="note">under policy</span>
                  <span class="mono">{record.sentPolicy}</span>
                </>
              ) : record.sentClient === null ? (
                <>
                  <span class="note">as</span>
                  <span class="note">the default policy</span>
                </>
              ) : (
                <>
                  <span class="note">as</span>
                  <span class="mono">{record.sentClient}</span>
                </>
              )}
              <span class="note ring-detail">
                {record.result.rule === null ? (
                  <>no rule matched · policy {record.result.policy}</>
                ) : (
                  <>
                    <span class="mono">{record.result.rule}</span> ·{' '}
                    {record.result.list === USER_RULES_LIST
                      ? 'your custom rules'
                      : (record.result.list ?? 'no list')}
                  </>
                )}
              </span>
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}
