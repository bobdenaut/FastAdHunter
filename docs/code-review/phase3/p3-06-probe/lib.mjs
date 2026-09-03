// lib.mjs — shared code for the p3-06 probe scripts.
//
// Plan: plan/wip/phase3/p3-06-testing-plan.md §Scripts. Nothing is measured
// here. Owns: flag parsing (--probe, --key, --out, --tip on every script), the
// bearer API call, timed TLS/HTTPS requests, percentiles, the valid / INVALID /
// degraded reporting, and the results directory (run.log, raw.jsonl,
// config.json snapshot). Node >= 20, no dependencies.

import fs from 'node:fs';
import path from 'node:path';
import https from 'node:https';
import dgram from 'node:dgram';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';

export const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
export const CA_ISSUER_CN = 'FastAdHunter CA';
export const MiB = 1024 * 1024;

const COMMON = {
  probe: { type: 'string', default: '172.17.0.4', help: 'probe address' },
  'api-port': { type: 'number', default: 8443, help: 'probe API port' },
  key: { type: 'string', default: process.env.FAH_PROBE_KEY ?? null, help: 'API key, or a path to a file holding it (env FAH_PROBE_KEY)' },
  out: { type: 'string', default: null, help: 'results directory (default <scripts>/results-<ts>)' },
  tip: { type: 'string', default: null, help: 'tip hash override when git is unavailable' },
  'allow-busy': { type: 'boolean', default: false, help: 'proceed on a non-idle host; the stage labels the run degraded' },
  'skip-host-checks': { type: 'boolean', default: false, help: 'skip the Windows idle / network-profile checks' },
};

function fail(msg) {
  process.stderr.write(`error: ${msg}\n`);
  process.exit(64);
}

function usage(spec) {
  const lines = Object.entries(spec).map(([k, s]) => {
    const def = s.default === null || s.default === undefined ? '' : ` (default ${JSON.stringify(s.default)})`;
    return `  --${k}${s.type === 'boolean' ? '' : ' <' + s.type + '>'}${s.required ? ' [required]' : ''}  ${s.help ?? ''}${def}`;
  });
  process.stdout.write(`flags:\n${lines.join('\n')}\n`);
}

export function parseArgs(spec, argv = process.argv.slice(2)) {
  const full = { ...COMMON, ...spec };
  const out = {};
  for (const [k, s] of Object.entries(full)) out[k] = s.default;
  for (let i = 0; i < argv.length; i += 1) {
    let a = argv[i];
    if (!a.startsWith('--')) fail(`unexpected argument ${a}`);
    a = a.slice(2);
    let v;
    const eq = a.indexOf('=');
    if (eq >= 0) {
      v = a.slice(eq + 1);
      a = a.slice(0, eq);
    }
    if (a === 'help') {
      usage(full);
      process.exit(0);
    }
    const s = full[a];
    if (!s) fail(`unknown flag --${a}`);
    if (s.type === 'boolean') {
      out[a] = v === undefined ? true : v !== 'false';
      continue;
    }
    if (v === undefined) v = argv[(i += 1)];
    if (v === undefined) fail(`--${a} needs a value`);
    if (s.type === 'number') {
      out[a] = Number(v);
      if (!Number.isFinite(out[a])) fail(`--${a} must be a number`);
    } else if (s.type === 'list') {
      out[a] = v.split(',').map((x) => x.trim()).filter(Boolean);
    } else {
      out[a] = v;
    }
  }
  for (const [k, s] of Object.entries(full)) {
    if (s.required && (out[k] === null || out[k] === undefined)) fail(`--${k} is required (--help for the list)`);
  }
  return out;
}

export function stamp() {
  return new Date().toISOString().replace(/[-:]/g, '').replace(/\.\d+Z$/, 'Z');
}

export function quantile(sorted, q) {
  if (!sorted.length) return null;
  return sorted[Math.round((sorted.length - 1) * q)];
}

export function summary(values) {
  const s = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (!s.length) return { n: 0, min: null, p50: null, p99: null, max: null, mean: null };
  return {
    n: s.length,
    min: round(s[0]),
    p50: round(quantile(s, 0.5)),
    p99: round(quantile(s, 0.99)),
    max: round(s[s.length - 1]),
    mean: round(s.reduce((a, b) => a + b, 0) / s.length),
  };
}

export function median(values) {
  const s = values.filter(Number.isFinite).sort((a, b) => a - b);
  return s.length ? quantile(s, 0.5) : null;
}

export function round(v, d = 3) {
  return v === null || v === undefined || !Number.isFinite(v) ? null : Number(v.toFixed(d));
}

export function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

export function readPemOrValue(v) {
  if (v === null || v === undefined) return null;
  try {
    if (fs.existsSync(v) && fs.statSync(v).isFile()) return fs.readFileSync(v, 'utf8').trim();
  } catch {
    return v;
  }
  return v;
}

export function gitTip() {
  try {
    const hash = execFileSync('git', ['rev-parse', '--short=12', 'HEAD'], { cwd: SCRIPT_DIR, encoding: 'utf8' }).trim();
    const dirty = execFileSync('git', ['status', '--porcelain', '--untracked-files=no'], { cwd: SCRIPT_DIR, encoding: 'utf8' }).trim();
    return dirty ? `${hash}-dirty` : hash;
  } catch {
    return null;
  }
}

// One fresh TLS connection per call (agent: false), like curl. Timings are
// monotonic ms: connect (TCP), secureConnect (TLS done = curl appconnect),
// firstByte (first response byte = curl starttransfer), end.
export function timedRequest(opts) {
  return new Promise((resolve, reject) => {
    const t = { start: performance.now(), connect: null, secureConnect: null, firstByte: null, end: null };
    const info = {};
    const req = https.request({
      host: opts.host,
      port: opts.port,
      path: opts.path,
      method: opts.method || 'GET',
      headers: opts.headers || {},
      servername: opts.servername,
      ca: opts.ca,
      rejectUnauthorized: opts.rejectUnauthorized ?? false,
      localAddress: opts.localAddress,
      agent: false,
      timeout: opts.timeout ?? 30000,
      ALPNProtocols: opts.alpn,
    });
    req.on('socket', (s) => {
      s.once('connect', () => {
        t.connect = performance.now();
      });
      s.once('secureConnect', () => {
        t.secureConnect = performance.now();
        Object.assign(info, peerInfo(s));
      });
      s.once('data', () => {
        t.firstByte = performance.now();
      });
    });
    req.on('timeout', () => req.destroy(new Error('timeout')));
    req.on('error', reject);
    req.on('response', (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('error', reject);
      res.on('end', () => {
        t.end = performance.now();
        resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks), timings: t, tls: info });
      });
    });
    if (opts.body !== undefined && opts.body !== null) req.write(opts.body);
    req.end();
  });
}

export function peerInfo(socket) {
  const c = socket.getPeerCertificate ? socket.getPeerCertificate() : null;
  return {
    issuerCN: c?.issuer?.CN ?? null,
    subjectCN: c?.subject?.CN ?? null,
    fingerprint256: c?.fingerprint256 ?? null,
    tls: socket.getProtocol ? socket.getProtocol() : null,
    alpn: socket.alpnProtocol || null,
    authorized: socket.authorized ?? null,
  };
}

export function dnsQuery(name, id, type = 1) {
  const labels = name.replace(/\.$/, '').split('.');
  const qname = Buffer.concat(labels.map((l) => Buffer.concat([Buffer.from([l.length]), Buffer.from(l, 'ascii')])).concat([Buffer.from([0])]));
  const head = Buffer.alloc(12);
  head.writeUInt16BE(id, 0);
  head.writeUInt16BE(0x0100, 2);
  head.writeUInt16BE(1, 4);
  const tail = Buffer.alloc(4);
  tail.writeUInt16BE(type, 0);
  tail.writeUInt16BE(1, 2);
  return Buffer.concat([head, qname, tail]);
}

export function dnsId(buf) {
  return buf && buf.length >= 2 ? buf.readUInt16BE(0) : null;
}

function skipName(buf, off) {
  for (;;) {
    if (off >= buf.length) return buf.length;
    const len = buf[off];
    if (len === 0) return off + 1;
    if ((len & 0xc0) === 0xc0) return off + 2;
    off += 1 + len;
  }
}

export function dnsParse(buf) {
  if (!buf || buf.length < 12) return { rcode: null, answers: [] };
  const rcode = buf[3] & 0x0f;
  const qd = buf.readUInt16BE(4);
  const an = buf.readUInt16BE(6);
  let off = 12;
  for (let i = 0; i < qd; i += 1) off = skipName(buf, off) + 4;
  const answers = [];
  for (let i = 0; i < an && off + 10 <= buf.length; i += 1) {
    off = skipName(buf, off);
    const type = buf.readUInt16BE(off);
    const rdlen = buf.readUInt16BE(off + 8);
    const rd = buf.subarray(off + 10, off + 10 + rdlen);
    if (type === 1 && rdlen === 4) answers.push(Array.from(rd).join('.'));
    else if (type === 28 && rdlen === 16) answers.push(rd.toString('hex').replace(/(.{4})(?=.)/g, '$1:'));
    off += 10 + rdlen;
  }
  return { rcode, answers };
}

export function localAddressToward(host, port = 53) {
  return new Promise((resolve, reject) => {
    const s = dgram.createSocket('udp4');
    s.on('error', reject);
    s.connect(port, host, () => {
      const a = s.address().address;
      s.close();
      resolve(a);
    });
  });
}

function ip4(s) {
  const p = s.split('.').map(Number);
  if (p.length !== 4 || p.some((x) => !Number.isInteger(x) || x < 0 || x > 255)) return null;
  return ((p[0] << 24) | (p[1] << 16) | (p[2] << 8) | p[3]) >>> 0;
}

export function ipInList(ip, list) {
  const a = ip4(ip);
  if (a === null) return list.includes(ip);
  return (list || []).some((entry) => {
    const [net, bitsRaw] = String(entry).split('/');
    const n = ip4(net);
    if (n === null) return false;
    const bits = bitsRaw === undefined ? 32 : Number(bitsRaw);
    const mask = bits === 0 ? 0 : (0xffffffff << (32 - bits)) >>> 0;
    return (a & mask) === (n & mask);
  });
}

const BUSY_PROCESSES = ['chrome', 'msedge', 'firefox', 'brave', 'opera', 'vivaldi', 'vlc', 'mpc-hc', 'wmplayer', 'spotify', 'teams', 'zoom', 'obs64', 'steam'];

function powershell(cmd) {
  return execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', cmd], { encoding: 'utf8', timeout: 30000 }).trim();
}

export class Run {
  constructor(script, args, { needsHttps = false, needsKey = true, resultName = null } = {}) {
    this.script = script;
    this.args = args;
    this.needsHttps = needsHttps;
    this.needsKey = needsKey;
    this.resultName = resultName ?? `${script}.json`;
    this.key = readPemOrValue(args.key);
    this.degradedReasons = [];
    this.startedAt = new Date().toISOString();
  }

  async init() {
    this.out = this.args.out || path.join(SCRIPT_DIR, `results-${stamp()}`);
    fs.mkdirSync(this.out, { recursive: true });
    this.logPath = path.join(this.out, 'run.log');
    this.rawPath = path.join(this.out, 'raw.jsonl');
    this.tip = this.args.tip || gitTip();
    process.on('unhandledRejection', (e) => this.invalid(`unhandled rejection: ${e?.code ?? ''} ${e?.message ?? e}`.trim()));
    process.on('uncaughtException', (e) => this.invalid(`uncaught exception: ${e?.code ?? ''} ${e?.message ?? e}`.trim()));
    this.log(`== ${this.script} start ${this.startedAt} tip=${this.tip ?? 'unknown'} out=${this.out}`);
    this.log(`args ${JSON.stringify({ ...this.args, key: this.args.key ? '<set>' : null })}`);
    if (this.tip === null) this.degraded('tip hash unknown (no git; pass --tip)');
    if (this.needsKey && !this.key) this.invalid('no API key: --key <key|file> or FAH_PROBE_KEY');
    this.hostChecks();
    const health = await this.api('/health', { auth: false }).catch((e) => ({ status: 0, text: String(e) }));
    this.log(`GET /health -> ${health.status} ${health.text ?? ''}`.slice(0, 300));
    if (health.status !== 200) this.invalid(`probe /health answered ${health.status}`);
    this.health = health.json;
    this.config = await this.snapshotConfig();
    const mode = this.config?.engine?.mode ?? null;
    this.log(`engine.mode = ${mode}`);
    if (this.needsHttps && !(typeof mode === 'string' && mode.includes('https'))) this.invalid(`engine.mode ${mode} carries no https listener`);
    return this;
  }

  hostChecks() {
    this.host = { platform: process.platform, idle: null, network: null };
    if (this.args['skip-host-checks'] || process.platform !== 'win32') {
      this.log(`host checks skipped (${process.platform})`);
      return;
    }
    try {
      const net = powershell('Get-NetConnectionProfile | Select-Object InterfaceAlias,NetworkCategory | Format-Table -HideTableHeaders | Out-String');
      this.host.network = net.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
      this.log(`Get-NetConnectionProfile: ${this.host.network.join(' | ')}`);
      const tasks = powershell('tasklist /FO CSV /NH').toLowerCase();
      const busy = BUSY_PROCESSES.filter((p) => tasks.includes(`"${p}.exe"`));
      this.host.idle = busy.length === 0;
      this.host.busy = busy;
      this.log(`idle check: ${this.host.idle ? 'PASS' : 'FAIL ' + busy.join(',')} (recorded; each stage decides)`);
    } catch (e) {
      this.log(`host checks failed to run: ${e.message}`);
      this.host.error = e.message;
    }
  }

  async snapshotConfig() {
    const r = await this.api('/api/v1/config');
    if (r.status !== 200) this.invalid(`GET /api/v1/config answered ${r.status}: ${r.text.slice(0, 200)}`);
    const live = JSON.stringify(r.json, null, 2);
    const p = path.join(this.out, 'config.json');
    if (!fs.existsSync(p)) {
      fs.writeFileSync(p, live);
      this.log(`config snapshot -> ${p}`);
    } else if (fs.readFileSync(p, 'utf8') !== live) {
      const alt = path.join(this.out, `config.${this.script}.${stamp()}.json`);
      fs.writeFileSync(alt, live);
      this.log(`probe config differs from ${p}; live copy -> ${alt}`);
      this.degraded('probe config changed since this results directory was opened');
    }
    return r.json;
  }

  log(line) {
    const l = `${new Date().toISOString()} [${this.script}] ${line}`;
    process.stdout.write(`${l}\n`);
    if (this.logPath) fs.appendFileSync(this.logPath, `${l}\n`);
  }

  raw(obj) {
    fs.appendFileSync(this.rawPath, `${JSON.stringify({ ts: new Date().toISOString(), script: this.script, ...obj })}\n`);
  }

  degraded(reason) {
    this.degradedReasons.push(reason);
    this.log(`degraded: ${reason}`);
  }

  async api(p, { method = 'GET', body, headers = {}, auth = true, timeout } = {}) {
    const h = { ...headers };
    if (auth && this.key) h.authorization = `Bearer ${this.key}`;
    let payload = body;
    if (body !== undefined && body !== null && !Buffer.isBuffer(body) && typeof body !== 'string') {
      payload = JSON.stringify(body);
      h['content-type'] = 'application/json';
    }
    const r = await timedRequest({ host: this.args.probe, port: this.args['api-port'], path: p, method, headers: h, body: payload, timeout });
    const text = r.body.toString('utf8');
    let json = null;
    try {
      json = JSON.parse(text);
    } catch {
      json = null;
    }
    return { ...r, json, text };
  }

  async apiJson(p, opts) {
    const r = await this.api(p, opts);
    if (r.status !== 200) this.invalid(`${opts?.method ?? 'GET'} ${p} answered ${r.status}: ${r.text.slice(0, 200)}`);
    return r.json;
  }

  async telemetry() {
    return this.apiJson('/api/v1/telemetry');
  }

  async certificates() {
    return this.apiJson('/api/v1/certificates');
  }

  async processRss() {
    const m = await this.apiJson('/api/v1/debug/memory');
    return m.process_rss;
  }

  resultPath(name) {
    return path.join(this.out, name);
  }

  write(name, obj) {
    const p = this.resultPath(name);
    fs.writeFileSync(p, JSON.stringify(obj, null, 2));
    this.log(`wrote ${p}`);
    return p;
  }

  envelope(extra) {
    return {
      script: this.script,
      started_at: this.startedAt,
      finished_at: new Date().toISOString(),
      tip: this.tip,
      probe: this.args.probe,
      probe_version: this.health?.version ?? null,
      host: this.host,
      args: { ...this.args, key: this.args.key ? '<set>' : null },
      ...extra,
    };
  }

  invalid(reason, extra = {}, name = this.resultName) {
    this.log(`INVALID: ${reason}`);
    if (this.out) this.write(name, this.envelope({ valid: false, status: 'INVALID', reason, ...extra }));
    process.exit(2);
  }

  finish(obj, name = this.resultName) {
    const status = this.degradedReasons.length ? 'degraded' : 'valid';
    const result = this.envelope({ valid: true, status, degraded_reasons: this.degradedReasons, ...obj });
    this.write(name, result);
    if (obj.gate) this.log(`GATE ${status === 'degraded' ? '(degraded, not a gate) ' : ''}${JSON.stringify(obj.gate)}`);
    this.log(`== ${this.script} done (${status})`);
    process.exit(status === 'degraded' ? 3 : 0);
  }
}
