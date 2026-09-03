// p0-sni.mjs — SNI gate (plan §Measurements row SNI).
//
// Question: does a blocked domain close at SNI, before any certificate?
// Method: a hand-built ClientHello over a raw TCP socket to the probe's
// [https.listen] port, three kinds — a name the probe's lists block, a name
// they allow, and a hello with no SNI extension. Bytes received before the
// socket closes classify the attempt:
//   0 bytes, then FIN/RST          -> closed_silent   (before any certificate)
//   0x15 alert record, then close  -> alert           (before any certificate)
//   0x16 handshake record          -> server_hello    (certificate path opened;
//                                     TLS 1.3 encrypts the certificate, so the
//                                     ServerHello is the observable boundary)
//   no bytes, no close in --timeout -> timeout         (not closed: not a pass)
// Gate (boolean): every blocked and no-SNI attempt is closed_silent or alert
// AND every allowed attempt reaches server_hello (else the listener may be
// closing everything and the blocked rows prove nothing -> INVALID).
// Diagnostic: close latency (connect -> close) per attempt.
//
//   node p0-sni.mjs --key <file> --blocked ads.example.net --allowed example.com

import net from 'node:net';
import crypto from 'node:crypto';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, summary, round } from './lib.mjs';

const args = parseArgs({
  blocked: { type: 'string', required: true, help: 'a name the probe blocks' },
  allowed: { type: 'string', required: true, help: 'a name the probe allows (public, resolvable)' },
  port: { type: 'number', default: 8444, help: '[https.listen] port' },
  attempts: { type: 'number', default: 5, help: 'attempts per kind' },
  timeout: { type: 'number', default: 5000, help: 'ms to wait for bytes or close' },
});

const run = await new Run('sni', args, { needsHttps: true }).init();
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}

function u16(n) {
  const b = Buffer.alloc(2);
  b.writeUInt16BE(n);
  return b;
}
function u24(n) {
  return Buffer.from([(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff]);
}
function u16list(items) {
  const body = Buffer.concat(items.map(u16));
  return Buffer.concat([u16(body.length), body]);
}
function ext(type, body) {
  return Buffer.concat([u16(type), u16(body.length), body]);
}
function x25519PublicRaw() {
  const { publicKey } = crypto.generateKeyPairSync('x25519');
  return Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url');
}

function clientHello(sni) {
  const exts = [];
  if (sni) {
    const name = Buffer.from(sni, 'ascii');
    const entry = Buffer.concat([Buffer.from([0]), u16(name.length), name]);
    exts.push(ext(0x0000, Buffer.concat([u16(entry.length), entry])));
  }
  exts.push(ext(0x000a, u16list([0x001d, 0x0017, 0x0018])));
  exts.push(ext(0x000b, Buffer.from([1, 0])));
  exts.push(ext(0x000d, u16list([0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601])));
  exts.push(ext(0x002b, Buffer.concat([Buffer.from([4]), u16(0x0304), u16(0x0303)])));
  const key = x25519PublicRaw();
  const share = Buffer.concat([u16(0x001d), u16(key.length), key]);
  exts.push(ext(0x0033, Buffer.concat([u16(share.length), share])));
  exts.push(ext(0x002d, Buffer.from([1, 1])));
  const extBlock = Buffer.concat(exts);
  const body = Buffer.concat([
    u16(0x0303),
    crypto.randomBytes(32),
    Buffer.from([0]),
    u16list([0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8]),
    Buffer.from([1, 0]),
    u16(extBlock.length),
    extBlock,
  ]);
  const hs = Buffer.concat([Buffer.from([1]), u24(body.length), body]);
  return Buffer.concat([Buffer.from([0x16, 0x03, 0x01]), u16(hs.length), hs]);
}

function attempt(kind, sni) {
  return new Promise((resolve) => {
    const hello = clientHello(sni);
    const chunks = [];
    const t0 = performance.now();
    let tConnect = null;
    let done = false;
    const sock = net.connect({ host: args.probe, port: args.port });
    sock.setNoDelay(true);
    const finish = (how, error = null) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      const bytes = Buffer.concat(chunks);
      let outcome;
      if (bytes.length === 0) outcome = how === 'timeout' ? 'timeout' : 'closed_silent';
      else if (bytes[0] === 0x15) outcome = 'alert';
      else if (bytes[0] === 0x16) outcome = 'server_hello';
      else outcome = 'other_bytes';
      const row = {
        kind,
        sni,
        outcome,
        how,
        error,
        bytes_received: bytes.length,
        first_bytes_hex: bytes.subarray(0, 8).toString('hex'),
        alert: outcome === 'alert' && bytes.length >= 7 ? { level: bytes[5], description: bytes[6] } : null,
        connect_ms: tConnect === null ? null : round(tConnect - t0),
        close_ms: round(performance.now() - t0),
        closed_before_certificate: outcome === 'closed_silent' || outcome === 'alert',
      };
      sock.destroy();
      resolve(row);
    };
    const timer = setTimeout(() => finish('timeout'), args.timeout);
    sock.on('connect', () => {
      tConnect = performance.now();
      sock.write(hello);
    });
    sock.on('data', (c) => {
      chunks.push(c);
      if (c[0] === 0x16 || Buffer.concat(chunks)[0] === 0x16) finish('server_hello_seen');
    });
    sock.on('end', () => finish('fin'));
    sock.on('close', (hadError) => finish(hadError ? 'reset' : 'close'));
    sock.on('error', (e) => finish('error', e.code || e.message));
  });
}

const noSni = run.config?.https?.sni?.no_sni ?? null;
run.log(`https.sni.no_sni = ${noSni}`);
const before = (await run.telemetry()).listeners?.https ?? null;
if (!before) run.invalid('telemetry carries no listeners.https block');

const kinds = [
  ['blocked', args.blocked],
  ['allowed', args.allowed],
  ['no_sni', null],
];
const rows = [];
for (let i = 0; i < args.attempts; i += 1) {
  for (const [kind, sni] of kinds) {
    const row = await attempt(kind, sni);
    row.attempt = i + 1;
    rows.push(row);
    run.raw({ measurement: 'sni', ...row });
    run.log(`${kind.padEnd(8)} #${i + 1} ${row.outcome.padEnd(13)} bytes=${row.bytes_received} close=${row.close_ms}ms${row.error ? ' ' + row.error : ''}`);
  }
}
const after = (await run.telemetry()).listeners.https;
const delta = Object.fromEntries(Object.keys(before).map((k) => [k, (after[k] ?? 0) - (before[k] ?? 0)]));
run.log(`listeners.https delta ${JSON.stringify(delta)}`);

const byKind = (k) => rows.filter((r) => r.kind === k);
const allowedOpened = byKind('allowed').every((r) => r.outcome === 'server_hello');
if (!allowedOpened) {
  run.invalid('allowed name did not reach ServerHello through the probe; the listener closing everything proves nothing about blocked rows', {
    rows,
    telemetry_delta: delta,
    hint: 'check resolve_failures / refused_destination / upstream_failures in telemetry_delta and that --allowed is public and resolvable from the container',
  });
}
const expectedBlocked = args.attempts * (noSni === 'pass' ? 1 : 2);
if ((delta.blocked ?? 0) < expectedBlocked) {
  run.invalid(`blocked rows closed without a rule verdict: listeners.https.blocked moved by ${delta.blocked}, expected at least ${expectedBlocked} (resolve_failures ${delta.resolve_failures}, refused_destination ${delta.refused_destination}); --blocked is not blocked by the probe's ruleset`, {
    rows,
    telemetry_delta: delta,
    hint: 'check GET /api/v1/rules/user and the list set; a close caused by a resolve failure proves nothing about the SNI verdict',
  });
}
const blockedClosed = byKind('blocked').every((r) => r.closed_before_certificate);
const noSniClosed = byKind('no_sni').every((r) => r.closed_before_certificate);
const figures = {};
for (const [kind] of kinds) {
  const rs = byKind(kind);
  figures[kind] = {
    n: rs.length,
    outcomes: Object.fromEntries(rs.map((r) => [r.outcome, rs.filter((x) => x.outcome === r.outcome).length])),
    closed_before_certificate: rs.every((r) => r.closed_before_certificate),
    close_ms: summary(rs.map((r) => r.close_ms)),
  };
}

run.finish({
  measurement: 'SNI',
  no_sni_setting: noSni,
  figures,
  telemetry_delta: delta,
  supporting: {
    blocked_delta_expected_at_least: expectedBlocked,
    blocked_delta_observed: delta.blocked,
  },
  rows,
  gate: {
    statistic: 'every blocked and no-SNI attempt closed before a certificate (boolean)',
    blocked_closed: blockedClosed,
    no_sni_closed: noSniClosed,
    allowed_opened: allowedOpened,
    pass: blockedClosed && noSniClosed && allowedOpened,
  },
});
