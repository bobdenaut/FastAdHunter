import { useCallback, useEffect, useRef, useState } from 'preact/hooks';
import { ApiError } from '../api/core';
import { getUserRules, putUserRules } from '../api/rules';
import { BusyModal } from '../components/busy-modal';
import { Card } from '../components/card';
import { ConfirmDialog } from '../components/confirm-dialog';
import { ErrorState } from '../components/error-state';
import {
  LineEditor,
  lineRange,
  type EditorAnchor,
} from '../components/line-editor';
import {
  invalidLineCount,
  parseUserRulesError,
  type UserRulesError,
} from '../policy/validation';
import { blockNavigation } from '../router/router';
import type { PageProps } from '../router/routes';
import { ContentHeader } from '../shell/content-header';
import { Icon } from '../shell/icon';

import { ErrorList } from './rules/error-list';
import { HowThisSaves, Precedence } from './rules/how-this-saves';

/**
 * The user-rules document, as a document. `GET`/`PUT /api/v1/rules/user` takes
 * lines in and gives lines out and there is no per-rule identity behind it, so
 * there is no per-rule delete and no per-rule toggle — offering one would
 * promise a write the API cannot perform.
 *
 * **Route-scoped and timerless.** One entry read, one user-triggered write, no
 * event subscription and no interval; the connection indicator reads `not
 * needed here` for as long as this page is mounted.
 *
 * **The save blocks, and is never aborted.** `set_user_rules` runs a full
 * `compile()` and `swap_in` inline, so the request is held open for the whole
 * rebuild — seconds, the figure the Lists header prints as `last compile`. A
 * cancel mid-compile can land between persist and swap, so the modal blocks
 * navigation instead of offering one, and nothing is shown as saved until the
 * response lands.
 */
export function Rules(_props: PageProps) {
  const [document, setDocument] = useState<readonly string[] | null>(null);
  const [buffer, setBuffer] = useState('');
  const [loadError, setLoadError] = useState<Error | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<Error | null>(null);
  const [rejected, setRejected] = useState<UserRulesError | null>(null);
  const [duplicates, setDuplicates] = useState<number | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const editor = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const controller = new AbortController();
    getUserRules(controller.signal)
      .then((response) => {
        setDocument(response.rules);
        setBuffer(response.rules.join('\n'));
        setLoadError(null);
      })
      .catch((cause: unknown) => {
        if (cause instanceof DOMException && cause.name === 'AbortError') return;
        setLoadError(cause instanceof Error ? cause : new Error(String(cause)));
      });
    return () => controller.abort();
  }, []);

  const selectLine = useCallback(
    (line: number) => {
      const area = editor.current;
      if (area === null) return;
      const { start, end } = lineRange(area.value, line);
      area.focus();
      area.setSelectionRange(start, end);
    },
    [],
  );

  /**
   * No `AbortSignal` anywhere on this path, deliberately. The busy modal and
   * the router block is what keeps the page mounted; the request runs to its
   * response either way.
   */
  const save = useCallback(() => {
    if (saving) return;
    const sent = buffer.split('\n');
    setSaving(true);
    setSaveError(null);
    setRejected(null);
    setDuplicates(null);
    const unblock = blockNavigation();
    putUserRules(sent)
      .then((response) => {
        // C12 — the API drops exact-duplicate rule lines, so the response can
        // be shorter than the request. The editor takes the returned document
        // rather than keeping what was typed, and the page says how many went.
        setDocument(response.rules);
        setBuffer(response.rules.join('\n'));
        setDuplicates(sent.length - response.rules.length);
      })
      .catch((cause: unknown) => {
        // Every non-2xx path leaves the buffer byte-identical. It is only ever
        // written from a 2xx response.
        if (cause instanceof ApiError && cause.status === 422) {
          setRejected(parseUserRulesError(cause.message));
          return;
        }
        setSaveError(cause instanceof Error ? cause : new Error(String(cause)));
      })
      .finally(() => {
        unblock();
        setSaving(false);
      });
  }, [buffer, saving]);

  const pristine = document !== null && buffer === document.join('\n');
  // The live buffer's line count — T1 restated: an empty buffer is zero lines,
  // not the one empty line `split` reports, so a fresh install reads `0 lines`
  // exactly as `GET /rules/user`'s empty `rules` does.
  const lines = buffer === '' ? 0 : buffer.split('\n').length;
  const invalid = rejected === null ? 0 : invalidLineCount(rejected);
  const anchors: readonly EditorAnchor[] =
    rejected === null
      ? []
      : rejected.lines.map((entry) => ({
          line: entry.line,
          message: entry.detail,
        }));

  return (
    <>
      <ContentHeader
        title="Custom rules"
        context={
          <>
            Your own rules, on top of every enabled list —{' '}
            <span class="mono">GET | PUT /api/v1/rules/user</span>
          </>
        }
        actions={
          <>
            <button
              type="button"
              class="btn g"
              disabled={saving || pristine || document === null}
              onClick={() => setConfirmDiscard(true)}
            >
              Discard
            </button>
            <button
              type="button"
              class="btn"
              disabled={saving || document === null}
              onClick={save}
            >
              Validate and save
            </button>
          </>
        }
      />

      <main class="wrap">
        {rejected !== null && (
          <div class="banner bad" role="alert">
            <Icon name="warning" size={16} className="warning" />
            <div>
              <b>
                Not saved —{' '}
                {invalid === 0
                  ? 'the document was rejected'
                  : invalid === 1
                    ? 'one line failed validation'
                    : `${String(invalid)} lines failed validation`}
                .
              </b>{' '}
              The whole document is validated and swapped as one unit, so
              nothing was written and the running ruleset is unchanged. Your
              text is exactly as you left it
              {rejected.lines.length === 1 && rejected.lines[0] !== undefined
                ? `; fix line ${String(rejected.lines[0].line)} and save again.`
                : rejected.lines.length > 1
                  ? '; fix the lines listed below and save again.'
                  : '.'}
              {rejected.lines.length === 0 && (
                <span class="stacked mono">{rejected.raw}</span>
              )}
            </div>
          </div>
        )}

        {duplicates !== null && duplicates > 0 && (
          <div class="banner note" role="status">
            <Icon name="warning" size={16} />
            <div>
              Saved. {duplicates} duplicate line{duplicates === 1 ? '' : 's'}{' '}
              removed — the API keeps the first of any exact-duplicate rule, and
              the document below is what it stored.
            </div>
          </div>
        )}

        {saveError !== null && <ErrorState error={saveError} />}

        <div class="rules-grid">
          <Card
            title="rules.txt"
            secondary={
              <>
                {lines} line{lines === 1 ? '' : 's'}
                {invalid > 0 && ` · ${String(invalid)} invalid`}
              </>
            }
            bodyClass="rules-body"
          >
            {loadError !== null ? (
              <ErrorState error={loadError} />
            ) : (
              <>
                <LineEditor
                  value={buffer}
                  onInput={setBuffer}
                  anchors={anchors}
                  disabled={saving || document === null}
                  textareaRef={editor}
                  label="Custom rules document"
                />
                {rejected !== null && (
                  <ErrorList parsed={rejected} onSelectLine={selectLine} />
                )}
              </>
            )}
          </Card>

          <div class="rules-side">
            <HowThisSaves />
            <Precedence />
          </div>
        </div>
      </main>

      {saving && (
        <BusyModal title="Rebuilding the ruleset">
          The document is being validated and, once accepted, the whole ruleset
          is recompiled and swapped in — seconds of CPU, and more on the
          router. Nothing is saved until this finishes, and it cannot be
          cancelled: stopping it part-way could leave the stored document ahead
          of the running matcher.
        </BusyModal>
      )}

      {confirmDiscard && (
        <ConfirmDialog
          title="Discard your changes?"
          confirmLabel="Discard"
          onCancel={() => setConfirmDiscard(false)}
          onConfirm={() => {
            setBuffer(document?.join('\n') ?? '');
            setRejected(null);
            setSaveError(null);
            setDuplicates(null);
            setConfirmDiscard(false);
          }}
        >
          The editor goes back to the document the API last returned. Anything
          typed since is lost, and nothing on the server changes either way.
        </ConfirmDialog>
      )}
    </>
  );
}

export default Rules;
