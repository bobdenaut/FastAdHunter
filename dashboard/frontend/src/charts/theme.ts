import { useEffect, useState } from 'preact/hooks';
import { subscribeTheme } from '../theme/theme';

/**
 * uPlot paints to a canvas and cannot inherit a CSS custom property, so the
 * chart's colours are read out of the tokens once, at option-build time, and
 * the options are rebuilt when the theme changes. Nothing here hard-codes a
 * colour: this is the one place the token wall is crossed, and it crosses it by
 * reading, never by redefining.
 */
export interface ChartTheme {
  permitted: string;
  blocked: string;
  /** The categorical series, by their token names. The Performance charts pick
   *  from these; the bar chart uses `permitted`/`blocked`, which are roles
   *  rather than positions in the ramp. */
  series1: string;
  series2: string;
  series4: string;
  series5: string;
  /** The dashed budget marker. Amber because it is a target to notice, not a
   *  state to alarm at — nothing enforces it at runtime. */
  budget: string;
  grid: string;
  axis: string;
  tick: string;
  barLabel: string;
  segmentLabel: string;
  font: string;
  mono: string;
}

function token(style: CSSStyleDeclaration, name: string, fallback: string): string {
  const value = style.getPropertyValue(name).trim();
  return value === '' ? fallback : value;
}

/**
 * The fallbacks are not a second palette — they are what a non-browser test
 * environment gets, where `getComputedStyle` resolves nothing. A build that
 * loses `tokens.css` draws a legible chart instead of an invisible one.
 */
export function readChartTheme(): ChartTheme {
  const style = getComputedStyle(document.documentElement);
  return {
    permitted: token(style, '--series-permitted', '#1f9dbb'),
    blocked: token(style, '--series-blocked', '#d1504b'),
    series1: token(style, '--series-1', '#1f9dbb'),
    series2: token(style, '--series-2', '#3d9a63'),
    series4: token(style, '--series-4', '#8a95a3'),
    series5: token(style, '--series-5', '#6d5fa6'),
    budget: token(style, '--series-3', '#dd9a2f'),
    grid: token(style, '--border-row', '#eef2f6'),
    axis: token(style, '--border-control', '#cfd8e3'),
    tick: token(style, '--text-faint', '#8a95a3'),
    barLabel: token(style, '--text-control', '#47535f'),
    segmentLabel: '#fff',
    font: token(style, '--font', 'system-ui, sans-serif'),
    mono: token(style, '--font-mono', 'ui-monospace, monospace'),
  };
}

/**
 * The chart's colours, re-read when the theme changes. uPlot cannot inherit a
 * custom property, so this is the signal that rebuilds the options — and it is
 * one half of the `(range, theme)` key finding m4 requires them to be memoised
 * on.
 */
export function useChartTheme(): ChartTheme {
  const [theme, setTheme] = useState(readChartTheme);
  useEffect(() => subscribeTheme(() => setTheme(readChartTheme())), []);
  return theme;
}
