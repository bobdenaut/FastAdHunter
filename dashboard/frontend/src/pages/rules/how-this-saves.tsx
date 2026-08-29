import { Card } from '../../components/card';

/**
 * The two side cards, both static copy. The wording is `CustomRules.dc.html`'s
 * with one sentence replaced: the artboard said "No restart, no recompile of
 * the lists", and `set_user_rules` does run a full `compile()` and `swap_in` —
 * the same rebuild the Policies card labels RECOMPILES. What is true is that
 * no list is refetched, which is the owner's wording below.
 */
export function HowThisSaves() {
  return (
    <Card title="How this page saves">
      <p class="note" style={{ margin: 0 }}>
        The API takes lines in and gives lines out — there is no per-rule
        identity behind it, so this is a document, not a table.
        <span class="stacked">
          That is why there is no per-rule delete button and no per-rule enable
          toggle: they would promise a write the API cannot perform.
        </span>
        <span class="stacked">
          <b>Save is all-or-nothing.</b> Every line is validated, then the whole
          set is swapped atomically. One bad line means nothing is written —
          never a partial save.
        </span>
        <span class="stacked">
          A successful save applies immediately. <b>No restart and no list
          refetch. The ruleset is rebuilt and atomically swapped</b> — which
          takes seconds, so the save waits for it rather than reporting success
          early.
        </span>
      </p>
    </Card>
  );
}

export function Precedence() {
  return (
    <Card title="Precedence">
      <div class="rank-list">
        <div class="rank-row">
          <span class="rank rank-1">1</span>
          <span>
            <span class="mono">@@</span> exceptions win
          </span>
        </div>
        <div class="rank-row">
          <span class="rank rank-2">2</span>
          <span>blocking rules</span>
        </div>
        <div class="rank-row">
          <span class="rank rank-3">3</span>
          <span>otherwise the query passes</span>
        </div>
      </div>
      <p class="note rank-footnote">
        To check what a rule actually does for a given client, use the{' '}
        <b>Rule Tester</b> — it runs the real engine rather than matching text.
      </p>
    </Card>
  );
}
