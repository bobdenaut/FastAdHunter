// p6-certs-time.mjs — P6 CA generate / API-pair import time (plan
// §Measurements row P6; declaration delta 7).
//
// --generate x POST /api/v1/certificates/ca/generate {confirm: true} and
// --import x POST /api/v1/certificates/import with a real pair
// (--cert / --key-file). Each call is one fresh TLS connection. Columns:
//   starttransfer - appconnect = first response byte - TLS done
//     (client-observed request-processing time excluding the TLS handshake:
//     request transit, server work, first-byte transit) — the gate column
//   time_total = end - start (handshake-inclusive diagnostic)
// Gate: median of --generate on generate < 100 ms, median of --import on
// import < 50 ms. Invalidity: every call must answer 200 (a 409 archive_full
// is INVALID, not a slow row); the archive counts are read first — the API
// does not expose them, so they come from the owner's sftp listing
// (--ca-archive-count / --api-archive-count, recorded; absent = unknown,
// degraded).
//
// Side effects, by design: each generate replaces the CA (leaf cache purged,
// every earlier export invalid) — the script re-exports the final CA PEM to
// --ca-out; each import archives the previous API pair and leaves
// api_certificate.source = "imported" with restart_required (the acceptor
// keeps the old pair until the owner restarts the probe — Runbook 7's
// import-then-restart check, owner side). Five generates plus four more reach
// the ninth for the archive-cap check (owner side, propose only).
//
//   node p6-certs-time.mjs --key <file> --cert api.crt --key-file api.key \
//     --ca-archive-count 0 --api-archive-count 0

import fs from 'node:fs';
import path from 'node:path';
import { parseArgs, Run, summary, round, median } from './lib.mjs';

const args = parseArgs({
  generate: { type: 'number', default: 5, help: 'ca/generate calls' },
  import: { type: 'number', default: 5, help: 'import calls' },
  cert: { type: 'string', default: null, help: 'PEM certificate (chain allowed) for the import arm' },
  'key-file': { type: 'string', default: null, help: 'PEM private key for --cert' },
  'ca-archive-count': { type: 'number', default: null, help: 'directories under /config/ca-archive before the run (owner sftp ls)' },
  'api-archive-count': { type: 'number', default: null, help: 'directories under /config/api-archive before the run' },
  'ca-out': { type: 'string', default: null, help: 'where to write the re-exported CA PEM (default <out>/ca-after-p6.pem)' },
});

const run = await new Run('p6', args, { needsHttps: false }).init();
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}
if (args.import > 0 && !(args.cert && args['key-file'])) run.invalid('import arm needs --cert and --key-file (a real pair on disk)');
const MAX_ARCHIVES = 8;
const before = await run.certificates();
run.log(`before: ca=${JSON.stringify(before.ca)} api_certificate=${JSON.stringify(before.api_certificate)}`);
const archivesFromGenerates = before.ca?.present ? args.generate : Math.max(0, args.generate - 1);
if (args['ca-archive-count'] === null) run.degraded('ca-archive count before the run unknown (--ca-archive-count)');
else if (args['ca-archive-count'] + archivesFromGenerates > MAX_ARCHIVES) run.invalid(`ca-archive holds ${args['ca-archive-count']}; ${args.generate} generates would add ${archivesFromGenerates} (the first on an empty store archives nothing) and pass the cap of ${MAX_ARCHIVES}`);
if (args.import > 0) {
  if (args['api-archive-count'] === null) run.degraded('api-archive count before the run unknown (--api-archive-count)');
  else if (args['api-archive-count'] + args.import > MAX_ARCHIVES) run.invalid(`api-archive holds ${args['api-archive-count']}; ${args.import} imports would pass the cap of ${MAX_ARCHIVES}`);
}

function columns(r) {
  const t = r.timings;
  return {
    status: r.status,
    starttransfer_minus_appconnect_ms: t.secureConnect !== null && t.firstByte !== null ? round(t.firstByte - t.secureConnect) : null,
    time_total_ms: round(t.end - t.start),
    appconnect_ms: t.secureConnect !== null ? round(t.secureConnect - t.start) : null,
  };
}

async function arm(name, count, call) {
  const rows = [];
  for (let i = 1; i <= count; i += 1) {
    const r = await call();
    const row = { op: name, index: i, ...columns(r), response: r.json ?? r.text.slice(0, 200) };
    rows.push(row);
    run.raw({ measurement: 'P6', ...row });
    run.log(`${name} #${i}: ${row.status} starttransfer-appconnect=${row.starttransfer_minus_appconnect_ms} ms total=${row.time_total_ms} ms`);
    if (r.status !== 200) run.invalid(`${name} #${i} answered ${r.status}: ${r.text.slice(0, 200)}`, { rows });
  }
  return rows;
}

const generateRows = await arm('ca/generate', args.generate, () => run.api('/api/v1/certificates/ca/generate', { method: 'POST', body: { confirm: true } }));
let importRows = [];
if (args.import > 0) {
  const cert_pem = fs.readFileSync(args.cert, 'utf8');
  const key_pem = fs.readFileSync(args['key-file'], 'utf8');
  importRows = await arm('import', args.import, () => run.api('/api/v1/certificates/import', { method: 'POST', body: { format: 'pem', cert_pem, key_pem } }));
}

const after = await run.certificates();
run.log(`after: ca=${JSON.stringify(after.ca)} api_certificate=${JSON.stringify(after.api_certificate)}`);
const caOut = args['ca-out'] ?? path.join(run.out, 'ca-after-p6.pem');
if (args.generate > 0) {
  const pem = await run.api('/api/v1/certificates/ca/export?format=pem');
  if (pem.status !== 200) run.invalid(`re-export after generate answered ${pem.status}`);
  fs.writeFileSync(caOut, pem.body);
  run.log(`re-exported CA -> ${caOut} (fingerprint ${after.ca?.fingerprint_sha256}); every earlier export is invalid`);
}
if (args.import > 0 && after.api_certificate?.source !== 'imported') run.invalid(`api_certificate.source is ${after.api_certificate?.source} after import`);

const gen = summary(generateRows.map((r) => r.starttransfer_minus_appconnect_ms));
const imp = summary(importRows.map((r) => r.starttransfer_minus_appconnect_ms));
const genMed = median(generateRows.map((r) => r.starttransfer_minus_appconnect_ms));
const impMed = median(importRows.map((r) => r.starttransfer_minus_appconnect_ms));

run.finish({
  measurement: 'P6',
  archive_count_before: { ca: args['ca-archive-count'], api: args['api-archive-count'] },
  figures: {
    'ca/generate': { ...gen, median_time_total_ms: median(generateRows.map((r) => r.time_total_ms)) },
    import: { ...imp, median_time_total_ms: median(importRows.map((r) => r.time_total_ms)) },
  },
  ca_after: after.ca,
  api_certificate_after: after.api_certificate,
  ca_exported_to: args.generate > 0 ? caOut : null,
  rows: [...generateRows, ...importRows],
  gate: {
    statistic: 'median of starttransfer - appconnect: generate < 100 ms, import < 50 ms',
    generate_median_ms: round(genMed),
    import_median_ms: round(impMed),
    generate_pass: genMed !== null ? genMed < 100 : null,
    import_pass: impMed !== null ? impMed < 50 : null,
    pass: (genMed === null || genMed < 100) && (impMed === null || impMed < 50) && (genMed !== null || impMed !== null),
  },
});
