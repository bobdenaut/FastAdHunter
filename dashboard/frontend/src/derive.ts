/**
 * **Every derived display value on the Dashboard and Lists lives here.**
 *
 * Phase 5 standing constraint 1 forbids figures the API does not support. A
 * derivation is a stated arithmetic function of documented fields; anything not
 * in this module is read from a response verbatim. The row ids are the p5-06
 * plan's §8.2 table, so a reviewer checks one file against one table rather than
 * hunting arithmetic through the pages.
 *
 * Never derived at all, and deliberately absent: a 24 h HTTP figure (the API
 * has none), a combined DNS + HTTP "queries" total (the pipelines are never
 * summed), an upstream share of traffic (no per-query attribution exists), and
 * a per-client blocked figure from `/stats` (it is not there).
 */

/** R2 / R3 — the chart's range totals, summed from the same items that draw
 *  the bars. `/stats` is a rolling 24 h window and cannot state a 7 d total. */
export function sumOver<T>(items: readonly T[], of: (item: T) => number): number {
  let total = 0;
  for (const item of items) total += of(item);
  return total;
}

/** R4 — the one aggregate the API does not serve. Exact over one response, not
 *  an average of averages. The per-bucket `blocked_percent` is served and is
 *  read verbatim by the tooltip; this is the range figure, which has no field. */
export function blockedPercent(queries: number, blocked: number): number {
  return queries === 0 ? 0 : (blocked / queries) * 100;
}

/** R8 / R9 / R18 — max-normalisation over the rendered rows, which is the
 *  scale `Main.dc.html` settles with its own figures (3,140 / 4,021 = 78 %).
 *  An all-zero column draws empty tracks rather than full ones. */
export function shareOfMax(value: number, max: number): number {
  return max <= 0 ? 0 : value / max;
}

export interface Slice {
  label: string;
  value: number;
}

export const OTHER_LABEL = 'other';

/**
 * R13 / R14 — the donut's slices. `per_type` is summed over the active range's
 * items first (zero buckets are omitted from the response, so an absent label
 * is a zero rather than an unknown), then everything outside the `keep` largest
 * is folded into one `other` slice, as the artboards draw.
 *
 * `other` is appended only when something actually falls outside, and it is
 * never one label renamed — a single leftover label keeps its own name.
 */
export function queryTypeSlices(
  totals: Record<string, number>,
  keep = 4,
): Slice[] {
  const ranked = Object.entries(totals)
    .filter(([, value]) => value > 0)
    .map(([label, value]) => ({ label, value }))
    .sort((a, b) => b.value - a.value || a.label.localeCompare(b.label));

  if (ranked.length <= keep + 1) return ranked;

  const head = ranked.slice(0, keep);
  const tail = ranked.slice(keep);
  head.push({
    label: OTHER_LABEL,
    value: sumOver(tail, (slice) => slice.value),
  });
  return head;
}

/** Sums `per_type` across a range's items. Kept beside the fold because the two
 *  are one figure in two steps and splitting them invites a second summation. */
export function sumPerType(
  items: ReadonlyArray<{ per_type: Record<string, number> }>,
): Record<string, number> {
  const totals: Record<string, number> = {};
  for (const item of items) {
    for (const [label, value] of Object.entries(item.per_type)) {
      totals[label] = (totals[label] ?? 0) + value;
    }
  }
  return totals;
}

/**
 * R18 / R19 — the Upstream Health bar, and **the complete list of upstream
 * arithmetic on this page**.
 *
 * ```text
 * bar width  = attempts / max(attempts)      relative workload
 * overlay    = failures / attempts           failure rate within that bar
 * state      → the dot and its colour, independently
 * text       → "N attempts · M failures", verbatim, always visible
 * ```
 *
 * Nothing else about an upstream is computed here: no success rate, no health
 * score, no share of traffic, no availability percentage, and nothing derived
 * from `consecutive_failures`, `failure_runs`, `penalty_round`, `penalties`,
 * `penalized_seconds_total`, `probes`, `probe_successes` or `tls_handshakes`.
 * Those fields exist on the response and this card does not render them.
 *
 * **The overlay is never given a minimum width.** At 12 failures in 201,883 it
 * is 0.006 % of the bar and simply disappears — the figure survives in the
 * printed text. Widening it to be visible would draw a failure rate the
 * endpoint does not have, which is the one thing this card must not do.
 */
export interface UpstreamBar {
  /** 0..1 of the track. */
  width: number;
  /** 0..1 **of the bar**, not of the track. */
  overlay: number;
}

export function upstreamBar(
  attempts: number,
  failures: number,
  maxAttempts: number,
): UpstreamBar {
  return {
    width: shareOfMax(attempts, maxAttempts),
    overlay: attempts <= 0 ? 0 : Math.min(1, failures / attempts),
  };
}
