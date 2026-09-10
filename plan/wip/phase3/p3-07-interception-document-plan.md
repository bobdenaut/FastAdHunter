# P3-07 — Interception Document — Implementation Plan

**Task:** [p3-07-interception-document.md](p3-07-interception-document.md) ·
**ADR:** [ADR-0008](../../../docs/decisions/0008-live-interception-and-client-certificate-rejection.md)
frozen at `fcc7244` · **Base for inspection:** `phase3-06` at `fcc7244` ·
**Status:** plan revised 2026-09-10 after two owner reviews and the final
pre-implementation gate (findings F1–F9 folded in, §21); decisions frozen
(§18), panic/poisoning semantics frozen (§3.7); awaiting implementation
approval. Nothing implemented.

## 1. Objective and scope

`clients` and `exclude_domains` leave the binary and the TOML. They live in
`/config/interception.json` (the Interception Document), are read and replaced
through `GET`/`PUT /api/v1/interception`, and a `PUT` applies on the next
accepted connection — no restart, no `restart_required`. `BASELINE_EXCLUSIONS`
is deleted. Release N of ADR-0008 §Migration ships in this task; release N+1
(deleting the two `Option` fields) does not.

In scope: document type, validation and caps, the runtime scope and its atomic
handle, the endpoint pair with a structured error contract, the apply path
across `fah-api` → binary → `fah-http`, one-boot migration, `FAH__` regression
test, deletions, tests, and the doc consequences listed in §15 (not edited by
this task's author without a yes).

## 2. Existing code paths

| Symbol | File | Role today |
| --- | --- | --- |
| `InterceptionConfig { clients, exclude_domains }` | `crates/fah-config/src/schema/https.rs:31-35` | TOML shape, `deny_unknown_fields`, `default`; two tests at the file's tail |
| `HttpsConfig.interception` | same file, line 14 | field of the `[https]` section |
| `Config::load` → `load_inner` | `crates/fah-config/src/lib.rs:27-60` | defaults < file < `FAH__`, validates; first boot writes `Config::default().to_toml_string()`. Returns the **effective** config — env overrides included — so it must never be the struct migration saves (§3.5, F1) |
| `Config::from_toml_str` | `lib.rs:64-66` | the file layer alone: no env, no `validate`; what migration re-reads and saves |
| `write_atomic` (private) | `lib.rs:94-112` | tmp-then-rename; no `fsync`; error-context closure `at(path)` evaluated after `fs::rename` returns |
| `Config::save` | `lib.rs:85` | whole-struct `toml::to_string_pretty` through `write_atomic` |
| `apply_one` | `crates/fah-config/src/env.rs:34-128` | hand-written env arms; `_ => Err(UnknownEnvKey)`; no array coercion |
| `BOOT_KEYS` (`"https"` whole section), `ConfigStore::apply_patch`, `merge`, `is_boot_key` | `crates/fah-api/src/config_store.rs` | classification, deep merge, validate → save → swap; test at 363-400 lists `https.interception.clients` as boot |
| `post_config` | `crates/fah-api/src/routes.rs:1384-1470` | rejects `rules.lists`, `policies`, `auth` on the patch before merging; publishes `Event::ConfigChanged` |
| `put_user_rules` | `routes.rs:1230+` | the whole-document `PUT` precedent (validate, dedupe, persist, return the stored document) |
| `apply_policies` / `republish_policies` | `routes.rs:1159-1183` | validate (compile) → persist via `apply_patch` → publish into `PolicyState` |
| `AppState`, `AppStateBuilder` | `crates/fah-api/src/state.rs` | shared handles |
| `ApiError`, `ErrorBody { error: ErrorDetail { code, message } }` | `crates/fah-api/src/error.rs` | envelope; `ValidationFailed` = 422, `Unavailable` = 503, `Internal` = 500; no `details` field today |
| `spawn_blocking` for `/config` I/O | `crates/fah-api/src/password.rs:324,337`, `certs.rs:217`, `main.rs:457,517,547` | the crate's rule for blocking filesystem work; `apply_patch`'s synchronous save is the outlier |
| one multi-thread runtime for DNS and API | `main.rs:243-254`, `dns.serve` 614, `ApiServer::bind` 584 | a blocked worker delays DNS query tasks (`main.rs:1118` says so for the memory scan) |
| `PolicyState { active: ArcSwap<ActivePolicies> }` | `crates/fah-rules/src/policy.rs:327-364` | the L2 live-swap precedent: `current()`, `publish()`, shared `Arc` with `fah-http` (`with_policies`) and `fah-api` |
| `ExclusionSet`, `BASELINE_EXCLUSIONS`, `InvalidExclusion` | `crates/fah-http/src/exclusions.rs` | seeded set; `contains` walks labels; tests 122-200 |
| `sni::normalize`, `MAX_NAME_LEN`, `MAX_LABEL_LEN` | `crates/fah-http/src/sni.rs:266-300` | the strict hostname validator both `ExclusionSet::new` and the ClientHello parser use |
| `Interception { server_config, client_config, store, clients, exclusions }`, `is_empty`, `intercepts`, `excludes` | `crates/fah-http/src/intercept.rs:47-83` | the machinery plus the two lists |
| `TlsProxy.interception: Option<Interception>`, `with_interception` (drops an empty one), `intercepts`, `interception_for` | `crates/fah-http/src/https.rs:43,90-104` | per-connection decision after the SNI verdict (`judge` at 170, `interception_for` at 183) |
| `interception()` | `crates/fastadhunter/src/main.rs:842-895` | parses lists, returns `None` when `clients` is empty **before** looking at the store; warns when the store did not open or has no CA |
| `tls_proxy = if https.is_some() {…}` | `main.rs:526-538` | proxy only when `engine.mode` has the HTTPS listener |
| `certs = CertStore::open(config_dir)` | `main.rs:515-525` | `Option<Arc<CertStore>>`; `None` = "store did not open" |
| `AppStateBuilder { … config: ConfigStore::new(config, path) … }` | `main.rs:588-603` | API state, config moved in here |
| `privilege::drop_to_service_user` | `main.rs:445` | everything that writes `/config` runs after this |
| `layering.rs` | `crates/fastadhunter/tests/layering.rs` | L1 `fah-model fah-config fah-common fah-logging`; L2 `fah-certs fah-rules`; L3 siblings; reads `dev-dependencies` too |
| Harness: `Setup { clients, exclusions, … }`, `harness()`, `empty_interception`, `listed()`, tests at 768-793, 1361-1420, 1825 | `crates/fah-http/tests/interception.rs` | every `Interception::new` call site outside the binary |
| Dashboard envelope | `dashboard/frontend/src/api/core.ts:7-48` | `ErrorEnvelope { error: { code, message } }`, `ApiError { code, status, retryAfter }` — consumed by p3-09 |
| Toolchain | `rust-toolchain.toml` 1.96.0 | `Mutex::clear_poison` (stable since 1.77) available |

`fah-api` already depends on `fah-common`, `fah-config`, `fah-model`,
`fah-rules`, `fah-certs`, `arc-swap`, `serde_json`. `fah-rules` depends on
`fah-common`, `fah-config`, `fah-model`, `arc-swap`. `fah-http` depends on
`fah-rules`. Nothing in `fah-model`, `fah-api` telemetry or the dashboard
references the two lists today (grep: none).

## 3. Design

### 3.1 Types and their homes (layering decides) — frozen: `fah-rules::interception`

The `PUT` handler is in `fah-api`; the value the hot path reads is in
`fah-http`; siblings never import each other. The tree's precedent for exactly
this shape is `PolicyState`: an L2 holder with an `ArcSwap` inside, shared as
one `Arc` with both siblings by the binary. This plan mirrors it:

| Type | Home | Layer | Why here |
| --- | --- | --- | --- |
| `InterceptionDocument { clients: Vec<String>, exclude_domains: Vec<String> }` | `crates/fah-model/src/interception.rs` (new) | L1 | pure data with serde; the wire and file shape |
| `normalize_host`, `MAX_NAME_LEN`, `MAX_LABEL_LEN` | `crates/fah-rules/src/interception.rs` (new), moved verbatim from `fah-http/src/sni.rs` | L2 | one validator for the document and the ClientHello (principle 4) |
| `ExclusionSet`, `InvalidExclusion` | same module, moved from `fah-http/src/exclusions.rs` minus the baseline | L2 | a matcher; `fah-rules` is the matcher crate |
| `InterceptionScope { clients: Box<[AllowedNet]>, exclusions: ExclusionSet }` | same module | L2 | the compiled document the hot path reads |
| `Active { document: InterceptionDocument, scope: InterceptionScope }` | same module | L2 | the one value that is published: what the operator sent and what the hot path reads, in one allocation |
| `InterceptionState { active: ArcSwap<Active> }` | same module | L2 | the holder, mirror of `PolicyState { active: ArcSwap<ActivePolicies> }` |
| `Active::compile(InterceptionDocument) -> Result<Active, DocumentError>` (the one entry point), `MAX_CLIENTS = 256`, `MAX_EXCLUDE_DOMAINS = 512`, `DocumentError` (`Serialize`) | same module | L2 | validation is logic, not data; the error is the API's structured contract |
| `InterceptionStore`, `load_or_migrate`, `InterceptionRuntime`, `InterceptionStoreError` | `crates/fah-api/src/interception_store.rs` (new) | L3 | mirror of `ConfigStore` without a second copy of the value: the file, the handle, the commit lock |
| `Interception { server_config, client_config, store, state: Arc<InterceptionState> }` | `crates/fah-http/src/intercept.rs` | L3 | machinery stays; the lists leave |

The port-trait alternative (`ports.rs` + a binary adapter) is not chosen: the
adapter would still need to build `fah-http` state from strings, so validation
would split across layers and duplicate the normaliser. With the compiled type
in L2, `fah-api` validates, builds, persists and publishes with no port, and
the binary only wires one `Arc`. `fah-api` never names `fah-http`.

### 3.2 The hot path after the change

```text
serve_connection → judge (SNI verdict) → approved_address
  → interception_for(ip, host):
        let interception = self.interception.as_ref()?;        // machinery
        let active = interception.state.load();                 // ArcSwap guard: no lock, no allocation
        (active.scope.intercepts(ip) && !active.scope.excludes(host)).then_some(interception)
  → intercept(interception, …)                                  // reads server/client config, store — never the state
```

`load()` derefs to one `Arc<Active>`; `.scope` is a field offset, not a second
pointer chase. The decision is taken once per accepted connection and the
guard is dropped before `intercept()` runs. Nothing inside a session re-reads
the state, so an in-flight session keeps the lists it was admitted under by
construction — not by holding a reference.

### 3.3 The apply path — frozen: prepare async-side, commit on the blocking pool

```text
PUT /api/v1/interception
  handler (async, API worker thread) — preparation, everything fallible or allocating:
    1. serde_json::from_value::<InterceptionDocument>(body)   → shape error → 422 { details: { reason: "shape" } }
    2. active = Active::compile(document)                      → DocumentError → 422 { details }; nothing touched
    3. runtime check: StoreClosed && active.scope.client_count() > 0 → 503; nothing touched
    4. text = serde_json::to_string_pretty(&active.document) + "\n"   // serialisation and its allocation, before the lock
    5. next = Arc::new(active)                                 // the only allocation of the published value, before the lock
    6. spawn_blocking(move || store.commit(next, text)).await
         commit (blocking pool thread; the critical section):
           a. guard = commit_lock.lock() — recovered per §3.7 if poisoned
           b. write_atomic(path, &text)                         → Err(Write) → return; nothing published
           c. state.active.store(next)                          → one pointer swap; infallible
           d. drop(guard)
           e. info!(clients, exclusions, "interception document replaced")
       Ok(next) → 200 with next.document; Err(Write) → 500; JoinError (panic) → §3.7
```

Why the blocking pool, concretely: `write_atomic` opens, writes and closes a
file up to ~130 KiB, then renames it, on the `/config` volume — on the RB5009 a
flash-backed mount where a write can take tens of milliseconds. The API shares
the one multi-thread runtime with the DNS pipeline (`main.rs:243`, `dns.serve`
614), so a worker stalled for that long delays every query task queued on it.
Every other `/config` write in `fah-api` already hops to the blocking pool
(`password.rs:324,337`, `certs.rs:217`), as does `CertStore::open` in the
binary; `ConfigStore::apply_patch`'s synchronous save is the exception and is
recorded as a follow-up (§16), not copied.

Why persist and publish share one blocking unit: axum drops a handler future
when the client goes away, but a `spawn_blocking` task runs to completion.
Keeping b–c inside `commit` means a cancelled `PUT` can never leave the file
ahead of the runtime for the process lifetime. The mutex is a
`std::sync::Mutex` locked **only** on the blocking pool — a tokio mutex held
across the `.await` would release on cancellation and let two commits
interleave (A persists, B persists, B publishes, A publishes → file B, runtime
A). Contention parks a blocking-pool thread, never an executor worker; `PUT`s
are operator actions, rare by nature.

Why preparation is async-side: `Active::compile` is CPU-only — at most 768 entries,
one `Box<str>` per host, two `HashSet` builds — well under a millisecond, and a
validation error then costs no pool hop. The serialisation (4) and the
`Arc::new` (5) are moved out of the critical section so that the section
contains no allocation of its own except the ones inside `write_atomic`, all
of which precede the rename (§3.7).

Validate, build, persist, publish — ADR §Atomic swap. Build cannot fail once
validation passed because `Active::compile` is both. There is no separate `current`:
`GET` reads `state.current().document`, so the document the API shows and the
scope the hot path reads are one `Arc`, published by one instruction.

### 3.4 Machinery exists whenever the listener runs and the store opened

`interception()` in `main.rs` stops looking at the client list. It returns
`Some(Interception)` whenever `certs` is `Some`, with the state built from the
loaded document — empty or not. `TlsProxy::with_interception` stops dropping
an empty one. The bootstrap `None → Some(..)` problem therefore does not
exist: adding the first client is a store of a new `Active`, not a machinery
build.

Runtime status handed to the store:

| `engine.mode` has HTTPS listener | `CertStore::open` | `InterceptionRuntime` | `PUT` listing a client |
| --- | --- | --- | --- |
| yes | `Some` | `Live` | accepted, applies next connection |
| yes | `None` | `StoreClosed` | 503 `unavailable`: "the certificate store did not open; repair /config and restart" |
| no | any | `NoListener` | accepted and stored; inert until a mode with the listener boots (the response says nothing special; the dashboard says it, p3-09) |

`clients: []` is accepted under every status.

### 3.5 Migration (release N)

`InterceptionConfig` fields become `Option<Vec<String>>`, serialised only when
present, and the whole `interception` field of `HttpsConfig` is skipped when
both are `None`, so a saved TOML carries neither the keys nor an empty
`[https.interception]` table. `load_or_migrate` runs once per boot, after the
privilege drop and before the proxy and the `ConfigStore` are built.

**Two configs, one rule (F1).** The `Config` the binary holds is the
*effective* one — `defaults < file < FAH__` (`load_inner`). Saving it would
bake every environment override of that boot into `fastadhunter.toml`, and
removing the variable later would no longer restore the default. Migration
therefore never saves the effective config. It re-reads the **file layer** —
`Config::from_toml_str(fs::read_to_string(config_path))`, no env, no
`validate` — takes the legacy keys from *that*, clears them there, and saves
*that* struct. The effective config gets the same two fields cleared in
memory, and nothing else, so `ConfigStore` and `GET /api/v1/config` agree
with the file on the two keys while the env layer keeps winning for
everything it sets. No env arm exists for either key (§13), so the file layer
and the effective config always agree on their values.

```text
file    = Config::from_toml_str(read_to_string(config_path)?)?   // file layer only; unreadable/unparseable → boot fails naming the TOML
lists   = file.https.interception.take()                          // legacy keys, from the file
carried = lists.clients.is_some() || lists.exclude_domains.is_some()
config.https.interception = InterceptionConfig::default()         // effective config: both None in memory, nothing else touched
match read(/config/interception.json)
  Ok(text)        → document = parse(text)?                      // malformed → boot fails naming the file
                    if carried { warn!(document, config, "[https.interception] ignored; interception.json is the source of truth") }
  NotFound        → document = { clients: lists.clients.unwrap_or_default(),
                                 exclude_domains: lists.exclude_domains.unwrap_or_default() }
                    active = Active::compile(document)?           // invalid TOML values → boot fails naming the list, file untouched
                    write_atomic(interception.json, pretty(active.document))?   // unwritable /config → boot fails, as first-boot generation does
                    info!("migrated [https.interception] into interception.json")
  Err(other)      → boot fails naming the file                    // permission denied etc. — never falls through to a write
active = Active::compile(document)?                               // existing document over cap or invalid → boot fails naming the file
if carried { file.save(config_path)? }                            // the FILE-LAYER struct: TOML now carries neither key, env values not written
```

Properties: the document is written on exactly one branch, `NotFound`, so an
existing file — readable or not — is never overwritten; it is never rewritten
at boot; the TOML is rewritten only on a boot that found a key in it, and
then from the file layer, so a value that came from `FAH__` is never written
to disk by migration; a crash between the two writes is healed by the next
boot (document present, keys still carried → warn, save). A fresh install
(first-boot generation writes no keys) takes the `NotFound` branch with empty
lists and never touches the TOML. `Config::load` has already parsed the same
file moments earlier, so the re-read cannot fail for a reason boot would not
already have failed on; it costs one read of a file under 16 KiB, once per
boot.

`ConfigStore::apply_patch` still saves the effective config (`config_store.rs`
`candidate.save`), so an operator's `POST /api/v1/config` bakes env values
today. Pre-existing, operator-triggered, and out of scope (§16); migration is
held to the stricter rule because it runs without anyone asking.

### 3.6 Structured error contract — frozen

The envelope stays `{ "error": { "code", "message" } }` and gains an optional
`details` object, present on the interception route's 422s and absent
everywhere else (the field is `skip_serializing_if = None`, so no existing
consumer sees a change). `code` remains the stable machine code the API
already has; `details.reason` is the second-level machine code; `message`
stays human-readable and is never parsed by a client.

```json
{ "error": { "code": "validation_failed",
             "message": "exclude_domains[7]: \"Bank.ro.\" duplicates entry 2 after normalization",
             "details": { "reason": "duplicate", "list": "exclude_domains", "index": 7, "entry": "Bank.ro.", "duplicate_of": 2 } } }
{ "error": { "code": "validation_failed", "message": "clients: 300 entries exceed the cap of 256 by 44",
             "details": { "reason": "over_cap", "list": "clients", "len": 300, "cap": 256 } } }
{ "error": { "code": "validation_failed", "message": "clients[3]: \"10.0.0.300\" is not an IP address or CIDR block",
             "details": { "reason": "invalid_entry", "list": "clients", "index": 3, "entry": "10.0.0.300" } } }
{ "error": { "code": "validation_failed", "message": "unknown field `client`, expected `clients` or `exclude_domains`",
             "details": { "reason": "shape" } } }
```

`details` is `serde_json::to_value(&DocumentError)` — `DocumentError` derives
`Serialize` with `#[serde(tag = "reason", rename_all = "snake_case")]`, so the
contract has one source of truth and no hand mapping. `index` and
`duplicate_of` are 0-based positions in the list **as sent**. `list` is
`"clients"` or `"exclude_domains"`. `reason` is closed: `over_cap`,
`invalid_entry`, `duplicate`, `shape`.

### 3.7 Panic and poisoning semantics — frozen (owner review 2, 2026-09-10)

The vetoed semantic — "a panic inside `commit` poisons the lock; later commits
answer 500 until a restart" — guarded a state that is provably consistent. It
is replaced by the following, with the proof recorded here so the invariant is
reviewable.

**The critical section.** With preparation moved out (§3.3 steps 4–5), the
section under `commit_lock` is exactly:

```text
a. lock                       (std::sync::Mutex<()>; recovered if poisoned — below)
b. write_atomic(path, &text)  create_dir_all · fs::write(tmp) · fs::rename(tmp, path)   → io::Result
c. state.active.store(next)   one atomic pointer swap; the old Arc<Active> is dropped after readers drain
d. unlock
```

`info!` runs after d. **Nothing may be inserted between `fs::rename` and
`store`** — this is a code-review invariant, restated in A5 and C2, and the
reason `write_atomic` hoists its error-context closures before the rename.

**What can panic in a–d, in normal Rust operation:** nothing.

- a. `Mutex::lock` returns `Err(PoisonError)`; it panics only if unwrapped, and
  it is not unwrapped.
- b. `create_dir_all`, `fs::write`, `fs::rename` return `io::Result`;
  `file_name().unwrap_or_default()` and `with_file_name` have no panic path;
  `format!` and `to_path_buf()` allocate. Allocation failure **aborts** the
  process (Rust's default `handle_alloc_error`); it does not unwind and cannot
  poison a lock.
- c. `ArcSwap::store` swaps a pointer, waits for readers, drops the old
  `Arc<Active>` — `Vec<String>`, `HashSet<Box<str>>`, `Box<[AllowedNet]>`,
  no user `Drop`. No panic path.
- d. `MutexGuard::drop` has no panic path.

A poison can therefore arise only from a future bug, or from code someone
later inserts inside the section. The policy below is designed for that case.

**State at every hypothetical unwind point.**

| Unwind point | `interception.json` | active `Arc<Active>` | Consistent? |
| --- | --- | --- | --- |
| before `fs::rename` completes | A (a stray `interception.json.tmp.<pid>` may exist) | A | yes |
| between `fs::rename` and `store` | B | A | **no** — but this window contains one instruction with no allocation, no `?`, no call that can unwind; only process death can land here, and the next boot reads the file |
| after `store` | B | B | yes |

**Structure guarantee.** All fallible and allocating preparation happens before
the lock (§3.3 steps 1–5). Inside the section the only fallible operation is
`write_atomic`, which returns before `store` on any error, and the only
transition is `store`, which cannot fail. `write_atomic` is amended (A5) so
that after `fs::rename` returns `Ok` the remaining path is literally `Ok(())`:
its error-context closures (`at(&tmp)`, `at(path)` — each a `to_path_buf()`)
are created before the write and the rename, not evaluated after.

**Recovery policy.** `commit` acquires the lock with
`lock().unwrap_or_else(PoisonError::into_inner)`, then, if the lock was
poisoned, calls `clear_poison()` and logs at `error!` that a previous commit
panicked and the lock was recovered. The panic payload itself is logged at
`error!` by the handler at the time it happens, from `JoinError::into_panic()`,
and that `PUT` answers 500 `internal` ("the commit panicked; nothing was
applied or the change was fully applied — see the log"). No long-lived API
state: the next `PUT` proceeds normally.

Why `into_inner()` is correct here: poisoning signals "an invariant guarded by
this lock may be broken by an unwind". The lock guards *serialisation*; the
*data* invariant (file and active agree, or file is one commit ahead only
across process death) holds at every instruction boundary of the section
except the single one that cannot unwind. The recovered guard is a real guard,
so serialisation is preserved; it carries no data, so nothing is "observed".

Why not process-fatal: a panic in `commit` is already isolated by
`spawn_blocking` into a `JoinError`; aborting would take the household's DNS
resolver down for a bug on an operator-only path whose state is provably
consistent. Fail loudly (the `error!` lines, the 500), not fatally.

**Proof that no forbidden state can arise under this policy.**

- *Persisted B + active A, process alive:* requires an unwind after
  `fs::rename` and before `store`. That window has no unwinding operation
  (above), so it is reachable only by process death; the next boot reads the
  file. After a recovered lock, the next commit C ends with persisted C and
  active C.
- *Persisted A + active B:* requires `store` before a successful `rename`.
  Ordering forbids it: `write_atomic`'s `Err` returns before `store`, and
  `store` has no failure path after which the file would be reverted.
- *Partially written `interception.json`:* the final path is written only by
  `fs::rename` — an atomic replace in the same directory on the same
  filesystem. Partial content can exist only under the tmp name, which is
  never read and is truncated by the next `fs::write`.
- *A new commit observing or publishing unknown state:* a commit never reads
  the current state — whole-document replace, no read-modify-write; it
  publishes exactly the `next` prepared and validated before the lock. The
  recovered guard is `()`. `GET` and the hot path only ever load a fully
  constructed `Arc<Active>` published by one instruction.
- *Serialisation:* both `rename` and `store` happen under one guard, recovered
  or not; two commits cannot interleave.
- *Failed normal persistence:* `write_atomic` `Err` → return before `store`;
  `next` is dropped; the active `Arc<Active>` is untouched and remains valid.

**Existing limitations, kept and documented, no scope change (owner decision).**

- `write_atomic` does not `fsync`. On power loss some filesystems can leave a
  zero-length file at the final path after the rename. `fastadhunter.toml`
  has the same exposure today. Boot then fails naming the file (§3.5), which is
  the ADR's contract.
- A crash leaves `interception.json.tmp.<pid>`; the pid changes across boots,
  so stale tmp files can accumulate. Pre-existing for the TOML. Harmless:
  never read.

## 4. File-by-file changes

Each step: target · change · why · invariants · tests. Steps are ordered so the
workspace compiles after each group (A: L1/L2 types; B: `fah-http`; C:
`fah-api`; D: binary; E: deletions and docs).

### A — L1 / L2

**A1 · `crates/fah-model/src/interception.rs` (new) + `lib.rs` export**
`InterceptionDocument` with `#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]`,
`#[serde(deny_unknown_fields, default)]`. No methods beyond derives. Why: the
JSON file and the wire shape are one type; `deny_unknown_fields` is the "unknown
keys rejected" contract. Invariant: hard rule 2 — no logic here. Tests:
`fah-model` round-trip (`{}` parses as empty; `{"clients":[],"extra":1}` fails).

**A2 · `crates/fah-common/src/egress.rs` `AllowedNet`** — add `Hash` to the
derive list (frozen). Consistent with the derived `Eq` over `{ addr, prefix_len }`.
Why: duplicate detection in `Active::compile` is a `HashSet<AllowedNet>` insert —
O(n), never O(n²). "Duplicate" means equal after parse, exactly as `Eq`
defines it; a host inside a listed CIDR is not a duplicate. Test: existing
`AllowedNet` tests green; `Active::compile` tests §14.1.

**A3 · `crates/fah-rules/src/interception.rs` (new) + `lib.rs` exports**
- `pub const MAX_NAME_LEN: usize = 253; pub const MAX_LABEL_LEN: usize = 63;`
  and `pub fn normalize_host(raw: &[u8]) -> Option<Box<str>>` moved verbatim from
  `fah-http/src/sni.rs:266-300` (and the two consts).
- `ExclusionSet` and `InvalidExclusion` moved from `fah-http/src/exclusions.rs`:
  `new` no longer seeds anything; `empty()` deleted (`Default` is the empty
  set); `len`, `is_empty`, `contains` unchanged; the private `normalize`
  (`Cow`) stays with it.
- `pub const MAX_CLIENTS: usize = 256; pub const MAX_EXCLUDE_DOMAINS: usize = 512;`
- `pub struct InterceptionScope { clients: Box<[AllowedNet]>, exclusions: ExclusionSet }`
  with `intercepts(&self, ip) -> bool` (linear `any`, moved from
  `Interception::intercepts`), `excludes(&self, host) -> bool`,
  `client_count()`, `exclusion_count()`, `Default`.
- `pub struct Active { pub document: InterceptionDocument, pub scope: InterceptionScope }`
  with `Default` (empty document, empty scope) and
  `Active::compile(document: InterceptionDocument) -> Result<Active, DocumentError>`
  — **the only compile entry point** (F9). It validates and builds the scope,
  then moves the document in beside it; every caller (`prepare`,
  `load_or_migrate`, the harnesses) wants the published pair, so no free
  `compile(&doc) -> InterceptionScope` exists. `InterceptionScope` has no
  public constructor other than `Default`.
- `#[derive(Debug, Clone, PartialEq, Eq, Serialize)] #[serde(tag = "reason", rename_all = "snake_case")]
  pub enum DocumentError { OverCap { list: &'static str, len: usize, cap: usize },
  InvalidEntry { list: &'static str, index: usize, entry: String },
  Duplicate { list: &'static str, index: usize, entry: String, duplicate_of: usize } }`
  with `Display` producing the messages of §3.6. **`fah-rules` has no `serde`
  dependency today** (`crates/fah-rules/Cargo.toml`: arc-swap, fah-common,
  fah-config, fah-model, memchr, reqwest, thiserror, tokio, tracing), so
  `Cargo.toml` gains `serde = { workspace = true }` — the workspace entry
  already carries `features = ["derive"]`. External crate, already in the
  build graph through `fah-model`; no layering change (F2).
- `Active::compile` body: caps first (both lists), then `clients` parsed with `AllowedNet::from_str`
  (the parser `interception()` in `main.rs` uses today) after `trim()` — a
  leniency the boot parser did not have (review F-03, 2026-09-10): `" 10.0.0.1"`
  is accepted, stored as sent, and deduplicated against `"10.0.0.1"` — and
  deduplicated by `HashSet<AllowedNet>` insert, then `exclude_domains`
  normalised with `normalize_host` after `trim().trim_end_matches('.')` (as
  `ExclusionSet::new` does today) and deduplicated by `HashSet<Box<str>>`
  insert. The stored document keeps the operator's spelling; only the compiled
  set is normalised. `ExclusionSet::new` and `InvalidExclusion` were deleted
  after the review (F-04): `Active::compile` is the one validator and builds
  the set directly.
- `pub struct InterceptionState { active: ArcSwap<Active> }` with
  `new(active: Active)`, `load(&self) -> arc_swap::Guard<Arc<Active>>` (hot
  path), `current(&self) -> Arc<Active>`, `store(&self, next: Arc<Active>)`.
  Mirror of `PolicyState`; `Debug` impl prints counts only. `store` takes the
  already-built `Arc` so the critical section allocates nothing for it.
Why: one home for validator, matcher, compiled form, published value, holder
and error contract, reachable from both L3 siblings. Invariants: `contains`
cost is two to four probes whatever the size; `load` takes no lock and
allocates nothing; `store` is one atomic swap. Tests: §14.1.

**A4 · `crates/fah-config/src/schema/https.rs` `InterceptionConfig`**
```text
#[serde(deny_unknown_fields, default)]
pub struct InterceptionConfig {
    #[serde(skip_serializing_if = "Option::is_none")] pub clients: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub exclude_domains: Option<Vec<String>>,
}
impl InterceptionConfig { pub fn is_absent(&self) -> bool; pub fn take(&mut self) -> Self }
```
and on `HttpsConfig`: `#[serde(default, skip_serializing_if = "InterceptionConfig::is_absent")] pub interception: InterceptionConfig`.
Why: presence must be observable for migration and absence must serialise to
nothing, or release N+1's deletion breaks every file. Invariants: an old TOML
with `clients = []` still parses (`Some(vec![])`); `Config::default()` serialises
without the table; `deny_unknown_fields` still rejects typos. Tests: §14.2.

**A5 · `crates/fah-config/src/lib.rs` `write_atomic`** — becomes
`pub fn write_atomic(path: &Path, text: &str) -> Result<(), ConfigError>` and is
exported; and its two error-context closures are created before the write:
```text
let tmp_err = at(&tmp); let path_err = at(path);          // the two to_path_buf() allocations, up front
fs::write(&tmp, text).map_err(tmp_err)?;
fs::rename(&tmp, path).map_err(path_err)                  // after a successful rename: nothing but Ok(())
```
Why: one atomic writer for `/config` files, and §3.7's invariant that no
operation of any kind follows a successful rename inside the critical section.
Behaviour on every error path is unchanged (same messages, same paths). Tests:
the existing `fah-config` save/load tests; §14.3 exercises it through the
store.

### B — `fah-http`

**B1 · `crates/fah-http/src/sni.rs`** — delete `normalize`, `MAX_NAME_LEN`,
`MAX_LABEL_LEN`; `use fah_rules::interception::{normalize_host, MAX_NAME_LEN}`;
the parser's `[0u8; MAX_NAME_LEN]` and the call at line 143 use the imports.
Invariant: byte-identical behaviour; the `sni.rs` tests are unchanged and
green. `lto = true` and `codegen-units = 1` in the release profile keep the
call inlinable; it is one call per ClientHello that already allocates the name.

**B2 · `crates/fah-http/src/exclusions.rs`** — deleted. **`lib.rs`** drops
`pub use exclusions::{…}` and `mod exclusions`; tests import
`fah_rules::interception::ExclusionSet` instead.

**B3 · `crates/fah-http/src/intercept.rs` `Interception`**
```text
pub struct Interception { server_config, client_config, store, state: Arc<InterceptionState> }
pub fn new(server_config, client_config, store, state: Arc<InterceptionState>) -> Self
pub fn state(&self) -> &Arc<InterceptionState>
```
`is_empty`, `intercepts`, `excludes` removed from `Interception` (they live on
`InterceptionScope`). Why: the machinery is immutable for the process life; the
lists are not. Invariant: `intercept()` never touches `state` — the accept arm
p3-08 edits must keep it that way.
Call sites of the old shape that this change breaks, all in this crate and
all covered by `clippy --all-targets` (F7):
- `intercept.rs:538` unit test `an_empty_client_list_intercepts_nobody` —
  uses `ExclusionSet::empty()`; deleted (its two assertions are
  `the_empty_set_matches_nothing` and the wire test of §14.5).
- `benches/intercept.rs:140-146` `interception(store, root)` — the 5-arg
  `Interception::new` with `ExclusionSet::empty()`; becomes
  `Interception::new(server, client, store, Arc::new(InterceptionState::new(Active::compile(listed_document()).unwrap())))`.
- `tests/interception.rs` harness — §14.5.

**B4 · `crates/fah-http/src/https.rs`**
- `with_interception(mut self, interception)` → `self.interception = Some(interception)`.
- `intercepts(&self, ip)` → `self.interception.as_ref().is_some_and(|i| i.state().load().scope.intercepts(ip))`.
- `interception_for(&self, ip, host)` per §3.2.
Invariants: one `load()` per accepted connection, guard dropped before
`intercept()`; no other reader of `state` in the crate.

**B5 · `crates/fah-http/Cargo.toml`** — no new dependency (`arc-swap` is
reached through `fah-rules`' public type; `Guard` is used by type inference
only). If a direct `arc_swap::Guard` name is needed, add the workspace
`arc-swap` — an external crate, not a layering change.

### C — `fah-api`

**C1 · `crates/fah-api/src/error.rs`** — `ErrorDetail` gains
`#[serde(skip_serializing_if = "Option::is_none")] details: Option<serde_json::Value>`;
new variant `ApiError::ValidationFailedWithDetails { message: String, details: serde_json::Value }`
→ 422, code `validation_failed`, `details` emitted. Every existing variant
serialises byte-identically (field absent). Tests: envelope with and without
`details`.

**C2 · `crates/fah-api/src/interception_store.rs` (new) + `lib.rs` exports**
- `pub const DOCUMENT_FILE: &str = "interception.json";`
- `#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum InterceptionRuntime { Live, NoListener, StoreClosed }`
- `pub struct Loaded { pub active: Active, pub migrated: bool }`
- `pub fn load_or_migrate(config_dir: &Path, config: &mut Config, config_path: &Path) -> Result<Loaded, InterceptionStoreError>`
  per §3.5 (synchronous; called once at boot before any listener exists).
  `config` is the effective config and is only ever *cleared* here — the
  struct that is saved is the file layer re-read from `config_path` inside
  the function (F1). `InterceptionStoreError` gains
  `Toml { path, source: ConfigError }` for the re-read/save of
  `fastadhunter.toml`, distinct from `Read`/`Parse`/`Write` on the document.
- `pub struct InterceptionStore { path: PathBuf, state: Arc<InterceptionState>, runtime: InterceptionRuntime, commit_lock: std::sync::Mutex<()> }`
  — no `current`: the document the API shows is `state.current().document`.
  Methods: `new(state, path, runtime)`, `current() -> Arc<Active>` (lock-free),
  `state() -> &Arc<InterceptionState>`, `runtime()`,
  `prepare(&self, document: InterceptionDocument) -> Result<Prepared, InterceptionStoreError>`
  (`Active::compile`, runtime check, `text`, `Arc::new(active)` — §3.3 steps
  2–5; pure apart from allocation), and
  `commit(&self, prepared: Prepared) -> Result<Arc<Active>, InterceptionStoreError>`
  (blocking; §3.7 a–d then `info!`; documented as "call from `spawn_blocking` only").
  `commit` acquires the lock with `unwrap_or_else(PoisonError::into_inner)`;
  when `commit_lock.is_poisoned()` was true it calls `clear_poison()` and logs
  `error!("a previous interception commit panicked; lock recovered — file and active state are consistent by construction")`.
- **Test-only panic hook (F3, gate corrected 2026-09-10).**
  `#[cfg(any(test, feature = "test-harness"))] panic_after_lock: AtomicBool`
  on `InterceptionStore`. When set, `commit` panics immediately after step a
  (lock acquired) and before step b (`write_atomic`) — the one place a
  hypothetical bug could unwind while holding the lock with the file still
  old. It is the only way to drive the panic → `JoinError` → 500 path,
  because the recovery policy makes a *poisoned* lock succeed, not fail
  (§14.3, §14.4). The `cfg(test)`-only gate this plan first specified does not
  work: `#[cfg(test)]` applies to the crate compiled as its own test binary,
  and `crates/fah-api/tests/api.rs` is a separate integration crate that links
  the library built **without** `cfg(test)`, so the field would not exist
  there. `crates/fah-api/src/routes.rs`'s `#[cfg(test)] mod tests` holds pure
  functions only and builds no `AppState`, so there is no in-crate route-test
  surface to fall back on. `test-harness` is therefore the gate: `fah-api`
  already enables it for its own integration tests through the self
  dev-dependency `fah-api = { path = ".", features = ["test-harness"] }`, and
  `crates/fah-api/src/lib.rs` already fails the build with `compile_error!`
  when it is on in a release profile, so the feature cannot reach a shipped
  binary. The hook is one `AtomicBool`, default `false`, read at exactly one
  point inside `commit`: no production behaviour, no production code path, no
  new feature surface beyond the gate that already exists. Keeping it is what
  proves the real handler → `JoinError` → 500 route (§14.4) rather than only
  the store-level unwind (§14.3).
- `pub enum InterceptionStoreError { Invalid(DocumentError), Unavailable(&'static str), Read { path, source: io::Error }, Parse { path, source: serde_json::Error }, Write { path, source: ConfigError } }`.
- File text: `serde_json::to_string_pretty` plus a trailing newline.
Why: mirror of `ConfigStore` with a single published value, validation before
any side effect, and the commit as one uncancellable, unwind-free unit.
Invariants: `prepare` has no side effects; `commit` touches nothing on a
`Write` error except the tmp file it leaves; **nothing between `rename` and
`store`**; `commit_lock` is never locked on an executor thread. Tests: §14.3.

**C3 · `crates/fah-api/src/state.rs`** — `AppState.interception: Arc<InterceptionStore>`
and the builder field. No tokio mutex for it (§3.3 explains why the lock
lives inside `commit`).

**C4 · `crates/fah-api/src/routes.rs`**
- `.route("/interception", get(get_interception).put(put_interception))`.
- `get_interception` → `Json(state.interception.current().document.clone())`.
- `put_interception(State, body: Result<Json<serde_json::Value>, JsonRejection>)`
  (F4): the body goes through the crate's existing rejection mapping —
  `certs.rs:229 body_error` (moved to `error.rs` or re-exported, one copy) —
  so a syntax error, a wrong `Content-Type` or an unreadable body answers
  **400 `bad_request`** *inside the envelope*, as `/certificates`, `login`
  and `password` do (`routes.rs:1510,1552`), not axum's plain-text default
  that `post_config` still emits. Then
  `serde_json::from_value::<InterceptionDocument>(body)` → on error
  `ValidationFailedWithDetails { message: serde's, details: {"reason":"shape"} }`;
  `prepare` → `Invalid(e)` → `ValidationFailedWithDetails { message: e.to_string(), details: to_value(&e) }`,
  `Unavailable` → `Unavailable { retry_after: None }` (503);
  `spawn_blocking(move || store.commit(prepared)).await` →
  `Err(join_error)` (F5): **both branches**, never an unguarded `into_panic()`:
  `if join_error.is_panic()` → `error!(payload = ?join_error.into_panic(), "interception commit panicked")`
  then `Internal("the commit panicked; the change was either fully applied or not at all — see the log")`;
  `else` (the task was cancelled — only runtime shutdown does that) →
  `error!(error = %join_error, "interception commit did not run")` then
  `Internal("the commit did not run; nothing was applied — see the log")`.
  `Ok(Err(Write))` → `Internal` (500, message names the path and the I/O error).
  Returns `Json(active.document.clone())` — the stored document, never a
  `restart_required` field.
- `post_config`: a fourth early rejection, mirroring the `rules.lists` one:
  `patch.get("https").and_then(|h| h.get("interception")).is_some()` → 422
  `ValidationFailed("https.interception is not settable here: clients and
  exclude_domains live in /config/interception.json and are managed by
  GET/PUT /api/v1/interception, which applies live with no restart")`. A check
  on the patch, so it survives release N+1. Plain message, no `details` (no
  client parses it).
- No new `Event` on the hub. The dashboard re-reads after its own `PUT`
  (p3-09); a second tab is last-write-wins, as the ADR accepts.
Tests: §14.4.

**C5 · `crates/fah-api/src/config_store.rs`** — the classification test at
363-400 drops its `https.interception.clients` row (the key is no longer a
config key). `BOOT_KEYS` keeps `"https"` (listen, timeouts and `sni` stay
boot).

### D — binary

**D1 · `crates/fastadhunter/src/main.rs`**
- `Engine::start(config: Config, …)` → `let mut config = config;` at the top
  of the `/config`-writing block (after `privilege::drop_to_service_user`,
  line 445, beside `ApiKeyStore::load_or_create`):
  `let loaded = fah_api::load_or_migrate(config_dir, &mut config, config_path)?;`
  (error → the same `Box<dyn Error>` return every other boot failure takes;
  the message names the file or the list). `config` here is the effective
  config `main.rs:140` loaded; the function clears its two fields and saves
  the file layer it re-reads itself (§3.5, F1) — the binary passes nothing
  else and saves nothing. Runs inside the existing `spawn_blocking` region
  that already wraps the other `/config` work at boot, or its own — it is
  one-shot, before any listener.
  `let interception_state = Arc::new(InterceptionState::new(loaded.active));`
- `interception(config, certs, state: Arc<InterceptionState>)`: no client
  parsing, no `clients.is_empty()` early return. `certs == None` → warn as
  today (wording: "the certificate store did not open — listed clients are
  spliced, not intercepted, until /config is repaired and the container
  restarted") and return `None`. `!store.has_ca()` → warn only when
  `state.current().scope.client_count() > 0`, as today. Build `server_config`,
  `client_config`, return `Some(Interception::new(server, client, store, state))`.
  `info!(clients, exclusions, "HTTPS interception machinery ready — each listed client must hold a static lease")`.
- Runtime status per §3.4 computed from `https.is_some()` and `certs.is_some()`.
- `AppStateBuilder { interception: Arc::new(InterceptionStore::new(Arc::clone(&interception_state), config_dir.join(DOCUMENT_FILE), runtime)), … }`.
  `ConfigStore::new(config, …)` receives the config **after** migration
  cleared the fields, so `GET /api/v1/config` and the file agree.
- `healthcheck` untouched (it only loads the TOML).

**D2 · `crates/fastadhunter/tests`** — the one seeder is
`common/mod.rs:633 full_mode_config` (`[https.interception] clients = […]`
at line 685, driven by `FullMode.clients`), used by `e2e_https.rs` and
`security_phase3.rs`. `boot_full_in` writes `interception.json` beside the
TOML from `FullMode.clients` instead, and the TOML template loses the table;
one `e2e_https.rs` scenario drives `PUT /api/v1/interception` to prove the
live path end to end (§14.6). `history_e2e.rs:439` constructs
`AppStateBuilder` by struct literal and gains the `interception` field (F7).

### E — deletions and docs

**E1** `BASELINE_EXCLUSIONS` gone with `exclusions.rs`; `ExclusionSet::empty()`
gone (`Default`). **E2** tests per §14.5. **E3** docs per §15 — proposed, not
edited by this task without a yes.

## 5. Data and control flow

**Boot, before:** `Config::load` → … → `interception(&config, certs)`
parses the lists → `None` if empty → `TlsProxy::with_interception` drops an
empty one → `AppState` never sees the lists.

**Boot, after:** `Config::load` → privilege drop → `load_or_migrate` (file
read or written, TOML possibly cleared and saved) → `InterceptionState` →
`certs` → proxy with `Interception` whenever `https.is_some() && certs.is_some()`
→ `AppState.interception` (state, runtime, lock) → `ConfigStore` with the
cleared config.

**Change, before:** `POST /api/v1/config` → TOML → `restart_required: true` →
container restart → new `interception()`.

**Change, after:** `PUT /api/v1/interception` → §3.3 → the next
`serve_connection` loads the new `Active`.

**Connection, before and after:** identical up to `interception_for`; after,
the decision reads the scope through one guard (§3.2).

## 6. API contract

```text
GET /api/v1/interception
200 { "clients": ["192.168.88.10", "192.168.88.0/24"], "exclude_domains": ["bank.example"] }

PUT /api/v1/interception            body: the whole document; a missing key is an empty list (serde default)
200 the stored document (as sent, spelling preserved)
400 bad_request                     body is not JSON, wrong Content-Type, or unreadable — the envelope, via body_error (F4); no details
422 validation_failed + details     reason: shape | over_cap | invalid_entry | duplicate — §3.6
503 unavailable                     certificate store did not open and the document lists a client
500 internal                        the file could not be written, the commit task panicked (§3.7), or it was cancelled before running (shutdown); nothing half-applied
401                                 as every route
```

No `restart_required` in any response shape. `POST /api/v1/config` with a
`https.interception` key → 422 naming `/api/v1/interception` (no `details`).
`GET /api/v1/config` no longer contains `https.interception`. The `details`
object is new to the envelope and appears only on this route's 422s.

## 7. Persistence and migration — cases

| Boot state | Document | TOML | Result |
| --- | --- | --- | --- |
| upgrade from 0.3.x, keys present (empty or not) | absent | `Some` | document written from TOML values; TOML re-saved from the file layer without keys — a `FAH__` value in force at that boot is **not** written into the file (F1); one `info!` |
| same, with `FAH__API__PORT=9443` set | absent | `Some` | as above; the saved TOML still carries `port = 8443` (or whatever the file said), the running config still uses 9443 |
| fresh install | absent | absent | empty document written; TOML untouched |
| second boot of N | present | absent | document read; nothing written |
| hand re-added keys after migration | present | `Some` | document wins; `warn!` naming both files; TOML re-saved without keys |
| crash between document write and TOML save | present | `Some` | same as the row above — idempotent |
| document unreadable (permissions) | present | any | boot fails naming the file; **not** overwritten |
| document malformed / unknown key / over cap / invalid entry | present | any | boot fails naming `/config/interception.json`; not overwritten |
| zero-length document after power loss (no `fsync`, §3.7) | present | any | boot fails naming the file; not overwritten — existing limitation, same as the TOML |
| TOML values invalid or over cap | absent | `Some` | boot fails naming the list; document not written; TOML untouched |
| `/config` unwritable during migration | absent | any | boot fails with the I/O error, as first-boot generation does |

The TOML is rewritten from the file-layer struct (`Config::save`); hand-written
comments do not survive it, as with every API write today. Unlike an API
write, migration does not serialise the effective config (§3.5, F1).

## 8. Runtime lifecycle

- **Startup:** §3.5 then §3.4. Migration runs before the API and the listeners
  exist, so no request can race it.
- **First activation:** boot with `clients: []`; machinery exists; `PUT` with
  one client → `store` → that device's next connection is intercepted. No
  restart, no lazy build. This is mandatory, not optional: `with_interception`
  never drops the machinery, and §14.5 pins it.
- **Updates:** every `PUT` replaces the whole `Active`. Removing a client stops
  its next connection from being intercepted; its open session finishes as
  admitted. Adding an exclusion splices the next connection to that host.
- **Concurrent access:** commits serialised by `commit_lock` on the blocking
  pool; preparation concurrent; readers (`GET`, every accepted connection)
  lock-free through `ArcSwap`, and they read the same `Arc` — `GET` can never
  show a document the hot path is not using.
- **Shutdown / reload:** no task, no timer, no channel — the state is dropped
  with the proxy and the API. A commit in flight at shutdown completes on the
  blocking pool (tokio drains blocking tasks on runtime drop) — the file is
  either old or new, never torn. There is no reload path other than `PUT`.
- **Failure paths:** §3.3, §3.7 and §7. A `PUT` that fails leaves file and
  active as they were; a `PUT` whose commit panicked is either fully applied
  or not at all (§3.7), and the next `PUT` proceeds; a boot that fails names
  its cause.

## 9. Concurrency and ownership

- `InterceptionState: Send + Sync` (`ArcSwap<T>` is, `Active` holds only owned
  data). Shared as `Arc` by the proxy (through `Interception`) and the API
  store. `InterceptionStore: Send + Sync` (`Arc`, `std::sync::Mutex<()>`,
  `PathBuf`).
- `store` is one `ArcSwap::store`; a connection holding a `Guard` keeps the
  old `Arc<Active>` alive until the guard drops — no torn read, no lock, no
  wait.
- Races: two `PUT`s → both prepare, commits serialised, last commit wins on
  file and runtime together; `PUT` vs connection → the connection sees either
  the old or the new `Active`, never a mix; `GET` vs `PUT` → same `Arc` as the
  hot path, so `GET` and the runtime can never disagree.
- Stale state: none retained beyond a guard's lifetime; the old `Active` is
  freed when the last guard drops.
- Cancellation: the handler future may be dropped at any `.await`; the only
  side-effecting step is the `spawn_blocking` unit, which completes regardless.
  A response may be lost; the file/runtime pair cannot diverge.
- Panic: §3.7 — isolated by `spawn_blocking`, logged with its payload, lock
  recovered and cleared by the next commit; no persistent degraded state.
- Task lifetime / leaks: one blocking task per `PUT`, finite; no detached
  tasks, no channels, no timers.
- Blocking: file I/O only on the blocking pool; `commit_lock` only there;
  executor workers never block on this path.

## 10. Error propagation and failure atomicity

| Failure | Where | Effect |
| --- | --- | --- |
| not JSON / wrong content type / unreadable body | `JsonRejection` → `body_error` | 400 `bad_request` in the envelope; nothing touched |
| unknown key / wrong type | `from_value` in handler | 422 `shape`; nothing touched |
| cap / syntax / duplicate | `Active::compile` in `prepare` | 422 with `details`; nothing touched |
| store closed + client listed | `prepare` | 503; nothing touched |
| tmp write or rename fails | `write_atomic` inside `commit` | 500; file is the old one (rename is atomic); active unchanged; `next` dropped; tmp may remain |
| panic inside `commit` (a bug, §3.7) | `JoinError::is_panic()` | 500 with the payload in the log; state is A/A or B/B — never mixed; lock recovered and cleared by the next commit |
| blocking task cancelled before it ran (runtime shutdown) | `JoinError`, not a panic | 500, `into_panic()` never called; nothing touched |
| `store` | `ArcSwap::store` | cannot fail |

## 11. Hot-path performance and allocations

Per accepted connection, after the SNI verdict: one `ArcSwap::load` (a few
atomic operations, no allocation), a field read for `.scope`, the same linear
`AllowedNet` walk as today (now bounded at 256), the same label walk in
`contains` (`Cow`; the SNI is already lowercase from `scan_client_hello`, so no
allocation). No new work on the request path. No bench is owed unless the
plan's review finds the `load` measurable; the p3-06 TLS budget rows are the
reference if it is.

## 12. Memory

- `Active`: ≤ 256 × `AllowedNet` (24 B) + ≤ 512 normalised names ≤ 253 B each
  plus `HashSet` overhead, plus the document's own strings once — under
  260 KiB worst case, one copy live plus at most one retiring copy while
  guards drain. Before this revision the document strings were held twice
  (`current` and the scope's source); now once.
- The file ≤ ~130 KiB; `text` lives only for the duration of a commit.
- One blocking-pool thread per in-flight `PUT`, released on completion.
- Nothing grows with traffic or uptime (hard rule 4).

## 13. Security and correctness

- Only an authenticated `PUT` writes the document; migration is the one
  lifecycle writer; no traffic path can reach `commit` (the detector in p3-08
  lives in `fah-http`, which has no path to `fah-api` — `layering.rs`).
- `clients` decides who is decrypted; the document is on the `/config`
  volume with the CA key and the auth hash, same mode and ownership as the
  TOML (written after the privilege drop by the service user).
- `FAH__` cannot set either list: no arm, no array coercion; an attempt fails
  boot with `UnknownEnvKey` (regression test §14.2).
- `clients: []` means no device is intercepted; the SNI verdict still runs for
  everyone (`judge` precedes `interception_for`, unchanged).
- Exact strings are stored; normalisation is applied only to the compiled set,
  so what the operator sees is what they sent.
- `details` never echoes anything but the operator's own entry.

## 14. Test strategy

### 14.1 `fah-rules/src/interception.rs` (unit)

- moved: `an_exact_host_matches`, `a_parent_suffix_matches_its_subdomains`,
  `a_lookalike_that_only_ends_with_the_name_does_not_match`,
  `user_entries_are_normalized` (length equals the entry count now),
  `a_malformed_entry_is_rejected_by_name`, `the_empty_set_matches_nothing`.
- `normalize_host_*`: the `sni.rs` cases that cover it move with it.
- `compile_rejects_over_cap_naming_list_and_overage` (513 hosts; 257 clients).
- `compile_rejects_an_invalid_client_naming_its_index`.
- `compile_rejects_duplicates_after_normalization` (`Bank.ro` / `bank.ro.`;
  `192.168.88.10` twice) and reports `duplicate_of` as the first index.
- `compile_accepts_a_host_inside_a_listed_cidr`.
- `compile_keeps_the_document_spelling_out_of_the_scope`.
- `document_error_serializes_to_the_documented_details` (each variant →
  the exact JSON of §3.6).
- `a_held_guard_keeps_the_old_active_and_the_next_load_sees_the_new_one`.
- `lookup_cost_is_independent_of_list_size`: the 10 000-miss-under-a-second
  assertion from the deleted `the_baseline_exclusions_ship_without_any_configuration`,
  re-pinned on a 512-entry compiled set (ADR §Consequences).

### 14.2 `fah-config` (unit)

- `interception_keys_are_absent_from_a_default_toml` (`to_toml_string()` has no
  `interception`).
- `interception_keys_still_parse_as_present` (`clients = []` → `Some(vec![])`).
- `a_present_interception_section_round_trips` (skip only when both `None`).
- `env_interception_paths_are_rejected`: `FAH__HTTPS__INTERCEPTION__CLIENTS`
  and `…__EXCLUDE_DOMAINS` → `ConfigError::UnknownEnvKey` (ADR §Acceptance).
- `from_toml_str_is_the_file_layer_alone`: a TOML with `[https.interception]`
  keys parses to `Some` with no env pair applied and no `validate` run — the
  contract §3.5 relies on (F1).
- `write_atomic_error_paths_are_unchanged` (unwritable tmp, rename over a
  directory → the same `ConfigError::Io` paths as before the hoist).
- `parses_full_reference_toml_verbatim` and
  `defaults_match_configuration_md_sample` stay green.

### 14.3 `fah-api/src/interception_store.rs` (unit, tempdir)

- `first_boot_migrates_toml_lists_and_clears_them`: TOML with both lists →
  document file equals them; `config.https.interception.is_absent()`; the
  re-read TOML text contains neither key nor `[https.interception]`.
- `migration_saves_the_file_layer_not_the_effective_config` (F1): TOML with
  the keys and `port = 8443`; the effective `Config` passed in has
  `api.port = 9443` (as `FAH__API__PORT` would leave it) and a different
  `log.level`; after `load_or_migrate` the saved TOML has no interception
  keys, still says `port = 8443` and the file's log level, and the effective
  config still says 9443 with only `https.interception` changed
  (field-by-field equality against a clone taken before the call).
- `migration_fails_boot_when_the_toml_cannot_be_reread` (F1): a directory at
  `config_path` → `InterceptionStoreError::Toml`; document not written.
- `an_upgrade_with_literal_empty_lists_migrates_to_an_empty_document`.
- `a_fresh_install_writes_an_empty_document_and_leaves_the_toml_alone`
  (TOML bytes identical before/after).
- `an_existing_document_wins_and_is_byte_identical_after_boot` (TOML lists
  present → ignored; document bytes unchanged; TOML cleared).
- `an_unreadable_document_fails_boot_without_writing` (read-only dir / chmod
  where the platform allows; otherwise a directory at the document path).
- `a_second_boot_touches_nothing` (mtime/bytes of both files unchanged).
- `a_malformed_document_fails_boot_naming_the_file`;
  `an_unknown_key_in_the_document_fails_boot`; `an_over_cap_document_fails_boot`.
- `invalid_toml_values_fail_boot_before_the_document_is_written`.
- `commit_persists_then_publishes_one_arc` (file == `current().document`;
  `current().scope` counts match; `GET`'s document and the hot path's scope
  are `Arc::ptr_eq`).
- `prepare_rejects_without_side_effects` (over cap, duplicate, invalid,
  store-closed-with-client → file bytes and `Arc::ptr_eq(state.current())`
  unchanged).
- `store_closed_rejects_listing_a_client_but_accepts_an_empty_list`.
- `no_listener_stores_the_document`.
- `a_write_failure_publishes_nothing` (path whose parent is a regular file;
  `Arc::ptr_eq(state.current())` unchanged).
- `two_commits_serialize_and_the_last_one_wins_on_file_and_runtime_together`
  (two threads through `commit`; after both, file and `current().document`
  agree).
- `a_poisoned_lock_is_recovered_cleared_and_the_commit_proceeds`: poison
  `commit_lock` from a thread that panics while holding it (in-module test,
  private field reachable); `commit` succeeds; `commit_lock.is_poisoned()` is
  false afterwards; file and active agree.
- `a_recovered_commit_publishes_only_what_it_prepared` (the recovered commit's
  `Active` is the one passed in — pointer equality — never something read
  back from disk).
- `a_commit_that_panics_after_the_lock_leaves_file_and_active_unchanged`
  (F3): set `panic_after_lock`, run `commit` on a thread, `join()` is `Err`;
  file bytes and `Arc::ptr_eq(state.current())` unchanged;
  `commit_lock.is_poisoned()` is true; clear the flag; the next `commit`
  succeeds and `is_poisoned()` is false — the hook and the recovery test
  are two tests, not one.

### 14.4 `fah-api` routes (the crate's existing route-test style)

- `get_interception_returns_the_document`.
- `put_replaces_the_whole_document_and_returns_it`.
- `the_put_response_carries_no_restart_required` (response object keys are
  exactly `clients`, `exclude_domains`).
- `put_with_an_unknown_key_is_422_with_shape_details`.
- `put_with_a_non_json_body_is_400_in_the_envelope` (F4): `{` and a
  `text/plain` body → 400, `error.code == "bad_request"`, no `details`,
  `GET` unchanged.
- `put_over_cap_is_422_with_over_cap_details_and_get_is_unchanged`.
- `put_with_a_duplicate_is_422_with_duplicate_details`.
- `put_with_an_invalid_entry_is_422_with_invalid_entry_details`.
- `a_write_failure_is_500_and_get_is_unchanged`.
- `a_commit_panic_is_500_and_the_next_put_succeeds` (F3): the
  `test-harness` `panic_after_lock` hook (C2) — **not** a poisoned lock,
  which the recovery policy turns into a successful commit; asserts the 500
  body (`error.code == "internal"`), `GET` unchanged, then clears the hook
  and a following `PUT` returns 200 and `GET` matches it.
- `post_config_rejects_https_interception_naming_the_endpoint` (and persists
  nothing: TOML bytes unchanged).
- `get_config_omits_https_interception`.
- `error.rs`: `details_is_absent_unless_set`.

### 14.5 `fah-http/tests/interception.rs` (wire)

- `harness()`, `empty_interception`, `Setup` updated to build an
  `InterceptionState` from `Active::compile(InterceptionDocument {…})`;
  `Harness` exposes `state: Arc<InterceptionState>`.
- deleted, not adapted: `the_baseline_exclusions_ship_without_any_configuration`
  (its lookup-cost assertion moved, §14.1); the two baseline tests in
  `exclusions.rs` go with the file.
- `a_baseline_bank_is_never_intercepted_even_for_a_listed_client` →
  `a_parent_entry_in_the_document_splices_its_subdomain_for_a_listed_client`
  (`unicredit.ro` in the document, `homebanking.unicredit.ro` on the wire,
  origin's own certificate seen, `minted_total == 0`, both events `HttpsSni`).
- `an_empty_client_list_intercepts_nobody_whatever_else_the_config_says` and
  `a_listed_network_intercepts_exactly_its_members` kept (the `is_empty`
  assertion dropped).
- new `the_machinery_exists_with_an_empty_document_and_the_first_client_is_a_store`:
  empty document → `with_interception` keeps it; `proxy.intercepts` false for
  every ip; a connection splices (origin leaf seen); `state.store(Arc::new(Active::compile(listed)))`
  → next connection intercepted (FAH leaf, `minted_total 1`). No rebuild, no
  restart.
- new `a_removed_client_keeps_its_session_and_loses_the_next`: intercepted h2
  session open; store empty; a request on the open session is still served
  through the terminate leg; a new connection splices.
- new `a_stored_exclusion_splices_the_next_connection`.

### 14.6 binary and layering

- `crates/fastadhunter/tests/layering.rs` unchanged and green: `fah-rules`
  gains no *internal* dependency (`serde` is external, F2), `fah-http` keeps
  L1/L2 only, `fah-api` never names `fah-http` (both directions).
- Struct-literal constructions that gain the new `AppState` field (F7):
  `crates/fah-api/tests/api.rs:569` and
  `crates/fastadhunter/tests/history_e2e.rs:439` (`AppStateBuilder { … }`),
  plus `main.rs:588`. `crates/fah-api/tests/api.rs:373` builds
  `fah_model::ListenerCounters` by literal — untouched by this task, listed
  because p3-08 adds a field there.
- `e2e_https.rs` (`--all-features`) seeds `interception.json` and, in one
  scenario, lists the client through `PUT` and proves the next connection is
  intercepted with no restart (ADR §Acceptance line 1 on the real binary).

### 14.7 Gates

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --all-features --workspace`. No bench owed (§11).

## 15. Documentation and runbook consequences (not edited here)

- CONFIGURATION.md: `[https.interception]` sample rows go; a paragraph states
  that `fastadhunter.toml` no longer owns `clients` or `exclude_domains`, that
  `/config/interception.json` is their persistent source of truth, read and
  changed through `GET`/`PUT /api/v1/interception`; the release-N migration
  and the "keys re-added by hand are warned about and removed" behaviour.
- API.md: `GET`/`PUT /api/v1/interception` (§6) including the `details`
  contract (§3.6) and the caps; the envelope section gains the optional
  `details` field; the `POST /api/v1/config` 422 for `https.interception`;
  `GET /api/v1/config` omitting it.
- SECURITY.md: the opt-in paragraph and "Exclusions always splice" lose the
  compiled-in baseline; the document and the volume are named.
- CONTEXT.md: §Exclusion loses the baseline; new term **Interception
  Document**; "Interception" gains no third meaning (the runtime types are
  internal).
- README: "ADRs 0001–0007" → 0008.
- `docs/code-review/phase3/p3-06-phase3-verification-runbook.md`: `R2` drops
  the `https` part of its body; `R8` becomes `PUT /api/v1/interception` with
  no restart and no rollback rows. `p3-06-measurement-audit.md` Runbook 4
  precondition: the entry comes from the document, not the constant.
- `docs/project-state.md` Next row after the task lands (owner's file).

## 16. Out of scope and follow-ups

- Release N+1: deleting `InterceptionConfig` and the `interception` field —
  a task in the phase that ships the release after N.
- Detection and status 525 (p3-08); the dashboard (p3-09).
- `ConfigStore::apply_patch` saving the TOML synchronously on an executor
  worker, and `password.rs` keeping its own tmp-then-rename — two cleanups
  onto `spawn_blocking` + `fah_config::write_atomic`, separate task.
- `ConfigStore::apply_patch` and `post_config` serialising the *effective*
  config (env values baked into the file on an operator write) and
  `post_config` answering axum's plain-text rejection on a non-JSON body —
  pre-existing; migration and `PUT /api/v1/interception` are held to the
  stricter rule (§3.5 F1, C4 F4), the older route is not changed here.
- `fsync` in `write_atomic` and stale `.tmp.<pid>` cleanup — documented
  existing limitations (§3.7), not changed here by owner decision.
- A `WS /api/v1/events` message for document changes — not needed by p3-09's
  design; reopen if a second live surface needs it.
- Widening, auto-exclusion, the missing-CA diagnosis.

## 17. Dependencies

Depends on p3-04 (the machinery). Nothing deploys before the 0.3.3 soak
verdict; code lands on `phase3-06`. What the next tasks consume from this one
is stated in their plans: p3-08 the `Interception`/`InterceptionState`
runtime contract and the accept-arm invariant; p3-09 the endpoint and the
`details` contract.

## 18. Decisions — frozen by owner reviews, 2026-09-10

| # | Decision | Frozen as |
| --- | --- | --- |
| 1 | Home of moved types | `fah-rules::interception` (§3.1) |
| 2 | Client dedupe | `AllowedNet: Hash`, hash-based, never O(n²) (A2) |
| 3 | Persistence failure status | 500 `internal`; 422 only for invalid input (§3.3, §10) |
| 4 | File write in the handler | `spawn_blocking`; persist + publish one uncancellable unit; `std::sync::Mutex` on the pool only; reason in §3.3 |
| 5 | Error contract | structured `details` with closed `reason`, `list`, `index`, `entry`, `duplicate_of`, `len`, `cap` (§3.6); `message` never parsed |
| 6 | Published value | one `Arc<Active { document, scope }>` through one `ArcSwap`; no separate `current` (§3.3, §3.7) |
| 7 | Critical section | lock → atomic file replace → `ArcSwap::store` → unlock; all fallible/allocating preparation before the lock; nothing between `rename` and `store` (§3.7, A5, C2) |
| 8 | Poisoning | `PoisonError::into_inner()`, `clear_poison()`, explicit `error!` of the recovered panic and of the payload; no persistent "500 until restart"; not process-fatal (§3.7) |
| 9 | Failed normal persistence | active state unchanged, `next` dropped (§3.7 proof) |
| — | Stored spelling | the document keeps what the operator sent; only the compiled set is normalised (ADR) |
| — | Migration rewrites the TOML from the struct | accepted for comment loss; the struct is the **file layer**, never the effective config, so `FAH__` values are not written (F1, gate 2026-09-10) |
| 10 | Compile entry point | `Active::compile(document) -> Active` only; no free `compile` (F9) |
| 11 | Panic → 500 test | `#[cfg(any(test, feature = "test-harness"))] panic_after_lock` hook on `InterceptionStore`; poisoned-lock recovery is a separate test (F3). Gate corrected 2026-09-10: `#[cfg(test)]` is invisible to `crates/fah-api/tests/api.rs`, which links the library compiled without it, and `routes.rs` builds no `AppState` in-crate; `fah-api` already enables `test-harness` for its integration tests and `lib.rs` already refuses to build it into a release profile. Test-only `AtomicBool`, no production behaviour; required to exercise the real handler → `JoinError` → 500 path (C2) |
| 12 | Non-JSON `PUT` body | `Result<Json<Value>, JsonRejection>` + `body_error` → 400 `bad_request` in the envelope (F4) |
| 13 | `JoinError` | both branches handled; `into_panic()` only behind `is_panic()` (F5) |
| — | Boot fails on an over-cap or invalid existing document | accepted; the TOML's own contract |
| — | `fsync` / stale tmp files | existing limitations, documented in §3.7, no scope change |

## 19. Cross-task integration (D), open decisions (E), ADR acceptance map (F)

### D — order and integration

```text
p3-07  types (L1/L2) → fah-http swap → fah-api store + routes + details → binary wiring → migration → tests → docs
p3-08  predicate + 525 in the accept arm; harness rejecting connector; counter; docs
p3-09  dashboard: rejection view (needs 525 events) + exclude action and editor (need GET/PUT + details)
```

Order is contractual, not cosmetic — see p3-08 plan §11 for what p3-08
consumes from p3-07. All three land in one build — release N of §Migration —
which is the build the 24 h full-mode soak exercises after the 0.3.3 verdict.
No step ships alone (ADR §Phasing).

### E — open decisions

None remaining for p3-07 after §18. p3-08 and p3-09 decisions are frozen in
their plans (§12 of each).

### F — ADR-0008 §Acceptance → task (each line exactly once; two marked cross-task by design)

| # | ADR line | Task | Proof |
| --- | --- | --- | --- |
| 1 | A change takes effect on the next connection, no restart | p3-07 | §14.5 store tests; `e2e_https.rs` scenario |
| 2 | Machinery exists whenever the listener runs and the store opened | p3-07 | §14.5 empty-document test; §14.3 store-closed / no-listener tests |
| 3 | Response carries no `restart_required` | p3-07 | §14.4 response-keys test |
| 4 | An empty document excludes nothing | p3-07 | §14.1 `the_empty_set_matches_nothing`; no constant exists |
| 5 | Migration runs once, never overwrites, TOML carries neither key | p3-07 | §14.3 migration tests incl. unreadable-document |
| 6 | `POST /api/v1/config` rejects both keys | p3-07 | §14.4 |
| 7 | `FAH__` cannot set either list | p3-07 | §14.2 `env_interception_paths_are_rejected` |
| 8 | `fah-http` has no path to `fah-api` | cross-task | `layering.rs` green in p3-07 and p3-08 (p3-08 touches `fah-http`; nothing else does after p3-07) |
| 9 | Unclassified accept failure stays on `status 0` | p3-08 | p3-08 §7 |
| 10 | `UnknownCA` is never a 525 | p3-08 | p3-08 §7 |
| 11 | No new `Event` kind | p3-08 | p3-08 §7 |
| 12 | Nothing writes the document from traffic observation | cross-task | p3-07: `commit` callers are `put_interception` only, migration is boot-only (review grep); p3-08: detector has no path (layering, pointer-equality test); p3-09: a `PUT` happens only after a click (request-log test) |
| 13 | A rejected `PUT` changes nothing | p3-07 | §14.3 / §14.4 |
| 14 | A completed `PUT` leaves file and active state equal | p3-07 | §14.3 `commit_persists_then_publishes_one_arc`, two-commit test, poisoned-lock tests |

## 20. Final verification pass (owner's checklist)

| Check | Where it holds |
| --- | --- |
| Every ADR acceptance criterion covered, none duplicated or omitted | §19 F: 14 lines, 12 single-task, 2 cross-task with each task's half named |
| No task beyond its declared scope | p3-07 adds the `details` envelope field (its own contract), `AllowedNet: Hash` (its own dedupe) and the `write_atomic` hoist (its own invariant, same function A5 already touches); the two `spawn_blocking` cleanups and `fsync` are follow-ups or documented limitations (§16), not done here |
| Concurrency, cancellation, shutdown, error propagation, atomicity concrete | §3.3, §3.7, §8, §9, §10 |
| Panic / poisoning semantics proven | §3.7: no unwinding operation between `rename` and `store`; the four forbidden states each shown unreachable; recovery via `into_inner` + `clear_poison`; tests §14.3, §14.4 |
| No hot-path blocking or new allocation | §3.2, §11: one `ArcSwap::load`, a field read, no lock, no allocation |
| `fah-api` has no dependency on `fah-http` | §3.1: shared L2 handle; `fah-api/Cargo.toml` untouched; `layering.rs` |
| Detection cannot mutate live policy | the only writer of `state` is `commit`, reachable from `put_interception` and boot; `fah-http` cannot name `fah-api` |
| No auto-exclusion | no code path from an event to `commit`; §13 |
| Bootstrap with `clients=[]` mandatory | §3.4, §8 "First activation", §14.5 |
| Migration cannot overwrite an existing `interception.json` | §3.5: write only on `NotFound`; any other read outcome fails boot; §7 rows; §14.3 unreadable-document test |
| Migration writes no `FAH__` value into the TOML | §3.5 file-layer re-read; §14.3 `migration_saves_the_file_layer_not_the_effective_config` |

## 21. Final gate corrections (2026-09-10)

Independent last-pass review before implementation; each finding folded in
where it belongs, listed here so the diff is auditable.

| # | Finding | Where fixed |
| --- | --- | --- |
| F1 | migration saved the effective config, baking `FAH__` values into the TOML | §2 rows, §3.5, C2 `load_or_migrate`, D1, §7 rows, §14.2, §14.3, §16, §18, §20 |
| F2 | `fah-rules` has no `serde` dependency | A3 |
| F3 | panic → 500 test cited a poisoned lock, which the policy recovers | C2 hook, §14.3, §14.4, §18. Gate corrected at implementation time from `cfg(test)` to `cfg(any(test, feature = "test-harness"))` — see C2 and decision 11 |
| F4 | non-JSON `PUT` body bypassed the envelope | C4, §6, §10, §14.4, §16, §18 |
| F5 | non-panic `JoinError` branch unspecified | C4, §6, §10, §18 |
| F7 | missed call sites: `intercept.rs:538`, `benches/intercept.rs:140`, `api.rs:569`, `history_e2e.rs:439` (+ `api.rs:373` for p3-08) | B3, D2, §14.6 |
| F9 | two compile entry points | §3.1 table, §3.3, A3, C2 |

F6 and F8 are p3-08 / p3-09 findings; see their §Final gate sections.
