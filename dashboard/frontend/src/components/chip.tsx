/**
 * One choice in a chip set — a toggle drawn as a pill, not a checkbox.
 *
 * `aria-pressed` rather than a radio group: a chip set is a set of independent
 * on/off filters as far as assistive technology is concerned, and the pages
 * that use it enforce single-selection themselves by what they pass to `on`.
 */
export function Chip({
  label,
  on,
  onPick,
}: {
  label: string;
  on: boolean;
  onPick: () => void;
}) {
  return (
    <button
      type="button"
      class={on ? 'chip on' : 'chip'}
      aria-pressed={on}
      onClick={onPick}
    >
      {label}
    </button>
  );
}
