import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  brotliCompressSync,
  constants as zlibConstants,
  gzipSync,
} from 'node:zlib';

export const BUDGET_BYTES = 150 * 1024;

// The dev-only gallery route carries this literal. A build that emits it has
// shipped a route that opens a socket and renders fixtures — tree-shaking is
// not trusted to remove it, this grep is.
export const DEV_GALLERY_MARKER = '__fah_dev_gallery__';

const TEXT_EXTENSIONS = new Set(['.html', '.js', '.css', '.svg', '.json']);

// `xmlns` on an inline or standalone SVG is a namespace identifier, not a
// fetch. Nothing resolves it, and there is no way to author SVG without it.
// Preact's `createElement` carries the XHTML and MathML ones for the same
// reason, so the list is the W3C namespace set rather than SVG alone.
const NAMESPACE_URIS = [
  'http://www.w3.org/2000/svg',
  'http://www.w3.org/1999/xhtml',
  'http://www.w3.org/1998/Math/MathML',
  'http://www.w3.org/1999/xlink',
  'http://www.w3.org/XML/1998/namespace',
];

export function extensionOf(name) {
  const dot = name.lastIndexOf('.');
  return dot === -1 ? '' : name.slice(dot).toLowerCase();
}

export function isSibling(name) {
  return name.endsWith('.gz') || name.endsWith('.br');
}

export function isOverBudget(gzipTotal, budget = BUDGET_BYTES) {
  return gzipTotal > budget;
}

export function gzipOf(raw) {
  return gzipSync(raw, { level: 9 });
}

export function brotliOf(raw, name) {
  const text = TEXT_EXTENSIONS.has(extensionOf(name));
  return brotliCompressSync(raw, {
    params: {
      [zlibConstants.BROTLI_PARAM_QUALITY]: 11,
      [zlibConstants.BROTLI_PARAM_MODE]: text
        ? zlibConstants.BROTLI_MODE_TEXT
        : zlibConstants.BROTLI_MODE_GENERIC,
      [zlibConstants.BROTLI_PARAM_SIZE_HINT]: raw.length,
    },
  });
}

/**
 * Returns the forbidden-content rules a file's text violates, empty when clean.
 * Text is scanned as authored: the emitted chunk is what ships, so a marker
 * that survived minification is found here whatever produced it.
 */
export function forbiddenHits(name, text) {
  const hits = [];
  if (text.includes(DEV_GALLERY_MARKER)) {
    hits.push('dev gallery marker');
  }
  let stripped = text;
  for (const uri of NAMESPACE_URIS) {
    stripped = stripped.split(uri).join('');
  }
  if (/https?:\/\//i.test(stripped)) {
    hits.push('external URL — no CDN, no external reference');
  }
  if (/pi-?hole/i.test(text)) {
    hits.push('Pi-hole string');
  }
  return hits;
}

function walk(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(full, out);
    } else if (entry.isFile()) {
      out.push(full);
    }
  }
  return out;
}

function pad(value, width) {
  const s = String(value);
  return s.length >= width ? s : ' '.repeat(width - s.length) + s;
}

function run(distDir) {
  const files = walk(distDir)
    .filter((full) => !isSibling(full))
    .sort();

  if (files.length === 0) {
    console.error(`postbuild: no files under ${distDir}`);
    return 1;
  }

  let rawTotal = 0;
  let gzipTotal = 0;
  let brotliTotal = 0;
  const rows = [];
  const violations = [];

  for (const full of files) {
    const name = relative(distDir, full).split(sep).join('/');
    const raw = readFileSync(full);
    const gz = gzipOf(raw);
    const br = brotliOf(raw, name);

    // `ServeDir` picks a sibling whenever one exists, so a sibling larger than
    // its source would make the served response bigger than the file.
    if (gz.length < raw.length) writeFileSync(`${full}.gz`, gz);
    if (br.length < raw.length) writeFileSync(`${full}.br`, br);

    rawTotal += raw.length;
    gzipTotal += gz.length;
    brotliTotal += br.length;
    rows.push([name, raw.length, gz.length, br.length]);

    if (TEXT_EXTENSIONS.has(extensionOf(name))) {
      for (const hit of forbiddenHits(name, raw.toString('utf8'))) {
        violations.push(`${name}: ${hit}`);
      }
    }
  }

  const width = Math.max(4, ...rows.map((r) => r[0].length));
  console.log('');
  console.log(
    `${'file'.padEnd(width)}  ${pad('raw', 9)}  ${pad('gzip', 9)}  ${pad('brotli', 9)}`,
  );
  for (const [name, raw, gz, br] of rows) {
    console.log(
      `${name.padEnd(width)}  ${pad(raw, 9)}  ${pad(gz, 9)}  ${pad(br, 9)}`,
    );
  }
  console.log(
    `${'TOTAL'.padEnd(width)}  ${pad(rawTotal, 9)}  ${pad(gzipTotal, 9)}  ${pad(brotliTotal, 9)}`,
  );
  console.log('');
  console.log(
    `budget ${BUDGET_BYTES} B gzip — used ${gzipTotal} B (${((gzipTotal / BUDGET_BYTES) * 100).toFixed(1)} %), brotli ${brotliTotal} B`,
  );

  if (violations.length > 0) {
    console.error('');
    for (const violation of violations) {
      console.error(`postbuild: forbidden content — ${violation}`);
    }
    return 1;
  }

  if (isOverBudget(gzipTotal)) {
    console.error('');
    console.error(
      `postbuild: over budget by ${gzipTotal - BUDGET_BYTES} B gzip`,
    );
    return 1;
  }

  return 0;
}

const invokedDirectly =
  process.argv[1] !== undefined &&
  fileURLToPath(import.meta.url) === process.argv[1];

if (invokedDirectly) {
  const distDir = join(fileURLToPath(new URL('..', import.meta.url)), 'dist');
  process.exit(run(distDir));
}
