// p5-paired-diag.mjs — DIAGNOSTIC, not a stage. P5's arms, paired per host.
//
// p5-mint.mjs and p5-conc-diag.mjs run every first-sight handshake first and
// every repeat handshake afterwards. C0/C1 (2026-09-04, RB5009, fah-probe
// a2d0802) with a read-only cpu-frequency poll beside them showed the device
// clock at 350 MHz through the whole first-sight pass, starting from idle, and
// at 1400/700 through the repeat pass; beside a background load that kept the
// clock off 350 the incremental fell from 1.393 to 0.817 ms. Arm order and the
// arms' different duty cycles pick the clock regime, so the two medians are
// not taken at the same clock.
//
// This script removes the order: for each host, one first-sight handshake and
// then, immediately, its repeat. Both arms sample the same clock regime and
// the same duty cycle. Figures: the median of the per-pair differences, and
// median(first-sight) - median(repeat) beside it for comparison with P5.
//
// No gate. Same preconditions as p5-conc-diag.mjs: CA in the store, DoT
// listening, size + hosts <= capacity so nothing evicts.
//
//   node p5-paired-diag.mjs --key <file> --hosts 240

import tls from 'node:tls';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, CA_ISSUER_CN, summary, round, stamp, peerInfo, median } from './lib.mjs';

const args = parseArgs({
  hosts: { type: 'number', default: 240, help: 'first-sight hosts, one pair each' },
  seed: { type: 'string', default: stamp(), help: 'host name seed; a new seed makes every host first-sight again' },
  'dot-port': { type: 'number', default: 853, help: 'DoT port' },
  timeout: { type: 'number', default: 10000, help: 'ms per handshake' },
});

const run = await new Run('p5-paired', args, { needsHttps: true }).init();
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running; --allow-busy to proceed degraded`);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}

const before = await run.certificates();
run.log(`certificates before: ca.present=${before.ca?.present} leaf_cache=${JSON.stringify(before.leaf_cache)}`);
if (!before.ca?.present) run.invalid('no CA in the probe store: nothing would be minted');
if (before.dot?.state !== 'listening') run.invalid(`DoT listener is ${JSON.stringify(before.dot)}`);
const lc = before.leaf_cache;
if (lc.size + args.hosts > lc.capacity) run.invalid(`leaf_cache.size ${lc.size} + ${args.hosts} hosts exceeds capacity ${lc.capacity}: restart the probe to purge the cache first`);

const seed = args.seed.toLowerCase().replace(/[^a-z0-9]/g, '');
const hosts = Array.from({ length: args.hosts }, (_, i) => `p5p-${seed}-${i}.mint.test`);

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

const rows = [];
const pairs = [];
const t0 = performance.now();
for (const [i, h] of hosts.entries()) {
  const first = await handshake(h);
  const repeat = await handshake(h);
  first.arm = 'first-sight';
  repeat.arm = 'repeat';
  first.pair = i;
  repeat.pair = i;
  rows.push(first, repeat);
  run.raw({ measurement: 'P5-paired', ...first });
  run.raw({ measurement: 'P5-paired', ...repeat });
  if (first.error === null && repeat.error === null) pairs.push({ pair: i, diff_ms: round(first.handshake_ms - repeat.handshake_ms) });
}
const wall = round(performance.now() - t0);

const after = (await run.certificates()).leaf_cache;
const delta = Object.fromEntries(Object.keys(after).map((k) => [k, after[k] - lc[k]]));
run.log(`leaf_cache delta ${JSON.stringify(delta)}`);

const failed = rows.filter((r) => r.error !== null);
const fallback = rows.filter((r) => r.error === null && r.issuer !== CA_ISSUER_CN);
const invalidReasons = [];
if (failed.length) invalidReasons.push(`${failed.length} handshakes failed (${[...new Set(failed.map((r) => r.error))].join(',')})`);
if (fallback.length) invalidReasons.push(`${fallback.length} rows served issuer ${[...new Set(fallback.map((r) => r.issuer))].join('|')}, not the CA`);
if (delta.minted_total !== args.hosts) invalidReasons.push(`minted_total moved by ${delta.minted_total}, expected ${args.hosts}`);
if (delta.evictions !== 0) invalidReasons.push(`evictions moved by ${delta.evictions}`);
if (invalidReasons.length) run.invalid(invalidReasons.join('; '), { leaf_cache_before: lc, leaf_cache_after: after, delta });

const firstMs = rows.filter((r) => r.arm === 'first-sight').map((r) => r.handshake_ms);
const repeatMs = rows.filter((r) => r.arm === 'repeat').map((r) => r.handshake_ms);
const diffs = pairs.map((p) => p.diff_ms);
const pairedMedian = round(median(diffs));
const unpaired = round(median(firstMs) - median(repeatMs));
const under1ms = pairs.filter((p) => p.diff_ms < 1).length;

run.log(`pairs=${pairs.length} wall=${wall} ms; first-sight p50=${round(median(firstMs))} ms, repeat p50=${round(median(repeatMs))} ms`);
run.log(`paired median(first - repeat) = ${pairedMedian} ms; median(first) - median(repeat) = ${unpaired} ms; pairs under 1 ms: ${under1ms}/${pairs.length}`);

run.finish({
  measurement: 'P5-paired (diagnostic)',
  hosts: args.hosts,
  seed,
  figures: {
    paired_median_ms: pairedMedian,
    unpaired_incremental_ms: unpaired,
    pairs_under_1ms: under1ms,
    pairs: pairs.length,
    wall_ms: wall,
    first_sight: summary(firstMs),
    repeat: summary(repeatMs),
    diff: summary(diffs),
    leaf_cache_before: lc,
    leaf_cache_after: after,
    delta,
  },
});
