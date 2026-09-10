# Live policy is not boot configuration; detect rejection, never auto-exclude

Two questions arrived together and turned out to have one answer.

The first: HTTPS interception mints a leaf per host from the CA the client
installed, and an application that pins its certificate refuses that leaf and
stops working. The current Phase 3 build handles this by prediction —
`fah_http::BASELINE_EXCLUSIONS` lists 34 hostname families known to pin, and a
matching SNI takes the splice leg instead of the terminate leg. That is where
the build happens to stand, not a model Phase 3 set out to adopt.

Prediction has a shelf life. On 2026-09-10, checking candidate additions to that
list, two Romanian banks that would have been added a year earlier turned out to
be gone: `otpbank.ro` answers SERVFAIL after OTP Bank Romania was absorbed by
Banca Transilvania, and `alphabank.ro` has no A record and an NXDOMAIN `www`
after Alpha Bank Romania merged into UniCredit. Neither failure is visible to
anyone maintaining the list, and the same blindness runs the other way: a bank's
mobile app often calls an API host sharing no parent with the brand.

The second: `[https.interception] clients` and `exclude_domains` live in
`fastadhunter.toml` beside `hello_timeout_ms` and `max_connections`, and the
whole `https` section is a boot key (`fah-api/src/config_store.rs:46`). So an
operator excluding one host must restart the household's DNS resolver. The
obvious patch — promote one key out of `BOOT_KEYS` — is the wrong shape, and the
comment on that list says so: promotion needs a live consumer, *"never by moving
it out of this list and hoping."*

The answer to both is that these two lists were never configuration. They are
policy, they change while FAH runs, and they belong somewhere that says so.

## Two lifetimes, two homes

**`config.toml` is boot configuration** — how FAH starts. Listen addresses,
timeouts, cache sizing, runtime counts, upstreams. Read once, applied at
startup, changed rarely and deliberately.

**`policy.json` is live operator policy** — how FAH behaves right now:

```json
{
  "clients": [],
  "exclude_domains": []
}
```

`clients` names the devices whose TLS is intercepted. `exclude_domains` names
the hosts that are never intercepted for those devices. Both change during
normal operation, both are edited from a dashboard, and neither has any business
forcing a restart.

**`clients` governs interception only. SNI filtering stays household-wide.**
The SNI verdict is taken before interception is considered at all
(`https.rs:169` judges, `https.rs:183` then asks whether to intercept), so an
unlisted television still has its ad hosts blocked at the SNI — it is simply
never decrypted and never sees the CA. `clients = []` therefore means *no
device is MITM'd*, not *no device is filtered*. What listing a device buys is
judgement inside TLS: a single URL on an otherwise allowed host can be blocked
for it, where an unlisted device only ever gets whole-host decisions.

The precedent is already in the tree: rules, lists and history live outside the
TOML with their own lifecycle. These two lists are far closer to a ruleset than
to `hello_timeout_ms`.

`policy.json` lives in `/config`, not `/data`. It is operator intent, not
derived state — it belongs with what you back up and restore, not with the cache
and the history.

## The policy document and its API

```text
GET /api/v1/policy   → the whole document
PUT /api/v1/policy   → replace the whole document
```

`/config` keeps `GET` and `POST` for boot configuration. The split is semantic
and worth the two endpoints:

```text
/config  = how FAH starts
/policy  = how FAH behaves now
```

`PUT` replaces the whole document rather than merging. Add and remove become the
same operation, the dashboard round-trips exactly what it displayed, and there
is no deep-merge rule to reason about — the array semantics the old
`POST /api/v1/config` had by accident (`config_store.rs:153` recurses only on
objects) become the contract on purpose.

Whole-document replacement means concurrent editors silently last-write-wins.
Acceptable for a single-operator dashboard; if that stops being true, the answer
is a version or ETag on the document, not a merge.

## Atomic swap, and no `restart_required`

A policy change applies on the **next connection**. No restart, and the response
carries no `restart_required` — the field is meaningless here, which is the
point of the split.

`ExclusionSet` and the client list are read on the hot path, once per accepted
connection, so the swap follows the pattern hard rule 3 already sanctions for
ruleset and config changes: build the new value off the hot path, publish it
with a single atomic store, let in-flight connections finish under the old one.
No locks, no allocation in the read path.

The swap point does not exist yet: with `clients = []` the binary builds no
`Interception` at all, so today there is nothing to publish into. Creating one
is the task's first move, and its shape belongs in the task file, not here.

Connections already established are not reconsidered. A device removed from
`clients` stops being intercepted on its next connection, not mid-session.

## `FAH__` does not reach policy

Environment variables override `config.toml` today: `defaults < file < FAH__`.
**Policy is not part of that chain.** `FAH__` cannot set `clients` or
`exclude_domains`.

Two reasons. Policy is written by the running system on operator action, so an
env var would fight the API and win silently on every restart, reverting changes
the operator made through the dashboard. And a container env list is exactly the
place a security-relevant list should not hide — on this deployment `fah-env` is
edited on the router, far from FAH's own audit surface.

## Migration

Both keys ship in `fastadhunter.toml` today and `InterceptionConfig` carries
`deny_unknown_fields`, so an existing config would hard-fail the moment the
fields are removed from the schema. Deletion alone is not a migration.

**`policy.json` always wins.** If it exists, it is the source of truth and any
legacy TOML values are ignored, with a warning naming both files so the
disagreement is visible. If it does not exist, first boot reads
`[https.interception]`, writes its two lists into `policy.json` atomically —
the tmp-then-rename path at `fah-config/src/lib.rs:110` — and records that it
did.

Migration therefore runs exactly once and can never overwrite a policy the
operator has already edited. A half-written `policy.json` cannot exist, so a
crash mid-migration leaves the old TOML authoritative and the next boot retries.
An operator who never touched the keys sees nothing; one who did keeps their
lists without editing anything.

The window is exactly two releases, and the criterion is the release number,
not a judgement call:

- **Release N** — accept the TOML keys, warn naming `policy.json` as their new
  home, migrate on first boot. `policy.json` is authoritative from that moment;
  the TOML copy is inert.
- **Release N+1** — reject the TOML keys. `deny_unknown_fields` does this by
  itself once the fields leave the schema, so the work in N+1 is deletion, and
  the only requirement is that N shipped first.

A config that reaches N+1 without ever booting N fails to start with an error
naming `policy.json`. That is the intended outcome, not a regression: it is a
config skipping its migration, and failing loudly beats starting with an empty
policy the operator did not choose.

Two steps in the p3-06 runbook set `https.interception.clients` through
`POST /api/v1/config` and move to `PUT /api/v1/policy` in the same change. `R2`
sets it to `[]` alongside `engine.mode` and `egress.allow_destinations`, which
stay on `/config`. `R8` lists a real client, and its "takes effect at the next
container start", its restart row and its rollback row all go with it — listing
a client stops needing a restart, which is the point.

## Detect instead of predict, but never auto-exclude

`BASELINE_EXCLUSIONS` is deleted. `exclude_domains` in `policy.json` becomes the
only source, and its default is `[]`.

Compiling the list into the binary was the underlying mistake that prediction's
shelf life made visible. The coupling it creates is the wrong shape:

```text
a domain changes  →  edit source  →  rebuild  →  redeploy the resolver
```

when the operator's need is:

```text
a domain changes  →  edit policy  →  next connection
```

The data is also regional — eight Romanian banks compiled into a binary someone
elsewhere runs — and, decisively, **irrevocable**: `exclusions.rs:60` seeds the
constant on every construction and only `insert`s user entries, so no config,
API call or dashboard action can remove one. Thirty-four hosts are permanently
un-interceptable. The problem is not that the list is stale. It is that nobody
can correct it, which cannot coexist with an operator-controlled model.

In its place, FAH detects what it previously guessed. `intercept.rs:151` already
catches the moment a client refuses our leaf and names the host, then throws it
away into a `debug!` line. That observation becomes a first-class event.

## detect ≠ auto-exclude, and the code cannot blur it

This separation is the decision. Everything else is mechanism.

**Detection observes. It never changes policy.** A `ClientCertRejected` event
records that a client refused our certificate for a host, at a time, from an
address. It does not add that host to `exclude_domains`, does not mark it
pending, and does not stage a change for some later step to apply. The host
keeps being intercepted, and keeps failing, until a human decides otherwise. A
rejection nobody acts on must remain a rejection forever — that is correct
behaviour, not a gap to close.

**The operator identifies the host and adds it, deliberately.** Reading the
view, judging that this host should not be intercepted, and choosing whether to
exclude the exact host or its parent are human acts. The dashboard's buttons
spell the hostname correctly; they are not a shortcut around the decision, and
no default, countdown or bulk action may take it on the operator's behalf.

**No runtime path may modify policy as a consequence of traffic observation or
detection. After migration, every policy change requires an explicit operator
action.**

The invariant is worded that way deliberately. "No code path ever writes
policy" would be false: first-boot migration writes `policy.json` without
anyone clicking anything, and so would any future import or restore. Those are
legitimate — they are lifecycle operations with a defined trigger, not
inferences drawn from traffic. What is forbidden is policy changing *because
FAH observed something*.

**The forbidden direction is also structural.** The detector lives in
`fah-http` (L3); the policy writer is in `fah-api` (L3), a sibling. Hard rule 1
forbids sibling imports,
`fah-http/Cargo.toml` depends only on L1 and L2 crates, and
`crates/fastadhunter/tests/layering.rs` fails the build if that changes. The
component that detects *cannot reach* the component that writes policy, whatever
a future contributor intends.

One naming consequence: the list is `exclude_domains`, an exclusion list —
never a "pinned" list, in code, API, dashboard or conversation. Rejection is the
observation; pinning is one possible cause among several.

## What counts as a rejection

`intercept.rs:150` treats every `rustls` accept failure alike and emits
`status 0`, which six other paths in the same function also emit — unusable
SNI, upstream connect failure, no leaf, incomplete minting, our own handshake
deadline, upstream HTTP handshake failure. A list mixing those together is not
actionable.

Only a client TLS alert corresponding to certificate rejection counts. A version
mismatch, a transport reset or a truncated hello is not evidence about
certificates and stays on the generic `0`.

The mechanism is measured, not assumed. On the accept side a client's fatal
alert reaches `acceptor.accept()` as an `io::Error` of kind `InvalidData`
carrying `rustls::Error::AlertReceived(AlertDescription)`, on TLS 1.3 and TLS
1.2 alike (`fah-http/tests/client_rejection.rs`). `tls.rs:85`
`certificate_error` is the precedent for that downcast, not the machinery for
it: it matches `InvalidCertificate(_)`, a variant the accept side never
produces, so the detector needs its own predicate and must not reuse that one.

**The exact mapping is established by test, not by this document, and the set
of alerts is open rather than closed.** Which alert a real client sends is a
property of that client's TLS stack, and the tests own it: each classified
alert gets a case, and anything unclassified stays on `0` by default rather
than by omission. What is measured so far is one stack — rustls 0.23.42
clients, driven by a verifier returning a chosen verdict: an empty root store
and `UnknownIssuer` both produce `UnknownCA` (0x30), `NotValidForName` produces
`BadCertificate` (0x2a), and `ApplicationVerificationFailure` — the verdict a
pinning-style verifier returns — produces **`AccessDenied` (0x31)**. That is
rustls's own verdict-to-alert mapping, not a claim about what every pinning
client sends: the alert is a property of the client's stack, and `AccessDenied`
belongs in the classified set because it was observed, not because pinning
implies it. `CertificateUnknown` (0x2e) has the same shape and is expected from
other stacks. `BadCertificate` is likewise not exclusive to pinning, which is
one more reason the event states what was observed and leaves the diagnosis to
the reader.

Two facts from reading rustls that shape the design:

- **`UnknownCA` is a strong signal of an untrusted or missing FAH CA, and does
  not produce `ClientCertRejected`.** It does not prove the CA is absent — the
  client is saying it could not build a trusted chain, which a missing CA, an
  untrusted one, a policy that ignores user-installed roots, or a store the
  application does not consult would all produce. That is a different question
  from a pinning client, which builds the chain successfully and then rejects
  the identity with `AccessDenied`, `BadCertificate` or `CertificateUnknown`.
  The two stay separate all the way through: `UnknownCA` feeds the diagnosis in
  §missing-CA, never a row in the exclusion view, because excluding a host on
  the strength of it would paper over a trust problem with a permanent policy
  entry.
- **Not every rejection produces an alert.** Clients may close the connection
  with RST or FIN instead, which reaches `accept()` as a plain transport
  `io::Error` and carries no `AlertReceived`. Those rejections are **not
  classifiable** and will not appear in the view. The feature surfaces the
  rejections that announce themselves; it is not a complete census of hosts
  that refuse our certificate, and must not be described as one.

The rejection gets its own status code, **525**, beside the existing
`UPSTREAM_CERT_FAILURE = 526` at `intercept.rs:36` — the same convention already
chosen for the upstream case.

The event is `https`, never `https-sni`. A pinned application's SNI verdict is
`pass`, and the failure happens afterwards inside the terminate leg. Blocked
`https-sni` events are ad domains stopped by a filter rule; attaching this
feature to them would invite an operator to exempt the very hosts they meant to
block.

The technical event name is `ClientCertRejected`. The dashboard says
"Certificate rejected by client". It is deliberately not "Pinned Apps": a
missing CA, a clock skew, an enterprise policy and a genuinely pinned app all
produce it, so the name states what was observed and leaves the diagnosis to
the reader.

## The operator's path

A dedicated live-feed view, separate from general HTTPS traffic:

```text
Live Feed
  ├─ All
  ├─ DNS
  ├─ HTTPS
  └─ Interception Issues
        └─ Certificate rejected by client
```

The view is entered only when something probably does not tolerate
interception, so every row on it is worth a decision. Each row names the host,
the client and the time, and offers one action: exclude the exact host
observed. A wider entry covers everything beneath it, so widening is a
deliberate act — and here it is one the operator performs by editing policy,
not one the view offers.

**The widening button is not built, and that is a decision, not a
placeholder.** Widening correctly means public-suffix semantics, not a
parent-label walk: `api.foo.co.uk` widens to `foo.co.uk`, never to `co.uk`;
`bank.ro` widens to itself, not to `ro`. A label walk would offer a button that
excludes an entire TLD, a catastrophic click one pixel from the safe one. The
tree's `fah_rules::url_matcher::registrable()` is exactly that walk, kept
deliberately as an approximation because a wrong answer there only ever
*narrows* a rule. Here the error direction inverts, so it must not be reused.

Correct widening therefore needs a public-suffix list, and p2-03 already
declined one — a dependency, ~200 KB of tables and a refresh story. That
judgement holds harder in this decision than in the one that made it: a
compiled-in public-suffix table is the same compiled-in data with a shelf life
that `BASELINE_EXCLUSIONS` is being deleted for, and the Mozilla list is
MPL-2.0 data in a workspace that ships no third-party notice today. A
dependency introduced to make a button work is the wrong reason to take one.

The action reads the current policy, adds the entry, and `PUT`s the whole
document back.

## The missing-CA case is a diagnosis, not a list

A listed client without the CA installed rejects every host it visits — p3-04 L2
already establishes that such a client is closed rather than spliced. Under this
decision that is not dangerous, because nothing auto-excludes, but it would fill
the view with dozens of rows when the real finding is one: *this client has no
CA*. That finding belongs at the client level, not as a row per host.

**What counts as "CA configured" is left to the implementation to define, and
it is not "has completed a handshake at least once".** That test is too strong
and ambiguous at the edges: a newly listed device has completed nothing yet and
is indistinguishable from a misconfigured one, and a device that simply has not
browsed since boot looks the same again. Whatever definition is chosen must
survive a device's first minutes on the list without accusing it, and must not
depend on state that a restart erases — otherwise every restart re-accuses
every client. The `UnknownCA` alert above is the strongest single input, since
it is the client stating the cause directly, but the definition is a design
decision for the task, and its edges belong in tests.

This is the clearest argument for the whole decision. The same failure that
would silently disable interception under auto-exclusion becomes, here, a
labelling problem.

## A long exclusion list is a symptom

`clients` and `exclude_domains` are unbounded today, unlike
`MAX_UPSTREAM_SERVERS = 8` and `MAX_POLICIES = 16`. That was tolerable while the
lists were hand-written. Once a dashboard button appends to them, it is not.

Lookup cost does not grow with length — `ExclusionSet::contains` walks the
queried host's labels, two to four probes regardless of list size — so the cap
is not about performance. It is about the pathology: if `exclude_domains`
reaches hundreds, the likely cause is not three hundred pinned banking apps but
the missing-CA case, with the operator clicking through symptoms one at a time
and switching interception off host by host while it appears to work. A cap
around 512 turns that slide into a named error pointing at the diagnosis.

**Reaching the cap rejects the update; the existing policy is unchanged.** A
`PUT` whose document exceeds the limit fails validation and nothing is written
— no partial application, no truncation to the first 512, no silent drop of the
overflow. The operator keeps the policy they had and gets an error saying which
list is over and by how much. Since `PUT` replaces the whole document, the way
back under the cap is to remove entries and submit again.

## Consequences to handle

- `a_baseline_bank_is_never_intercepted_even_for_a_listed_client`
  (`crates/fah-http/tests/interception.rs`) drives a *baseline* name and must be
  rewritten to drive a policy entry. The contract it pins — an excluded host is
  spliced even for a client eligible for interception — does not change.
- `the_baseline_is_present_with_an_empty_user_list`,
  `the_baseline_exclusions_ship_without_any_configuration` and
  `every_baseline_entry_is_a_valid_hostname` are **deleted, not adapted**. Each
  asserts that a shipped list exists with no configuration, which is the
  behaviour this ADR removes; keeping them under new names would preserve the
  claim they were written to defend. Nothing replaces them: the test that an
  empty policy excludes nothing already exists as `the_empty_set_matches_nothing`
  (`fah-http/src/exclusions.rs`). One assertion inside the deleted middle test
  does need a new home — it bounds 10 000 lookup misses at under a second, the
  only lookup-cost check in that file, and it is re-pinned on a policy-built
  set.
- `ExclusionSet::empty()` and `ExclusionSet::new(&[])` are the same function
  once the baseline is gone. One of the two goes with it.
- CONFIGURATION.md must say plainly that **`config.toml` no longer owns
  `clients` or `exclude_domains`**, and that **`policy.json` is their persistent
  source of truth** — not merely that the keys moved. Wording that says the keys
  were removed invites the reading that they still exist somewhere else under
  configuration, which is exactly the confusion this ADR exists to end. The same
  sentence names `GET`/`PUT /api/v1/policy` as the way to read and change them.
- API.md gains `/api/v1/policy`; SECURITY.md's opt-in description and its
  "Exclusions always splice" bullet both name a compiled-in baseline today.
- Runbook 4's excluded arm relies on `unicredit.ro` sitting in the constant.
  Its precondition moves from source to policy; the check itself does not
  change.

## Phasing

**Nothing changes while the 0.3.3 soak is frozen.** All of it ships in the
full-mode build that the 24 h soak then exercises, which is also the build the
owner will not approve as production-ready without it.

The order carries the argument:

1. **Policy leaves the binary and the TOML.** `policy.json`, `/api/v1/policy`,
   atomic swap, migration. `BASELINE_EXCLUSIONS` deleted, default `[]`.
2. **Detection.** Alert classification, status 525, `ClientCertRejected` on the
   `https` event.
3. **The view and the actions.**

Doing 3 before 1 leaves a legible failure the operator still cannot fix without
a restart. Doing 1's empty default before 3 exists breaks pinned applications
with no view to find them in. The sequence is as much the decision as the parts.

## Revisit criteria

Reopen auto-exclusion only if operating experience shows the manual path is a
real bottleneck — many rejections per week from hosts the operator would have
approved every time — and only with the missing-CA case handled first, since it
is the failure that makes automation unsafe rather than merely inconvenient.

Reopen the empty default if operating a fresh install proves painful enough
that a seeded starting list earns its keep. The seed would be a `policy.json`
written at first install, editable and removable like anything else in it.
**What is not reopened is restoring policy to compiled-in source code**: policy
having a default is a product question and stays open; policy living in the
binary is settled.

Reopen exact-host-only if operators are shown to be widening by hand, host by
host, often enough to matter — and then with the public-suffix list's lifecycle
answered first, never with a crate chosen to make the button work.

Reopen the `/policy` and `/config` split if a third lifetime appears — something
neither boot-fixed nor operator-edited — rather than widening either endpoint to
absorb it.

Reopen `FAH__`'s exclusion from policy only alongside a story for what happens
when the environment and the dashboard disagree. Today there is none, which is
why the environment does not get a vote.
