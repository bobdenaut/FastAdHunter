/**
 * **The schema this form is written against — hand-carried, and the only
 * source of a bound, an enum or a mutability class in the application.**
 *
 * `GET /api/v1/config` supplies current effective values and nothing else. It
 * is not a schema endpoint: API.md describes it as "effective configuration
 * (all sources merged), secrets redacted", and the handler serialises the typed
 * config verbatim — no types, no bounds, no enums, no mutability classes. Every
 * one of those was carried here by hand from the documents and the Rust named
 * in each field's `source`, which is the cost the hand-written-form decision
 * pays on purpose and the drift risk it accepts (IA §Settings).
 *
 * Anything that reads as "the API told us the bounds" is wrong. The response is
 * read for values; this module decides what is legal.
 *
 * Two rules for editing it:
 *
 * 1. **Every field carries its `source`.** A bound with no anchor cannot be
 *    checked against the thing it came from, which is the whole mitigation for
 *    the drift this design accepts.
 * 2. **A field absent here is not absent from the config.** The raw
 *    All-settings panel renders the whole response, so an unmodelled key stays
 *    visible rather than silently missing — that panel is what makes this
 *    subset safe.
 */

/** `restart_required: true` for a `boot` key, `applied: true` for a `live` one
 *  — the two classes `POST /api/v1/config` answers with. */
export type Mutability = 'live' | 'restart';

export const U32_MAX = 4_294_967_295;

/** `u64` in the schema. Past 2^53 a browser cannot hold the integer exactly, so
 *  the client refuses rather than sending a rounded value. */
export const SAFE_MAX = Number.MAX_SAFE_INTEGER;

export const PORT_MIN = 1;
export const PORT_MAX = 65_535;

/** `fah-config/src/lib.rs` `MIN_CACHE_MAX_BYTES` — 1 MiB over 16 shards is
 *  ~64 KiB each, one maximum-size DNS answer. */
export const MIN_CACHE_MAX_BYTES = 1_048_576;

/** `fah-config/src/lib.rs` `MAX_UPSTREAM_SERVERS`. Rejected at load, never
 *  truncated. */
export const MAX_UPSTREAM_SERVERS = 8;

export type FieldControl =
  | { kind: 'int'; min: number; max: number }
  | { kind: 'bytes'; min: number; max: number }
  | { kind: 'bool' }
  | { kind: 'enum'; values: readonly string[] }
  | { kind: 'ip' }
  | { kind: 'tz' }
  | { kind: 'string-list' }
  | { kind: 'server-list' };

export interface FieldMeta {
  /** Dotted, exactly as the config tree spells it — this is the patch key. */
  key: string;
  label: string;
  help: string;
  control: FieldControl;
  mutability: Mutability;
  /** Where the bound, the enum and the class above were carried from. */
  source: string;
  /**
   * Saving this field opens a confirmation naming the consequence first. Only
   * `[api]` sets it: those three keys move or remove the origin this dashboard
   * is being used over.
   */
  consequence?: string;
}

export interface SectionMeta {
  /** The `[section]` heading, and the anchor id the side nav scrolls to. */
  id: string;
  note: string;
  fields: readonly FieldMeta[];
}

const CONFIG_REFERENCE = 'CONFIGURATION.md §Reference';
const BOOT_KEYS = 'config_store.rs BOOT_KEYS';
const VALIDATE = 'fah-config/src/lib.rs validate()';

/** `[[dns.upstreams.servers]] protocol`. The array is one patch key, so the row
 *  editor's own cells are not `FieldMeta` — the whole list is one field. */
export const UPSTREAM_PROTOCOLS = ['udp', 'dot', 'doh'] as const;

/**
 * Grouped by config section as CONFIGURATION.md organises them, in its order.
 * `[http.listen]` and `[http]` share one card because they are one subject and
 * the artboard's nav draws one entry; the keys stay fully dotted, so the patch
 * is unaffected.
 */
export const SECTIONS: readonly SectionMeta[] = [
  {
    id: 'engine',
    note: 'The filtering scope, fixed at container start.',
    fields: [
      {
        key: 'engine.mode',
        label: 'mode',
        help: 'Which pipelines run. There is deliberately no second enabled switch — mode is the only one.',
        control: {
          kind: 'enum',
          values: ['dns', 'dns+http', 'dns+http+https'],
        },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [engine]; schema/engine.rs EngineMode; ${BOOT_KEYS} engine.mode`,
      },
    ],
  },
  {
    id: 'dns.listen',
    note: 'Where the resolver listens. UDP and TCP on the same port.',
    fields: [
      {
        key: 'dns.listen.address',
        label: 'address',
        help: 'Two colons is one dual-stack socket serving IPv4 and IPv6; v4 clients are reported canonically, never as mapped addresses.',
        control: { kind: 'ip' },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [dns.listen]; ${VALIDATE} validate_ip; ${BOOT_KEYS} dns.listen`,
      },
      {
        key: 'dns.listen.port',
        label: 'port',
        help: 'UDP and TCP. Zero is refused.',
        control: { kind: 'int', min: PORT_MIN, max: PORT_MAX },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [dns.listen]; ${VALIDATE} validate_nonzero_port; u16 in schema/dns/listen.rs`,
      },
    ],
  },
  {
    id: 'dns.blocking',
    note: 'What a blocked answer is, and how long a client keeps it.',
    fields: [
      {
        key: 'dns.blocking.mode',
        label: 'mode',
        help: 'null_ip answers 0.0.0.0 and the v6 equivalent. It is the only value the schema deserialises — the documented future modes are not accepted values, and an unimplemented one fails at config load rather than falling back silently.',
        control: { kind: 'enum', values: ['null_ip'] },
        mutability: 'restart',
        source:
          'schema/dns/blocking.rs BlockingMode — the enum, not CONFIGURATION.md’s future-value list',
      },
      {
        key: 'dns.blocking.ttl_seconds',
        label: 'ttl_seconds',
        help: 'TTL of a synthesized blocked answer.',
        control: { kind: 'int', min: 0, max: U32_MAX },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [dns.blocking]; u32 in schema/dns/blocking.rs; ${BOOT_KEYS} dns.blocking`,
      },
    ],
  },
  {
    id: 'dns.cache',
    note: 'Read once at startup — every field here needs a restart. POST /api/v1/cache/clean releases cache memory without one.',
    fields: [
      {
        key: 'dns.cache.max_entries',
        label: 'max_entries',
        help: 'Upper bound on cached answers; the real capacity rounds to the shard count. Raise to 100k+ if RAM allows.',
        control: { kind: 'int', min: 0, max: U32_MAX },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [dns.cache]; u32 in schema/dns/cache.rs`,
      },
      {
        key: 'dns.cache.max_bytes',
        label: 'max_bytes',
        help: 'The second bound — eviction runs until entries and bytes are both inside. At least 1 MiB, so a shard can always hold one answer.',
        control: { kind: 'bytes', min: MIN_CACHE_MAX_BYTES, max: SAFE_MAX },
        mutability: 'restart',
        source: `${VALIDATE} MIN_CACHE_MAX_BYTES = 1 MiB; u64 in schema/dns/cache.rs`,
      },
      {
        key: 'dns.cache.min_ttl_seconds',
        label: 'min_ttl_seconds',
        help: 'Clamp floor. Must not exceed max_ttl_seconds.',
        control: { kind: 'int', min: 0, max: U32_MAX },
        mutability: 'restart',
        source: `${VALIDATE} min_ttl_seconds <= max_ttl_seconds; u32 in schema/dns/cache.rs`,
      },
      {
        key: 'dns.cache.max_ttl_seconds',
        label: 'max_ttl_seconds',
        help: 'Clamp ceiling — 24 h by default.',
        control: { kind: 'int', min: 0, max: U32_MAX },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [dns.cache]; u32 in schema/dns/cache.rs`,
      },
      {
        key: 'dns.cache.negative_ttl_max_seconds',
        label: 'negative_ttl_max_seconds',
        help: 'RFC 2308 negative-cache cap.',
        control: { kind: 'int', min: 0, max: U32_MAX },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [dns.cache]; u32 in schema/dns/cache.rs`,
      },
      {
        key: 'dns.cache.serve_stale',
        label: 'serve_stale',
        help: 'Answer from an expired entry, up to 24 h old, rather than failing.',
        control: { kind: 'bool' },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [dns.cache]; bool in schema/dns/cache.rs`,
      },
      {
        key: 'dns.cache.swr_workers',
        label: 'swr_workers',
        help: 'Background refreshers for stale entries (ADR-0005): a stale hit answers from cache at once and the refresh happens off the query path. Zero disables it, restoring "stale only after a failed forward".',
        control: { kind: 'int', min: 0, max: U32_MAX },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [dns.cache]; u32 in schema/dns/cache.rs`,
      },
      {
        key: 'dns.cache.cleanup_interval_seconds',
        label: 'cleanup_interval_seconds',
        help: 'Background sweep of entries past the stale window. Zero disables it. It does not bound the cache — max_entries and max_bytes do; this returns memory underneath them.',
        control: { kind: 'int', min: 0, max: U32_MAX },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [dns.cache]; u32 in schema/dns/cache.rs`,
      },
    ],
  },
  {
    id: 'dns.upstreams',
    note: 'Which resolvers are asked, in what order, and how failure is handled.',
    fields: [
      {
        key: 'dns.upstreams.strategy',
        label: 'strategy',
        help: 'adaptive penalizes an endpoint that keeps failing and probes it on the way past. It is the only strategy the engine accepts.',
        control: { kind: 'enum', values: ['adaptive'] },
        mutability: 'restart',
        source: `schema/dns/upstreams.rs UpstreamStrategy; ${BOOT_KEYS} dns.upstreams`,
      },
      {
        key: 'dns.upstreams.timeout_ms',
        label: 'timeout_ms',
        help: 'Per-attempt timeout, bounding one leg; one attempt against one endpoint is bounded at three times this. Under adaptive it also fixes the penalty backoff, which is derived and never a key.',
        control: { kind: 'int', min: 1, max: 10_000 },
        mutability: 'restart',
        source: `${VALIDATE} validate_range(1, 10000)`,
      },
      {
        key: 'dns.upstreams.penalty_failures',
        label: 'penalty_failures',
        help: 'Consecutive transport failures that penalize a healthy endpoint. Read only under adaptive; an RCODE is a transport success and clears the streak.',
        control: { kind: 'int', min: 1, max: 255 },
        mutability: 'restart',
        source: `${VALIDATE} validate_range(1, 255)`,
      },
      {
        key: 'dns.upstreams.servers',
        label: 'servers',
        help: 'One to eight, in the order they are tried — the index is what a query reports as its answering endpoint. dot needs a hostname for certificate verification; doh is addressed by URL and its scheme must be https.',
        control: { kind: 'server-list' },
        mutability: 'restart',
        source: `${VALIDATE} MAX_UPSTREAM_SERVERS = 8, non-empty, dot needs hostname, doh needs an https scheme; schema/dns/upstreams.rs UpstreamProtocol`,
      },
    ],
  },
  {
    id: 'http',
    note: 'Inert unless engine.mode includes http. The whole section is boot: the connection semaphore is sized once when the listener binds.',
    fields: [
      {
        key: 'http.listen.address',
        label: 'listen.address',
        help: 'As dns.listen.address. Validated even while the mode is DNS-only, so a typo is caught while it is being made rather than on the restart months later that first turns the mode on.',
        control: { kind: 'ip' },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [http.listen]; ${VALIDATE} validate_ip`,
      },
      {
        key: 'http.listen.port',
        label: 'listen.port',
        help: '8080, not 80: the container runs unprivileged and the router dst-nats 80 here instead.',
        control: { kind: 'int', min: PORT_MIN, max: PORT_MAX },
        mutability: 'restart',
        source: `${VALIDATE} validate_nonzero_port; u16 in schema/http.rs`,
      },
      {
        key: 'http.max_connections',
        label: 'max_connections',
        help: 'Ceiling on concurrent proxied connections; a burst queues in the kernel rather than in process memory. It looks runtime-shaped and is not — the semaphore is sized at bind.',
        control: { kind: 'int', min: 1, max: SAFE_MAX },
        mutability: 'restart',
        source: `${VALIDATE} "must be at least 1"; ${BOOT_KEYS} http, whole section`,
      },
      {
        key: 'http.idle_timeout_ms',
        label: 'idle_timeout_ms',
        help: 'How long an idle upstream connection is kept in the pool.',
        control: { kind: 'int', min: 0, max: SAFE_MAX },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [http]; u64 in schema/http.rs`,
      },
      {
        key: 'http.header_timeout_ms',
        label: 'header_timeout_ms',
        help: 'Slowloris bound: the deadline for a client to finish sending its request head, and the cap on how long a keep-alive connection may sit between requests.',
        control: { kind: 'int', min: 0, max: SAFE_MAX },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [http]; u64 in schema/http.rs`,
      },
    ],
  },
  {
    id: 'egress',
    note: 'Where the proxies may connect. Default-deny is a security property, not a preference: the destination comes from a header the client wrote, so with no allow-list every private, loopback and link-local address is refused.',
    fields: [
      {
        key: 'egress.allow_destinations',
        label: 'allow_destinations',
        help: 'One IP or CIDR block per line. Judged on the resolved address, so a public name pointing at a private one is refused too. Empty means every private destination is refused.',
        control: { kind: 'string-list' },
        mutability: 'restart',
        source: `${VALIDATE} validate_allowed_destination — an IP, or an IP with a prefix length within its family; ${BOOT_KEYS} egress`,
      },
      {
        key: 'egress.allow_ip_literal_hosts',
        label: 'allow_ip_literal_hosts',
        help: 'Whether a client may name a bare IP as its destination. A browser resolving a name never produces one, so this is the shape of a probe; prefer the allow-list, which is checked against the resolved address.',
        control: { kind: 'bool' },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [egress]; bool in schema/egress.rs`,
      },
    ],
  },
  {
    id: 'rules',
    note: 'List membership is not edited here.',
    fields: [
      {
        key: 'rules.refresh_hours_default',
        label: 'refresh_hours_default',
        help: 'The interval a list inherits when it carries no override of its own. Read per request, so a change applies without a restart.',
        control: { kind: 'int', min: 0, max: U32_MAX },
        mutability: 'live',
        source: `${CONFIG_REFERENCE} [rules] runtime; u32 in schema/rules.rs; absent from ${BOOT_KEYS}`,
      },
    ],
  },
  {
    id: 'schedule',
    note: 'The timezone every policy window is read in.',
    fields: [
      {
        key: 'schedule.timezone',
        label: 'timezone',
        help: 'A POSIX TZ string, not an IANA name — the distroless image ships no timezone database. Bucharest is EET-2EEST,M3.5.0/3,M10.5.0/4; POSIX signs offsets WEST-positive, so EET-2 is UTC+2.',
        control: { kind: 'tz' },
        mutability: 'live',
        source: `${CONFIG_REFERENCE} [schedule] runtime; the grammar is validated server-side by fah-config tz::PosixTz and is deliberately not mirrored here`,
      },
    ],
  },
  {
    id: 'stats',
    note: 'The periodic snapshot to /data, and how long an idle client stays listed.',
    fields: [
      {
        key: 'stats.snapshot_interval_seconds',
        label: 'snapshot_interval_seconds',
        help: 'How often the rolling statistics are written out. The timer is built at startup.',
        control: { kind: 'int', min: 0, max: U32_MAX },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [stats]; u32 in schema/stats.rs; ${BOOT_KEYS} stats.snapshot_interval_seconds`,
      },
      {
        key: 'stats.client_idle_expiry_days',
        label: 'client_idle_expiry_days',
        help: 'An unnamed client not seen for this many days leaves the registry on the next 20 s tick. Named clients never expire. Applied live.',
        control: { kind: 'int', min: 1, max: 3650 },
        mutability: 'live',
        source: `${VALIDATE} validate_range(1, 3650); routes.rs post_config set_client_idle_expiry_days; absent from ${BOOT_KEYS}`,
      },
    ],
  },
  {
    id: 'history',
    note: 'Mixed — two fields apply live, one is read at startup.',
    fields: [
      {
        key: 'history.enabled',
        label: 'enabled',
        help: 'Master switch over what is persisted about traffic: off stops both history writers and the perf sampler. Pushed into the writers, so it applies live.',
        control: { kind: 'bool' },
        mutability: 'live',
        source: `${CONFIG_REFERENCE} [history] runtime; routes.rs post_config apply_history_config`,
      },
      {
        key: 'history.sample_interval_seconds',
        label: 'sample_interval_seconds',
        help: 'One perf sample per interval; at 60 s a day is 1,440 samples. The sampler’s ticker is built at startup.',
        control: { kind: 'int', min: 1, max: 86_400 },
        mutability: 'restart',
        source: `${VALIDATE} validate_range(1, 86400); ${BOOT_KEYS} history.sample_interval_seconds`,
      },
      {
        key: 'history.retention_days',
        label: 'retention_days',
        help: 'Age cap on the /data/history day-files, applied live to the next prune.',
        control: { kind: 'int', min: 1, max: 3650 },
        mutability: 'live',
        source: `${VALIDATE} validate_range(1, 3650); absent from ${BOOT_KEYS}`,
      },
    ],
  },
  {
    id: 'api',
    note: 'The listener this dashboard is reaching right now. Each field moves or removes it, so saving one asks first.',
    fields: [
      {
        key: 'api.address',
        label: 'address',
        help: 'Bind LAN-side only; never expose it to the WAN. A literal IP is also included in the generated certificate’s SANs.',
        control: { kind: 'ip' },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [api]; ${VALIDATE} validate_ip; ${BOOT_KEYS} api.address`,
        consequence:
          'The API and this dashboard move to another address. After the restart this page’s URL stops answering, and the certificate is regenerated around the new one.',
      },
      {
        key: 'api.port',
        label: 'port',
        help: 'The port the dashboard and every API client reach.',
        control: { kind: 'int', min: PORT_MIN, max: PORT_MAX },
        mutability: 'restart',
        source: `${VALIDATE} validate_nonzero_port; ${BOOT_KEYS} api.port`,
        consequence:
          'The API and this dashboard move to another port. After the restart this page’s URL stops answering.',
      },
      {
        key: 'api.tls',
        label: 'tls',
        help: 'TLS on the API listener. The self-signed certificate is generated on first boot.',
        control: { kind: 'bool' },
        mutability: 'restart',
        source: `${CONFIG_REFERENCE} [api] "opt-out is UNSAFE"; ${BOOT_KEYS} api.tls; API.md §Session authentication`,
        consequence:
          'Turning TLS off removes the only origin a Secure __Host- session cookie can exist on. After the restart, signing in stops working: the dashboard cannot authenticate at all, there is no HTTP fallback by design, and only bearer-key access remains. Login then answers 503 with no Retry-After, which never clears on its own.',
      },
    ],
  },
  {
    id: 'log',
    note: 'Both fields become a tracing filter at startup.',
    fields: [
      {
        key: 'log.level',
        label: 'level',
        help: 'How much the container log carries.',
        control: {
          kind: 'enum',
          values: ['error', 'warn', 'info', 'debug', 'trace'],
        },
        mutability: 'restart',
        source: `schema/log.rs LogLevel; ${BOOT_KEYS} log.level`,
      },
      {
        key: 'log.format',
        label: 'format',
        help: 'json for a log shipper, text for a human.',
        control: { kind: 'enum', values: ['text', 'json'] },
        mutability: 'restart',
        source: `schema/log.rs LogFormat; ${BOOT_KEYS} log.format`,
      },
    ],
  },
];

/** Every modelled field, flattened. Sections are the presentation; the patch
 *  builder and the validator work in dotted keys. */
export const FIELDS: readonly FieldMeta[] = SECTIONS.flatMap(
  (section) => section.fields,
);

export function fieldMeta(key: string): FieldMeta | null {
  return FIELDS.find((field) => field.key === key) ?? null;
}

/** The `[api]` keys among a dirty set. A save touching one confirms first. */
export function gatedFields(keys: readonly string[]): readonly FieldMeta[] {
  return FIELDS.filter(
    (field) => field.consequence !== undefined && keys.includes(field.key),
  );
}
