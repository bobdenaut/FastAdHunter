// p2-handshake.mjs — P2 handshake cost: direct vs spliced vs intercepted
// (plan §Measurements row P2; declaration deltas 5 and 9).
//
// RUNS ON THE WIRED BRIDGED VM, never on bobdenaut: every socket of all three
// arms binds a VM address. bobdenaut launches it over ssh and copies the
// results directory back. Two verified same-family IPv4 addresses on the
// VM's bridged interface — --listed is in the Interception Document's
// `clients` (GET/PUT /api/v1/interception, live, no restart), --unlisted is
// not. Identity precondition, proven before round 1, else INVALID: both
// addresses on the interface (`os.networkInterfaces()`, so Linux, macOS and
// Windows alike), each observed by the probe (/api/v1/clients after one
// UDP/53 query bound to each), exactly one listed (the document lib.mjs
// snapshots as interception.json — never /api/v1/config, which omits it).
//
// Arms, interleaved per round with a rotating start so no arm always runs
// first:
//   direct       unlisted -> origin :443            (system roots)
//   spliced      unlisted -> probe :8444, SNI origin (system roots; issuer must
//                                                     NOT be the FastAdHunter CA)
//   intercepted  listed   -> probe :8444, SNI origin (--ca trusted; issuer MUST
//                                                     be the FastAdHunter CA)
// Per row: handshake = TCP connect -> secureConnect; first-byte = secureConnect
// -> first response byte of `GET --path HTTP/1.1`. Served issuer per row is
// the authoritative leg proof; counters (minted_total, blocked, connections)
// before/after the whole run are supporting only, never asserted (+1 is not
// required — the origin's leaf may already be cached).
// NAMING (review file, campaign-2 declaration change 4): `handshake_ms` is
// secureConnect - connect. On `direct` that is a TLS handshake. On `spliced`
// and `intercepted` it is PROXY SETUP + RELAYED HANDSHAKE — ClientHello read,
// SNI verdict, one uncached upstream A+AAAA resolve, egress check, upstream
// TCP connect, then the handshake. The ratio gate is unaffected (both arms pay
// it, it cancels); the row value spliced-minus-direct is not, and ships named.
//
// PRE-RELAY SPLIT (declaration change 5, diagnostic only, never a gate): the
// probe emits `https-sni` events whose `duration_ms` is measured before the
// relay starts, so it isolates resolve + connect from the relayed handshake.
// Spliced arm only — a successful intercepted session emits no such event, and
// direct never reaches the probe. Rows attribute by client address. Any socket
// failure degrades the run, never invalidates it. --no-events opts out.
//
// Gate: handshake p50(intercepted) <= 2 x p50(spliced). Row value:
// spliced p50 - direct p50, handshake and first-byte.
//
//   node p2-handshake.mjs --key <file> --origin example.com --ca fastadhunter-ca.pem \
//     --listed 192.168.10.41 --unlisted 192.168.10.42 --rounds 200

import fs from 'node:fs';
import tls from 'node:tls';
import dgram from 'node:dgram';
import crypto from 'node:crypto';
import os from 'node:os';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, CA_ISSUER_CN, summary, round, dnsQuery, dnsId, ipInList, peerInfo } from './lib.mjs';

const args = parseArgs({
  origin: { type: 'string', required: true, help: 'one fixed public origin' },
  'origin-port': { type: 'number', default: 443, help: 'origin port' },
  path: { type: 'string', default: '/', help: 'request path for the first-byte column' },
  ca: { type: 'string', required: true, help: 'FastAdHunter CA PEM (export from the probe)' },
  listed: { type: 'string', required: true, help: 'VM IPv4 listed in the Interception Document (GET /api/v1/interception, clients)' },
  unlisted: { type: 'string', required: true, help: 'VM IPv4 that is not listed' },
  port: { type: 'number', default: 8444, help: '[https.listen] port on the probe' },
  'dns-port': { type: 'number', default: 53, help: 'probe DNS port for the identity query' },
  rounds: { type: 'number', default: 200, help: 'rounds; each round runs all three arms' },
  timeout: { type: 'number', default: 10000, help: 'ms per attempt' },
  'max-fail-pct': { type: 'number', default: 2, help: 'failure rate per arm above which the run is degraded' },
  'no-events': { type: 'boolean', default: false, help: 'skip the pre-relay split (declaration change 5); the declared figures are unaffected' },
});

const run = await new Run('p2', args, { needsHttps: true }).init();
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}
const caPem = fs.readFileSync(args.ca, 'utf8');

function prefixOf(netmask) {
  if (typeof netmask !== 'string') return null;
  const octets = netmask.split('.').map(Number);
  if (octets.length !== 4 || octets.some((o) => !Number.isInteger(o) || o < 0 || o > 255)) return null;
  return octets.reduce((n, o) => n + (o.toString(2).match(/1/g)?.length ?? 0), 0);
}

// `ip -4 -o addr` exists only on Linux: smoke Layer 1 got `spawnSync ip ENOENT`
// on this host and P2 went INVALID before round 1. os.networkInterfaces() needs
// no subprocess, works on Linux, macOS and Windows alike, and names the
// interface each address sits on — which is what the wired-alias precondition
// (plan §The Mac endpoint item 5) is actually about. Same pass/fail semantics
// as before: presence of both addresses, nothing new asserted.
function addressesOnInterfaces() {
  try {
    const out = [];
    for (const [iface, entries] of Object.entries(os.networkInterfaces())) {
      for (const entry of entries ?? []) {
        // Node >= 18 reports 'IPv4'; older builds reported the number 4.
        if (entry.family !== 'IPv4' && entry.family !== 4) continue;
        if (entry.internal) continue;
        out.push({ ip: entry.address, prefix: prefixOf(entry.netmask), iface });
      }
    }
    return out;
  } catch (e) {
    return { error: e.message };
  }
}

function identityQuery(localAddress) {
  return new Promise((resolve) => {
    const s = dgram.createSocket('udp4');
    const id = Math.floor(Math.random() * 65536);
    const timer = setTimeout(() => {
      s.close();
      resolve({ localAddress, answered: false });
    }, 3000);
    s.on('error', (e) => {
      clearTimeout(timer);
      resolve({ localAddress, answered: false, error: e.code || e.message });
    });
    s.on('message', (m) => {
      if (dnsId(m) !== id) return;
      clearTimeout(timer);
      s.close();
      resolve({ localAddress, answered: true });
    });
    s.bind({ address: localAddress }, () => {
      s.send(dnsQuery(`p2-identity-${localAddress.replace(/\./g, '-')}.fah.test`, id), args['dns-port'], args.probe);
    });
  });
}

const iface = addressesOnInterfaces();
run.log(`os.networkInterfaces() IPv4 (${os.platform()}): ${JSON.stringify(iface)}`);
const present = Array.isArray(iface) ? [args.listed, args.unlisted].filter((a) => iface.some((x) => x.ip === a)) : [];
if (present.length !== 2) run.invalid(`identity precondition: both --listed and --unlisted must be on this host's interfaces (found ${present.join(',') || 'none'})`, { iface });
const seen = [await identityQuery(args.listed), await identityQuery(args.unlisted)];
run.log(`identity queries: ${JSON.stringify(seen)}`);
if (!seen.every((s) => s.answered)) run.invalid('identity precondition: the probe did not answer a UDP/53 query bound to each address', { seen });
const clients = (await run.apiJson('/api/v1/clients')).items?.map((c) => c.ip) ?? [];
const observed = [args.listed, args.unlisted].filter((a) => clients.includes(a));
if (observed.length !== 2) run.invalid(`identity precondition: /api/v1/clients observed ${observed.join(',') || 'neither'}; a bridged VM on Wi-Fi is the usual cause (plan §Traps)`, { clients });
const listedSet = run.interception.clients;
const listedOk = ipInList(args.listed, listedSet) && !ipInList(args.unlisted, listedSet);
run.log(`/api/v1/interception clients = ${JSON.stringify(listedSet)} listed=${ipInList(args.listed, listedSet)} unlisted=${ipInList(args.unlisted, listedSet)}`);
if (!listedOk) run.invalid('identity precondition: exactly one of the two addresses must be listed in the Interception Document (GET /api/v1/interception, clients)', { listedSet });

const ARMS = {
  direct: { host: args.origin, port: args['origin-port'], localAddress: args.unlisted, ca: undefined, expectCa: false },
  spliced: { host: args.probe, port: args.port, localAddress: args.unlisted, ca: undefined, expectCa: false },
  intercepted: { host: args.probe, port: args.port, localAddress: args.listed, ca: caPem, expectCa: true },
};
const ORDER = ['direct', 'spliced', 'intercepted'];

function attempt(arm) {
  const a = ARMS[arm];
  return new Promise((resolve) => {
    const t = { connect: null, secure: null, first: null };
    let info = null;
    let settled = false;
    const finish = (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      sock.destroy();
      resolve({
        arm,
        error,
        handshake_ms: t.connect !== null && t.secure !== null ? round(t.secure - t.connect) : null,
        first_byte_ms: t.secure !== null && t.first !== null ? round(t.first - t.secure) : null,
        issuer: info?.issuerCN ?? null,
        subject: info?.subjectCN ?? null,
        tls: info?.tls ?? null,
        alpn: info?.alpn ?? null,
        authorized: info?.authorized ?? null,
      });
    };
    const timer = setTimeout(() => finish('timeout'), args.timeout);
    const sock = tls.connect({ host: a.host, port: a.port, servername: args.origin, localAddress: a.localAddress, ca: a.ca, rejectUnauthorized: true });
    sock.setNoDelay(true);
    sock.on('connect', () => {
      t.connect = performance.now();
    });
    sock.on('secureConnect', () => {
      t.secure = performance.now();
      info = peerInfo(sock);
      sock.write(`GET ${args.path} HTTP/1.1\r\nHost: ${args.origin}\r\nUser-Agent: fah-p2\r\nConnection: close\r\n\r\n`);
    });
    sock.on('data', () => {
      if (t.first === null) {
        t.first = performance.now();
        finish(null);
      }
    });
    sock.on('error', (e) => finish(e.code || e.message));
    sock.on('close', () => finish(t.first === null ? 'closed_without_response' : null));
  });
}

// Minimal RFC 6455 client over the API's own TLS socket. No dependency: the
// probe image carries no `ws` package, and Node's global WebSocket cannot be
// pointed at the API's self-signed certificate the way lib.mjs's timedRequest
// is. Server frames are unmasked, client frames must be masked.
function wsFrame(opcode, text) {
  const data = Buffer.from(text, 'utf8');
  const mask = crypto.randomBytes(4);
  let header;
  if (data.length < 126) {
    header = Buffer.alloc(2);
    header[1] = 0x80 | data.length;
  } else if (data.length < 65536) {
    header = Buffer.alloc(4);
    header[1] = 0x80 | 126;
    header.writeUInt16BE(data.length, 2);
  } else {
    header = Buffer.alloc(10);
    header[1] = 0x80 | 127;
    header.writeBigUInt64BE(BigInt(data.length), 2);
  }
  header[0] = 0x80 | opcode;
  const body = Buffer.allocUnsafe(data.length);
  for (let i = 0; i < data.length; i += 1) body[i] = data[i] ^ mask[i % 4];
  return Buffer.concat([header, mask, body]);
}

function openEvents() {
  const state = { rows: [], opened: false, done: false, failure: null, frames: 0, sock: null };
  if (args['no-events']) {
    state.failure = 'disabled by --no-events';
    return state;
  }
  const fail = (why) => {
    if (state.done) return;
    if (state.failure === null) state.failure = state.opened ? `${why} after ${state.frames} events` : why;
  };
  // No `servername`: --probe is an IP on the device (172.17.0.4) and Node
  // refuses an IP as SNI. lib.mjs's API calls omit it for the same reason.
  const sock = tls.connect({ host: args.probe, port: args['api-port'], rejectUnauthorized: false });
  state.sock = sock;
  sock.on('error', (e) => fail(e.code || e.message));
  sock.on('close', () => fail('socket closed before the run ended'));

  let buf = Buffer.alloc(0);
  let upgraded = false;
  let pending = null;
  let pendingOpcode = 0;

  const onText = (text) => {
    let message;
    try {
      message = JSON.parse(text);
    } catch {
      return;
    }
    if (message?.type !== 'query') return;
    const d = message.data;
    if (d?.kind !== 'https-sni') return;
    state.frames += 1;
    state.rows.push({ client: d.client, domain: d.domain, verdict: d.verdict, duration_ms: d.duration_ms });
  };

  const parseFrames = () => {
    for (;;) {
      if (buf.length < 2) return;
      const fin = (buf[0] & 0x80) !== 0;
      const opcode = buf[0] & 0x0f;
      const masked = (buf[1] & 0x80) !== 0;
      let len = buf[1] & 0x7f;
      let offset = 2;
      if (len === 126) {
        if (buf.length < 4) return;
        len = buf.readUInt16BE(2);
        offset = 4;
      } else if (len === 127) {
        if (buf.length < 10) return;
        len = Number(buf.readBigUInt64BE(2));
        offset = 10;
      }
      let mask = null;
      if (masked) {
        if (buf.length < offset + 4) return;
        mask = buf.subarray(offset, offset + 4);
        offset += 4;
      }
      if (buf.length < offset + len) return;
      let payload = buf.subarray(offset, offset + len);
      if (mask) {
        const un = Buffer.allocUnsafe(len);
        for (let i = 0; i < len; i += 1) un[i] = payload[i] ^ mask[i % 4];
        payload = un;
      }
      buf = buf.subarray(offset + len);

      if (opcode === 0x9) {
        sock.write(wsFrame(0xa, payload.toString('utf8')));
        continue;
      }
      if (opcode === 0x8) {
        state.done = true;
        sock.destroy();
        return;
      }
      if (opcode === 0xa) continue;
      if (opcode === 0x0) {
        pending = pending ? Buffer.concat([pending, payload]) : payload;
      } else {
        pending = payload;
        pendingOpcode = opcode;
      }
      if (fin) {
        if (pendingOpcode === 0x1 && pending) onText(pending.toString('utf8'));
        pending = null;
      }
    }
  };

  sock.on('data', (chunk) => {
    buf = Buffer.concat([buf, chunk]);
    if (!upgraded) {
      const end = buf.indexOf('\r\n\r\n');
      if (end < 0) return;
      const head = buf.subarray(0, end).toString('utf8');
      buf = buf.subarray(end + 4);
      if (!/^HTTP\/1\.1 101/.test(head)) {
        fail(`events upgrade answered ${head.split('\r\n')[0]}`);
        sock.destroy();
        return;
      }
      upgraded = true;
      state.opened = true;
      // Query events only: the periodic stats push becomes a Ping we answer,
      // which keeps the socket alive without adding frames to parse.
      sock.write(wsFrame(0x1, JSON.stringify({ subscribe: ['query'] })));
    }
    parseFrames();
  });

  sock.on('secureConnect', () => {
    const nonce = crypto.randomBytes(16).toString('base64');
    sock.write(
      `GET /api/v1/events HTTP/1.1\r\nHost: ${args.probe}:${args['api-port']}\r\n` +
        `Upgrade: websocket\r\nConnection: Upgrade\r\n` +
        `Sec-WebSocket-Key: ${nonce}\r\nSec-WebSocket-Version: 13\r\n` +
        `Authorization: Bearer ${run.key}\r\n\r\n`,
    );
  });
  return state;
}

const events = openEvents();
const certsBefore = (await run.certificates()).leaf_cache;
const telBefore = (await run.telemetry()).listeners.https;
const rows = [];
for (let r = 1; r <= args.rounds; r += 1) {
  for (let k = 0; k < ORDER.length; k += 1) {
    const arm = ORDER[(r - 1 + k) % ORDER.length];
    const row = await attempt(arm);
    row.round = r;
    rows.push(row);
    run.raw({ measurement: 'P2', ...row });
  }
  if (r % 20 === 0 || r === args.rounds) {
    const last = ORDER.map((a) => {
      const rs = rows.filter((x) => x.arm === a && x.error === null);
      return `${a}=${rs.length ? rs[rs.length - 1].handshake_ms : '-'}`;
    });
    run.log(`round ${r}/${args.rounds} handshake ms ${last.join(' ')}`);
  }
}
// The https-sni event is published when the session closes, carrying the
// pre-relay duration measured before the relay began. Give the last rounds'
// events time to land before reading them.
if (events.sock && !events.done) {
  await new Promise((r) => setTimeout(r, 1500));
  events.done = true;
  events.sock.destroy();
}

const certsAfter = (await run.certificates()).leaf_cache;
const telAfter = (await run.telemetry()).listeners.https;
const counters = {
  minted_total_delta: certsAfter.minted_total - certsBefore.minted_total,
  unwarmed_misses_delta: certsAfter.unwarmed_misses - certsBefore.unwarmed_misses,
  listeners_https_delta: Object.fromEntries(Object.keys(telBefore).map((k) => [k, (telAfter[k] ?? 0) - (telBefore[k] ?? 0)])),
};
run.log(`counters ${JSON.stringify(counters)}`);

const byArm = Object.fromEntries(ORDER.map((a) => [a, rows.filter((x) => x.arm === a)]));
const provenance = {};
for (const a of ORDER) {
  const ok = byArm[a].filter((x) => x.error === null);
  const caRows = ok.filter((x) => x.issuer === CA_ISSUER_CN).length;
  provenance[a] = { ok: ok.length, failed: byArm[a].length - ok.length, issuer_ca_rows: caRows, issuers: [...new Set(ok.map((x) => x.issuer))], tls: [...new Set(ok.map((x) => x.tls))], alpn: [...new Set(ok.map((x) => x.alpn))] };
}
run.log(`provenance ${JSON.stringify(provenance)}`);
for (const a of ORDER) {
  if (provenance[a].ok === 0) run.invalid(`${a} arm: zero completed rows (delta 11)`, { provenance, counters });
}
if (provenance.spliced.issuer_ca_rows !== 0 || provenance.direct.issuer_ca_rows !== 0) run.invalid('a spliced or direct row was served the FastAdHunter CA: the unlisted address was intercepted', { provenance, counters });
if (provenance.intercepted.issuer_ca_rows !== provenance.intercepted.ok) run.invalid('an intercepted row was served the origin chain: the listed address was spliced', { provenance, counters });
for (const a of ORDER) {
  const pct = (100 * provenance[a].failed) / byArm[a].length;
  if (pct > args['max-fail-pct']) run.degraded(`${a} failure rate ${round(pct, 1)}% over ${args['max-fail-pct']}%`);
}
if (counters.listeners_https_delta.blocked !== 0) run.degraded(`listeners.https.blocked moved by ${counters.listeners_https_delta.blocked} during the run (plan: blocked = 0)`);

const figures = {};
for (const a of ORDER) {
  const ok = byArm[a].filter((x) => x.error === null);
  figures[a] = { handshake_ms: summary(ok.map((x) => x.handshake_ms)), first_byte_ms: summary(ok.map((x) => x.first_byte_ms)), served_issuer: provenance[a].issuers.join('|') };
}
const rowValue = {
  handshake_ms: round(figures.spliced.handshake_ms.p50 - figures.direct.handshake_ms.p50),
  first_byte_ms: round(figures.spliced.first_byte_ms.p50 - figures.direct.first_byte_ms.p50),
};
const ratio = figures.spliced.handshake_ms.p50 ? round(figures.intercepted.handshake_ms.p50 / figures.spliced.handshake_ms.p50) : null;

// Diagnostic (declaration change 5): splits the spliced arm's socket-side
// figure into pre-relay work (SNI verdict + uncached A+AAAA resolve + egress
// check + upstream connect) and the relayed handshake. Never a gate.
const sniRows = events.rows.filter((x) => x.client === args.unlisted);
const preRelay = sniRows.length ? summary(sniRows.map((x) => x.duration_ms)) : null;
const preRelaySplit = {
  source: 'https-sni events, duration_ms (fah-http/src/https.rs:216)',
  arm: 'spliced',
  note: 'intercepted emits no pre-relay event on success; direct never reaches the probe',
  events_seen: events.rows.length,
  events_matched: sniRows.length,
  domains: [...new Set(sniRows.map((x) => x.domain))],
  verdicts: [...new Set(sniRows.map((x) => x.verdict))],
  pre_relay_ms: preRelay,
  relayed_handshake_p50_ms:
    preRelay && figures.spliced.handshake_ms.p50 !== null
      ? round(figures.spliced.handshake_ms.p50 - preRelay.p50)
      : null,
  failure: events.failure,
};
run.log(`pre-relay split ${JSON.stringify(preRelaySplit)}`);
if (events.failure) run.degraded(`pre-relay split unavailable: ${events.failure} (diagnostic only, declared figures unaffected)`);
else if (sniRows.length === 0) run.degraded('pre-relay split: no https-sni events matched the spliced arm (diagnostic only)');

run.finish({
  measurement: 'P2',
  quantity: {
    handshake_ms: 'secureConnect - connect',
    direct: 'TLS handshake',
    spliced: 'proxy setup + relayed handshake (includes one uncached upstream A+AAAA resolve)',
    intercepted: 'proxy setup + upstream TLS + relayed handshake',
    note: 'campaign-2 declaration change 4: the ratio gate cancels this cost, the spliced-minus-direct row does not',
  },
  origin: { name: args.origin, port: args['origin-port'], path: args.path, tls: provenance.direct.tls, alpn: provenance.direct.alpn },
  identities: { listed: args.listed, unlisted: args.unlisted, direct_arm_source: args.unlisted },
  rounds: args.rounds,
  figures,
  row_spliced_minus_direct_p50: rowValue,
  pre_relay_split_spliced: preRelaySplit,
  first_byte_ratio_intercepted_over_spliced: figures.spliced.first_byte_ms.p50 ? round(figures.intercepted.first_byte_ms.p50 / figures.spliced.first_byte_ms.p50) : null,
  provenance,
  counters,
  gate: {
    statistic: 'handshake p50: intercepted <= 2 x spliced',
    intercepted_p50_ms: figures.intercepted.handshake_ms.p50,
    spliced_p50_ms: figures.spliced.handshake_ms.p50,
    ratio,
    pass: ratio !== null && ratio <= 2,
  },
});
