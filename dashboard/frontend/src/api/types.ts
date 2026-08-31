/** Shapes documented in API.md. Unknown fields are ignored, never rejected. */

export interface Health {
  status: 'ok' | 'degraded';
  version: string;
  uptime_seconds: number;
}

export interface CacheUsage {
  entries: number;
  capacity: number;
  fresh: number;
  stale: number;
  expired: number;
  hits: number;
  misses: number;
  evictions: number;
  bytes: number;
  max_bytes: number;
  load_percent: number;
  byte_load_percent: number;
}

/**
 * `POST /api/v1/cache/clean`. Six fields, all read verbatim.
 *
 * `freed_bytes` is the removed entries' own heap and **not** an RSS delta: a
 * clean never shrinks the hash-table slab, so resident memory does not fall by
 * this figure (API.md §Cache).
 */
export interface CacheCleanResponse {
  removed_expired: number;
  removed_stale: number;
  entries_before: number;
  entries_after: number;
  freed_bytes: number;
  duration_ms: number;
}

export interface DnsAnswers {
  servfail_synthesized: number;
  servfail_relayed: number;
  refused_relayed: number;
}

export interface DnsCounters {
  pass: number;
  allow: number;
  block: number;
  cache_hits: number;
  cache_misses: number;
  cache_stale: number;
  answers: DnsAnswers;
}

export interface HttpCounters {
  pass: number;
  allow: number;
  block: number;
  response_bytes: number;
  refused: number;
}

export interface SwrCounters {
  enqueued: number;
  deduplicated: number;
  dropped: number;
  completed: number;
  failed: number;
}

export interface CacheCleanupCounters {
  runs: number;
  entries_removed: number;
  bytes_freed: number;
  /** A last-value gauge, not a total — deltaing it produces nonsense. */
  last_duration_micros: number;
}

/**
 * List-refresh network truth. `bodies` counts refreshes that downloaded a full
 * body (its size lands in `bytes_fetched`); `not_modified` counts refreshes
 * answered `304 Not Modified` (or byte-identical), which move no body and
 * trigger no recompile.
 */
export interface ListsCounters {
  bodies: number;
  not_modified: number;
  bytes_fetched: number;
}

export interface Counters {
  dns: DnsCounters;
  http: HttpCounters;
  events_dropped: number;
  swr: SwrCounters;
  cache_cleanup: CacheCleanupCounters;
  lists: ListsCounters;
}

/** `count` and `sum_seconds`, never an average: a lifetime mean flattens
 *  within hours of uptime. */
export interface LatencyBucket {
  count: number;
  sum_seconds: number;
}

export interface Latency {
  dns: Record<string, LatencyBucket>;
  http: Record<string, LatencyBucket>;
}

export type UpstreamState = 'healthy' | 'penalized' | 'probing';

/**
 * One endpoint's round-trip time, in **seconds**, over the attempts it
 * actually answered — a timed-out attempt is a failure, not a slow answer, and
 * counting it would pin `p99` at `timeout_ms` for as long as an endpoint stays
 * down.
 *
 * `count` and `sum_seconds` are cumulative since process start, like
 * `attempts` beside them. The percentiles are **not**: on `/telemetry` they
 * are the process lifetime, on a `/history/perf` row they cover that row's
 * interval alone, which is the only form that still moves after a day of
 * uptime. Both are bucket-granularity estimates that saturate at the top
 * finite bucket (2 s), so they are a trend line rather than exact quantiles.
 */
export interface UpstreamRtt {
  count: number;
  sum_seconds: number;
  p50: number;
  p99: number;
}

export interface Upstream {
  address: string;
  protocol: string;
  attempts: number;
  failures: number;
  consecutive_failures: number;
  tls_handshakes: number;
  failure_runs: number[];
  state: UpstreamState;
  penalty_round: number;
  penalties: number;
  penalized_seconds_total: number;
  probes: number;
  probe_successes: number;
  family: 'v4' | 'v6' | null;
  /** Optional so a dashboard served beside an engine predating the field
   *  renders "not recorded" instead of `NaN` ms. */
  rtt?: UpstreamRtt;
}

export interface ProcessInfo {
  version: string;
  uptime_seconds: number;
}

export interface Ruleset {
  rules: number;
  duplicates_removed: number;
  compile_duration_seconds: number;
}

export interface Telemetry {
  process: ProcessInfo;
  ruleset: Ruleset;
  counters: Counters;
  latency: Latency;
  upstreams: Upstream[];
  cache: CacheUsage;
  memory: Memory;
}

/**
 * `MemoryComponentsResponse` is `#[serde(flatten)]`ed into `MemoryResponse`, so
 * these arrive as one flat object rather than a nested block.
 *
 * **The nulls are the contract, not defensiveness.** `residual_bytes` is `null`
 * when RSS could not be read; the `process_*` and `*_page_faults` figures are
 * `null` off Linux/Unix, which is every dev box that is not a container. A
 * consumer that treats them as `number` reads `null` as a zero-byte process.
 */
export interface Memory {
  ruleset_bytes: number;
  cache_estimated_bytes: number;
  stats_aggregates_bytes: number;
  stats_clients_bytes: number;
  accounted_bytes: number;
  residual_bytes: number | null;
  cache_entries: number;
  process_rss: number | null;
  process_peak_rss: number | null;
  major_page_faults: number | null;
  minor_page_faults: number | null;
  process_rss_anon: number | null;
  process_rss_file: number | null;
}

/**
 * `GET /api/v1/debug/memory` — `Memory` plus exactly the two allocator
 * counters, flattened into the same object (`wire.rs` `DebugMemoryResponse`).
 *
 * **The two extra fields carry no compatibility promise.** They describe
 * whichever allocator is linked in and would read near zero under another one,
 * which is why they sit under `/debug/*`. `null` is "the allocator reported
 * nothing", never zero — and it nulls only these two, because the kernel
 * figures above come from `getrusage` and a different `Option`.
 */
export interface DebugMemory extends Memory {
  allocator_committed_bytes: number | null;
  allocator_committed_peak_bytes: number | null;
}

/* --------------------------------------------------------------- events */

/**
 * One `query` item off `WS /api/v1/events` — both pipelines, tagged by `kind`.
 *
 * **Every key is always present**, so a client never has to tell "absent" from
 * "not applicable": a DNS event leaves the HTTP-only fields `null` and an HTTP
 * event leaves `qtype` `null` and `cached` `false`.
 *
 * `endpoint` is the one exception: it is present **only** on a DNS item an
 * upstream answered, and is that server's index in
 * `[dns.upstreams.servers]` — so a fallback past a dead primary reads
 * `"endpoint": 1`. Cache hits, blocks and HTTP items carry no key at all.
 *
 * `verdict` is `pass` | `allow` | `block` and there is no fourth value:
 * `counters.http.refused` is counted on the proxy, not on this stream.
 */
export interface QueryEvent {
  kind: string;
  ts: string;
  client: string;
  client_name: string | null;
  domain: string;
  qtype: string | null;
  verdict: string;
  rule: string | null;
  list: string | null;
  duration_ms: number;
  upstream: string | null;
  endpoint?: number;
  cached: boolean;
  method: string | null;
  path: string | null;
  resource_type: string | null;
  status: number | null;
  bytes: number | null;
}

/* ---------------------------------------------------------------- statistics */

export interface TopDomain {
  domain: string;
  count: number;
}

export interface TopClient {
  ip: string;
  name: string | null;
  count: number;
}

export interface StatsBucket {
  start: string;
  queries: number;
  blocked: number;
}

/** Counts both pipelines, unlike the domain tables which stay DNS-only. */
export interface PolicyStat {
  policy: string;
  queries: number;
  blocked: number;
}

/**
 * A rolling 24 h view. `window` is `"24h"`; nothing here is a lifetime figure,
 * and nothing here covers HTTP — `counters.http` on `/telemetry` does, over a
 * different window.
 */
export interface Stats {
  window: string;
  queries_total: number;
  blocked_total: number;
  blocked_percent: number;
  cache_hit_percent: number;
  top_blocked_domains: TopDomain[];
  top_queried_domains: TopDomain[];
  top_clients: TopClient[];
  buckets: StatsBucket[];
  policies: PolicyStat[];
}

/* ------------------------------------------------------------------- history */

export type HistoryResolution = 'hour' | 'day';

/**
 * `ts` is the **start** of the bucket. `per_type` uses the fixed rollup label
 * set and omits zero buckets, so a missing label means zero rather than
 * unknown. There is no allowed series: `permitted` is `queries − blocked` and
 * the UI derives it, never labelling it `allow`.
 */
export interface HistoryItem {
  ts: string;
  queries: number;
  blocked: number;
  blocked_percent: number;
  cache_hits: number;
  per_type: Record<string, number>;
}

export interface HistorySummary {
  resolution: HistoryResolution;
  from: string;
  to: string;
  /** Above 1 when the response was decimated. Decimation keeps whole rows. */
  stride: number;
  items: HistoryItem[];
}

/**
 * Per-stage percentiles **in seconds**, estimated from fixed histogram buckets
 * over one sampling interval: `pXX` is the smallest bucket upper bound whose
 * cumulative count reaches the quantile, so it is coarse by construction and
 * saturates at the top finite bucket.
 *
 * **A stage with no queries in the interval reports `0.0`**
 * (`crates/fah-model/src/perf.rs`). That is an absence of traffic rather than a
 * measurement of zero — a real reading is a bucket bound and can never be
 * exactly `0.0` — so the UI maps it to a gap and never plots it as a dip.
 */
export interface PerfLatency {
  block_p50: number;
  block_p99: number;
  cache_hit_p50: number;
  cache_hit_p99: number;
  forward_p50: number;
  forward_p99: number;
}

/**
 * One persisted `PerfSample`. `ts` is always present; every other key is
 * **absent** — not null — when `fields` did not ask for it, so each is optional
 * here and a consumer has to handle absence rather than read a null as a zero.
 *
 * `qps` and the three `*_delta` counters are per-interval, not cumulative.
 * `pass` is not served: it is `queries_delta − blocked_delta − allowed_delta`,
 * and `allowed_delta` is the real `allow` verdict the engine counted, never the
 * derived `permitted` band the Dashboard chart draws.
 */
/**
 * The persisted half of the memory breakdown: the six component figures, minus
 * every live-only one — no RSS (`PerfItem.rss_bytes` is it) and no allocator
 * counters.
 *
 * `residual_bytes` is derived server-side on read from the row's own
 * `rss_bytes`, so it cannot disagree with the components beside it. Rows
 * written before the field shipped read back as **zeros**, not nulls.
 */
export interface PerfMemory {
  ruleset_bytes: number;
  cache_estimated_bytes: number;
  stats_aggregates_bytes: number;
  stats_clients_bytes: number;
  accounted_bytes: number;
  residual_bytes: number;
}

export interface PerfItem {
  ts: string;
  qps?: number;
  queries_delta?: number;
  blocked_delta?: number;
  allowed_delta?: number;
  latency?: PerfLatency;
  rss_bytes?: number;
  /**
   * `getrusage`'s high-water RSS, **monotone within one process lifetime** — a
   * decrease between consecutive rows is a restart, never a reclaim. `0` means
   * the row predates the field or `getrusage` was unavailable, not that the
   * peak was zero.
   */
  peak_rss?: number;
  memory?: PerfMemory;
  /** Cumulative since process start. Charted as its derivative and never as
   *  the counter, which would draw a ramp. */
  minor_page_faults?: number;
  /** The allocator's own committed-bytes reading at capture — `0` where
   *  unavailable or the row predates the field. No compatibility promise,
   *  like the `DebugMemory` pair. No page requests it yet. */
  allocator_committed_bytes?: number;
  /** The persisted, cumulative half of `/telemetry`'s `counters.lists` —
   *  row-to-row deltas attribute an RSS excursion to a list refresh. Rows
   *  written before the field read back all-zero. No page requests it yet. */
  list_fetch?: ListsCounters;
  /** The endpoints as they stood at capture. Every counter on the row is
   *  cumulative except `rtt.p50` / `rtt.p99`, which cover this row's interval
   *  — the Upstreams page's round-trip chart is that pair over time. */
  upstreams?: Upstream[];
}

export interface HistoryPerf {
  from: string;
  to: string;
  /** Above 1 when the response was decimated. Decimation keeps whole rows —
   *  every point served is a real reading, never an average. */
  stride: number;
  items: PerfItem[];
}

/* ------------------------------------------------------------------- clients */

export interface Client {
  ip: string;
  name: string | null;
  first_seen: string;
  last_seen: string;
  queries_24h: number;
  blocked_24h: number;
  policy: string;
  /** Present only when an assignment names that exact address. */
  assignment_source?: string;
}

export interface ClientsResponse {
  items: Client[];
}

/* --------------------------------------------------------------------- lists */

/**
 * `degraded` is not a milder `ok`: the fetch succeeded and most of the body
 * failed to parse, which is the signature of a format misdetection.
 */
export type ListStatus = 'ok' | 'degraded' | 'failed' | 'rejected' | 'never';

/**
 * `url` carries the source whether it is remote or a mounted path — the wire
 * type has one field for both (`crates/fah-api/src/wire.rs`), so the UI reads
 * one and never guesses which key is present.
 *
 * The `rules_*` counts and `parse_errors` describe the copy **currently
 * serving**, and `last_status` describes the last refresh *attempt*. They are
 * deliberately independent: `failed` with a non-zero `rules_total` is the
 * normal report for a list whose download broke but whose rules keep blocking.
 */
export interface ListItem {
  id: string;
  url: string;
  format: string;
  enabled: boolean;
  refresh_hours: number;
  /** `null` until the first successful refresh in this process. */
  last_refresh: string | null;
  last_status: ListStatus;
  rules_total: number;
  rules_active_dns: number;
  rules_active_url: number;
  rules_inactive: number;
  parse_errors: number;
  /** Present only when `last_status` is `failed` or `rejected`. */
  last_error?: string;
}

/** `compiled_rules` and `duplicates_removed` describe the **merged** ruleset,
 *  which is why they sit on the envelope rather than on an item. */
export interface ListsResponse {
  items: ListItem[];
  compiled_rules: number;
  duplicates_removed: number;
}

export interface AddListRequest {
  /** Exactly one of `url` or `path`. */
  url?: string;
  path?: string;
  id?: string;
  enabled?: boolean;
  refresh_hours?: number;
}

/** `refresh_hours: null` clears a per-list override back to the default. */
export interface PatchListRequest {
  enabled?: boolean;
  refresh_hours?: number | null;
}

/** `degraded` does not occur here — the content gate refuses a misparsed body
 *  before it can commit — and `never` describes a list, not an attempt. */
export type RefreshOutcome = 'ok' | 'failed' | 'rejected';

export interface RefreshAllResult {
  id: string;
  status: RefreshOutcome;
  rules_active_dns?: number;
  error?: string;
}

/** `failed` counts every list that did not refresh, rejected ones included, so
 *  `refreshed + failed` is the number of `results`. */
export interface RefreshAllResponse {
  refreshed: number;
  failed: number;
  results: RefreshAllResult[];
}

/* -------------------------------------------------------------------- config */

/** `null` when unset. Required by a `dot` upstream, optional for `doh`, where
 *  the certificate name comes from the URL host. */
export interface UpstreamServerConfig {
  address: string;
  protocol: string;
  hostname: string | null;
}

/** `refresh_hours: null` follows `rules.refresh_hours_default`. The array is
 *  read-only here: `POST /config` answers `422` for `rules.lists`. */
export interface RuleListConfig {
  id: string;
  url: string;
  enabled: boolean;
  refresh_hours: number | null;
}

export interface ConfigAssignment {
  client: string;
  days?: string | null;
  start?: string | null;
  end?: string | null;
}

/** The `[[policies]]` entry as the **config tree** carries it, which is not the
 *  `/policies` response shape: `name` defaults to `id` there and is optional
 *  here. Rendered by the raw panel and by nothing else. */
export interface ConfigPolicy {
  id: string;
  name?: string | null;
  lists?: string[] | null;
  blocking_mode?: string | null;
  assignments?: ConfigAssignment[];
}

/**
 * The whole configuration tree `GET /api/v1/config` returns — every section of
 * `fah-config`'s `Config`, under the same key names.
 *
 * **Values, and nothing else.** The response carries no types, no bounds, no
 * enums and no mutability classes, so nothing may be built as though it did:
 * `pages/settings/metadata.ts` is where every constraint lives, hand-carried
 * from CONFIGURATION.md and `crates/fah-config/src/schema/`.
 *
 * `policies` is the one optional key — `skip_serializing_if = "Vec::is_empty"`
 * drops it when none is configured, which the raw panel renders as "none
 * configured" rather than leaving as an absence.
 *
 * There is no `auth` key and there never will be: auth material is not part of
 * the config tree at all (API.md §Configuration).
 */
export interface Config {
  engine: { mode: string };
  dns: {
    listen: { address: string; port: number };
    blocking: { mode: string; ttl_seconds: number };
    cache: {
      max_entries: number;
      max_bytes: number;
      min_ttl_seconds: number;
      max_ttl_seconds: number;
      negative_ttl_max_seconds: number;
      serve_stale: boolean;
      swr_workers: number;
      cleanup_interval_seconds: number;
    };
    upstreams: {
      strategy: string;
      timeout_ms: number;
      penalty_failures: number;
      servers: UpstreamServerConfig[];
    };
  };
  http: {
    listen: { address: string; port: number };
    max_connections: number;
    idle_timeout_ms: number;
    header_timeout_ms: number;
  };
  egress: { allow_destinations: string[]; allow_ip_literal_hosts: boolean };
  rules: { refresh_hours_default: number; lists: RuleListConfig[] };
  schedule: { timezone: string };
  policies?: ConfigPolicy[];
  stats: { snapshot_interval_seconds: number };
  /** `enabled` and `retention_days` are runtime-mutable;
   *  `sample_interval_seconds` is boot-only and is what one persisted perf row
   *  covers (API.md §History). */
  history: {
    enabled: boolean;
    sample_interval_seconds: number;
    retention_days: number;
  };
  api: { address: string; port: number; tls: boolean };
  log: { level: string; format: string };
}

/** `POST /api/v1/config`. Both flags are read verbatim: `applied` means at
 *  least one runtime key is live, `restart_required` that at least one boot key
 *  is persisted and waiting. */
export interface ConfigUpdateResponse {
  applied: boolean;
  restart_required: boolean;
}

/** `POST /api/v1/config/apikey/rotate`. Returned **once**; the UI renders it
 *  and stores it nowhere. */
export interface ApiKeyResponse {
  api_key: string;
}

/** `POST /api/v1/auth/password`. `204` on success, and every session dies with
 *  it — the caller's included, with no replacement cookie. */
export interface PasswordChangeBody {
  current_password: string;
  new_password: string;
}

/* ------------------------------------------------------------------ policies */

/**
 * `client` is a selector as configured — an address, a CIDR prefix, or a client
 * name (`crates/fah-rules/src/policy.rs` `parse_selector`). It is echoed back
 * on a `PATCH` exactly as it was given: the UI never re-spells it.
 *
 * `days`, `start` and `end` are absent rather than null when unset, and `start`
 * and `end` are set together or not at all.
 */
export interface Assignment {
  client: string;
  days?: string;
  start?: string;
  end?: string;
}

/**
 * `lists: null` means every enabled list, which is what an omitted `lists`
 * means in the TOML too — not "no lists". `blocking_mode: null` means the
 * policy inherits the global mode.
 */
export interface Policy {
  id: string;
  name: string;
  lists: string[] | null;
  blocking_mode: string | null;
  assignments: Assignment[];
}

/**
 * `items` holds the **configured** policies. `default` is never among them: it
 * is implicit and reserved (`fah-config/src/lib.rs`), so the ceiling of 16
 * counts it as one of the sixteen.
 *
 * `active_assignments` is assignments in force at this instant, after a `Name`
 * selector has been expanded into one entry per matching named client — so it
 * can exceed the number of configured assignment rows and is never phrased as
 * "of N configured".
 */
export interface PoliciesResponse {
  timezone: string;
  items: Policy[];
  active_assignments: number;
}

export interface CreatePolicyBody {
  id: string;
  name?: string;
  lists?: string[] | null;
  blocking_mode?: string | null;
  assignments?: Assignment[];
}

/**
 * A partial update. `lists` and `blocking_mode` are **double options**: absent
 * leaves the field alone, an explicit `null` clears it, a value sets it
 * (`wire.rs` `double_option_lists` / `double_option_string`). The request
 * builder must emit the key with a literal `null` rather than drop it, which
 * `resources.test.ts` pins.
 */
export interface PatchPolicyBody {
  name?: string;
  lists?: string[] | null;
  blocking_mode?: string | null;
  assignments?: Assignment[];
}

/* ------------------------------------------------- per-client policy and name */

/** `null` clears the name back to unnamed. */
export interface ClientNameBody {
  name: string | null;
}

export interface ClientPolicyBody {
  policy: string;
  days?: string;
  start?: string;
  end?: string;
}

/** The single-address read and the write path's response. No page in `p5-07`
 *  issues the `GET`; this is what the `PUT` answers with. */
export interface ClientPolicyResponse {
  ip: string;
  policy: string;
  assignment?: Assignment;
}

/* ---------------------------------------------------------------- user rules */

/**
 * The document as lines, both directions. There is no per-rule identity behind
 * it — the whole set is validated and swapped as one unit — and exact-duplicate
 * rule lines are dropped on the way in, so the response can be shorter than the
 * request.
 */
export interface UserRules {
  rules: string[];
}

/* --------------------------------------------------------------- rule tester */

/**
 * `qtype` is anything the resolver can name: `parse_qtype` maps everything but
 * `A` and `AAAA` to `Other(name)`, so the five the artboard draws are all
 * valid. `client` and `policy` may both be sent — an explicit `policy` wins.
 */
export interface RuleTestBody {
  domain: string;
  qtype?: string;
  client?: string;
  policy?: string;
}

/** Exactly four fields. Anything else on the result card is derived and is
 *  listed in the plan's §8.5 table. */
export interface RuleTestResult {
  verdict: 'pass' | 'allow' | 'block';
  rule: string | null;
  list: string | null;
  policy: string;
}
