import type { Policy } from '../../api/types';
import { Card } from '../../components/card';
import { DEFAULT_POLICY } from '../../policy/assignment';

/**
 * `parse_qtype` maps everything but `A` and `AAAA` to `Other(name)`, so the
 * five the artboard draws are all valid and the chips are not an invented
 * enum.
 */
export const QUERY_TYPES = ['A', 'AAAA', 'HTTPS', 'PTR', 'TXT'] as const;

export type Mode = 'client' | 'policy';

export function QueryForm({
  domain,
  qtype,
  mode,
  subject,
  policies,
  busy,
  onDomain,
  onQtype,
  onMode,
  onSubject,
  onSubmit,
}: {
  domain: string;
  qtype: string;
  mode: Mode;
  subject: string;
  policies: readonly Policy[];
  busy: boolean;
  onDomain: (next: string) => void;
  onQtype: (next: string) => void;
  onMode: (next: Mode) => void;
  onSubject: (next: string) => void;
  onSubmit: () => void;
}) {
  return (
    <Card title="Query" className="tester-form">
      <form
        class="form tester-fields"
        onSubmit={(event) => {
          event.preventDefault();
          onSubmit();
        }}
      >
        <div>
          <label class="field-label tester-label" for="tester-domain">
            Domain
          </label>
          <input
            id="tester-domain"
            class="field-input mono"
            value={domain}
            placeholder="metrics.vendor.net"
            disabled={busy}
            autocomplete="off"
            spellcheck={false}
            onInput={(event) => onDomain(event.currentTarget.value)}
          />
        </div>

        <fieldset class="field-set field-set-block">
          <legend>Query type</legend>
          <div class="chips tester-chips">
            {QUERY_TYPES.map((type) => (
              <button
                key={type}
                type="button"
                class={type === qtype ? 'chip on' : 'chip'}
                aria-pressed={type === qtype}
                disabled={busy}
                onClick={() => onQtype(type)}
              >
                {type}
              </button>
            ))}
          </div>
        </fieldset>

        <div class="tester-decide">
          <fieldset class="field-set field-set-block">
            <legend>Decide as</legend>
            <div class="chips tester-chips">
              <button
                type="button"
                class={mode === 'client' ? 'chip on' : 'chip'}
                aria-pressed={mode === 'client'}
                disabled={busy}
                onClick={() => onMode('client')}
              >
                a client
              </button>
              <button
                type="button"
                class={mode === 'policy' ? 'chip on' : 'chip'}
                aria-pressed={mode === 'policy'}
                disabled={busy}
                onClick={() => onMode('policy')}
              >
                a policy
              </button>
            </div>
          </fieldset>

          {mode === 'client' ? (
            <>
              <input
                class="field-input mono"
                value={subject}
                placeholder="192.168.10.50"
                aria-label="Client address or name"
                disabled={busy}
                autocomplete="off"
                spellcheck={false}
                onInput={(event) => onSubject(event.currentTarget.value)}
              />
              <p class="note tester-note">
                An address, or a name this box has already seen. The engine
                selects a policy from the <i>address</i> only, so a name is
                resolved against the observed clients first and the address it
                resolved to is shown on the result. A name also satisfies any{' '}
                <span class="mono">$client</span> rule.
              </p>
            </>
          ) : (
            <>
              <select
                class="field-input"
                value={subject}
                aria-label="Policy"
                disabled={busy}
                onChange={(event) => onSubject(event.currentTarget.value)}
              >
                <option value={DEFAULT_POLICY}>default</option>
                {policies.map((policy) => (
                  <option key={policy.id} value={policy.id}>
                    {policy.id}
                  </option>
                ))}
              </select>
              <p class="note tester-note">
                Assignments are ignored and the named policy decides, so a
                policy can be checked before anything is assigned to it.
              </p>
            </>
          )}
        </div>

        <button
          type="submit"
          class="btn tester-submit"
          disabled={busy || domain.trim() === ''}
        >
          Test
        </button>
      </form>
    </Card>
  );
}
