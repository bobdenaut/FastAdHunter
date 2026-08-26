import { NotYetBuilt } from './not-yet-built';
import type { Route } from '../router/routes';

/**
 * The Settings and Diagnostics group loads as its own chunk. It holds only the
 * not-yet-built page in p5-05 and is split anyway: it proves route-level lazy
 * loading end to end, and p5-09 fills it.
 */
export function SystemPlaceholder({ route }: { route: Route }) {
  return <NotYetBuilt route={route} />;
}

export default SystemPlaceholder;
