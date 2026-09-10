# The interception lists are live, not boot configuration; detect rejection, never auto-exclude

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
whole `https` section is a boot key (`BOOT_KEYS` in
`fah-api/src/config_store.rs`). So an operator excluding one host must restart
the household's DNS resolver. The comment on that list says how a key earns a
live classification: by getting a live consumer first, *"never by moving it out
of this list and hoping."*

The answer to both is that these two lists are not boot configuration. They
change while FAH runs, a dashboard edits them, and they belong in a document
that says so.

## Two lifetimes, two homes

**`fastadhunter.toml` is boot configuration** — how FAH starts. Listen
addresses, timeouts, cache sizing, runtime counts, upstreams. Read once, applied
at startup, changed rarely and deliberately.

**`interception.json` is the Interception Document** — how interception
behaves right now:

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

The name is deliberate. *Policy* is taken: CONTEXT.md's Policy is the
parental-control bundle behind `[[policies]]`, `/api/v1/policies` and
`MAX_POLICIES`, and every request event already carries a `policy` field naming
one. A second thing called policy, one letter away from `/api/v1/policies`,
would be a trap in every API call and every conversation. So the document is
named for what it governs, and nothing in code, API, dashboard or prose calls it
a policy.

**`clients` governs interception only. SNI filtering stays household-wide.**
The SNI verdict is taken before interception is considered at all (`https.rs`
calls `judge`, and only then `interception_for`), so an unlisted television
still has its ad hosts blocked at the SNI — it is simply never decrypted and
never sees the CA. `clients = []` therefore means *no device is intercepted*,
not *no device is filtered*. What listing a device buys is judgement inside
TLS: a single URL on an otherwise allowed host can be blocked for it, where an
unlisted device only ever gets whole-host decisions.

`interception.json` lives on the `/config` volume, not `/data`. It is operator
intent, not derived state — it belongs with what you back up and restore,
beside `fastadhunter.toml`, `auth-hash` and the CA, not with the cache and the
history.

### Promoting in place was the alternative

The tree's own pattern for a live, operator-edited list is a TOML key with a
live consumer and its own endpoint: `[[policies]]` and `[[rules.lists]]` are
exactly that, and `BOOT_KEYS` already classifies `history` field by field.
Promoting the two keys the same way needs no new file, no migration and no
second loader — `POST /api/v1/config` already replaces arrays wholesale, and
`FAH__` cannot set an array whichever file holds it (§`FAH__`).

What that path cannot give is a file whose contents are live by construction.
`fastadhunter.toml` mixes lifetimes today and keeps doing so after this
decision; a dashboard button that appends to a list inside it is one
classification mistake away from writing a boot key, and a reader has to
consult `BOOT_KEYS` to know which is which. The separate document is the only
thing this button can write, and everything in it applies on the next
connection.
This decision pays §Migration for that guarantee — once — and the price is
listed there in full: a second loader and writer, a two-release window, and two
files to back up instead of one.

## The document and its API

```text
GET /api/v1/interception   → the whole document
PUT /api/v1/interception   → replace the whole document
```

`/api/v1/config` keeps `GET` and `POST` for boot configuration. The split is
semantic and worth the two endpoints:

```text
/api/v1/config        = how FAH starts
/api/v1/interception  = how interception behaves now
```

`PUT` replaces the whole document rather than merging. Add and remove become the
same operation, the dashboard round-trips exactly what it displayed, and there
is no deep-merge rule to reason about — the array semantics
`POST /api/v1/config` has by accident (`merge` recurses only on objects) become
the contract on purpose.

`PUT` validates before anything else happens, with the validators the TOML path
uses today — `AllowedNet` for a client, `sni::normalize` for a host — plus two
things a hand-written list never needed. Unknown keys are rejected, as they are
in the TOML. A duplicate after normalization is rejected rather than collapsed,
so a dashboard that appends blindly cannot grow the document with repeats, and
the stored document is always what the operator sent. The caps are in §A long
exclusion list is a symptom. `PUT` sits behind the same authentication as
`POST /api/v1/config`, and an accepted change is logged at `info` with both
counts — the log is FAH's audit surface.

Whole-document replacement means concurrent editors silently last-write-wins.
Acceptable for a single-operator dashboard; if that stops being true, the answer
is a version or ETag on the document, not a merge.

## Atomic swap, and no `restart_required`

A change applies on the **next connection**. No restart, and the response
carries no `restart_required` — the field is meaningless here, which is the
point of the split.

The hot path reads the current interception state once per accepted connection
and never rebuilds it there, so the swap follows the pattern hard rule 3 already
sanctions for ruleset and config changes: build the new value off the hot path,
publish it with a single atomic store, let in-flight connections finish under
the old one. Obtaining that state takes no lock and no allocation. Which owned
or atomic handle carries it is the implementation task's to define.

The swap point does not exist yet: `interception()` in `main.rs` returns `None`
when `clients` is empty, before it looks at the certificate store, so today
there is nothing to publish into. Under this decision the machinery is built
whenever the HTTPS listener runs and the certificate store opened, whatever the
client list says — a store with no CA yet builds it too, as it does today for a
listed client, and the client closes until a CA exists. Two boots leave it
absent. An `engine.mode` without the HTTPS listener has no proxy to publish
into; the document is still stored and applies at the first boot of a mode that
has one, exactly as `[https.interception]` behaves in that mode today. A boot
where the store did not open has a listener and no machinery; a `PUT` that
lists a client on such a boot is rejected with that reason, and the fix is the
one the existing warning already names — repair `/config` and restart. That
leaves the implementation
task one problem this decision does not solve: what live replacement means for
a value the hot path reads by reference (`interception_for` returns
`Option<&Interception>` borrowed from the proxy). Which abstraction carries it
belongs in that task and its approved plan, not here.

One constraint is settled here because hard rule 1 settles it. The `PUT`
handler lives in `fah-api`; the state it must produce — the `Interception`
value holding the client nets and the `ExclusionSet`, and the handle the hot
path reads — is `fah-http`'s. Siblings
never import each other, so the handler cannot build that state itself. Either
the binary owns the apply path and hands `fah-api` a way to submit a validated
document and await the outcome — a channel or a callback constructed in
`main.rs`, the wiring the layering rule already prescribes — or the runtime
types move down to a layer both can see. The task chooses between those two;
`fah-api` importing `fah-http` is not a third option.

The order inside the apply path follows from one requirement: after a `PUT`
returns success, the file and the active state are the same document, and after
a failure both are what they were. Validate, build, persist, publish. Build
cannot fail once validation passed; persist can, and then nothing is published;
publish cannot fail. A crash between persist and publish leaves the file ahead
of the runtime, and the next boot reads the file — the only order in which a
crash heals itself.

Connections already established are not reconsidered. A device removed from
`clients` stops being intercepted on its next connection, not mid-session.

## `FAH__` does not reach the document

Environment variables override `fastadhunter.toml`: `defaults < file < FAH__`.
`apply_one` in `fah-config/src/env.rs` maps each variable to one scalar field
by hand and fails boot on any path it does not know
(`ConfigError::UnknownEnvKey`). No arm exists for either list, no coercion for
an array exists, and this decision adds neither.
`FAH__HTTPS__INTERCEPTION__CLIENTS` fails boot today and keeps failing, for the
same reason a typo does.

That is the right outcome, not an accident. The document is written by the
running system on operator action; an environment variable would fight the API
and win silently on every restart, reverting changes made through the
dashboard. And a container env list is exactly the place a security-relevant
list should not hide — on this deployment `fah-env` is edited on the router, far
from FAH's own audit surface.

## Migration

Both keys ship in `fastadhunter.toml` today, and not only in hand-edited files:
first-boot generation writes them into every new file
(`Config::default().to_toml_string()`), every `POST /api/v1/config` writes them
back (`Config::save` serialises the whole struct), and `InterceptionConfig`
carries `deny_unknown_fields`. So the moment the two fields leave the schema,
**every** file FAH ever wrote fails to parse. Deletion alone is not a migration,
and neither is deletion after a release that leaves the keys in the file.

**`interception.json` always wins.** If it exists, it is the source of truth; a
TOML that still carries either key is warned about, naming both files, and the
TOML value is ignored. If it does not exist, boot reads `[https.interception]`,
writes its two lists into `interception.json` atomically — `write_atomic` in
`fah-config/src/lib.rs`, the tmp-then-rename path — and records that it did. A
fresh install goes through the same path and gets the empty document, so `GET`
always has a document to return.

Then, in either case, boot removes the keys from the TOML. In release N the two
fields become `Option<Vec<String>>`, serialised only when present, so presence
is observable and absence is the normal state; migration consumes them, sets
both to `None` and saves the file through the same atomic path. After any boot
of release N, `fastadhunter.toml` carries neither key. A `POST /api/v1/config`
that names either key is rejected with an error naming `/api/v1/interception` —
a check on the patch, not on the schema, so it survives N+1's deletion — and
nothing can put them back through the API.

Migration therefore runs exactly once and can never overwrite a document the
operator has already edited: the document is written only when absent and is
never rewritten at boot. A half-written document cannot exist, so a crash
mid-migration leaves the old TOML authoritative and the next boot retries; a
crash between writing the document and saving the TOML is healed by the next
boot's warn-and-remove. A document that cannot be read, parsed or validated
fails boot naming the file — the contract `fastadhunter.toml` already has — and
so does a `/config` that cannot be written during migration, exactly as
first-boot generation fails today. An operator who never touched the keys sees
one log line at one boot; one who did keeps their lists without editing
anything.

The window is exactly two releases, and the criterion is the release number,
not a judgement call:

- **Release N** — accept the TOML keys as `Option`, migrate on first boot,
  remove them from the file, reject them on `/api/v1/config`.
  `interception.json` is authoritative from that moment.
- **Release N+1** — delete the two fields. `deny_unknown_fields` then rejects a
  file that still carries them, so the work in N+1 is deletion, and the only
  requirement is that N shipped first.

A config that reaches N+1 without ever booting N fails to start with serde's
unknown-field error for the key. It does not name `interception.json`, because
nothing in N+1 knows the key existed; CONFIGURATION.md's entry for the removed
keys says where they went. That is the intended outcome, not a regression: it
is a config skipping its migration, and failing loudly beats starting with an
empty document the operator did not choose.

Two steps in the p3-06 runbook touch `https.interception.clients` through
`POST /api/v1/config`, and both change in the same release. `R2` sets it to `[]`
alongside `engine.mode` and `egress.allow_destinations`; the two boot keys stay
on `/api/v1/config`, and the `https` part of the body is deleted — the document
is empty by default. `R8` lists a real client; it becomes a
`PUT /api/v1/interception`, and its "takes effect at the next container start",
its restart row and its rollback row all go with it — listing a client stops
needing a restart, which is the point.

## Detect instead of predict, but never auto-exclude

`BASELINE_EXCLUSIONS` is deleted. `exclude_domains` in `interception.json`
becomes the only source, and its default is `[]`.

Compiling the list into the binary was the underlying mistake that prediction's
shelf life made visible. The coupling it creates is the wrong shape:

```text
a domain changes  →  edit source  →  rebuild  →  redeploy the resolver
```

when the operator's need is:

```text
a domain changes  →  edit the document  →  next connection
```

The data is also regional — eight Romanian banks compiled into a binary someone
elsewhere runs — and, decisively, **irrevocable**: `ExclusionSet::new` seeds
the constant on every construction and only `insert`s user entries, so no
config, API call or dashboard action can remove one. Thirty-four hosts are
permanently un-interceptable. The problem is not that the list is stale. It is
that nobody can correct it, which cannot coexist with an operator-controlled
model.

In its place, FAH detects what it previously guessed. The accept arm of
`intercept()` already catches the moment a client refuses our leaf and names
the host, then throws it away into a `debug!` line and a `status 0` event. That
observation becomes a first-class event.

## detect ≠ auto-exclude, and the code cannot blur it

This separation is the decision. Everything else is mechanism.

**Detection observes. It never changes the document.** A `ClientCertRejected`
event — an `https` event with status 525, §What counts as a rejection — records
that a client refused our certificate for a host, at a time, from an address.
It does not add that host to `exclude_domains`, does not mark it
pending, and does not stage a change for some later step to apply. The host
keeps being intercepted, and keeps failing, until a human decides otherwise. A
rejection nobody acts on must remain a rejection forever — that is correct
behaviour, not a gap to close.

**The operator identifies the host and adds it, deliberately.** Reading the
view, judging that this host should not be intercepted, and choosing whether to
exclude the exact host or its parent are human acts. The view's one button
spells the hostname correctly; it is not a shortcut around the decision, and no
default, countdown or bulk action may take it on the operator's behalf.

**No runtime path may modify the document as a consequence of traffic
observation or detection. After migration, every change to it requires an
explicit operator action.**

The invariant is worded that way deliberately. "No code path ever writes the
document" would be false: first-boot migration writes it without anyone
clicking anything, and so would any future import or restore. Those are
legitimate — lifecycle operations with a defined trigger, not inferences drawn
from traffic. What is forbidden is the document changing *because FAH observed
something*.

**The forbidden direction is also structural.** The detector lives in
`fah-http` (L3); the writer is in `fah-api` (L3), a sibling. Hard rule 1
forbids sibling imports, `fah-http/Cargo.toml` depends only on L1 and L2
crates, and `crates/fastadhunter/tests/layering.rs` fails the build if that
changes — it reads `dev-dependencies` too, so even a test-only edge fails. The
component that detects *cannot reach* the component that writes, whatever a
future contributor intends.

One naming consequence: the list is `exclude_domains`, an exclusion list —
never a "pinned" list, in code, API, dashboard or conversation. Rejection is the
observation; pinning is one possible cause among several.

## What counts as a rejection

The accept arm treats every `rustls` accept failure alike and emits `status 0`,
which six other paths in the same function also emit — unusable SNI, upstream
connect failure, no leaf, incomplete minting, our own handshake deadline,
upstream HTTP handshake failure. A list mixing those together is not
actionable.

Only a client TLS alert that rejects the certificate we presented counts — not
one saying a trusted chain could not be built (`UnknownCA`, below). A version
mismatch, a transport reset or a truncated hello is not evidence about
certificates and stays on the generic `0`.

The mechanism is measured, not assumed. On the accept side a client's fatal
alert reaches `acceptor.accept()` as an `io::Error` of kind `InvalidData`
carrying `rustls::Error::AlertReceived(AlertDescription)`, on TLS 1.3 and TLS
1.2 alike (`fah-http/tests/client_rejection.rs`). `certificate_error` in
`tls.rs` is the precedent for that downcast, not the machinery for it. The two
sides match different shapes:

```text
connect side (upstream)  InvalidData + rustls::Error::InvalidCertificate(_)
accept side (detection)  InvalidData + rustls::Error::AlertReceived(..)
```

The accept side never produces `InvalidCertificate` — that variant is a local
verification verdict, and here the verdict is the client's, arriving as an
alert. The detector therefore carries its own predicate and does not reuse
`certificate_error`.

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
implies it. `CertificateUnknown` (0x2e) is the TLS RFCs' generic "certificate
unacceptable, reason unspecified" and is classified on that definition; its
test case pins the wire value, not a client stack observed sending it.
`BadCertificate` is not exclusive to pinning either — a leaf whose name the
client did not expect produces it — which is one more reason the event states
what was observed and leaves the diagnosis to the reader.

Two facts from reading rustls that shape the design:

- **`UnknownCA` is a strong signal of an untrusted or missing FAH CA, and does
  not produce `ClientCertRejected`.** It does not prove the CA is absent — the
  client is saying it could not build a trusted chain, which a missing CA, an
  untrusted one, an OS policy that ignores user-installed roots, or a store the
  application does not consult would all produce. That is a different question
  from a pinning client, which builds the chain successfully and then rejects
  the identity with `AccessDenied`, `BadCertificate` or `CertificateUnknown`.
  The two stay separate all the way through: `UnknownCA` is not classified in
  this phase and stays on `status 0`, never a row in the rejection view,
  because excluding a host on the strength of it would paper over a trust
  problem with a permanent entry. It is a trust diagnostic, not an exclusion
  event; if a client-level diagnosis is built later, it defines its own
  contract for it (§missing-CA).
- **Not every rejection produces an alert.** Clients may close the connection
  with RST or FIN instead, which reaches `accept()` as a plain transport
  `io::Error` and carries no `AlertReceived`. Those rejections are **not
  classifiable** and will not appear in the view. The feature surfaces the
  rejections that announce themselves; it is not a complete census of hosts
  that refuse our certificate, and must not be described as one.

The rejection gets its own status code, **525**, beside the existing
`UPSTREAM_CERT_FAILURE = 526` in `intercept.rs` — the same convention already
chosen for the upstream case. Neither is a standard HTTP status; both are FAH's
private codes on the `https` event, documented in API.md. No new `Event` kind
is added: the event is an `https` event with status 525, the `kind` set stays
`dns`, `http`, `https-sni`, `https`, and `ClientCertRejected` names the
constant and the dashboard filter, not a variant.

The event is `https`, never `https-sni`. A pinned application's SNI verdict is
`pass`, and the failure happens afterwards inside the terminate leg. Blocked
`https-sni` events are ad domains stopped by a filter rule; attaching this
feature to them would invite an operator to exempt the very hosts they meant to
block.

The dashboard says "Certificate rejected by client". It is deliberately not
"Pinned Apps": pinning, a name the minted leaf does not cover, and any verifier
of the application's own all produce 525, so the name states what was observed
and leaves the diagnosis to the reader.

## The operator's path

A dedicated live-feed view — the rejection view — separate from general HTTPS
traffic:

```text
Live Feed
  kinds   all · dns · http · https-sni · https
  issues  Certificate rejected by client  —  https events with status 525
```

The view is entered only when something probably does not tolerate
interception, so every row on it is worth a decision. A pinned application
retries, so the raw stream carries the same host from the same client many
times a minute; the view groups by client and host, showing a count and the
last time seen, and the event stream underneath stays raw and bounded as it is
today. Each row offers one action: exclude the exact host observed. A wider
entry covers everything beneath it, so widening is a deliberate act — and here
it is one the operator performs by editing the document, not one the view
offers.

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

The action reads the current document, adds the entry, and `PUT`s the whole
document back.

## The missing-CA case is a diagnosis, not a list

A listed client without the CA installed rejects every host it visits — p3-04 L2
already establishes that such a client is closed rather than spliced. Under this
decision that is not dangerous, because nothing auto-excludes, and it is not
noisy either, because those rejections are `UnknownCA`, unclassified, and never
rows. But the real finding is one — *this client has no CA* — and it belongs at
the client level, not as a row per host.

**That client-level diagnosis is not part of this decision.** It is a separate
feature with its own contract, and `UnknownCA` — the client stating the cause
directly — is the input it would start from. Two constraints are recorded here
so they are not relearned: an accusation needs positive evidence, never the
absence of a success, because "has not completed a handshake yet" accuses a
device in its first minutes on the list and a device that has not browsed since
boot alike; and a restart must not *create* accusations, which only an
absence-based definition does.

This is the clearest argument for the whole decision. The same failure that
would silently disable interception under auto-exclusion is, here, an
unlabelled client: its connections close, nothing is excluded, and the label
can be added later without undoing anything.

## A long exclusion list is a symptom

`clients` and `exclude_domains` are unbounded today, unlike
`MAX_UPSTREAM_SERVERS = 8` and `MAX_POLICIES = 16`. That was tolerable while the
lists were hand-written. Once a dashboard button appends to them, it is not.

`exclude_domains` is capped at **512**. Lookup cost does not grow with length —
`ExclusionSet::contains` walks the queried host's labels, two to four probes
regardless of list size — so the cap is not about performance. It is about the
pathology: if the list reaches hundreds, the likely cause is not three hundred
pinned banking apps but the missing-CA case, with the operator clicking through
symptoms one at a time and switching interception off host by host while it
appears to work. The cap turns that slide into a named error pointing at the
diagnosis. Memory is bounded with it: `sni::normalize` refuses anything over
253 bytes (`MAX_NAME_LEN`), so the set holds under 128 KiB of names at the cap.

`clients` is capped at **256**, and there the reason is the hot path:
`Interception::intercepts` walks the list once per accepted connection, so its
length is a per-connection cost. A household is tens of devices, a CIDR entry
covers a range, and 256 comparisons are invisible beside a TLS handshake; a
thousand would not be.

**Reaching a cap rejects the update; the existing document is unchanged.** A
`PUT` whose document exceeds a limit fails validation and nothing is written —
no partial application, no truncation, no silent drop of the overflow. The
operator keeps the document they had and gets an error saying which list is
over and by how much. Since `PUT` replaces the whole document, the way back
under the cap is to remove entries and submit again.

## Consequences to handle

- `a_baseline_bank_is_never_intercepted_even_for_a_listed_client`
  (`crates/fah-http/tests/interception.rs`) drives a *baseline* name and must be
  rewritten to drive a document entry. The contract it pins — an excluded host
  is spliced even for a client eligible for interception — does not change.
- `the_baseline_is_present_with_an_empty_user_list`,
  `the_baseline_exclusions_ship_without_any_configuration` and
  `every_baseline_entry_is_a_valid_hostname` are **deleted, not adapted**. Each
  asserts that a shipped list exists with no configuration, which is the
  behaviour this ADR removes; keeping them under new names would preserve the
  claim they were written to defend. Nothing replaces them: the test that an
  empty document excludes nothing already exists as
  `the_empty_set_matches_nothing` (`fah-http/src/exclusions.rs`). One assertion
  inside the deleted middle test does need a new home — it bounds 10 000 lookup
  misses at under a second, the only lookup-cost check in that file, and it is
  re-pinned on a document-built set.
- `ExclusionSet::empty()` and `ExclusionSet::new(&[])` are the same function
  once the baseline is gone. One of the two goes with it.
- CONFIGURATION.md must say plainly that **`fastadhunter.toml` no longer owns
  `clients` or `exclude_domains`**, and that **`interception.json` is their
  persistent source of truth** — not merely that the keys moved. Wording that
  says the keys were removed invites the reading that they still exist
  somewhere else under configuration, which is exactly the confusion this ADR
  exists to end. The same sentence names `GET`/`PUT /api/v1/interception` as the
  way to read and change them. The `[https.interception]` sample rows go, and
  `defaults_match_configuration_md_sample` follows the sample.
- API.md gains `/api/v1/interception`, status 525 on the `https` event, and
  the rejection of the two keys on `POST /api/v1/config`.
- SECURITY.md's opt-in description and its "Exclusions always splice" bullet
  both name a compiled-in baseline today.
- CONTEXT.md §Exclusion names the compiled-in baseline today, and two terms
  are new: **Interception Document** and **Client Certificate Rejection**
  (`ClientCertRejected`, 525). README's tree line still says "ADRs 0001–0007".
- Runbook 4's excluded arm relies on `unicredit.ro` sitting in the constant.
  Its precondition moves from source to the document; the check itself does not
  change.
- Each step of §Phasing is one task under `plan/wip/phase3`, written against
  this ADR and numbered by the plan.

## Acceptance

Each line is a property of the shipped system and names what falsifies it.
Green tests are not the criterion; these are.

- **A change takes effect on the next connection, with no restart.** Two
  connections either side of a `PUT`, against a live origin, taking different
  legs. An endpoint test that reads the document back proves the endpoint, not
  the swap.
- **The interception machinery exists whenever the HTTPS listener runs and the
  certificate store opened.** Boot with `clients: []`, add one through `PUT`,
  and that device's next connection is intercepted — no restart. On a boot with
  the listener where the store did not open, a `PUT` listing a client is
  rejected and the document is unchanged; in a mode without the listener the
  `PUT` is stored and takes effect at the first boot of a mode that has one.
- **The response carries no `restart_required`.** The field is absent from the
  response shape, not present and false.
- **An empty document excludes nothing.** No compiled-in baseline exists, and a
  fresh install intercepts every host for a listed client unless that host is
  explicitly present in `exclude_domains`.
- **Migration runs once and never overwrites an edited document.** A second
  boot with `interception.json` present leaves it byte-identical regardless of
  the TOML, and warns naming both files if the TOML carries a key. After any
  boot of release N, `fastadhunter.toml` carries neither key.
- **`POST /api/v1/config` rejects both keys** with an error naming
  `/api/v1/interception`, and persists nothing.
- **`FAH__` cannot set either list.** `apply_one` has no arm for either path
  and no array coercion exists; setting the variable fails boot with
  `UnknownEnvKey`, as every unmapped path does.
- **`fah-http` has no path to `fah-api`.** The layering test rejects even a
  test-only dependency edge.
- **An accept failure carrying no classified alert stays on `status 0`.** A
  test pins this as an intentional fallback rather than a missing match arm.
- **`UnknownCA` is never a 525.** A client whose root store lacks the CA
  produces a `status 0` event and no row in the rejection view; a test pins
  that alert on the fallback by name, not by omission.
- **No new `Event` kind.** The 525 event is an `https` event; the `kind`
  filter set is unchanged.
- **Nothing writes the document as a consequence of traffic observation.** The
  detector, the event path and the view do not mutate the document. After
  migration, the only normal runtime writer is the explicit `PUT`; first-boot
  migration remains the defined lifecycle exception.
- **A rejected `PUT` changes nothing.** Over a cap, a duplicate, an unknown key
  or an invalid entry: the stored document is unchanged, the active runtime
  state is unchanged, and the error identifies the list and the entry or the
  overage.
- **A completed `PUT` leaves the file and the active state equal, and a failed
  one leaves both untouched.** Validate, build, persist, publish; no partially
  constructed interception state is ever published.

## Phasing

**Nothing changes while the 0.3.3 soak is frozen.** All of it ships in the
full-mode build that the 24 h soak then exercises, which is also the build the
owner will not approve as production-ready without it. That build is release N
of §Migration.

The order is the order of the tasks, not of releases — the three land in one
build, and it carries the argument:

1. **The lists leave the binary and the TOML.** `interception.json`,
   `/api/v1/interception`, atomic swap, migration. `BASELINE_EXCLUSIONS`
   deleted, default `[]`.
2. **Detection.** Alert classification, status 525, `ClientCertRejected` on
   the `https` event.
3. **The view and the action.**

Doing 3 before 1 leaves a legible failure the operator still cannot fix without
a restart. Shipping 1's empty default before 3 exists breaks pinned
applications with no view to find them in — which is why no step ships alone.
The sequence is as much the decision as the parts.

## Revisit criteria

Reopen auto-exclusion only if operating experience shows the manual path is a
real bottleneck — many rejections per week from hosts the operator would have
approved every time — and only with the missing-CA case handled first, since it
is the failure that makes automation unsafe rather than merely inconvenient.

Reopen the missing-CA diagnosis as its own feature, with its own contract for
`UnknownCA`, when the unlabelled client proves costly in practice. This ADR
guarantees only that the case is harmless and quiet until then.

Reopen the empty default if operating a fresh install proves painful enough
that a seeded starting list earns its keep. The seed would be an
`interception.json` written at first install, editable and removable like
anything else in it. **What is not reopened is restoring the list to
compiled-in source code**: the list having a default is a product question and
stays open; the list living in the binary is settled.

Reopen exact-host-only if operators are shown to be widening by hand, host by
host, often enough to matter — and then with the public-suffix list's lifecycle
answered first, never with a crate chosen to make the button work.

A future live document gets its own name and its own endpoint, as
`/api/v1/policies` and `/api/v1/lists` have — not a generic bucket beside
`/api/v1/config`, and not a widening of `/api/v1/interception` to absorb it.

Reopen `FAH__`'s exclusion from the document only alongside a story for what
happens when the environment and the dashboard disagree — an arm in `apply_one`
is a few lines; the story is the work. Today there is none, which is why the
environment does not get a vote.
