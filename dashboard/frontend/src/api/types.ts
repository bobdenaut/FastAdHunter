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
