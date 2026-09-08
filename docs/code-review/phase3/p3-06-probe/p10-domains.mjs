// p10-domains.mjs — P10, one N per invocation (plan §P10 rig, §Load
// generators). Sequences the arms, drives oha where the plan says so,
// p10-connrate.mjs for the keep-alive arm and p10-dnsload.mjs for the DNS
// half of the mixed arms, samples /api/v1/debug/memory every
// --sample-interval s, and computes cores ((Δcpu_user_ms + Δcpu_system_ms) ÷
// wall), CPU ms per request, ΔRSS against the arm-local floor (the arm's
// first `before` sample — levels are never compared across arms), the 502
// count, and the tail sample. The owner restarts the probe at the intended N
// before this runs; the script reads runtime.http_runtimes back from /config
// and refuses to run when it differs from --n (the fah-env trap: four
// identical rows reported as a plateau).
//
// Arms (--arm, comma list; default all — the shutdown arm is owner-run from
// the router and has no script):
//   close            oha --disable-keepalive -c --concurrency -z --duration,
//                    Host: --origin, files from --files, plaintext via :8080
//   keepalive        p10-connrate.mjs, --ka-requests per connection,
//                    plaintext then TLS (--driver oha is refused)
//   mixed            close + p10-dnsload.mjs at --dns-qps, same window
//   tls-spliced      oha, a new TLS connection per request, --connect-to the
//                    probe's :8444: the allowed name (--origin), then the
//                    blocked name (--blocked). This host must be UNLISTED
//   tls-intercepted  the same allowed-name run from a LISTED host: locally
//                    (smoke posture B) or on the Mac over ssh
//                    (--intercepted-ssh user@mac, --intercepted-ca <path
//                    there>). The arm split is by host, never by flag
//   mixed-tls        p10-dnsload.mjs + tls-spliced + tls-intercepted together
//   transfers        oha through the splice: --single-n × --single-path on one
//                    connection, --par-n × --par-path on --par-c; the direct
//                    (P1-control) pass beside it; cores per pass, ΔRSS step and
//                    the +--transfers-tail-min sample. sizePerSec is a
//                    cross-check figure, not P1's row
// Every TLS arm takes a served-issuer sample before and after (Node tls — the
// same observation as `openssl s_client`; over ssh for the intercepted host)
// and must match the leg it claims; --skip-issuer-sample turns the arm into
// a diagnostic. oha is pinned at 1.16.0 and --worker-threads must be given.
//
// The origin (--origin, a publicly resolvable name the probe allows) serves
// /1k.bin /10k.bin /50k.bin /10mb.bin /100mb.bin over plaintext on
// --origin-http-port and TLS on --origin-https-port — a real server
// (static-web-server, as in phase 2.6), never a toy one (§Traps); the owner
// records its local throughput before the arm.
//
//   node p10-domains.mjs --key <file> --n 2 --origin 192-168-10-20.nip.io \
//     --blocked ads.example.net --ca fah-ca.pem --worker-threads 8 \
//     --arm close,keepalive,mixed

import fs from 'node:fs';
import tls from 'node:tls';
import { spawn, execFileSync } from 'node:child_process';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, CA_ISSUER_CN, MiB, round, peerInfo, sleep } from './lib.mjs';
import { runConnrate } from './p10-connrate.mjs';
import { DnsLoad } from './p10-dnsload.mjs';

const OHA_VERSION = '1.16.0';
const ALL_ARMS = ['close', 'keepalive', 'mixed', 'tls-spliced', 'tls-intercepted', 'mixed-tls', 'transfers'];
const TLS_ARMS = new Set(['keepalive', 'tls-spliced', 'tls-intercepted', 'mixed-tls', 'transfers']);

const args = parseArgs({
  n: { type: 'number', required: true, help: 'the http_runtimes the probe was restarted at; must equal /config' },
  arm: { type: 'list', default: ALL_ARMS, help: `comma list of ${ALL_ARMS.join(',')}` },
  origin: { type: 'string', required: true, help: 'origin name: Host header, SNI, the allowed name' },
  'origin-http-port': { type: 'number', default: 80, help: 'origin plaintext port' },
  'origin-https-port': { type: 'number', default: 443, help: 'origin TLS port' },
  port: { type: 'number', default: 8080, help: 'probe HTTP port' },
  'https-port': { type: 'number', default: 8444, help: 'probe HTTPS port' },
  files: { type: 'list', default: ['1k.bin', '10k.bin', '50k.bin'], help: 'paths under / for the rate arms' },
  blocked: { type: 'string', default: null, help: 'a name the probe blocks at SNI (tls arms)' },
  ca: { type: 'string', default: null, help: 'FastAdHunter CA PEM (the intercepted arm, local; the TLS keep-alive half)' },
  'intercepted-ssh': { type: 'string', default: null, help: 'user@host that runs the intercepted arm (a listed client); unset = this host' },
  'intercepted-ca': { type: 'string', default: null, help: 'CA PEM path on the ssh host (default --ca)' },
  'intercepted-oha': { type: 'string', default: 'oha', help: 'oha binary on the ssh host' },
  concurrency: { type: 'number', default: 48, help: 'oha -c / connrate loops' },
  duration: { type: 'number', default: 300, help: 'seconds per rate arm' },
  'worker-threads': { type: 'number', default: null, help: 'oha --worker-threads; must be set explicitly (the default is the physical core count)' },
  'ka-requests': { type: 'number', default: 20, help: 'keep-alive requests per connection' },
  'connrate-workers': { type: 'number', default: 6, help: 'p10-connrate worker threads' },
  'dns-qps': { type: 'number', default: 300, help: 'DNS load in the mixed arms' },
  'dns-port': { type: 'number', default: 53, help: 'probe UDP DNS port' },
  'single-path': { type: 'string', default: '/100mb.bin', help: 'transfers: single-connection object' },
  'single-n': { type: 'number', default: 5, help: 'transfers: single-connection fetches' },
  'par-path': { type: 'string', default: '/10mb.bin', help: 'transfers: parallel object' },
  'par-n': { type: 'number', default: 40, help: 'transfers: parallel fetches in total' },
  'par-c': { type: 'number', default: 8, help: 'transfers: parallel connections' },
  'sample-interval': { type: 'number', default: 2, help: 'seconds between /debug/memory samples' },
  'tail-min': { type: 'number', default: 3, help: 'minutes after a rate arm before its tail sample (0 = none)' },
  'transfers-tail-min': { type: 'number', default: 15, help: 'minutes after the transfers arm before its tail sample (0 = none)' },
  driver: { type: 'string', default: 'auto', help: 'checked for the keep-alive arm only: oha is refused' },
  'skip-issuer-sample': { type: 'boolean', default: false, help: 'skip the served-issuer samples; every TLS arm becomes a diagnostic' },
  oha: { type: 'string', default: 'oha', help: 'oha binary' },
  'max-fail-pct': { type: 'number', default: 2, help: 'failure share above which an arm is degraded' },
});

const arms = args.arm;
const run = await new Run('p10-domains', args, { needsHttps: arms.some((a) => TLS_ARMS.has(a)), resultName: `p10-N${args.n}.json` }).init();

const unknown = arms.filter((a) => !ALL_ARMS.includes(a));
if (unknown.length) run.invalid(`unknown arm ${unknown.join(',')}; choose from ${ALL_ARMS.join(',')}`);
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}
if (run.httpRuntimes === null) run.invalid('could not read runtime.http_runtimes from /api/v1/config');
if (run.httpRuntimes !== args.n) run.invalid(`probe reports http_runtimes=${run.httpRuntimes}, arm declared ${args.n} — restart the probe at the intended N (fahprobe-env) before this arm`);
if (args['worker-threads'] === null) run.invalid('--worker-threads must be set explicitly — oha defaults to the physical core count (24 on bobdenaut) and the client competes with the reading');
if (arms.includes('keepalive') && args.driver === 'oha') run.invalid('keepalive is driven by p10-connrate.mjs; oha has no requests-per-connection control (plan §Load generators)');
if (arms.some((a) => a.startsWith('tls-') || a === 'mixed-tls') && !args.blocked) run.invalid('--blocked <name> is required for the TLS connection-rate arms');
if (arms.some((a) => a === 'tls-intercepted' || a === 'mixed-tls') && !args['intercepted-ssh'] && !args.ca) run.invalid('--ca <FastAdHunter CA PEM> is required for the intercepted arm on this host');
if (args.ca && !fs.existsSync(args.ca)) run.invalid(`--ca ${args.ca} unreadable`);

function spawnCapture(bin, argv) {
  return new Promise((resolve) => {
    const child = spawn(bin, argv, { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    const out = [];
    const err = [];
    child.stdout.on('data', (c) => out.push(c));
    child.stderr.on('data', (c) => err.push(c));
    child.on('error', (e) => resolve({ code: null, stdout: '', stderr: e.message }));
    child.on('close', (code) => resolve({ code, stdout: Buffer.concat(out).toString('utf8'), stderr: Buffer.concat(err).toString('utf8') }));
  });
}

function shellQuote(s) {
  return `'${String(s).replace(/'/g, `'\\''`)}'`;
}

async function ohaVersion(bin, ssh = null) {
  const r = ssh ? await spawnCapture('ssh', [ssh, bin, '--version']) : await spawnCapture(bin, ['--version']);
  const m = r.stdout.match(/oha\s+(\S+)/);
  return m ? m[1] : null;
}

const ohaLocal = await ohaVersion(args.oha);
if (ohaLocal !== OHA_VERSION) run.invalid(`oha ${OHA_VERSION} required, found ${ohaLocal ?? 'nothing (' + args.oha + ' not runnable)'}`);
run.log(`oha ${ohaLocal} on this host; --worker-threads ${args['worker-threads']}`);
let ohaRemote = null;
if (args['intercepted-ssh'] && arms.some((a) => a === 'tls-intercepted' || a === 'mixed-tls')) {
  ohaRemote = await ohaVersion(args['intercepted-oha'], args['intercepted-ssh']);
  if (ohaRemote !== OHA_VERSION) run.invalid(`oha ${OHA_VERSION} required on ${args['intercepted-ssh']}, found ${ohaRemote ?? 'nothing'} — a version split puts the two TLS rate arms on two different clients`);
  run.log(`oha ${ohaRemote} on ${args['intercepted-ssh']}`);
}

class Sampler {
  constructor() {
    this.samples = [];
    this.timer = null;
    this.busy = false;
  }

  async take(label, arm) {
    const m = await run.api('/api/v1/debug/memory');
    const j = m.status === 200 && m.json ? m.json : {};
    const s = { t: new Date().toISOString(), arm, label, status: m.status, process_rss: j.process_rss ?? null, cpu_user_ms: j.cpu_user_ms ?? null, cpu_system_ms: j.cpu_system_ms ?? null };
    this.samples.push(s);
    run.raw({ sample: s });
    return s;
  }

  start(arm) {
    this.timer = setInterval(() => {
      if (this.busy) return;
      this.busy = true;
      this.take('during', arm).catch(() => {}).finally(() => {
        this.busy = false;
      });
    }, args['sample-interval'] * 1000);
  }

  stop() {
    clearInterval(this.timer);
    this.timer = null;
  }

  during(arm) {
    return this.samples.filter((s) => s.arm === arm && s.label === 'during');
  }
}

const sampler = new Sampler();
let cpuWarned = false;

function cpuOf(s) {
  return s.cpu_user_ms === null || s.cpu_system_ms === null ? null : s.cpu_user_ms + s.cpu_system_ms;
}

function metrics(arm, floor, before, end, wallS, requests) {
  const c0 = cpuOf(before);
  const c1 = cpuOf(end);
  const dcpu = c0 === null || c1 === null ? null : c1 - c0;
  if (dcpu === null && !cpuWarned) {
    cpuWarned = true;
    run.degraded('cpu_user_ms / cpu_system_ms are null on this probe (the kernel reading exists in-container only) — no cores figure');
  }
  const rssDuring = sampler.during(arm).map((s) => s.process_rss).filter(Number.isFinite);
  const d = (v) => (Number.isFinite(v) && Number.isFinite(floor.process_rss) ? round((v - floor.process_rss) / MiB, 2) : null);
  return {
    wall_s: round(wallS, 1),
    cpu_ms: dcpu,
    cores: dcpu === null ? null : round(dcpu / 1000 / wallS, 2),
    cpu_ms_per_request: dcpu === null || !requests ? null : round(dcpu / requests, 3),
    rss_floor_mib: Number.isFinite(floor.process_rss) ? round(floor.process_rss / MiB, 2) : null,
    rss_end_mib: Number.isFinite(end.process_rss) ? round(end.process_rss / MiB, 2) : null,
    delta_rss_end_mib: d(end.process_rss),
    delta_rss_max_mib: rssDuring.length ? d(Math.max(...rssDuring)) : null,
    samples_during: rssDuring.length,
  };
}

function diffCounters(a, b) {
  const out = {};
  for (const [k, v] of Object.entries(b ?? {})) if (typeof v === 'number' && typeof a?.[k] === 'number') out[k] = v - a[k];
  return out;
}

async function counters() {
  const t = await run.api('/api/v1/telemetry');
  return t.status === 200 && t.json ? { http: t.json.listeners?.http ?? null, https: t.json.listeners?.https ?? null } : { http: null, https: null };
}

async function leafCache() {
  const c = await run.api('/api/v1/certificates');
  return c.status === 200 && c.json ? c.json.leaf_cache ?? null : null;
}

function ohaFigures(j) {
  const ms = (v) => (v === null || v === undefined ? null : round(v * 1000, 3));
  const codes = j.statusCodeDistribution ?? {};
  const errors = j.errorDistribution ?? {};
  const requests = Object.values(codes).reduce((a, b) => a + b, 0);
  const failed = Object.values(errors).reduce((a, b) => a + b, 0);
  const non2xx = Object.entries(codes).filter(([k]) => !k.startsWith('2')).reduce((a, [, v]) => a + v, 0);
  return {
    requests,
    failed,
    non_2xx: non2xx,
    count_502: codes['502'] ?? 0,
    rps: round(j.summary?.requestsPerSec, 1),
    attempts_per_s: j.summary?.total ? round((requests + failed) / j.summary.total, 1) : null,
    success_rate: j.summary?.successRate ?? null,
    duration_s: round(j.summary?.total, 2),
    bytes: j.summary?.totalData ?? null,
    size_per_sec_mib: Number.isFinite(j.summary?.sizePerSec) ? round(j.summary.sizePerSec / MiB, 2) : null,
    latency_ms: { p50: ms(j.latencyPercentiles?.p50), p95: ms(j.latencyPercentiles?.p95), p99: ms(j.latencyPercentiles?.p99), mean: ms(j.summary?.average), max: ms(j.summary?.slowest) },
    first_byte_ms: { p50: ms(j.firstBytePercentiles?.p50), p95: ms(j.firstBytePercentiles?.p95), p99: ms(j.firstBytePercentiles?.p99) },
    rps_percentiles: { p50: round(j.rps?.percentiles?.p50, 1), p95: round(j.rps?.percentiles?.p95, 1), p99: round(j.rps?.percentiles?.p99, 1) },
    dns_dialup_ms: { average: ms(j.details?.DNSDialup?.average), fastest: ms(j.details?.DNSDialup?.fastest), slowest: ms(j.details?.DNSDialup?.slowest) },
    status: codes,
    error_distribution: errors,
  };
}

async function oha(label, argv, { ssh = null } = {}) {
  const full = ['--no-tui', '--output-format', 'json', '--worker-threads', String(args['worker-threads']), ...argv];
  const bin = ssh ? 'ssh' : args.oha;
  const spawnArgv = ssh ? [ssh, args['intercepted-oha'], ...full.map(shellQuote)] : full;
  run.log(`${label}: ${bin} ${spawnArgv.join(' ')}`);
  const t0 = performance.now();
  const r = await spawnCapture(bin, spawnArgv);
  const wallS = (performance.now() - t0) / 1000;
  const file = run.resultPath(`oha-${label}.json`);
  fs.writeFileSync(file, r.stdout || '');
  let json = null;
  try {
    json = JSON.parse(r.stdout);
  } catch {
    json = null;
  }
  if (!json) {
    run.log(`${label}: oha produced no JSON (exit ${r.code}): ${r.stderr.slice(0, 300)}`);
    return { ok: false, error: `oha exit ${r.code}: ${r.stderr.slice(0, 300)}`, wall_s: round(wallS, 1), file };
  }
  const f = ohaFigures(json);
  run.raw({ oha: label, ...f });
  run.log(`${label}: requests=${f.requests} failed=${f.failed} rps=${f.rps} p50/p95/p99=${f.latency_ms.p50}/${f.latency_ms.p95}/${f.latency_ms.p99} ms 502=${f.count_502} MiB/s=${f.size_per_sec_mib}${f.failed ? ' errors=' + JSON.stringify(f.error_distribution).slice(0, 200) : ''}`);
  return { ok: true, ...f, wall_s: round(wallS, 1), file };
}

function budget(label, f, { expectFailures = false } = {}) {
  if (!f.ok) {
    run.degraded(`${label}: ${f.error}`);
    return;
  }
  const attempts = f.requests + f.failed;
  if (attempts === 0) {
    run.degraded(`${label}: no request was attempted`);
    return;
  }
  if (expectFailures) return;
  const share = ((f.failed + f.non_2xx) / attempts) * 100;
  if (share > args['max-fail-pct']) run.degraded(`${label}: ${round(share, 2)} % of ${attempts} attempts failed or non-2xx (budget ${args['max-fail-pct']} %; ${f.count_502} × 502 — a toy origin refuses upstream connects)`);
}

function timeWait() {
  if (process.platform !== 'win32') return null;
  try {
    const n = execFileSync('netstat', ['-an'], { encoding: 'utf8', windowsHide: true }).split(/\r?\n/).filter((l) => l.includes('TIME_WAIT')).length;
    run.log(`TIME_WAIT sockets on this host: ${n}`);
    if (n > 8000) run.degraded(`${n} TIME_WAIT sockets on this host — the client closed first somewhere; Windows port exhaustion caps the loop near 130 conn/s (§Traps)`);
    return n;
  } catch (e) {
    run.log(`netstat failed: ${e.message}`);
    return null;
  }
}

function issuerLocal(servername) {
  return new Promise((resolve) => {
    const s = tls.connect({ host: args.probe, port: args['https-port'], servername, rejectUnauthorized: false, ALPNProtocols: ['http/1.1'] }, () => {
      const p = peerInfo(s);
      s.end();
      resolve({ issuerCN: p.issuerCN, subjectCN: p.subjectCN, tls: p.tls, host: 'local' });
    });
    s.setTimeout(10000, () => {
      s.destroy();
      resolve({ issuerCN: null, error: 'timeout', host: 'local' });
    });
    s.on('error', (e) => resolve({ issuerCN: null, error: e.code || e.message, host: 'local' }));
  });
}

async function issuerRemote(servername) {
  const script = `const s=require("tls").connect({host:"${args.probe}",port:${args['https-port']},servername:"${servername}",rejectUnauthorized:false},()=>{const c=s.getPeerCertificate();console.log(JSON.stringify({issuerCN:c.issuer&&c.issuer.CN||null,subjectCN:c.subject&&c.subject.CN||null,tls:s.getProtocol()}));s.end();});s.setTimeout(10000,()=>{s.destroy();console.log(JSON.stringify({issuerCN:null,error:"timeout"}));});s.on("error",(e)=>console.log(JSON.stringify({issuerCN:null,error:e.code||e.message})));`;
  const r = await spawnCapture('ssh', [args['intercepted-ssh'], 'node', '-e', shellQuote(script)]);
  try {
    return { ...JSON.parse(r.stdout.trim().split('\n').pop()), host: args['intercepted-ssh'] };
  } catch {
    return { issuerCN: null, error: `ssh: ${r.stderr.slice(0, 200)}`, host: args['intercepted-ssh'] };
  }
}

async function issuerSample(when, arm, servername, expectedLeg, ssh = null) {
  if (args['skip-issuer-sample']) {
    run.log(`${arm}: issuer sample ${when} skipped (--skip-issuer-sample) — the arm is a diagnostic`);
    return { skipped: true };
  }
  const p = ssh ? await issuerRemote(servername) : await issuerLocal(servername);
  const leg = p.issuerCN === CA_ISSUER_CN ? 'intercepted' : p.issuerCN ? 'spliced' : null;
  run.log(`${arm}: issuer sample ${when} from ${p.host}: issuer=${p.issuerCN ?? 'none'} subject=${p.subjectCN ?? '-'} ${p.tls ?? ''} → leg=${leg ?? 'unknown'} expected=${expectedLeg}${p.error ? ' error=' + p.error : ''}`);
  return { ...p, leg, expected: expectedLeg, matches: leg === expectedLeg };
}

function legStatus(arm, before, after) {
  if (before.skipped || after?.skipped) return { status: 'diagnostic', reason: 'no issuer sample — path proof is certificate-based (plan §Load generators)' };
  if (!before.matches || (after && !after.matches)) return { status: 'INVALID', reason: `served issuer says ${before.leg ?? 'unknown'}/${after?.leg ?? 'unknown'}, the arm claims ${before.expected}` };
  return { status: 'valid', reason: null };
}

async function armStart(arm) {
  run.log(`== arm ${arm} start`);
  const before = await sampler.take('before', arm);
  const tel = await counters();
  const leaf = TLS_ARMS.has(arm) ? await leafCache() : null;
  sampler.start(arm);
  return { arm, floor: before, before, tel, leaf, t0: performance.now() };
}

async function armEnd(ctx, requests) {
  sampler.stop();
  const wallS = (performance.now() - ctx.t0) / 1000;
  const end = await sampler.take('end', ctx.arm);
  const tel = await counters();
  const leaf = ctx.leaf ? await leafCache() : null;
  const m = metrics(ctx.arm, ctx.floor, ctx.before, end, wallS, requests);
  run.log(`${ctx.arm}: wall=${m.wall_s}s cores=${m.cores ?? '-'} ms/req=${m.cpu_ms_per_request ?? '-'} ΔRSS end=${m.delta_rss_end_mib ?? '-'} max=${m.delta_rss_max_mib ?? '-'} MiB (floor ${m.rss_floor_mib ?? '-'} MiB)`);
  return {
    metrics: m,
    telemetry_delta: { http: diffCounters(ctx.tel.http, tel.http), https: diffCounters(ctx.tel.https, tel.https) },
    leaf_cache_delta: leaf ? diffCounters(ctx.leaf, leaf) : null,
    time_wait: timeWait(),
  };
}

async function tail(arm, floor, minutes) {
  if (!minutes) return null;
  run.log(`${arm}: waiting ${minutes} min for the tail sample`);
  await sleep(minutes * 60 * 1000);
  const s = await sampler.take(`plus${minutes}`, arm);
  const delta = Number.isFinite(s.process_rss) && Number.isFinite(floor.process_rss) ? round((s.process_rss - floor.process_rss) / MiB, 2) : null;
  run.log(`${arm}: +${minutes} min ΔRSS=${delta ?? '-'} MiB against the arm floor`);
  return { minutes, process_rss: s.process_rss, delta_rss_mib: delta };
}

function closeArgs() {
  const files = args.files.map((f) => f.replace(/\./g, '[.]')).join('|');
  return ['-z', `${args.duration}s`, '-c', String(args.concurrency), '--disable-keepalive', '-H', `Host: ${args.origin}`, '--rand-regex-url', `http://${args.probe}:${args.port}/(${files})`];
}

function tlsRateArgs(name, { ca = null } = {}) {
  const p = args['origin-https-port'];
  const trust = ca ? ['--cacert', ca] : ['--insecure'];
  return ['-z', `${args.duration}s`, '-c', String(args.concurrency), '--disable-keepalive', '--connect-to', `${name}:${p}:${args.probe}:${args['https-port']}`, ...trust, `https://${name}:${p}/${args.files[0]}`];
}

function transferArgs(pathName, n, c, { spliced }) {
  const p = args['origin-https-port'];
  const via = spliced ? ['--connect-to', `${args.origin}:${p}:${args.probe}:${args['https-port']}`] : [];
  return ['-n', String(n), '-c', String(c), '--disable-keepalive', ...via, '--insecure', `https://${args.origin}:${p}${pathName}`];
}

async function dnsHalf(label) {
  const d = new DnsLoad({ host: args.probe, port: args['dns-port'], qps: args['dns-qps'], duration: args.duration }, (l) => run.log(`${label}-dns: ${l}`));
  await d.open();
  await d.warm();
  return d;
}

function dnsBudget(label, r) {
  run.raw({ dns: label, ...r });
  run.log(`${label}-dns: sent=${r.sent}/${r.planned} qps=${r.qps} p50/p95/p99=${r.lat_ms.p50}/${r.lat_ms.p95}/${r.lat_ms.p99} ms timeouts=${r.timeouts} unmatched=${r.unmatched} blocked→0.0.0.0=${r.by_class_ms.blocked.answered_0000}/${r.by_class_ms.blocked.n}`);
  if (r.sent < r.planned * 0.95) run.degraded(`${label}-dns: sent ${r.sent} of ${r.planned}`);
  if (r.timeouts + r.errors > r.sent * 0.02) run.degraded(`${label}-dns: ${r.timeouts} timeouts + ${r.errors} errors of ${r.sent}`);
}

async function armClose(withDns) {
  const arm = withDns ? 'mixed' : 'close';
  const dns = withDns ? await dnsHalf(arm) : null;
  const ctx = await armStart(arm);
  const [http, dnsResult] = await Promise.all([oha(`${arm}-http`, closeArgs()), dns ? dns.load().finally(() => dns.close()) : null]);
  const fin = await armEnd(ctx, http.requests ?? 0);
  budget(`${arm}-http`, http);
  if (dnsResult) dnsBudget(arm, dnsResult);
  const t = await tail(arm, ctx.floor, args['tail-min']);
  return { arm, status: 'valid', driver: 'oha', http, dns: dnsResult, ...fin, tail: t };
}

async function armKeepalive() {
  const out = {};
  for (const transport of ['plaintext', 'tls']) {
    const arm = transport === 'tls' ? 'keepalive-tls' : 'keepalive';
    const expected = transport === 'tls' ? (args.ca ? 'intercepted' : 'spliced') : null;
    const issuerBefore = transport === 'tls' ? await issuerSample('before', arm, args.origin, expected) : null;
    const ctx = await armStart(arm);
    let figures;
    try {
      figures = await runConnrate(
        { probe: args.probe, port: transport === 'tls' ? args['https-port'] : args.port, tls: transport === 'tls', origin: args.origin, ca: transport === 'tls' ? args.ca : null, files: args.files, concurrency: args.concurrency, workers: args['connrate-workers'], duration: args.duration, requests: args['ka-requests'] },
        (l) => run.log(`${arm}: ${l}`),
      );
    } catch (e) {
      sampler.stop();
      run.degraded(`${arm}: ${e.message}`);
      out[arm] = { arm, status: 'INVALID', reason: e.message };
      continue;
    }
    const fin = await armEnd(ctx, figures.requests);
    const issuerAfter = transport === 'tls' ? await issuerSample('after', arm, args.origin, expected) : null;
    run.raw({ connrate: arm, ...figures });
    run.log(`${arm}: rps=${figures.rps} conn/s=${figures.conns_per_s} p50/p95/p99=${figures.lat_ms.p50}/${figures.lat_ms.p95}/${figures.lat_ms.p99} ms errors=${figures.errors} full=${figures.requests_per_connection.full_connections}/${figures.connections}${figures.leg && figures.leg !== 'plaintext' ? ' leg=' + figures.leg : ''}`);
    const attempts = figures.requests + figures.errors;
    if (attempts && (figures.errors / attempts) * 100 > args['max-fail-pct']) run.degraded(`${arm}: ${figures.errors} errors of ${attempts} (${JSON.stringify(figures.errkind)})`);
    if (figures.requests_per_connection.short_connections > 0) run.degraded(`${arm}: ${figures.requests_per_connection.short_connections} connections ended short of ${args['ka-requests']} requests`);
    const leg = transport === 'tls' ? legStatus(arm, issuerBefore, issuerAfter) : { status: 'valid', reason: null };
    if (leg.status === 'INVALID') run.degraded(`${arm}: ${leg.reason}`);
    const t = await tail(arm, ctx.floor, args['tail-min']);
    out[arm] = { arm, status: leg.status, reason: leg.reason, driver: 'p10-connrate.mjs', figures, issuer: { before: issuerBefore, after: issuerAfter }, ...fin, tail: t };
  }
  return out;
}

async function tlsRun(arm, name, expectedLeg, { ssh = null, ca = null, expectFailures = false } = {}) {
  const issuerBefore = await issuerSample('before', arm, name, expectedLeg, ssh);
  const ctx = await armStart(arm);
  const http = await oha(arm, tlsRateArgs(name, { ca }), { ssh });
  const fin = await armEnd(ctx, http.requests ?? 0);
  const issuerAfter = expectFailures ? null : await issuerSample('after', arm, name, expectedLeg, ssh);
  budget(arm, http, { expectFailures });
  const leg = expectFailures ? { status: 'valid', reason: null } : legStatus(arm, issuerBefore, issuerAfter);
  if (leg.status === 'INVALID') run.degraded(`${arm}: ${leg.reason}`);
  if (expectFailures) {
    // Failures the client never got onto the wire cannot appear in `blocked`:
    // connections oha abandons at its own deadline, and local socket
    // allocation failures (Windows os error 10048 once the ephemeral range is
    // exhausted — the §Traps case this host hits at ~15k TIME_WAIT). Counting
    // them fails the arm on a handful of sockets out of hundreds of thousands.
    const CLIENT_SIDE = [/deadline/i, /os error 10048/i, /usage of each socket address/i];
    const unsent = Object.entries(http.error_distribution ?? {})
      .filter(([kind]) => CLIENT_SIDE.some((re) => re.test(kind)))
      .reduce((a, [, n]) => a + n, 0);
    const attempts = (http.requests ?? 0) + (http.failed ?? 0) - unsent;
    const blocked = fin.telemetry_delta.https?.blocked ?? null;
    if (blocked !== null && blocked < attempts) {
      leg.status = 'INVALID';
      leg.reason = `listeners.https.blocked moved by ${blocked} for ${attempts} attempts that reached the listener (${unsent} never left the client) — closes were not SNI verdicts`;
      run.degraded(`${arm}: ${leg.reason}`);
    }
    if (http.requests > 0) {
      leg.status = 'INVALID';
      leg.reason = `${http.requests} requests completed against the blocked name`;
      run.degraded(`${arm}: ${leg.reason}`);
    }
  }
  const t = await tail(arm, ctx.floor, args['tail-min']);
  return { arm, status: leg.status, reason: leg.reason, driver: ssh ? `oha via ssh ${ssh}` : 'oha', name, http, issuer: { before: issuerBefore, after: issuerAfter }, ...fin, tail: t };
}

async function armTlsSpliced() {
  return {
    'tls-spliced': await tlsRun('tls-spliced', args.origin, 'spliced'),
    'tls-spliced-blocked': await tlsRun('tls-spliced-blocked', args.blocked, 'spliced', { expectFailures: true }),
  };
}

function interceptedOpts() {
  const ssh = args['intercepted-ssh'];
  return { ssh, ca: ssh ? args['intercepted-ca'] ?? args.ca : args.ca };
}

async function armTlsIntercepted() {
  return { 'tls-intercepted': await tlsRun('tls-intercepted', args.origin, 'intercepted', interceptedOpts()) };
}

async function armMixedTls() {
  const arm = 'mixed-tls';
  const io = interceptedOpts();
  const iss = {
    spliced: await issuerSample('before', `${arm}-spliced`, args.origin, 'spliced'),
    intercepted: await issuerSample('before', `${arm}-intercepted`, args.origin, 'intercepted', io.ssh),
  };
  const dns = await dnsHalf(arm);
  const ctx = await armStart(arm);
  const [spliced, intercepted, dnsResult] = await Promise.all([
    oha(`${arm}-spliced`, tlsRateArgs(args.origin)),
    oha(`${arm}-intercepted`, tlsRateArgs(args.origin, { ca: io.ca }), { ssh: io.ssh }),
    dns.load().finally(() => dns.close()),
  ]);
  const fin = await armEnd(ctx, (spliced.requests ?? 0) + (intercepted.requests ?? 0));
  const issAfter = {
    spliced: await issuerSample('after', `${arm}-spliced`, args.origin, 'spliced'),
    intercepted: await issuerSample('after', `${arm}-intercepted`, args.origin, 'intercepted', io.ssh),
  };
  budget(`${arm}-spliced`, spliced);
  budget(`${arm}-intercepted`, intercepted);
  dnsBudget(arm, dnsResult);
  const legs = { spliced: legStatus(`${arm}-spliced`, iss.spliced, issAfter.spliced), intercepted: legStatus(`${arm}-intercepted`, iss.intercepted, issAfter.intercepted) };
  const bad = Object.values(legs).find((l) => l.status === 'INVALID');
  const diag = Object.values(legs).find((l) => l.status === 'diagnostic');
  if (bad) run.degraded(`${arm}: ${bad.reason}`);
  const t = await tail(arm, ctx.floor, args['tail-min']);
  return { arm, status: bad ? 'INVALID' : diag ? 'diagnostic' : 'valid', reason: bad?.reason ?? diag?.reason ?? null, driver: 'oha + oha + p10-dnsload.mjs', spliced, intercepted, dns: dnsResult, issuer: { before: iss, after: issAfter }, ...fin, tail: t };
}

async function armTransfers() {
  const arm = 'transfers';
  const issuerBefore = await issuerSample('before', arm, args.origin, 'spliced');
  const ctx = await armStart(arm);
  const single = await oha(`${arm}-single-spliced`, transferArgs(args['single-path'], args['single-n'], 1, { spliced: true }));
  const par = await oha(`${arm}-par-spliced`, transferArgs(args['par-path'], args['par-n'], args['par-c'], { spliced: true }));
  const fin = await armEnd(ctx, (single.requests ?? 0) + (par.requests ?? 0));
  const issuerAfter = await issuerSample('after', arm, args.origin, 'spliced');
  const controlSingle = await oha(`${arm}-single-control`, transferArgs(args['single-path'], args['single-n'], 1, { spliced: false }));
  const controlPar = await oha(`${arm}-par-control`, transferArgs(args['par-path'], args['par-n'], args['par-c'], { spliced: false }));
  for (const [l, f] of [[`${arm}-single-spliced`, single], [`${arm}-par-spliced`, par], [`${arm}-single-control`, controlSingle], [`${arm}-par-control`, controlPar]]) budget(l, f);
  const leg = legStatus(arm, issuerBefore, issuerAfter);
  if (leg.status === 'INVALID') run.degraded(`${arm}: ${leg.reason}`);
  const bytesMib = ((single.bytes ?? 0) + (par.bytes ?? 0)) / MiB;
  const ratio = (a, b) => (Number.isFinite(a?.size_per_sec_mib) && Number.isFinite(b?.size_per_sec_mib) && b.size_per_sec_mib ? round(a.size_per_sec_mib / b.size_per_sec_mib) : null);
  const t = await tail(arm, ctx.floor, args['transfers-tail-min']);
  return {
    arm,
    status: leg.status,
    reason: leg.reason,
    driver: 'oha',
    note: 'sizePerSec includes connect, handshake and teardown — a cross-check figure, not the P1 row (plan §Load generators)',
    spliced: { single, parallel: par },
    control: { single: controlSingle, parallel: controlPar },
    spliced_over_control: { single: ratio(single, controlSingle), parallel: ratio(par, controlPar) },
    relayed_mib: round(bytesMib, 1),
    cores_per_900_mib: fin.metrics.cpu_ms === null || !bytesMib ? null : round((fin.metrics.cpu_ms / 1000) * (900 / bytesMib), 2),
    issuer: { before: issuerBefore, after: issuerAfter },
    ...fin,
    tail: t,
  };
}

const results = {};
for (const arm of arms) {
  let r;
  if (arm === 'close') r = { close: await armClose(false) };
  else if (arm === 'mixed') r = { mixed: await armClose(true) };
  else if (arm === 'keepalive') r = await armKeepalive();
  else if (arm === 'tls-spliced') r = await armTlsSpliced();
  else if (arm === 'tls-intercepted') r = await armTlsIntercepted();
  else if (arm === 'mixed-tls') r = { 'mixed-tls': await armMixedTls() };
  else if (arm === 'transfers') r = { transfers: await armTransfers() };
  Object.assign(results, r);
  await sleep(2000);
}

const perArm = {};
for (const [k, v] of Object.entries(results)) {
  const h = v.http ?? v.figures ?? v.spliced?.single ?? null;
  perArm[k] = { status: v.status, rps: h?.rps ?? null, conns_per_s: v.figures?.conns_per_s ?? null, p50_ms: h?.latency_ms?.p50 ?? h?.lat_ms?.p50 ?? null, p95_ms: h?.latency_ms?.p95 ?? h?.lat_ms?.p95 ?? null, p99_ms: h?.latency_ms?.p99 ?? h?.lat_ms?.p99 ?? null, cores: v.metrics?.cores ?? null, cpu_ms_per_request: v.metrics?.cpu_ms_per_request ?? null, count_502: h?.count_502 ?? null, delta_rss_end_mib: v.metrics?.delta_rss_end_mib ?? null, dns_p99_ms: v.dns?.lat_ms?.p99 ?? null };
}

run.finish({
  measurement: 'P10',
  n: args.n,
  http_runtimes: run.httpRuntimes,
  oha_version: ohaLocal,
  oha_version_remote: ohaRemote,
  worker_threads: args['worker-threads'],
  concurrency: args.concurrency,
  duration_s: args.duration,
  origin: args.origin,
  blocked: args.blocked,
  intercepted_host: args['intercepted-ssh'] ?? 'local',
  arms: results,
  samples: sampler.samples,
  gate: { statistic: 'none — P10 proposes an N against the phase-2.6 criterion; the owner decides', n: args.n, per_arm: perArm },
});
