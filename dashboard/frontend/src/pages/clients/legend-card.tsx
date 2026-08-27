import { Card } from '../../components/card';

/**
 * The footnote under the table. `Clients.dc.html`'s wording, with one sentence
 * corrected: the artboard says the API tells the UI *which* kind of
 * inheritance a client has, and it does not. `assignment_source` is `"direct"`
 * or absent, and absent covers a subnet, a name **and** nothing at all — so a
 * selector is named only when exactly one assignment in the in-force policy
 * matches, and otherwise the row says `inherited` and claims nothing.
 *
 * The third sentence is the C3 legend: this bar is a share of the client's own
 * traffic, unlike the Dashboard's top-N bars, which are normalised against
 * their table's largest row. Two encodings in one application is a real cost,
 * so it is stated rather than left to be discovered.
 */
export function TableFootnote() {
  return (
    <p class="note clients-footnote">
      A solid chip is an assignment naming <i>this address</i>. A dashed chip is
      inherited — from a subnet, a name, or <span class="mono">default</span>.
      The API says whether an assignment names this address; which selector it
      was inherited through is worked out from the policies, and is named only
      when exactly one of them matches.
      <span class="stacked">
        A client whose schedule window is shut still shows its policy, marked as
        not in force, together with the policy that is. That is the honest
        reading: the assignment exists, it is simply not deciding anything right
        now.
      </span>
      <span class="stacked">
        The blocked-share bar is that client&apos;s own blocked share of its own
        queries — not a share of the table, which is what the Dashboard&apos;s
        top-ten bars draw.
      </span>
    </p>
  );
}

export function EditingCard() {
  return (
    <Card title="Editing a client">
      <p class="note" style={{ margin: 0 }}>
        The pencil on a row opens its two actions in place: rename, and change
        policy. Both are one round trip and both are live in milliseconds —
        assignments change no rule mask, so nothing here recompiles and nothing
        here asks for confirmation.
        <span class="stacked">
          Start and end are set together or not at all. Saving replaces any
          assignment this address already had, in whichever policy held it — one
          address, one assignment.
        </span>
        <span class="stacked">
          Choosing <span class="mono">default</span> clears the assignment
          rather than making one: <span class="mono">default</span> is the
          implicit policy every unassigned client already gets.
        </span>
      </p>
    </Card>
  );
}

export function WhatThisIsNot() {
  return (
    <Card title="What this page is not">
      <p class="note" style={{ margin: 0 }}>
        Clients here are <b>observed by traffic</b>. There is no ARP table, no
        DHCP lease list, and no device inventory behind this — a machine that
        has never sent a query does not appear, and one that stops asking stays
        listed with an old <span class="mono">last_seen</span> rather than
        dropping off.
        <span class="stacked">
          Names are a label you set, not something discovered from the network.
          FastAdHunter does not do reverse lookups to fill them in.
        </span>
        <span class="stacked">
          The 24 h counts come from the same rolling window as the Dashboard, so
          they will not agree with a longer range picked on the history pages.
        </span>
      </p>
    </Card>
  );
}
