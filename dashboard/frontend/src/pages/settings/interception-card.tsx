import { useCallback, useEffect, useRef, useState } from 'preact/hooks';
import { ApiError } from '../../api/core';
import {
  documentErrorDetails,
  getInterception,
  putInterception,
  type DocumentList,
} from '../../api/interception';
import type { InterceptionDocument } from '../../api/types';
import { Card } from '../../components/card';
import { ErrorState } from '../../components/error-state';
import { LineEditor, type EditorAnchor } from '../../components/line-editor';
import { blockNavigation } from '../../router/router';

const LISTS = ['clients', 'exclude_domains'] as const;

/**
 * The caps, hand-carried from `fah_rules::interception` exactly as every other
 * constraint on this page is hand-carried from CONFIGURATION.md and the schema.
 * `GET /interception` returns values, not bounds.
 */
const CAPS: Record<DocumentList, number> = {
  clients: 256,
  exclude_domains: 512,
};

const LABELS: Record<DocumentList, string> = {
  clients: 'Intercepted clients — one IP address or CIDR block per line',
  exclude_domains: 'Never intercepted hosts — one hostname per line',
};

/** What each list accepts, in the words `DocumentError::InvalidEntry` uses. */
const EXPECTED: Record<DocumentList, string> = {
  clients: 'an IP address or CIDR block',
  exclude_domains: 'a hostname',
};

type Buffers = Record<DocumentList, string>;
type Anchors = Record<DocumentList, readonly EditorAnchor[]>;

const NO_ANCHORS: Anchors = { clients: [], exclude_domains: [] };

/**
 * The Interception Document, edited as two lists and saved as one document.
 *
 * **Not a config card.** `clients` and `exclude_domains` are not config keys:
 * they live in `/config/interception.json`, are `422` on `POST /config`, and
 * are read and written only here through `GET`/`PUT /api/v1/interception`. So
 * this card fetches for itself, holds its own state, and takes no part in the
 * form's patch, its baseline, its `config_changed` re-read or its restart
 * banner — the shape `AccessCard` already established for a Settings card with
 * its own endpoint.
 *
 * **No restart banner, ever.** The response carries no `restart_required`
 * because there is nothing to restart: a saved change applies on the next
 * accepted connection, and an already-open session finishes under the lists it
 * was admitted with.
 */
export function InterceptionCard({ mode }: { mode: string | null }) {
  const [document, setDocument] = useState<InterceptionDocument | null>(null);
  const [buffers, setBuffers] = useState<Buffers>({
    clients: '',
    exclude_domains: '',
  });
  const [loadError, setLoadError] = useState<Error | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<Error | null>(null);
  const [anchors, setAnchors] = useState<Anchors>(NO_ANCHORS);
  /** A rejection no editor line can carry — an over-cap list, a shape the
   *  server would not read, a `503` or a `500`. */
  const [rejection, setRejection] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const clientsRef = useRef<HTMLTextAreaElement>(null);
  const excludeRef = useRef<HTMLTextAreaElement>(null);

  const adopt = useCallback((fresh: InterceptionDocument) => {
    setDocument(fresh);
    setBuffers(buffersOf(fresh));
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    getInterception(controller.signal)
      .then((fresh) => {
        adopt(fresh);
        setLoadError(null);
      })
      .catch((cause: unknown) => {
        if (cause instanceof Error && cause.name === 'AbortError') return;
        setLoadError(cause instanceof Error ? cause : new Error(String(cause)));
      });
    return () => controller.abort();
  }, [adopt]);

  const clearRejection = () => {
    setAnchors(NO_ANCHORS);
    setRejection(null);
    setSaveError(null);
    setSaved(false);
  };

  /**
   * A rejection describes the document **as it was sent**, and every index in
   * it addresses that text. The moment a buffer changes, an anchor can point at
   * different content — so editing drops the whole rejection and the next save
   * produces a fresh one, exactly as the rules editor does.
   */
  const edit = (list: DocumentList, next: string) => {
    setBuffers((current) => ({ ...current, [list]: next }));
    clearRejection();
  };

  const save = () => {
    if (saving) return;
    const sent: Record<DocumentList, readonly Entry[]> = {
      clients: entriesOf(buffers.clients),
      exclude_domains: entriesOf(buffers.exclude_domains),
    };
    setSaving(true);
    clearRejection();
    const unblock = blockNavigation();
    putInterception({
      clients: sent.clients.map((entry) => entry.text),
      exclude_domains: sent.exclude_domains.map((entry) => entry.text),
    })
      .then((stored) => {
        adopt(stored);
        setSaved(true);
      })
      .catch((cause: unknown) => {
        // Every non-2xx leaves both buffers byte-identical. A rejected `PUT`
        // changed nothing on the server, so there is nothing to rebase onto.
        const outcome = rejectionOf(cause, sent);
        if (outcome.anchors !== null) setAnchors(outcome.anchors);
        if (outcome.message !== null) setRejection(outcome.message);
        if (outcome.error !== null) setSaveError(outcome.error);
      })
      .finally(() => {
        unblock();
        setSaving(false);
      });
  };

  const reset = () => {
    if (document === null) return;
    setBuffers(buffersOf(document));
    clearRejection();
  };

  const pristine =
    document !== null &&
    buffers.clients === document.clients.join('\n') &&
    buffers.exclude_domains === document.exclude_domains.join('\n');
  const anchored =
    anchors.clients.length > 0 || anchors.exclude_domains.length > 0;

  /** A mode with no HTTPS listener stores and validates the document all the
   *  same; nothing reads it until such a mode boots. Saying so is the whole of
   *  it — the card does not disable itself, because listing the first client
   *  before turning interception on is a legitimate order to work in. */
  const inert = mode !== null && !mode.includes('https');

  return (
    <Card title="Interception" className="set-section set-section-interception">
      <p class="note set-section-note">
        Who is HTTPS-intercepted, and what always splices. Not a config key:
        this is <span class="mono">/config/interception.json</span>, read and
        replaced whole through{' '}
        <span class="mono">GET | PUT /api/v1/interception</span>. A save applies
        on the next accepted connection — there is nothing to restart, and an
        already-open session finishes under the lists it was admitted with.
      </p>

      {loadError !== null ? (
        <ErrorState error={loadError} />
      ) : (
        <>
          {LISTS.map((list) => (
            <div key={list} class="set-field">
              <div class="set-field-name">
                <span class="set-field-label mono">{list}</span>
                <p class="note">
                  {LABELS[list]}
                  <span class="footnote-line mono">
                    {entriesOf(buffers[list]).length} / {CAPS[list]}
                  </span>
                </p>
              </div>
              <div class="set-field-control">
                <LineEditor
                  value={buffers[list]}
                  onInput={(next) => edit(list, next)}
                  anchors={anchors[list]}
                  disabled={saving || document === null}
                  textareaRef={list === 'clients' ? clientsRef : excludeRef}
                  label={LABELS[list]}
                />
              </div>
            </div>
          ))}

          {/* Bands alone would leave the operator to infer whether anything
              was written. The whole document is validated and swapped as one
              unit, so one bad entry means none of it was stored — and that is
              the sentence, not something read off the editors. */}
          {(rejection !== null || anchored) && (
            <p class="note" role="alert">
              {rejection ??
                'Not saved — the API rejected an entry, marked below.'}{' '}
              The stored document is unchanged, and your text is exactly as you
              left it.
            </p>
          )}

          {saveError !== null && <ErrorState error={saveError} />}

          <div class="set-bar">
            <div class="note">
              {inert ? (
                <>
                  <span class="mono">engine.mode</span> is{' '}
                  <span class="mono">{mode}</span>, which runs no HTTPS
                  listener. The document is stored and validated all the same,
                  and takes effect at the first boot of a mode that has one.
                </>
              ) : saved ? (
                <span role="status">Applied on the next connection.</span>
              ) : pristine ? (
                'No unsaved changes.'
              ) : (
                'Unsaved changes — the whole document is sent.'
              )}
            </div>
            <div class="page-buttons">
              <button
                type="button"
                class="btn g"
                disabled={saving || pristine || document === null}
                onClick={reset}
              >
                Reset
              </button>
              <button
                type="button"
                class="btn"
                disabled={saving || pristine || document === null}
                onClick={save}
              >
                {saving ? 'Saving…' : 'Save document'}
              </button>
            </div>
          </div>
        </>
      )}
    </Card>
  );
}

function buffersOf(document: InterceptionDocument): Buffers {
  return {
    clients: document.clients.join('\n'),
    exclude_domains: document.exclude_domains.join('\n'),
  };
}

/** One entry as sent, and the editor line it came from. */
export interface Entry {
  text: string;
  line: number;
}

/**
 * The entries a buffer sends, each carrying its one-based line.
 *
 * Blank lines are dropped — an empty line is not an entry, and sending one
 * would earn an `invalid_entry` for something nobody typed. Keeping the line
 * is what turns the server's index, which counts entries, back into a place in
 * the text, which counts lines.
 */
export function entriesOf(buffer: string): Entry[] {
  const found: Entry[] = [];
  buffer.split('\n').forEach((text, index) => {
    if (text.trim() !== '') found.push({ text, line: index + 1 });
  });
  return found;
}

/** One list's anchors, with the other list's cleared — a rejection names one
 *  list and one entry, so a stale band on the other editor would be a second
 *  problem the server never reported. */
function anchorsFor(list: DocumentList, found: readonly EditorAnchor[]): Anchors {
  return list === 'clients'
    ? { clients: found, exclude_domains: [] }
    : { clients: [], exclude_domains: found };
}

function anchor(
  sent: Record<DocumentList, readonly Entry[]>,
  list: DocumentList,
  index: number,
  message: string,
): EditorAnchor[] {
  const line = sent[list][index]?.line;
  return line === undefined ? [] : [{ line, message }];
}

/**
 * How a failed save is shown: anchored on the offending lines, stated as one
 * card-level line, or handed to `ErrorState` when it never reached the server.
 *
 * Only `details.reason` is branched on. `message` is the API's own words and is
 * rendered verbatim where no better sentence exists — it is never parsed, which
 * is exactly what `details` exists to make unnecessary.
 */
function rejectionOf(
  cause: unknown,
  sent: Record<DocumentList, readonly Entry[]>,
): { anchors: Anchors | null; message: string | null; error: Error | null } {
  const details = documentErrorDetails(cause);
  if (details === null) {
    if (cause instanceof ApiError) {
      // A `500` is the one answer that says nothing about the document: the
      // write failed, and the contract's own words are that nothing was left
      // half-applied.
      return {
        anchors: null,
        message:
          cause.status >= 500
            ? `${cause.message} Nothing was applied.`
            : cause.message,
        error: null,
      };
    }
    return {
      anchors: null,
      message: null,
      error: cause instanceof Error ? cause : new Error(String(cause)),
    };
  }

  switch (details.reason) {
    case 'over_cap':
      return {
        anchors: null,
        message: `Not saved — ${details.list} holds ${String(details.len)} entries and the cap is ${String(details.cap)}.`,
        error: null,
      };
    case 'shape':
      return {
        anchors: null,
        message:
          cause instanceof ApiError
            ? cause.message
            : 'Not saved — the document was not accepted as sent.',
        error: null,
      };
    case 'invalid_entry':
      return {
        anchors: anchorsFor(
          details.list,
          anchor(
            sent,
            details.list,
            details.index,
            `${details.entry} is not ${EXPECTED[details.list]}`,
          ),
        ),
        message: null,
        error: null,
      };
    case 'duplicate':
      return {
        anchors: anchorsFor(details.list, [
          ...anchor(
            sent,
            details.list,
            details.index,
            `${details.entry} duplicates an earlier entry`,
          ),
          ...anchor(
            sent,
            details.list,
            details.duplicate_of,
            'the earlier entry it duplicates',
          ),
        ]),
        message: null,
        error: null,
      };
  }
}
