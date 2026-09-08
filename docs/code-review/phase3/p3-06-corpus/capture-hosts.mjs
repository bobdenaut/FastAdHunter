import { appendFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';

const url = process.argv[2];
const out = process.argv[3];
const token = process.env.FAH_KEY;
if (!url || !out || !token) {
  console.error('usage: FAH_KEY=... node capture-hosts.mjs <wss-url> <out.jsonl>');
  process.exit(2);
}
process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0';

let rows = 0;
let started = new Date().toISOString();
writeFileSync(out, '');

function connect() {
  const socket = new WebSocket(`${url}?token=${encodeURIComponent(token)}`);
  socket.addEventListener('open', () => {
    socket.send(JSON.stringify({ subscribe: ['query'] }));
    console.log(`open ${new Date().toISOString()}`);
  });
  socket.addEventListener('message', (event) => {
    let frame;
    try {
      frame = JSON.parse(event.data);
    } catch {
      return;
    }
    if (frame.type !== 'query') return;
    const data = frame.data ?? {};
    const host = data.domain ?? data.host ?? null;
    if (!host) return;
    const id = createHash('sha256').update(host).digest('hex').slice(0, 16);
    appendFileSync(out, `${JSON.stringify({ ts: data.ts, kind: data.kind, host: id })}
`);
    rows += 1;
    if (rows % 500 === 0) console.log(`${new Date().toISOString()} rows=${rows}`);
  });
  socket.addEventListener('close', () => {
    console.log(`closed after ${rows} rows; reconnecting in 5s`);
    setTimeout(connect, 5000);
  });
  socket.addEventListener('error', (err) => {
    console.log(`error: ${err.message ?? err}`);
  });
}

process.on('SIGINT', () => {
  console.log(`captured ${rows} rows since ${started}`);
  process.exit(0);
});

connect();
