# Merge plan — `phase3-06` into `main`

Integration plan for landing Phase 3 (certificates, SNI filtering, HTTPS
interception, DoT/DoH listeners) on `main`. This file is not itself a phase
task — it has no `NN` and no status row. It does move a phase: `plan/wip/phase3`
arrives on `main` with the merge, by the owner's decision recorded in §Step 3.

## Decision already taken

Interception ships **disabled, not deleted**. The code, its tests, the
Interception Document and the dashboard views all land on `main` and stay
inert. Nothing from `phase3-06` is thrown away.

Rationale: `https.rs` already holds interception behind `Option<Interception>`,
and `interception_for` consults the Interception Document's client scope on
every accepted connection. Removing p3-04 now and restoring it later costs more
than carrying it switched off.

**The off switch is an empty `clients` list in `/config/interception.json`, not
`engine.mode`.** SNI filtering shares the 443 listener with interception, so a
mode without `https` would switch off the feature the deployment is keeping.
Mode stays `dns+http+https`; `interception_for` returns `None` for every client
because no IP is in scope, and the connection falls through to the SNI path.

## Direction

`main` merges **into** `phase3-06` first. Conflicts are resolved once, on the
integration branch, while `main` stays green and deployable. Only when
`phase3-06` passes every gate does it go back to `main`.

A rebase is the wrong tool here: 102 commits replayed means resolving the same
`tcp.rs` conflict dozens of times. One merge is one resolution.

## Starting state

Measured 2026-09-13.

| | |
| --- | --- |
| merge-base | `857865d` |
| `phase3-06` | 102 commits ahead, 18 behind |
| Conflicts | 16 files, all `CONFLICT (content)` |
| Modify/delete, renames | none |

The absence of modify/delete conflicts means this merge has no file-level
delete/modify decision to resolve. It does not by itself prove that content from
either side survived correctly — a file deleted on one side and untouched on the
other is removed silently, with no conflict raised. Content conflicts and silent
auto-merges are covered separately by the conflict inventory, the Step 2 review
and the targeted tests.

### What `main` gained since the fork

`ed28395` F1 DNS-over-TCP connection ceiling and message bound · `b0b091e` F2
UDP in-flight ceiling · `32d7776` F10 stats flush on clean stop · `ad110d8`
F1/F2/F10 close-out tests · `d307c36` adaptive as the only upstream strategy ·
`0fb8dd0` F11 supervisor · `07d4d68` + `c220956` F3 query borrow and pre-sized
`domain_of` · `3287418` H1-H3/D1 allocation removals · `8941770` idle upstream
pool reaper · `28c751d` HTTP refusal counter split · plus docs and review files.

### Files that arrive clean

`udp.rs`, `cache.rs`, `qtype.rs`, `swr.rs`, `testkit.rs`, `supervisor.rs`,
`shutdown_e2e.rs`, `udp_inflight_cost.rs`, `strategy_ab.rs`, `proxy_alloc.rs`,
`connections.rs`. `phase3-06` never touched them. They are additions, not
deletions — a `git diff main..phase3-06` displays them as removals purely
because of diff direction.

## Step 0 — before touching anything

### Clean the tree without rewriting it

`main` carries an uncommitted `.gitignore` change. **Stash it** — or park it in
another worktree. Do not commit it to clean the tree: this plan states `main` is
at `28c751d`, and an unrelated commit makes `main-pre-phase3-merge` point
somewhere else, so the rollback target stops matching the document describing
it. Do not discard it either; it is the owner's change and unrelated to the
merge.

A merge started on a dirty tree also makes "did the merge do this?"
unanswerable.

### Record where everything stands

```sh
git status --short
git rev-parse main phase3-06
git merge-base main phase3-06
```

Cheap, and it makes every later claim in this document checkable against what
was actually merged rather than against the state measured on 2026-09-13.

### Two rollback points, not one

```sh
git tag phase3-06-pre-main-merge phase3-06
git tag main-pre-phase3-merge main
```

Tag targets are explicit on purpose: `git tag <name>` while checked out on
`main` tags `main`, whatever the name says.

The second tag is the one that matters at Step 5. Until then `main` is
untouched, but once the fast-forward lands there is otherwise no recorded way
back to `28c751d`.

### Confirm `main` is green before anything else

The primary checkout is already on `main` at this point, so run the gates here
rather than in the benchmark worktree — same evidence, without a second full
build in a fresh `target/`:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
```

The known-good Windows E2E exception below still applies. Anything else red here
is inherited, and must not be attributed to the merge later.

### Capture the performance baseline from `main`

The merge must be shown to have kept what `main` gained, not merely to compile.
That evidence splits in two, and the split matters:

- **Functional fixes** — F1, F2, F10, F11, `8941770` — are proven by their
  tests. A bench says nothing useful about a connection ceiling or a flush on
  shutdown. See §Targeted checks.
- **Performance-sensitive work** — F3, H1-H3, D1 — is proven by an A/B against
  `main`, because a lost allocation removal still compiles and still passes
  every test that has no ceiling.

Only the second needs a baseline, and it only exists if `main`'s numbers are
taken **before** the merge, on this machine.

```sh
git worktree add --detach ../fah-main-bench main
cd ../fah-main-bench
cargo bench 2>&1 | tee ../fah-bench-main-r1.txt
```

`--detach` is required, not stylistic: the primary checkout already has `main`,
and git refuses the same branch in two worktrees —
`fatal: 'main' is already used by worktree at 'E:/FastAdHunter'`. Detaching also
pins the baseline to one commit, so it cannot drift if `main` moves.

This is round 1 of four. Rounds 2 to 4 run at Step 4, alternating between this
worktree and the merged branch. **The worktree stays until all four are done** —
removing it early destroys the protocol.

A separate detached worktree pins the `main` commit used for the baseline and
gives it its own `target/criterion`, which is what keeps the comparison honest:
there is no way to accidentally A/B against criterion's stored baseline instead
of real pre-change code (docs/measurement-traps.md).

### Housekeeping — the older worktrees

`git worktree list` shows two left from earlier work:

- `E:/fah-ab-base` at `c220956` — an old A/B experiment, not today's `main`
- `E:/FastAdHunter-var-h1cap` at `66df219` — the H1-cap diagnostic

Neither is reused here. `fah-ab-base` is close enough to what this step creates
to be mistaken for it, and it is at the wrong commit — that is the reason to
remove it, not disk space. Removing them is **explicit housekeeping, not part of
the merge**: decide it separately, do it separately, so the three objects stay
distinguishable.

| Worktree | What it is |
| --- | --- |
| `fah-ab-base` | old experiment |
| `FastAdHunter-var-h1cap` | old diagnostic |
| `fah-main-bench` | the Phase 3 merge baseline |

## Step 1 — merge `main` into `phase3-06`

```sh
git switch phase3-06
git merge main
```

### Resolution rule

For anything `main` introduced after the fork, **`main` is the source of
truth**. `phase3-06` never saw those changes, so a hunk that looks like the
branch "removing" them is the branch being old.

The rule has three exceptions — the files where the fix being merged and the
test that protects it live together. They are treated in §Conflict hotspots.

### Remaining conflicts

| File | Resolution |
| --- | --- |
| `crates/fah-dns/src/server.rs` | keep F1/F2 wiring; add the 853 bind and `serve(pipeline, Option<DotTls>)` |
| `crates/fastadhunter/src/main.rs` | keep F10 flush, F11 supervisor, adaptive-only wiring; add `dot_tls`, cert store, HTTPS listener |
| `crates/fah-http/src/proxy.rs` | keep H1-H3/D1 and the idle reaper; add the HTTPS/SNI paths |
| `crates/fah-http/src/request.rs`, `domain.rs`, `lib.rs` | `main`'s allocation work wins; re-add branch exports |
| `crates/fah-model/src/lib.rs`, `crates/fah-api/src/config_store.rs` | additive both sides; union |
| `crates/fah-dns/tests/forward_alloc.rs` | **`main` wins.** See below |
| `crates/fah-dns/tests/server_integration.rs` | keep `main`'s F1/F2 cases; add the DoT cases |
| API.md, ARCHITECTURE.md, README.md, ROADMAP.md, docs/project-state.md | union of both; `project-state.md` is rewritten at the end anyway |

### `Cargo.lock` merges without complaint and is not to be trusted

Both sides changed it and git resolves it silently. A lockfile reconciled by
three-way merge can be internally valid and still wrong.

Inspect it, then verify it without letting cargo repair it behind your back:

```sh
git diff -- Cargo.lock
cargo check --workspace --locked
```

`--locked` is the whole point: it fails rather than rewriting. A plain
`cargo check` would quietly fix a bad merge and leave you deducing afterwards
what changed. If the merged lockfile genuinely needs regenerating, that is a
deliberate separate action, not a side effect.

`Cargo.toml` at the workspace root is not in the conflict set — only `main`
touched it, so the `0.3.4` bump carries over cleanly from the branch's `0.3.3`.

### Conflict hotspots with co-located regression alarms

Three conflict files deserve explicit treatment because the implementation being
merged and the regression test protecting it live in the same file. A bad
resolution removes the fix and its alarm in one move, and nothing downstream
complains.

#### `crates/fah-dns/src/tcp.rs` — F1 + DoT

The hardest content conflict. Both sides rewrote the same framing loop:

- `main` (F1): connection ceiling taken as a permit before `accept`, 16 KiB
  per-message length bound, `TcpConnectionGauge`.
- `phase3-06` (p3-05): the loop generalised over `Transport` so `dot.rs` can
  reuse `handle_connection` and `report_connection_end`.

Neither side wins. Reapply the DoT generalisation **on top of** F1's version,
preserving the permit-before-accept ordering, the length bound and the gauge.
"Take ours / take theirs" loses either DoT or the F1 bounds.

Both F1 tests live in this file and must survive the resolution:

- `a_length_prefix_over_the_bound_closes_the_connection_and_counts` (:305)
- `the_connection_ceiling_holds_the_next_accept_until_one_closes` (:353)

A third, `the_configured_tcp_and_udp_ceilings_reach_the_listeners`, sits in
`tests/server_integration.rs`, also a conflict file.

#### `crates/fah-http/src/proxy.rs` — H1-H3/D1 + `8941770`

Holds main's allocation work, the idle upstream pool reaper, and the two tests
that protect the reaper:

- `an_idle_upstream_connection_is_reaped_after_the_idle_timeout` (:719)
- `an_active_upstream_connection_is_reused_across_requests` (:789)

The merged client construction keeps all three builder calls:

```rust
.pool_timer(TokioTimer::new())
.pool_idle_timeout(upstream_idle_timeout)
.pool_max_idle_per_host(max_idle_per_host)
```

`pool_idle_timeout` does nothing without `pool_timer` — that was the bug
`8941770` fixed. `pool_max_idle_per_host` is what made it expensive: 8 per host
across 2 allocation domains, sixteen connections each pinning a grown 408 KiB
buffer. Taking the Phase 3 version wholesale restores the pre-`8941770`
behaviour, and only RSS on the device would eventually say so.

#### `crates/fah-dns/tests/forward_alloc.rs` — F3 allocation guard

It existed at the fork and both sides changed it. `main`'s version carries the
per-handle allocation ceilings from the F3 follow-up — the test that fails
loudly if a resolution puts a clone back on the query path.

Keep those ceilings. Extend the test only where a Phase 3 transport path
demonstrably needs a different bound.

#### Procedure for all three

1. Read the pre-merge implementation together with its local tests.
2. Resolve the content conflict.
3. Read the resulting implementation and tests together.
4. Run that file's targeted test immediately.
5. Only then move to the next conflict.

### Before calling the resolution done

```sh
git status --short
git diff --check
git grep -n -E '^(<<<<<<<|>>>>>>>)'
git log --oneline --decorate -n 5
```

`git diff --check` catches whitespace damage. The explicit `git grep` searches
the resulting worktree for the two unambiguous conflict-marker forms. `=======`
is omitted deliberately: the repository contains legitimate uses of it in
Markdown, where it underlines a setext heading.

## Step 2 — review what merged quietly

Auto-merging is not correctness. These files produced no conflict and still need
reading:

- **`crates/fah-dns/src/upstream/mod.rs`**, **`schema/dns/upstreams.rs`**,
  **`tests/adaptive_behaviour.rs`** — adaptive-only exists on both sides as two
  different commits: `fa9451a` on the branch, `d307c36` on `main` as its
  cherry-pick. Git treats them as unrelated work that happens to agree, and
  resolves all three files silently. Verify the `fallback` walk is gone exactly
  once, that a config naming `fallback` still fails at load, and that no test
  asserting the old strategy survived.
- **`crates/fah-dns/src/pipeline.rs`** — F3's borrow of
  `request.queries.first()` versus the branch threading `Transport` through the
  event-build site. Confirm the borrow survived and no clone came back.
- **`crates/fah-metrics/src/registry.rs`**, **`crates/fah-model/src/engine.rs`**
  — counters added on both sides; check for duplicate or shadowed entries.
- **`crates/fastadhunter/src/adapters.rs`** — `DnsWireAdapter` from p3-05 next
  to `main`'s adapter changes.

## Step 3 — decisions taken

Owner's answers, 2026-09-13. Recorded here because each one changes what a
merged build does on first boot.

1. **`dot_enabled` and `doh_enabled` stay `true`** (`schema/dns/listen.rs`).
   A merged build binds `:853` and serves `/dns-query` without further
   configuration. Accepted with the LAN reaching both. There is no listener ACL
   yet — the only bound on DoT is its 64-connection cap, so exposing 853 beyond
   the LAN is a separate decision that has not been made.
2. **`engine.mode` moves to `dns+http+https`**, from the `dns+http` production
   runs today. That raises the 443 listener and with it SNI filtering.
   Interception stays off through the empty client scope, not through the mode —
   see §Decision already taken.
3. **`plan/wip/phase3` lands on `main`.** `main`'s `plan/wip/` holds only
   `.gitkeep`; the branch carries the phase directory, so the merge performs an
   `open` → `wip` phase move as a side effect. Approved deliberately, since it
   is the one move the working agreement says an agent never makes alone.

DoT and DoH sit under `[dns.listen]`, not under the HTTPS mode. They work at any
`engine.mode`.

Together these three mean a merged build listens on more ports than today's:
`53` keeps UDP and TCP, `853` and `443` are new, alongside HTTP and the API.

## Step 4 — verification on `phase3-06`

Gates, per the root CLAUDE.md:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --all-features --workspace
```

### Targeted checks — one per thing `main` fixed

`cargo test --all-features --workspace` runs the normal targeted tests below.
Ignored measurement and behavioural harnesses are invoked separately, where
explicitly listed — the workspace gate does not execute them.

They are listed one by one because this is the list of regressions a conflict
resolution can reintroduce silently, and each is read individually rather than
trusted to a green summary line.

Benches protect what is performance-sensitive. Tests protect what is
functional. Most of what `main` gained is the second kind.

**F1 — DNS-over-TCP bounds** (`src/tcp.rs`, both inside the conflict)
- `a_length_prefix_over_the_bound_closes_the_connection_and_counts`
- `the_connection_ceiling_holds_the_next_accept_until_one_closes`
- `the_configured_tcp_and_udp_ceilings_reach_the_listeners`
  (`tests/server_integration.rs`, also conflicting)

**F2 — UDP in-flight ceiling.** The file holds one test,
`udp_inflight_cost_under_upstream_outage`, and it is `#[ignore]` — an RSS
measurement harness the workspace gate never runs. Invoke it explicitly:

```sh
cargo test -p fah-dns --test udp_inflight_cost -- --ignored
```

**F3 — query borrow and allocation ceilings**
- `crates/fah-dns/tests/forward_alloc.rs` — per-handle ceilings
- `warm_pipeline_handles_allocate_a_steady_amount`

**F10 — stats flush on clean stop**
- `crates/fastadhunter/tests/shutdown_e2e.rs`

**F11 — task supervision.** The six `supervisor.rs` tests arrive clean and
cannot be lost — and they are not sufficient. They exercise `reap()` directly,
so they stay green even if the merge drops the call site in the conflicting
`main.rs`. Verify the wiring by reading it:
- `supervision.tick() => self.reap_dead_tasks()` still on the select arm
  (`main.rs:623`)
- `reap_dead_tasks` still reaps **both** `self.tasks` and
  `self.stats_schedulers`. Keeping one and dropping the other leaves the stats
  schedulers unsupervised and every test passing.

**H1-H3/D1 — allocation removals**
- `crates/fah-http/tests/proxy_alloc.rs`

**`8941770` — idle upstream pool reaper** (`src/proxy.rs`, inside the conflict)
- `an_idle_upstream_connection_is_reaped_after_the_idle_timeout`
- `an_active_upstream_connection_is_reused_across_requests`

**adaptive-only.** `crates/fah-dns/tests/adaptive_behaviour.rs` is auto-merged,
so read it first: no test asserting the deleted `fallback` walk may have
survived.

Then run it. Of its six tests the workspace gate executes exactly one,
`g3_default_gate_arms`; the other five are `#[ignore]` and cover the behaviour
adaptive-only is supposed to preserve — black-hole handling
(`b1_black_hole`), recovery and flapping (`b5_recovery_and_flapping`), host
isolation (`b7_resolve_host_isolation`), SWR interaction (`b8_swr_interaction`)
and pacing (`b1_pacing_dry_run`):

```sh
cargo test -p fah-dns --test adaptive_behaviour -- --include-ignored --test-threads=1
```

Serial execution is not optional here: `require_serial` asserts on it, because
every latency figure in those arms assumes the arm runs alone.

The Phase 3 surface, running for the first time against post-F1 code. These
exercise interception end to end with rustls clients on the dev box — they
switch it on themselves and need no configuration and no device. They are what
makes "interception ships disabled" safe rather than a slow leak:

- `crates/fah-http/tests/interception.rs` — 2531 lines
- `crates/fastadhunter/tests/security_phase3.rs` — 1010
- `crates/fah-http/tests/sni.rs` — 856
- `crates/fastadhunter/tests/e2e_https.rs` — 525
- `crates/fastadhunter/tests/interception_migration.rs` — 312

The dashboard has its own runner and its own gate. Both sides changed
`dashboard/frontend/src/api/types.ts`, so a merge can break the frontend while
every cargo gate stays green:

```sh
cd dashboard/frontend
npm run typecheck
npm test
```

### Benches — the baseline is `main`, not the branch tag

Allocation ceilings are tests and fail loudly. Throughput and latency are not,
so they need the A/B whose first half was captured at Step 0.

**Why not the branch tag.** Comparing against `phase3-06-pre-main-merge` proves
nothing: the merge *brings* main's performance work with it, so the comparison
reads as an improvement whether the merge kept all of F3/H1-H3/D1 or half of
them. Both outcomes look "better than before". `main` is the only baseline
where those gains were measured, so only "slower than `main`" means the merge
lost something.

**Method, per docs/measurement-traps.md:** run **A/B/A/B**, alternating between
the `main` worktree and the merged branch, two rounds each.

1. `main` worktree — R1, already captured at Step 0
2. merged `phase3-06` — R1
3. `main` worktree — R2
4. merged `phase3-06` — R2

Alternate. Running all of one side first and then all of the other lets machine
drift masquerade as a code difference, which is the whole reason the protocol
exists. Compare means and ranges across the two versions, never a single pair.

The merged branch is the **primary checkout** — there is no second integration
worktree; only `main` got one:

```sh
cd /e/FastAdHunter
cargo bench 2>&1 | tee ../fah-bench-merged-r1.txt

cd ../fah-main-bench
cargo bench 2>&1 | tee ../fah-bench-main-r2.txt

cd /e/FastAdHunter
cargo bench 2>&1 | tee ../fah-bench-merged-r2.txt
```

Criterion's own `change:` line compares against whatever ran previously in that
benchmark directory, not against the other checkout. It is ignored here.

The `.txt` files are artefacts to keep, not the comparison itself: `diff` tells
you two numbers differ, not whether the difference is outside the run-to-run
range. Read the reported means and intervals per benchmark. The 10% gate from
the root CLAUDE.md applies to those figures, not to textual difference.

Phase 3's own benches — `fah-certs/benches/certs.rs`, handshake and TLS costs —
have no counterpart on `main`. Record them, do not compare them.

Run every round on the same machine in the same sitting; thermal state moves
results more than most changes do. Then clean up:

```sh
git worktree remove ../fah-main-bench
```

**Known-good note:** `cargo test -p fastadhunter --test e2e` fails on the
Windows dev box with `WSAEACCES` (10013) when WinNAT reserves the ephemeral port
block. Environmental — not a merge defect.

## Step 5 — `phase3-06` into `main`

Only after Step 4 is fully green. Approval is required before this step and
before any push; both remotes (`origin` and `backup`) must succeed.

Step 1 already put `main` inside `phase3-06`, so this direction is a
fast-forward — no conflicts, no merge commit, nothing left to resolve:

```sh
git switch main
git merge --ff-only phase3-06
```

`--ff-only` is the check, not a preference: if git refuses, `main` moved after
Step 1 and Step 4's evidence no longer describes what would land.

`main` stays at `0.3.4` — the branch carries `0.3.3` but never touched the
workspace manifest.

Deployment to the RB5009 is a separate decision, not part of this plan. The
router is not touched by any step here.

## Rollback

- **Merge still in progress**, conflicts unresolved: `git merge --abort`.
  `reset --hard` is the wrong command while `MERGE_HEAD` exists.
- **Merge committed on the integration branch:**
  `git reset --hard phase3-06-pre-main-merge`
- **Before Step 5:** `main` is untouched, so rollback is doing nothing
- **After Step 5:** `git reset --hard main-pre-phase3-merge`, which is why
  Step 0 tags it. Only safe while nothing has been pushed — once `origin` and
  `backup` have it, the way back is a revert, not a reset

## Out of scope

Deleting interception · deploying to the router · the public certificate for
`dns.localbox.ro` · a listener ACL · moving `plan/wip/phase3` to `closed`.

### The device campaign is a separate gate

p3-06 stays `AWAITING SOAK` after the merge. Merging moves code; it produces no
on-device evidence and is not blocked by the lack of it.

The campaign also cannot be re-run as written: it was designed against a build
with interception **on**. With interception off, the CA-install walkthrough and
the per-client interception arms (P1, P2, P3) have no subject, while the TLS
budgets, Android Private DNS, DoH over h2, 443 steering and the soak (N5, N6)
all still apply.

Re-scoping it belongs to **p3-06b** (`p3-06-after-interception-impl.md`,
`WAITING`) — the task that already owns the p3-06 runbook, smoke plan, testing
plan and probe scripts. Order: merge, gates green, land on `main`, then p3-06b
rewrites the campaign for an interception-off build, then the campaign runs.
