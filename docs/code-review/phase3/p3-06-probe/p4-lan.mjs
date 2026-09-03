// p4-lan.mjs — P4-LAN: DoT / DoH latency vs UDP as the LAN sees it (plan
// §Measurements row P4-LAN). Diagnostic only — the LAN hop and one handshake
// per batch are inside the figure; the PERFORMANCE.md row comes from the
// in-device harness (Dockerfile.p4).
//
// --rounds rounds; each round runs one batch per transport (udp, dot, doh),
// --queries sequential queries for the blocked --domain on ONE connection per
// batch (UDP: one socket; DoT: one TLS connection to :853 with SNI
// --dot-host, 2-byte length framing reassembled; DoH: one h2 session to the
// API listener, POST /dns-query application/dns-message). Replies matched by
// the 16-bit transaction ID; unanswered (timeout) and unmatched counted and
// reported. Per-query latency = send -> matched reply. Handshake time per
// batch recorded separately and excluded from the per-query figures.
//
//   node p4-lan.mjs --key <file> --domain ads.example.net --dot-host dns.fah.test

import tls from 'node:tls';
import http2 from 'node:http2';
import dgram from 'node:dgram';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, summary, round, dnsQuery, dnsId, dnsParse, peerInfo, sleep } from './lib.mjs';

const args = parseArgs({
  domain: { type: 'string', required: true, help: 'a name the probe blocks' },
  'dot-host': { type: 'string', required: true, help: 'SNI for the DoT connection' },
  'dns-port': { type: 'number', default: 53, help: 'UDP port' },
  'dot-port': { type: 'number', default: 853, help: 'DoT port' },
  queries: { type: 'number', default: 2000, help: 'queries per batch' },
  rounds: { type: 'number', default: 3, help: 'rounds' },
  timeout: { type: 'number', default: 2000, help: 'ms per query' },
});

const run = await new Run('p4-lan', args, { needsHttps: true }).init();
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}
const dotState = run.config?.dns?.listen?.dot_enabled;
run.log(`dns.listen.dot_enabled=${dotState} doh_enabled=${run.config?.dns?.listen?.doh_enabled}`);

function newId() {
  return Math.floor(Math.random() * 65536);
}

class Matcher {
  constructor() {
    this.pending = new Map();
    this.unmatched = 0;
  }
  expect(id) {
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        resolve({ answered: false, latency_ms: null, parsed: null });
      }, args.timeout);
      const t0 = performance.now();
      this.pending.set(id, (buf) => {
        clearTimeout(timer);
        this.pending.delete(id);
        resolve({ answered: true, latency_ms: performance.now() - t0, parsed: dnsParse(buf) });
      });
    });
  }
  deliver(buf) {
    const cb = this.pending.get(dnsId(buf));
    if (cb) cb(buf);
    else this.unmatched += 1;
  }
}

async function udpBatch() {
  const m = new Matcher();
  const sock = dgram.createSocket('udp4');
  sock.on('message', (msg) => m.deliver(msg));
  await new Promise((r) => sock.connect(args['dns-port'], args.probe, r));
  const lat = [];
  let unanswered = 0;
  let first = null;
  const t0 = performance.now();
  for (let i = 0; i < args.queries; i += 1) {
    const id = newId();
    const p = m.expect(id);
    sock.send(dnsQuery(args.domain, id));
    const r = await p;
    if (r.answered) {
      lat.push(r.latency_ms);
      if (first === null) first = r.parsed;
    } else unanswered += 1;
  }
  const wall = performance.now() - t0;
  sock.close();
  return { transport: 'udp', handshake_ms: null, latencies: lat, unanswered, unmatched: m.unmatched, wall_ms: round(wall), first_answer: first, tls: null };
}

async function dotBatch() {
  const m = new Matcher();
  let acc = Buffer.alloc(0);
  const t0 = performance.now();
  const sock = tls.connect({ host: args.probe, port: args['dot-port'], servername: args['dot-host'], rejectUnauthorized: false });
  sock.setNoDelay(true);
  const info = await new Promise((resolve, reject) => {
    sock.once('secureConnect', () => resolve(peerInfo(sock)));
    sock.once('error', reject);
  });
  const handshake = performance.now() - t0;
  sock.on('data', (c) => {
    acc = Buffer.concat([acc, c]);
    while (acc.length >= 2) {
      const len = acc.readUInt16BE(0);
      if (acc.length < 2 + len) break;
      m.deliver(acc.subarray(2, 2 + len));
      acc = acc.subarray(2 + len);
    }
  });
  sock.on('error', () => {});
  const lat = [];
  let unanswered = 0;
  let first = null;
  const t1 = performance.now();
  for (let i = 0; i < args.queries; i += 1) {
    const id = newId();
    const q = dnsQuery(args.domain, id);
    const frame = Buffer.alloc(2 + q.length);
    frame.writeUInt16BE(q.length, 0);
    q.copy(frame, 2);
    const p = m.expect(id);
    sock.write(frame);
    const r = await p;
    if (r.answered) {
      lat.push(r.latency_ms);
      if (first === null) first = r.parsed;
    } else unanswered += 1;
  }
  const wall = performance.now() - t1;
  sock.destroy();
  return { transport: 'dot', handshake_ms: round(handshake), latencies: lat, unanswered, unmatched: m.unmatched, wall_ms: round(wall), first_answer: first, tls: info };
}

async function dohBatch() {
  const t0 = performance.now();
  const session = http2.connect(`https://${args.probe}:${args['api-port']}`, { rejectUnauthorized: false });
  const info = await new Promise((resolve, reject) => {
    session.once('connect', () => resolve(peerInfo(session.socket)));
    session.once('error', reject);
  });
  const handshake = performance.now() - t0;
  const lat = [];
  let unanswered = 0;
  let unmatched = 0;
  let first = null;
  const statuses = {};
  const t1 = performance.now();
  for (let i = 0; i < args.queries; i += 1) {
    const id = newId();
    const q = dnsQuery(args.domain, id);
    const r = await new Promise((resolve) => {
      const ts = performance.now();
      const req = session.request({ ':method': 'POST', ':path': '/dns-query', 'content-type': 'application/dns-message', 'content-length': q.length });
      const chunks = [];
      let status = null;
      const timer = setTimeout(() => {
        req.close();
        resolve({ answered: false });
      }, args.timeout);
      req.on('response', (h) => {
        status = h[':status'];
      });
      req.on('data', (c) => chunks.push(c));
      req.on('end', () => {
        clearTimeout(timer);
        const body = Buffer.concat(chunks);
        resolve({ answered: status === 200, status, body, latency_ms: performance.now() - ts });
      });
      req.on('error', () => {
        clearTimeout(timer);
        resolve({ answered: false, status });
      });
      req.end(q);
    });
    if (r.status !== undefined) statuses[r.status] = (statuses[r.status] ?? 0) + 1;
    if (r.answered && dnsId(r.body) === id) {
      lat.push(r.latency_ms);
      if (first === null) first = dnsParse(r.body);
    } else if (r.answered) unmatched += 1;
    else unanswered += 1;
  }
  const wall = performance.now() - t1;
  session.close();
  return { transport: 'doh', handshake_ms: round(handshake), latencies: lat, unanswered, unmatched, wall_ms: round(wall), first_answer: first, tls: info, statuses };
}

const batches = [];
for (let r = 1; r <= args.rounds; r += 1) {
  for (const fn of [udpBatch, dotBatch, dohBatch]) {
    const b = await fn().catch((e) => ({ transport: fn.name.replace('Batch', ''), error: e.code || e.message, latencies: [], unanswered: args.queries, unmatched: 0 }));
    b.round = r;
    const s = summary(b.latencies);
    run.raw({ measurement: 'P4-LAN', round: r, transport: b.transport, ...s, unanswered: b.unanswered, unmatched: b.unmatched, wall_ms: b.wall_ms, handshake_ms: b.handshake_ms, issuer: b.tls?.issuerCN ?? null, first_answer: b.first_answer, error: b.error ?? null, statuses: b.statuses ?? null });
    run.log(`round ${r} ${b.transport.padEnd(3)} p50=${s.p50 ?? '-'} p99=${s.p99 ?? '-'} ms n=${s.n} unanswered=${b.unanswered} unmatched=${b.unmatched} wall=${b.wall_ms ?? '-'}ms hs=${b.handshake_ms ?? '-'}${b.error ? ' error=' + b.error : ''}`);
    batches.push(b);
    await sleep(500);
  }
}

const figures = {};
for (const t of ['udp', 'dot', 'doh']) {
  const bs = batches.filter((b) => b.transport === t);
  const pooled = bs.flatMap((b) => b.latencies);
  figures[t] = {
    label: 'P4-LAN (LAN hop + one handshake per batch inside; diagnostic, not the row)',
    ...summary(pooled),
    per_round_p50: bs.map((b) => summary(b.latencies).p50),
    unanswered: bs.reduce((a, b) => a + b.unanswered, 0),
    unmatched: bs.reduce((a, b) => a + b.unmatched, 0),
    batch_wall_ms_mean: round(bs.reduce((a, b) => a + (b.wall_ms ?? 0), 0) / bs.length),
    handshake_ms: bs.map((b) => b.handshake_ms),
    served_issuer: [...new Set(bs.map((b) => b.tls?.issuerCN ?? null))],
    first_answer: bs[0]?.first_answer ?? null,
  };
}
for (const t of ['dot', 'doh']) figures[t].p50_minus_udp_p50 = figures[t].p50 !== null && figures.udp.p50 !== null ? round(figures[t].p50 - figures.udp.p50) : null;
const lossy = Object.entries(figures).filter(([, f]) => f.n < args.queries * args.rounds * 0.98);
for (const [t, f] of lossy) run.degraded(`${t}: ${f.n} answered of ${args.queries * args.rounds}`);

run.finish({ measurement: 'P4-LAN', domain: args.domain, dot_host: args['dot-host'], figures, gate: { statistic: 'none — diagnostic', dot_p50_minus_udp_p50_ms: figures.dot.p50_minus_udp_p50, doh_p50_minus_udp_p50_ms: figures.doh.p50_minus_udp_p50 } });
