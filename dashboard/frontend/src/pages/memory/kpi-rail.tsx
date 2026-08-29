import type { ComponentChildren } from 'preact';
import type { DebugMemory, PerfItem } from '../../api/types';
import { formatMiB } from '../../charts/format';
import { sliceShare } from '../../derive';
import { STEADY_STATE_BUDGET, budgetLabel, latestRssState } from './budgets';
import { PeakSpark, Sparkline } from './sparkline';
import { RANGES, type RangeKey } from '../dashboard/ranges';

type Tone = 'ink' | 'watch' | 'over' | 'residual' | 'peak' | 'muted';

/**
 * The four figures the page opens with — RSS, residual, lifetime peak,
 * allocator committed.
 *
 * **Each carries its own denominator, and two of them deliberately have none.**
 * RSS is a share of the steady-state budget. Residual is a share of current
 * RSS, because it is a slice of RSS and measuring a slice against a budget
 * would say nothing. The other two get no bar at all, which is the whole reason
 * this is four cards rather than four identical gauges:
 *
 * - **Peak against the budget is a false breach.** It is a lifetime high-water
 *   mark, and the mark this process sets is the startup compile — seconds long,
 *   never sampled. Drawing it as "115 % of budget" invites treating a transient
 *   as a violation and setting `memory-high`, which has already OOM-killed this
 *   household's resolver once.
 * - **Committed carries no contract.** It is whatever allocator is linked in
 *   reports, and it runs several times RSS as a matter of course. It is not
 *   monotone: the p2.6 audit observed it decreasing under mimalloc v3, so a
 *   fall is the allocator accounting a purge, not a restart. RSS is the
 *   authority on footprint; a budget bar under committed would make the honest
 *   number look like the alarming one.
 */
export function KpiRail({
  memory,
  items,
  rss,
  range,
}: {
  memory: DebugMemory;
  items: readonly PerfItem[];
  rss: number;
  /** The window the sparklines cover. The chips govern the whole page, so a
   *  card that reads history must name the range it is showing. */
  range: RangeKey;
}) {
  const residual = memory.residual_bytes;
  // The same walk the trend line is drawn from, read at its tail: the window's
  // samples with the live reading after them. The history is where successive
  // readings actually exist — a render-phase ref only ever held one page visit's
  // single reading, so the band it carried could never be crossed.
  const state = latestRssState([
    ...items.map((item) => item.rss_bytes),
    rss,
  ]);
  const tone: Tone = state === 'normal' ? 'ink' : state;
  const budgetShare = sliceShare(rss, STEADY_STATE_BUDGET);

  return (
    <div class="row c4 kpi-rail">
      <Kpi
        label="RSS · resident"
        aside={`${RANGES[range].label} trend`}
        value={rss}
        tone={tone}
        description="what the process actually holds — the authority on footprint"
        spark={
          <Sparkline items={items} of={(item) => item.rss_bytes ?? null} tone="ink" />
        }
        rail={{
          share: budgetShare,
          tone,
          left: '0',
          middle: `${budgetShare.toFixed(0)} % of steady-state budget`,
          right: budgetLabel(STEADY_STATE_BUDGET),
        }}
      />

      <Kpi
        label="Residual / unaccounted"
        aside={`${RANGES[range].label} trend`}
        value={residual}
        tone="residual"
        description={
          <>
            <span class="mono">process_rss − accounted_bytes</span> — never zero,
            only the trend matters
          </>
        }
        spark={
          <Sparkline
            items={items}
            of={(item) => item.memory?.residual_bytes ?? null}
            tone="residual"
          />
        }
        rail={
          residual === null
            ? null
            : {
                share: sliceShare(residual, rss),
                tone: 'residual',
                left: '0',
                middle: `${sliceShare(residual, rss).toFixed(1)} % of current RSS`,
                right: formatMiB(rss),
              }
        }
      />

      <Kpi
        label="Peak RSS · lifetime"
        aside="high-water"
        value={memory.process_peak_rss}
        tone="peak"
        description="the startup compile — seconds long, never sampled by the 60 s series"
        spark={<PeakSpark items={items} />}
        sparkNote="restart resets it"
        marks={
          memory.process_peak_rss === null ? null : (
            <PeakMarks peak={memory.process_peak_rss} />
          )
        }
      />

      <Kpi
        label="Allocator committed"
        badge="no contract"
        value={memory.allocator_committed_bytes}
        tone="muted"
        description="several times RSS is the expected state, not a problem — and a fall is the allocator accounting a purge, not a restart"
        footer={
          <>
            <FooterRow
              label="committed peak"
              value={
                memory.allocator_committed_peak_bytes === null
                  ? '—'
                  : formatMiB(memory.allocator_committed_peak_bytes)
              }
            />
            <p class="note kpi-footnote">
              The two can diverge: mimalloc v3 accounts purges, so the live
              figure falls below the peak after a reclaim. Not charted here.
            </p>
          </>
        }
      />
    </div>
  );
}

/**
 * The peak card's rail: a track, the budget's tick, and peak's own dot.
 *
 * **Not a fill.** A bar would read as "115 % of budget", and a lifetime
 * high-water mark measured against a steady-state budget is a false breach —
 * the reading that invites setting `memory-high`, which has OOM-killed this
 * household's resolver once. The track is drawn to 1.25× the budget so the tick
 * sits inside it and the dot has somewhere to be on either side.
 */
function PeakMarks({ peak }: { peak: number }) {
  const axis = STEADY_STATE_BUDGET * 1.25;
  const at = (value: number) => Math.min(98, (value / axis) * 100);
  return (
    <>
      <svg class="rail-marks" viewBox="0 0 300 12" preserveAspectRatio="none">
        <rect x="0" y="3" width="300" height="7" rx="3" class="marks-track" />
        <line
          x1={(at(STEADY_STATE_BUDGET) * 3).toFixed(1)}
          y1="0"
          x2={(at(STEADY_STATE_BUDGET) * 3).toFixed(1)}
          y2="12"
          class="marks-budget"
        />
        <circle cx={(at(peak) * 3).toFixed(1)} cy="6.5" r="4" class="marks-peak" />
      </svg>
      <div class="railx">
        <span class="railx-budget">▏{budgetLabel(STEADY_STATE_BUDGET)} budget</span>
        <span class="railx-mid">
          {peak > STEADY_STATE_BUDGET
            ? 'past it — transient, not a breach'
            : 'inside the budget'}
        </span>
      </div>
    </>
  );
}

function FooterRow({ label, value }: { label: string; value: string }) {
  return (
    <div class="kpi-footer-row">
      <span>{label}</span>
      <span class="num">{value}</span>
    </div>
  );
}

interface Rail {
  share: number;
  tone: Tone;
  left: string;
  middle: string;
  right: string;
}

/**
 * `value === null` renders an em dash, never `0`. Every nullable figure here
 * means "this platform or this allocator reported nothing", and a zero would be
 * a measurement nobody took — `process_rss` is null off Linux, and the two
 * allocator counters are null under an allocator that does not keep them.
 */
function Kpi({
  label,
  aside,
  badge,
  value,
  tone,
  description,
  spark,
  sparkNote,
  marks,
  rail,
  footer,
}: {
  label: string;
  aside?: string;
  badge?: string;
  value: number | null;
  tone: Tone;
  description: ComponentChildren;
  spark?: ComponentChildren;
  /** A caption under the sparkline, where the shape needs one word of help. */
  sparkNote?: string;
  /** A marker rail, for a figure that must not be drawn as a share. */
  marks?: ComponentChildren | null;
  rail?: Rail | null;
  footer?: ComponentChildren;
}) {
  const parts = value === null ? null : formatMiB(value).split(' ');
  return (
    <section class="card kpi">
      <div class="kpil">
        <span class="l">{label}</span>
        {badge !== undefined && <span class="tagq">{badge}</span>}
        {aside !== undefined && <span class="t">{aside}</span>}
      </div>
      <div class="kpi-figure">
        <div>
          <span class={`kpiv kpiv-${tone}`}>{parts === null ? '—' : parts[0]}</span>
          {parts !== null && <span class="kpiu">{parts[1]}</span>}
        </div>
        {spark === undefined ? null : (
          <div class="kpi-spark-wrap">
            {spark}
            {sparkNote !== undefined && (
              <span class="kpi-spark-note">{sparkNote}</span>
            )}
          </div>
        )}
      </div>
      <p class="kpid">{description}</p>
      {marks != null && <div class="kpi-foot">{marks}</div>}
      {rail != null && (
        <div class="kpi-foot">
          <div class="rail">
            <div
              class={`rail-fill rail-${rail.tone}`}
              style={`width: ${Math.min(rail.share, 100).toFixed(1)}%`}
            />
          </div>
          <div class="railx">
            <span>{rail.left}</span>
            <span class="railx-mid">{rail.middle}</span>
            <span>{rail.right}</span>
          </div>
        </div>
      )}
      {footer !== undefined && <div class="kpi-foot kpi-figures">{footer}</div>}
    </section>
  );
}
