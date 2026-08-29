import type { UserRulesError } from '../../policy/validation';

/**
 * The anchored list under the editor. It exists whichever callout treatment
 * ships: a floating callout is the artboard's placement, but at 390 px there
 * is nowhere for one to float, and a message that cannot be reached with a tap
 * is not an anchor. Each entry focuses the textarea and selects that line's
 * range, which is what "anchors to its line" functionally requires.
 *
 * The entries carry the API's own words. The `422` envelope has no diagnosis
 * in it, so none is written here.
 */
export function ErrorList({
  parsed,
  onSelectLine,
}: {
  parsed: UserRulesError;
  onSelectLine: (line: number) => void;
}) {
  if (parsed.lines.length === 0 && parsed.more === 0) return null;
  return (
    <div class="rule-errors">
      {parsed.lines.map((entry, index) => (
        <button
          key={`${String(entry.line)}-${String(index)}`}
          type="button"
          class="rule-error"
          onClick={() => onSelectLine(entry.line)}
        >
          <span class="mono rule-error-line">line {entry.line}</span>
          <span class="rule-error-detail">{entry.detail}</span>
        </button>
      ))}
      {parsed.more > 0 && (
        <p class="note rule-error-more">
          and {parsed.more} more invalid line
          {parsed.more === 1 ? '' : 's'} the API did not list — it reports at
          most 100.
        </p>
      )}
    </div>
  );
}
