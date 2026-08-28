import { describe, expect, it } from 'vitest';
import type { Config } from '../../api/types';
import {
  FIELDS,
  SECTIONS,
  fieldMeta,
  gatedFields,
  type FieldMeta,
} from './metadata';
import {
  anchorError,
  buildPatch,
  dirtyKeys,
  fieldError,
  fieldValue,
  looksLikeDestination,
  looksLikeIp,
  readPath,
  rebase,
  setEdit,
  validateEdits,
  type Edits,
} from './patch';

/**
 * The write path, as arithmetic. The page test asserts the same
 * changed-keys-only property on the actual `fetch` body; this asserts it on the
 * object, where every branch is reachable without a DOM.
 */

function config(over: Record<string, unknown> = {}): Config {
  return {
    engine: { mode: 'dns' },
    dns: {
      listen: { address: '::', port: 53 },
      blocking: { mode: 'null_ip', ttl_seconds: 10 },
      cache: {
        max_entries: 10_000,
        max_bytes: 67_108_864,
        min_ttl_seconds: 0,
        max_ttl_seconds: 86_400,
        negative_ttl_max_seconds: 60,
        serve_stale: true,
        swr_workers: 3,
        cleanup_interval_seconds: 360,
      },
      upstreams: {
        strategy: 'fallback',
        timeout_ms: 800,
        penalty_failures: 2,
        servers: [
          { address: '1.1.1.1', protocol: 'udp', hostname: null },
          { address: '9.9.9.9', protocol: 'udp', hostname: null },
        ],
      },
    },
    http: {
      listen: { address: '::', port: 8080 },
      max_connections: 1024,
      idle_timeout_ms: 60_000,
      header_timeout_ms: 10_000,
    },
    egress: { allow_destinations: [], allow_ip_literal_hosts: false },
    rules: {
      refresh_hours_default: 24,
      lists: [
        {
          id: 'oisd-basic',
          url: 'https://small.oisd.nl',
          enabled: true,
          refresh_hours: null,
        },
      ],
    },
    schedule: { timezone: 'UTC' },
    stats: { snapshot_interval_seconds: 300 },
    history: { enabled: true, sample_interval_seconds: 60, retention_days: 30 },
    api: { address: '0.0.0.0', port: 8443, tls: true },
    log: { level: 'info', format: 'text' },
    ...over,
  } as Config;
}

describe('the metadata module', () => {
  // The acceptance criterion is that no bound, enum or class comes from the API
  // response. That is only checkable if every one of them carries the document
  // or the Rust it was carried from.
  it('anchors every field to where its bound came from', () => {
    for (const field of FIELDS) {
      expect(field.source, field.key).not.toBe('');
      expect(field.help, field.key).not.toBe('');
    }
  });

  it('models no key the endpoint rejects', () => {
    // `rules.lists`, `policies` and `auth.*` are 422 by design — each has
    // exactly one writer, and none of them is this form.
    for (const field of FIELDS) {
      expect(field.key.startsWith('rules.lists')).toBe(false);
      expect(field.key.startsWith('policies')).toBe(false);
      expect(field.key.startsWith('auth')).toBe(false);
    }
  });

  it('offers `null_ip` and nothing else as the blocking mode', () => {
    // CONFIGURATION.md documents nxdomain/refused/custom as future values; the
    // schema deserialises exactly one, so the select offers exactly one.
    const mode = fieldMeta('dns.blocking.mode');
    expect(mode?.control).toEqual({ kind: 'enum', values: ['null_ip'] });
  });

  it('gates the three `[api]` keys and nothing else', () => {
    const gated = FIELDS.filter((field) => field.consequence !== undefined);
    expect(gated.map((field) => field.key)).toEqual([
      'api.address',
      'api.port',
      'api.tls',
    ]);
  });

  it('names the lock-out in the TLS consequence', () => {
    const tls = fieldMeta('api.tls');
    expect(tls?.consequence).toContain('signing in stops working');
    expect(tls?.consequence).toContain('bearer');
  });

  it('classifies exactly the four runtime keys as live', () => {
    // CONFIGURATION.md §Mutability classes, cross-checked against
    // `config_store.rs` BOOT_KEYS: everything not listed there is runtime.
    const live = FIELDS.filter((field) => field.mutability === 'live');
    expect(live.map((field) => field.key).sort()).toEqual([
      'history.enabled',
      'history.retention_days',
      'rules.refresh_hours_default',
      'schedule.timezone',
    ]);
  });

  it('gives every section at least one field and a unique id', () => {
    const ids = SECTIONS.map((section) => section.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const section of SECTIONS) expect(section.fields.length).toBeGreaterThan(0);
  });

  it('finds the gated fields in a dirty set', () => {
    expect(
      gatedFields(['dns.cache.max_entries', 'api.tls']).map((f) => f.key),
    ).toEqual(['api.tls']);
    expect(gatedFields(['dns.cache.max_entries'])).toEqual([]);
  });
});

describe('reading a value', () => {
  it('follows the dotted key into the response', () => {
    expect(readPath(config(), 'dns.cache.max_entries')).toBe(10_000);
    expect(readPath(config(), 'api.tls')).toBe(true);
  });

  it('is undefined for a key the response does not carry', () => {
    expect(readPath({ dns: {} }, 'dns.cache.max_entries')).toBeUndefined();
    expect(readPath(null, 'api.tls')).toBeUndefined();
  });

  it('prefers the edit over the baseline', () => {
    const edits: Edits = { 'dns.cache.max_entries': 20_000 };
    expect(fieldValue(config(), edits, 'dns.cache.max_entries')).toBe(20_000);
    expect(fieldValue(config(), edits, 'api.port')).toBe(8443);
  });
});

describe('dirty tracking', () => {
  it('marks a changed field and leaves the rest alone', () => {
    const edits = setEdit(config(), {}, 'dns.cache.max_entries', 20_000);
    expect(dirtyKeys(edits)).toEqual(['dns.cache.max_entries']);
  });

  it('clears the mark when the value is typed back', () => {
    let edits = setEdit(config(), {}, 'dns.cache.max_entries', 20_000);
    edits = setEdit(config(), edits, 'dns.cache.max_entries', 10_000);
    expect(dirtyKeys(edits)).toEqual([]);
  });

  it('compares arrays by content, not by identity', () => {
    const same = [
      { address: '1.1.1.1', protocol: 'udp', hostname: null },
      { address: '9.9.9.9', protocol: 'udp', hostname: null },
    ];
    const edits = setEdit(config(), {}, 'dns.upstreams.servers', same);
    expect(dirtyKeys(edits)).toEqual([]);
  });

  it('keeps a dirty field across a baseline replacement', () => {
    // `config_changed` re-reads; the operator's unsaved edit is theirs.
    const edits = setEdit(config(), {}, 'dns.cache.max_entries', 20_000);
    const fresh = config({ log: { level: 'debug', format: 'text' } });
    expect(dirtyKeys(rebase(fresh, edits))).toEqual(['dns.cache.max_entries']);
  });

  it('drops an edit the server has since made itself', () => {
    const edits = setEdit(config(), {}, 'log.level', 'debug');
    const fresh = config({ log: { level: 'debug', format: 'text' } });
    expect(dirtyKeys(rebase(fresh, edits))).toEqual([]);
  });
});

describe('the request body', () => {
  it('is the changed keys, nested, and nothing else', () => {
    const edits = setEdit(config(), {}, 'dns.cache.max_entries', 20_000);
    expect(buildPatch(edits)).toEqual({ dns: { cache: { max_entries: 20_000 } } });
  });

  it('merges two keys of one section into one object', () => {
    let edits = setEdit(config(), {}, 'dns.cache.max_entries', 20_000);
    edits = setEdit(config(), edits, 'dns.cache.serve_stale', false);
    expect(buildPatch(edits)).toEqual({
      dns: { cache: { max_entries: 20_000, serve_stale: false } },
    });
  });

  it('merges two sections without either erasing the other', () => {
    let edits = setEdit(config(), {}, 'dns.cache.max_entries', 20_000);
    edits = setEdit(config(), edits, 'dns.listen.port', 5353);
    expect(buildPatch(edits)).toEqual({
      dns: { cache: { max_entries: 20_000 }, listen: { port: 5353 } },
    });
  });

  it('sends an array whole, because the server merge replaces arrays', () => {
    const servers = [
      { address: '1.1.1.1', protocol: 'dot', hostname: 'cloudflare-dns.com' },
      { address: '9.9.9.9', protocol: 'udp', hostname: null },
    ];
    const edits = setEdit(config(), {}, 'dns.upstreams.servers', servers);
    expect(buildPatch(edits)).toEqual({ dns: { upstreams: { servers } } });
  });

  it('is empty when nothing was touched', () => {
    expect(buildPatch({})).toEqual({});
  });

  it('never carries a key the operator did not change', () => {
    let edits = setEdit(config(), {}, 'history.retention_days', 60);
    edits = setEdit(config(), edits, 'history.enabled', true);
    // `enabled` was already true, so only one key travels.
    expect(buildPatch(edits)).toEqual({ history: { retention_days: 60 } });
  });
});

describe('the client-side bounds', () => {
  function check(key: string, value: unknown): string | null {
    const meta = fieldMeta(key) as FieldMeta;
    return fieldError(meta, value);
  }

  it('accepts and refuses at each numeric edge', () => {
    for (const [key, low, high] of [
      ['dns.upstreams.timeout_ms', 1, 10_000],
      ['dns.upstreams.penalty_failures', 1, 255],
      ['history.sample_interval_seconds', 1, 86_400],
      ['history.retention_days', 1, 3650],
      ['dns.listen.port', 1, 65_535],
      ['api.port', 1, 65_535],
    ] as const) {
      expect(check(key, low), `${key} low edge`).toBeNull();
      expect(check(key, high), `${key} high edge`).toBeNull();
      expect(check(key, low - 1), `${key} under`).not.toBeNull();
      expect(check(key, high + 1), `${key} over`).not.toBeNull();
    }
  });

  it('holds the cache byte floor at 1 MiB', () => {
    expect(check('dns.cache.max_bytes', 1_048_576)).toBeNull();
    expect(check('dns.cache.max_bytes', 1_048_575)).not.toBeNull();
  });

  it('holds the connection ceiling at one', () => {
    expect(check('http.max_connections', 1)).toBeNull();
    expect(check('http.max_connections', 0)).not.toBeNull();
  });

  it('refuses a fraction where the schema takes an integer', () => {
    expect(check('dns.cache.max_entries', 10.5)).not.toBeNull();
  });

  it('refuses an enum value the schema does not deserialise', () => {
    expect(check('dns.blocking.mode', 'null_ip')).toBeNull();
    expect(check('dns.blocking.mode', 'nxdomain')).not.toBeNull();
    expect(check('dns.upstreams.strategy', 'adaptive')).toBeNull();
    expect(check('dns.upstreams.strategy', 'roundrobin')).not.toBeNull();
  });

  it('accepts the IP forms the server parses', () => {
    for (const value of ['0.0.0.0', '192.168.10.5', '::', '::1', 'fe80::1']) {
      expect(looksLikeIp(value), value).toBe(true);
    }
    for (const value of ['', 'localhost', '256.1.1.1', '1.2.3']) {
      expect(looksLikeIp(value), value).toBe(false);
    }
  });

  it('refuses a leading zero, which the server also refuses', () => {
    // The guard compared a number to itself, so it never ran: `01.0.0.1`
    // reached a `validate_ip` whose `str::parse::<IpAddr>()` rejects it, and
    // this check exists precisely so the form never accepts what the server
    // will not.
    for (const value of ['01.0.0.1', '192.168.010.5', '1.2.3.04']) {
      expect(looksLikeIp(value), value).toBe(false);
    }
    // A bare zero octet is not a leading zero.
    expect(looksLikeIp('0.0.0.0')).toBe(true);
    expect(looksLikeIp('10.0.0.1')).toBe(true);
  });

  it('accepts an address or a CIDR block in the egress list', () => {
    for (const value of ['192.168.10.50', '192.168.10.0/24', '::1', 'fd00::/8']) {
      expect(looksLikeDestination(value), value).toBe(true);
    }
    for (const value of ['192.168.10.0/33', 'example.com', '10.0.0.0/8/8']) {
      expect(looksLikeDestination(value), value).toBe(false);
    }
  });

  it('mirrors the two per-protocol upstream rules', () => {
    expect(
      check('dns.upstreams.servers', [
        { address: '1.1.1.1', protocol: 'dot', hostname: null },
      ]),
    ).toContain('requires a hostname');
    expect(
      check('dns.upstreams.servers', [
        { address: '1.1.1.1', protocol: 'doh', hostname: null },
      ]),
    ).toContain('https URL');
    expect(
      check('dns.upstreams.servers', [
        { address: 'https://dns.example/dns-query', protocol: 'doh', hostname: null },
      ]),
    ).toBeNull();
  });

  it('holds the upstream row count between one and eight', () => {
    const row = { address: '1.1.1.1', protocol: 'udp', hostname: null };
    expect(check('dns.upstreams.servers', [])).toContain('at least one');
    expect(check('dns.upstreams.servers', Array(8).fill(row))).toBeNull();
    expect(check('dns.upstreams.servers', Array(9).fill(row))).toContain('at most 8');
  });

  it('checks the timezone for presence only, and says why', () => {
    // The POSIX grammar is a real parser server-side; half of one here would
    // reject strings the server accepts.
    expect(check('schedule.timezone', 'EET-2EEST,M3.5.0/3,M10.5.0/4')).toBeNull();
    expect(check('schedule.timezone', 'Europe/Bucharest')).toBeNull();
    expect(check('schedule.timezone', '  ')).not.toBeNull();
  });
});

describe('the cross-field cache rule', () => {
  it('refuses a floor above the ceiling', () => {
    const edits = setEdit(config(), {}, 'dns.cache.min_ttl_seconds', 90_000);
    expect(validateEdits(config(), edits)).toEqual([
      {
        key: 'dns.cache.min_ttl_seconds',
        message: 'must be <= max_ttl_seconds (90,000 > 86,400)',
      },
    ]);
  });

  it('accepts raising both in one save', () => {
    let edits = setEdit(config(), {}, 'dns.cache.min_ttl_seconds', 90_000);
    edits = setEdit(config(), edits, 'dns.cache.max_ttl_seconds', 100_000);
    expect(validateEdits(config(), edits)).toEqual([]);
  });

  it('says nothing while neither is dirty', () => {
    const skewed = config({
      dns: { ...config().dns, cache: { ...config().dns.cache, min_ttl_seconds: 99_999 } },
    });
    expect(validateEdits(skewed, {})).toEqual([]);
  });
});

describe('anchoring a 422', () => {
  it('pins the server message under the field it names', () => {
    expect(
      anchorError('invalid value for `dns.cache.max_bytes`: must be at least 1048576 (got 4)'),
    ).toEqual({
      key: 'dns.cache.max_bytes',
      message: 'must be at least 1048576 (got 4)',
    });
  });

  it('pins a per-row rejection on the row editor above it', () => {
    expect(
      anchorError(
        'invalid value for `dns.upstreams.servers.hostname`: dot upstream 1.1.1.1 requires a hostname for certificate verification',
      ).key,
    ).toBe('dns.upstreams.servers');
  });

  it('falls back to the whole message when no field is named', () => {
    const message = 'unknown field `nope`, expected one of `engine`, `dns`';
    expect(anchorError(message)).toEqual({ key: null, message });
  });

  it('does not anchor a message that merely contains backticks', () => {
    // A loose "first backticked token" match would pin this under `engine`.
    const message = 'policies is not settable here: use `/api/v1/policies`';
    expect(anchorError(message).key).toBeNull();
  });
});
