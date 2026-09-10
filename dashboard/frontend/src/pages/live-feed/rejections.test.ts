import { describe, expect, it } from 'vitest';
import type { InterceptionDocument, QueryEvent } from '../../api/types';
import {
  exclusionsOf,
  groupRejections,
  isExcluded,
  isRejection,
  normalizeHost,
  withExclusion,
} from './rejections';

/**
 * The rejection view's whole logic, away from the DOM: what counts as a
 * rejection, how the ring's rows become one decision per client and host, and
 * what the exclude action may send.
 */

function event(over: Partial<QueryEvent> = {}): QueryEvent {
  return {
    kind: 'https',
    ts: '2026-09-10T10:41:03.610Z',
    client: '192.168.88.10',
    client_name: null,
    domain: 'api.bank.example',
    qtype: null,
    verdict: 'pass',
    rule: null,
    list: null,
    duration_ms: 1.5,
    upstream: null,
    cached: false,
    method: null,
    path: null,
    resource_type: null,
    status: 525,
    bytes: null,
    ...over,
  };
}

describe('what counts as a rejection', () => {
  it('is an https row with status 525 and nothing else', () => {
    expect(isRejection(event())).toBe(true);
    expect(isRejection(event({ status: 200 }))).toBe(false);
    expect(isRejection(event({ kind: 'https-sni' }))).toBe(false);
    expect(isRejection(event({ kind: 'dns', status: null }))).toBe(false);
  });

  it('leaves status 0 alone', () => {
    // `UnknownCA` and every unclassified accept failure stay `0` (ADR-0008
    // step 2). A missing CA is a diagnosis, not a host that refuses us, and
    // excluding the host would be the wrong fix for it.
    expect(isRejection(event({ status: 0 }))).toBe(false);
  });
});

describe('grouping', () => {
  it('counts one row per client and host, newest first', () => {
    const groups = groupRejections([
      event({ ts: '2026-09-10T10:00:00.000Z', domain: 'x.example' }),
      event({ ts: '2026-09-10T10:00:01.000Z', domain: 'x.example' }),
      event({ ts: '2026-09-10T10:00:02.000Z', domain: 'x.example' }),
      event({
        ts: '2026-09-10T10:00:03.000Z',
        domain: 'x.example',
        client: '192.168.88.11',
      }),
      event({ status: 200, domain: 'ignored.example' }),
      event({ kind: 'https-sni', verdict: 'block', domain: 'ignored.example' }),
      event({ kind: 'dns', status: null, domain: 'ignored.example' }),
    ]);

    expect(groups).toEqual([
      {
        client: '192.168.88.11',
        clientName: null,
        host: 'x.example',
        count: 1,
        last: '2026-09-10T10:00:03.000Z',
      },
      {
        client: '192.168.88.10',
        clientName: null,
        host: 'x.example',
        count: 3,
        last: '2026-09-10T10:00:02.000Z',
      },
    ]);
  });

  it('orders by instant, not by string, inside one second', () => {
    // The API trims the fraction: `…:00Z`, `…:00.25Z` and `…:00.5Z` are one
    // second's worth of `ts` values, and lexically `Z` sorts above `.`.
    const groups = groupRejections([
      event({ ts: '2026-09-10T10:00:00Z', domain: 'a.example' }),
      event({ ts: '2026-09-10T10:00:00.25Z', domain: 'b.example' }),
      event({ ts: '2026-09-10T10:00:00.5Z', domain: 'c.example' }),
    ]);
    expect(groups.map((group) => group.host)).toEqual([
      'c.example',
      'b.example',
      'a.example',
    ]);
  });

  it('keeps a client name once any row carries one', () => {
    const groups = groupRejections([
      event({ ts: '2026-09-10T10:00:00.000Z', client_name: 'phone' }),
      event({ ts: '2026-09-10T10:00:01.000Z', client_name: null }),
    ]);
    expect(groups[0]?.clientName).toBe('phone');
    expect(groups[0]?.count).toBe(2);
  });

  it('is empty over an empty ring', () => {
    expect(groupRejections([])).toEqual([]);
  });
});

describe('the exclusion check', () => {
  it('normalizes the way the matcher does', () => {
    expect(normalizeHost('  Bank.RO.  ')).toBe('bank.ro');
    expect(normalizeHost('bank.ro')).toBe('bank.ro');
  });

  it('matches exactly and under a parent, never a sibling', () => {
    const list = exclusionsOf(['bank.ro', 'Api.Example.']);
    expect(isExcluded('bank.ro', list)).toBe(true);
    expect(isExcluded('m.bank.ro', list)).toBe(true);
    expect(isExcluded('a.b.bank.ro', list)).toBe(true);
    expect(isExcluded('api.example', list)).toBe(true);
    expect(isExcluded('notbank.ro', list)).toBe(false);
    expect(isExcluded('ro', list)).toBe(false);
    expect(isExcluded('bank.ro', exclusionsOf([]))).toBe(false);
  });
});

describe('what the exclude action sends', () => {
  const document: InterceptionDocument = {
    clients: ['192.168.88.0/24'],
    exclude_domains: ['bank.ro'],
  };

  it('appends exactly the observed host, spelling kept, order kept', () => {
    expect(withExclusion(document, 'API.Bank.example')).toEqual({
      clients: ['192.168.88.0/24'],
      exclude_domains: ['bank.ro', 'API.Bank.example'],
    });
  });

  it('never widens and never touches the client list', () => {
    const next = withExclusion(document, 'api.foo.co.uk');
    expect(next.exclude_domains).toContain('api.foo.co.uk');
    expect(next.exclude_domains).not.toContain('foo.co.uk');
    expect(next.exclude_domains).not.toContain('co.uk');
    expect(next.clients).toEqual(document.clients);
  });

  it('leaves the document it was given untouched', () => {
    withExclusion(document, 'x.example');
    expect(document.exclude_domains).toEqual(['bank.ro']);
  });

  it('does not dedupe — the server is the only authority on that', () => {
    expect(withExclusion(document, 'bank.ro').exclude_domains).toEqual([
      'bank.ro',
      'bank.ro',
    ]);
  });
});
