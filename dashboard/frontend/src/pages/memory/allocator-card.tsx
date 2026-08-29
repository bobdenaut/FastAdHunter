import type { DebugMemory, HistoryPerf } from '../../api/types';
import { formatMiB } from '../../charts/format';
import { Card } from '../../components/card';
import { extentOf } from '../../derive';
import { RANGES, type RangeKey } from '../dashboard/ranges';

/**
 * The readings that are figures rather than charts, and the reason each one is.
 *
 * **Two different contracts sit in this table, and the tag is the boundary.**
 * The kernel figures come from `getrusage` and stay readable whatever is linked
 * in as the global allocator, which is what lets `/telemetry` promise them. The
 * two committed counters read near zero under a different allocator, carry no
 * compatibility promise, and are labelled `no contract` for that reason rather
 * than for looking cautious.
 *
 * **None of them is charted, and that is the same decision three times.** The
 * kernel figures are cumulative or monotone within a process lifetime, so a
 * chart of one draws a ramp — and a ramp that resets on restart reads as a
 * fall that never happened. The committed pair is not monotone — mimalloc v3
 * accounts purges, and the p2.6 audit observed it decreasing — but charting it
 * would need a design pass this page has not opened. The one that moves is the
 * fault *rate*, which is a card of its own.
 */
export function AllocatorCard({
  memory,
  history,
  range,
}: {
  memory: DebugMemory | null;
  history: HistoryPerf | null;
  /** The window `sampled series min / max` covers — it moves with the chips. */
  range: RangeKey;
}) {
  // D7, the shared window extent. It skips absent rows rather than reading them
  // as zero, which is the whole distinction this page keeps everywhere else.
  const sampled = extentOf(history?.items ?? [], (item) => item.rss_bytes);

  return (
    <Card title="Kernel & allocator" secondary="figures, not charts">
      <table class="kv-table">
        <tbody>
          <tr>
            <td>major page faults</td>
            <td
              class={
                memory?.major_page_faults === 0 ? 'num kv-good' : 'num'
              }
            >
              {memory?.major_page_faults ?? '—'}
            </td>
          </tr>
          <tr>
            <td>
              minor page faults <span class="note">lifetime</span>
            </td>
            <td class="num">
              {/* Grouped in full, not compacted: this row's whole job is the
                  exact lifetime counter — the rate is charted next door, and
                  `4.2M` throws away the precision the row exists for. */}
              {memory?.minor_page_faults == null
                ? '—'
                : memory.minor_page_faults.toLocaleString()}
            </td>
          </tr>
          <tr>
            <td>
              peak RSS <span class="note">lifetime</span>
            </td>
            <td class="num">
              {memory?.process_peak_rss == null
                ? '—'
                : formatMiB(memory.process_peak_rss)}
            </td>
          </tr>
          <tr>
            <td>
              sampled series min / max{" "}
              <span class="note">{RANGES[range].label}</span>
            </td>
            <td class="num">
              {sampled === null
                ? '—'
                : `${formatMiB(sampled.min)} / ${formatMiB(sampled.max)}`}
            </td>
          </tr>
          <tr>
            <td>
              allocator committed <span class="tagq">no contract</span>
            </td>
            <td class="num">
              {memory?.allocator_committed_bytes == null
                ? '—'
                : formatMiB(memory.allocator_committed_bytes)}
            </td>
          </tr>
          <tr>
            <td>
              committed peak <span class="tagq">no contract</span>
            </td>
            <td class="num">
              {memory?.allocator_committed_peak_bytes == null
                ? '—'
                : formatMiB(memory.allocator_committed_peak_bytes)}
            </td>
          </tr>
        </tbody>
      </table>
      <p class="note kv-note">
        <b>Major faults are structurally near-zero</b> — nothing FastAdHunter
        touches is demand-paged from disk, so a non-zero value means host memory
        pressure, not a FastAdHunter problem.
        <span class="footnote-line">
          The committed pair comes from whichever allocator is linked in and
          carries no compatibility promise, which is why it is labelled and
          parked here rather than charted beside the kernel readings.
        </span>
      </p>
    </Card>
  );
}
