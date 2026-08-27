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
