/**
 * A large mono figure over its label — the Ruleset card's four, and the same
 * four at the top of Lists. It lives here rather than on one of those pages
 * because a page importing a component out of another page's directory couples
 * two routes and gives the bundler a shared chunk to emit.
 */
export function Figure({ value, label }: { value: string; label: string }) {
  return (
    <div>
      <div class="figure mono">{value}</div>
      <div class="note">{label}</div>
    </div>
  );
}
