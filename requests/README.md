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
   (`https://172.17.0.2:8443`) or `local` (`https://127.0.0.1:8443`).

3. Click **Send Request** above any `###` block.

TLS verification is disabled in those settings on purpose: the API certificate
is self-signed by rcgen on first boot (SECURITY.md) and is not meant to chain
to a public root.

## Files

| File | Covers |
| ---- | ------ |
| `health.http` | `GET /health` |
| `telemetry.http` | `GET /api/v1/telemetry` — the whole engine state as JSON |
| `debug.http` | `GET /api/v1/debug/memory` — allocator internals, no contract |
| `auth.http` | Key behaviour, 401 shapes, key rotation |
| `lists.http` | Lists CRUD, refresh, persistence check |
| `rules.http` | Inline user rules, verdict dry-run |
| `stats.http` | Aggregated 24h statistics |
| `history.http` | Persisted series: summary, perf, top-N |
| `clients.http` | Client discovery and naming |
| `cache.http` | Cache usage and `POST /api/v1/cache/clean` |
| `settings.http` | `GET`/`POST /api/v1/config` |

**One endpoint, one file.** No request appears in two files, so a response
shape has exactly one place to be checked. `auth.http` is the sole exception,
and only because testing the key needs *some* protected path — it probes one no
endpoint owns, which 401s without a key and 404s with one.

Each file includes the failure cases, not just the happy path — 401s, 404s,
409s and 422s are requests you can run, because "does it reject this correctly"
is as much a part of the contract as "does it accept that".

## Not covered

`WS /api/v1/events` — the REST Client extension cannot open a WebSocket, and it
is now the **only** per-query surface: there is no HTTP endpoint that lists
individual queries or requests. Use `websocat` or a browser console:

```sh
websocat --insecure "wss://172.17.0.2:8443/api/v1/events?token=YOUR-API-KEY"
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

**An unknown config section is a boot failure, not a warning.** The root config
is `deny_unknown_fields`, so a key this build does not know stops the binary
starting — `POST /config` returns 422 for the same reason. Covered at the
bottom of `settings.http`.
