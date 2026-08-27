import type { ComponentChildren } from 'preact';

/**
 * Page name plus a short line of context. `cluster` is the refresh cluster a
 * route reading exactly one polled endpoint puts here, beside the title — the
 * placement `Cache` draws.
 *
 * `actions` is the page's own header buttons — `Discard` / `Validate and save`,
 * `New policy`. Three of `p5-07`'s four artboards draw them there, and the
 * shell renders a title alone, which is why Lists' pair ended up in the page
 * body. A route declaring `ownsHeader` renders this component itself and fills
 * both slots; the shell then renders no header of its own.
 */
export function ContentHeader({
  title,
  context,
  cluster,
  actions,
}: {
  title: string;
  context?: ComponentChildren;
  cluster?: ComponentChildren;
  actions?: ComponentChildren;
}) {
  return (
    <div class="hd">
      <div>
        <p class="h1">{title}</p>
        {context !== undefined && <p class="sub">{context}</p>}
      </div>
      {cluster}
      {actions !== undefined && <div class="hd-actions">{actions}</div>}
    </div>
  );
}
