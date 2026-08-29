import { describe, expect, it } from 'vitest';
import {
  RANGES,
  plottedRange,
  plottedResolution,
  rangeQuery,
} from './ranges';

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

/**
 * The same rule for anything that prints the range in words. `plottedResolution`
 * cannot serve it: 7 d and 30 d are both `day`, so only the window tells them
 * apart.
 */
describe('the range the words name', () => {
  const NOW = Date.UTC(2026, 7, 26, 11, 37);
  /** What the server answers: the `from` it was given, and its own now. */
  const responseFor = (key: '24h' | '7d' | '30d') => ({
    from: rangeQuery(key, NOW).from,
    to: new Date(NOW).toISOString(),
  });

  it('names the response’s window, not the chip’s, while a change is in flight', () => {
    // The 30 d response is still on screen; the chips already say 24 h. Reading
    // the chip here printed `last 24 h` over the 30 d figures for the fetch.
    expect(plottedRange(responseFor('30d'), '24h')).toBe('30d');
    expect(plottedRange(responseFor('24h'), '30d')).toBe('24h');
    // The pair `plottedResolution` cannot separate, because both are `day`.
    expect(plottedRange(responseFor('7d'), '30d')).toBe('7d');
    expect(plottedRange(responseFor('30d'), '7d')).toBe('30d');
  });

  it('agrees with the chip once the response has landed', () => {
    for (const key of ['24h', '7d', '30d'] as const) {
      expect(plottedRange(responseFor(key), key)).toBe(key);
    }
  });

  it('falls back to the chip when nothing is plotted or the window is unusable', () => {
    expect(plottedRange(null, '7d')).toBe('7d');
    expect(plottedRange({ from: 'not a date', to: 'nor this' }, '30d')).toBe(
      '30d',
    );
    // `to <= from` cannot happen — the API refuses it — but a zero span must
    // not be read as "nearest to 24 h".
    const same = new Date(NOW).toISOString();
    expect(plottedRange({ from: same, to: same }, '7d')).toBe('7d');
  });

  it('tolerates clock skew between the browser and the server', () => {
    // `to` is the server's now and `from` is the browser's, so the span carries
    // the skew. The three spans are a day, a week and a month apart; an hour
    // either way changes nothing.
    const skewed = {
      from: rangeQuery('7d', NOW).from,
      to: new Date(NOW + 60 * 60 * 1000).toISOString(),
    };
    expect(plottedRange(skewed, '24h')).toBe('7d');
  });
});
