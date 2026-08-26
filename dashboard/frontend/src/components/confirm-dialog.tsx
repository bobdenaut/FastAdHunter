import type { ComponentChildren } from 'preact';
import { useRef } from 'preact/hooks';
import { useFocusTrap } from './focus-trap';

/**
 * Focus-trapped while open, `Escape` closes, and focus returns to whatever
 * invoked it — all of it from the shared trap, which the phone drawer uses too.
 */
export function ConfirmDialog({
  title,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  onConfirm,
  onCancel,
  children,
}: {
  title: string;
  confirmLabel?: string;
  cancelLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
  children?: ComponentChildren;
}) {
  const dialog = useRef<HTMLDivElement>(null);

  useFocusTrap(true, () => dialog.current, onCancel);

  return (
    <div class="dialog-scrim">
      <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        ref={dialog}
      >
        <h2>{title}</h2>
        {children !== undefined && <div class="note">{children}</div>}
        <div class="dialog-actions">
          <button type="button" class="btn g" onClick={onCancel}>
            {cancelLabel}
          </button>
          <button type="button" class="btn" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
