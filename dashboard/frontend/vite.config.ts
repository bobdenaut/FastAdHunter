import { defineConfig } from 'vitest/config';
import preact from '@preact/preset-vite';

// The TLS listener development talks to. `POST /auth/login` answers 503 when
// `api.tls = false` and the cookie is `__Host-`/`Secure`, so this is never
// plain HTTP. Override when the API is on another port — a container mapped
// elsewhere, for instance.
const API_TARGET = process.env['FAH_API_TARGET'] ?? 'https://localhost:8443';

// `changeOrigin` rewrites `Host`, never `Origin`. The API validates a
// cookie-authenticated WebSocket upgrade against its own effective origin
// (`crates/fah-api/src/routes.rs`, `same_origin`), deriving the scheme from
// `api.tls` rather than from what the browser presented — so through this proxy
// the browser's `Origin: http://localhost:5173` fails on scheme and on port.
// Without the explicit header the dev socket 401s, the REST probe reads that as
// an expired session, and development bounces to /login for ever.
const proxy = {
  '/api': {
    target: API_TARGET,
    secure: false,
    changeOrigin: true,
    ws: true,
    headers: { Origin: API_TARGET },
  },
  '/health': {
    target: API_TARGET,
    secure: false,
    changeOrigin: true,
  },
};

export default defineConfig({
  plugins: [preact()],
  base: '/',
  build: {
    target: 'es2022',
    sourcemap: false,
    // The default injects <link rel="modulepreload"> for a dynamic import's
    // dependencies, eagerly fetching the very chunks the split created.
    modulePreload: false,
    cssCodeSplit: false,
    assetsInlineLimit: 0,
    assetsDir: 'assets',
  },
  server: { port: 5173, proxy },
  preview: { port: 4173, proxy },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx', 'scripts/**/*.test.mjs'],
  },
});
