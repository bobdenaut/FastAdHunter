import { Card } from '../../components/card';

/** The panel the task asks for by name: why no share-of-traffic chart exists.
 *  Per-query upstream attribution is deliberately not carried, so the honest
 *  answer is a sentence rather than a chart nobody could source. */
export function NoPieCard() {
  return (
    <Card title="Why this is not a traffic pie">
      <p class="note">
        A per-query “which upstream answered” share cannot be drawn here, and no
        attempt is made to fake one: the address is deliberately not carried per
        query. What a query does report is the <b>answering endpoint index</b>,
        and only when an upstream actually answered — a cache hit or a block
        carries no endpoint at all.
      </p>
      <p class="note">
        So this page charts health, which is what the data actually supports:
        attempts, failures, and how the strategy is currently treating each
        endpoint.
      </p>
    </Card>
  );
}
