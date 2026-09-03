// p5-mint.mjs — P5 cold `prewarm` per first-sight host over DoT (plan
// §Measurements row P5; declaration deltas 3 and 6).
//
// State the script configures, not assumes (else INVALID before any
// handshake): a CA present in the probe's store; leaf_cache.size + --hosts
// <= leaf_cache.capacity, so the repeat pass evicts nothing.
// Two passes over --hosts synthetic names (syntactically valid, never
// resolved): first-sight pass — one TLS handshake to :853 per host with the
// host as SNI (the listener pre-warms the leaf before the handshake, so the
// mint is inside the timing); repeat pass — the same handshake again, leaf
// already cached. Handshake = TCP connect -> secureConnect; the
// spawn_blocking hop is paid by both arms and cancels.
// Figure: median(first-sight) - median(repeat), an incremental p50 estimate of
// mint + insert. Gate: < 1 ms.
// Invalidity: minted_total delta must equal --hosts; evictions delta 0; every
// row's served issuer = FastAdHunter CA (the fallback API certificate on any
// row means no CA in the store); the repeat arm ran; unwarmed_misses delta
// reported.
//
//   node p5-mint.mjs --key <file> --hosts 256

import tls from 'node:tls';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, CA_ISSUER_CN, summary, round, stamp, peerInfo } from './lib.mjs';

const args = parseArgs({
  hosts: { type: 'number', default: 256, help: 'first-sight hosts (below LEAF_CACHE_CAPACITY minus what is cached)' },
  seed: { type: 'string', default: stamp(), help: 'host name seed; a new seed makes every host first-sight again' },
  'dot-port': { type: 'number', default: 853, help: 'DoT port' },
  timeout: { type: 'number', default: 10000, help: 'ms per handshake' },
});

const run = await new Run('p5', args, { needsHttps: true }).init();
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running (plan §Running item 3); --allow-busy to proceed degraded`);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}
const before = await run.certificates();
run.log(`certificates before: ca.present=${before.ca?.present} dot=${JSON.stringify(before.dot)} leaf_cache=${JSON.stringify(before.leaf_cache)}`);
if (!before.ca?.present) run.invalid('no CA in the probe store: nothing would be minted (POST /api/v1/certificates/ca/generate first)');
if (before.dot?.state !== 'listening') run.invalid(`DoT listener is ${JSON.stringify(before.dot)}`);
const lc = before.leaf_cache;
if (lc.size + args.hosts > lc.capacity) run.invalid(`leaf_cache.size ${lc.size} + ${args.hosts} hosts exceeds capacity ${lc.capacity}: the repeat pass would evict`);
if (lc.inflight !== 0) run.degraded(`leaf_cache.inflight = ${lc.inflight} before the run`);

const seed = args.seed.toLowerCase().replace(/[^a-z0-9]/g, '');
const hosts = Array.from({ length: args.hosts }, (_, i) => `p5-${seed}-${i}.mint.test`);

function handshake(host) {
  return new Promise((resolve) => {
    let tConnect = null;
    let settled = false;
    const finish = (error, info = null, ms = null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      sock.destroy();
      resolve({ host, error, handshake_ms: ms, issuer: info?.issuerCN ?? null, subject: info?.subjectCN ?? null, tls: info?.tls ?? null });
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
  for (const h of hosts) {
    const r = await handshake(h);
    r.arm = arm;
    rows.push(r);
    run.raw({ measurement: 'P5', ...r });
  }
  const ok = rows.filter((r) => r.error === null);
  run.log(`${arm}: ${ok.length}/${rows.length} handshakes, p50=${summary(ok.map((r) => r.handshake_ms)).p50} ms, issuers=${[...new Set(ok.map((r) => r.issuer))].join('|')}, errors=${rows.length - ok.length}`);
  return rows;
}

const first = await pass('first-sight');
const mid = (await run.certificates()).leaf_cache;
const repeat = await pass('repeat');
const after = (await run.certificates()).leaf_cache;
const delta = Object.fromEntries(Object.keys(after).map((k) => [k, after[k] - lc[k]]));
const repeatDelta = Object.fromEntries(Object.keys(after).map((k) => [k, after[k] - mid[k]]));
run.log(`leaf_cache delta whole run ${JSON.stringify(delta)}; repeat pass ${JSON.stringify(repeatDelta)}`);

const all = [...first, ...repeat];
const failed = all.filter((r) => r.error !== null);
const fallback = all.filter((r) => r.error === null && r.issuer !== CA_ISSUER_CN);
const invalidReasons = [];
if (failed.length) invalidReasons.push(`${failed.length} handshakes failed (${[...new Set(failed.map((r) => r.error))].join(',')})`);
if (fallback.length) invalidReasons.push(`${fallback.length} rows served issuer ${[...new Set(fallback.map((r) => r.issuer))].join('|')}, not the CA (fallback certificate => no usable CA in the store)`);
if (delta.minted_total !== args.hosts) invalidReasons.push(`minted_total moved by ${delta.minted_total}, expected ${args.hosts}`);
if (repeatDelta.evictions !== 0) invalidReasons.push(`evictions moved by ${repeatDelta.evictions} across the repeat pass`);
if (repeatDelta.minted_total !== 0) invalidReasons.push(`repeat pass minted ${repeatDelta.minted_total} leaves`);
if (invalidReasons.length) run.invalid(invalidReasons.join('; '), { leaf_cache_before: lc, leaf_cache_after: after, delta, repeat_delta: repeatDelta, rows: all });

const f = summary(first.map((r) => r.handshake_ms));
const rp = summary(repeat.map((r) => r.handshake_ms));
const figure = round(f.p50 - rp.p50);

run.finish({
  measurement: 'P5',
  hosts: args.hosts,
  seed,
  figures: {
    'first-sight': { ...f, served_issuer: CA_ISSUER_CN },
    repeat: { ...rp, served_issuer: CA_ISSUER_CN },
    counters_delta: { minted_total: delta.minted_total, unwarmed_misses: delta.unwarmed_misses, evictions: delta.evictions, superseded: delta.superseded, prewarm_hits: delta.prewarm_hits, size: delta.size },
  },
  leaf_cache_before: lc,
  leaf_cache_after: after,
  gate: { statistic: 'p50(first-sight) - p50(repeat) < 1 ms (incremental p50 estimate of mint + insert)', value_ms: figure, pass: figure !== null && figure < 1 },
});
