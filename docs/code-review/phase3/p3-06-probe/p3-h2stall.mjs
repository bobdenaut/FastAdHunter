// p3-h2stall.mjs — P3 intercepted h2 relay: throughput and per-session RSS
// under a 64-stream stall (plan §Measurements row P3; declaration delta 8).
//
// Runs on the LISTED client (its address toward the probe must be in
// https.interception.clients — checked, else INVALID). Every session is an
// h2 session to --origin through the probe's terminate leg: TLS to the probe
// :8444 with SNI = origin, --ca trusted, the served issuer must be the
// FastAdHunter CA (else INVALID).
//
// RSS arm, per run (3 stall runs interleaved with 3 matched control runs,
// S/C/S/C/S/C):
//   T0 = /telemetry            (exclusive-window proof, before the warm-up)
//   warm-up: one --warmup-path request on its own session, drained, closed
//   before = process_rss       (/api/v1/debug/memory)
//   session: --streams requests for --path opened at once
//   barrier: every stream has :status 200 AND a first DATA chunk
//            stall   -> stop reading (stream.pause()), window = --settle s
//            control -> keep draining, window = until all drained, >= --settle s
//   process_rss sampled every second inside the window; max taken
//   close everything, wait, after = process_rss, T1 = /telemetry
//   exclusive: listeners.https.connections delta == 2 (warm-up + session)
//              and counters.dns pass/allow/block flat
// delta = max(window) - before, per run; gate statistic = max stall delta
// over the runs, read against ~5.5 MiB ONLY when attribution is RESOLVED
// (min stall delta > max control delta). Otherwise UNRESOLVED: both deltas
// reported, no reading. Raw process_rss is never called "per-session RSS".
// A barrier not met, a non-exclusive window, or a missing control ->
// no RSS figure (INVALID).
//
// Throughput arm: one unstalled --path stream per run, --throughput-runs runs,
// MiB/s = bytes / (first byte -> end). Gate: median >= 50 MiB/s.
//
//   node p3-h2stall.mjs --key <file> --ca fastadhunter-ca.pem \
//     --origin h2origin.example --path /8mib.bin --warmup-path /

import fs from 'node:fs';
import tls from 'node:tls';
import http2 from 'node:http2';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, CA_ISSUER_CN, MiB, round, median, sleep, ipInList, localAddressToward, peerInfo } from './lib.mjs';

const args = parseArgs({
  origin: { type: 'string', required: true, help: 'public h2 origin' },
  path: { type: 'string', required: true, help: 'resource of --bytes MiB' },
  'warmup-path': { type: 'string', default: '/', help: 'small resource for the warm-up' },
  ca: { type: 'string', required: true, help: 'FastAdHunter CA PEM' },
  port: { type: 'number', default: 8444, help: '[https.listen] port' },
  bytes: { type: 'number', default: 8, help: 'MiB per stream (verified against content-length when present)' },
  streams: { type: 'number', default: 64, help: 'concurrent streams' },
  runs: { type: 'number', default: 3, help: 'stall runs (each followed by a control run)' },
  settle: { type: 'number', default: 30, help: 'seconds sampled inside the window' },
  'barrier-timeout': { type: 'number', default: 60, help: 'seconds for all streams to reach status + first DATA' },
  'throughput-runs': { type: 'number', default: 3, help: 'throughput arm runs' },
  arm: { type: 'string', default: 'both', help: 'rss | throughput | both' },
  'max-fail-pct': { type: 'number', default: 2, help: 'throughput arm: incomplete-stream rate above which the result is degraded (plan "all" row; the plan names no number)' },
});

const run = await new Run('p3', args, { needsHttps: true }).init();
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}
const caPem = fs.readFileSync(args.ca, 'utf8');

const local = await localAddressToward(args.probe, args.port);
const listed = run.config?.https?.interception?.clients ?? [];
run.log(`local address toward the probe: ${local}; https.interception.clients = ${JSON.stringify(listed)}`);
if (!ipInList(local, listed)) run.invalid(`this host (${local}) is not in https.interception.clients: the terminate leg is not reachable from here`);

function connectSession() {
  return new Promise((resolve, reject) => {
    let info = null;
    const session = http2.connect(`https://${args.origin}`, {
      createConnection: () =>
        tls.connect({ host: args.probe, port: args.port, servername: args.origin, ca: caPem, rejectUnauthorized: true, ALPNProtocols: ['h2'] }),
    });
    session.on('error', reject);
    session.on('connect', () => {
      info = peerInfo(session.socket);
      resolve({ session, tls: info });
    });
  });
}

function openStream(session, path, { stallAfterFirstData }) {
  const state = { path, status: null, firstData: null, bytes: 0, ended: false, error: null, contentLength: null, tOpen: performance.now(), tFirst: null, tEnd: null };
  const req = session.request({ ':path': path, ':method': 'GET', ':authority': args.origin, 'user-agent': 'fah-p3' });
  state.req = req;
  state.ready = new Promise((resolve) => {
    let settled = false;
    const done = () => {
      if (!settled) {
        settled = true;
        resolve(state);
      }
    };
    req.on('response', (h) => {
      state.status = h[':status'];
      state.contentLength = h['content-length'] !== undefined ? Number(h['content-length']) : null;
      if (state.status !== 200) done();
    });
    req.on('data', (c) => {
      state.bytes += c.length;
      const now = performance.now();
      state.tEnd = now;
      if (state.firstData === null) {
        state.firstData = c.length;
        state.tFirst = now;
        if (stallAfterFirstData) req.pause();
        done();
      }
    });
    req.on('end', () => {
      state.ended = true;
      state.tEnd = performance.now();
      done();
    });
    req.on('error', (e) => {
      state.error = e.code || e.message;
      done();
    });
    req.on('close', () => {
      if (!state.ended && state.error === null) state.error = 'closed';
      done();
    });
  });
  state.finished = new Promise((resolve) => {
    req.on('end', () => resolve(state));
    req.on('error', () => resolve(state));
    req.on('close', () => resolve(state));
  });
  return state;
}

async function warmup() {
  const { session, tls: info } = await connectSession();
  const s = openStream(session, args['warmup-path'], { stallAfterFirstData: false });
  await Promise.race([s.finished, sleep(args['barrier-timeout'] * 1000)]);
  session.close();
  await sleep(1000);
  return { status: s.status, bytes: s.bytes, error: s.error, tls: info };
}

function dnsTotal(t) {
  const d = t.counters?.dns ?? {};
  return (d.pass ?? 0) + (d.allow ?? 0) + (d.block ?? 0);
}

async function rssRun(kind, index) {
  const t0 = await run.telemetry();
  const w = await warmup();
  run.log(`${kind} run ${index}: warm-up ${w.status} ${w.bytes}B issuer=${w.tls?.issuerCN}${w.error ? ' error=' + w.error : ''}`);
  if (w.status !== 200 || w.tls?.issuerCN !== CA_ISSUER_CN) return { kind, index, valid: false, reason: `warm-up ${w.status} issuer=${w.tls?.issuerCN}`, warmup: w };
  const before = await run.processRss();
  const { session, tls: info } = await connectSession();
  const streams = Array.from({ length: args.streams }, () => openStream(session, args.path, { stallAfterFirstData: kind === 'stall' }));
  const barrier = await Promise.race([Promise.all(streams.map((s) => s.ready)).then(() => 'met'), sleep(args['barrier-timeout'] * 1000).then(() => 'timeout')]);
  const snapshot = () => streams.map((s) => ({ status: s.status, first_data: s.firstData, bytes: s.bytes, ended: s.ended, error: s.error }));
  const met = barrier === 'met' && streams.every((s) => s.status === 200 && s.firstData !== null);
  const opened = streams.filter((s) => s.status === 200 && s.firstData !== null).length;
  run.log(`${kind} run ${index}: barrier ${met ? 'MET' : 'NOT MET'} (${opened}/${args.streams} streams with :status 200 + first DATA, ${barrier})`);
  if (!met) {
    session.destroy();
    await sleep(2000);
    return { kind, index, valid: false, reason: `barrier not met: ${opened}/${args.streams} streams answered`, streams: snapshot(), before_rss: before };
  }
  const samples = [];
  const tWindow = performance.now();
  const windowOpen = () => {
    const elapsed = (performance.now() - tWindow) / 1000;
    if (kind === 'stall') return elapsed < args.settle;
    return elapsed < args.settle || !streams.every((s) => s.ended || s.error !== null);
  };
  while (windowOpen()) {
    samples.push(await run.processRss());
    await sleep(1000);
  }
  const drained = streams.filter((s) => s.ended).length;
  const bytesTotal = streams.reduce((a, s) => a + s.bytes, 0);
  for (const s of streams) s.req.close();
  session.close();
  await sleep(2000);
  session.destroy();
  const after = await run.processRss();
  const t1 = await run.telemetry();
  const connDelta = t1.listeners.https.connections - t0.listeners.https.connections;
  const dnsFlat = dnsTotal(t1) === dnsTotal(t0);
  const exclusive = connDelta === 2 && dnsFlat;
  const maxDuring = Math.max(...samples);
  const row = {
    kind,
    index,
    valid: exclusive,
    reason: exclusive ? null : `window not exclusive: connections delta ${connDelta} (expected 2), dns flat ${dnsFlat}`,
    barrier: 'met',
    tls: info,
    before_rss: before,
    max_during_rss: maxDuring,
    after_rss: after,
    delta_mib: round((maxDuring - before) / MiB),
    after_minus_before_mib: round((after - before) / MiB),
    samples_mib: samples.map((s) => round(s / MiB, 2)),
    window_s: round((performance.now() - tWindow) / 1000, 1),
    streams_drained: drained,
    bytes_total: bytesTotal,
    connections_delta: connDelta,
    dns_flat: dnsFlat,
  };
  run.raw({ measurement: 'P3-rss', ...row, streams: snapshot() });
  run.log(`${kind} run ${index}: before=${round(before / MiB, 2)} max=${round(maxDuring / MiB, 2)} after=${round(after / MiB, 2)} MiB delta=${row.delta_mib} MiB drained=${drained}/${args.streams} exclusive=${exclusive}`);
  return row;
}

async function throughputRun(index) {
  const { session, tls: info } = await connectSession();
  const s = openStream(session, args.path, { stallAfterFirstData: false });
  await Promise.race([s.finished, sleep(args['barrier-timeout'] * 1000)]);
  session.close();
  const ok = s.ended && s.error === null && s.status === 200 && (s.contentLength === null || s.contentLength === s.bytes);
  const row = {
    index,
    status: s.status,
    bytes: s.bytes,
    content_length: s.contentLength,
    ok,
    issuer: info?.issuerCN,
    mib_s: ok && s.tFirst !== null && s.tEnd !== null && s.tEnd > s.tFirst ? round(s.bytes / MiB / ((s.tEnd - s.tFirst) / 1000)) : null,
    first_byte_ms: s.tFirst !== null ? round(s.tFirst - s.tOpen) : null,
  };
  run.raw({ measurement: 'P3-throughput', ...row });
  run.log(`throughput run ${index}: ${row.ok ? row.mib_s + ' MiB/s' : 'FAILED'} bytes=${s.bytes}${s.error ? ' error=' + s.error : ''}`);
  await sleep(1000);
  return row;
}

const result = { measurement: 'P3', origin: args.origin, path: args.path, streams: args.streams, local_address: local };

if (args.arm === 'throughput' || args.arm === 'both') {
  const rows = [];
  for (let i = 1; i <= args['throughput-runs']; i += 1) rows.push(await throughputRun(i));
  if (rows.some((r) => r.issuer !== CA_ISSUER_CN)) run.invalid('throughput arm: a session was not served the FastAdHunter CA', { throughput: rows });
  const okRows = rows.filter((r) => r.ok);
  if (!okRows.length) run.invalid('throughput arm: no stream completed (the P3 BLOCKED state?)', { throughput: rows });
  const failPct = (100 * (rows.length - okRows.length)) / rows.length;
  if (failPct > args['max-fail-pct']) run.degraded(`throughput arm: ${rows.length - okRows.length} of ${rows.length} streams incomplete (${round(failPct, 1)}% over ${args['max-fail-pct']}%); figures from the completed ones`);
  const med = median(okRows.map((r) => r.mib_s));
  const expected = args.bytes * MiB;
  if (okRows.some((r) => r.bytes !== expected)) run.degraded(`throughput stream bytes ${okRows.map((r) => r.bytes).join(',')} differ from --bytes ${expected}`);
  result.throughput = { runs: rows, median_mib_s: round(med), gate: { statistic: 'median of runs >= 50 MiB/s', median_mib_s: round(med), pass: med !== null && med >= 50 } };
}

if (args.arm === 'rss' || args.arm === 'both') {
  const rows = [];
  for (let i = 1; i <= args.runs; i += 1) {
    rows.push(await rssRun('stall', i));
    rows.push(await rssRun('control', i));
  }
  const bad = rows.filter((r) => !r.valid);
  if (bad.length) run.invalid(`RSS arm: ${bad.length} run(s) produced no figure: ${bad.map((r) => `${r.kind}#${r.index} ${r.reason}`).join('; ')}`, { ...result, rss_runs: rows });
  const stall = rows.filter((r) => r.kind === 'stall').map((r) => r.delta_mib);
  const control = rows.filter((r) => r.kind === 'control').map((r) => r.delta_mib);
  const resolved = Math.min(...stall) > Math.max(...control);
  result.rss = {
    runs: rows.map(({ samples_mib, ...r }) => r),
    stall_delta_mib: stall,
    control_delta_mib: control,
    max_stall_delta_mib: Math.max(...stall),
    max_control_delta_mib: Math.max(...control),
    attribution: resolved ? 'RESOLVED' : 'UNRESOLVED',
    gate: {
      statistic: 'max stall delta over runs vs ~5.5 MiB, read only when RESOLVED',
      attribution: resolved ? 'RESOLVED' : 'UNRESOLVED',
      max_stall_delta_mib: Math.max(...stall),
      reading_against_5_5_mib: resolved ? (Math.max(...stall) <= 5.5 ? 'within' : 'above — a finding, not a fail') : 'not read (stall delta inside the control / allocator band)',
    },
  };
}

run.finish({ ...result, gate: { throughput: result.throughput?.gate ?? null, rss: result.rss?.gate ?? null } });
