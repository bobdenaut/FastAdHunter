// p5-conc-diag.mjs — DIAGNOSTIC, not a stage. Segments P5's unexplained gap.
//
// Standing evidence (2026-09-04, RB5009, tip a2d0802):
//   certs_mint          450.88 us   in-process, criterion, D11-on-device
//   certs_prewarm_warm    0.563 us  in-process, a warm pre-warm is ~free
//   P5 incremental     1389 us      p50(first-sight) - p50(repeat) over DoT
// so ~938 us of P5 is not the mint. The plan assumed the spawn_blocking hop
// "is paid by both arms and cancels"; certs_prewarm_warm says the repeat arm
// pays almost nothing, so it cannot cancel a dispatch the first-sight arm
// makes and waits on.
//
// This script changes ONE variable: how many first-sight handshakes are in
// flight. Same instrument as p5-mint.mjs (TCP connect -> secureConnect to
// :853, host as SNI), same host count, fresh seed per invocation.
//
//   --conc 1   fully serial: dispatch latency is paid once per handshake
//   --conc 8   eight in flight: dispatch and worker wake-up amortise
//
// Reading it:
//   incremental collapses toward ~0.5 ms at conc 8  => dispatch / wake / queueing
//   incremental stays ~1.3-1.4 ms at conc 8         => serial work certs_mint
//                                                      does not cover; instrument
//                                                      the listener next
// Pass wall-clock is reported beside the percentiles: at conc 8 a throughput
// that scales with concurrency says the workers were idle, not saturated.
//
// The leaf cache holds 512 entries. Two 256-host arms fill it exactly, so run
// POST /api/v1/certificates/ca/generate between arms - it replaces the CA and
// purges the cache - and watch the reported evictions delta, which must stay 0.
// Each generate costs one ca-archive slot (MAX_ARCHIVES = 8).
//
// No gate. Nothing here decides anything about fah-certs, whose own on-device
// measurement is inside budget.
//
//   node p5-conc-diag.mjs --key <file> --conc 1
//   node p5-conc-diag.mjs --key <file> --conc 8

import tls from 'node:tls';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, CA_ISSUER_CN, summary, round, stamp, peerInfo } from './lib.mjs';

const args = parseArgs({
  hosts: { type: 'number', default: 256, help: 'first-sight hosts per arm' },
  conc: { type: 'number', default: 1, help: 'handshakes in flight' },
  seed: { type: 'string', default: stamp(), help: 'host name seed; a new seed makes every host first-sight again' },
  'dot-port': { type: 'number', default: 853, help: 'DoT port' },
  timeout: { type: 'number', default: 10000, help: 'ms per handshake' },
});

const run = await new Run('p5-conc', args, { needsHttps: true }).init();
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}
const before = await run.certificates();
run.log(`certificates before: ca.present=${before.ca?.present} leaf_cache=${JSON.stringify(before.leaf_cache)}`);
if (!before.ca?.present) run.invalid('no CA in the probe store: nothing would be minted');
if (before.dot?.state !== 'listening') run.invalid(`DoT listener is ${JSON.stringify(before.dot)}`);
const lc = before.leaf_cache;
if (lc.size + args.hosts > lc.capacity) run.invalid(`leaf_cache.size ${lc.size} + ${args.hosts} hosts exceeds capacity ${lc.capacity}: regenerate the CA to purge the cache first`);

const seed = args.seed.toLowerCase().replace(/[^a-z0-9]/g, '');
const hosts = Array.from({ length: args.hosts }, (_, i) => `p5c-${seed}-${i}.mint.test`);

function handshake(host) {
  return new Promise((resolve) => {
    let tConnect = null;
    let settled = false;
    const finish = (error, info = null, ms = null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (error === null) sock.end();
      else sock.destroy();
      resolve({ host, error, handshake_ms: ms, issuer: info?.issuerCN ?? null, tls: info?.tls ?? null });
    };
    const timer = setTimeout(() => finish('timeout'), args.timeout);
    const sock = tls.connect({ host: args.probe, port: args['dot-port'], servername: host, rejectUnauthorized: false });
    sock.setNoDelay(true);
    sock.on('connect', () => {
      tConnect = performance.now();
    });
    sock.on('secureConnect', () => {
      const ms = tConnect === null ? null : performance.now() - tConnect;
      finish(null, peerInfo(sock), round(ms));
    });
    sock.on('error', (e) => finish(e.code || e.message));
    sock.on('close', () => finish('closed'));
  });
}

async function pass(arm) {
  const rows = [];
  const queue = [...hosts];
  const t0 = performance.now();
  const workers = Array.from({ length: Math.max(1, args.conc) }, async () => {
    for (;;) {
      const h = queue.shift();
      if (h === undefined) return;
      const r = await handshake(h);
      r.arm = arm;
      rows.push(r);
      run.raw({ measurement: 'P5-conc', conc: args.conc, ...r });
    }
  });
  await Promise.all(workers);
  const wall = round(performance.now() - t0);
  const ok = rows.filter((r) => r.error === null);
  const s = summary(ok.map((r) => r.handshake_ms));
  run.log(`${arm} conc=${args.conc}: ${ok.length}/${rows.length} handshakes, p50=${s.p50} ms, wall=${wall} ms, ${round((ok.length / wall) * 1000)} handshakes/s, issuers=${[...new Set(ok.map((r) => r.issuer))].join('|')}, errors=${rows.length - ok.length}`);
  return { rows, wall, summary: s };
}

const first = await pass('first-sight');
const mid = (await run.certificates()).leaf_cache;
const repeat = await pass('repeat');
const after = (await run.certificates()).leaf_cache;
const delta = Object.fromEntries(Object.keys(after).map((k) => [k, after[k] - lc[k]]));
const repeatDelta = Object.fromEntries(Object.keys(after).map((k) => [k, after[k] - mid[k]]));
run.log(`leaf_cache delta whole run ${JSON.stringify(delta)}; repeat pass ${JSON.stringify(repeatDelta)}`);

const all = [...first.rows, ...repeat.rows];
const failed = all.filter((r) => r.error !== null);
const fallback = all.filter((r) => r.error === null && r.issuer !== CA_ISSUER_CN);
const invalidReasons = [];
if (failed.length) invalidReasons.push(`${failed.length} handshakes failed (${[...new Set(failed.map((r) => r.error))].join(',')})`);
if (fallback.length) invalidReasons.push(`${fallback.length} rows served issuer ${[...new Set(fallback.map((r) => r.issuer))].join('|')}, not the CA`);
if (delta.minted_total !== args.hosts) invalidReasons.push(`minted_total moved by ${delta.minted_total}, expected ${args.hosts}`);
if (repeatDelta.evictions !== 0) invalidReasons.push(`evictions moved by ${repeatDelta.evictions} across the repeat pass`);
if (repeatDelta.minted_total !== 0) invalidReasons.push(`repeat pass minted ${repeatDelta.minted_total} leaves`);
if (invalidReasons.length) run.invalid(invalidReasons.join('; '), { leaf_cache_before: lc, leaf_cache_after: after, delta, repeat_delta: repeatDelta });

const incremental = round(first.summary.p50 - repeat.summary.p50);

run.finish({
  measurement: 'P5-conc (diagnostic)',
  hosts: args.hosts,
  conc: args.conc,
  seed,
  figures: {
    'first-sight': { ...first.summary, wall_ms: first.wall, handshakes_per_s: round((args.hosts / first.wall) * 1000) },
    repeat: { ...repeat.summary, wall_ms: repeat.wall, handshakes_per_s: round((args.hosts / repeat.wall) * 1000) },
    incremental_p50_ms: incremental,
    counters_delta: { minted_total: delta.minted_total, unwarmed_misses: delta.unwarmed_misses, evictions: delta.evictions, prewarm_hits: delta.prewarm_hits },
  },
  leaf_cache_before: lc,
  leaf_cache_after: after,
  gate: { statistic: 'none — diagnostic (P5 path segmentation)', incremental_p50_ms: incremental, conc: args.conc },
});
