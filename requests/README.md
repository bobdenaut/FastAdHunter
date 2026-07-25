# requests/

Runnable `.http` files covering every endpoint in [API.md](../API.md), for
poking a live instance by hand.

## Setup

Install the **REST Client** extension (`humao.rest-client`). Shared settings
live in [`.vscode/settings.json`](../.vscode/settings.json):

1. Paste your API key into `apiKey` under `$shared`. It is printed to the
   container log exactly once, on first boot:

   ```text
   INFO generated API key — store it now; it is not shown again api_key=…
   ```

   Lost it? Rotate — see `auth.http`. There is no way to read it back.

2. Pick a target from the environment selector in the status bar: `rb5009`
   (`https://172.17.0.3:8443`) or `local` (`https://127.0.0.1:8443`).

3. Click **Send Request** above any `###` block.

TLS verification is disabled in those settings on purpose: the API certificate
is self-signed by rcgen on first boot (SECURITY.md) and is not meant to chain
to a public root.

## Files

| File | Covers |
| ---- | ------ |
| `health.http` | `GET /health` |
| `metrics.http` | `GET /metrics` (Prometheus text) |
| `auth.http` | Key behaviour, 401 shapes, key rotation |
| `lists.http` | Lists CRUD, refresh, persistence check |
| `rules.http` | Inline user rules, verdict dry-run |
| `stats.http` | Aggregates, query log, filters, pagination |
| `history.http` | Persisted series: summary, perf, top-N |
| `clients.http` | Client discovery and naming |
| `cache.http` | Cache usage and `POST /api/v1/cache/clean` |
| `settings.http` | `GET`/`POST /api/v1/config` |

Each file includes the failure cases, not just the happy path — 401s, 404s,
409s and 422s are requests you can run, because "does it reject this correctly"
is as much a part of the contract as "does it accept that".

## Not covered

`WS /api/v1/events` — the REST Client extension cannot open a WebSocket. Use
`websocat` or a browser console:

```sh
websocat --insecure "wss://172.17.0.3:8443/api/v1/events?token=YOUR-API-KEY"
```

Auth is via the `Authorization` header or a `?token=` query parameter on the
upgrade request. Server → client messages are `query`, `stats`,
`config_changed` and `list_refreshed`. Slow consumers are disconnected rather
than allowed to back-pressure the engine.

## Two things that will bite you

**Config writes bake in environment variables.** `POST /api/v1/config` persists
the *effective* config, so any value that arrived via `FAH__*` becomes a
permanent entry in `fastadhunter.toml`. Covered at the top of `settings.http`.

**Arrays replace, they do not merge.** Sending a partial `servers` array
silently drops everything you left out. Always send the complete set.
(`rules.lists` is no longer settable through `POST /config` at all — it returns
422 and points you at `/lists`, which applies changes live and writes the TOML
back for you. See `lists.http`.)

**`GET /api/v1/queries` only sees the in-RAM ring.** It reads `ring_entries`
(default 10 000) most-recent events, not the `/data` segments that
`retention_days`/`retention_max_mb` govern — so its reach is
`ring_entries ÷ QPS`, and a `from`/`to` range older than that returns an empty
list rather than an error. Long-term series live behind `history.http`.
