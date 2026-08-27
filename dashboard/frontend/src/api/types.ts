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

export interface Counters {
  dns: DnsCounters;
  http: HttpCounters;
  events_dropped: number;
  swr: SwrCounters;
  cache_cleanup: CacheCleanupCounters;
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

/**
 * Deliberately **narrow**. `GET /api/v1/config` returns the whole configuration
 * tree; typing all of it here would duplicate CONFIGURATION.md in TypeScript
 * and rot against it. Only the two keys this phase's built pages read are
 * declared, and `p5-09` owns the full shape.
 */
export interface Config {
  history?: { enabled?: boolean };
  dns?: { upstreams?: { strategy?: string } };
}
