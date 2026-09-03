// smoke/h2-origin.mjs — the h2 origin p3-h2stall.mjs talks to. Two uses:
// the smoke rows of plan/wip/phase3/p3-06-smoke-plan.md (dev box, self-signed
// certificate) and, per testing-plan declaration delta 14, the campaign's P3
// origin on the second LAN endpoint under a public name with a publicly
// trusted certificate (the release probe verifies upstreams against
// webpki-roots only). Never runs on the device itself. Not a measurement tool.
//
// Paths:
//   /            3-byte body (warm-up)
//   /8mib        --bytes MiB body (throughput and stall rows)
//   /stall       200 with headers only, DATA never sent, stream held open —
//                the "barrier not met" negative row: every stream gets
//                :status 200, none gets a first DATA chunk
// HEAD answers headers only (content-length included), so `curl -I --http2`
// is a clean preflight.
// Certificate: SAN = the origin name, CA:FALSE + EKU serverAuth (the
// interception leg verifies it; see p1-origin.mjs for the openssl line), or
// a Let's Encrypt fullchain.pem + privkey.pem for the campaign.
// Limits are set high so the origin itself is never the bottleneck under
// 64 concurrent 8 MiB streams (--max-streams, --session-memory).
// Binds 127.0.0.1 unless --address is given; the LAN endpoint needs
// --address 0.0.0.0 (or its LAN address).
//
//   node smoke/h2-origin.mjs --cert p3.crt --key-file p3.key --port 443 --bytes 8
//   node smoke/h2-origin.mjs --cert fullchain.pem --key-file privkey.pem --address 0.0.0.0 --port 443 --bytes 8

import fs from 'node:fs';
import http2 from 'node:http2';

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, tok, i, a) => {
    if (tok.startsWith('--')) acc.push([tok.slice(2), a[i + 1]]);
    return acc;
  }, []),
);
const USAGE = 'usage: node smoke/h2-origin.mjs --cert <pem> --key-file <pem> [--port 443] [--address 127.0.0.1] [--bytes 8] [--max-streams 256] [--session-memory 4096]';
if ('help' in args) {
  console.log(USAGE);
  process.exit(0);
}
for (const flag of ['cert', 'key-file']) {
  if (!args[flag] || !fs.existsSync(args[flag])) {
    console.error(`--${flag} missing or not a file\n${USAGE}`);
    process.exit(64);
  }
}
const port = Number(args.port ?? 443);
const address = args.address ?? '127.0.0.1';
const bytes = Number(args.bytes ?? 8) * 1024 * 1024;
const payload = Buffer.alloc(bytes, 0x61);
const small = Buffer.from('ok\n');
const held = new Set();

const server = http2.createSecureServer({
  cert: fs.readFileSync(args.cert),
  key: fs.readFileSync(args['key-file']),
  allowHTTP1: false,
  maxSessionMemory: Number(args['session-memory'] ?? 4096),
  settings: { maxConcurrentStreams: Number(args['max-streams'] ?? 256), initialWindowSize: 1024 * 1024 },
});

server.on('stream', (stream, headers) => {
  const path = headers[':path'] ?? '/';
  const head = headers[':method'] === 'HEAD';
  stream.on('error', (e) => console.error(`stream ${path} error ${e.code || e.message}`));
  if (path === '/stall') {
    stream.respond({ ':status': 200, 'content-type': 'application/octet-stream' }, { endStream: head });
    if (head) return;
    held.add(stream);
    stream.on('close', () => held.delete(stream));
    return;
  }
  const body = path === '/8mib' ? payload : small;
  stream.respond({ ':status': 200, 'content-length': body.length, 'content-type': 'application/octet-stream' }, { endStream: head });
  if (!head) stream.end(body);
});
server.on('sessionError', (e) => console.error(`sessionError ${e.code || e.message}`));
server.on('error', (e) => {
  console.error(`listen failed: ${e.code || e.message}`);
  process.exit(2);
});
server.listen(port, address, () => {
  console.log(`h2-origin listening on ${address}:${port}, /8mib = ${bytes / (1024 * 1024)} MiB, /stall holds headers-only streams, cert ${args.cert}`);
});
