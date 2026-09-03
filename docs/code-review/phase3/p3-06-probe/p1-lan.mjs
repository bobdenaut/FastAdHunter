// p1-lan.mjs — P1-LAN splice throughput and P1-control (plan §Measurements
// rows P1-LAN, P1-control; §Choosing SPLICE_BUF step 4).
//
// Arms (one per invocation, labelled in the result):
//   spliced    (default)         client -> probe :8444 (SNI = origin) -> origin :443
//   p1_control (--direct)        client -> origin :443, same origin process
//   aggregate  (--connections 8) 8 concurrent spliced connections, sum MiB/s;
//                                pair it with a /tool/profile read (--profile-share)
// Per connection the origin (p1-origin.mjs) sends --bytes MiB and closes.
// Steady-state MiB/s = bytes / (last byte - first byte): connect, ClientHello
// and teardown are outside it (p3-03 M4). Total MiB/s (bytes / (TCP connect
// -> last byte)) is reported beside it as a diagnostic.
//
// Gate (single connection): median of --runs steady MiB/s >= 100 AND inside
// P1-control's min-max. The band comes from a P1-control result file
// (--control <p1-control.json>); without it the band half is "not evaluated".
// Invalidity: a run whose byte count differs from --bytes MiB is INVALID; the
// spliced arm also reads listeners.https before/after and attributes a
// handshake failure to resolve_failures / refused_destination /
// upstream_failures (plan §Traps: nip.io through the container's resolver).
//
//   node p1-lan.mjs --key <file> --origin 192-168-10-20.nip.io --origin-cert p1-origin.crt
//   node p1-lan.mjs --key <file> --origin 192-168-10-20.nip.io --origin-cert p1-origin.crt --direct
//   node p1-lan.mjs --key <file> --origin ... --origin-cert ... --connections 8 --profile-share "fah-probe 61%"

import fs from 'node:fs';
import tls from 'node:tls';
import dns from 'node:dns/promises';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, MiB, round, summary, median, peerInfo, ipInList } from './lib.mjs';

const args = parseArgs({
  origin: { type: 'string', required: true, help: 'origin name (publicly resolvable, SAN of --origin-cert)' },
  'origin-ip': { type: 'string', default: null, help: 'origin address for --direct (default: resolve --origin locally)' },
  'origin-port': { type: 'number', default: 443, help: 'origin port' },
  'origin-cert': { type: 'string', default: null, help: 'PEM the origin serves; trusted as the only CA. Absent: unverified, fingerprint recorded' },
  port: { type: 'number', default: 8444, help: '[https.listen] port on the probe' },
  bytes: { type: 'number', default: 64, help: 'MiB the origin serves per connection (must match p1-origin.mjs)' },
  runs: { type: 'number', default: 5, help: 'runs' },
  connections: { type: 'number', default: 1, help: 'concurrent connections per run (8 = aggregate arm)' },
  direct: { type: 'boolean', default: false, help: 'P1-control: bypass the probe' },
  control: { type: 'string', default: null, help: 'p1-control.json to read the band from' },
  'profile-share': { type: 'string', default: null, help: '/tool/profile share read during the aggregate arm, verbatim' },
  timeout: { type: 'number', default: 120000, help: 'ms per connection' },
  'max-fail-pct': { type: 'number', default: 2, help: 'incomplete-run rate above which the result is degraded (plan "all" row; the plan names no number)' },
});

const arm = args.direct ? 'p1_control' : args.connections > 1 ? 'aggregate' : 'spliced';
const resultName = args.direct ? 'p1-control.json' : args.connections > 1 ? 'p1-aggregate.json' : 'p1.json';
const run = await new Run('p1', args, { needsHttps: !args.direct }).init();
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`, {}, resultName);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}
const expected = args.bytes * MiB;
const ca = args['origin-cert'] ? fs.readFileSync(args['origin-cert']) : undefined;
if (!ca) run.degraded('origin certificate not verified (--origin-cert absent)');

let target;
const originIp = args['origin-ip'] ?? (await dns.lookup(args.origin, { family: 4 }).then((r) => r.address).catch(() => null));
run.log(`origin ${args.origin} resolves locally to ${originIp ?? 'nothing'}`);
if (args.direct) {
  if (!originIp) run.invalid(`--origin does not resolve locally and --origin-ip is absent`, {}, resultName);
  target = { host: originIp, port: args['origin-port'] };
} else {
  const allowed = run.config?.egress?.allow_destinations ?? [];
  run.log(`egress.allow_destinations = ${JSON.stringify(allowed)}`);
  if (originIp && !ipInList(originIp, allowed)) run.invalid(`origin address ${originIp} is not in egress.allow_destinations: the probe refuses the destination (plan §Environment)`, { allowed }, resultName);
  target = { host: args.probe, port: args.port };
}
run.log(`arm=${arm} target=${target.host}:${target.port} sni=${args.origin} expected=${args.bytes} MiB x ${args.connections} conn x ${args.runs} runs`);

function pull(index) {
  return new Promise((resolve) => {
    const t = { start: performance.now(), connect: null, secure: null, first: null, last: null };
    let bytes = 0;
    let info = null;
    let settled = false;
    const finish = (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      sock.destroy();
      resolve({
        index,
        error,
        bytes,
        tls: info,
        handshake_ms: t.connect !== null && t.secure !== null ? round(t.secure - t.connect) : null,
        first_byte_ms: t.secure !== null && t.first !== null ? round(t.first - t.secure) : null,
        steady_ms: t.first !== null && t.last !== null ? round(t.last - t.first) : null,
        total_ms: t.connect !== null && t.last !== null ? round(t.last - t.connect) : null,
        t_first: t.first,
        t_last: t.last,
      });
    };
    const timer = setTimeout(() => finish('timeout'), args.timeout);
    const sock = tls.connect({
      host: target.host,
      port: target.port,
      servername: args.origin,
      ca,
      rejectUnauthorized: Boolean(ca),
      ALPNProtocols: undefined,
    });
    sock.setNoDelay(true);
    sock.on('connect', () => {
      t.connect = performance.now();
    });
    sock.on('secureConnect', () => {
      t.secure = performance.now();
      info = peerInfo(sock);
    });
    sock.on('data', (c) => {
      const now = performance.now();
      if (t.first === null) t.first = now;
      t.last = now;
      bytes += c.length;
    });
    const closeReason = () => (bytes === expected ? null : t.secure === null ? 'closed_before_handshake' : bytes === 0 ? 'closed_zero_bytes' : 'closed_early');
    sock.on('end', () => finish(closeReason()));
    sock.on('error', (e) => finish(e.code || e.message));
    sock.on('close', () => finish(closeReason()));
  });
}

const before = args.direct ? null : (await run.telemetry()).listeners.https;
const runs = [];
for (let i = 1; i <= args.runs; i += 1) {
  const conns = await Promise.all(Array.from({ length: args.connections }, (_, k) => pull(k)));
  const ok = conns.filter((c) => c.error === null && c.bytes === expected);
  const firsts = conns.map((c) => c.t_first).filter((x) => x !== null);
  const lasts = conns.map((c) => c.t_last).filter((x) => x !== null);
  const aggregateMs = firsts.length && lasts.length ? Math.max(...lasts) - Math.min(...firsts) : null;
  const row = {
    run: i,
    connections: conns.length,
    ok: ok.length,
    bytes_total: conns.reduce((a, c) => a + c.bytes, 0),
    steady_mib_s: conns.length === 1 && ok.length === 1 ? round(args.bytes / (ok[0].steady_ms / 1000)) : null,
    total_mib_s: conns.length === 1 && ok.length === 1 ? round(args.bytes / (ok[0].total_ms / 1000)) : null,
    aggregate_mib_s: aggregateMs && ok.length === conns.length ? round((args.bytes * conns.length) / (aggregateMs / 1000)) : null,
    handshake_ms: summary(conns.map((c) => c.handshake_ms)),
    first_byte_ms: summary(conns.map((c) => c.first_byte_ms)),
    errors: conns.filter((c) => c.error !== null).map((c) => ({ index: c.index, error: c.error, bytes: c.bytes })),
    tls: conns[0].tls,
  };
  runs.push(row);
  run.raw({ measurement: 'P1', arm, ...row, per_connection: conns.map(({ t_first, t_last, ...c }) => c) });
  run.log(`run ${i}: ok=${row.ok}/${row.connections} steady=${row.steady_mib_s ?? '-'} MiB/s total=${row.total_mib_s ?? '-'} aggregate=${row.aggregate_mib_s ?? '-'} hs=${row.handshake_ms.p50}ms${row.errors.length ? ' errors=' + JSON.stringify(row.errors) : ''}`);
}
const after = args.direct ? null : (await run.telemetry()).listeners.https;
const delta = before ? Object.fromEntries(Object.keys(before).map((k) => [k, (after[k] ?? 0) - (before[k] ?? 0)])) : null;
if (delta) run.log(`listeners.https delta ${JSON.stringify(delta)}`);

const completed = runs.filter((r) => r.ok === r.connections);
const failed = runs.length - completed.length;
if (!completed.length) {
  run.invalid(`no ${arm} sample completed ${args.bytes} MiB on every connection`, { arm, runs, telemetry_delta: delta }, resultName);
}
const failPct = (100 * failed) / runs.length;
if (failPct > args['max-fail-pct']) run.degraded(`${failed} of ${runs.length} runs incomplete (${round(failPct, 1)}% over ${args['max-fail-pct']}%); figures from the ${completed.length} completed runs`);
const issuer = completed[0].tls?.issuerCN ?? null;
if (!args.direct && ca && runs.some((r) => r.tls?.authorized === false)) run.invalid('origin certificate did not verify through the splice', { arm, runs }, resultName);

let band = null;
if (args.control) {
  const c = JSON.parse(fs.readFileSync(args.control, 'utf8'));
  band = c.figures?.steady_mib_s ? { min: c.figures.steady_mib_s.min, p50: c.figures.steady_mib_s.p50, max: c.figures.steady_mib_s.max, source: args.control } : null;
}
const steady = summary(runs.map((r) => r.steady_mib_s));
const total = summary(runs.map((r) => r.total_mib_s));
const aggregate = summary(runs.map((r) => r.aggregate_mib_s));
const gate =
  arm === 'spliced'
    ? {
        statistic: 'median of runs steady MiB/s >= 100 and inside P1-control min-max',
        median_steady_mib_s: steady.p50,
        at_least_100: steady.p50 !== null && steady.p50 >= 100,
        band,
        inside_band: band ? steady.p50 >= band.min && steady.p50 <= band.max : 'not evaluated (--control absent)',
        pass: band ? steady.p50 >= 100 && steady.p50 >= band.min && steady.p50 <= band.max : null,
      }
    : arm === 'p1_control'
      ? { statistic: 'none — this arm is the band', band: { min: steady.min, p50: steady.p50, max: steady.max } }
      : { statistic: 'none — aggregate arm is diagnostic; CPU axis from /tool/profile', aggregate_median_mib_s: aggregate.p50, profile_share: args['profile-share'] };
if (arm === 'aggregate' && !args['profile-share']) run.degraded('aggregate arm without a /tool/profile share (--profile-share): throughput of the loop only');

run.finish(
  {
    measurement: arm === 'p1_control' ? 'P1-control' : 'P1-LAN',
    arm,
    origin: args.origin,
    target,
    bytes_mib: args.bytes,
    connections: args.connections,
    served_issuer: issuer,
    figures: { steady_mib_s: steady, total_mib_s: total, aggregate_mib_s: aggregate, median_steady_mib_s: median(runs.map((r) => r.steady_mib_s)) },
    telemetry_delta: delta,
    runs,
    gate,
  },
  resultName,
);
