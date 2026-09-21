import { Card } from '../../components/card';

/** The artboard's three explainer columns. The third is the one the phase's
 *  standing constraint 6 exists for: a budget is a marker, never a wall. */
export function ReadingCard() {
  return (
    <Card title="Reading this page" bodyClass="reading-body">
      <div>
        <div class="reading-title">Three stages, never one number</div>
        <p class="note">
          Every resolved query lands in exactly one of block, cache hit or
          forward. A latency figure without its stage is meaningless — a fast
          average is usually just a high cache-hit rate.
        </p>
      </div>
      <div>
        <div class="reading-title">Decimation is visible</div>
        <p class="note">
          One row per sample interval adds up, so a wide range is thinned.
          Only every n-th row is returned — whole rows, never averaged — and the
          chart says when that happens.
        </p>
      </div>
      <div>
        <div class="reading-title">The budget is a target, not a limit</div>
        <p class="note">
          Nothing enforces these at runtime. A line crossed is something to
          investigate against a pre-change build, not a failure the engine will
          act on.
        </p>
      </div>
    </Card>
  );
}
