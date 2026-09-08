// p2-handshake.mjs — P2 handshake cost: direct vs spliced vs intercepted
// (plan §Measurements row P2; declaration deltas 5 and 9).
//
// RUNS ON THE WIRED BRIDGED VM, never on bobdenaut: every socket of all three
// arms binds a VM address. bobdenaut launches it over ssh and copies the
// results directory back. Two verified same-family IPv4 addresses on the
// VM's bridged interface — --listed is in [https.interception] clients,
// --unlisted is not. Identity precondition, proven before round 1, else
// INVALID: both addresses on the interface (`os.networkInterfaces()`, so Linux,
// macOS and Windows alike), each observed by
// the probe (/api/v1/clients after one UDP/53 query bound to each), exactly
// one listed (/api/v1/config).
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
// Gate: handshake p50(intercepted) <= 2 x p50(spliced). Row value:
// spliced p50 - direct p50, handshake and first-byte.
//
//   node p2-handshake.mjs --key <file> --origin example.com --ca fastadhunter-ca.pem \
//     --listed 192.168.10.41 --unlisted 192.168.10.42 --rounds 200

import fs from 'node:fs';
import tls from 'node:tls';
import dgram from 'node:dgram';
import os from 'node:os';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, CA_ISSUER_CN, summary, round, dnsQuery, dnsId, ipInList, peerInfo } from './lib.mjs';

const args = parseArgs({
  origin: { type: 'string', required: true, help: 'one fixed public origin' },
  'origin-port': { type: 'number', default: 443, help: 'origin port' },
  path: { type: 'string', default: '/', help: 'request path for the first-byte column' },
  ca: { type: 'string', required: true, help: 'FastAdHunter CA PEM (export from the probe)' },
  listed: { type: 'string', required: true, help: 'VM IPv4 that is in https.interception.clients' },
  unlisted: { type: 'string', required: true, help: 'VM IPv4 that is not' },
  port: { type: 'number', default: 8444, help: '[https.listen] port on the probe' },
  'dns-port': { type: 'number', default: 53, help: 'probe DNS port for the identity query' },
  rounds: { type: 'number', default: 200, help: 'rounds; each round runs all three arms' },
  timeout: { type: 'number', default: 10000, help: 'ms per attempt' },
  'max-fail-pct': { type: 'number', default: 2, help: 'failure rate per arm above which the run is degraded' },
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
const listedSet = run.config?.https?.interception?.clients ?? [];
const listedOk = ipInList(args.listed, listedSet) && !ipInList(args.unlisted, listedSet);
run.log(`https.interception.clients = ${JSON.stringify(listedSet)} listed=${ipInList(args.listed, listedSet)} unlisted=${ipInList(args.unlisted, listedSet)}`);
if (!listedOk) run.invalid('identity precondition: exactly one of the two addresses must be in https.interception.clients', { listedSet });

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

run.finish({
  measurement: 'P2',
  origin: { name: args.origin, port: args['origin-port'], path: args.path, tls: provenance.direct.tls, alpn: provenance.direct.alpn },
  identities: { listed: args.listed, unlisted: args.unlisted, direct_arm_source: args.unlisted },
  rounds: args.rounds,
  figures,
  row_spliced_minus_direct_p50: rowValue,
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
