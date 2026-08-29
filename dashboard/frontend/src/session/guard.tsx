import { useEffect } from 'preact/hooks';
import { installSessionGuard } from './session';

/**
 * The guard is shell-level, not per-page: one `401` handler installed once,
 * with the router's navigate behind it. A page never handles `401`.
 */
export function useSessionGuard(): void {
  useEffect(installSessionGuard, []);
}
