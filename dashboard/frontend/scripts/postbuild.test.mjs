import { describe, expect, it } from 'vitest';
import {
  BUDGET_BYTES,
  DEV_GALLERY_MARKER,
  brotliOf,
  extensionOf,
  forbiddenHits,
  gzipOf,
  isOverBudget,
  isSibling,
  isLoginPathAsset,
  chartSplitViolations,
} from './postbuild.mjs';
import {
  BUDGET_BYTES as CONSTANTS_BUDGET_BYTES,
  DEV_GALLERY_MARKER as CONSTANTS_MARKER,
} from '../src/constants.ts';

describe('size gate', () => {
  it('passes at the budget and fails above it', () => {
    expect(isOverBudget(BUDGET_BYTES)).toBe(false);
    expect(isOverBudget(BUDGET_BYTES - 1)).toBe(false);
    expect(isOverBudget(BUDGET_BYTES + 1)).toBe(true);
  });

  it('is the figure the application also compiles in', () => {
    expect(BUDGET_BYTES).toBe(CONSTANTS_BUDGET_BYTES);
    expect(DEV_GALLERY_MARKER).toBe(CONSTANTS_MARKER);
  });
});

describe('sibling selection', () => {
  it('skips already-emitted siblings', () => {
    expect(isSibling('assets/index-abc.js.gz')).toBe(true);
    expect(isSibling('assets/index-abc.js.br')).toBe(true);
    expect(isSibling('assets/index-abc.js')).toBe(false);
  });

  it('reads the extension case-insensitively', () => {
    expect(extensionOf('a/b/index.HTML')).toBe('.html');
    expect(extensionOf('LICENSE')).toBe('');
  });

  it('compresses deterministically', () => {
    const raw = Buffer.from('a'.repeat(4096));
    expect(gzipOf(raw).equals(gzipOf(raw))).toBe(true);
    expect(brotliOf(raw, 'x.js').equals(brotliOf(raw, 'x.js'))).toBe(true);
  });
});

describe('forbidden content', () => {
  it('accepts a clean file', () => {
    expect(forbiddenHits('index.html', '<title>FastAdHunter</title>')).toEqual(
      [],
    );
  });

  it('rejects the dev gallery marker', () => {
    expect(
      forbiddenHits('assets/index.js', `const m="${DEV_GALLERY_MARKER}";`),
    ).toContain('dev gallery marker');
  });

  it('rejects an external reference', () => {
    expect(forbiddenHits('index.html', '<script src="https://cdn.x/a.js">'))
      .toHaveLength(1);
    expect(forbiddenHits('index.html', 'fetch("http://example.test")'))
      .toHaveLength(1);
  });

  it('permits the SVG namespace, which is an identifier and not a fetch', () => {
    expect(
      forbiddenHits(
        'assets/sprite.svg',
        '<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0"/></svg>',
      ),
    ).toEqual([]);
  });

  it('rejects a Pi-hole string in any casing or spelling', () => {
    expect(forbiddenHits('index.html', 'inspired by Pi-hole')).toHaveLength(1);
    expect(forbiddenHits('index.html', 'PIHOLE_MODE')).toHaveLength(1);
  });
});

describe('the chart chunk split', () => {
  it('knows which assets the login path fetches', () => {
    expect(isLoginPathAsset('index.html')).toBe(true);
    expect(isLoginPathAsset('assets/index-Bcvo.js')).toBe(true);
    expect(isLoginPathAsset('assets/login-AASu.js')).toBe(true);
    expect(isLoginPathAsset('assets/style-57rS.css')).toBe(true);
    expect(isLoginPathAsset('assets/uplot-Q1x2.js')).toBe(false);
    expect(isLoginPathAsset('assets/dashboard-Q1x2.js')).toBe(false);
  });

  it('fails a build that puts uPlot back on the login path', () => {
    expect(
      chartSplitViolations('assets/index-Bcvo.js', 'var uPlot=function(){}'),
    ).toHaveLength(1);
    expect(
      chartSplitViolations('assets/login-AASu.js', 'import "uplot"'),
    ).toHaveLength(1);
  });

  it('leaves the chart chunk alone — that is where uPlot belongs', () => {
    expect(
      chartSplitViolations('assets/uplot-Q1x2.js', 'var uPlot=function(){}'),
    ).toEqual([]);
  });

  it('fails a stylesheet that carries uPlot’s vendor rules again', () => {
    expect(
      chartSplitViolations(
        'assets/style-57rS.css',
        '.u-legend{font-size:14px}',
      ),
    ).toHaveLength(1);
  });

  it('accepts the hand-written `.chart` subset, which is ours', () => {
    expect(
      chartSplitViolations(
        'assets/style-57rS.css',
        '.chart .uplot{width:min-content}.chart .u-wrap{position:relative}',
      ),
    ).toEqual([]);
  });
});
