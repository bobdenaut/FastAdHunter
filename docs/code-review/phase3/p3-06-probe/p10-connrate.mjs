// p10-connrate.mjs — P10's keep-alive arm only (plan §Load generators):
// --requests requests per connection, the last with `Connection: close`,
// plaintext to the probe's :8080 or TLS to :8444 (--tls; SNI and Host =
// --origin). oha has no requests-per-connection control, so this is the one
// HTTP shape it cannot express; ported from E:/fah-diag/tools/connrate.py so
// `connections/s` keeps its phase-2.6 meaning (165 conn/s at N=0). Every
// other HTTP arm is oha.
//
// Client discipline (§Traps): the client reads to the server's FIN before
// closing, so TIME_WAIT sits on the probe and not on the Windows client.
// --workers worker threads share --concurrency connection loops; raw sockets,
// no HTTP library; bodies read to content-length, chunked, or EOF. A response
// that is not 200 is an error. Files are chosen uniformly from --files.
//
// TLS: the leg is whatever the probe gives this host — spliced when unlisted,
// terminate when listed — and the served issuer is recorded so the JSON says
// which (`leg`). --ca <FastAdHunter CA PEM> validates the terminate leg;
// without it the certificate is not verified (the splice serves the
// origin's). The intercepted keep-alive row on the device is this script run
// on the listed host (the Mac) and its JSON copied back.
//
// Standalone (writes p10-connrate.json) or imported by p10-domains.mjs
// (runConnrate).
//
//   node p10-connrate.mjs --key <file> --origin 192-168-10-20.nip.io
//   node p10-connrate.mjs --key <file> --origin ... --tls --ca fah-ca.pem

import net from 'node:net';
import tls from 'node:tls';
import fs from 'node:fs';
import { Worker, isMainThread, parentPort, workerData } from 'node:worker_threads';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, CA_ISSUER_CN, quantile, round, peerInfo, sleep, isMain } from './lib.mjs';

const SPEC = {
  origin: { type: 'string', required: true, help: 'origin name: Host header (and SNI with --tls)' },
  port: { type: 'number', default: 8080, help: 'probe HTTP port (plaintext)' },
  'https-port': { type: 'number', default: 8444, help: 'probe HTTPS port (--tls)' },
  tls: { type: 'boolean', default: false, help: 'TLS to --https-port instead of plaintext' },
  ca: { type: 'string', default: null, help: 'CA PEM that must have issued the served certificate (the FastAdHunter CA on the terminate leg); unset = not verified' },
  files: { type: 'list', default: ['1k.bin', '10k.bin', '50k.bin'], help: 'paths under / chosen uniformly' },
  concurrency: { type: 'number', default: 48, help: 'connection loops in flight' },
  workers: { type: 'number', default: 6, help: 'worker threads sharing the loops' },
  duration: { type: 'number', default: 300, help: 'seconds' },
  requests: { type: 'number', default: 20, help: 'requests per connection, the last with Connection: close' },
  'max-fail-pct': { type: 'number', default: 2, help: 'error share above which the run is degraded' },
};

class Reader {
  constructor(sock) {
    this.sock = sock;
    this.buf = Buffer.alloc(0);
    this.eof = false;
    this.err = null;
    this.waiter = null;
    sock.on('data', (c) => {
      this.buf = this.buf.length ? Buffer.concat([this.buf, c]) : c;
      this.wake();
    });
    sock.on('end', () => {
      this.eof = true;
      this.wake();
    });
    sock.on('close', () => {
      this.eof = true;
      this.wake();
    });
    sock.on('error', (e) => {
      this.err = e;
      this.wake();
    });
  }

  wake() {
    const w = this.waiter;
    this.waiter = null;
    if (w) w();
  }

  wait() {
    return new Promise((r) => {
      this.waiter = r;
    });
  }

  async need(pred) {
    while (!pred()) {
      if (this.err) throw this.err;
      if (this.eof) throw Object.assign(new Error('closed before the response completed'), { code: 'ECLOSED' });
      await this.wait();
    }
  }

  async readResponse() {
    await this.need(() => this.buf.indexOf('\r\n\r\n') >= 0);
    const hdrEnd = this.buf.indexOf('\r\n\r\n');
    const head = this.buf.subarray(0, hdrEnd).toString('latin1');
    this.buf = this.buf.subarray(hdrEnd + 4);
    const lines = head.split('\r\n');
    const status = Number(lines[0].split(' ')[1]) || 0;
    let length = null;
    let chunked = false;
    for (const l of lines.slice(1)) {
      const i = l.indexOf(':');
      if (i < 0) continue;
      const name = l.slice(0, i).trim().toLowerCase();
      const value = l.slice(i + 1).trim();
      if (name === 'content-length') length = Number(value);
      else if (name === 'transfer-encoding' && /chunked/i.test(value)) chunked = true;
    }
    let got = 0;
    if (chunked) {
      for (;;) {
        await this.need(() => this.buf.indexOf('\r\n') >= 0);
        const le = this.buf.indexOf('\r\n');
        const size = parseInt(this.buf.subarray(0, le).toString('latin1'), 16);
        if (!Number.isFinite(size)) throw Object.assign(new Error('bad chunk size'), { code: 'EBADCHUNK' });
        await this.need(() => this.buf.length >= le + 2 + size + 2);
        this.buf = this.buf.subarray(le + 2 + size + 2);
        got += size;
        if (size === 0) break;
      }
    } else if (length !== null) {
      await this.need(() => this.buf.length >= length);
      this.buf = this.buf.subarray(length);
      got = length;
    } else {
      while (!this.eof) {
        if (this.err) throw this.err;
        await this.wait();
      }
      got = this.buf.length;
      this.buf = Buffer.alloc(0);
    }
    return { status, length: got };
  }

  async drainToEof(ms) {
    const t = setTimeout(() => {
      this.eof = true;
      this.wake();
    }, ms);
    while (!this.eof && !this.err) await this.wait();
    clearTimeout(t);
    this.buf = Buffer.alloc(0);
  }
}

function connect(cfg) {
  return new Promise((resolve, reject) => {
    const onError = (e) => reject(e);
    let sock;
    if (cfg.tls) {
      sock = tls.connect({ host: cfg.probe, port: cfg.port, servername: cfg.origin, ca: cfg.ca ?? undefined, rejectUnauthorized: Boolean(cfg.ca), ALPNProtocols: ['http/1.1'] }, () => {
        sock.removeListener('error', onError);
        resolve(sock);
      });
    } else {
      sock = net.connect({ host: cfg.probe, port: cfg.port }, () => {
        sock.removeListener('error', onError);
        resolve(sock);
      });
    }
    sock.setNoDelay(true);
    sock.setTimeout(30000, () => sock.destroy(Object.assign(new Error('socket timeout'), { code: 'ETIMEDOUT' })));
    sock.once('error', onError);
  });
}

async function oneConnection(cfg, deadline, rec) {
  const t0 = performance.now();
  const sock = await connect(cfg);
  rec.connLat.push(performance.now() - t0);
  rec.stats.conns += 1;
  if (cfg.tls && !rec.tlsInfo) rec.tlsInfo = peerInfo(sock);
  const reader = new Reader(sock);
  let sent = 0;
  let closeRequested = false;
  try {
    for (let i = 0; i < cfg.requests; i += 1) {
      if (Date.now() >= deadline) {
        rec.stats.cutByDeadline += 1;
        break;
      }
      const last = i === cfg.requests - 1;
      closeRequested = last;
      const file = cfg.files[Math.floor(Math.random() * cfg.files.length)];
      const t1 = performance.now();
      sock.write(`GET /${file} HTTP/1.1\r\nHost: ${cfg.origin}\r\nConnection: ${last ? 'close' : 'keep-alive'}\r\n\r\n`);
      const { status, length } = await reader.readResponse();
      rec.lat.push(performance.now() - t1);
      rec.stats.req += 1;
      rec.stats.bytes += length;
      rec.stats.status[status] = (rec.stats.status[status] ?? 0) + 1;
      if (status !== 200) rec.stats.err += 1;
      sent += 1;
    }
    rec.stats.perConn[sent] = (rec.stats.perConn[sent] ?? 0) + 1;
    if (!closeRequested) sock.end();
    await reader.drainToEof(5000);
  } finally {
    sock.destroy();
  }
}

async function loop(cfg, deadline, rec) {
  while (Date.now() < deadline) {
    try {
      await oneConnection(cfg, deadline, rec);
    } catch (e) {
      rec.stats.err += 1;
      const kind = e.code || e.name || 'Error';
      rec.stats.errkind[kind] = (rec.stats.errkind[kind] ?? 0) + 1;
      await sleep(10);
    }
  }
}

async function workerMain() {
  const { cfg, loops, deadline } = workerData;
  const rec = { lat: [], connLat: [], tlsInfo: null, stats: { req: 0, bytes: 0, err: 0, conns: 0, cutByDeadline: 0, status: {}, errkind: {}, perConn: {} } };
  await Promise.all(Array.from({ length: loops }, () => loop(cfg, deadline, rec)));
  parentPort.postMessage(rec);
}

function pct(values, q) {
  const s = values.filter(Number.isFinite).sort((a, b) => a - b);
  return s.length ? round(quantile(s, q)) : null;
}

export async function runConnrate(opts, log = () => {}) {
  const o = { probe: '172.17.0.4', port: 8080, tls: false, origin: null, ca: null, files: ['1k.bin', '10k.bin', '50k.bin'], concurrency: 48, workers: 6, duration: 300, requests: 20, ...opts };
  const cfg = { probe: o.probe, port: o.port, tls: o.tls, origin: o.origin, ca: o.ca ? fs.readFileSync(o.ca, 'utf8') : null, files: o.files, requests: o.requests };
  const workers = Math.max(1, Math.min(o.workers, o.concurrency));
  const deadline = Date.now() + o.duration * 1000;
  const start = performance.now();
  log(`connrate: ${o.tls ? 'tls' : 'plaintext'} ${o.probe}:${o.port} origin=${o.origin} concurrency=${o.concurrency} workers=${workers} requests/conn=${o.requests} duration=${o.duration}s`);
  const results = await Promise.all(
    Array.from({ length: workers }, (_, i) => {
      const loops = Math.floor(o.concurrency / workers) + (i < o.concurrency % workers ? 1 : 0);
      return new Promise((resolve, reject) => {
        const w = new Worker(new URL(import.meta.url), { workerData: { cfg, loops, deadline } });
        w.once('message', resolve);
        w.once('error', reject);
        w.once('exit', (code) => {
          if (code !== 0) reject(new Error(`worker exited ${code}`));
        });
      });
    }),
  );
  const wall = (performance.now() - start) / 1000;
  const merged = { req: 0, bytes: 0, err: 0, conns: 0, cutByDeadline: 0, status: {}, errkind: {}, perConn: {} };
  const lat = [];
  const connLat = [];
  let tlsInfo = null;
  for (const r of results) {
    for (const k of ['req', 'bytes', 'err', 'conns', 'cutByDeadline']) merged[k] += r.stats[k];
    for (const k of ['status', 'errkind', 'perConn']) for (const [kk, v] of Object.entries(r.stats[k])) merged[k][kk] = (merged[k][kk] ?? 0) + v;
    lat.push(...r.lat);
    connLat.push(...r.connLat);
    tlsInfo ??= r.tlsInfo;
  }
  const full = merged.perConn[String(o.requests)] ?? 0;
  const mean = lat.length ? round(lat.reduce((a, b) => a + b, 0) / lat.length) : null;
  return {
    mode: 'keepalive',
    transport: o.tls ? 'tls' : 'plaintext',
    leg: o.tls ? (tlsInfo?.issuerCN === CA_ISSUER_CN ? 'intercepted' : tlsInfo?.issuerCN ? 'spliced' : null) : 'plaintext',
    served_issuer: tlsInfo?.issuerCN ?? null,
    tls: tlsInfo ? { version: tlsInfo.tls, alpn: tlsInfo.alpn, authorized: tlsInfo.authorized } : null,
    workers,
    concurrency: o.concurrency,
    duration_s: round(wall, 1),
    requests: merged.req,
    connections: merged.conns,
    rps: round(merged.req / wall, 1),
    conns_per_s: round(merged.conns / wall, 1),
    bytes: merged.bytes,
    errors: merged.err,
    errkind: merged.errkind,
    status: merged.status,
    lat_ms: { p50: pct(lat, 0.5), p95: pct(lat, 0.95), p99: pct(lat, 0.99), mean },
    connect_ms: { p50: pct(connLat, 0.5), p95: pct(connLat, 0.95), p99: pct(connLat, 0.99) },
    requests_per_connection: { target: o.requests, histogram: merged.perConn, full_connections: full, cut_by_deadline: merged.cutByDeadline, short_connections: merged.conns - full - merged.cutByDeadline },
  };
}

async function cli() {
  const args = parseArgs(SPEC);
  const run = await new Run('p10-connrate', args, { needsHttps: args.tls }).init();
  if (run.host.idle === false) {
    if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`);
    run.degraded(`host not idle: ${run.host.busy.join(',')}`);
  }
  if (args.ca && !fs.existsSync(args.ca)) run.invalid(`--ca ${args.ca} unreadable`);
  const figures = await runConnrate(
    { probe: args.probe, port: args.tls ? args['https-port'] : args.port, tls: args.tls, origin: args.origin, ca: args.ca, files: args.files, concurrency: args.concurrency, workers: args.workers, duration: args.duration, requests: args.requests },
    (l) => run.log(l),
  );
  run.raw({ measurement: 'P10-keepalive', ...figures });
  run.log(`keepalive ${figures.transport}${figures.leg && figures.leg !== 'plaintext' ? ' ' + figures.leg : ''}: rps=${figures.rps} conn/s=${figures.conns_per_s} p50/p95/p99=${figures.lat_ms.p50}/${figures.lat_ms.p95}/${figures.lat_ms.p99} ms errors=${figures.errors} full=${figures.requests_per_connection.full_connections}/${figures.connections}${figures.served_issuer ? ' issuer=' + figures.served_issuer : ''}`);
  const attempts = figures.requests + figures.errors;
  if (attempts === 0) run.invalid('no request completed');
  if ((figures.errors / attempts) * 100 > args['max-fail-pct']) run.degraded(`${figures.errors} errors of ${attempts} attempts (budget ${args['max-fail-pct']} %): ${JSON.stringify(figures.errkind)} ${JSON.stringify(figures.status)}`);
  if (figures.requests_per_connection.short_connections > 0) run.degraded(`${figures.requests_per_connection.short_connections} connections ended before ${args.requests} requests and not by the deadline`);
  if (args.tls && args.ca && figures.leg !== 'intercepted') run.degraded(`--ca given but the served issuer is ${figures.served_issuer} — this host is not on the terminate leg`);
  run.finish({ measurement: 'P10-keepalive', figures, gate: { statistic: 'none — P10 proposes an N, the owner decides', rps: figures.rps, conns_per_s: figures.conns_per_s } });
}

if (!isMainThread) await workerMain();
else if (isMain(import.meta.url)) await cli();
