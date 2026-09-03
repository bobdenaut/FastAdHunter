// smoke/h2-preflight.mjs — P3 origin preflight (testing-plan declaration
// delta 14): one HEAD and one GET over h2 for --path, one line each, then a
// PASS / FAIL line. PASS only when ALPN is h2, both answer 200, both carry
// content-length = --bytes MiB and the GET body is exactly that long.
// Neither curl on bobdenaut speaks h2, so this stands in for
// `curl -I --http2`. Output is saved beside the results, never just shown.
//
//   node smoke/h2-preflight.mjs --host p3-origin.example.ro --path /8mib --bytes 8
//   node smoke/h2-preflight.mjs --host localhost --port 9443 --path /8mib --insecure
//
// From Git Bash prefix the command with MSYS2_ARG_CONV_EXCL='*', or --path
// arrives as C:/Program Files/Git/8mib (smoke finding F23).

import http2 from 'node:http2';

const USAGE = 'usage: node smoke/h2-preflight.mjs --host <name> [--port 443] [--path /8mib] [--bytes 8] [--insecure]';
const args = {};
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i += 1) {
  const tok = argv[i];
  if (!tok.startsWith('--')) continue;
  const key = tok.slice(2);
  if (key === 'insecure') args[key] = true;
  else args[key] = argv[(i += 1)];
}
if ('help' in args || !args.host) {
  console.log(USAGE);
  process.exit(args.host ? 0 : 64);
}
const host = args.host;
const port = Number(args.port ?? 443);
const path = args.path ?? '/8mib';
const expected = Number(args.bytes ?? 8) * 1024 * 1024;

const session = http2.connect(`https://${host}:${port}`, { rejectUnauthorized: !args.insecure });
session.on('error', (e) => {
  console.log(`${host}:${port} error ${e.code || e.message}`);
  process.exit(2);
});
setTimeout(() => {
  console.log(`FAIL no verdict within 30 s (origin holding ${path}?)`);
  process.exit(3);
}, 30000).unref();

function request(method) {
  return new Promise((resolve) => {
    const r = session.request({ ':method': method, ':path': path });
    const row = { method, status: null, contentLength: null, bytes: 0, error: null };
    r.on('response', (h) => {
      row.status = h[':status'];
      row.contentLength = h['content-length'] === undefined ? null : Number(h['content-length']);
    });
    r.on('data', (c) => {
      row.bytes += c.length;
    });
    r.on('end', () => resolve(row));
    r.on('error', (e) => {
      row.error = e.code || e.message;
      resolve(row);
    });
    r.end();
  });
}

session.on('connect', async () => {
  const alpn = session.socket.alpnProtocol;
  const peer = session.socket.getPeerCertificate?.();
  console.log(`${host}:${port} alpn ${alpn} tls ${session.socket.getProtocol?.()} issuer ${peer?.issuer?.CN ?? '?'} authorized ${session.socket.authorized}`);
  const rows = [await request('HEAD'), await request('GET')];
  for (const r of rows) console.log(`${r.method} ${path} status ${r.status} content-length ${r.contentLength} bytes ${r.bytes}${r.error ? ' error ' + r.error : ''}`);
  const [head, get] = rows;
  const ok = alpn === 'h2' && head.status === 200 && get.status === 200 && head.contentLength === expected && get.contentLength === expected && get.bytes === expected && head.bytes === 0;
  console.log(ok ? `PASS ${expected} bytes over h2` : `FAIL expected alpn h2, 200, content-length ${expected}, GET body ${expected} bytes, empty HEAD body`);
  session.close(() => process.exit(ok ? 0 : 1));
});
