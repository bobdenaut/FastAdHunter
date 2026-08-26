/**
 * `pass` neutral · `allow` green · `block` red — always with its word, never
 * colour alone.
 *
 * `allow` and `permitted` are different words for different things: `allow` is
 * the explicit allow-verdict counter the API exposes, an exception match.
 * `permitted` is the derived `queries − blocked` band, which the API does not
 * carry — it is never a verdict and never appears here.
 */
export type Verdict = 'pass' | 'allow' | 'block';

const TONE: Record<Verdict, string> = {
  pass: 'neutral',
  allow: 'good',
  block: 'bad',
};

export function VerdictPill({ verdict }: { verdict: Verdict }) {
  return <span class={`pill ${TONE[verdict]}`}>{verdict}</span>;
}
