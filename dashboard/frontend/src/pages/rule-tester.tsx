import { useCallback, useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { getClients } from '../api/clients';
import { getPolicies } from '../api/policies';
import { testRule } from '../api/rules';
import type { Client, Policy, RuleTestBody } from '../api/types';
import { Card } from '../components/card';
import { ErrorState } from '../components/error-state';
import {
  CHOSEN_POLICY_REASON,
  classifyAssignment,
  DEFAULT_POLICY,
  whyPolicyApplies,
} from '../policy/assignment';
import { equalsAsciiCaseInsensitive, parseIp } from '../policy/selectors';
import type { PageProps } from '../router/routes';
import { ContentHeader } from '../shell/content-header';

import { QueryForm, type Mode } from './rule-tester/query-form';
import { ResultCard, type TestRecord } from './rule-tester/result-card';
import { pushRecord, SessionRing } from './rule-tester/session-ring';

/**
 * The verdict dry-run: what the running engine would decide, without waiting
 * for the client to ask.
 *
 * **A name is resolved to an address, visibly.** `test_rule` selects a policy
 * only on the address branch — a name yields a bare `ClientContext` whose
 * policy stays `default`, however that client is assigned. So a typed name is
 * matched against the observed clients first and the address it resolved to is
 * printed on the result. Where it cannot resolve, the answer is marked partial
 * rather than presented as a working name test.
 *
 * **Two matches block.** `set_name` writes the field with no uniqueness check,
 * so two addresses can carry one name — and the fold is ASCII-case-insensitive,
 * so `Tv` and `tv` are two matches too. There is no correct single answer, so
 * the form asks instead of picking, and sends no request until it is told.
 *
 * The four result fields are the API's. The fifth row — why that policy
 * applies — is assembled locally from the same two responses the Clients page
 * cross-references, and nothing else on this page is derived: no domain
 * matching, no rule evaluation, no list attribution happens in the browser.
 */
/**
 * Client mode with the client field left blank: the engine answers from a bare
 * context, so `default` decides. Its own sentence, because the policy-mode one
 * ("you chose this policy") would claim a choice nobody made.
 */
const NO_CLIENT_REASON = 'no client given — the default policy decides';

export function RuleTester(_props: PageProps) {
  const [clients, setClients] = useState<readonly Client[]>([]);
  const [policies, setPolicies] = useState<readonly Policy[]>([]);
  const [loadError, setLoadError] = useState<Error | null>(null);
  const [testError, setTestError] = useState<Error | null>(null);
  const [domain, setDomain] = useState('');
  const [qtype, setQtype] = useState<string>('A');
  const [mode, setMode] = useState<Mode>('client');
  const [subject, setSubject] = useState('');
  const [policyChoice, setPolicyChoice] = useState(DEFAULT_POLICY);
  const [ring, setRing] = useState<readonly TestRecord[]>([]);
  const [busy, setBusy] = useState(false);
  const [ambiguous, setAmbiguous] = useState<readonly Client[] | null>(null);
  const controller = useRef<AbortController | null>(null);
  const testController = useRef<AbortController | null>(null);

  useEffect(() => {
    const boot = new AbortController();
    controller.current = boot;
    Promise.all([getPolicies(boot.signal), getClients(boot.signal)])
      .then(([policyList, clientList]) => {
        setPolicies(policyList.items);
        setClients(clientList.items);
        setLoadError(null);
      })
      .catch((cause: unknown) => {
        if (cause instanceof DOMException && cause.name === 'AbortError') return;
        setLoadError(cause instanceof Error ? cause : new Error(String(cause)));
      });
    // The POST does not recompile and does not block (§5.4), so an unmount
    // aborts it like any other in-flight read.
    return () => {
      boot.abort();
      testController.current?.abort();
    };
  }, []);

  /**
   * T13. The engine's own `policy` is what is classified against, not the
   * `/clients` snapshot's — the test result is the fresher of the two, and
   * where they disagree the classification's branch-2 wording is exactly the
   * right story.
   */
  const explain = useCallback(
    (address: string, decided: string): string => {
      const observed = clients.find((client) => client.ip === address);
      const subjectClient: Client =
        observed === undefined
          ? {
              ip: address,
              name: null,
              first_seen: '',
              last_seen: '',
              queries_24h: 0,
              blocked_24h: 0,
              policy: decided,
            }
          : { ...observed, policy: decided };
      return whyPolicyApplies(classifyAssignment(subjectClient, policies));
    },
    [clients, policies],
  );

  const send = useCallback(
    (body: RuleTestBody, record: Omit<TestRecord, 'result' | 'why'>) => {
      setBusy(true);
      setTestError(null);
      setAmbiguous(null);
      const test = new AbortController();
      testController.current = test;
      testRule(body, test.signal)
        .then((result) => {
          const why =
            record.sentPolicy !== null
              ? CHOSEN_POLICY_REASON
              : record.partial
                ? 'no address was given, so no assignment could apply'
                : record.sentClient === null
                  ? NO_CLIENT_REASON
                  : explain(record.sentClient, result.policy);
          setRing((current) =>
            pushRecord(current, { ...record, result, why }),
          );
        })
        .catch((cause: unknown) => {
          if (cause instanceof DOMException && cause.name === 'AbortError') {
            return;
          }
          setTestError(
            cause instanceof Error ? cause : new Error(String(cause)),
          );
        })
        .finally(() => setBusy(false));
    },
    [explain],
  );

  const runTest = useCallback(
    (forcedAddress?: string) => {
      const cleanDomain = domain.trim().toLowerCase();
      if (cleanDomain === '' || busy) return;

      if (mode === 'policy') {
        send(
          { domain: cleanDomain, qtype, policy: policyChoice },
          {
            domain: cleanDomain,
            qtype,
            sentClient: null,
            resolvedFrom: null,
            sentPolicy: policyChoice,
            partial: false,
          },
        );
        return;
      }

      const typed = (forcedAddress ?? subject).trim();
      if (typed === '') {
        send(
          { domain: cleanDomain, qtype },
          {
            domain: cleanDomain,
            qtype,
            sentClient: null,
            resolvedFrom: null,
            sentPolicy: null,
            partial: false,
          },
        );
        return;
      }

      // An address goes straight through. Only a name needs resolving.
      if (forcedAddress !== undefined || parseIp(typed) !== null) {
        send(
          { domain: cleanDomain, qtype, client: typed },
          {
            domain: cleanDomain,
            qtype,
            sentClient: typed,
            resolvedFrom:
              forcedAddress !== undefined && subject.trim() !== typed
                ? subject.trim()
                : null,
            sentPolicy: null,
            partial: false,
          },
        );
        return;
      }

      const matches = clients.filter(
        (client) =>
          client.name !== null &&
          equalsAsciiCaseInsensitive(client.name, typed),
      );

      if (matches.length > 1) {
        setAmbiguous(matches);
        return;
      }

      const only = matches[0];
      if (only !== undefined) {
        send(
          { domain: cleanDomain, qtype, client: only.ip },
          {
            domain: cleanDomain,
            qtype,
            sentClient: only.ip,
            resolvedFrom: only.name ?? typed,
            sentPolicy: null,
            partial: false,
          },
        );
        return;
      }

      // Never observed. The request still answers a real verdict — a
      // `$client` rule naming it is satisfied — but the policy half is not an
      // answer, and the result says so.
      send(
        { domain: cleanDomain, qtype, client: typed },
        {
          domain: cleanDomain,
          qtype,
          sentClient: typed,
          resolvedFrom: null,
          sentPolicy: null,
          partial: true,
        },
      );
    },
    [busy, clients, domain, mode, policyChoice, qtype, send, subject],
  );

  const latest = useMemo(() => ring[0] ?? null, [ring]);

  return (
    <>
      <ContentHeader
        title="Rule tester"
        context={
          <>
            Ask the running engine what it would decide, without waiting for the
            client to ask — <span class="mono">POST /api/v1/rules/test</span>
          </>
        }
      />

      <main class="wrap">
        {loadError !== null && <ErrorState error={loadError} />}

        <div class="tester-grid">
          <QueryForm
            domain={domain}
            qtype={qtype}
            mode={mode}
            subject={mode === 'policy' ? policyChoice : subject}
            policies={policies}
            busy={busy}
            onDomain={setDomain}
            onQtype={setQtype}
            onMode={(next) => {
              setMode(next);
              setAmbiguous(null);
            }}
            onSubject={mode === 'policy' ? setPolicyChoice : setSubject}
            onSubmit={() => runTest()}
          />

          <div class="tester-column">
            {ambiguous !== null && (
              <Card title="Which one?" className="tester-ambiguous">
                <p class="note" style={{ margin: '0 0 10px' }}>
                  <b>
                    {ambiguous.length} observed clients answer to that name.
                  </b>{' '}
                  Client names are a label, not a key — nothing stops two
                  addresses carrying one, and the match ignores ASCII case. The
                  engine picks a policy from an address, so this has to be
                  settled before anything is sent.
                </p>
                <div class="ambiguous-list">
                  {ambiguous.map((client) => (
                    <button
                      key={client.ip}
                      type="button"
                      class="btn g ambiguous-choice"
                      onClick={() => runTest(client.ip)}
                    >
                      <span class="mono">{client.ip}</span>
                      <span class="note">{client.name}</span>
                    </button>
                  ))}
                </div>
              </Card>
            )}

            {testError !== null && <ErrorState error={testError} />}

            {latest !== null && <ResultCard record={latest} />}

            <SessionRing ring={ring} />

            <Card title="Two questions this answers" bodyClass="cost-body">
              <div>
                <div class="cost-head">
                  &ldquo;Why is this blocked for the TV?&rdquo;
                </div>
                <p class="note" style={{ margin: 0 }}>
                  Test as the client. The engine picks whatever policy is in
                  force for that address at this moment, so the answer accounts
                  for schedules — including a window that just closed.
                </p>
              </div>
              <div>
                <div class="cost-head">
                  &ldquo;What would the kids policy do?&rdquo;
                </div>
                <p class="note" style={{ margin: 0 }}>
                  Test under a policy instead. Assignments are ignored and the
                  named policy decides, so a policy can be checked before
                  anything is assigned to it.
                </p>
              </div>
            </Card>
            <p class="note cost-footnote">
              This runs the compiled ruleset — the same code path a real query
              takes — so it cannot drift from what clients actually get. It
              answers the verdict only: nothing is resolved, no upstream is
              contacted, and nothing is cached.
            </p>
          </div>
        </div>
      </main>
    </>
  );
}

export default RuleTester;
