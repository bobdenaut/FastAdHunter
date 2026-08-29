import { useCallback, useEffect, useRef, useState } from 'preact/hooks';
import { getConfig, postConfig } from '../api/config';
import { getHealth } from '../api/health';
import type { Config, ConfigUpdateResponse } from '../api/types';
import { ConfirmDialog } from '../components/confirm-dialog';
import { ErrorState } from '../components/error-state';
import { ApiError } from '../api/core';
import { nowMs } from '../lifecycle/timers';
import type { PageProps } from '../router/routes';
import { socket } from '../services';
import { ContentHeader } from '../shell/content-header';
import {
  armRestartBanner,
  clearIfRestarted,
  restartArming,
} from '../system/restart-banner';
import { AccessCard } from './settings/access-card';
import { RawPanel } from './settings/raw-panel';
import { SECTIONS, fieldMeta, gatedFields } from './settings/metadata';
import { SectionCard } from './settings/section-card';
import {
  anchorError,
  buildPatch,
  dirtyKeys,
  rebase,
  setEdit,
  validateEdits,
  type Edits,
  type FieldValue,
} from './settings/patch';

/** One `GET /config`: the promise, and the instant it left. The stamp is what
 *  orders two reads that are in flight together. */
interface Read {
  run: Promise<Config>;
  startedAt: number;
}

/**
 * The effective configuration, edited one section at a time.
 *
 * **Its data lifecycle, in one place.** One `GET /config` on entry and one more
 * per `config_changed` or successful save, both through a single-flight reader
 * so an event landing beside a save costs one request rather than two. One
 * `GET /health` on entry, and **only while the restart banner is armed** — the
 * banner's revalidation, not a status read. It declares `config_changed` and
 * no polled endpoint, so `activeTimers()` is zero here and the socket carries
 * one event type.
 *
 * **Nothing on this page reads a constraint from the API.** Bounds, enums and
 * mutability classes all come from `settings/metadata.ts`, hand-carried from
 * CONFIGURATION.md and the `fah-config` schema. `GET /config` is values.
 */
export function Settings(_props: PageProps) {
  const [baseline, setBaseline] = useState<Config | null>(null);
  const [edits, setEdits] = useState<Edits>({});
  const [errors, setErrors] = useState<ReadonlyMap<string, string>>(new Map());
  const [formError, setFormError] = useState<Error | string | null>(null);
  const [outcome, setOutcome] = useState<ConfigUpdateResponse | null>(null);
  const [gate, setGate] = useState<readonly string[] | null>(null);
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState<Error | null>(null);
  const inFlight = useRef<Read | null>(null);
  /** The stamp of the newest document already adopted, so a read that settles
   *  out of order cannot walk the baseline backwards. */
  const adoptedAt = useRef(0);

  /**
   * One reader for the entry read, the event re-read and the post-save
   * re-read. The server publishes `config_changed` on its own successful
   * `POST`, so those last two race by construction — p5-06's F11, applied to
   * the page that actually provokes the race.
   */
  const readConfig = useCallback(
    (signal?: AbortSignal, startedAfter?: number): Read => {
      const pending = inFlight.current;
      // **A read provoked by a change may only join one that began after it.**
      // The join was unconditional, so a read triggered by `config_changed` —
      // or by this browser's own save — could adopt a request that had already
      // left before the write, making the pre-change document the baseline with
      // nothing left to re-read it. `startedAfter` is the instant the change
      // became known; an older request is not an answer to it.
      if (
        pending !== null &&
        (startedAfter === undefined || pending.startedAt >= startedAfter)
      ) {
        return pending;
      }
      const run = getConfig(signal);
      const slot = { run, startedAt: nowMs() };
      inFlight.current = slot;
      const clear = () => {
        if (inFlight.current === slot) inFlight.current = null;
      };
      run.then(clear, clear);
      return slot;
    },
    [],
  );

  /**
   * A fresh document replaces the baseline and keeps the operator's unsaved
   * edits — except any the server has since made itself (KTD2).
   *
   * **`startedAt` is the read's own instant, and an older one is dropped.**
   * Making the join conditional allows two `GET /config` to be in flight at
   * once, and two requests can settle in either order: without this the one
   * that *resolved* last won the baseline even when it *left* first, which
   * installs a document one change behind with nothing left to re-read it —
   * the same failure the conditional join was closing, arriving from the other
   * side. Equal stamps still adopt: they are two answers to the same instant.
   */
  const adopt = useCallback((fresh: Config, startedAt: number) => {
    if (startedAt < adoptedAt.current) return;
    adoptedAt.current = startedAt;
    setBaseline(fresh);
    setEdits((current) => rebase(fresh, current));
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    const read = readConfig(controller.signal);
    read.run
      .then((fresh) => adopt(fresh, read.startedAt))
      .catch((cause: unknown) => {
        if (cause instanceof Error && cause.name === 'AbortError') return;
        setLoadError(cause instanceof Error ? cause : new Error(String(cause)));
      });
    return () => controller.abort();
  }, [readConfig, adopt]);

  // The banner's revalidation, and the only reason this page reads `/health`.
  // It runs once on entry and only while something is actually pending: a read
  // on every entry would be a status poll wearing a banner's clothes.
  useEffect(() => {
    if (restartArming() === null) return;
    const controller = new AbortController();
    getHealth(controller.signal)
      .then((health) => clearIfRestarted(health, nowMs()))
      .catch(() => undefined);
    return () => controller.abort();
  }, []);

  /**
   * The event is a nudge: it carries `restart_required` and nothing else, so a
   * change made from another browser re-reads the document and arms the banner
   * without naming keys this browser never saw.
   */
  useEffect(
    () =>
      socket.on('config_changed', (data) => {
        const announcedAt = nowMs();
        const read = readConfig(undefined, announcedAt);
        read.run
          .then((fresh) => adopt(fresh, read.startedAt))
          .catch(() => undefined);
        if (data['restart_required'] === true) armRestartBanner([], announcedAt);
      }),
    [readConfig, adopt],
  );

  const change = (key: string, value: FieldValue) => {
    setEdits((current) => setEdit(baseline, current, key, value));
    // The message described the value that is being replaced; Save revalidates
    // and produces a fresh one. Only this key's — a second field's rejection
    // still stands.
    setErrors((current) => {
      if (!current.has(key)) return current;
      const next = new Map(current);
      next.delete(key);
      return next;
    });
    setOutcome(null);
  };

  const dirty = dirtyKeys(edits);

  const send = () => {
    setGate(null);
    setSaving(true);
    setFormError(null);
    setOutcome(null);
    const bootKeys = dirty.filter(
      (key) => fieldMeta(key)?.mutability === 'restart',
    );
    postConfig(buildPatch(edits))
      .then((response) => {
        const appliedAt = nowMs();
        setOutcome(response);
        setEdits({});
        if (response.restart_required) armRestartBanner(bootKeys, appliedAt);
        // The server publishes `config_changed` for this write too, and the
        // single-flight reader still collapses that pair into one request —
        // but only onto a read that began after the write landed. An entry read
        // still in flight from before the `POST` is not an answer to it.
        const read = readConfig(undefined, appliedAt);
        return read.run.then((fresh) => adopt(fresh, read.startedAt));
      })
      .catch((cause: unknown) => {
        if (cause instanceof ApiError && cause.status === 422) {
          const anchored = anchorError(cause.message);
          if (anchored.key !== null) {
            setErrors(new Map([[anchored.key, anchored.message]]));
            return;
          }
        }
        setFormError(cause instanceof Error ? cause : new Error(String(cause)));
      })
      .finally(() => setSaving(false));
  };

  const save = () => {
    const found = validateEdits(baseline, edits);
    setErrors(new Map(found.map((error) => [error.key, error.message])));
    // An out-of-range value costs no round trip: the bounds are known here.
    if (found.length > 0) return;
    const gated = gatedFields(dirty);
    if (gated.length > 0) {
      setGate(gated.map((field) => field.key));
      return;
    }
    send();
  };

  const discard = () => {
    setEdits({});
    setErrors(new Map());
    setFormError(null);
    setOutcome(null);
  };

  return (
    <>
      <ContentHeader
        title="Settings"
        context={
          <>
            The effective configuration, all sources merged, secrets redacted —{' '}
            <span class="mono">GET /api/v1/config</span>. Bounds, enums and the
            live/restart tags are carried from CONFIGURATION.md and the schema,
            never from the response.
          </>
        }
      />
      <main class="wrap">
        {loadError !== null && <ErrorState error={loadError} />}

        <div class="set-layout">
          <nav class="set-nav" aria-label="Configuration sections">
            {SECTIONS.map((section) => (
              <a key={section.id} href={`#set-section-${section.id}`}>
                <span class="mono">{section.id}</span>
              </a>
            ))}
          </nav>

          <div class="set-sections">
            {SECTIONS.map((section) => (
              <div key={section.id} id={`set-section-${section.id}`}>
                <SectionCard
                  section={section}
                  baseline={baseline}
                  edits={edits}
                  errors={errors}
                  onChange={change}
                />
              </div>
            ))}

            <AccessCard />

            <div class="set-bar">
              <div class="note">
                {dirty.length === 0 ? (
                  'No unsaved changes.'
                ) : (
                  <>
                    {dirty.length} unsaved{' '}
                    {dirty.length === 1 ? 'change' : 'changes'} —{' '}
                    {dirty.map((key, index) => (
                      <span key={key}>
                        {index > 0 && ', '}
                        <span class="mono">{key}</span>
                      </span>
                    ))}
                    . Only these keys are sent.
                  </>
                )}
                {outcome !== null && (
                  <span class="footnote-line" role="status">
                    {outcome.applied && 'Applied live.'}
                    {outcome.applied && outcome.restart_required && ' '}
                    {outcome.restart_required &&
                      'Saved to the configuration file — it needs a restart.'}
                    {!outcome.applied &&
                      !outcome.restart_required &&
                      'Nothing changed.'}
                  </span>
                )}
              </div>
              <div class="page-buttons">
                <button
                  type="button"
                  class="btn g"
                  disabled={dirty.length === 0 || saving}
                  onClick={discard}
                >
                  Discard
                </button>
                <button
                  type="button"
                  class="btn"
                  disabled={dirty.length === 0 || saving}
                  onClick={save}
                >
                  {saving ? 'Saving…' : 'Save changes'}
                </button>
              </div>
            </div>

            {formError !== null && <ErrorState error={formError} />}

            <RawPanel config={baseline} />
          </div>
        </div>

        {gate !== null && (
          <ConfirmDialog
            title="This changes how the dashboard is reached"
            confirmLabel="Save anyway"
            onCancel={() => setGate(null)}
            onConfirm={send}
          >
            <>
              {gatedFields(gate).map((field) => (
                <p key={field.key} class="note">
                  <b class="mono">{field.key}</b> — {field.consequence}
                </p>
              ))}
              <p class="note">
                Nothing is sent until you confirm. All of it takes effect on the
                next restart, not now.
              </p>
            </>
          </ConfirmDialog>
        )}
      </main>
    </>
  );
}

export default Settings;
