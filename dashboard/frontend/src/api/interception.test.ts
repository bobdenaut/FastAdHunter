import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError, NetworkError } from './core';
import {
  documentErrorDetails,
  getInterception,
  putInterception,
  INTERCEPTION_PATH,
} from './interception';

/**
 * The document endpoint's client half: one path, a whole-document `PUT`, and a
 * `422` read through `details` rather than through `message`.
 */

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

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

const DOCUMENT = { clients: ['192.168.88.10'], exclude_domains: ['bank.ro'] };

function rejected(details: unknown, status = 422): ApiError {
  return new ApiError(status, 'validation_failed', 'rejected', null, details);
}

describe('the requests', () => {
  it('reads the document from the one path', async () => {
    fetchMock.mockResolvedValue(respond(200, DOCUMENT));
    await expect(getInterception()).resolves.toEqual(DOCUMENT);
    expect(fetchMock.mock.calls[0]?.[0]).toBe(INTERCEPTION_PATH);
    expect((fetchMock.mock.calls[0]?.[1] as RequestInit).method).toBe('GET');
  });

  it('sends the whole document on a PUT and returns what was stored', async () => {
    const stored = { clients: [], exclude_domains: ['bank.ro'] };
    fetchMock.mockResolvedValue(respond(200, stored));
    await expect(putInterception({ clients: [], exclude_domains: ['bank.ro'] }))
      .resolves.toEqual(stored);
    const init = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect(init.method).toBe('PUT');
    expect(JSON.parse(String(init.body))).toEqual({
      clients: [],
      exclude_domains: ['bank.ro'],
    });
  });

  it('carries the 422 details through the request path onto the error', async () => {
    fetchMock.mockResolvedValue(
      respond(422, {
        error: {
          code: 'validation_failed',
          message: 'clients[3]: "10.0.0.300" is not an IP address or CIDR block',
          details: {
            reason: 'invalid_entry',
            list: 'clients',
            index: 3,
            entry: '10.0.0.300',
          },
        },
      }),
    );
    const error = await putInterception(DOCUMENT).catch((e: unknown) => e);
    expect(documentErrorDetails(error)).toEqual({
      reason: 'invalid_entry',
      list: 'clients',
      index: 3,
      entry: '10.0.0.300',
    });
  });
});

describe('documentErrorDetails', () => {
  it('accepts each of the four documented shapes', () => {
    expect(documentErrorDetails(rejected({ reason: 'shape' }))).toEqual({
      reason: 'shape',
    });
    expect(
      documentErrorDetails(
        rejected({ reason: 'over_cap', list: 'clients', len: 300, cap: 256 }),
      ),
    ).toEqual({ reason: 'over_cap', list: 'clients', len: 300, cap: 256 });
    expect(
      documentErrorDetails(
        rejected({
          reason: 'invalid_entry',
          list: 'exclude_domains',
          index: 2,
          entry: 'not a host',
        }),
      ),
    ).toEqual({
      reason: 'invalid_entry',
      list: 'exclude_domains',
      index: 2,
      entry: 'not a host',
    });
    expect(
      documentErrorDetails(
        rejected({
          reason: 'duplicate',
          list: 'exclude_domains',
          index: 7,
          entry: 'Bank.ro.',
          duplicate_of: 2,
        }),
      ),
    ).toEqual({
      reason: 'duplicate',
      list: 'exclude_domains',
      index: 7,
      entry: 'Bank.ro.',
      duplicate_of: 2,
    });
  });

  it('refuses anything that is not one of them', () => {
    // The consumers branch on `reason` alone, so a half-shape has to read as
    // "no structured detail" rather than as a shape with holes in it.
    expect(documentErrorDetails(rejected(null))).toBeNull();
    expect(documentErrorDetails(rejected({ reason: 'future_reason' }))).toBeNull();
    expect(
      documentErrorDetails(rejected({ reason: 'over_cap', list: 'clients' })),
    ).toBeNull();
    expect(
      documentErrorDetails(
        rejected({ reason: 'invalid_entry', list: 'nope', index: 1, entry: 'x' }),
      ),
    ).toBeNull();
    expect(
      documentErrorDetails(
        rejected({ reason: 'duplicate', list: 'clients', index: 1, entry: 'x' }),
      ),
    ).toBeNull();
  });

  it('refuses a 422 with no details, another status, and a non-ApiError', () => {
    expect(documentErrorDetails(rejected(null))).toBeNull();
    expect(
      documentErrorDetails(new ApiError(422, 'validation_failed', 'x', null)),
    ).toBeNull();
    expect(
      documentErrorDetails(rejected({ reason: 'shape' }, 503)),
    ).toBeNull();
    expect(documentErrorDetails(new NetworkError('down'))).toBeNull();
    expect(documentErrorDetails('not an error')).toBeNull();
  });
});
