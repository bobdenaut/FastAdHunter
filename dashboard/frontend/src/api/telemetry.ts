import { request } from './core';
import type { Telemetry } from './types';

export const TELEMETRY_PATH = '/api/v1/telemetry';

export function getTelemetry(signal?: AbortSignal): Promise<Telemetry> {
  return request<Telemetry>(TELEMETRY_PATH, {
    ...(signal === undefined ? {} : { signal }),
  });
}
