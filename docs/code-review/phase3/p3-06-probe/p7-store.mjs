// p7-store.mjs — Runbook 7, script side (plan §Measurements "Runbook 7
// split"): no key material over the API.
//
// --ca-key is a MANDATORY precondition: a local copy of the probe's
// /config/ca-key.pem, taken by the owner over sftp (read-only). Missing,
// unreadable, or not parseable as a private key => INVALID: without the key
// the payload search cannot be performed and a marker-only search proves
// nothing. Nothing here writes to the probe.
//
// Checks (each pass / fail; gate = every check pass):
//   1. GET /api/v1/certificates/ca/export?format=pem  200, application/x-pem-file,
//      only CERTIFICATE blocks, no PRIVATE KEY marker, no key payload
//   2. GET /api/v1/certificates/ca/export?format=der  200, application/pkix-cert,
//      DER sequence, no key payload
//   3. GET /api/v1/config                              200, no key payload
//   4. the security suite's 20 static / traversal paths against the API
//      listener, each with and without the bearer: no response carries the
//      PRIVATE KEY marker or the key payload. Status is recorded, never
//      asserted — the dashboard is a single-page app whose catch-all answers
//      200 text/html (index.html) for any unmatched route, /config/ca-key.pem
//      included; the security suite asserts needles, not status (F20)
// "Key payload" = the key's base64 body (searched in the response text with
// and without whitespace) and its DER bytes (searched in the raw body).
//
//   node p7-store.mjs --key <apikey-file> --ca-key ./ca-key.pem

import fs from 'node:fs';
import crypto from 'node:crypto';
import { parseArgs, Run } from './lib.mjs';

const args = parseArgs({
  'ca-key': { type: 'string', required: true, help: 'local copy of the probe /config/ca-key.pem (owner sftp, read-only)' },
});

const run = await new Run('p7-store', args, { needsHttps: false, resultName: 'certs.json' }).init();
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}

let keyPem;
try {
  keyPem = fs.readFileSync(args['ca-key'], 'utf8');
} catch (e) {
  run.invalid(`--ca-key unreadable: ${e.message}`);
}
let keyObject;
try {
  keyObject = crypto.createPrivateKey(keyPem);
} catch (e) {
  run.invalid(`--ca-key is not a private key: ${e.message}`);
}
const b64 = keyPem.replace(/-----[A-Z ]+-----/g, '').replace(/\s+/g, '');
const der = Buffer.from(b64, 'base64');
const derPkcs8 = keyObject.export({ format: 'der', type: 'pkcs8' });
run.log(`ca key: ${keyObject.asymmetricKeyType} ${keyObject.asymmetricKeyDetails?.namedCurve ?? ''}, payload ${b64.length} base64 chars / ${der.length} DER bytes`);
if (der.length < 32) run.invalid('--ca-key payload too short to be a key');

const PATHS = [
  '/',
  '/index.html',
  '/assets/',
  '/assets/nope.js',
  '/config/ca-key.pem',
  '/ca-key.pem',
  '/../config/ca-key.pem',
  '/..%2fconfig%2fca-key.pem',
  '/%2e%2e/config/ca-key.pem',
  '/assets/../../config/ca-key.pem',
  '/assets/..%2f..%2fconfig%2fca-key.pem',
  '/assets/..%5c..%5cconfig%5cca-key.pem',
  '/assets/..\\..\\config\\ca-key.pem',
  '/web/../config/ca-key.pem',
  '/api/v1/../../config/ca-key.pem',
  '/api/v1/certificates/ca/../../../../config/ca-key.pem',
  '/config/api-key.pem',
  '/config/fastadhunter.toml',
  '/config/apikey',
  '/data/session-secret',
];
const SENSITIVE = (p) => p.includes('..') || p.startsWith('/config') || p.startsWith('/data') || p === '/ca-key.pem';
const spaShell = (r) => r.status === 200 && (r.headers['content-type'] ?? '').startsWith('text/html');

function leaks(body) {
  const text = body.toString('latin1');
  const compact = text.replace(/\s+/g, '');
  const found = [];
  if (/PRIVATE KEY/.test(text)) found.push('PRIVATE KEY marker');
  if (b64.length >= 32 && (text.includes(b64) || compact.includes(b64))) found.push('key base64 payload');
  if (body.includes(der)) found.push('key DER bytes');
  if (!derPkcs8.equals(der) && body.includes(derPkcs8)) found.push('key PKCS#8 DER bytes');
  return found;
}

const checks = [];
function record(name, pass, detail) {
  checks.push({ check: name, pass, detail });
  run.raw({ measurement: 'P7-store', check: name, pass, detail });
  run.log(`${pass ? 'PASS' : 'FAIL'} ${name} ${JSON.stringify(detail)}`);
}

const pem = await run.api('/api/v1/certificates/ca/export?format=pem');
const pemText = pem.body.toString('utf8');
const blocks = [...pemText.matchAll(/-----BEGIN ([A-Z ]+)-----/g)].map((m) => m[1]);
record('ca/export?format=pem', pem.status === 200 && (pem.headers['content-type'] ?? '').startsWith('application/x-pem-file') && blocks.length > 0 && blocks.every((b) => b === 'CERTIFICATE') && leaks(pem.body).length === 0, {
  status: pem.status,
  content_type: pem.headers['content-type'],
  blocks,
  leaks: leaks(pem.body),
});

const derExport = await run.api('/api/v1/certificates/ca/export?format=der');
record('ca/export?format=der', derExport.status === 200 && (derExport.headers['content-type'] ?? '').startsWith('application/pkix-cert') && derExport.body[0] === 0x30 && leaks(derExport.body).length === 0, {
  status: derExport.status,
  content_type: derExport.headers['content-type'],
  bytes: derExport.body.length,
  first_byte: derExport.body[0],
  leaks: leaks(derExport.body),
});

const config = await run.api('/api/v1/config');
record('/api/v1/config', config.status === 200 && leaks(config.body).length === 0, { status: config.status, leaks: leaks(config.body) });

const traversal = [];
let traversalPass = true;
for (const p of PATHS) {
  for (const auth of [true, false]) {
    const r = await run.api(p, { auth }).catch((e) => ({ status: 0, body: Buffer.alloc(0), headers: {}, text: e.message }));
    const l = leaks(r.body);
    const row = { path: p, auth, status: r.status, content_type: r.headers['content-type'] ?? null, bytes: r.body.length, leaks: l, served: spaShell(r) ? 'spa_shell' : r.status === 200 ? 'other_200' : 'rejected' };
    if (SENSITIVE(p) && row.served === 'other_200') run.log(`note: ${p} answered 200 with ${row.content_type} (${row.bytes} bytes) — not the SPA shell; leaks=${JSON.stringify(l)}`);
    const ok = l.length === 0;
    if (!ok) traversalPass = false;
    traversal.push({ ...row, pass: ok });
    run.raw({ measurement: 'P7-store', check: 'traversal', ...row, pass: ok });
  }
}
record('traversal list against the API listener', traversalPass, {
  paths: PATHS.length,
  requests: traversal.length,
  served: Object.fromEntries(['spa_shell', 'other_200', 'rejected'].map((k) => [k, traversal.filter((t) => t.served === k).length])),
  failing: traversal.filter((t) => !t.pass),
});

run.finish({
  measurement: 'P7-store',
  ca_key: { type: keyObject.asymmetricKeyType, curve: keyObject.asymmetricKeyDetails?.namedCurve ?? null, der_bytes: der.length },
  checks,
  traversal,
  gate: { statistic: 'every check pass', pass: checks.every((c) => c.pass), failing: checks.filter((c) => !c.pass).map((c) => c.check) },
});
