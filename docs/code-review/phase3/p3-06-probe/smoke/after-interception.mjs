// smoke/after-interception.mjs — the smoke rows p3-06-after-interception-impl.md
// §1 names after B1–B4: the three B10 migration boot paths, the B6 posture
// negatives by PUT, the B1 posture-change label, and the p2 Windows negative
// path. Dev box only, release binary, no device, no measurement.
//
//   node smoke/after-interception.mjs --out <root> [--binary <path>]
//     [--dns-port 5300 --dot-port 8853 --http-port 8080 --https-port 8444 --api-port 8443]
//
// Every row boots the binary from a fresh fixture directory under --out/work/,
// so nothing persists between rows except the report. work/ is the store —
// CA and API keys, session secret, engine logs — and is never committed
// (.gitignore `docs/code-review/**/smoke-*/work/`); the evidence beside it is
// the report, run.log and the probe scripts' own output directories. Exit 0
// when every row passes, 1 otherwise; the table is printed and written to
// <root>/after-interception.json. Runs unattended. Everything the driver
// persists — run.log, after-interception.json, the probe scripts' stdout —
// goes through redact() first (see there).

import { spawn, spawnSync } from 'node:child_process';
import dgram from 'node:dgram';
import fs from 'node:fs';
import https from 'node:https';
import os from 'node:os';
import path from 'node:path';
import tls from 'node:tls';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PROBE_DIR = path.resolve(HERE, '..');
const REPO = path.resolve(HERE, '../../../../..');

const args = { out: null, binary: null, 'dns-port': 5300, 'dot-port': 8853, 'http-port': 8080, 'https-port': 8444, 'api-port': 8443 };
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i += 1) {
  const tok = argv[i];
  if (!tok.startsWith('--')) continue;
  const key = tok.slice(2);
  const raw = argv[(i += 1)];
  args[key] = key.endsWith('-port') ? Number(raw) : raw;
}
if (!args.out) {
  console.log('usage: node smoke/after-interception.mjs --out <root> [--binary <fastadhunter>] [--*-port]');
  process.exit(64);
}
const BINARY = args.binary ?? path.join(REPO, 'target', 'release', process.platform === 'win32' ? 'fastadhunter.exe' : 'fastadhunter');
if (!fs.existsSync(BINARY)) {
  console.log(`binary not found: ${BINARY} — build it first: cargo build --release --locked -p fastadhunter`);
  process.exit(66);
}
const ROOT = path.resolve(args.out);
fs.mkdirSync(ROOT, { recursive: true });
const REPORT = [];

// The engine prints two secrets once, on first boot — the `api_key=…` and
// `dashboard_password=…` fields (main.rs, SECURITY.md §API access) — and a
// FAIL row copies an engine-log tail into its reason. Every byte the driver
// persists goes through here: the two fields are blanked by name, and every
// API key the driver has read from a fixture is blanked by value wherever else
// it might appear. The rest of the diagnostic text is untouched.
const SECRET_VALUES = new Set();
function redact(text) {
  let out = String(text).replace(/\b(api_key|dashboard_password)=\S+/g, '$1=<redacted>');
  for (const value of SECRET_VALUES) out = out.split(value).join('<redacted>');
  return out;
}
function rememberSecrets(fx) {
  const p = path.join(fx.config, 'apikey');
  if (!fs.existsSync(p)) return;
  const value = fs.readFileSync(p, 'utf8').trim();
  if (value.length >= 16) SECRET_VALUES.add(value);
}

const log = (line) => {
  const l = redact(`${new Date().toISOString()} [after-interception] ${line}`);
  process.stdout.write(`${l}\n`);
  fs.appendFileSync(path.join(ROOT, 'run.log'), `${l}\n`);
};

// ─── fixtures ──────────────────────────────────────────────────────────

const MIGRATED_LINE = 'migrated [https.interception] into interception.json';
const IGNORED_LINE = '[https.interception] ignored; interception.json is the source of truth';

// The probe's only upstream: a UDP responder answering every A query with
// 127.0.0.1, so any SNI name resolves inside `egress.allow_destinations` and
// a listed client reaches the terminate leg without the LAN resolver. Nothing
// in these rows measures DNS.
let UPSTREAM = null;
function dnsMock() {
  return new Promise((resolve) => {
    const sock = dgram.createSocket('udp4');
    sock.on('message', (msg, rinfo) => {
      if (msg.length < 12) return;
      let i = 12;
      while (i < msg.length && msg[i] !== 0) i += msg[i] + 1;
      const qend = i + 1 + 4;
      if (qend > msg.length) return;
      const header = Buffer.alloc(12);
      msg.copy(header, 0, 0, 2);
      header.writeUInt16BE(0x8180, 2);
      header.writeUInt16BE(1, 4);
      header.writeUInt16BE(msg.readUInt16BE(qend - 4) === 1 ? 1 : 0, 6);
      const question = msg.subarray(12, qend);
      const answer = Buffer.from([0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 127, 0, 0, 1]);
      const reply = header.readUInt16BE(6) === 1 ? Buffer.concat([header, question, answer]) : Buffer.concat([header, question]);
      sock.send(reply, rinfo.port, rinfo.address);
    });
    sock.bind(0, '127.0.0.1', () => {
      UPSTREAM = `127.0.0.1:${sock.address().port}`;
      resolve(sock);
    });
  });
}

function toml(mode, extra = '') {
  return `[engine]
mode = "${mode}"

[runtime]
http_runtimes = 2

[dns.listen]
address = "127.0.0.1"
port = ${args['dns-port']}
dot_port = ${args['dot-port']}

[[dns.upstreams.servers]]
address = "${UPSTREAM}"
protocol = "udp"

[http.listen]
address = "127.0.0.1"
port = ${args['http-port']}

[https.listen]
address = "127.0.0.1"
port = ${args['https-port']}

[egress]
allow_destinations = ["127.0.0.0/8"]

[api]
address = "127.0.0.1"
port = ${args['api-port']}
tls = true

[log]
level = "info"
format = "text"

${extra}
`;
}

const LEGACY = (clients, exclude = '["bank.example"]') => `[https.interception]\nclients = ${clients}\nexclude_domains = ${exclude}\n`;

function fixture(name) {
  const dir = path.join(ROOT, 'work', name);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(path.join(dir, 'config'), { recursive: true });
  fs.mkdirSync(path.join(dir, 'data'), { recursive: true });
  return { dir, config: path.join(dir, 'config'), data: path.join(dir, 'data'), log: path.join(dir, 'engine.log') };
}

// ─── the binary ────────────────────────────────────────────────────────

function api(p, { method = 'GET', body = null, key = null } = {}) {
  return new Promise((resolve) => {
    const payload = body === null ? null : JSON.stringify(body);
    const headers = {};
    if (key) headers.authorization = `Bearer ${key}`;
    if (payload !== null) { headers['content-type'] = 'application/json'; headers['content-length'] = Buffer.byteLength(payload); }
    const req = https.request({ host: '127.0.0.1', port: args['api-port'], path: p, method, headers, rejectUnauthorized: false, timeout: 10000 }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => {
        const text = Buffer.concat(chunks).toString('utf8');
        let json = null;
        try { json = JSON.parse(text); } catch { /* not JSON */ }
        resolve({ status: res.statusCode, text, json });
      });
    });
    req.on('error', (e) => resolve({ status: 0, text: String(e), json: null }));
    req.on('timeout', () => { req.destroy(); resolve({ status: 0, text: 'timeout', json: null }); });
    if (payload !== null) req.write(payload);
    req.end();
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Boots from a fixture; resolves { child, key } once /health answers, or
// { exited, log } when the binary exits first (the refusal rows want that).
// Whatever the outcome, an API key the boot left in the fixture is registered
// for redaction before any row can quote the log.
async function boot(fx, config) {
  fs.writeFileSync(path.join(fx.config, 'fastadhunter.toml'), config);
  const out = fs.openSync(fx.log, 'w');
  const child = spawn(BINARY, ['--config', path.join(fx.config, 'fastadhunter.toml'), '--data', fx.data], { stdio: ['ignore', out, out] });
  let exited = null;
  child.on('exit', (code) => { exited = code ?? -1; });
  const outcome = await (async () => {
    const deadline = Date.now() + 30000;
    while (Date.now() < deadline) {
      if (exited !== null) return { child: null, exited, log: fs.readFileSync(fx.log, 'utf8') };
      const h = await api('/health');
      if (h.status === 200) {
        const key = fs.readFileSync(path.join(fx.config, 'apikey'), 'utf8').trim();
        return { child, exited: null, key, log: () => fs.readFileSync(fx.log, 'utf8') };
      }
      await sleep(100);
    }
    child.kill();
    return { child: null, exited: 'timeout', log: fs.readFileSync(fx.log, 'utf8') };
  })();
  rememberSecrets(fx);
  return outcome;
}

// Waits for the process to be gone, not for a timer: the next fixture rebinds
// the same five ports, and a kill() that has not landed yet loses that race.
async function stop(boot) {
  if (!boot?.child || boot.child.exitCode !== null || boot.child.signalCode !== null) return;
  const exited = new Promise((resolve) => boot.child.once('exit', resolve));
  boot.child.kill();
  await exited;
}

function row(id, pass, reason) {
  const safe = redact(reason);
  REPORT.push({ id, pass, reason: safe });
  log(`${pass ? 'PASS' : 'FAIL'} ${id}: ${safe}`);
}

// A TLS origin on 127.0.0.1:443 presenting the probe's own API certificate
// (self-signed, written to the config dir on first boot). The release binary
// verifies upstreams against webpki roots only, so the terminate leg refuses
// it: `upstream_cert_failures` +1, session closed before our handshake (526,
// intercept.rs). The splice leg passes it through untouched. Port 443 is what
// the proxy dials for an SNI host — unprivileged on Windows; on Linux the
// driver needs CAP_NET_BIND_SERVICE and says so if the bind fails.
function selfSignedOrigin(fx) {
  return new Promise((resolve) => {
    const server = tls.createServer({
      cert: fs.readFileSync(path.join(fx.config, 'api-cert.pem')),
      key: fs.readFileSync(path.join(fx.config, 'api-key.pem')),
    }, (socket) => socket.end());
    server.on('error', (e) => resolve({ server: null, error: `${e.code ?? e.message}` }));
    server.listen(443, '127.0.0.1', () => resolve({ server, error: null }));
  });
}

// What the active scope does with one connection from 127.0.0.1 to :8444 for
// `name`, read from the listener counters and the socket outcome so that two
// probes compare the runtime policy, not a display string:
//   listed   → upstream_cert_failures +1, socket closed before any handshake
//   unlisted → spliced through, handshake completes against the origin's cert
async function behaviour(name, key) {
  const counters = async () => (await api('/api/v1/telemetry', { key })).json?.listeners?.https ?? null;
  const before = await counters();
  const outcome = await new Promise((resolve) => {
    let settled = false;
    const done = (v) => { if (!settled) { settled = true; resolve(v); } };
    const s = tls.connect({ host: '127.0.0.1', port: args['https-port'], servername: name, rejectUnauthorized: false, timeout: 10000 }, () => {
      const issuer = s.getPeerCertificate()?.issuer?.CN ?? null;
      s.destroy();
      done(`handshake issuer=${issuer}`);
    });
    s.on('error', (e) => done(`closed ${e.code ?? e.message}`));
    s.on('timeout', () => { s.destroy(); done('timeout'); });
    s.on('close', () => done('closed'));
  });
  await sleep(300);
  const after = await counters();
  const delta = (k) => (before?.[k] == null || after?.[k] == null ? null : after[k] - before[k]);
  return { outcome, certFailDelta: delta('upstream_cert_failures'), upstreamFailDelta: delta('upstream_failures') };
}

function runScript(script, scriptArgs, outDir) {
  const r = spawnSync(process.execPath, [path.join(PROBE_DIR, script), ...scriptArgs, '--out', outDir], { encoding: 'utf8', timeout: 120000 });
  const text = redact(`${r.stdout ?? ''}${r.stderr ?? ''}`);
  fs.writeFileSync(path.join(outDir, `${script}.stdout.txt`), text);
  return { status: r.status, text };
}

// ─── rows ──────────────────────────────────────────────────────────────

// B10 (a): legacy block, no document — migrated, TOML stripped, one info line.
async function b10a() {
  const fx = fixture('b10a-migrate');
  const b = await boot(fx, toml('dns', LEGACY('["10.0.0.5", "192.168.88.0/24"]')));
  if (!b.child) return row('B10a migration', false, `binary did not come up: exit=${b.exited}\n${b.log.slice(-800)}`);
  try {
    const doc = await api('/api/v1/interception', { key: b.key });
    const cfg = await api('/api/v1/config', { key: b.key });
    const file = fs.existsSync(path.join(fx.config, 'interception.json'));
    const tomlNow = fs.readFileSync(path.join(fx.config, 'fastadhunter.toml'), 'utf8');
    const l = b.log();
    const ok = file
      && JSON.stringify(doc.json) === JSON.stringify({ clients: ['10.0.0.5', '192.168.88.0/24'], exclude_domains: ['bank.example'] })
      && cfg.json?.https?.interception === undefined
      && !tomlNow.includes('interception')
      && l.includes(MIGRATED_LINE) && !l.includes(IGNORED_LINE);
    row('B10a migration', ok, ok ? 'document written, GET reflects it, /config omits it, TOML stripped, info line present'
      : `file=${file} doc=${doc.status}:${doc.text.slice(0, 120)} cfg.https.interception=${JSON.stringify(cfg.json?.https?.interception)} tomlHasKey=${tomlNow.includes('interception')} migratedLine=${l.includes(MIGRATED_LINE)}`);
  } finally { await stop(b); }
  return fx;
}

// B10 (b): document exists, TOML re-carries the block — ignored, warned,
// stripped again; legacy entries not validated (an invalid one does not refuse).
async function b10b(fxFromA) {
  const fx = fixture('b10b-existing-document');
  const carried = path.join(fxFromA.config, 'interception.json');
  if (!fs.existsSync(carried)) return row('B10b existing document', false, `B10a left no document to carry: ${carried}`);
  fs.copyFileSync(carried, path.join(fx.config, 'interception.json'));
  const b = await boot(fx, toml('dns', LEGACY('["10.0.0.300"]')));
  if (!b.child) return row('B10b existing document', false, `binary refused a boot it must accept: exit=${b.exited}\n${b.log.slice(-800)}`);
  try {
    const doc = await api('/api/v1/interception', { key: b.key });
    const tomlNow = fs.readFileSync(path.join(fx.config, 'fastadhunter.toml'), 'utf8');
    const l = b.log();
    const ok = JSON.stringify(doc.json) === JSON.stringify({ clients: ['10.0.0.5', '192.168.88.0/24'], exclude_domains: ['bank.example'] })
      && !tomlNow.includes('interception') && l.includes(IGNORED_LINE) && !l.includes(MIGRATED_LINE);
    row('B10b existing document', ok, ok ? 'document authoritative, warn line present, TOML stripped again, no migration line'
      : `doc=${doc.status}:${doc.text.slice(0, 120)} tomlHasKey=${tomlNow.includes('interception')} ignored=${l.includes(IGNORED_LINE)} migrated=${l.includes(MIGRATED_LINE)}`);
  } finally { await stop(b); }
}

// B10 (a, refused): invalid legacy entry, no document — boot refused naming
// list, index, entry; nothing written; TOML untouched. Over-cap likewise.
async function b10aRefused() {
  const fx = fixture('b10a-invalid-entry');
  const b = await boot(fx, toml('dns', LEGACY('["10.0.0.5", "10.0.0.300"]')));
  await stop(b);
  const expected = 'clients[1]: "10.0.0.300" is not an IP address or CIDR block';
  const l = b.child ? b.log() : b.log;
  const ok = !b.child && b.exited !== 'timeout' && l.includes(expected)
    && !fs.existsSync(path.join(fx.config, 'interception.json'))
    && fs.readFileSync(path.join(fx.config, 'fastadhunter.toml'), 'utf8').includes('[https.interception]');
  row('B10a invalid legacy entry', ok, ok ? `refused: ${expected}; no document, TOML untouched` : `exit=${b.exited} log tail:\n${l.slice(-600)}`);

  const fx2 = fixture('b10a-over-cap');
  const clients = Array.from({ length: 257 }, (_, i) => `"10.${Math.floor(i / 256)}.${i % 256}.1"`);
  const b2 = await boot(fx2, toml('dns', LEGACY(`[${clients.join(', ')}]`, '[]')));
  await stop(b2);
  const expected2 = 'clients: 257 entries exceed the cap of 256 by 1';
  const l2 = b2.child ? b2.log() : b2.log;
  const ok2 = !b2.child && b2.exited !== 'timeout' && l2.includes(expected2) && !fs.existsSync(path.join(fx2.config, 'interception.json'));
  row('B10a over-cap legacy list', ok2, ok2 ? `refused: ${expected2}` : `exit=${b2.exited} log tail:\n${l2.slice(-600)}`);
}

// B10 (c): unreadable document — boot refused naming the file, file untouched.
async function b10c() {
  const fx = fixture('b10c-unreadable-document');
  fs.writeFileSync(path.join(fx.config, 'interception.json'), '{ not json\n');
  const b = await boot(fx, toml('dns'));
  await stop(b);
  const l = b.child ? b.log() : b.log;
  const ok = !b.child && b.exited !== 'timeout' && l.includes('interception.json')
    && fs.readFileSync(path.join(fx.config, 'interception.json'), 'utf8') === '{ not json\n';
  row('B10c unreadable document', ok, ok ? 'refused naming the file; file left as found' : `exit=${b.exited} log tail:\n${l.slice(-600)}`);
}

// B5 / B6 / B1 / p2 / p3, one https-mode boot: posture by PUT with no
// restart, the p3 negative on clients=[], the p2 Windows negative path, and
// the posture-change `degraded` label from lib.mjs's interception snapshot.
async function postures() {
  const fx = fixture('postures');
  const b = await boot(fx, toml('dns+http+https'));
  if (!b.child) return row('posture boot', false, `binary did not come up: exit=${b.exited}\n${b.log.slice(-800)}`);
  try {
    const gen = await api('/api/v1/certificates/ca/generate', { method: 'POST', body: { confirm: true }, key: b.key });
    const pem = await api('/api/v1/certificates/ca/export?format=pem', { key: b.key });
    const caPath = path.join(fx.dir, 'smoke-ca.pem');
    fs.writeFileSync(caPath, pem.text);
    if (gen.status !== 200 || !pem.text.includes('BEGIN CERTIFICATE')) return row('posture CA', false, `generate=${gen.status} export=${pem.status}`);

    const empty = { clients: [], exclude_domains: [] };
    const a = await api('/api/v1/interception', { method: 'PUT', body: empty, key: b.key });
    const getA = await api('/api/v1/interception', { key: b.key });
    row('B5 posture A by PUT', a.status === 200 && JSON.stringify(getA.json) === JSON.stringify(empty) && a.json?.restart_required === undefined,
      `PUT ${a.status}, GET ${JSON.stringify(getA.json)}, restart_required absent=${a.json?.restart_required === undefined}`);

    const keyPath = path.join(fx.dir, 'apikey');
    fs.writeFileSync(keyPath, b.key);
    const common = ['--probe', '127.0.0.1', '--api-port', String(args['api-port']), '--key', keyPath, '--skip-host-checks', '--allow-busy', '--tip', 'smoke'];

    const p3Out = path.join(ROOT, 'p3-negative');
    fs.mkdirSync(p3Out, { recursive: true });
    const p3 = runScript('p3-h2stall.mjs', [...common, '--origin', 'origin.invalid', '--path', '/8mib', '--ca', caPath, '--port', String(args['https-port'])], p3Out);
    const p3Expected = 'is not listed in the Interception Document (GET /api/v1/interception, clients)';
    row('B6 p3 negative: clients=[]', p3.status === 2 && p3.text.includes(p3Expected) && fs.existsSync(path.join(p3Out, 'interception.json')),
      `exit=${p3.status} message=${p3.text.includes(p3Expected)} snapshot=${fs.existsSync(path.join(p3Out, 'interception.json'))}`);

    const listed = { clients: ['127.0.0.1'], exclude_domains: [] };
    const bPut = await api('/api/v1/interception', { method: 'PUT', body: listed, key: b.key });
    const getB = await api('/api/v1/interception', { key: b.key });
    row('B5 posture B by PUT, no restart', bPut.status === 200 && JSON.stringify(getB.json) === JSON.stringify(listed), `PUT ${bPut.status}, GET ${JSON.stringify(getB.json)}`);

    const p3b = runScript('p3-h2stall.mjs', [...common, '--origin', 'origin.invalid', '--path', '/8mib', '--ca', caPath, '--port', String(args['https-port'])], p3Out);
    const changed = 'interception document changed since this results directory was opened';
    row('B1 posture change labels the run', p3b.text.includes(changed) && !p3b.text.includes(p3Expected),
      `degraded label=${p3b.text.includes(changed)}; precondition now passes=${!p3b.text.includes(p3Expected)} (exit=${p3b.status}, later stages may INVALID on the dummy origin — not this row's concern)`);

    if (process.platform === 'win32') {
      const p2Out = path.join(ROOT, 'p2-negative');
      fs.mkdirSync(p2Out, { recursive: true });
      const p2 = runScript('p2-handshake.mjs', [...common, '--origin', 'example.com', '--ca', caPath, '--listed', '127.0.0.1', '--unlisted', '127.0.0.2', '--port', String(args['https-port']), '--dns-port', String(args['dns-port'])], p2Out);
      const p2Expected = 'identity precondition';
      row('p2 Windows negative path', p2.status === 2 && p2.text.includes(p2Expected) && fs.existsSync(path.join(p2Out, 'interception.json')) && !p2.text.includes('https.interception.clients'),
        `exit=${p2.status} identity INVALID=${p2.text.includes(p2Expected)} snapshot=${fs.existsSync(path.join(p2Out, 'interception.json'))} no dead key in output=${!p2.text.includes('https.interception.clients')}`);
    } else {
      row('p2 Windows negative path', true, `skipped on ${os.platform()} — the row exists for the Windows dev box`);
    }

    // N4 rehearsal — persistence AND runtime atomicity: the connection after
    // the rejected PUT behaves exactly as the one before it (listed → the
    // terminate leg refuses the self-signed origin, 526 path).
    const origin = await selfSignedOrigin(fx);
    if (origin.server === null) {
      row('N4 rehearsal: invalid PUT is 422 with details; document and active policy unchanged', false, `could not bind the origin on 127.0.0.1:443: ${origin.error}`);
    } else {
      try {
        const beforeBad = await behaviour('before.n4.test', b.key);
        const bad = await api('/api/v1/interception', { method: 'PUT', body: { clients: ['10.0.0.300'], exclude_domains: [] }, key: b.key });
        const after = await api('/api/v1/interception', { key: b.key });
        const afterBad = await behaviour('after.n4.test', b.key);
        const persisted = bad.status === 422 && bad.json?.error?.details?.reason === 'invalid_entry' && JSON.stringify(after.json) === JSON.stringify(listed);
        const runtime = beforeBad.certFailDelta === 1 && afterBad.certFailDelta === 1 && beforeBad.outcome === afterBad.outcome && !beforeBad.outcome.startsWith('handshake');
        row('N4 rehearsal: invalid PUT is 422 with details; document and active policy unchanged', persisted && runtime,
          `PUT ${bad.status} details=${JSON.stringify(bad.json?.error?.details)} GET after=${JSON.stringify(after.json)}; before=${JSON.stringify(beforeBad)} after=${JSON.stringify(afterBad)}`);

        // Control for the runtime read: the unlisted posture splices the same
        // origin through, so the +1 above is the listed policy and nothing else.
        await api('/api/v1/interception', { method: 'PUT', body: empty, key: b.key });
        const unlisted = await behaviour('control.n4.test', b.key);
        row('N4 control: unlisted client is spliced through, no upstream_cert_failure', unlisted.certFailDelta === 0 && unlisted.outcome.startsWith('handshake'), JSON.stringify(unlisted));
      } finally { origin.server.close(); }
    }
  } finally { await stop(b); }
}

// ─── drive ─────────────────────────────────────────────────────────────

log(`binary=${BINARY} out=${ROOT} ports dns=${args['dns-port']} dot=${args['dot-port']} http=${args['http-port']} https=${args['https-port']} api=${args['api-port']}`);
const pre = await api('/health');
if (pre.status !== 0) {
  log(`something already answers on 127.0.0.1:${args['api-port']} — stop it or pass other --*-port values`);
  process.exit(65);
}
const mock = await dnsMock();
log(`mock upstream answering 127.0.0.1 on ${UPSTREAM}`);
const fxA = await b10a();
if (fxA?.config) await b10b(fxA);
await b10aRefused();
await b10c();
await postures();

const failed = REPORT.filter((r) => !r.pass);
fs.writeFileSync(path.join(ROOT, 'after-interception.json'), redact(JSON.stringify({ binary: BINARY, started: REPORT.length, failed: failed.length, rows: REPORT }, null, 2)));
console.log('\n| row | result | reason |\n| --- | --- | --- |');
for (const r of REPORT) console.log(`| ${r.id} | ${r.pass ? 'PASS' : 'FAIL'} | ${r.reason.split('\n')[0].slice(0, 160)} |`);
console.log(`\n${failed.length === 0 ? 'ALL ROWS PASS' : `${failed.length} row(s) FAILED`} — report: ${path.join(ROOT, 'after-interception.json')}`);
mock.close();
process.exit(failed.length === 0 ? 0 : 1);
