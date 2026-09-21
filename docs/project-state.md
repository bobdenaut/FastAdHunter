# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-09-21 (twenty-fifth pass — **the workspace is at 0.4.3**,
`fe73cec`: one line in `Cargo.toml` and the lockfile across all 13 members, no
code, no tag. The twenty-fourth pass, the same day — **the Diagnostics · Memory
residual verdict is fixed**, `5c27cbe`, dashboard only: restarts are detected on
either counter (the 07:15Z one went 91 → 93 MiB and was invisible), the residual
trend is judged by median so a six-sample excursion no longer reads as a ramp,
and over-accounted rows are left out. **The deployed image carries the old
dashboard** — `/web` is image content — so the router's page keeps reading the
old way until a 0.4.2 tar is built. `e1c610b` updates README for the HTTPS and
HTML phases; no code. Both remotes are at `5c27cbe`.
The twenty-third pass, the same day — **the workspace is at 0.4.2**,
`25d7582`: one line in `Cargo.toml` and the lockfile refresh across all 13
members, no code touched, and **no `v0.4.2` tag yet** — `git tag -l 'v0.4*'`
lists only `v0.4.0` and `v0.4.1`. **Nothing is built from it.** The ARM64 tar
that would carry Fix 1 and the feed-status commits to the router does not exist,
so the RB5009 still runs 0.4.1 and the version number alone changes nothing
there (§Deployed, §Build ≠ tip). Both remotes are at `25d7582`, pushed together,
with `9eee73b` — this file's twenty-second pass — just before it. The
twenty-second pass, the same day — **Fix 1 of the IPv6
privacy-rotation review shipped**, `6350fc1`: unnamed clients unseen for
`stats.client_idle_expiry_days` (default 7, live) leave the registry on the 20 s
policy tick, `GET /api/v1/clients?seen_within=` filters on `last_seen`, and the
clients page opens on IPv4 seen in the last 24 h — the list that grew ~19 IPv6
privacy addresses a day now stops. **ADR-0010 chose the RouterOS REST
connector for device identity** (`4d2889d`): identity by MAC, read-only, polled
on demand with a 10 min full refresh, verified on the RB5009 the same day, and
planned as phase 2.7 in `plan/open/` — six tasks `WAITING`, nothing built, the
router untouched. Both remotes reached `4d2889d` that day. The same day the 0.4.1
residual soak opened at t0 `2026-09-21T07:14:50Z` (`e21a4f2`) and the feed's
https-sni status reached the screen (`4f49a9e`) and tui-monitor (`0e49ce9`).
The twenty-first pass, the same morning — **0.4.1 is deployed on the RB5009**
since 05:37:31Z, run from the install guide with the 0.4.1 tar under the
guide's 0.4.0 names: `dns+http+https`, DoT, DoH, the 443 steer and the QUIC
rejects are live and were verified read-only afterwards (§Deployed). **The
heating gateway was cut by the steer within three minutes** and exempted within
twenty; it never appeared in the feed because the SNI proxy closed it on a
silent path, and **`c5732da` makes both silent paths emit feed events** (408 no
hello, 400 non-TLS bytes), with tests — the tip, one commit past `origin`, four
past `backup`, not in the deployed build. The guide's rollback order was
corrected the same morning (`4408a43`): the old TOML must be restored *before*
0.3.4 starts, or it exits on the 0.4.x keys and nothing resolves. The twentieth
pass, 2026-09-19 — the workspace went to 0.4.1: one line in `Cargo.toml` and
the lockfile refresh, since committed as `a437bec` and tagged `v0.4.1`; the
guide still names 0.4.0 everywhere (§Version). The nineteenth pass,
2026-09-17 — **four commits closed nine audit
findings, three of them with production code**: `b0c7709` SP2, N2 and R2 of the
post-merge audit; `067427a` S1, S3, S7 and TODO row 9 of the second hot-path
audit; `9d51567` the review of those two, no defect; `9b55cbb` V1 and V3 of a
new whole-repository read, its V2 deferred by decision — §Audit closures below.
Both remotes are at `9b55cbb`. The eighteenth pass, the same day — **the 0.4.0
deploy guide exists and the router was read to write it**:
[0.4.0-install.md](0.4.0-install.md) (`2b47737`) lists only the settings missing
between the running 0.3.4 and 0.4.0, every claim checked read-only against the
RB5009 that day. Nothing was changed on the router. **Phase 3's HTTPS/DoT/DoH
claims were then proven by execution**, not by reading — §Phase 3 verified below.
**The public certificate's renewal path was corrected and automated**
(`04856ad`), and it is blocked on an empty Cloudflare token — §Public certificate
below. The seventeenth pass, 2026-09-16 — the workspace went to 0.4.0
(`0197ae5`) and the deploy config landed raised to `dns+http+https` (`98eda8d`,
`be54185`). The sixteenth, the same day — **the 0.3.4 soak was stopped on day 5
with a memory finding**: the residual floor doubled, 19.0 → 38.6 MiB, and stayed
elevated across every 12 h bucket while `accounted_bytes` held at 28.00 MiB.
Report, reducer and a collector fix at `ad795b0`; the phase-3 plan files that
gated on that soak retired at `455ea36`, which also gave p3-11 a new
precondition. The fifteenth closed the TCP length-prefix framing by dev-box
measurement)

## Now

| | |
| --- | --- |
| Branch | `main` is at **`fe73cec`** (2026-09-21, the 0.4.3 version bump) — **`origin/main` and `backup/main` are both at `fe73cec`**, all three hashes read together that day. **Sixteen commits followed `c5732da`, eight of them production:** `5f3108c` this file; `dcbcbe9` the guide's heating-gateway and office-laptop exclusion rules; `e21a4f2` the 0.4.1 residual soak opened with its t0 and hourly collector; **`4f49a9e`** the feed's https-sni status drawn in the Detail cell; **`0e49ce9`** tui-monitor rendering the `https-sni` and `https` feed kinds instead of counting them as drift; **`cafd059`** the health page's short cards beside Backpressure; **`e7e905e`** the `c-share` rename that stopped ad blockers hiding the column; **`6350fc1` Fix 1 — idle expiry of unnamed clients, `stats.client_idle_expiry_days` applied live, `GET /clients?seen_within=`, the clients page opening on IPv4 seen in 24 h, `fah_common::idle::older_than` as the one age predicate, `BOOT_KEYS` `stats` narrowed to `stats.snapshot_interval_seconds`**; `4d2889d` ADR-0010 and `plan/open/phase2.7-device-identity`; `9eee73b` this file; `25d7582` the workspace bumped to 0.4.2, `Cargo.toml` and the lockfile only, untagged; `6e05593` this file; `e1c610b` README's filtering and deployment claims; **`5c27cbe` the Diagnostics · Memory residual verdict — restart detection across both counters, median for the residual trend, over-accounted rows kept out of it**; `3fe0097` this file; `fe73cec` the workspace bumped to 0.4.3, `Cargo.toml` and the lockfile only, untagged. **Phase 3 landed on `main`** on 2026-09-13, by fast-forward — `bc49e4e..78238b4`, 105 commits, no merge commit ([plan/plan-merge.md](../plan/plan-merge.md), all five steps closed). `backup` had drifted five commits behind on 2026-09-17 and was caught up by the `04856ad` push — **it has drifted silently before while this row claimed both were current, so check it with `git rev-parse HEAD origin/main backup/main` rather than trust the row.** Read the remotes that way, not from the tracking refs. **Eight commits followed `9b55cbb`, two of them production:** `785c479` the PowerShell 5.1 ssh quoting trap and RouterOS exit codes in the docs; `70a8e72` `renew-certificate.ps1` working from PowerShell 5.1; `c35f7b8` the Phase 4 plan files; `a437bec` the 0.4.1 bump, tagged `v0.4.1` and **the deployed build**; **`90a208a` log messages kept ASCII for the RouterOS log**; the owner's three above; **`c5732da`**. **Never read the tip from this row** — it has been wrong on ten occasions now (`99e4953`, `9d9d792`, `26ffe1c`, `3e97d16`, `e30af33`, `19693be`, `bb15de7`, `9cbbfd1`, `af5cf61`, `52eec09`), so use `git rev-parse main`. **Four commits followed `04856ad`, three of them production:** `b0c7709` the DoT leaf pre-warm inside the handshake deadline (SP2), the three DNS gauges on the Health page (N2) and the two missing pair-matrix rows (R2); `067427a` the cache sweep recomputing a shard's byte count (S1), the `qtype_label` fold (S3), the dashboard `Other(code)` comments (S7) and the >64 KiB TCP reply test (TODO 9); `9d51567` the review of both, docs only; `9b55cbb` the list-path guard and the rollup boot walk (V1, V3) — §Audit closures below. **Seven commits followed `52eec09`, none production:** `ad795b0` the 0.3.4 soak analysis, its reducer and the `collect-soak.py` container-selection fix; `455ea36` the phase-3 plan files; `98eda8d` the deployed config checked in as `docs/fastadhunter.toml`; `be54185` that file raised for 0.4.0; `0197ae5` the workspace version bump; **`2b47737` the 0.4.0 deploy guide**; **`04856ad` the certificate-renewal corrections, `scripts/renew-certificate.ps1`, the `scripts/` line in `CLAUDE.md` and the DoT hostname settled on `fah-dot.localbox.ro`**. Before them, `93e3ff2` carried a Fable review's findings. Eleven commits followed `f12eec3`, **five of them production**, and every one comes from the second hot-path audit: **`3ce7ec5`** the poisoned cache shard plus the audit document, **`c93e2d1`** the stale-serve key clone, **`bb15de7`** the rotation error logged once, `f51f70a` this file, `f51a830` Finding 7's extra oracle batches, `9cbbfd1` Finding 9's HTTPS oracle cases, `cedbaf5` this file, **`f351980`** the masked `slots` warning gated to `test-harness`, **`af5cf61`** Finding 9's `QueryType` redesign, `93fa665` this file, `52eec09` the TCP framing bench closing TODO row 11 — §Second hot-path audit below. Findings 2 and 3 were measured on the RB5009 and closed with **no** commit. Before them, fourteen commits followed `9d9d792` and exactly two were production: `e8e7cf8` the enumeration sweep's record, `2b03a30` `bd6b1f0` `0662df4` `8abb9c1` the first hot-path audit and its rewrites, **`26ffe1c` the A1/A2 listener fix**, `ad12103` this file, `3e97d16` the hook and its test, `59a4f84` `847d369` this file again, `76427d9` that audit's close with hard rule 3, `e30af33` `CLAUDE.md`, `514d935` `f12eec3` this file, and **`19693be` the errno probe with A2's classification narrowed** — §Hot-path audit and the A1/A2 fix below. Everything before them is in `git log`; the ten from 2026-09-14/15 are listed in §Session 2026-09-14/15. `phase3-06` (`78238b4`) has served its purpose and sits well behind. Rollback tags: `main-pre-phase3-merge` = `ebc46f1`, `phase3-06-pre-main-merge` = `185139b`; `eb693e2` is the merge commit inside the branch, two parents. `pre-alloc-domain-2026-09-06` = `64be513` stays the rollback point before the allocation domains |
| Tree | **Clean at `fe73cec`, apart from this file and the image tar the repo ignores; nothing unpushed.** Pushing goes to **both** remotes and is not done until both succeed — the last push this row can vouch for on both is `fe73cec`, 2026-09-21, the 0.4.3 bump, and it was vouched for by running `git rev-parse HEAD origin/main backup/main` and getting the same hash three times, which is still the only trustworthy read. A top-level `scripts/` exists (operator PowerShell, outside the build and outside the gates), listed in `CLAUDE.md` §Layout. The dev box also holds `fastadhunter.toml.0.3.4`, the chapter-3 backup of the router's pre-deploy config, which the rollback needs |
| Tests | **The full gate ran on the `6350fc1` tree, 2026-09-21, Windows dev box** — `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --message-format=short -- -D warnings` clean, `cargo test --all-features --workspace --no-fail-fast` **60 binaries, 1 686 passed, 0 failed**, totalled from the `test result:` lines themselves, not from the `rtk` view; dashboard `tsc --noEmit` clean and **58 files, 1 059 tests, 0 failures**. Fix 1 added the registry expiry tests (fresh, stale, named, age equal to the limit, backwards clock), the snapshot and live-setting tests in `fah-stats`, `older_than`'s own boundary tests in `fah-common`, `parse_seen_within` unit tests and two API tests, the config default/zero/env tests and the boot-key classification row; the dashboard gained the URL, chip and empty-state tests. **`4d2889d`, `9eee73b`, `25d7582`, `6e05593` and `e1c610b` touch no code** — docs, this file, the version bump and README — and `5c27cbe` is dashboard TypeScript only, so that Rust gate still describes the tree the 0.4.2 build comes from. **The dashboard was re-run at `5c27cbe`, 2026-09-21, same box** — `tsc --noEmit` clean, **58 files, 1 081 tests, 0 failures**, 22 more than the 1 059 of `6350fc1`, which is what that commit's tests add. `npm run build` was not re-run, so the gzip figure later in this row is still the one measured at `9d9d792`. **The earlier full gate ran on the `c5732da` tree, the same day, same box** — `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --message-format=short -- -D warnings` clean, `cargo test --all-features --workspace` exit 0; **no total was taken**, the run was read through a filter and this row's own rule is never to total a gate from filtered output. The commit adds three tests to `fah-http --lib` (`tls_server`: a stalled hello, a client closing before its hello, non-TLS bytes — each proving its feed event and that the counters still move) and moves one integration test off port 443: `an_unreachable_upstream_is_reported_within_the_hello_deadline` connected to `192.0.2.1:443`, and **from a LAN behind the 0.4.x steer every address on 443 is answered by the real FAH** — the dev box was handed example.com's certificate by TEST-NET-1 — so its "nothing answers there" premise became false the morning of the deploy; it uses 4443 now. Any other test that expects a port-443 connect to fail will break the same way on this LAN. **The earlier full gate ran at `9b55cbb`, 2026-09-17** — `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features --message-format=short -- -D warnings` clean, `cargo test --all-features --workspace` **1 662 passed / 0 failed / 12 ignored**, 62 suites, exit 0. Six more than `455ea36`, which is the count the four commits added: SP2's deadline test, S1's skewed-shard sweep, TODO 9's >64 KiB reply, the two rollup boot tests and the rooted-path acceptance test; R2's two rows extend an existing test. Incremental, not clean-process. **A first count of this run was taken through the `rtk` output filter and read 1 152 with 3 ignored — the filter drops whole `test result:` lines, so never total a gate from filtered output.** The `9b55cbb` review file carries that wrong "3 ignored" figure. **The earlier gate at `455ea36`, 2026-09-16** — `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --message-format=short -- -D warnings` clean, `cargo test --all-features --workspace` **1 656 passed / 0 failed / 12 ignored**, 62 suites in 128 s. Identical to the `52eec09` total, as expected: the two commits since touch only `docs/` and `plan/`. The run was taken with the `455ea36` plan edits in the working tree and committed unchanged, so the figure holds at that commit. Incremental, not clean-process. **The earlier gate at the then-tip `52eec09`, same day, same box** —  `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --all-features --workspace` **1 656 passed / 0 failed / 12 ignored**, the same total as `af5cf61`. That commit changes no `crates/*/src` file; it adds a bench, its `[[bench]]` entry and a documentation edit. **The bench does execute under `cargo test` and still adds nothing to the count** — a `harness = false` bench target defaults to `test = true`, so criterion runs each benchmark id once in test mode (28 `Testing …/Success` lines) and emits no libtest `test result:` line. Execution surface changed, counted surface did not; do not read the unchanged 1 656 as the bench being skipped. This run was incremental, not clean-process. **The clean-process gate ran at `af5cf61`, 2026-09-16, Windows dev box, after `cargo clean` removed 466 GiB** so there is no incremental-cache ambiguity: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --all-features --workspace` **1 656 passed / 0 failed** (1 652 at `9cbbfd1`, 1 648 at `26ffe1c`). The `QueryType` redesign (`af5cf61`) added four tests: `fah-dns --lib` is at 225 (224 before) for `an_unnamed_type_carries_its_wire_code`, plus one drift guard in `fah-rules` (`every_named_query_type_carries_the_bit_its_rule_spelling_parses_to`, which pins the enum's indices against `rrtype_bit`'s table so the two vocabularies cannot silently diverge) and serde/round-trip cases in `fah-model` and `fah-api`. The `slots` fix (`f351980`) added none — it gates a field, and both configs are proven directly: `cargo clippy -p fah-api -- -D warnings` (no `test-harness`) now exits 0 where it failed on `field slots is never read`, and `--features test-harness` stays clean with `close_admission` still reading the field (`api.rs` 138 passed). **That warning was masked by the workspace gate** — `--all-targets` feature-unifies `test-harness` in, `close_admission` reads the field, the warning vanishes; the isolated default-feature build is the only place it shows, the same masking class as a dead test helper. Earlier and still current: the poison test `a_shard_poisoned_by_a_panicking_sweep_still_serves_and_stores` (falsified — restoring `.unwrap()` fails it on `PoisonError`); `fah-http --lib` 92; `fah-common --lib` 58. The dashboard figures below were measured at `9d9d792` and were not re-run for a change that touches no frontend code. From the earlier passes and still current: the listener fix added twelve tests, R1 one and F14 two, so `fah-config --lib` is at 98 (97 after the listener fix, 90 before it) and `fah-api --test api` 138 (133). Both of the last two fixes are mutation-verified: for R1, deleting the two new arms fails its test on `UnknownEnvKey` and pointing one arm at the wrong field fails it on the value; for F14, reverting the atomic fails the behavioural test while deleting the `post_config` call fails the wiring test and leaves the behavioural one passing — which is the proof the two cover different halves. `shipped_path_e2e` now carries six tests in 21 s — four added on 2026-09-15 for the HTTPS listener's own edge cases, the last of them mutation-verified (`max_connections: 2` fails its negative control). The dashboard ran on the same tip and is green: `npm run typecheck` clean, **58 test files, 1 043 tests, 0 failures** (three fewer than the 1 046 of 2026-09-14 — the `fallback` mode's tests went with the mode in `5b5d3e8`), and `npm run build` is under budget at 136 921 B gzip against 153 600 B. `cargo bench -p fastadhunter --bench pipeline --no-run` builds for the first time since p5-04, with no profile override (`3a1b5b8`). The `e2e` `WSAEACCES` trap (§Known-good gate note) did not fire. Bench A/B against `main` over four alternating rounds: **no regression demonstrated** |
| Version | **0.4.3, committed and untagged** — `fe73cec` (2026-09-21) raised `[workspace.package]` from the 0.4.2 that `25d7582` set the same day, which had itself come from the 0.4.1 of `a437bec` on 2026-09-19; one line plus the lockfile refresh, nothing else, each time. **`v0.4.0` → `4fec917` and `v0.4.1` → `a437bec` exist, `v0.4.2` and `v0.4.3` do not** (`git tag -l 'v0.4*'` is the read), so a build from the tip is identified by commit, not by tag. `cargo metadata` counts 13 workspace members (12 under `crates/` plus `tui-monitor`); all inherit the number. No test pins the version string. **The install guide still names 0.4.0** — [0.4.0-install.md](0.4.0-install.md): tar, container name, root-dir, the expected `"version":"0.4.0"`, a `git checkout v0.4.0` — and the deploy went out under those names with the 0.4.1 tar, so the router's container is called `fastadhunter-0.4.0` and `/health` says `0.4.1` (see Deployed); running the guide's chapter 1 literally would now build the *older* 0.4.0 and revert `docs/fastadhunter.toml` to its `no_sni = "block"` / IPv4-only-API version. `deploy-rb5009.md` and `public-certificate.md` still say 0.4.0 too. Older trap: **`be54185` was titled "version 0.4.0" and bumped nothing**, it changed only `docs/fastadhunter.toml` |
| Deployed | **0.4.1, live since 2026-09-21T05:37:31Z (08:37:31 local).** The owner ran [0.4.0-install.md](0.4.0-install.md) chapters 3–9 that morning. Image `kingston/fastadhunter-arm64-0.4.1.tar` (16.2 MiB, built on the dev box 2026-09-19 14:58 local, 36 minutes after the `a437bec` tag and before the next commit, so **the build is `a437bec` = `v0.4.1` by timestamp** — `/health` answers `0.4.1`), container **`fastadhunter-0.4.0`** (the guide's name, kept), root-dir `/kingston/fastadhunter/root` reused after the 0.3.4 container was stopped and removed at 08:35 local, `veth1` / `172.17.0.2` + `fd6c:7f32:8e91:1::2`, mounts `fah-config,fah-data`, `fah-env` unchanged (N=2 and the three mimalloc knobs). Config is the repo's [fastadhunter.toml](fastadhunter.toml): `mode = "dns+http+https"`, DoT 853 and DoH `/dns-query` on, `[https.listen]` 8444, **`no_sni = "pass"`**, `api.address = "::"`. Boot log: DNS on `[::]:53` udp+tcp, HTTP `[::]:8080`, HTTPS SNI `[::]:8444`, API `[::]:8443`, `DoT serves the API certificate`, an empty interception store written. **Router side, verified read-only after the run:** the two QUIC rejects sit in front of `FastTrack` / `fasttrack IPv6`; `dstnat` has the skip-list accept and the 443→8444 dst-nat for IPv4, the skip6/lan6 accepts and the dst-nat for IPv6, all behind the port-80 rules; the wildcard certificate covers `fah-dot.localbox.ro` and both names resolve to `172.17.0.2`. Chapters 12 (port-80 v6 rule on `BRIDGE`) and 13 (external DoT reject) were **not** applied — optional; zero connections to port 853 were leaving the LAN when checked, and the proxy resolves IPv4 first so the port-80 v6 self-steer only bites on an IPv6-only origin. Chapter 11's AAAA records are not added; chapter 10 is not recorded here. **First casualty, fixed within twenty minutes:** the heating gateway `192.168.10.15` (`Comunicator centrala`, `B8:2C:A0:0F:83:31`, IPv4-only) lost its Azure cloud link under the steer and released/renewed its DHCP lease every 100 s from 08:40:52 to 08:59:12 local; two `dstnat` accepts placed before the steer (IPv4 by `src-address`, IPv6 by MAC) took it out, and the guide's device-goes-quiet section records them (`5a4d684`). It talks to three Azure addresses on 443 every ~20 s, resolves DNS only on reconnect, and produced **no** feed event while it was being cut — it died on one of the SNI proxy's two silent paths (non-TLS bytes or hello timeout; `non_tls` reached 192 in that window but keeps rising from other clients, so the count is not attributable to it). Which path is unproven; a 30 s sniffer capture of its port-443 traffic would settle it and was proposed, not run. That silence is what `c5732da` fixes, and **the running build does not have it**. **The 0.3.4 soak's named mechanism is now fixed in production:** `8941770` is in this build, so the residual is to be re-measured from this boot (t0 `2026-09-21T05:37:31Z`, §0.3.4 soak); `p3-11`'s third precondition is met. The p3-06 probe (`fah-probe` on `veth3` / 172.17.0.4) stays torn down; `veth3` remains |
| Build ≠ tip | the running 0.4.1 is **`a437bec`** (`v0.4.1`) by build timestamp — it **has** everything the 0.3.4 lacked: the F11 supervisor, F3's query borrow, the H1-H3/D1 allocation removals, **the idle upstream pool reaper (`8941770`)**, the refusal-counter split, the whole of Phase 3 (certificates, SNI filtering, the interception machinery, DoT, DoH) and the 2026-09-17 audit closures. It **lacks** eight production commits: `90a208a` (log messages kept ASCII for the RouterOS log — cosmetic, in `/log` only); **`c5732da`** (feed events for HTTPS connections closed before a hello — the visibility the heating incident showed was missing), with `4f49a9e` and `0e49ce9` putting that status on the dashboard feed and in tui-monitor; `cafd059` and `e7e905e` (health-page layout, the `c-share` rename); **`6350fc1` (Fix 1 — the client registry stops growing: idle expiry, `seen_within`, the 24 h clients page)**; and **`5c27cbe`** — the dashboard served from the image's `/web` still misses a restart that the peak alone hides, still judges the residual by mean over every row, and still averages in the over-accounted rows its own chart blanks. The other commits since the tag are docs, plan files, the PowerShell script, the soak collector and the owner's config backups. **A silently cut device is therefore still invisible on the router today, and the registry there still grows ~19 addresses a day**; the DHCP log is the only tell until the next build. **The version bumps change nothing here** — 0.4.2 (`25d7582`) and 0.4.3 (`fe73cec`) rename the next build; only installing one closes this row |
| Phase | **2.7 opened 2026-09-21 in `plan/open/phase2.7-device-identity`** — the implementation of [ADR-0010](decisions/0010-device-identity-from-routeros-rest.md): a client's durable identity is its MAC, read from the router over REST, read-only, on demand (pending-set poll on the 20 s policy tick, full refresh every 10 min, one poll per tick, whole-poll timeout), with address and device lifetimes kept separate and both caps explicit. The ADR is approved conceptually and closed; six tasks `WAITING`; the plan ([dev-plan.md](../plan/open/phase2.7-device-identity/dev-plan.md)) awaits the owner's review before task files are split and `p2.7-01` starts. Nothing built, the router untouched; every step ships behind `[routeros] url = ""`, so the deployed behaviour changes only at step 6 with two owner-run router commands. **4 is revived as built-dormant, owner decision 2026-09-19 (`c35f7b8`)**, superseding the 2026-09-15 park in [ADR-0009](decisions/0009-phase-4-parked.md): all five tasks are to be built and shipped behind `[html] enabled = false`, switchable at runtime, with nothing changing on the deployed box until the owner turns it on. **Nothing is built yet** — the five task plans exist, every task is `WAITING`, `lol_html` is not in the build, and the folder stays in `plan/open/` until Phase 3 closes and the selector reaches it (never moved `open` → `wip` without the owner). ADR-0009's evidence still describes the deployment — interception is off and the deployed lists carry 720 URL rules and no cosmetic ones — which is why the default is off; switching it on later needs both interception for real clients and cosmetic lists loaded. **0–2.6 and 5 closed** (2.6 closed 2026-09-07, all 13 tasks `DONE`; `p2.6-12` reached `main` on 2026-09-11 as the cherry-pick of `fa9451a` — `adaptive` is the only strategy, the `fallback` walk is deleted, a config naming it fails at load). **3 is now in `plan/wip/phase3` on `main`** — the `open` → `wip` move landed with the merge, by the owner's decision (plan-merge.md §Step 3). `p3-01`…`p3-05` `DONE`, `p3-06` and `p3-06b` `PARKED` (2026-09-13, the interception decision), `p3-07`…`p3-09` `DONE` (ADR-0008: Interception Document, 525 classification, rejection view + editor). **`p3-10b` and `p3-10c` are `DONE`, both 2026-09-14** — the DoT connection gauge (`counters.dns_dot_connections`, counted at accept so a stalled handshake is in the figure that sizes the cap) and the HTTP, HTTPS and API acceptors reporting an unplanned end through `record_task_death`. Row 11's soak waits for neither. `p3-10` and `p3-11` stay `WAITING` — the only two the selector will pick. **`p3-11` gained a third precondition at `455ea36`: the deployed build must carry `8941770`.** A seven-day verification soak on a build with a known unbounded floor measures the leak, not the phase, and its RSS budget row would fail on a defect already diagnosed. The commit is an ancestor of `main` today, so a build from the tip satisfies it — **and the deployed 0.4.1 carries it since 2026-09-21, so the precondition is met.** The same commit retired the soak gates in six phase-3 plan files — "cannot start before the 0.3.3 soak ends 2026-09-16", "nothing deploys until the 0.3.3 soak verdict" — struck through with the outcome recorded rather than deleted. **Stopping the soak also released `p3-06-after`'s load arms**, held only because a flood would invalidate a running soak; that task is `PARKED`, so the release changes nothing on its own. **N3 follow-up closed 2026-09-11 as a technical experiment, not promoted** — the client alert names the TLS stack, not the cause; HTTP/3 must be refused for intercepted clients ([p3-06-n3-alert-ab.md](code-review/phase3/p3-06-n3-alert-ab.md)) |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): §5.1–6 cleared; §5.7–14 gate Phase 3. S1-G2 tiers 1–3 met; S1-G4 and S1-G5 route 2 not validated and will not be |
| **Next** | **Review the phase 2.7 plan** ([dev-plan.md](../plan/open/phase2.7-device-identity/dev-plan.md)), then split it into task files and start `p2.7-01` (`fah-model`: `MacAddr`, `ClientSelector::Mac`, the poll-failure counter). Each step is committed after its own review and leaves the deployed behaviour unchanged; step 6 needs, owner-run, the `fastadhunter` user in the proven `monitor` group restricted to `172.17.0.2/32`, `www-ssl` accepting `172.17.0.2/32`, the `localbox-ca` PEM and a one-line password file in `/config`. **Next build, 0.4.3:** the version is bumped and pushed (`fe73cec`) and **the ARM64 tar is built** — `fastadhunter-arm64-0.4.3.tar` in the repo root, exported 2026-09-21 17:44 local from that tree, arm64, carrying `/fastadhunter`, the seed `/config` and `/data` and the rebuilt `/web`. **`buildx -o type=docker` wrote an OCI archive** — the layout RouterOS hangs on at `status=extracting`, 7.25 MiB, layers under `blobs/sha256/` — so it was converted with skopeo as [deploy-rb5009.md](deploy-rb5009.md) §1 prescribes; the file in the root is the converted one, 16.24 MiB, `manifest.json` listing `<hash>.tar` layers, and the raw archive was deleted so it cannot be uploaded by mistake. The size is the check: about 16 MiB is legacy and ready, about 7 MiB is the OCI archive that will hang. It is neither uploaded nor installed, and the guide's names still say 0.4.0, so the deploy would run it under `fastadhunter-0.4.0`. That tar is the one where the registry on the router stops growing, a device cut on 443 shows up in the feed by name, and the memory page's verdict stops being fooled by a restart or a transient; until it is built and installed, §Build ≠ tip holds unchanged. **The 0.4.1 residual soak is running** — t0 `2026-09-21T07:14:50Z`, the build's second boot after a router reboot (`e21a4f2`: RSS 58.73 MiB, accounted 26.25, residual 32.48 at uptime 157 s), hourly collector, the same reducer and same-hour-of-day floors as [soak-0.3.4/README.md](code-review/phase2.6/soak-0.3.4/README.md); a first read at h48 says whether the floor still climbs, and only that build-to-build comparison can close §0.3.4 soak. **Heating gateway, optional:** a 30 s router sniffer capture of `192.168.10.15` port 443 to learn which silent path cut it (its hello now goes straight to Azure, so the capture shows exactly the bytes FAH saw); the exemption stays either way. **Guide follow-up, needs a yes:** [0.4.0-install.md](0.4.0-install.md) still says 0.4.0 and `git checkout v0.4.0` while the router runs the 0.4.1 tar under 0.4.0 names; chapters 1–2 are done, 12 and 13 remain optional — 13 breaks any device pointed at a public DoT resolver until it is re-pointed, and none was found using one; 12 is cheap and only matters for an IPv6-only plain-HTTP origin. **Unblocked by the deploy:** p3-10's B2 sweep on the RB5009 and p3-11's device half — its precondition (the build carries `8941770`) is met — and Private DNS on the phone (chapter 10, `fah-dot.localbox.ro`, the wildcard certificate verified against it). The DoT/DoH load generator and the splice-RSS runner still wait on p3-11 fixing the budgets on the device (PERFORMANCE.md's `DoT / DoH added latency vs UDP, p50` is still `TBD`; the TLS/HTTP legs do not convert from x86); Finding 8's listener-layer oracle still needs a production visibility change first. **Dated:** the Cloudflare token is empty and the public certificate's renewal window opens 2026-11-08 (§Public certificate) — that certificate now backs DoT in production, so an expiry stops Android Private DNS silently rather than warning in a browser. Interception ships **disabled** through an empty client scope (`interception: null` in the live config), by the owner's 2026-09-13 decision; the rejection view is wired, tested and idle until a client is enrolled. Off the critical path, unchanged: dashboard settings metadata for `runtime.http_runtimes` (a deliberate omission) and alloc 11b (`Connection::graceful_shutdown` into `Proxy::serve_connection`) |

## Audit closures and the top-3 audit — 2026-09-17, four commits

Three review files carry the evidence; only what they cannot say lives here.

- **`b0c7709`** closes SP2, N2 and R2 of
  [post-merge-audit-2026-09-15.md](code-review/phase3/post-merge-audit-2026-09-15.md).
  SP2 was the one owner-decision item left from that audit's second pass: the
  DoT leaf pre-warm now sits inside the same `timeout_at` as the handshake, so
  a stalled mint no longer holds one of the 64 DoT slots for as long as the
  blocking pool takes; on expiry the connection closes like any handshake
  timeout and a mint already dispatched still lands in the store. N2 is the
  three DNS listener gauges on the Health page's Backpressure card, active /
  peak plus the UDP shed count. **SP8 is now the only open item from that
  audit**, and R3–R7 stay notes.
- **`067427a`** closes S1, S3, S7 and TODO row 9 of
  [hot-path-audit-dns-http.md](code-review/phase3/hot-path-audit-dns-http.md).
  S1 is the one worth carrying: after `3ce7ec5` a panic under the shard lock
  no longer kills the shard, but a panic mid-`retain` left `Shard::bytes`
  over-counting for the life of the process, so `over_bounds` evicted earlier
  than `byte_capacity` allows. `clean` now sums the entries it keeps during
  the walk it already makes, so any skew heals at the next sweep. **Rows 8 and
  10 are the audit's whole open surface.**
- **`9d51567`** —
  [audit-closures-2026-09-17-review.md](code-review/phase3/audit-closures-2026-09-17-review.md)
  reviewed the two commits above: no defect in either, every gate green. One
  doc figure corrected: both audits had recorded the vitest run as 625 passed
  where the suite reports 1 056.
- **`9b55cbb`** —
  [top3issues-review.md](code-review/phase3/top3issues-review.md) is a
  whole-repository read of every production Rust line outside inline
  `#[cfg(test)]` modules (36 654 of 95 064 tracked lines; inline test
  modules, `tests/`, `benches/` and ~4 000 lines of `tui-monitor` unread),
  its same-day verification and the review of the fixes, folded into one file.
  Three defects, each contradicting its own doc comment; the audit's
  high / medium / medium became medium / low / low on verification. **V1
  fixed:** `POST /api/v1/lists` rejected `..` only, and `Path::join` replaces
  the base when the argument is rooted, so `{"path": "/config/auth-hash"}`
  became a list the engine opened on every refresh — the guard now also
  rejects a rooted path unless it starts with the data dir, and the documented
  `/data/lists/local.txt` form stays accepted. **V3 fixed:**
  `RollupWriter::boot` read only the newest day file, so an empty one reset
  the cursor and the next tick re-appended up to 23 restored hours, which the
  history summary double-counted; `boot` now walks the day files newest-first
  until one yields a row. **V2 deferred by the owner's decision:** `is_fresh`
  counts a future-stamped slot as fresh after a backward clock step; the only
  fix on the table trades one wrong reading for another and the skew clears
  within 24 h either way. Nothing owed until a backward step is observed.

## Deploy guide — 2026-09-17, written from the live router; executed 2026-09-21

**Executed 2026-09-21** — chapters 3–9 ran as written, with three deviations: the tar
was 0.4.1 (on the router since 2026-09-19), the container kept the guide's 0.4.0
name and reused `/kingston/fastadhunter/root` after the 0.3.4 container was
removed, and the heating gateway needed the device-goes-quiet exemption within
twenty minutes (§Deployed). Two corrections landed the same morning: the rollback
restores the TOML *before* 0.3.4 starts (`4408a43`) — 0.3.4 rejects the `dot_*`,
`doh_enabled` and `[https]` keys and exits, so the old order left the LAN without
DNS — and the exemption rules are recorded (`5a4d684`). Chapters 1–2 were already
done, 11–13 were not run, 10 is not recorded here. The guide still says 0.4.0 and
`git checkout v0.4.0` throughout; chapter 1 as written would now build the older
version.

[0.4.0-install.md](0.4.0-install.md) holds **only what is missing** between the
running 0.3.4 and 0.4.0. Everything in it was read off the RB5009 read-only that
day; **nothing on the router was changed**, and the guide is the owner's to
execute.

What is already in place and must not be redone: the 0.4.0 tar, the mounts and
envlist, the Let's Encrypt pair, port-80 steering on both families, and the
outbound plain-DNS blocks. What is missing is the config swap, the container
add/stop/start, a QUIC reject, the 443 dst-nat on v4 and v6, and a fix to the
existing port-80 v6 rule.

Three things the guide establishes that are not obvious:

- **`fah-lan6` is dynamic.** `/ipv6/dhcp-client` on `DIGI` carries
  `prefix-address-lists=fah-lan6`, so it follows the ISP prefix by itself.
  Nothing in the new rules names the prefix — the only prefix-dependent match is
  that list.
- **The v6 rules match `in-interface=BRIDGE`, not the `LAN` list.** `CONTAINERS`
  is in `LAN` and the container holds a SLAAC global on that link, so a
  list-matched rule steers the container's own egress back into its own
  listener. The existing `fastadhunter http v6` rule has this defect today and
  the guide fixes it. Matching the interface is also prefix-proof: the SLAAC
  address moves, `BRIDGE` does not.
- **Stopping the container takes the whole house off DNS.** The router's own
  `/ip/dns servers=172.17.0.2`, the v4 and v6 dst-nats of port 53, and the RA
  `dns=` all lead to this one container. Hence add-then-stop, so the 16.2 MiB
  extraction happens outside the window.

Its dev-box commands are **PowerShell**, tested on 5.1. `curl` is an alias there
and must be spelled `curl.exe`; `openssl` needs `'' |` to close stdin; binary
must go through a file rather than a pipe. The API takes
`Authorization: Bearer`, not an `x-api-key` header.

## Phase 3 verified — 2026-09-17, by execution

The phase-3 task files claim SNI filtering plus DoT and DoH. That was checked
against the code and then **run**, on the Windows dev box:

| Suite | Result |
| ----- | ------ |
| `fah-http`, `fah-dns`, `fah-api` units | 570 passed, 0 failed |
| `e2e_https`, `security_phase3`, `shipped_path_e2e` | 18 passed, 0 failed |

`the_shipped_configuration_blocks_at_every_layer` is the one that settles it —
one test walking DNS/UDP, DNS/TCP, HTTP, a blocked SNI closed before any
ServerHello, an allowed SNI spliced with the **origin's own chain asserted**
("the assertion that says this build does not intercept"), DoT blocked and
allowed against a CA-minted leaf with a concurrency burst, and DoH blocked and
allowed on the API listener.

Confirmed by reading rather than assumed: `sni.rs` parses the ClientHello with
raw record constants and no TLS stack, bounded at 16 KiB; a no-SNI hello is
**closed either way** and `no_sni` only changes the classification; DoT hands off
to `tcp::handle_connection` so the framing and pipeline are shared; DoH reaches
`fah-api` as `Arc<dyn DnsWireSource>` with no `fah-dns` dependency in that
crate's `Cargo.toml`. The 8 ignored unit tests are network smokes against real
upstreams, client-side.

**Scope.** All of it ran on x86 against loopback origins. It says the code is
correct and complete; it says nothing about RB5009 throughput or household
traffic. That is p3-11's soak, still `WAITING` on the deploy.

## Public certificate — renewal corrected, automated, and blocked

[public-certificate.md](public-certificate.md) was dry-tested command by command
on 2026-09-17 and carries nine corrections. Two mattered:

- **`.vscode/cloudflare.token` is empty, 0 bytes.** The documented renewal reads
  it and fails part-way through DNS-01, *after* an ACME order is open — spending
  one of the five failed validations per hour. **It must be re-created before
  2026-11-08.** This is the one item here with a deadline.
- **Renewal never finished.** The guide stopped before copying the pair to
  `/config` and restarting the container. The restart is unavoidable:
  `ApiServer::bind` builds its `TlsAcceptor` once
  (`fah-api/src/server.rs:59-74`), there is no `ResolvesServerCert` and no swap
  cell, so the loaded pair is fixed for the life of the process — and that
  applies to `POST /api/v1/certificates/import` as well.

Dates: the pair runs 2026-09-09 → **2026-12-08**; lego's own window opens
**2026-11-08** (one third of lifetime, so nothing to track by hand). Renewal is
four times a year and nothing in FastAdHunter will ever do it — an ACME client in
the binary would need a Cloudflare zone-edit token on the router, a worse trade
than a quarterly task.

`scripts/renew-certificate.ps1` automates the dev-box side for Task Scheduler.
Its chain gate is a real `openssl verify` against the pinned ISRG Root X2, not a
substring match: the first version searched for the string, which **appears in
the wrong chain too**, so it accepted both. The default run never touches the
router; `-Deploy` is opt-in because it restarts the resolver. Tested paths:
`-CheckOnly` → 0, `-CheckOnly -WarnDays <high>` → 2, empty token → 1 at
preflight, and `-Deploy` also stops at preflight. The lego call and the
scp/restart are untested by construction.

**Two schtasks defaults are wrong for this job**, measured by creating and
deleting throwaway tasks: `LogonType = Interactive` (no run unless logged on) and
**`StartWhenAvailable = False`** — a missed run is never made up, so a machine
off at 04:00 on the 1st silently skips that month. `ExecutionTimeLimit` is
`PT72H` and needs nothing.

**Open and unresolved:** `_.localbox.ro.json` records
`"preferredChain": "ISRG Root X1"`, which is not what the guide prescribes. The
files on disk are right — the delivered chain reaches ISRG Root X2 and verifies
against it. Found by dry-running, not by a failure. Since `--preferred-chain`
falls back **silently** when it matches nothing, the next renewal must not be
assumed to reproduce this chain.

**Toolchain change.** OpenSSL is now installed natively — ShiningLight 4.0.2 via
winget, at `C:\Program Files\OpenSSL-Win64\bin`, appended to the user PATH by
hand because winget does not. It ships **no CA bundle**, so anything verifying a
chain needs `-CAstore 'org.openssl.winstore://'`. Before this it was reachable
from Git Bash only.

## 0.3.4 soak — stopped on day 5 with a memory finding

**Superseded in production on 2026-09-21:** the build now running carries
`8941770`, the mechanism this soak named; the 0.3.4 run is that fix's "before"
arm on the device and the "after" is unmeasured — re-measure from t0
`2026-09-21T05:37:31Z` (§Next). On the 0.3.4 build the residual floor kept
climbing after the soak stopped, 23 → 42 MiB from 2026-09-12 to 2026-09-21 by
daily minimum of `history/perf`, while `stats_clients_bytes` held at 1 034 KiB
throughout — the rotating IPv6 addresses in `/clients` are ruled out as the
cause. The client registry is capped at 4 096 entries and evicts the
least-recently-seen unnamed client first; at the cap it costs ~8.5 MiB, not the
4 MiB the number suggests, because hashbrown rounds 4 096 entries up to 8 192
slots.

[soak-0.3.4/README.md](code-review/phase2.6/soak-0.3.4/README.md) §Interim
analysis — day 5 holds the numbers, the method and the retraction. Only what it
cannot say lives here. `reduce.py` recomputes every figure from `pulls/` alone.

**The finding.** Across h0–h113.9 the residual floor **doubled** and stayed
elevated across every 12 h bucket, while everything the process can account for
stayed still:

| Floor, MiB | h0–12 | h96–108 |
| --- | --- | --- |
| Residual | 19.0 | **38.6** |
| RSS | 44.6 | 66.2 |
| `accounted_bytes` | 25.59 (min of 1 140 samples) | 28.00 (max) |

Same-hour-of-day floors, which remove the daily traffic cycle, rose +1.3, +5.3,
+8.2 MiB/day on the quiet window and +2.2, +4.3, +6.3 on the busy one — **still
accelerating when the run was stopped**, not settling. The named mechanism is
`8941770`, the idle upstream-pool reaper, which is **not** in the 0.3.4 build;
its retention lands wholly in `residual_bytes`, which is the shape observed. Its
size on ARM64 is **unmeasured** — the dev-box A/B is +18.23 MB against +4.26 MB
and nothing has carried it on this device. 0.3.4 is that A/B's "before" arm.

**A finding was published and retracted the same day.** An allocator-retention
claim was built on `allocator_committed_bytes` being monotone across 1 140
samples. That is a property of the counter, not of the memory:
[memory.rs:98-112](../crates/fah-model/src/memory.rs) documents that mimalloc v3
never decrements `current_commit` on purge, and says in terms "do not subtract
anything from this field and present the result as retention".
`current_commit == peak_commit` in 116 of 116 pulls is that signature. The
refresh-step ratchet it shadows was measured two phases ago —
[p2-11-compile-transient.md](code-review/phase2/p2-11-compile-transient.md),
saturating at ~230 MiB, cut to 181.4 MiB by `MIMALLOC_PURGE_DELAY=0` — so it is
bounded. **The repo had answered the question before the soak began.** Finding 4
is unaffected: it is measured on RSS from `/proc/self/status`.

**`memory-high` sizing, corrected.** 155.5 MiB of observed `peak_rss` plus a
69.5 MiB container offset ≈ 225 MiB is this run's *ratchet position*, not where
the ratchet stops. p2-11's saturation is 181.4 MiB post-fix on a larger corpus,
so ≈ **250 MiB** with the offset. **Size any limit from a measured saturation
point on the current corpus, never from a soak's high-water.** The container is
`memory-high=unlimited` and that is why nothing died.

**Two collection problems, one fixed.** `collect-soak.py` wrote the *last*
`memory-current=` in `/container/print detail` into `meta.json` with no container
filter, so the one pull where a second container (`fah-diagprobe`) was running
recorded 33.9 MiB against the FAH container's 135.7 — silently, with
`meta.errors` empty. Fixed at `ad795b0`: `own_container_memory()` selects the
`fastadhunter-*` entry and a no-match now lands in `meta.errors`. Replayed over
all 116 stored pulls, 116/116 match, one value changes. The container-offset
series still needs re-deriving with that pull repaired.

**Everything else was clean in 113.9 h** at 0.164 % of one core: no crash, no
restart, no shed, no eviction, no SERVFAIL synthesis, no listener saturation, no
container `WARN`/`ERROR` line and no config drift. `adaptive` handled its first
real failover exactly to specification — one run of 4 consecutive failures on
1.1.1.1, penalty armed, 24 s skip, probe, restore, and no SERVFAIL reached a
client. Upstream accounting reconciles to the query: attempts 64 566 =
`cache_misses` 5 951 + `swr.completed` 58 607 + 8 failover retries, and 8 is the
whole failure count.

## Second hot-path audit — 2026-09-16, eight items resolved

[hot-path-audit-dns-http.md](code-review/phase3/hot-path-audit-dns-http.md)
holds the findings, the measurements and the debt register, with a status
column that moves as fixes land. Only what that file cannot say lives here.

**Three audit files now overlap, and the two older ones differ by one word:**

| File | Scope | Labels |
| --- | --- | --- |
| `post-merge-audit-2026-09-15.md` | merge surface, defect hunt, listener sweep | N1–N3, SP1–SP8, R1–R7 |
| `post-merge-performance-audit-2026-09-15.md` | hot path, SNAPSHOT, no diff | A1–A6 |
| `hot-path-audit-dns-http.md` | the same discipline inside the listeners | 1–10 |

**That file's `F1`–`F4` are the older audit's `A2`, `A5`, `A3` and `A6`** — its
brief relabelled them as already-filed. A bare `F3` there means the TCP/DoT
length-prefix realloc, not any `F3` from the risk inventory. Three numbering
spaces over one body of code: read every label with its file.

Fixed by a change and pushed: **`3ce7ec5`** Finding 1 — a poisoned shard
answered no query on it ever again, silently, and `record_task_death` could not
see it because the sweep continues rather than ends; **`c93e2d1`** Finding 4;
**`bb15de7`** Finding 5; **`f51a830`** Finding 7; **`af5cf61`** Finding 9 — the
`QueryType` redesign (`Other(String)` → `Copy` `Other(u16)` + named variants),
removing the per-query allocation for every non-A/AAAA type. Separately,
**`f351980`** gated the masked `slots` warning to `test-harness`.

Closed by measurement on the RB5009 with **no** code change: **2** and **3**. A
diag-timing probe (`fah-diagprobe`, veth3, `172.17.0.4`, production on veth1
untouched) drove both. **Finding 2** — an adversarial ClientHello (~4051 tiny
records) up to 32 concurrent slow-drippers, 16× the two allocation-domain
threads, moved HTTPS-peek and HTTP-proxy p95 not at all; the 16 KiB
`MAX_HELLO_BYTES` cap, `hello_timeout` and `max_connections` neutralise the
rescan amplification, so its HIGH severity did not survive the device.
**Finding 3** — the `spawn_blocking` round trip is `dispatch_wait_us` ≈ 62 µs
warm, ~4.5 % of the recorded 1.389 ms DoT budget miss; the inline-cache fast
path was rejected on the number, not deferred. **Finding 9** was also validated
on the same probe (every `QueryType` arm correct; a frozen-cache A-vs-HTTPS
experiment showed the two indistinguishable in CPU and memory) — the dev-box
`forward_alloc` oracle carries the +0-allocation claim, the device carries
runtime equivalence.

Closed by measurement on the **dev box**, no code change, `52eec09`: the **TCP
length-prefix framing** at `tcp.rs:223` — TODO row 11, and `F3`/`A3` in the two
older numbering spaces. Two facts settled it, and the second is the one worth
carrying: hickory's encoder starts at `Vec::with_capacity(512)`
(`hickory-proto-0.26.1/src/op/message.rs:503`), so the realloc the finding named
fires only when a reply lands exactly on that capacity rather than on every
reply; and **the vectored replacement is slower than the code it would replace
below ~1 KiB** — tokio builds a `[IoSlice; 64]` array per write
(`tokio-1.53.1/src/io/util/write_all_buf.rs:50`), a flat ~16.4 ns that exceeds
the memmove of a 500-byte reply at 14.5 ns. Real reply sizes cost 13–16 ns,
an order of magnitude under the 1 µs screening gate; the worst case measured is
413 ns at the 16 KiB `MAX_MESSAGE_LEN` ceiling with exact capacity. The
mechanism question — whether `write_all_buf` produces a real vectored write on
this stack or degrades to two writes — was answered from the three sources
rather than assumed: tokio dispatches on `is_write_vectored`, and both
`TcpStream` (`tokio/src/net/tcp/stream.rs:1503`) and `tokio_rustls::TlsStream`
(`tokio-rustls-0.26.4/src/common/mod.rs:348`) return `true`.

`crates/fah-dns/benches/frame_reply.rs` is **kept rather than deleted**: the
argument for not adding complexity to `tcp.rs` needs its evidence to stay
runnable, and `cargo bench -p fah-dns --bench frame_reply` reproduces every
figure in minutes. It is the first bench in the tree written to justify a
*refusal* rather than to guard a budget.

**2026-09-17, `067427a`:** S1, S3, S7 and TODO row 9 closed — §Audit closures
above. Open: **8** and **10** carry no date, and **8 is not the test-only item
it looks like** — `udp` and `tcp` are private modules exporting neither `run`
nor their listener traits, so the oracle needs a production visibility change
before it can be written.

**Standing decision, Finding 10.** PERFORMANCE.md's allocation-free hot path is
the invariant and the code is in debt against it. Narrowing the rule to the
matcher was proposed and **rejected**: the register is worked down one row at a
time, and the invariant does not move to meet the implementation.

The `#[cfg(test)]` cut trap recurred in the very pass that documents it —
`cache.rs` carries the attribute on two methods at 675 and 685, and its test
module starts at 862.

## Session 2026-09-14/15 — ten commits, all on both remotes

Newest first. Everything here is test, harness or documentation except
`2583c27`, which is the only production change of the session.

| Commit | What |
| --- | --- |
| `b495b05` | PERFORMANCE.md: the bench override is gone, the old absolutes are not. The recorded `full_pipeline` A/Bs used `CARGO_PROFILE_BENCH_DEBUG_ASSERTIONS=true` on both arms and stay fair; their absolutes stay ineligible as budget rows. Only runs from `3a1b5b8` onwards measure shipped codegen |
| `3a1b5b8` | `cargo bench -p fastadhunter` builds again, first time since p5-04. The dev-dependency no longer asks for `fah-api/test-harness` — cargo unified it into the bench profile, where `fah-api`'s `compile_error!` refuses it. `history_e2e.rs` is gated on the package's own feature instead. The security barrier in `fah-api` is untouched |
| `e028328` | p3-10 track B1: the two unrun DoT/DoH rows say what they wait on, not just that the tool is missing — the budget is `TBD` until p3-11 fixes it on the device, and x86 does not convert |
| `d4ab231` | ARCHITECTURE.md named `fallback` as the current upstream strategy. It has been rejected at load since 0.3.3 |
| `5b5d3e8` | The dashboard carried `fallback` as a live mode: a variant of `UpstreamMode`, three render branches, a prop gating half of every endpoint row, and a selectable settings value. All of it described a state no engine can report. `unknown` stays — it is also `GET /config` having failed |
| `2583c27` | **Production.** `fah-stats`' `append_line` wrote a record and its `\n` as two awaited calls, so an abort between them lost an hour of history; and it never flushed, so `tokio::fs`'s buffer could lose the line outright. The second defect was found by the test written to prove the first fix |
| `816dde7` | `adaptive_behaviour.rs` formatted. `d71ca0b` shipped unformatted because its gate used `cargo fmt \| tail`, and a pipe reports the wrong exit status |
| `7ab6645` | The DoT accept flake keeps its failure-only diagnostics, and its mechanism is now reproducible on demand in 0.48 s rather than once in ~810 loaded runs. Historical cause unassigned; see §Open, honestly |
| `b5355ca`, `533a180` | The `fah-dns --lib` flake that had been lost is named, and the handover replaced with the cause hunt |

### Open, honestly

- The **DoT accept flake**'s historical cause is **unassigned**, and stays that
  way until the instrumented assertion goes red again. The mechanism is
  reproducible and the production invariant is verified — `tls_handshakes == 2`
  passed in the recorded failure, so the pool opened exactly the connections it
  should. What is unknown is what opened the third one. Six hundred further
  runs under load found nothing;
  [allocation-oracles-that-only-hold-on-an-idle-machine.md](solutions/design-patterns/allocation-oracles-that-only-hold-on-an-idle-machine.md)
  §2 carries the verdict and the four readings its dial journal allows.
- ~~`fah-api`'s `server.rs` warns that `slots` is never read when the crate is
  built without `test-harness`.~~ **Fixed 2026-09-16 in `f351980`** — the field
  and its initializer are gated to `test-harness`, the only feature whose
  `close_admission` reads them. The workspace `--all-targets` gate had masked it
  by feature-unifying `test-harness` in; the isolated default-feature build was
  the only place it showed.

## Integration audit — status-pass 2026-09-14

[main-phase3-integration-audit.md](code-review/phase3/main-phase3-integration-audit.md)
was written 2026-09-08; p3-04…p3-09 and the merge have landed since, so every
finding was re-checked against the code rather than against the file (`a9e642d`).

Closed by the pass:

- **F1** — a documentation correction, no production code change.
  `allow_ip_literal_hosts` permits a direct IP-literal upstream connection on the
  HTTP proxy path **only**; an IP-literal SNI stays unsupported and is rejected
  at hostname resolution. CONFIGURATION.md claimed the switch governed both paths
  and now states the asymmetry. RFC 6066 forbids an IP literal in SNI and the
  switch is off by default, so the branch is deliberately not ported.
- **F2** and **F5** were already fixed by `8a5c809` on 2026-09-08, thirteen hours
  after the audit was written, and sat open for six days because nobody came back.
- **F6** closed by annotation: the stale section carries its own correction.

Still holding, none of them blocking — except F7, which closed on 2026-09-14:

- **F3** — recorded.
- **F7 — closed 2026-09-14** by `d71ca0b`. The `b5_recovery_and_flapping` p99
  guard was unrepairable, not merely weak: a capped penalty window is exactly
  one unit long, the flapping phase was 0.6 of one, so once the black-hole
  phase drove the backoff to its cap the window outlasted the phase on every
  arm and the comparison never exercised the property. It is replaced by a
  penalty-count oracle whose allowance is derived from `nominal_penalty_ms` at
  the 75 % jitter floor, all three arms run independently with their failures
  aggregated, and "insufficient samples" can no longer read as a pass.
  Mutation-verified: the oracle rejects the regression it is there to catch.
  The test keeps its `#[ignore]`; it is not a gate.
- **F8** — `tcp_max_connections` ships armed at 1024 while `udp_max_inflight`
  ships inert at 0 (confirmed on the live container, 2026-09-15). Inherited from
  `main`; no default was changed. **The inert bound also makes the gauge inert**,
  which F8 did not say: `admit()` returns before touching `active` or `peak`, so
  `dns_udp_inflight` reports `peak: 0` on a loaded build and that zero is not
  evidence. A ceiling that is off cannot be sized from its own telemetry, however
  long a soak runs ([measurement-traps.md](measurement-traps.md) §A metric can be
  present and not measuring, `99e4953`). No production task opened for it.
- **F9** — seven benches whose own-side spread on identical code is wider than
  the 10 % gate they are supposed to police, independently reconfirmed by the B1
  characterization on benches the audit did not cover.

`E:/fah-main-bench` is **kept**, detached at `ebc46f1`: the frozen pre-Phase-3
baseline p3-10 measures against. Rebuilt later it would be a different baseline,
not the same one. Do not switch its checkout or delete its `target/`.

### Hot-path audit and the A1/A2 fix — 2026-09-15

[post-merge-performance-audit-2026-09-15.md](code-review/phase3/post-merge-performance-audit-2026-09-15.md).
**Not the same file as the integration audit below**, whose name differs by one
word (`post-merge-audit-…`); the two are easy to confuse and cover different
things. This one is hot-path performance, memory and Rust quality, read-only,
SNAPSHOT mode — there was no code diff to review.

Its first pass was wrong and was rewritten over four commits. It had reported
"0 locks" and "0 panics" for files its own scope listed, because cutting each
file at the first `#[cfg(test)]` drops 187 lines of `cache.rs` production code —
that attribute sits on two individual methods long before the test module.
Anchor such a cut at column 0. It had also read passing allocation oracles as
headroom when the ceilings equal the measurements: 8 of 12 ceiling checks clear
by exactly the 4-allocation jitter allowance, so they are tight regression
detectors and nothing more.

Findings are labelled `A1`–`A6`, local to that file. Bare `F` numbers were not
available: the review registry already uses them for whole files
(`phase2.6/f2-udp-inflight.md`, `f3-name-alloc-attribution.md`,
`phase3/f7-flapping-oracle-redesign.md`).

- **A1, A2 — fixed 2026-09-15 in `26ffe1c`**, the only production commit of the
  set. The HTTP/HTTPS accept loop had no backoff, so descriptor exhaustion spun a
  core; and four `warn!` sites a client could drive had nothing limiting their
  rate. `RetryPolicy` moved from `fah-dns` to `fah-common` (hard rule 1 forbids
  `fah-http` importing `fah-dns`), and `LogThrottle` is new there.
- **The acceptor recovers rather than dying** — owner's decision. `Fatal` after
  40 consecutive errors is ~33 s and descriptor exhaustion outlasts that, so
  `accept_loop` uses `RetryPolicy::never_fatal()`. The three DNS listeners keep
  the old escalation. The signal is a throttled `warn` with a cumulative count,
  not a task death through `record_task_death`.
- **A throttle must not be per connection.** `DotTls` is `Clone` and is cloned
  once per connection; a throttle field there would have been the defect itself.
  Instances live on the connection gauges and on `Pipeline`.
- **A2's classification was measured, and narrowed as a result — `19693be`.**
  The four `ErrorKind`s had been chosen by reasoning about POSIX errno, with a
  test that asserted the reasoning back to itself and never touched a socket.
  `crates/fah-dns/examples/udp_send_errno.rs` drives `send_to` on an
  unconnected `UdpSocket`, the kind `udp::run` binds, and prints the raw errno
  beside the kind. **`ConnectionRefused` was not produced by that path under the
  tested environment** — three sends to a closed port returned `Ok`, and only a
  connected socket saw errno 111 — so it is gone from the classifier.
  `PermissionDenied` (errno 13, broadcast without `SO_BROADCAST`) and
  `NetworkUnreachable` (errno 101, no route) are confirmed and kept;
  `HostUnreachable` was never produced and is kept on plausibility, labelled
  unconfirmed. EMSGSIZE maps to `Uncategorized`, not to a named kind, so the
  audit's claim that `MessageSize` keeps its warning named a variant the probe
  does not produce — the behaviour was right by accident. Measured on x86_64
  static musl under WSL2, kernel 6.18; the mapping has no architecture-specific
  step so aarch64 is **expected** to match, and the routing rows reflect this
  host's routing table and carry nowhere. The example is checked in so the
  container can settle the ARM case where that answer actually exists.
- **A `Datagrams` wiring test now pins the call site.** The classifier and the
  throttle were each tested alone, and an inverted condition in
  `handle_datagram` would have passed both. A stub socket returning a chosen
  kind drives the real path, and the assertion reads the gauge's throttle count
  rather than captured log output. Falsified by inverting the branch — the two
  tests fail in opposite directions — then reverted.
- **Two A1/A2 items wait on the deploy, not on work here.** The `RLIMIT_NOFILE`
  fault injection that would show A1's actual benefit — CPU staying down and DNS
  still answering while `accept` fails — needs the container, so it belongs
  beside p3-11's security suite. And the errno probe wants one run on
  aarch64/musl. Neither is a unit test and neither is owed before a deploy.
- **A6 — fixed 2026-09-15 in `3e97d16`, and two claims under it withdrawn.** The
  hook cited "hard rule 20" for a rule CLAUDE.md numbers 7, left over from
  `plan/CLAUDE.md`'s old copy of the principles; it references the rule by name
  now. Withdrawn: that 8369 comment lines mean rule 7 "does not describe the
  tree" — a prohibition is not a description, and the existing comments predate
  it — and that the hook is too strict for rejecting an edit that carries a
  pre-existing comment through unchanged. **Hard rule 7 is not open for
  discussion.** A comment is an input cost paid on every read of the file, by
  every agent, in every session, against a one-time benefit. A rule that needs
  the model's judgement to apply ("2–3 lines where needed") was tried and
  eroded; a binary, machine-checkable one holds.
- **A5 — fixed 2026-09-15.** Hard rule 3 now reads "no allocations, no regex,
  and no locks the architecture does not already name", and requires an ADR plus
  a figure in a measurement file for any new hot-path lock. Two wordings were
  rejected: "no *contended* locks" describes the design more truthfully but
  needs the agent's judgement to apply, and "no *unjustified* locks" fails the
  same way — so judgement was replaced by two artifacts that either exist or do
  not. Removing the cache's lock was considered and declined; the trigger to
  revisit is a `try_lock`-failure count per shard under household traffic.
- **A7 removed, not fixed.** It described a bug in the audit recipe, which is
  neither performance, memory nor code quality, so it does not belong in a
  findings list. The fact survives in the audit's own header, because it is why
  the first pass reported "0 locks".
- **A3 closed on measurement 2026-09-16, A4 resolved procedurally — nothing
  from this audit is open.** A3 is the TCP/DoT length-prefix realloc, carried
  since as the second audit's TODO row 11. It was deferred under "measure before
  optimizing", the measurement was taken, and it closed with no code change —
  §Second hot-path audit above for the numbers. Its dominant-transport trigger
  is spent: the framing cost is 13–16 ns at real reply sizes whatever the
  transport mix, so Android Private DNS at scale would not revive it. The
  obvious fix stays invalid on top of being unnecessary — hickory emits
  name-compression pointers as absolute buffer indices. A4 is not a code item
  and gets no task: **every oracle citation carries observed / ceiling /
  headroom** — `832 / 836 / 4` — and says what it means, since a pass means no
  regression beyond the measurement allowance and not four allocations
  available to spend.

### `CLAUDE.md` working language is English — `e30af33`

Chat joins the artifacts; the rule required Romanian replies and the owner's
reason was that the translations read badly. The English-artifact list survived
the rule's removal, because it still forecloses an agent switching language for
a doc or a commit message on its own.

`§How to answer` item 1 gained the prohibition it was missing: write like a
person talking, never like a telegram — no dropped articles, no headless
fragments, no clipped noun stacks. **Cut sentences, not grammar.** It went into
that item rather than a new section, since the item already said "brief means
fewer sentences, not denser ones — ordinary words, complete sentences"; a second
section restating it is how the principles duplicated into `plan/CLAUDE.md`
drifted.

### The no-comments hook now covers the dashboard — `3e97d16`

`.claude/hooks/no-rust-comments.sh` gates `.rs`, `.ts` and `.tsx`. The dashboard
had never had a gate and sits at **15.7 % comment lines** (6342 of 40469)
against **9.7 %** in `crates` (8405 of 86829) — and the `crates` figure is
mostly pre-hook code, since the hook blocks new edits rather than cleaning old
ones. Single-line template literals are stripped before the scan: `socket.ts`
builds a websocket URL with a literal `//` inside backticks, which the old
string-stripping read as a comment. No new exemption was needed — the dashboard
has zero functional pragmas, so all 6342 lines are prose.

`.claude/hooks/no-rust-comments.test.sh` covers it: 14 cases, blocked / allowed /
out of scope, and falsified rather than trusted — dropping `.ts` from the
extension filter fails three, dropping the backtick stripping fails exactly the
template-literal case. The `MultiEdit` path had no coverage before.

**The existing comments are left alone, deliberately.** Each is either a
duplicate of a fact that already has a home — `api/types.ts` cites API.md for
the two traps it repeats, and both are there at `API.md:289` and `:784` — or the
only copy, and deleting it loses the fact. Per file, not a `sed`.

### Post-merge audit — 2026-09-15

[post-merge-audit-2026-09-15.md](code-review/phase3/post-merge-audit-2026-09-15.md)
covers what the 2026-09-08 audit could not: it was written against the first
in-branch merge, and the second one (`eb693e2`, the landing) was reviewed only by
plan-merge's §Step 2 and §Step 4 checklists. Scope was agreed before the pass and
kept small — the `ConnectionGauge` move to `fah-common`, the `eb693e2` hunks no
earlier review names, instrumentation validity, and merge-window tests that could
pass with the wiring they prove broken. Two later passes the same day widened it
— a defect hunt and a listener-configuration sweep, findings SP1–SP8, below.
**PASS WITH DEFERRED FINDINGS.**

Two facts worth carrying out of it, neither obvious from the code:

- The true pre-integration base is **`bc49e4e`**, not the tag. `main-pre-phase3-merge`
  (`ebc46f1`) is its parent, one plan-doc commit behind, and is the frozen bench
  checkout — a rollback point, never a diff base.
- `git show --cc --stat` on a merge prints files taken whole from one side too.
  The real hand-decision surface of `eb693e2` is **36 files**, not 67; it is the
  intersection of `git diff --name-only <parent> eb693e2` over both parents.

Findings:

- **N1 — closed 2026-09-15, the only defect the first pass found.** `CONFIGURATION.md` documented
  `FAH__DNS__TCP_MAX_CONNECTIONS` and `FAH__DNS__UDP_MAX_INFLIGHT`, and
  `env::apply_one` had no arm for either, so its `_ =>` arm returned
  `UnknownEnvKey` and **the process refused to start** with a documented variable
  set. Independently re-verified and reproduced on the built binary before any
  fix. The two arms landed with three `fah-config` tests, one child-process test
  through `--healthcheck` (no `std::env::set_var`), and an anti-drift test that
  walks every `Env: FAH__…` name in CONFIGURATION.md through
  `apply_env_overrides` — the doc and the allowlist can no longer diverge with
  the suite green. Gates green: `fmt` and `clippy -D warnings` clean,
  `cargo test --all-features --workspace` 0 failed, `fah-config` 90 passed
  (87 before), `healthcheck` 6 (5 before). **This is the same `udp_max_inflight`
  F8 leaves inert at 0** — arming it from the container's `fah-env` was the one
  route that looked available and did not exist.
- **N2 — closed 2026-09-17 in `b0c7709`.** `counters.dns_tcp_connections`,
  `dns_dot_connections` and `dns_udp_inflight` reached `/api/v1/telemetry`
  correctly and had no dashboard consumer, although they are the figures
  CONFIGURATION.md tells the operator to retune the ceilings from. The Health
  page's Backpressure card now shows them as active / peak rows plus the UDP
  shed count.
- **N3 — info, nothing owed.** `strategy_ab.rs` loops over one strategy since
  `fallback` was removed; the disposition is already recorded and the harness is
  not claimed to discriminate.

Not reopened, by instruction: F1–F9, p3-10 Track A, p3-11, and the dashboard as a
review surface. Untouched and still true: an unknown `FAH__` variable on a fresh
`/config` volume still leaves a defaults-only TOML before the load fails, because
`Config::load` writes before it applies the environment. Outside N1's scope.

#### Second pass and listener sweep — 2026-09-15, findings SP1–SP8

The same file carries two later passes: a defect hunt for what a green suite can
miss (SP1–SP3), then a sweep on one question — which invalid or mutually
incompatible **listener** configurations the product accepts as valid, and what
happens afterwards. Every conclusion came from the source and from reproduction
on the built binary; no existing review document was taken as evidence.

**Four confirmed defects, all fixed the same day, all on the configuration and
startup surface and all fail-closed:**

- **SP1** — of the six listener port pairs, the three not involving `https`
  (`dns–api`, `dns–http`, `api–http`) were compared by nothing. A colliding pair
  validated clean and could sit latent: under the shipped `mode = "dns"`,
  `api.port = 8080` beside the HTTP default was accepted and persisted, and the
  patch that later enabled `dns+http` also passed, answered `restart_required`,
  and the restart did not come back.
- **SP4** — `[api] address` accepted any IPv6 literal, including the `::`
  CONFIGURATION.md offers, then failed to bind it: `ApiServer::bind` assembled
  the address with `format!("{address}:{port}")`, the exact trap
  `fah_common::listen::listen_addr` exists to avoid. It also meant the API, the
  dashboard and DoH could not be reached over IPv6 at all.
- **SP5** — the API was the one listener whose bind failure never went through
  `bind_error`, so a port conflict printed a bare OS errno and a privileged
  `[api] port` would have lost the `CAP_NET_BIND_SERVICE` hint.
- **SP6** — `bind_error`'s `AddrInUse` arm dropped the config key its own doc
  comment promised, and told the operator another process held a port this
  process was holding itself.

**The rule SP1 now implements**, agreed before any code: *shape is validated
always, relationships only when both ends are live.* Sockets that actually bind
are compared, not config keys; same port plus overlapping addresses is a
conflict; `::` overlaps both families because `bind_tcp` clears `IPV6_V6ONLY` by
our own decision, `0.0.0.0` overlaps IPv4, and two concrete addresses never
overlap. A listener joins only when it would bind. Address **syntax** stays
unconditional. This is safe because the flip is always revalidated — the API
validates the whole merged candidate and boot revalidates the whole file — so a
parked collision is refused the moment it is enabled, by validation, with the key
named. It closes **SP7** (the `https` loop refusing configurations that could not
collide) without a change of its own.

Two consequences worth carrying, neither cleanup: `https.listen.port` is now
checked **less** often than before, since it was the only listener compared
unconditionally; and the e2e harness had to stop drawing duplicate ports, because
with the matrix complete a duplicate stops being a retryable `EADDRINUSE` at bind
and becomes a hard validation refusal. The harness was fixed at the source
(`free_tcp_port_excluding`), and the `is_port_conflict` needle list was
deliberately **not** widened — that would let a genuine wrong refusal hide behind
a retry.

~~Still open from these passes, both owner decisions, neither blocking: **SP2**
(the DoT leaf pre-warm awaited outside the handshake deadline while holding an
accept permit — Low, structural, and the measured 450.88 µs mint argues against
it being live)~~ — **SP2 closed 2026-09-17 in `b0c7709`** (§Audit closures);
the test holds a one-thread blocking pool and expects the connection closed at
the 300 ms deadline with the gauge back to 0, and it failed on the old code by
waiting out its 5 s guard. Still open, owner decision, not blocking:
**SP8** (DoH has no status surface when `[api] tls = false`
silences it, where DoT has `DotListener::Closed { reason }` on
`GET /api/v1/certificates`). **SP3** is info: `wiring.rs` asserts on `main.rs`
source text, and `acceptor_death.rs` is the test that discriminates.

One thing the fix could not carry: deleting `main.rs`'s `http_enabled` /
`https_enabled` removed the six-line comment explaining why the mode match is
exhaustive. The match moved to `schema/engine.rs`; the rationale could not,
because hard rule 7 forbids an agent writing Rust comments. Behaviour is
self-enforcing without it — the match really is exhaustive — but the reasoning
now lives only in the audit file and in git history.

#### Independent review of the two fix commits — 2026-09-15, findings R1–R7

`f32f214` and `2eb5018` were then reviewed by a reviewer who did not write them:
plan compliance against the resolutions above, correctness, architecture,
performance, memory, Rust quality, tests, regression. **One should-fix, six
notes, no blocker, and no defect in the DNS or HTTP data path.**

- **R1 — closed 2026-09-15.** `fah-http`'s two `PORT_SETTING` constants
  advertised `FAH__HTTP__LISTEN__PORT` and `FAH__HTTPS__LISTEN__PORT`;
  `env::apply_one` had no arm for either, so following the hint stopped the
  process from booting. N1's class in a **second population**: N1's anti-drift
  test walks the names *CONFIGURATION.md* advertises, and nothing walked the
  names the *code* advertises. The two arms landed, the five constants moved to
  `fah-config/src/port_setting.rs` — `fah-common` owns `bind_error` but is an L1
  sibling and may not import the allowlist — and one test parses the variable out
  of each constant, so a renamed constant carries its own check. CONFIGURATION.md
  gained the two `Env:` lines, which makes the doc-walking test cover five names
  instead of three.
- **R2 — closed 2026-09-17 in `b0c7709`**: the `dot–dns` and `dot–http` rows
  make every one of the 10 listener pairs asserted somewhere.
- **R3–R7 — open, notes, nothing owed.** `addresses_overlap` misses IPv4-mapped IPv6; the blamed key is
  the later entry in the socket table, not the edited one; the `Env:` scan's
  `>= 3` floor equals the count it guards; one API test binds `[::]:0` on every
  interface and needs host IPv6; `validate_listen_sockets` takes both the config
  and the addresses derived from it.

**Neither R1 nor N1 was created by the Phase 3 merge** — worth recording, because
the audit that found them was triggered by it. `git merge-base --is-ancestor`
against `bc49e4e`, the pre-merge tip of `main`: N1's two keys landed on `main`
itself on 2026-09-11 (`ed28395`, `b0b091e`), and R1's **HTTP half** landed on
2026-07-26 with p2-01 (`4ef6d4a`). Only R1's **HTTPS half** arrived with the merge
(`40ca0cc`, p3-03) — `tls_server.rs` did not exist on `main` before it. On
pre-merge `main` the wrong HTTP variable was near-invisible: `bind_error`'s
`AddrInUse` arm dropped `port_setting` entirely, so it printed only on
`PermissionDenied`, which needs `[http.listen] port` below 1024 against a default
of 8080. **SP6's fix is what made a two-month-old defect visible.** The version
number does not discriminate any of this: `bc49e4e` already reads `0.3.4`.

## Risk inventory close-out — 2026-09-11

[project-risk-inventory.md](code-review/project-risk-inventory.md) surveyed
`main` at `baa2ecd`. Its three material findings — F1 (DNS-over-TCP had no
connection ceiling and allocated from the client's length prefix), F2 (UDP
in-flight queries unbounded) and F10 (no stats flush on a clean stop) — are
**closed**: fixed in `ed28395`, `b0b091e` and `32d7776`, verified by the
close-out audit, and moved to the inventory's §Closed with their evidence. No
material finding is open.

Follow-ups, neither a risk:

- **F1 soak — tuning only.** 1024 connections and 16 KiB per message are
  initial safety bounds. After 7 days on the RB5009 *with the F1 build*, read
  `counters.dns_tcp_connections.{peak,closed_oversize}` from
  `/api/v1/telemetry`, check the container fd budget, set the final
  `tcp_max_connections` default, and record corpus, workload and device under
  `docs/code-review/`. **The soak ran 4.7 of the 7 days and was stopped**
  (§0.3.4 soak), so the reading is short of the specified window but not
  ambiguous. **Final reading, last pull `20260916T160001Z`, 116 pulls, 113.9 h:**
  `peak` **33**, `closed_oversize` **0**, `active` 0 in every sample, uptime
  monotonic to 113.94 h so nothing restarted and no counter was zeroed. The peak
  was reached in the second pull, sixteen minutes after t0, and **did not move
  once in the remaining 113 hours** — most plausibly clients falling back to TCP
  while the resolver came up, which is the case the ceiling has to survive rather
  than one to discount. 1024 is 31× it. **What the shortfall costs is nothing
  here and everything next door:** a figure flat for 113 h will not be moved by
  54 more, but this build predates the `p3-10b` DoT gauge, so the number is TCP
  alone and the DoT half has still never been measured. Setting the final
  `tcp_max_connections` default needs the next soak, on a build that can count
  both.
- **F10 history-write residual.** `fah-stats` `history/mod.rs` `append_line`
  writes a rollup line and its `\n` as two `write_all`s; a stop landing between
  them leaves a partial line that the reader skips — one completed hour lost.
  Pre-existing, a microsecond window once per 300 s; the fix is one combined
  write. Its own go.

**F14 opened and closed 2026-09-15** — `rules.refresh_hours_default` was
classified runtime, and the only component it governs never saw a change:
`ListManager` copied it into a plain `u32` at construction, so `POST
/api/v1/config` answered `applied, no restart` while the refresh scheduler kept
the boot value. Its **two readers** are what hid it — the `/lists` handlers read
the config store live, so `GET /config` and `GET /lists` both reported the new
number and the only witness was the timing of the next fetch. It reached the
shipped default, since `oisd-basic` carries no per-list `refresh_hours`. Fixed
with an `AtomicU32` and `set_default_refresh_hours`, called from `post_config`
beside the applies it already ran; mutation-verified on two tests that fail
separately. **Predated the Phase 3 merge** (`7920415`, `e056190` — p1.5-07).
The durable lesson is about enumeration tests, not this key: the family *was*
enumerated at `config_store.rs`, but the `consumer` column named a reader rather
than an applier and the assertion only checked the classification, so the
enumeration certified the defect instead of catching it
([project-risk-inventory.md](code-review/project-risk-inventory.md) §Closed).

**The enumeration sweep is finished, 2026-09-15 — four families counted, two
clean, two findings.** F14 was the first, and the method it proved is the thing
to carry: write down both sides of a set and compare the counts. A diff shows
changed lines; it cannot show an absent member, which is what every finding of
this sweep turned out to be.

- `BOOT_KEYS` against the keys with a live consumer → **F14**, the only defect.
- Schema fields against CONFIGURATION.md, in the schema → doc direction the
  `Env:` walk does not cover → **clean**, 25 structs and 55 leaf keys, all 55
  documented.
- `/api/v1/telemetry` fields against dashboard consumers → **F15**, minor. The
  whole `listeners` block is unmodelled by `interface Telemetry`, and an
  unmodelled JSON field is silently ignored, so adding it produced no signal
  anywhere. N2 was not the only instance of that family; this one is 34 leaves
  against N2's three. **The only finding of the sweep the merge created** — at
  `bc49e4e` there was no `listeners` field to model. Owner has not decided
  whether the dashboard consumes it at all.
- Per-listener runtime dispositions → **clean**. A disposition earns its keep
  only where the real state can differ from the config, which is true of DoT
  alone — and DoT is the one that has `DotListener::Closed { reason }`. This
  refines SP8 rather than overturning it: DoH's state is two config reads away,
  not hidden. **F16** fell out beside the sweep: when an acceptor dies the
  identity of the dead task exists only in one log line, while `/config`,
  `/health` and `/telemetry` all keep reporting the lane as healthy. Not a
  defect — F11's design working as decided; F11 settled whether to restart, not
  whether the identity should be queryable.

All four are recorded with their numbers in
[project-risk-inventory.md](code-review/project-risk-inventory.md), the clean
ones in §Checked and clean so a later pass skips them.

**F11 closed 2026-09-12** — report-only supervisor: the run loop checks every
long-lived task on the 10 s telemetry tick, logs a death once and counts it
in `counters.tasks_died`; no restart, no exit, `/health` unchanged (see
[project-risk-inventory.md](code-review/project-risk-inventory.md) §Closed).

**F3 closed 2026-09-12** — `Pipeline::handle` borrows `request.queries.first()`
instead of cloning it: one allocation per query fewer for names past hickory
`Name`'s 32 inline label bytes, measured A/B against `0fb8dd0` with the new
`warm_pipeline_handles_allocate_a_steady_amount` (inventory §Closed).
Committed as `07d4d68`. Its follow-up attributed every remaining per-query
allocation and pre-sized `domain_of`
([f3-name-alloc-attribution.md](code-review/phase2.6/f3-name-alloc-attribution.md)):
13 / 19 / 10 / 16 allocations per handle across blocked and cache-hit paths,
inline and heap names. F4–F9 and F12 are record-only. N5 (`Semaphore::new` panics above `MAX_PERMITS`; neither
`max_connections` key has an upper bound) is recorded and excluded by owner
decision.

## HTTP allocation domains — merged

What is deployed: each HTTP connection served end to end on one of N
`current_thread` runtimes on their own OS threads behind one acceptor, N=2, so
a connection's allocations are freed by the thread that made them. Decision
and cost: [ADR-0006](decisions/0006-http-allocation-domains.md); term:
CONTEXT.md "Allocation Domain"; config: CONFIGURATION.md `[runtime]` (boot
class); code review with findings 1–23:
[alloc-domains-http-review.md](code-review/phase2.6/alloc-domains-http-review.md).

Why N=2 — the RB5009 N sweep of 2026-09-07
([alloc-domains-n-sweep.md](code-review/phase2.6/alloc-domains-n-sweep.md)):

| | N=0 (old way) | N=2 | N=3 | N=4 |
| --- | --- | --- | --- | --- |
| new connections/s, keep-alive rps | 2341, 3297 | 1860, 2574 | 1963, 3237 | 1947, 3412 |
| HTTP p95 ms close / keep-alive | 15.9 / 46.9 | 27.8 / 45.3 | 49.1 / 53.5 | 50.7 / 53.3 |
| cores busy (close) | 3.63 | 2.21 | 2.58 | 2.65 |
| DNS p50 / p99 ms under HTTP load | 3.35 / 20.1 | 0.96 / 15.8 | 1.11 / 13.6 | 1.01 / 12.7 |
| held after 900 MiB WAN, +15 min | +56..+60 | +19 | +32 | +45 |

N=2 carries the tested connection-rate workload with a third less CPU per
request than N=0, better DNS latency under load, and a third of the old way's
held memory; N=3/4 buy keep-alive rate at the cost of p95 and memory. The LAN
transfer pass is **not** a 1 GbE test (router forwarding path caps it at
~67–70 MiB/s). Untested: TLS termination and HTML rewriting — the N decision
is re-measured when Phase 3/4 exist.

Rollback without a rebuild: `FAH__RUNTIME__HTTP_RUNTIMES=0` on `fah-env` +
restart (the 0.3.1 code path, same image). Rollback of the build:
`kingston/fastadhunter-arm64-0.3.1.tar`, or `main` at the tag.

Deferred, each its own go: 11b graceful shutdown of keep-alive connections
(finish the in-flight exchange instead of the whole transfer); dashboard
settings metadata for `runtime.http_runtimes` (review finding 3); the capacity
microbench that decides whether N=2 clears 1 Gbit with TLS — **its AES-GCM half
ran 2026-09-15** by probe, no deploy, ~1.0 ms/MiB per core and so ~12 % of one
core at line rate, which removes encryption as the explanation but answers
nothing about the NIC, the forwarding path or the scheduler
([p3-10-track-b2-rb5009.md](code-review/phase3/p3-10-track-b2-rb5009.md)
§Measurements — bulk AEAD cost); **the lol_html half is cancelled, not deferred — Phase 4 is parked 2026-09-15 ([ADR-0009](decisions/0009-phase-4-parked.md))**; IPv6 privacy-address rotation versus
address-exact client identity — reviewed 2026-09-07
([ipv6-privacy-rotation-review.md](code-review/phase2.6/ipv6-privacy-rotation-review.md)):
**Fix 1 shipped 2026-09-21 (`6350fc1`)**, Fix 2 (`advertise-dns=no`) declined
by the owner, Fix 3 decided in [ADR-0010](decisions/0010-device-identity-from-routeros-rest.md)
and planned as phase 2.7 — not built.

## Deferrable (reconciled §6)

Type mirrors (`fah_config`/`fah_model`), `CacheStats` identity-DTO; compile
transient (peak 141.3 MiB on 0.3.1, monitored via `peak_rss`); policy
fail-open window and name-assignments-on-LRU; SWR no-EDNS truncation tax;
fah-common scope creep; `blocking_mode` inert; histogram 100 ms ceiling;
p2.5-10 n1 (test-helper readability in `fah-api`, test-only); 11b graceful
HTTP shutdown.

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (10013) when WinNAT reserves the ephemeral port block.
Environmental — do not attribute to a change.

p2.5-09 V3b (live healthcheck column) is a RouterOS 7.21.5 platform
limitation, not a build defect — documented as a trap in
[routeros-traps.md](routeros-traps.md) §Container configuration.
