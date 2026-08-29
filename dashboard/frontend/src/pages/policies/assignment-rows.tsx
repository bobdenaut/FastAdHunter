import type { Assignment } from '../../api/types';
import { scheduleText } from '../../policy/assignment';

/**
 * The assignment rows on a policy card: the selector as configured, and its
 * schedule as words.
 *
 * **No per-assignment in-force claim** (D4). `GET /policies` exposes one global
 * `active_assignments` and no per-assignment flag, so the green/grey dot and
 * the `ACTIVE NOW` tag the artboard draws are dropped rather than guessed at —
 * deciding one in the browser would need a clock and the POSIX timezone rules,
 * which is exactly the arithmetic this task keeps out of the bundle.
 *
 * **No resolved client name either** (X3). The artboard prints `tv` beside
 * `192.168.10.50`; that name lives only in `GET /clients`, which this page
 * does not fetch. A third request for one label is not worth it, and the
 * Clients page is one click away.
 */
export function AssignmentRows({
  assignments,
}: {
  assignments: readonly Assignment[];
}) {
  if (assignments.length === 0) {
    return <p class="note assignments-empty">No client is assigned to it.</p>;
  }
  return (
    <div class="assignments">
      {assignments.map((assignment, index) => (
        <div class="assignment" key={`${assignment.client}-${String(index)}`}>
          <span class="mono">{assignment.client}</span>
          <span class="note mono assignment-schedule">
            {scheduleText(assignment)}
          </span>
        </div>
      ))}
    </div>
  );
}
