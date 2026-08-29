import type { ComponentChildren } from 'preact';

/**
 * An empty result is not an error. A range that returned no buckets reads "no
 * data in this range", not as a failure.
 */
export function EmptyState({
  title = 'No data in this range',
  children,
}: {
  title?: string;
  children?: ComponentChildren;
}) {
  return (
    <div class="empty-state">
      <p class="empty-state-title">{title}</p>
      {children !== undefined && <p class="note">{children}</p>}
    </div>
  );
}
