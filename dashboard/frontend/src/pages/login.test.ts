import { describe, expect, it } from 'vitest';
import { ApiError, NetworkError } from '../api/core';
import { failureFor } from './login';

describe('the documented login failures', () => {
  it('renders a wrong password without claiming which 401 it was', () => {
    const failure = failureFor(
      new ApiError(401, 'unauthorized', 'unauthorized', null),
    );
    expect(failure.message).toBe('That password is not right.');
    expect(failure.retryAfter).toBeNull();
  });

  it('counts down a 429 from its Retry-After', () => {
    const failure = failureFor(
      new ApiError(429, 'rate_limited', 'slow down', 27),
    );
    expect(failure.retryAfter).toBe(27);
  });

  it('counts down a saturated 503', () => {
    const failure = failureFor(
      new ApiError(503, 'unavailable', 'saturated', 1),
    );
    expect(failure.message).toContain('saturated');
    expect(failure.retryAfter).toBe(1);
  });

  it('runs no timer for the api.tls = false 503, and says why', () => {
    const failure = failureFor(
      new ApiError(503, 'unavailable', 'unavailable', null),
    );
    expect(failure.retryAfter).toBeNull();
    expect(failure.message).toContain('not serving TLS');
    expect(failure.message).toContain('restarts');
  });

  it('shows the envelope’s own message for a malformed request', () => {
    const failure = failureFor(
      new ApiError(400, 'bad_request', 'missing field: password', null),
    );
    expect(failure.message).toBe('missing field: password');
    expect(failure.retryAfter).toBeNull();
  });

  it('separates "the API said no" from "the API did not answer"', () => {
    expect(failureFor(new NetworkError('gone')).message).toBe(
      'The API did not answer.',
    );
    expect(failureFor(new Error('?')).message).toBe('Sign-in failed.');
  });
});
