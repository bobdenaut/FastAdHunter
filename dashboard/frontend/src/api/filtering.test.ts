import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  clearClientPolicy,
  setClientName,
  setClientPolicy,
} from './clients';
import { setUnauthorizedHandler } from './core';
import { createPolicy, deletePolicy, patchPolicy, getPolicies } from './policies';
import { getUserRules, putUserRules, testRule } from './rules';

function respond(status: number, body?: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: () => null },
    json: async () => {
      if (body === undefined) throw new Error('no body');
      return body;
    },
  } as unknown as Response;
}

const fetchMock = vi.fn();

function calledWith(): [string, RequestInit] {
  const call = fetchMock.mock.calls[0];
  return [call?.[0] as string, call?.[1] as RequestInit];
}

function sentBody(): string {
  return calledWith()[1].body as string;
}

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  setUnauthorizedHandler(null);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the policies accessors', () => {
  it('reads the collection from one path', async () => {
    fetchMock.mockResolvedValue(
      respond(200, { timezone: 'UTC', items: [], active_assignments: 0 }),
    );
    await getPolicies();
    expect(calledWith()[0]).toBe('/api/v1/policies');
    expect(calledWith()[1].method).toBe('GET');
  });

  // The double option. `null` clears the subset back to "every enabled list";
  // dropping the key would leave it alone instead, which is a different write.
  it('emits `lists: null` rather than dropping the key', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await patchPolicy('kids', { lists: null });
    expect(JSON.parse(sentBody())).toEqual({ lists: null });
    expect(sentBody()).toContain('"lists":null');
  });

  it('emits `blocking_mode: null` rather than dropping the key', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await patchPolicy('kids', { blocking_mode: null });
    expect(JSON.parse(sentBody())).toEqual({ blocking_mode: null });
  });

  it('sends only the fields the caller set', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await patchPolicy('kids', { assignments: [{ client: '192.168.1.5' }] });
    expect(JSON.parse(sentBody())).toEqual({
      assignments: [{ client: '192.168.1.5' }],
    });
  });

  it('escapes an id into the path', async () => {
    fetchMock.mockResolvedValue(respond(204));
    await deletePolicy('a b/c');
    expect(calledWith()[0]).toBe('/api/v1/policies/a%20b%2Fc');
  });

  it('returns without parsing on the 204 a delete answers with', async () => {
    fetchMock.mockResolvedValue(respond(204));
    await expect(deletePolicy('kids')).resolves.toBeUndefined();
  });

  it('creates with POST', async () => {
    fetchMock.mockResolvedValue(respond(201, {}));
    await createPolicy({ id: 'kids', lists: ['oisd'] });
    expect(calledWith()[1].method).toBe('POST');
    expect(JSON.parse(sentBody())).toEqual({ id: 'kids', lists: ['oisd'] });
  });
});

describe('the client mutation accessors', () => {
  it('clears a name with a literal null', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await setClientName('192.168.1.5', null);
    expect(calledWith()[0]).toBe('/api/v1/clients/192.168.1.5');
    expect(calledWith()[1].method).toBe('PUT');
    expect(JSON.parse(sentBody())).toEqual({ name: null });
  });

  it('escapes an address into the policy path', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await setClientPolicy('fe80::1%eth0', { policy: 'kids' });
    expect(calledWith()[0]).toBe('/api/v1/clients/fe80%3A%3A1%25eth0/policy');
  });

  it('sends a schedule only when one was given', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await setClientPolicy('192.168.1.5', { policy: 'kids' });
    expect(JSON.parse(sentBody())).toEqual({ policy: 'kids' });
  });

  it('returns without parsing on the 204 a clear answers with', async () => {
    fetchMock.mockResolvedValue(respond(204));
    await expect(clearClientPolicy('192.168.1.5')).resolves.toBeUndefined();
  });
});

describe('the rules accessors', () => {
  it('reads the document as lines', async () => {
    fetchMock.mockResolvedValue(respond(200, { rules: ['||a.example^'] }));
    const document = await getUserRules();
    expect(calledWith()[0]).toBe('/api/v1/rules/user');
    expect(document.rules).toEqual(['||a.example^']);
  });

  it('writes the whole document under `rules`', async () => {
    fetchMock.mockResolvedValue(respond(200, { rules: [] }));
    await putUserRules(['||a.example^', '']);
    expect(calledWith()[1].method).toBe('PUT');
    expect(JSON.parse(sentBody())).toEqual({ rules: ['||a.example^', ''] });
  });

  it('posts a test to its own path', async () => {
    fetchMock.mockResolvedValue(
      respond(200, { verdict: 'pass', rule: null, list: null, policy: 'default' }),
    );
    const result = await testRule({ domain: 'example.com', qtype: 'A' });
    expect(calledWith()[0]).toBe('/api/v1/rules/test');
    expect(calledWith()[1].method).toBe('POST');
    expect(result.verdict).toBe('pass');
  });
});
