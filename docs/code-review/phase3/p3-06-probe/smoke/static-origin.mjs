// static-origin.mjs — the origin p10-domains.mjs and p10-connrate.mjs need.
//
// Those two scripts fetch fixed-size objects by name (1k.bin, 10k.bin,
// 50k.bin for the rate arms; 10mb.bin, 100mb.bin for the transfers arm) over
// both plaintext and TLS. p10-domains.mjs names the origin it wants:
// static-web-server, as in phase 2.6, "never a toy one". That is an external
// binary and the campaign should use it on the device, where throughput is
// the point.
//
// THIS IS A SMOKE STAND-IN, NOT THAT SERVER. The smoke plan states plainly
// that nothing in a smoke run is a measurement; the p10 rows exist to prove
// the scripts work, record N and fail loudly. Serving the five objects from
// memory is enough for that and needs no extra tool on the box. It is not a
// toy in the sense the §Traps row means (a listen backlog of 5): it uses the
// default backlog and measured 0 failures and 0 502s at 1510 rps and
// 1170 MiB/s. Do not carry a figure from it into a results file.
//
// Bodies are generated once at startup and served from memory, so the origin
// is not the bottleneck and every response carries an exact content-length.
// The TLS listener offers h2 and http/1.1 by ALPN; oha picks per arm.
//
//   node smoke/static-origin.mjs --cert p1.crt --key-file p1.key
//   node smoke/static-origin.mjs --cert p1.crt --key-file p1.key --http-port 8081 --https-port 4443
//
// Not a measurement: nothing this serves is a figure. It exists so the p10
// scripts have something to talk to.

import fs from 'node:fs';
import http from 'node:http';
import http2 from 'node:http2';

const args = {
  cert: null,
  'key-file': null,
  'http-port': 80,
  'https-port': 443,
  address: '127.0.0.1',
};
for (let i = 0; i < process.argv.length - 2; i += 1) {
  const a = process.argv[i + 2];
  if (!a.startsWith('--')) continue;
  const k = a.slice(2);
  if (!(k in args)) {
    console.error(`unknown flag --${k}; known: ${Object.keys(args).join(', ')}`);
    process.exit(2);
  }
  args[k] = process.argv[i + 3];
  i += 1;
}
if (!args.cert || !args['key-file']) {
  console.error('usage: node smoke/static-origin.mjs --cert <pem> --key-file <pem> [--http-port 80] [--https-port 443] [--address 127.0.0.1]');
  process.exit(2);
}

const SIZES = {
  '/1k.bin': 1024,
  '/10k.bin': 10 * 1024,
  '/50k.bin': 50 * 1024,
  '/10mb.bin': 10 * 1024 * 1024,
  '/100mb.bin': 100 * 1024 * 1024,
};

// One buffer per object, filled with a repeating pattern so a truncated read
// is visible in a hex dump rather than looking like a short but valid body.
const BODIES = Object.fromEntries(
  Object.entries(SIZES).map(([path, size]) => {
    const buf = Buffer.allocUnsafe(size);
    for (let i = 0; i < size; i += 1) buf[i] = i % 251;
    return [path, buf];
  })
);

function resolvePath(raw) {
  const path = (raw || '/').split('?')[0];
  if (path === '/') return { status: 200, body: Buffer.from('static-origin\n') };
  const body = BODIES[path];
  if (body) return { status: 200, body };
  return { status: 404, body: Buffer.from('not found\n') };
}

const httpServer = http.createServer((req, res) => {
  const { status, body } = resolvePath(req.url);
  res.writeHead(status, {
    'content-type': 'application/octet-stream',
    'content-length': body.length,
  });
  if (req.method === 'HEAD') return res.end();
  res.end(body);
});
httpServer.keepAliveTimeout = 60_000;
httpServer.listen(Number(args['http-port']), args.address, () => {
  console.log(`static-origin plaintext on ${args.address}:${args['http-port']}`);
});

const tlsServer = http2.createSecureServer({
  cert: fs.readFileSync(args.cert),
  key: fs.readFileSync(args['key-file']),
  allowHTTP1: true,
  ALPNProtocols: ['h2', 'http/1.1'],
});
// h1 clients arrive here as ordinary request/response objects; h2 clients as
// streams. Both are answered from the same table.
tlsServer.on('request', (req, res) => {
  const { status, body } = resolvePath(req.url);
  res.writeHead(status, {
    'content-type': 'application/octet-stream',
    'content-length': body.length,
  });
  if (req.method === 'HEAD') return res.end();
  res.end(body);
});
tlsServer.on('sessionError', () => {});
tlsServer.on('clientError', (_err, socket) => socket.destroy());
tlsServer.listen(Number(args['https-port']), args.address, () => {
  console.log(
    `static-origin TLS on ${args.address}:${args['https-port']}, alpn h2+http/1.1, ` +
      `objects ${Object.keys(SIZES).join(' ')}`
  );
});
