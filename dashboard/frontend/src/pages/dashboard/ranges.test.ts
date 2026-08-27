import { describe, expect, it } from 'vitest';
import { RANGES, plottedResolution, rangeQuery } from './ranges';

/**
 * The chips and the response disagree for exactly one round trip, and the chart
 * has to follow the response — see `plottedResolution`.
 */
describe('the resolution the chart draws with', () => {
  it('is the response’s, not the chip’s, while a range change is in flight', () => {
    // The 30 d response is still on screen; the chips already say 24 h. Reading
    // the chip here is what labelled thirty daily buckets `00:00` four times
    // over, for the length of the fetch.
    expect(plottedResolution({ resolution: 'day' }, '24h')).toBe('day');
    expect(plottedResolution({ resolution: 'hour' }, '30d')).toBe('hour');
  });

  it('falls back to the chip only when nothing is plotted', () => {
    expect(plottedResolution(null, '24h')).toBe('hour');
    expect(plottedResolution(null, '7d')).toBe('day');
  });

  it('agrees with the chip once the response has landed', () => {
    for (const key of ['24h', '7d', '30d'] as const) {
      const { resolution } = rangeQuery(key, Date.UTC(2026, 7, 26));
      expect(plottedResolution({ resolution }, key)).toBe(
        RANGES[key].resolution,
      );
    }
  });
});
