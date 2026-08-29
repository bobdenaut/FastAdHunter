import { Card } from '../../components/card';

/**
 * The contract §10 implements, stated on the page rather than only enforced.
 * A confirming dialog names the cost in the API's own terms and a
 * non-confirming action carries the LIVE treatment this card explains.
 *
 * The rule count is `compiled_rules` from the `GET /lists` response this page
 * already reads — the artboard's `752,585` is a drawing, and a figure on a
 * shipped page comes from a field.
 */
export function CostCard({ compiledRules }: { compiledRules: number | null }) {
  return (
    <Card title="What costs what" bodyClass="cost-body">
      <div>
        <div class="cost-head">
          <span class="pill warn">recompiles</span> seconds of CPU on the router
        </div>
        <p class="note" style={{ margin: 0 }}>
          Creating a policy, changing which lists it holds, deleting one. Per-rule
          policy masks are built at compile time over{' '}
          {compiledRules === null
            ? 'the whole compiled ruleset'
            : `${compiledRules.toLocaleString()} rules`}
          , so the whole ruleset is rebuilt and swapped. The UI asks before doing
          any of these, then waits for the rebuild rather than reporting success
          early.
        </p>
      </div>
      <div>
        <div class="cost-head">
          <span class="pill good">live</span> milliseconds
        </div>
        <p class="note" style={{ margin: 0 }}>
          Renaming a policy, changing its blocking-mode override, and assigning
          or unassigning a client. Assignments change no mask, so they apply at
          once and need no warning.
        </p>
      </div>
    </Card>
  );
}

export function CostFootnote() {
  return (
    <p class="note cost-footnote">
      One assignment per address — assigning a client already covered elsewhere
      replaces that assignment, in whichever policy held it. A shut schedule
      window is not &ldquo;no policy&rdquo;: the client falls back to whatever
      else covers it, a subnet assignment or <span class="mono">default</span>.
    </p>
  );
}
