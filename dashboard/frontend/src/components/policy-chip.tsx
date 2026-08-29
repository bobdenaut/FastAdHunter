import type { Classification } from '../policy/assignment';

/**
 * The policy in force, and how the client got it. Solid means an assignment
 * names *this address*; dashed means inherited — from a subnet, a name, or the
 * default policy.
 *
 * **The distinction is carried three ways, none of them hue**: border style,
 * font weight, and the note in words beside it. `visual-system.md`
 * §Accessibility requires colour never carry meaning alone, and `p5-06`'s
 * F17/N4 showed that a `color-mix(…, transparent)` tint premultiplies to about
 * 1.8 % alpha and vanishes in one of the two themes — so the dashed chip's
 * surface is a token, never a wash.
 *
 * Its style is `assignment_source`'s, taken verbatim: that is the one signal
 * Rust computes itself, and the classification only supplies the words.
 */
export function PolicyChip({
  classification,
  showNote = true,
}: {
  classification: Classification;
  /** The Rule Tester prints the reason in its own row, so the chip goes bare. */
  showNote?: boolean;
}) {
  const { style, policy, note, tone } = classification;
  return (
    <span class="pol">
      <span class={style === 'solid' ? 'pchip' : 'pchip inh'}>{policy}</span>
      {showNote && note !== '' && (
        <span class={tone === 'warn' ? 'note pol-note warn' : 'note pol-note'}>
          {note}
        </span>
      )}
    </span>
  );
}
