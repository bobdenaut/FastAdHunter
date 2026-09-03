// p1-origin.mjs — the P1 origin, its own process on the second LAN endpoint
// (plan §Scripts). Serves N MiB over TLS on :443 to every connection, then
// closes. No HTTP: the client reads bytes until EOF, so the figure is the
// relay and nothing else.
//
// The certificate must carry the origin's publicly resolvable name as a SAN
// (the probe resolves it through the container's stub resolver; `<dashed-ip>
// .nip.io` works — plan §Traps). One-liner, run on the endpoint:
//
//   openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
//     -keyout p1-origin.key -out p1-origin.crt -days 30 \
//     -subj "/CN=192-168-10-20.nip.io" \
//     -addext "subjectAltName=DNS:192-168-10-20.nip.io"
//
// The client (p1-lan.mjs --origin-cert p1-origin.crt) trusts exactly this
// certificate. Firewall: plan §Local firewall — inbound TCP 443 scoped to the
// probe address (and to the client host for the P1-control arm), never
// unscoped. Binding :443 needs elevation on Windows and root or
// CAP_NET_BIND_SERVICE on Linux; --port for an unprivileged alternative
// (then p1-lan.mjs --origin-port must match, and the probe's splice still
// targets :443 — the unprivileged port serves the P1-control arm only).
//
//   node p1-origin.mjs --cert p1-origin.crt --key-file p1-origin.key --bytes 64

import fs from 'node:fs';
import tls from 'node:tls';
import { performance } from 'node:perf_hooks';
import { parseArgs, MiB, round } from './lib.mjs';

const args = parseArgs({
  cert: { type: 'string', required: true, help: 'PEM certificate (SAN = the origin name)' },
  'key-file': { type: 'string', required: true, help: 'PEM private key for --cert' },
  bytes: { type: 'number', default: 64, help: 'MiB served per connection' },
  port: { type: 'number', default: 443, help: 'listen port' },
  address: { type: 'string', default: '0.0.0.0', help: 'listen address' },
});

const payload = Buffer.alloc(args.bytes * MiB, 0x78);
let serial = 0;

const server = tls.createServer(
  {
    cert: fs.readFileSync(args.cert),
    key: fs.readFileSync(args['key-file']),
    minVersion: 'TLSv1.2',
  },
  (sock) => {
    serial += 1;
    const id = serial;
    const t0 = performance.now();
    const peer = `${sock.remoteAddress}:${sock.remotePort}`;
    sock.setNoDelay(true);
    sock.on('error', (e) => process.stdout.write(`#${id} ${peer} error ${e.code || e.message}\n`));
    sock.on('close', () => {
      process.stdout.write(`#${id} ${peer} closed after ${round(performance.now() - t0, 1)} ms, ${sock.bytesWritten} bytes written, ${sock.getProtocol?.() ?? '?'} sni=${sock.servername ?? '-'}\n`);
    });
    sock.end(payload);
  },
);

server.on('tlsClientError', (e, sock) => {
  process.stdout.write(`tls client error from ${sock?.remoteAddress ?? '?'}: ${e.code || e.message}\n`);
});
server.on('error', (e) => {
  process.stderr.write(`INVALID: listen failed: ${e.code || e.message} (an existing listener on :${args.port}? plan §Running: Get-NetTCPConnection -LocalPort ${args.port} -State Listen must be empty)\n`);
  process.exit(2);
});
server.listen(args.port, args.address, () => {
  process.stdout.write(`p1-origin listening on ${args.address}:${args.port}, ${args.bytes} MiB per connection, cert ${args.cert}\n`);
});
