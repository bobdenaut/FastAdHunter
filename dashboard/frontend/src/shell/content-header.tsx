import type { ComponentChildren } from 'preact';

/**
 * Page name plus a short line of context. `cluster` is the refresh cluster a
 * route reading exactly one polled endpoint puts here, beside the title — the
 * placement `Cache` draws.
 */
export function ContentHeader({
  title,
  context,
  cluster,
}: {
  title: string;
  context?: ComponentChildren;
  cluster?: ComponentChildren;
}) {
  return (
    <div class="hd">
      <div>
        <p class="h1">{title}</p>
        {context !== undefined && <p class="sub">{context}</p>}
      </div>
      {cluster}
    </div>
  );
}
