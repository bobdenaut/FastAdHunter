// p10-dnsload.mjs — the DNS half of P10's mixed arms (plan §P10 rig): UDP
// queries to the probe at --qps with the phase-2.6 mix — 50 % cached (a fixed
// set of names, warmed first), 20 % blocked (ad hosts the probe's lists
// block), 30 % uncached (a random label under --uncached-zone, forwarded
// every time). Ported in full from E:/fah-diag/tools/dnsload.py so the
// phase-2.6 table stays comparable; oha is HTTP-only and has no equivalent.
//
// One UDP socket; replies matched by the 16-bit transaction ID (sequential
// ids, never reused while outstanding); unanswered (timeout) and unmatched
// counted and reported. Per-class p50 / p99, pooled p50 / p95 / p99, rcodes,
// and the share of blocked answers carrying 0.0.0.0 — a blocked name that
// does not answer 0.0.0.0 is not blocked by this probe's lists.
//
// Standalone (writes p10-dnsload.json) or imported by p10-domains.mjs
// (DnsLoad / runDnsLoad), which starts it beside the HTTP half so the two
// windows coincide.
//
//   node p10-dnsload.mjs --key <file> --qps 300 --duration 300

import dgram from 'node:dgram';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, quantile, round, dnsQuery, dnsId, dnsParse, sleep, isMain } from './lib.mjs';

export const CACHED = [
  'example.com', 'cloudflare.com', 'google.com', 'wikipedia.org', 'github.com',
  'microsoft.com', 'apple.com', 'amazon.com', 'mozilla.org', 'debian.org',
  'kernel.org', 'rust-lang.org', 'python.org', 'archive.org', 'bbc.co.uk',
  'reddit.com', 'stackoverflow.com', 'netflix.com', 'spotify.com', 'duckduckgo.com',
];
export const BLOCKED = [
  'ads.doubleclick.net', 'pagead2.googlesyndication.com', 'googleadservices.com',
  'adservice.google.com', 'ad.doubleclick.net',
];
export const MIX = { cached: 0.5, blocked: 0.2, uncached: 0.3 };

const SPEC = {
  'dns-port': { type: 'number', default: 53, help: 'UDP port on the probe' },
  qps: { type: 'number', default: 300, help: 'target queries per second' },
  duration: { type: 'number', default: 300, help: 'seconds' },
  timeout: { type: 'number', default: 2000, help: 'ms before a query counts as unanswered' },
  cached: { type: 'list', default: CACHED, help: 'names for the cached share (warmed first)' },
  blocked: { type: 'list', default: BLOCKED, help: 'names the probe blocks' },
  'uncached-zone': { type: 'string', default: 'example.com', help: 'zone the uncached share invents labels under' },
  'no-warm': { type: 'boolean', default: false, help: 'skip warming the cached and blocked names' },
};

function pct(values, q) {
  const s = values.filter(Number.isFinite).sort((a, b) => a - b);
  return s.length ? round(quantile(s, q)) : null;
}

export class DnsLoad {
  constructor(opts, log = () => {}) {
    this.o = { host: '172.17.0.4', port: 53, qps: 300, duration: 300, timeout: 2000, cached: CACHED, blocked: BLOCKED, uncachedZone: 'example.com', ...opts };
    this.log = log;
    this.pending = new Map();
    this.nextId = Math.floor(Math.random() * 65536);
    this.unmatched = 0;
    this.warmed = false;
  }

  async open() {
    this.sock = dgram.createSocket('udp4');
    this.sock.on('message', (msg) => {
      const cb = this.pending.get(dnsId(msg));
      if (cb) cb(msg);
      else this.unmatched += 1;
    });
    await new Promise((resolve, reject) => {
      this.sock.once('error', reject);
      this.sock.bind(0, () => {
        this.sock.removeListener('error', reject);
        resolve();
      });
    });
    return this;
  }

  ask(name, cls) {
    return new Promise((resolve) => {
      let id;
      do {
        id = this.nextId;
        this.nextId = (this.nextId + 1) & 0xffff;
      } while (this.pending.has(id));
      const t0 = performance.now();
      const timer = setTimeout(() => {
        this.pending.delete(id);
        resolve({ cls, name, answered: false, latency_ms: null, rcode: null, answers: null });
      }, this.o.timeout);
      this.pending.set(id, (buf) => {
        clearTimeout(timer);
        this.pending.delete(id);
        const p = dnsParse(buf);
        resolve({ cls, name, answered: true, latency_ms: performance.now() - t0, rcode: p.rcode, answers: p.answers });
      });
      this.sock.send(dnsQuery(name, id), this.o.port, this.o.host, (err) => {
        if (!err) return;
        clearTimeout(timer);
        this.pending.delete(id);
        resolve({ cls, name, answered: false, error: err.code || err.message, latency_ms: null, rcode: null, answers: null });
      });
    });
  }

  async warm() {
    let ok = 0;
    for (const name of [...this.o.cached, ...this.o.blocked]) {
      const r = await this.ask(name, 'warm');
      if (r.answered) ok += 1;
    }
    this.warmed = true;
    this.log(`warm: ${ok}/${this.o.cached.length + this.o.blocked.length} names answered`);
    return ok;
  }

  pick() {
    const r = Math.random();
    if (r < MIX.cached) return { cls: 'cached', name: this.o.cached[Math.floor(Math.random() * this.o.cached.length)] };
    if (r < MIX.cached + MIX.blocked) return { cls: 'blocked', name: this.o.blocked[Math.floor(Math.random() * this.o.blocked.length)] };
    return { cls: 'uncached', name: `u${Math.floor(Math.random() * 1e9)}.${this.o.uncachedZone}` };
  }

  async load() {
    const lat = { cached: [], blocked: [], uncached: [] };
    const stats = { sent: 0, answered: 0, timeouts: 0, errors: 0, rcode: {}, blocked_answered_0000: 0, blocked_answered: 0 };
    let outstanding = 0;
    const record = (r) => {
      outstanding -= 1;
      if (r.error) {
        stats.errors += 1;
        return;
      }
      if (!r.answered) {
        stats.timeouts += 1;
        return;
      }
      stats.answered += 1;
      stats.rcode[r.rcode] = (stats.rcode[r.rcode] ?? 0) + 1;
      lat[r.cls].push(r.latency_ms);
      if (r.cls === 'blocked') {
        stats.blocked_answered += 1;
        if (r.answers?.includes('0.0.0.0')) stats.blocked_answered_0000 += 1;
      }
    };
    const interval = 1000 / this.o.qps;
    const start = performance.now();
    const deadline = start + this.o.duration * 1000;
    let next = start;
    let lastLog = start;
    let lastSent = 0;
    for (;;) {
      const now = performance.now();
      if (now >= deadline) break;
      if (now < next) {
        await sleep(Math.min(next - now, 5));
        continue;
      }
      next += interval;
      const q = this.pick();
      stats.sent += 1;
      outstanding += 1;
      this.ask(q.name, q.cls).then(record);
      if (now - lastLog >= 10000) {
        this.log(`t=${Math.round((now - start) / 1000)}s sent=${stats.sent} qps=${round((stats.sent - lastSent) / ((now - lastLog) / 1000), 0)} timeouts=${stats.timeouts} unmatched=${this.unmatched}`);
        lastLog = now;
        lastSent = stats.sent;
      }
    }
    const sendWall = (performance.now() - start) / 1000;
    const waitUntil = performance.now() + this.o.timeout + 500;
    while (outstanding > 0 && performance.now() < waitUntil) await sleep(20);
    const pooled = [...lat.cached, ...lat.blocked, ...lat.uncached];
    const byClass = {};
    for (const [cls, v] of Object.entries(lat)) byClass[cls] = { n: v.length, p50: pct(v, 0.5), p99: pct(v, 0.99) };
    byClass.blocked.answered_0000 = stats.blocked_answered_0000;
    byClass.blocked.answered_0000_share = stats.blocked_answered ? round(stats.blocked_answered_0000 / stats.blocked_answered) : null;
    return {
      qps_target: this.o.qps,
      qps: round(stats.sent / sendWall, 1),
      duration_s: round(sendWall, 1),
      planned: Math.round(this.o.qps * this.o.duration),
      sent: stats.sent,
      answered: stats.answered,
      timeouts: stats.timeouts,
      unmatched: this.unmatched,
      errors: stats.errors,
      still_outstanding: outstanding,
      rcode: stats.rcode,
      lat_ms: { p50: pct(pooled, 0.5), p95: pct(pooled, 0.95), p99: pct(pooled, 0.99) },
      by_class_ms: byClass,
      mix: MIX,
      warmed: this.warmed,
    };
  }

  close() {
    for (const [, cb] of this.pending) cb(Buffer.alloc(0));
    this.sock?.close();
  }
}

export async function runDnsLoad(opts, log) {
  const d = new DnsLoad(opts, log);
  await d.open();
  if (opts.warm !== false) await d.warm();
  try {
    return await d.load();
  } finally {
    d.close();
  }
}

async function cli() {
  const args = parseArgs(SPEC);
  const run = await new Run('p10-dnsload', args).init();
  if (run.host.idle === false) {
    if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`);
    run.degraded(`host not idle: ${run.host.busy.join(',')}`);
  }
  const figures = await runDnsLoad(
    { host: args.probe, port: args['dns-port'], qps: args.qps, duration: args.duration, timeout: args.timeout, cached: args.cached, blocked: args.blocked, uncachedZone: args['uncached-zone'], warm: !args['no-warm'] },
    (l) => run.log(l),
  );
  run.raw({ measurement: 'P10-dns', ...figures });
  run.log(`dns: sent=${figures.sent}/${figures.planned} qps=${figures.qps} answered=${figures.answered} timeouts=${figures.timeouts} unmatched=${figures.unmatched} p50/p95/p99=${figures.lat_ms.p50}/${figures.lat_ms.p95}/${figures.lat_ms.p99} ms blocked→0.0.0.0=${figures.by_class_ms.blocked.answered_0000}/${figures.by_class_ms.blocked.n}`);
  if (figures.sent < figures.planned * 0.95) run.degraded(`sent ${figures.sent} of ${figures.planned} planned queries — the client could not hold ${args.qps} qps`);
  if (figures.unmatched > 0) run.degraded(`${figures.unmatched} unmatched replies`);
  if (figures.timeouts + figures.errors > figures.sent * 0.02) run.degraded(`${figures.timeouts} timeouts + ${figures.errors} errors of ${figures.sent} sent (budget 2 %)`);
  if (figures.by_class_ms.blocked.n && figures.by_class_ms.blocked.answered_0000 < figures.by_class_ms.blocked.n) run.degraded(`${figures.by_class_ms.blocked.n - figures.by_class_ms.blocked.answered_0000} blocked-class answers were not 0.0.0.0 — a name in --blocked is not blocked by this probe's lists`);
  run.finish({ measurement: 'P10-dns', figures, gate: { statistic: 'none — the DNS half of P10\'s mixed arm; P10 proposes an N, the owner decides', p50_ms: figures.lat_ms.p50, p99_ms: figures.lat_ms.p99 } });
}

if (isMain(import.meta.url)) await cli();
