// p5-clock-load.mjs — DIAGNOSTIC, not a stage. Background load for the P5
// clock-regime control (C1).
//
// P5-diag's server-side intervals sit on discrete levels at ratios ~1:2:4 in
// every interval including the pure kernel wake path (dispatch_wait 40 / 72 /
// 136 us), and the repeat arm shows the same levels with no mint in it. That
// reads as the device clock stepping under a light conc-1 workload, not as a
// cost the first-sight arm pays after its mint. This script keeps the probe
// busy with repeat DoT handshakes to hosts it has already cached, so the clock
// stays up while p5-conc-diag.mjs runs beside it at --conc 1.
//
// It mints its own hosts first (fresh seed, --hosts of them), then only
// repeats: the diag run's minted_total delta stays exactly its own host count
// and its evictions delta stays 0 as long as size + hosts <= capacity.
//
//   --rate N   paced, N handshakes/s, at most --conc in flight
//   --rate 0   closed loop, --conc handshakes always in flight (default)
//
// No gate. Reading the pair C0 (diag alone) / C1 (diag beside this load):
//   C1 incremental falls to the fast-regime floor (~0.5-0.8 ms) and the
//   per-connection sequence loses its block structure => clock-driven
//   C1 incremental stays ~1.3 ms                         => not the clock
//
//   node p5-clock-load.mjs --key <file> --hosts 16 --conc 2 --duration 60

import tls from 'node:tls';
import { performance } from 'node:perf_hooks';
import { parseArgs, Run, CA_ISSUER_CN, summary, round, stamp, peerInfo, sleep } from './lib.mjs';

const args = parseArgs({
  hosts: { type: 'number', default: 16, help: 'hosts to mint once, then cycle through' },
  conc: { type: 'number', default: 2, help: 'handshakes in flight' },
  rate: { type: 'number', default: 0, help: 'handshakes/s; 0 = closed loop at --conc' },
  duration: { type: 'number', default: 60, help: 'seconds of load after the mint pass' },
  seed: { type: 'string', default: stamp(), help: 'host name seed; a new seed makes every host first-sight again' },
  'dot-port': { type: 'number', default: 853, help: 'DoT port' },
  timeout: { type: 'number', default: 10000, help: 'ms per handshake' },
});

const run = await new Run('p5-clock-load', args, { needsHttps: true }).init();
if (run.host.idle === false) {
  if (!args['allow-busy']) run.invalid(`host not idle: ${run.host.busy.join(',')} running; --allow-busy to proceed degraded`);
  run.degraded(`host not idle: ${run.host.busy.join(',')}`);
}

const before = await run.certificates();
run.log(`certificates before: ca.present=${before.ca?.present} leaf_cache=${JSON.stringify(before.leaf_cache)}`);
if (!before.ca?.present) run.invalid('no CA in the probe store: nothing would be minted');
if (before.dot?.state !== 'listening') run.invalid(`DoT listener is ${JSON.stringify(before.dot)}`);
const lc = before.leaf_cache;
if (lc.size + args.hosts > lc.capacity) run.invalid(`leaf_cache.size ${lc.size} + ${args.hosts} hosts exceeds capacity ${lc.capacity}`);

const seed = args.seed.toLowerCase().replace(/[^a-z0-9]/g, '');
const hosts = Array.from({ length: args.hosts }, (_, i) => `p5l-${seed}-${i}.mint.test`);

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
      resolve({ host, error, handshake_ms: ms, issuer: info?.issuerCN ?? null });
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

const mintRows = [];
for (const h of hosts) mintRows.push(await handshake(h));
const mintErrors = mintRows.filter((r) => r.error !== null || r.issuer !== CA_ISSUER_CN);
const afterMint = (await run.certificates()).leaf_cache;
run.log(`mint pass: ${mintRows.length - mintErrors.length}/${mintRows.length} ok, minted_total delta ${afterMint.minted_total - lc.minted_total} (expected ${args.hosts})`);
if (mintErrors.length) run.invalid(`${mintErrors.length} mint handshakes failed or served the fallback certificate`);
if (afterMint.minted_total - lc.minted_total !== args.hosts) run.invalid(`mint pass minted ${afterMint.minted_total - lc.minted_total}, expected ${args.hosts}`);

const period = args.rate > 0 ? 1000 / args.rate : 0;
const t0 = performance.now();
const end = t0 + args.duration * 1000;
let inflight = 0;
let fired = 0;
let done = 0;
let errors = 0;
const windowMs = [];
let windowStart = t0;
let nextSlot = t0;

function fire() {
  const h = hosts[fired % hosts.length];
  fired += 1;
  inflight += 1;
  handshake(h).then((r) => {
    inflight -= 1;
    done += 1;
    if (r.error !== null || r.issuer !== CA_ISSUER_CN) errors += 1;
    else windowMs.push(r.handshake_ms);
  });
}

run.log(`load: conc=${args.conc} rate=${args.rate === 0 ? 'closed-loop' : `${args.rate}/s`} duration=${args.duration}s over ${hosts.length} cached hosts — READY for the diag run`);
while (performance.now() < end) {
  const now = performance.now();
  if (period === 0) {
    while (inflight < args.conc) fire();
  } else {
    while (nextSlot <= now && inflight < args.conc) {
      fire();
      nextSlot += period;
    }
    if (nextSlot < now - 5 * period) nextSlot = now;
  }
  if (now - windowStart >= 10000) {
    const s = summary(windowMs);
    run.log(`load window: ${windowMs.length} ok in ${round((now - windowStart) / 1000, 1)} s = ${round(windowMs.length / ((now - windowStart) / 1000), 1)}/s, p50=${s.p50} ms, errors so far ${errors}`);
    windowMs.length = 0;
    windowStart = now;
  }
  await sleep(period === 0 ? 1 : Math.max(1, nextSlot - performance.now()));
}
while (inflight > 0) await sleep(5);
const wall = (performance.now() - t0) / 1000;
const afterLoad = (await run.certificates()).leaf_cache;
const loadDelta = Object.fromEntries(Object.keys(afterLoad).map((k) => [k, afterLoad[k] - afterMint[k]]));
run.log(`load done: ${done} handshakes in ${round(wall, 1)} s = ${round(done / wall, 1)}/s, errors ${errors}; leaf_cache delta across load ${JSON.stringify(loadDelta)}`);
if (loadDelta.minted_total !== 0) run.degraded(`load phase minted ${loadDelta.minted_total} leaves; a diag run beside it would have over-counted`);

run.finish({
  measurement: 'P5-clock-load (diagnostic)',
  hosts: args.hosts,
  conc: args.conc,
  rate: args.rate,
  duration_s: args.duration,
  seed,
  figures: {
    handshakes: done,
    handshakes_per_s: round(done / wall, 1),
    errors,
    leaf_cache_before: lc,
    leaf_cache_after_mint: afterMint,
    leaf_cache_after_load: afterLoad,
    load_delta: loadDelta,
  },
});
