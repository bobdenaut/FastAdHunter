//! Rule list lifecycle (RULE_ENGINE.md §List lifecycle + §Sources): source ->
//! parse -> validate -> compile -> atomic swap, with a `/data` raw-copy cache
//! so boot never waits on the network, and a failure policy where a bad
//! refresh never degrades protection.
//!
//! [`ListManager`] owns the one compiled [`Matcher`] the DNS pipeline reads
//! (via [`ListManager::matcher`], an `arc-swap` load — no lock, ever, on that
//! path). Everything else here — fetching, parsing, recompiling — runs off
//! the hot path. Two locks divide that work: a per-list mutex serializes one
//! list's whole fetch+commit (a scheduled tick racing a manual trigger can't
//! commit a stale fetch over a newer one, while a slow list never blocks
//! another list's refresh), and a global mutex serializes compile+swap. The
//! `/data` cache is the source of truth for raw text — recompiles re-read it
//! from disk, so raw list bytes don't stay resident between refreshes — and
//! the CPU-heavy parse+build itself runs on the blocking pool so a recompile
//! never stalls a tokio worker the DNS pipeline shares.

mod cache;
mod resolver;
mod source;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use arc_swap::ArcSwap;
use fah_config::{RuleListConfig, RulesConfig};
use tokio::time::Instant;

use resolver::ReqwestResolver;
pub use resolver::{HostResolver, Resolving};

use self::source::ListSource;
use crate::matcher::{Matcher, MatcherBuilder};
use crate::policy::PolicySet;
use crate::rule_list::ParsedRuleList;

/// Synthetic list id for inline personal rules (RULE_ENGINE.md §Sources: User
/// rules) — not present in `[[rules.lists]]`, set directly via
/// [`ListManager::set_user_rules`].
const USER_RULES_ID: &str = "user-rules";

const DEFAULT_FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Hard cap on one list's raw size — hard rule 4 (bounded everything): a
/// rogue or misconfigured source must not be able to OOM the 1 GB RB5009. An
/// order of magnitude above the biggest real-world blocklist.
const MAX_LIST_BYTES: usize = 64 * 1024 * 1024;

/// How often the scheduler wakes to look for lists whose refresh interval has
/// elapsed. One task drives every list (rather than a task per list) so
/// `POST`/`DELETE /api/v1/lists` can add and remove lists at runtime without
/// spawning or aborting anything. RULE_ENGINE.md: boot compiles from cache
/// immediately, then "refresh happens asynchronously afterwards" — the first
/// tick fires at once, and refreshes run sequentially inside it, so a fresh
/// install fetches promptly without a thundering herd of parallel downloads
/// on the RB5009's single 1 GB of RAM.
const SCHEDULER_TICK: Duration = Duration::from_secs(60);

/// Failure modes for a single list operation. Fetch/read I/O and an
/// oversized payload are the only ones possible — a bad *parse* never fails
/// (RULE_ENGINE.md: unparseable lines are skipped and counted, never reject
/// a list).
#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error("unknown list id: {0}")]
    UnknownList(String),
    #[error("list id {0} already exists")]
    DuplicateList(String),
    #[error("list {0} is disabled")]
    ListDisabled(String),
    #[error("failed to build HTTP client: {0}")]
    Client(#[source] reqwest::Error),
    #[error("fetch {url} failed: {source}")]
    Fetch {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("read local list {path:?}: {source}")]
    LocalRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("list {origin} exceeds the size limit of {limit} bytes")]
    TooLarge { origin: String, limit: usize },
}

/// Rule counts from a successful parse of one list — what
/// [`ListManager::refresh_list`] / [`ListManager::set_user_rules`] return on
/// success, and what a [`RefreshResult::Ok`] status carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshStats {
    /// DNS-applicable rules.
    pub active: usize,
    /// Request-applicable rules — the URL tier (p2-03).
    pub url: usize,
    /// Rules no tier answers yet (cosmetic, `$client`, unsupported patterns).
    pub inactive: usize,
    pub parse_errors: u32,
}

impl From<&ParsedRuleList> for RefreshStats {
    fn from(parsed: &ParsedRuleList) -> Self {
        Self {
            active: parsed.active_count(),
            url: parsed.url_count(),
            inactive: parsed.inactive_count(),
            parse_errors: parsed.parse_errors,
        }
    }
}

/// Below this many errors nothing is reported, however bad the ratio: a
/// handful of malformed lines in a short list is ordinary, and
/// [`RefreshStats::looks_misparsed`] is meant to catch a *list-wide* misread,
/// not line noise.
const MISPARSE_ERROR_FLOOR: u32 = 100;

impl RefreshStats {
    /// True when the parse looks like the **list** was misread, not like a few
    /// lines were malformed — more failures than rules, past a floor.
    ///
    /// This exists because the failure it detects was invisible. Real EasyList
    /// used to be detected as a plain domain list and handed to the wrong
    /// parser, producing 83 rules and 69,514 errors from 83,516 lines — and
    /// reporting [`RefreshResult::Ok`], indistinguishable from a healthy load.
    /// One list read as the wrong format is *one* failure, and it should be
    /// legible as one rather than as tens of thousands of line errors nobody
    /// reads.
    pub fn looks_misparsed(&self) -> bool {
        self.parse_errors >= MISPARSE_ERROR_FLOOR
            && usize::try_from(self.parse_errors).unwrap_or(usize::MAX)
                > self.active + self.url + self.inactive
    }
}

/// The outcome of a list's most recent refresh attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum RefreshResult {
    NeverAttempted,
    Ok(RefreshStats),
    Failed(String),
}

/// One list's result from [`ListManager::refresh_all`]: its stats on success,
/// or the fetch error chain on failure. Emitted in configuration order so a
/// caller can report exactly which lists refreshed and which did not.
#[derive(Debug, Clone)]
pub struct ListRefreshOutcome {
    pub id: Arc<str>,
    pub result: Result<RefreshStats, String>,
}

/// Per-list status, surfaced to the API (p1-09) via
/// [`ListManager::status`]/[`ListManager::statuses`].
#[derive(Debug, Clone, PartialEq)]
pub struct ListStatus {
    /// When this list last *successfully* refreshed. `None` until the first
    /// success (boot-from-cache does not count — RULE_ENGINE.md's "compile
    /// from `/data`" is a load, not a refresh).
    pub last_refreshed: Option<SystemTime>,
    pub last_result: RefreshResult,
    /// What this list contributes to the ruleset that is *currently serving*,
    /// as measured by the last compile. Deliberately independent of
    /// `last_result`: a failed refresh leaves the previous ruleset in place
    /// (RULE_ENGINE.md failure policy), so the list keeps contributing its
    /// rules and must keep reporting them. `None` when it contributes nothing
    /// — disabled, or no cache copy on disk yet.
    pub compiled: Option<RefreshStats>,
}

impl Default for ListStatus {
    fn default() -> Self {
        Self {
            last_refreshed: None,
            last_result: RefreshResult::NeverAttempted,
            compiled: None,
        }
    }
}

struct ListEntry {
    id: Arc<str>,
    /// The configured source string, kept verbatim for `GET /api/v1/lists`
    /// (a local list's `path` round-trips through the same field).
    url: String,
    source: ListSource,
    /// Mutable at runtime via `PATCH /api/v1/lists/{id}`; atomics rather than
    /// a rebuilt entry so an in-flight refresh keeps holding the same
    /// `refresh_lock` it started under.
    enabled: AtomicBool,
    /// Per-list refresh interval in hours; `0` means "follow
    /// `[rules] refresh_hours_default`" (the `None` of
    /// [`fah_config::RuleListConfig::refresh_hours`], which cannot be 0 —
    /// intervals are clamped to at least 1 hour).
    refresh_hours: AtomicU32,
    /// Serializes this list's whole fetch+commit so a scheduled tick racing a
    /// manual trigger (`POST /api/v1/lists/{id}/refresh`) can't commit an
    /// older fetch over a newer one. Per-list, so one slow fetch never blocks
    /// another list's refresh.
    refresh_lock: tokio::sync::Mutex<()>,
}

impl ListEntry {
    fn new(config: &RuleListConfig, data_dir: &std::path::Path) -> Self {
        Self {
            id: Arc::from(config.id.as_str()),
            url: config.url.clone(),
            source: ListSource::from_url(&config.url, data_dir),
            enabled: AtomicBool::new(config.enabled),
            refresh_hours: AtomicU32::new(config.refresh_hours.unwrap_or(0)),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    fn view(&self) -> ListEntryView {
        ListEntryView {
            id: self.id.to_string(),
            url: self.url.clone(),
            enabled: self.is_enabled(),
            refresh_hours: match self.refresh_hours.load(Ordering::Relaxed) {
                0 => None,
                hours => Some(hours),
            },
        }
    }

    /// This list's effective interval: its own override, else the global
    /// default. Floored at 1 h — `tokio::time::interval` panics on a zero
    /// period and fah-config validates no lower bound.
    fn interval(&self, default_hours: u32) -> Duration {
        let hours = match self.refresh_hours.load(Ordering::Relaxed) {
            0 => default_hours,
            hours => hours,
        };
        Duration::from_secs(u64::from(hours.max(1)) * 3600)
    }
}

/// One configured list's identity and refresh policy, for
/// `GET /api/v1/lists`. The *outcome* of its last refresh lives in
/// [`ListStatus`]; the two are joined by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntryView {
    pub id: String,
    pub url: String,
    pub enabled: bool,
    pub refresh_hours: Option<u32>,
}

/// The mutable half of `PATCH /api/v1/lists/{id}` (API.md: "Enable/disable,
/// change refresh interval"). `None` leaves a field untouched;
/// `Some(None)` for `refresh_hours` clears the override back to the global
/// default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListPatch {
    pub enabled: Option<bool>,
    pub refresh_hours: Option<Option<u32>>,
}

/// Owns the compiled ruleset and drives its lifecycle. One instance per
/// process; the DNS pipeline (p1-04) holds a handle to read
/// [`ListManager::matcher`], the API (p1-09) holds a handle to call
/// [`ListManager::refresh_list`] / [`ListManager::set_user_rules`] / status —
/// this module exposes exactly that surface without depending on either.
pub struct ListManager {
    data_dir: PathBuf,
    http: reqwest::Client,
    fetch_timeout: Duration,
    /// `[rules] refresh_hours_default` — the interval a list without its own
    /// `refresh_hours` override follows.
    default_refresh_hours: u32,
    /// Mutable at runtime: `POST`/`DELETE /api/v1/lists` add and remove
    /// entries while the scheduler and in-flight refreshes run. Entries are
    /// `Arc`'d so a caller can clone one out and drop the guard before
    /// awaiting (a `std` lock must never be held across an `.await`).
    entries: RwLock<Vec<Arc<ListEntry>>>,
    /// When each list was last *attempted* (not necessarily successfully) —
    /// what the scheduler compares its interval against. `tokio`'s `Instant`
    /// so `start_paused` tests advance it with virtual time. Separate from
    /// [`ListStatus::last_refreshed`], which records successes only and is
    /// what the API reports.
    ///
    /// Monotonic, so it cannot outlive the process. [`Self::boot`] rebuilds it
    /// from the `/data` cache files' mtimes rather than starting every list due
    /// on every restart — see [`Self::seed_last_attempted_from_cache`].
    last_attempted: Mutex<HashMap<Arc<str>, Instant>>,
    /// Raw text for lists whose `/data` cache write failed — the *only* raw
    /// text ever held in memory. The `/data` cache is the durable source of
    /// truth; recompiles re-read it from disk, so list bytes don't double the
    /// resident footprint next to the compiled matcher (PERFORMANCE.md).
    ///
    /// Keyed by list id, and `commit_raw` uses `insert`, so a list that keeps
    /// failing replaces its own entry rather than stacking copies. The bound is
    /// therefore `lists × MAX_LIST_BYTES` — set by configuration, not by uptime
    /// or traffic, so hard rule 4 holds. It is a loose bound all the same: see
    /// the follow-up in `plan/wip/phase2/CLAUDE.md` for capping the aggregate
    /// and marking the list `degraded` instead. Note that simply *dropping* the
    /// text is not the fix — [`Self::compile`] re-reads `/data`, so the list
    /// would silently revert to its stale cached copy.
    pending_cache: Mutex<HashMap<Arc<str>, String>>,
    status: Mutex<HashMap<Arc<str>, ListStatus>>,
    /// Serializes compile+swap (and the cache write feeding it) so two lists
    /// committing at once can't interleave. Fetches run outside it — ordering
    /// per list is [`ListEntry::refresh_lock`]'s job.
    compile_lock: tokio::sync::Mutex<()>,
    /// The hot path's only touchpoint: an atomic-swap read, never a lock
    /// (PERFORMANCE.md, ARCHITECTURE.md §Runtime Model).
    matcher: ArcSwap<Matcher>,
    /// The compiled `[[policies]]`, swapped the same way.
    ///
    /// Held here rather than passed to the constructor so every existing call
    /// site keeps its signature, and because it has the same lifetime as the
    /// matcher: which policies exist decides how the ruleset is compiled, so
    /// the two must be replaced together (`set_policies` then `compile`).
    /// Defaults to [`PolicySet::single_default`], which is the pre-Policies
    /// behaviour.
    policies: ArcSwap<PolicySet>,
    /// Full ruleset rebuilds since start.
    ///
    /// A compile re-reads every enabled list and rebuilds the whole matcher, so
    /// it is the most expensive thing this crate does — seconds of ARM CPU and a
    /// transient well over 100 MiB. Counting them is what makes "one compile per
    /// batch, not one per list" an assertable property rather than an intention;
    /// see [`Self::refresh_due_lists`].
    compiles: std::sync::atomic::AtomicU64,
    /// Wall time of the most recent [`Self::compile`], in microseconds.
    ///
    /// Held here rather than pushed to `fah-metrics` on each compile because
    /// the binary's telemetry poll rewrites the whole ruleset snapshot every
    /// 10 s: a value written on a compile event would be erased by the next
    /// tick. Keeping it on the manager makes the poll *pull* a durable figure,
    /// the same way it already pulls `rules` and `heap_bytes` from the matcher.
    ///
    /// Microseconds in a `u64` so the store is a plain relaxed atomic — a
    /// `Duration` is two fields and could not be read without tearing. u64 µs
    /// covers ~584,000 years, and compiles are seconds.
    last_compile_micros: std::sync::atomic::AtomicU64,
}

impl ListManager {
    /// Builds a manager whose list downloads use the *system* resolver.
    ///
    /// Correct for tests, benches and any host with a working
    /// `/etc/resolv.conf`. In the shipped container prefer
    /// [`Self::with_resolver`]: RouterOS leaves `/etc/resolv.conf` empty, so
    /// the system resolver has no nameserver to ask (p1-11 defect 5).
    pub fn new(config: &RulesConfig, data_dir: PathBuf) -> Result<Self, LifecycleError> {
        Self::build(config, data_dir, None)
    }

    /// Builds a manager that resolves list hosts through `resolver` — in the
    /// binary, FastAdHunter's own configured upstreams. Removes the dependency
    /// on `/etc/resolv.conf` entirely: a process that is itself a working DNS
    /// resolver should not need a second one to fetch its own lists.
    pub fn with_resolver(
        config: &RulesConfig,
        data_dir: PathBuf,
        resolver: Arc<dyn HostResolver>,
    ) -> Result<Self, LifecycleError> {
        Self::build(config, data_dir, Some(resolver))
    }

    fn build(
        config: &RulesConfig,
        data_dir: PathBuf,
        resolver: Option<Arc<dyn HostResolver>>,
    ) -> Result<Self, LifecycleError> {
        let mut client = reqwest::Client::builder()
            // Explicit, though the `hickory-dns` feature already makes this
            // the default: the call is gated on that feature, so dropping it
            // breaks the build instead of silently restoring musl's
            // `getaddrinfo` and its all-or-nothing A/AAAA lookup (p1-11
            // defect 2 — a failure that presents as an empty ruleset on a
            // resolver that is otherwise serving traffic normally).
            .hickory_dns(true);
        if let Some(resolver) = resolver {
            // Supersedes the line above: our own upstreams, not the system's.
            client = client.dns_resolver(Arc::new(ReqwestResolver(resolver)));
        }
        let http = client.build().map_err(LifecycleError::Client)?;
        let entries = config
            .lists
            .iter()
            .map(|list| Arc::new(ListEntry::new(list, &data_dir)))
            .collect();
        // Every configured list starts as NeverAttempted so the status API
        // can distinguish "configured, never fetched" from "not configured".
        let status = config
            .lists
            .iter()
            .map(|list| (Arc::from(list.id.as_str()), ListStatus::default()))
            .collect();
        Ok(Self {
            data_dir,
            http,
            fetch_timeout: DEFAULT_FETCH_TIMEOUT,
            default_refresh_hours: config.refresh_hours_default,
            entries: RwLock::new(entries),
            last_attempted: Mutex::new(HashMap::new()),
            pending_cache: Mutex::new(HashMap::new()),
            status: Mutex::new(status),
            compile_lock: tokio::sync::Mutex::new(()),
            matcher: ArcSwap::new(Arc::new(MatcherBuilder::new().build())),
            policies: ArcSwap::from_pointee(PolicySet::single_default()),
            compiles: std::sync::atomic::AtomicU64::new(0),
            last_compile_micros: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// The current compiled ruleset. Allocation-free beyond an atomic
    /// refcount bump — safe to call on every query (PERFORMANCE.md).
    pub fn matcher(&self) -> Arc<Matcher> {
        self.matcher.load_full()
    }

    /// Loads whatever `/data` cache copies already exist and compiles the
    /// initial ruleset — no network (RULE_ENGINE.md: "Boot: compile from the
    /// `/data` cached copies immediately"). A list with no cache file yet
    /// (first-ever boot) is simply absent until its first successful
    /// [`Self::refresh_list`].
    pub async fn boot(&self) {
        let _guard = self.compile_lock.lock().await;
        let (matcher, stats) = self.compile().await;
        self.swap_in(matcher, &stats);

        self.seed_last_attempted_from_cache().await;
        self.remove_orphaned_copies().await;

        let mut status = self.status.lock().unwrap();
        for (id, list_stats) in stats {
            // Fill in cache-derived stats only where nothing newer exists — a
            // refresh that beat boot to the lock must not be overwritten.
            let entry = status.entry(id).or_default();
            if entry.last_result == RefreshResult::NeverAttempted {
                entry.last_result = RefreshResult::Ok(list_stats);
            }
        }
    }

    /// Recovers the refresh clock from the `/data` cache files' mtimes, so a
    /// restart does not refetch and recompile everything it just loaded.
    ///
    /// [`Self::last_attempted`] holds `tokio::time::Instant`s — monotonic and
    /// process-relative, so the map starts empty on every start and every list
    /// reads as never attempted, i.e. due at the scheduler's first tick.
    /// Measured on the RB5009 at 0.2.8: a restart compiled from cache at boot,
    /// then ~7 s later refetched all 16 lists and paid a **second** full
    /// compile — 2.3 s of ARM CPU, ~24 MB of downloads and the ~158 MiB
    /// peak-RSS transient — to rebuild a byte-identical ruleset. Serving was
    /// never blocked (the listeners bind before this), so it was waste rather
    /// than a fault, but it was waste on every restart.
    ///
    /// [`cache::write`] renames a list's file into place on every successful
    /// fetch, so the mtime is already the persisted form of that clock: no new
    /// on-disk state, no schema to migrate, and nothing that can fall out of
    /// sync with the copy it describes.
    ///
    /// Only lists whose cached copy is *younger* than their interval are
    /// seeded. Everything else — no cache file, a copy already older than the
    /// interval, an unreadable or future mtime (see [`cache::age`]), or an age
    /// that predates the monotonic clock's own origin — is left unseeded and
    /// stays due, which is precisely the behaviour this replaces. The fix
    /// therefore cannot make a list refresh *less* often than before; it can
    /// only remove a redundant fetch.
    ///
    /// One deliberate behaviour change: the mtime records the last *success*,
    /// while `last_attempted` records attempts. A list whose source is down
    /// keeps an old file, so it gets one immediate retry after a restart
    /// instead of waiting out its interval. That is one extra attempt, not a
    /// loop — the retry writes `last_attempted` and the in-memory clock takes
    /// over from there.
    async fn seed_last_attempted_from_cache(&self) {
        let wall_now = SystemTime::now();
        let now = Instant::now();
        // Clone the handles out first: `cache::age` awaits, and a `std` lock
        // must never be held across an `.await`.
        let entries: Vec<Arc<ListEntry>> = self
            .entries
            .read()
            .unwrap()
            .iter()
            .map(Arc::clone)
            .collect();

        let mut seeded = 0usize;
        for entry in entries {
            let Some(age) = cache::age(&self.data_dir, &entry.id, wall_now).await else {
                continue;
            };
            if age >= entry.interval(self.default_refresh_hours) {
                continue;
            }
            // `checked_sub`, not `-`: the monotonic clock counts from system
            // boot, so a router that rebooted minutes ago cannot represent an
            // instant hours in the past. `None` there means "fall back to
            // treating the list as due", not "panic".
            let Some(attempted_at) = now.checked_sub(age) else {
                continue;
            };
            self.last_attempted
                .lock()
                .unwrap()
                .insert(Arc::clone(&entry.id), attempted_at);
            seeded += 1;
        }

        if seeded > 0 {
            tracing::info!(
                lists = seeded,
                "refresh schedule restored from cached copies"
            );
        }
    }

    /// Deletes `/data` cached copies that no configured list claims any more,
    /// naming each one in the log as it goes.
    ///
    /// `DELETE /api/v1/lists/{id}` already deletes its own copy, so the only
    /// way to strand one is to edit `[[rules.lists]]` while FAH is stopped —
    /// or to lose the race [`Self::remove_list`] documents. Nothing reads a
    /// stranded copy ([`Self::compile`] iterates configured entries and looks
    /// each id up, never the directory), so it is disk cost only: up to
    /// `MAX_LIST_BYTES` each, on a volume shared with the history series.
    ///
    /// **Boot only, and deliberately.** Here the scheduler has not spawned and
    /// no API listener is bound, so nothing else can be writing to `lists/`.
    /// A periodic sweep would race [`Self::commit_raw`] — it would have to
    /// distinguish a stranded copy from one being written this instant, which
    /// a directory listing cannot do.
    ///
    /// The keep-set is every **configured** entry, enabled or not: a disabled
    /// list's copy must survive so `PATCH {"enabled": true}` can swap it in
    /// without a download. `user-rules` is added explicitly — it has a cache
    /// file but is not a list, and it is the one copy that cannot be
    /// re-downloaded if deleted.
    async fn remove_orphaned_copies(&self) {
        let mut keep: std::collections::HashSet<Arc<str>> = self
            .entries
            .read()
            .unwrap()
            .iter()
            .map(|entry| Arc::clone(&entry.id))
            .collect();
        keep.insert(Arc::from(USER_RULES_ID));

        for (id, bytes) in cache::remove_orphans(&self.data_dir, &keep).await {
            // `warn`, not `info`: a file was deleted, and the operator who
            // edited the config out from under it is the only one who can say
            // whether that was intended.
            tracing::warn!(
                list = %id,
                bytes,
                "deleted cached copy of a list that is no longer configured"
            );
        }
    }

    /// Downloads/reads one configured list, and on success recompiles and
    /// atomically swaps in the new combined ruleset. On failure — the only
    /// failure mode is I/O, never parsing — the previous ruleset keeps
    /// serving untouched (RULE_ENGINE.md failure policy) and the failure is
    /// recorded in [`Self::status`].
    pub async fn refresh_list(&self, id: &str) -> Result<RefreshStats, LifecycleError> {
        let entry = self
            .find(id)
            .ok_or_else(|| LifecycleError::UnknownList(id.to_string()))?;
        if !entry.is_enabled() {
            // A disabled list is skipped by `compile`; fetching it would burn
            // network + disk to report fabricated all-zero stats.
            return Err(LifecycleError::ListDisabled(id.to_string()));
        }

        // Fetch + commit through the shared helper. On failure the previous
        // ruleset keeps serving (RULE_ENGINE.md) and the cause is recorded —
        // the chain, not just `err`: on a distroless image with no shell this
        // string is the only diagnostic available, and the outermost layer
        // cannot tell a DNS failure from a refused connection or a rejected
        // certificate.
        if let Err(err) = self.fetch_and_commit(&entry).await {
            self.record_status(
                id,
                RefreshResult::Failed(fah_common::error_chain(&err)),
                false,
            );
            return Err(err);
        }

        let _guard = self.compile_lock.lock().await;
        let (matcher, mut stats) = self.compile().await;
        let rules = matcher.len();
        self.swap_in(matcher, &stats);

        // One line per list, by name — the router-log signal that a list
        // refreshed and how big the combined ruleset now is. The per-compile
        // dedup detail is at `debug` (see `compile`).
        tracing::info!(list = %id, rules, "list refreshed");
        let list_stats = stats.remove(id).unwrap_or_default();
        self.record_status(id, RefreshResult::Ok(list_stats.clone()), true);
        Ok(list_stats)
    }

    /// Fetches one list and commits its raw text to the `/data` cache — the
    /// fetch+commit half of a refresh, shared by [`Self::refresh_list`] and
    /// [`Self::refresh_all`]. Holds the list's `refresh_lock` across the whole
    /// fetch+commit so a stale fetch can never commit over a newer one (that
    /// serialization is the lock's only job — the compile is deliberately left
    /// out of it), and takes `compile_lock` just for the commit. It does **not**
    /// recompile: the caller chooses when — immediately for a single refresh, or
    /// once for a whole [`Self::refresh_all`] batch. On failure the previous
    /// cached copy is untouched (RULE_ENGINE.md failure policy).
    async fn fetch_and_commit(&self, entry: &ListEntry) -> Result<(), LifecycleError> {
        let _list_guard = entry.refresh_lock.lock().await;
        let text = entry
            .source
            .fetch(&self.http, self.fetch_timeout, MAX_LIST_BYTES)
            .await?;
        let _guard = self.compile_lock.lock().await;
        self.commit_raw(&entry.id, text).await;
        Ok(())
    }

    /// Refreshes every enabled list in one pass and recompiles the combined
    /// ruleset **once** (`POST /api/v1/lists/refresh`).
    ///
    /// Best-effort, like the scheduler: a list whose fetch fails is recorded
    /// and skipped — its last-good cached copy keeps serving (RULE_ENGINE.md
    /// failure policy) and every other list still refreshes; one dead source
    /// never aborts the batch. The one thing this does that looping
    /// [`Self::refresh_list`] would not is fetch+commit all lists first and
    /// compile the 683k-rule matcher a *single* time at the end, not once per
    /// list. On the RB5009 a full compile is seconds of CPU, so collapsing N of
    /// them to one is the difference between a snappy call and a multi-second
    /// stall that also churns the served ruleset N times over.
    ///
    /// Fetches run one at a time (never concurrently) so several million-line
    /// lists are never resident together — the same RAM discipline the
    /// scheduler keeps on a 1 GB box.
    pub async fn refresh_all(&self) -> Vec<ListRefreshOutcome> {
        // Snapshot the enabled entries' handles; the read guard must not span
        // the awaits below, and a concurrent add/remove is simply reflected (or
        // not) in this pass's membership captured here.
        let entries: Vec<Arc<ListEntry>> = self
            .entries
            .read()
            .unwrap()
            .iter()
            .filter(|entry| entry.is_enabled())
            .map(Arc::clone)
            .collect();

        // Fetch + persist each list through the shared helper, preserving
        // order. The slow network fetch stays off the compile path, and the
        // expensive compile is deferred to one pass after the whole batch is on
        // disk. `Ok(())` marks a committed list, `Err` a failed fetch whose old
        // cached copy keeps serving.
        let mut committed: Vec<(Arc<str>, Result<(), String>)> = Vec::with_capacity(entries.len());
        for entry in &entries {
            let outcome = self
                .fetch_and_commit(entry)
                .await
                .map_err(|err| fah_common::error_chain(&err));
            committed.push((entry.id.clone(), outcome));
        }

        // One compile for the whole batch, then map each list to its share of
        // the result and record status.
        let _guard = self.compile_lock.lock().await;
        let (matcher, mut stats) = self.compile().await;
        let rules = matcher.len();
        self.swap_in(matcher, &stats);
        let refreshed = committed.iter().filter(|(_, r)| r.is_ok()).count();

        // Per-list lines before the summary, as in `refresh_due_lists` — this
        // path batches its compile too, so without them a manual refresh-all
        // also names none of the 16 lists it just rebuilt.
        let outcomes: Vec<ListRefreshOutcome> = committed
            .into_iter()
            .map(|(id, outcome)| {
                let result = match outcome {
                    Ok(()) => {
                        let list_stats = stats.remove(&id).unwrap_or_default();
                        tracing::info!(
                            list = %id,
                            active = list_stats.active,
                            inactive = list_stats.inactive,
                            parse_errors = list_stats.parse_errors,
                            "list refreshed"
                        );
                        self.record_status(&id, RefreshResult::Ok(list_stats.clone()), true);
                        Ok(list_stats)
                    }
                    Err(error) => {
                        self.record_status(&id, RefreshResult::Failed(error.clone()), false);
                        Err(error)
                    }
                };
                ListRefreshOutcome { id, result }
            })
            .collect();

        tracing::info!(
            lists = entries.len(),
            refreshed,
            failed = entries.len() - refreshed,
            rules,
            "refreshed all lists"
        );

        outcomes
    }

    /// Full ruleset rebuilds since start — see [`Self::compiles`].
    pub fn compile_count(&self) -> u64 {
        self.compiles.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Wall time of the most recent compile — read + parse + build — or
    /// [`Duration::ZERO`] before the first one.
    ///
    /// Zero is therefore ambiguous only until boot finishes, since startup
    /// compiles from the `/data` cache before serving. Any later reading is a
    /// real measurement, which is what makes this safe to chart directly.
    pub fn last_compile_duration(&self) -> Duration {
        Duration::from_micros(
            self.last_compile_micros
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Clones one entry's handle out from under the lock — callers await
    /// afterwards, so the guard must not outlive this call.
    fn find(&self, id: &str) -> Option<Arc<ListEntry>> {
        self.entries
            .read()
            .unwrap()
            .iter()
            .find(|entry| entry.id.as_ref() == id)
            .map(Arc::clone)
    }

    /// Every configured list's identity and refresh policy, in configuration
    /// order (`GET /api/v1/lists`).
    pub fn lists(&self) -> Vec<ListEntryView> {
        self.entries
            .read()
            .unwrap()
            .iter()
            .map(|entry| entry.view())
            .collect()
    }

    /// Registers a new list (`POST /api/v1/lists`). No fetch happens here:
    /// the list has no `/data` copy yet, so it contributes nothing to the
    /// ruleset until its first refresh — which the scheduler picks up on its
    /// next tick (it has no recorded attempt, so it is immediately due), or
    /// the caller triggers explicitly via [`Self::refresh_list`].
    pub fn add_list(&self, config: &RuleListConfig) -> Result<ListEntryView, LifecycleError> {
        let mut entries = self.entries.write().unwrap();
        if entries.iter().any(|entry| entry.id.as_ref() == config.id) {
            return Err(LifecycleError::DuplicateList(config.id.clone()));
        }
        let entry = Arc::new(ListEntry::new(config, &self.data_dir));
        let view = entry.view();
        entries.push(entry);
        drop(entries);

        self.status
            .lock()
            .unwrap()
            .insert(Arc::from(config.id.as_str()), ListStatus::default());
        Ok(view)
    }

    /// Removes a list and recompiles without it (`DELETE /api/v1/lists/{id}`).
    /// The entry drops out of the ruleset first, then its `/data` copy is
    /// deleted — a refresh already in flight holds its own `Arc` and may
    /// still write a cache file afterwards, but `compile` only reads
    /// configured lists, so the re-created copy is inert until
    /// [`Self::remove_orphaned_copies`] sweeps it on the next boot.
    pub async fn remove_list(&self, id: &str) -> Result<(), LifecycleError> {
        {
            let mut entries = self.entries.write().unwrap();
            let before = entries.len();
            entries.retain(|entry| entry.id.as_ref() != id);
            if entries.len() == before {
                return Err(LifecycleError::UnknownList(id.to_string()));
            }
        }
        self.status.lock().unwrap().remove(id);
        self.last_attempted.lock().unwrap().remove(id);
        self.pending_cache.lock().unwrap().remove(id);

        let _guard = self.compile_lock.lock().await;
        let (matcher, stats) = self.compile().await;
        self.swap_in(matcher, &stats);

        if let Err(err) = cache::remove(&self.data_dir, id).await {
            tracing::warn!(list = id, error = %err, "failed to delete cached list copy");
        }
        Ok(())
    }

    /// Enables/disables a list or changes its refresh interval
    /// (`PATCH /api/v1/lists/{id}`). Toggling `enabled` recompiles and swaps
    /// immediately, so protection changes take effect without waiting for a
    /// refresh; an interval change only affects future scheduling.
    pub async fn update_list(
        &self,
        id: &str,
        patch: &ListPatch,
    ) -> Result<ListEntryView, LifecycleError> {
        let entry = self
            .find(id)
            .ok_or_else(|| LifecycleError::UnknownList(id.to_string()))?;

        let mut membership_changed = false;
        if let Some(enabled) = patch.enabled {
            membership_changed = entry.enabled.swap(enabled, Ordering::Relaxed) != enabled;
        }
        if let Some(refresh_hours) = patch.refresh_hours {
            entry
                .refresh_hours
                .store(refresh_hours.unwrap_or(0), Ordering::Relaxed);
        }

        if membership_changed {
            let _guard = self.compile_lock.lock().await;
            let (matcher, stats) = self.compile().await;
            self.swap_in(matcher, &stats);
        }
        Ok(entry.view())
    }

    /// The current inline user rules as stored (`GET /api/v1/rules/user`), or
    /// `None` when none have ever been set.
    pub async fn user_rules(&self) -> Option<String> {
        self.list_text(USER_RULES_ID).await
    }

    /// Sets the inline user rules (RULE_ENGINE.md §Sources: User rules),
    /// validates + recompiles + atomically swaps like any list refresh.
    /// Never fails on content — bad lines are parse errors, not rejections;
    /// the only failure mode is caching the raw text to `/data`, which is
    /// best-effort (a cache-write failure still applies the new rules, it
    /// just won't survive a restart until the next successful write).
    pub async fn set_user_rules(&self, raw_text: String) -> RefreshStats {
        let _guard = self.compile_lock.lock().await;
        self.commit_raw(USER_RULES_ID, raw_text).await;

        let (matcher, mut stats) = self.compile().await;
        self.swap_in(matcher, &stats);

        let list_stats = stats.remove(USER_RULES_ID).unwrap_or_default();
        self.record_status(USER_RULES_ID, RefreshResult::Ok(list_stats.clone()), true);
        list_stats
    }

    /// Persists freshly fetched raw text to the `/data` cache — the durable
    /// source of truth recompiles read from. A failed write keeps the text in
    /// [`Self::pending_cache`] instead (best-effort: the new rules still
    /// apply, they just won't survive a restart until a write succeeds).
    /// Caller must hold `compile_lock`.
    async fn commit_raw(&self, id: &str, text: String) {
        match cache::write(&self.data_dir, id, &text).await {
            Ok(()) => {
                self.pending_cache.lock().unwrap().remove(id);
            }
            Err(err) => {
                tracing::warn!(list = id, error = %err, "failed to cache list to /data; keeping raw text in memory");
                self.pending_cache
                    .lock()
                    .unwrap()
                    .insert(Arc::from(id), text);
            }
        }
    }

    pub fn status(&self, id: &str) -> Option<ListStatus> {
        self.status.lock().unwrap().get(id).cloned()
    }

    pub fn statuses(&self) -> HashMap<Arc<str>, ListStatus> {
        self.status.lock().unwrap().clone()
    }

    /// Spawns one background task per enabled list. The first refresh lands
    /// within [`FIRST_REFRESH_JITTER`] of startup — RULE_ENGINE.md's "refresh
    /// happens asynchronously afterwards", and a first-ever boot (no `/data`
    /// cache yet) gets its protection promptly instead of at a random point
    /// inside the 24 h interval. After that, each list refreshes every
    /// `rules.refresh_hours_default` (CONFIGURATION.md); the jitter only
    /// spreads simultaneous startup bursts. The caller keeps the returned
    /// handles for shutdown; a dropped `ListManager` still leaves these
    /// running until aborted (they hold an `Arc` clone, not a borrow) — abort
    /// them explicitly on shutdown.
    pub fn spawn_scheduler(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(SCHEDULER_TICK);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                // The first tick completes immediately, so a fresh install
                // (no `/data` cache) fetches its lists right after boot.
                ticker.tick().await;
                manager.refresh_due_lists().await;
            }
        })
    }

    /// One scheduler pass: refreshes every enabled list whose interval has
    /// elapsed since its last attempt (a list never attempted is due at once),
    /// then recompiles the combined ruleset **once** for the whole batch.
    ///
    /// Fetches stay sequential on purpose — parallel downloads of several
    /// 1M-domain lists would spike RAM on a 1 GB router, and the intervals are
    /// hours.
    ///
    /// **The single compile is the point.** Looping [`Self::refresh_list`] here
    /// used to recompile per list, and because [`Self::compile`] rebuilds the
    /// *whole* combined matcher from every enabled list, N due lists meant N
    /// full rebuilds of the same corpus. Measured on the RB5009 with 16 lists
    /// coming due together: 16 compiles in 131 s drove RSS from 52 MiB to
    /// **204 MiB** — past the 128 MB PERFORMANCE.md budget and to 80 % of the
    /// 256 MB container ceiling — because each rebuild's transients (parsed
    /// rules, the new 22 MiB matcher beside the live one, the dedup index) piled
    /// up faster than the allocator could return pages. It also churned the
    /// served ruleset 16 times and burned ~16× the necessary ARM CPU.
    ///
    /// This is the same fetch-all-then-compile-once shape [`Self::refresh_all`]
    /// already used; the scheduler was the one path that did not.
    async fn refresh_due_lists(&self) {
        let now = Instant::now();
        // Snapshot handles rather than ids: the read guard must not span the
        // awaits below, and `fetch_and_commit` needs the entry itself.
        let due: Vec<Arc<ListEntry>> = self
            .entries
            .read()
            .unwrap()
            .iter()
            .filter(|entry| entry.is_enabled())
            .filter(|entry| {
                let last = self.last_attempted.lock().unwrap().get(&entry.id).copied();
                match last {
                    None => true,
                    Some(last) => now >= last + entry.interval(self.default_refresh_hours),
                }
            })
            .map(Arc::clone)
            .collect();

        if due.is_empty() {
            return;
        }

        // Fetch + persist every due list first, recording the attempt as we go
        // so a list that fails is not retried until its next interval. One dead
        // source never aborts the batch: its last-good cached copy keeps serving
        // (RULE_ENGINE.md failure policy).
        //
        // Failures are recorded here rather than after the compile below,
        // because the compile is conditional and a skipped one must not swallow
        // them — an unrecorded failure is invisible in `GET /api/v1/lists`.
        // Successes cannot be: their `RefreshStats` only exist once the compile
        // has parsed the new text.
        let mut committed: Vec<Arc<str>> = Vec::with_capacity(due.len());
        for entry in &due {
            self.last_attempted
                .lock()
                .unwrap()
                .insert(entry.id.clone(), now);
            match self.fetch_and_commit(entry).await {
                Ok(()) => committed.push(entry.id.clone()),
                Err(err) => {
                    let error = fah_common::error_chain(&err);
                    tracing::warn!(list = %entry.id, %error, "scheduled list refresh failed");
                    self.record_status(&entry.id, RefreshResult::Failed(error), false);
                }
            }
        }

        // Nothing reached `/data`, so the ruleset cannot have changed.
        // Recompiling would rebuild a byte-identical matcher and pay the whole
        // multi-second, >100 MiB transient for it.
        if committed.is_empty() {
            return;
        }

        let _guard = self.compile_lock.lock().await;
        let (matcher, mut stats) = self.compile().await;
        let rules = matcher.len();
        self.swap_in(matcher, &stats);

        // One line per list, before the summary. Collapsing the per-list
        // compiles into one (see this method's doc) also collapsed the per-list
        // `list refreshed` lines the scheduler used to emit, leaving a boot log
        // that said only "16 lists" and named none of them — a real loss of
        // signal on a router where these lines are the only view of which
        // source produced what. They are restored here with each list's *own*
        // counts rather than the combined ruleset total the old lines repeated
        // 16 times, so this is now strictly more informative than before.
        let refreshed = committed.len();
        for id in committed {
            let list_stats = stats.remove(&id).unwrap_or_default();
            tracing::info!(
                list = %id,
                active = list_stats.active,
                inactive = list_stats.inactive,
                parse_errors = list_stats.parse_errors,
                "list refreshed"
            );
            self.record_status(&id, RefreshResult::Ok(list_stats), true);
        }

        tracing::info!(
            lists = due.len(),
            refreshed,
            failed = due.len() - refreshed,
            rules,
            "scheduled refresh complete"
        );
    }

    /// One list's current raw text: the in-memory pending copy if its last
    /// cache write failed (newer than whatever is on disk), else the `/data`
    /// cache file, else `None` (never fetched yet).
    async fn list_text(&self, id: &str) -> Option<String> {
        if let Some(text) = self.pending_cache.lock().unwrap().get(id).cloned() {
            return Some(text);
        }
        cache::read(&self.data_dir, id).await
    }

    /// Re-reads every enabled list's raw text from the `/data` cache and
    /// rebuilds one combined [`Matcher`] — RULE_ENGINE.md's compiled matcher
    /// covers all lists together, so any single list's change recompiles the
    /// whole ruleset (the diagram's "compile new ruleset" step). The
    /// CPU-heavy parse+build runs on the blocking pool: seconds of work for a
    /// 1M-domain corpus on the RB5009 must not stall a tokio worker the DNS
    /// pipeline shares. Caller must hold `compile_lock`.
    async fn compile(&self) -> (Matcher, HashMap<Arc<str>, RefreshStats>) {
        // Timed from here, so the figure covers read + parse + build — the whole
        // cost of producing a new ruleset, which is what an operator watching
        // `fastadhunter_ruleset_compile_duration_seconds` is asking about.
        // PERFORMANCE.md's startup note splits those three phases; this is their
        // sum, and `bench_startup_phases` is what breaks it down.
        let started = Instant::now();
        self.compiles
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Snapshot the enabled ids before any `.await` — the read guard must
        // not span the file reads below, and a concurrent add/remove is then
        // simply picked up by the next compile.
        let enabled: Vec<Arc<str>> = self
            .entries
            .read()
            .unwrap()
            .iter()
            .filter(|entry| entry.is_enabled())
            .map(|entry| entry.id.clone())
            .collect();

        let mut texts: Vec<(Arc<str>, String)> = Vec::new();
        for id in enabled {
            if let Some(text) = self.list_text(&id).await {
                texts.push((id, text));
            }
        }
        if let Some(text) = self.list_text(USER_RULES_ID).await {
            texts.push((Arc::from(USER_RULES_ID), text));
        }

        // Snapshotted with the texts, before the blocking compile: the ruleset
        // that comes out has to be the one this policy set describes, and a
        // concurrent `set_policies` is picked up by the next compile.
        let policies = self.policies.load_full();

        tokio::task::spawn_blocking(move || {
            // One pass over the raw text bounds how many rules can come out
            // of it, so the builder's dedup index is allocated once, before
            // the first list is parsed, instead of rehashing its way up
            // through a 1M-rule compile.
            let upper_bound: usize = texts
                .iter()
                .map(|(_, text)| crate::parser::rule_upper_bound(text))
                .sum();
            let mut builder = MatcherBuilder::with_capacity(upper_bound);
            builder.set_policy_universe(policies.universe());
            let mut stats = HashMap::new();
            for (id, text) in &texts {
                let policy_mask = policies.mask_for_list(id);
                let parsed = crate::parse_rule_list(text);
                let list_stats = RefreshStats::from(&parsed);
                // One line per misread list, not one per bad line: a list whose
                // errors outnumber its rules was almost certainly detected as
                // the wrong format, and that is a single fact an operator can
                // act on. Bounded by the number of lists, so it cannot flood
                // the router log.
                if list_stats.looks_misparsed() {
                    tracing::warn!(
                        list = %id,
                        format = ?parsed.format,
                        rules = list_stats.active + list_stats.inactive,
                        parse_errors = list_stats.parse_errors,
                        "list parsed as {:?} but most lines failed — probable format \
                         misdetection; check the list's syntax",
                        parsed.format,
                    );
                }
                stats.insert(id.clone(), list_stats);
                builder.add_parsed_list_masked(id.clone(), &parsed, policy_mask);
            }
            let lists = texts.len();
            let matcher = builder.build();
            if matcher.duplicates_removed() > 0 {
                // `debug`, not `info`: every list refresh recompiles the whole
                // combined ruleset, so at INFO this repeated once per list —
                // 16 identical lines on the RB5009's boot refresh. The per-list
                // outcome is logged once, by name, in `refresh_list`.
                tracing::debug!(
                    duplicates = matcher.duplicates_removed(),
                    lists,
                    rules = matcher.len(),
                    "removed duplicate rules across lists"
                );
            }
            (matcher, stats)
        })
        .await
        .inspect(|_| {
            self.last_compile_micros.store(
                started.elapsed().as_micros() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        })
        .expect("ruleset compile task panicked")
    }

    /// The compiled policy set the ruleset in [`Self::matcher`] was built for.
    pub fn policies(&self) -> Arc<PolicySet> {
        self.policies.load_full()
    }

    /// Rebuilds and swaps the ruleset without fetching. Only a policy-set or
    /// per-policy `lists` change needs it — those decide the masks. Assignments
    /// and schedules change no mask and go live through `PolicyState::refresh`,
    /// which matters: this costs seconds of ARM CPU.
    pub async fn recompile(&self) {
        let _guard = self.compile_lock.lock().await;
        let (matcher, stats) = self.compile().await;
        self.swap_in(matcher, &stats);
    }

    /// Replaces the policy set. **Takes effect on the next compile**, because
    /// which policies exist decides the per-rule masks the ruleset carries —
    /// the caller recompiles after this, and until it does the running matcher
    /// keeps answering under the policies it was built with.
    ///
    /// Publishing to the query path is the caller's separate step
    /// (`PolicyState::refresh`).
    pub fn set_policies(&self, policies: PolicySet) {
        self.policies.store(Arc::new(policies));
    }

    /// Publishes a freshly compiled ruleset and, in the same step, records what
    /// each list contributes to it. Every compile goes through here so the
    /// reported `compiled` counts cannot drift from the matcher that is
    /// actually serving.
    fn swap_in(&self, matcher: Matcher, stats: &HashMap<Arc<str>, RefreshStats>) {
        self.matcher.store(Arc::new(matcher));

        let mut status = self.status.lock().unwrap();
        // Known ids first, so a list that dropped out of this compile (removed,
        // disabled, cache file gone) stops reporting the rules it used to
        // contribute rather than keeping the last number it ever had.
        for (id, entry) in status.iter_mut() {
            entry.compiled = stats.get(id).cloned();
        }
        for (id, list_stats) in stats {
            status.entry(id.clone()).or_default().compiled = Some(list_stats.clone());
        }
    }

    fn record_status(&self, id: &str, result: RefreshResult, refreshed_now: bool) {
        let mut status = self.status.lock().unwrap();
        let entry = status.entry(Arc::from(id)).or_default();
        if refreshed_now {
            entry.last_refreshed = Some(SystemTime::now());
        }
        entry.last_result = result;
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use fah_config::RuleListConfig;
    use fah_model::QueryType;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::source::ListSource;
    use super::*;
    use crate::matcher::MatchDecision;

    fn config_with(lists: Vec<RuleListConfig>) -> RulesConfig {
        RulesConfig {
            refresh_hours_default: 24,
            lists,
        }
    }

    fn list(id: &str, url: &str) -> RuleListConfig {
        RuleListConfig {
            id: id.to_string(),
            url: url.to_string(),
            enabled: true,
            refresh_hours: None,
        }
    }

    /// The numbers are the ones real EasyList produced while it was being
    /// detected as a plain domain list: 83 rules against 69,514 failures, and
    /// `RefreshResult::Ok`.
    #[test]
    fn a_list_read_as_the_wrong_format_is_not_reported_as_healthy() {
        let misparsed = RefreshStats {
            active: 83,
            url: 0,
            inactive: 0,
            parse_errors: 69_514,
        };
        assert!(misparsed.looks_misparsed());
    }

    #[test]
    fn ordinary_line_noise_is_not_a_misparse() {
        // A real hosts list: 93,156 rules, one bad line.
        assert!(!RefreshStats {
            active: 93_156,
            url: 0,
            inactive: 0,
            parse_errors: 1,
        }
        .looks_misparsed());
        // A short list where every line failed still stays quiet — under the
        // floor there is not enough evidence to call it a format problem.
        assert!(!RefreshStats {
            active: 0,
            url: 0,
            inactive: 0,
            parse_errors: 12,
        }
        .looks_misparsed());
        // A clean parse never reports.
        assert!(!RefreshStats::default().looks_misparsed());
    }

    /// A minimal HTTP/1.1 server for one request at a time: `respond` builds
    /// the full response body text (as a rule list would return it) or `None`
    /// to close the connection immediately (simulating an unreachable/broken
    /// upstream without needing real internet access).
    async fn serve_once(listener: TcpListener, body: &'static str) {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await; // drain the request line/headers
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.shutdown().await.unwrap();
    }

    async fn local_server(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(serve_once(listener, body));
        format!("http://{addr}/")
    }

    /// Binds a listener then drops it immediately, freeing the port while
    /// guaranteeing nothing is listening there — connecting yields a real,
    /// immediate connection-refused, no real internet needed.
    async fn unreachable_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}/")
    }

    #[tokio::test]
    async fn boot_with_no_cache_yields_empty_matcher_and_never_touches_network() {
        let data_dir = tempfile::tempdir().unwrap();
        let config = config_with(vec![list("oisd-basic", "https://small.oisd.nl")]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        manager.boot().await;

        assert!(manager.matcher().is_empty());
        let status = manager.status("oisd-basic").unwrap();
        assert_eq!(
            status.last_result,
            RefreshResult::NeverAttempted,
            "configured but never fetched must be visible as NeverAttempted"
        );
        assert_eq!(status.last_refreshed, None);
    }

    #[tokio::test]
    async fn boot_compiles_from_data_cache_without_network() {
        let data_dir = tempfile::tempdir().unwrap();
        cache::write(data_dir.path(), "oisd-basic", "||ads.example.com^\n")
            .await
            .unwrap();
        let config = config_with(vec![list("oisd-basic", "https://unreachable.invalid")]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        manager.boot().await;

        assert!(matches!(
            manager.matcher().lookup("ads.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));
        let status = manager.status("oisd-basic").unwrap();
        assert_eq!(
            status.last_refreshed, None,
            "loaded from cache, not refreshed"
        );
        assert!(matches!(status.last_result, RefreshResult::Ok(_)));
    }

    #[tokio::test]
    async fn successful_refresh_compiles_caches_and_swaps() {
        let data_dir = tempfile::tempdir().unwrap();
        let url = local_server("||ads.example.com^\n").await;
        let config = config_with(vec![list("oisd-basic", &url)]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        let stats = manager.refresh_list("oisd-basic").await.unwrap();

        assert_eq!(stats.active, 1);
        assert!(matches!(
            manager.matcher().lookup("ads.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));
        assert_eq!(
            cache::read(data_dir.path(), "oisd-basic").await.as_deref(),
            Some("||ads.example.com^\n")
        );
        let status = manager.status("oisd-basic").unwrap();
        assert!(status.last_refreshed.is_some());
        assert_eq!(status.last_result, RefreshResult::Ok(stats));
    }

    #[tokio::test]
    async fn kill_the_network_keeps_previous_ruleset_and_surfaces_failure() {
        let data_dir = tempfile::tempdir().unwrap();
        // Seed a previously-successful cache, as if an earlier refresh worked.
        cache::write(data_dir.path(), "oisd-basic", "||ads.example.com^\n")
            .await
            .unwrap();
        let dead_url = unreachable_url().await;
        let config = config_with(vec![list("oisd-basic", &dead_url)]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();
        manager.boot().await;

        // Sanity: the cached rule is serving before the failed refresh.
        assert!(matches!(
            manager.matcher().lookup("ads.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));

        let err = manager.refresh_list("oisd-basic").await.unwrap_err();
        assert!(matches!(err, LifecycleError::Fetch { .. }));

        // Verdicts unchanged — the previous compiled set keeps serving.
        assert!(matches!(
            manager.matcher().lookup("ads.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));
        let status = manager.status("oisd-basic").unwrap();
        let RefreshResult::Failed(message) = &status.last_result else {
            panic!("expected a failed refresh, got {:?}", status.last_result);
        };
        // The recorded message must carry the cause, not just reqwest's outer
        // "error sending request for url (…)" — that sentence is identical for
        // a DNS failure, a refused connection and a rejected certificate, and
        // on a distroless image it is the only diagnostic there is.
        assert!(message.starts_with(&format!("fetch {dead_url} failed")));
        assert!(
            message.to_ascii_lowercase().contains("connect"),
            "the refused connection must be named, not hidden behind reqwest's \
             generic outer message; got: {message}"
        );
    }

    /// Sets up `count` lists backed by local files, so a scheduler pass needs no
    /// network. `ListSource::from_url` treats any non-`http` url as a path under
    /// `/data`; the files live in `srcs/` so they cannot collide with the
    /// `lists/{id}.raw` cache files a commit writes.
    fn local_lists(data_dir: &std::path::Path, count: usize) -> Vec<RuleListConfig> {
        let src = data_dir.join("srcs");
        std::fs::create_dir_all(&src).unwrap();
        (0..count)
            .map(|i| {
                std::fs::write(
                    src.join(format!("l{i}.txt")),
                    format!("||ads{i}.example.com^\n"),
                )
                .unwrap();
                list(&format!("l{i}"), &format!("srcs/l{i}.txt"))
            })
            .collect()
    }

    /// The scheduler's batching is worth a test of its own: `compile` rebuilds
    /// the *whole* combined matcher from every enabled list, so N lists coming
    /// due together must produce **one** rebuild, not N.
    ///
    /// Red/green, and the numbers are the deployment's: with 16 lists this
    /// asserted 16 before the batching change. On the RB5009 those 16 compiles
    /// landed inside 131 s and drove RSS from 52 MiB to 204 MiB — past the
    /// 128 MB budget, at 80 % of the container ceiling — because each rebuild's
    /// transients outpaced the allocator's ability to return pages.
    #[tokio::test]
    async fn a_scheduler_pass_compiles_once_no_matter_how_many_lists_are_due() {
        const LISTS: usize = 16;

        let data_dir = tempfile::tempdir().unwrap();
        let configured = local_lists(data_dir.path(), LISTS);
        let manager =
            ListManager::new(&config_with(configured), data_dir.path().to_path_buf()).unwrap();

        // `boot` compiles once from whatever cache exists; count from after it
        // so the assertion is about the scheduler pass alone.
        manager.boot().await;
        let before = manager.compile_count();

        // Every list is due: none has a recorded attempt yet.
        manager.refresh_due_lists().await;

        assert_eq!(
            manager.compile_count() - before,
            1,
            "{LISTS} due lists must trigger one full rebuild, not one each"
        );

        // The batch must also have applied — one compile is only correct if it
        // is a compile of *everything* that was fetched.
        for i in 0..LISTS {
            assert!(
                matches!(
                    manager
                        .matcher()
                        .lookup(&format!("ads{i}.example.com"), &QueryType::A),
                    MatchDecision::Block(_)
                ),
                "list l{i} committed but never reached the served ruleset"
            );
        }
    }

    /// Nothing reached `/data`, so the ruleset cannot have changed. Recompiling
    /// would rebuild a byte-identical matcher and pay the whole multi-second,
    /// >100 MiB transient for it — the exact cost the batching exists to avoid.
    #[tokio::test]
    async fn a_scheduler_pass_whose_every_fetch_fails_does_not_recompile() {
        let data_dir = tempfile::tempdir().unwrap();
        // Local sources that do not exist: every fetch fails on `metadata`.
        let configured = vec![
            list("gone-a", "srcs/missing-a.txt"),
            list("gone-b", "srcs/missing-b.txt"),
        ];
        let manager =
            ListManager::new(&config_with(configured), data_dir.path().to_path_buf()).unwrap();
        manager.boot().await;
        let before = manager.compile_count();

        manager.refresh_due_lists().await;

        assert_eq!(
            manager.compile_count(),
            before,
            "an all-failed batch must not recompile an unchanged ruleset"
        );
        // Both failures must still be recorded, so the operator sees them.
        for id in ["gone-a", "gone-b"] {
            let status = manager.status(id).unwrap();
            assert!(
                matches!(status.last_result, RefreshResult::Failed(_)),
                "{id} should be recorded as failed, got {:?}",
                status.last_result
            );
        }
    }

    /// A partial batch still compiles once — the surviving list must reach the
    /// ruleset, and the dead one must not block it (RULE_ENGINE.md: one dead
    /// source never aborts the batch).
    #[tokio::test]
    async fn a_partly_failed_scheduler_pass_still_compiles_once_and_applies() {
        let data_dir = tempfile::tempdir().unwrap();
        let mut configured = local_lists(data_dir.path(), 1);
        configured.push(list("gone", "srcs/missing.txt"));
        let manager =
            ListManager::new(&config_with(configured), data_dir.path().to_path_buf()).unwrap();
        manager.boot().await;
        let before = manager.compile_count();

        manager.refresh_due_lists().await;

        assert_eq!(manager.compile_count() - before, 1);
        assert!(matches!(
            manager.matcher().lookup("ads0.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));
        assert!(matches!(
            manager.status("gone").unwrap().last_result,
            RefreshResult::Failed(_)
        ));
    }

    /// The restart tax this seeding removes, as a test: a list whose cached
    /// copy is younger than its interval must not be refetched just because
    /// the process is new. Observed on the RB5009 at 0.2.8 — boot compiled
    /// from cache, then the scheduler's first tick refetched all 16 lists and
    /// compiled the same ruleset a second time.
    ///
    /// The source here is a local path that does not exist, so any fetch would
    /// fail: `last_result` staying `Ok` is what proves none was attempted.
    /// `compile_count` alone could not — a failed batch does not recompile
    /// either.
    #[tokio::test]
    async fn a_restart_does_not_refetch_lists_whose_cached_copy_is_still_fresh() {
        let data_dir = tempfile::tempdir().unwrap();
        cache::write(data_dir.path(), "oisd-basic", "||ads.example.com^\n")
            .await
            .unwrap();
        let config = config_with(vec![list("oisd-basic", "srcs/missing.txt")]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        manager.boot().await;
        let before = manager.compile_count();

        manager.refresh_due_lists().await;

        assert_eq!(
            manager.compile_count(),
            before,
            "a cache written minutes ago is not due against a 24h interval"
        );
        assert!(
            matches!(
                manager.status("oisd-basic").unwrap().last_result,
                RefreshResult::Ok(_)
            ),
            "no fetch may be attempted, so the cache-derived status must stand"
        );
        assert!(matches!(
            manager.matcher().lookup("ads.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));
    }

    /// The other half: seeding must not stretch an interval. A cached copy
    /// older than the interval is due at the first tick exactly as before.
    #[tokio::test]
    async fn a_restart_still_refreshes_a_cached_copy_older_than_the_interval() {
        let data_dir = tempfile::tempdir().unwrap();
        let configured = local_lists(data_dir.path(), 1);
        cache::write(data_dir.path(), "l0", "||stale.example.com^\n")
            .await
            .unwrap();
        cache::set_modified(
            data_dir.path(),
            "l0",
            SystemTime::now() - Duration::from_secs(48 * 3600),
        );
        let manager =
            ListManager::new(&config_with(configured), data_dir.path().to_path_buf()).unwrap();

        manager.boot().await;
        let before = manager.compile_count();

        manager.refresh_due_lists().await;

        assert_eq!(
            manager.compile_count() - before,
            1,
            "48h against a 24h interval is due, seeded or not"
        );
        assert!(
            matches!(
                manager.matcher().lookup("ads0.example.com", &QueryType::A),
                MatchDecision::Block(_)
            ),
            "the refreshed source must have replaced the stale cached copy"
        );
    }

    /// Seeding must not turn a hand-edited `/data` into a permanently missing
    /// list. Deleting one list's cached copy — the obvious thing an operator
    /// does to force a re-download — leaves that list with no mtime to seed
    /// from, so it stays due and the first scheduler tick fetches it back.
    ///
    /// The two survivors point at source files that do not exist, so any fetch
    /// of *them* would fail: their `last_result` staying `Ok` is what proves
    /// the repair was surgical and did not drag the whole set back down the
    /// network.
    #[tokio::test]
    async fn a_hand_deleted_cache_file_is_refetched_without_refetching_the_rest() {
        let data_dir = tempfile::tempdir().unwrap();
        tokio::fs::create_dir_all(data_dir.path().join("srcs"))
            .await
            .unwrap();
        tokio::fs::write(
            data_dir.path().join("srcs").join("gone.txt"),
            "||gone.example.com^\n",
        )
        .await
        .unwrap();

        for (id, rule) in [
            ("keep-a", "||keep-a.example.com^\n"),
            ("keep-b", "||keep-b.example.com^\n"),
            ("gone", "||gone.example.com^\n"),
        ] {
            cache::write(data_dir.path(), id, rule).await.unwrap();
        }
        // The hand edit: one list's cached copy is deleted, the rest untouched.
        cache::remove(data_dir.path(), "gone").await.unwrap();

        let config = config_with(vec![
            list("keep-a", "srcs/missing-a.txt"),
            list("keep-b", "srcs/missing-b.txt"),
            list("gone", "srcs/gone.txt"),
        ]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        manager.boot().await;

        // Boot compiles from what is on disk, so the deleted list contributes
        // nothing yet — this is the hole the scheduler has to close.
        assert!(
            matches!(
                manager.matcher().lookup("gone.example.com", &QueryType::A),
                MatchDecision::Pass
            ),
            "a deleted cache copy cannot be in the boot ruleset"
        );
        let before = manager.compile_count();

        manager.refresh_due_lists().await;

        assert_eq!(
            manager.compile_count() - before,
            1,
            "the missing list must be fetched and the ruleset rebuilt once"
        );
        assert!(
            matches!(
                manager.matcher().lookup("gone.example.com", &QueryType::A),
                MatchDecision::Block(_)
            ),
            "the deleted list must be back in the serving ruleset"
        );
        assert_eq!(
            cache::read(data_dir.path(), "gone").await.as_deref(),
            Some("||gone.example.com^\n"),
            "and its cached copy restored, so the next boot needs no network"
        );

        for id in ["keep-a", "keep-b"] {
            assert!(
                matches!(
                    manager.status(id).unwrap().last_result,
                    RefreshResult::Ok(_)
                ),
                "{id} still had a fresh cached copy and must not have been refetched"
            );
        }
        // Both survivors must also still be serving: a surgical repair that
        // dropped the other lists from the matcher would be worse than the
        // redundant refetch it replaced.
        for domain in ["keep-a.example.com", "keep-b.example.com"] {
            assert!(
                matches!(
                    manager.matcher().lookup(domain, &QueryType::A),
                    MatchDecision::Block(_)
                ),
                "{domain} dropped out of the ruleset during the repair"
            );
        }
    }

    #[tokio::test]
    async fn boot_deletes_a_cached_copy_no_configured_list_claims() {
        let data_dir = tempfile::tempdir().unwrap();
        cache::write(data_dir.path(), "keep", "||keep.example.com^\n")
            .await
            .unwrap();
        // Stranded by an edit to `[[rules.lists]]` while FAH was stopped.
        cache::write(data_dir.path(), "dropped", "||dropped.example.com^\n")
            .await
            .unwrap();

        let config = config_with(vec![list("keep", "srcs/missing.txt")]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        manager.boot().await;

        assert!(
            cache::read(data_dir.path(), "dropped").await.is_none(),
            "a copy no configured list claims must be deleted"
        );
        assert_eq!(
            cache::read(data_dir.path(), "keep").await.as_deref(),
            Some("||keep.example.com^\n"),
            "the configured list's copy must survive"
        );
    }

    /// The sweep's one irreversible mistake. `user-rules` is written through
    /// the same cache but never appears in `[[rules.lists]]`, so a keep-set
    /// built from configured lists alone deletes it — and unlike every other
    /// copy it is authored, not downloaded, so nothing can bring it back.
    #[tokio::test]
    async fn boot_never_sweeps_the_user_rules_copy() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(
            ListManager::new(&config_with(vec![]), data_dir.path().to_path_buf()).unwrap(),
        );
        manager
            .set_user_rules("||mine.example.com^\n".to_string())
            .await;

        manager.boot().await;

        assert_eq!(
            manager.user_rules().await.as_deref(),
            Some("||mine.example.com^\n"),
            "user rules are not a configured list and must never be swept"
        );
        assert!(matches!(
            manager.matcher().lookup("mine.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));
    }

    /// A disabled list is excluded from `compile`, but it is still configured.
    /// Sweeping its copy would turn `PATCH {"enabled": true}` from an instant
    /// swap into a download.
    #[tokio::test]
    async fn boot_keeps_a_disabled_lists_cached_copy() {
        let data_dir = tempfile::tempdir().unwrap();
        cache::write(data_dir.path(), "off", "||off.example.com^\n")
            .await
            .unwrap();
        let config = config_with(vec![RuleListConfig {
            id: "off".to_string(),
            url: "srcs/missing.txt".to_string(),
            enabled: false,
            refresh_hours: None,
        }]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        manager.boot().await;

        assert_eq!(
            cache::read(data_dir.path(), "off").await.as_deref(),
            Some("||off.example.com^\n"),
            "a disabled list is still configured; its copy must survive"
        );
        manager
            .update_list(
                "off",
                &ListPatch {
                    enabled: Some(true),
                    refresh_hours: None,
                },
            )
            .await
            .unwrap();
        assert!(
            matches!(
                manager.matcher().lookup("off.example.com", &QueryType::A),
                MatchDecision::Block(_)
            ),
            "re-enabling must swap the kept copy in without a download"
        );
    }

    /// `write` renames from a `.raw.tmp.{pid}` sidecar. Those are left alone: a
    /// listing cannot tell a leaked one from a write in flight, and the cost of
    /// guessing wrong is a failed refresh.
    #[tokio::test]
    async fn boot_leaves_half_written_temp_files_alone() {
        let data_dir = tempfile::tempdir().unwrap();
        cache::write(data_dir.path(), "keep", "||keep.example.com^\n")
            .await
            .unwrap();
        let leftover = data_dir.path().join("lists").join("keep.raw.tmp.4242");
        tokio::fs::write(&leftover, "partial").await.unwrap();

        let config = config_with(vec![list("keep", "srcs/missing.txt")]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        manager.boot().await;

        assert!(
            leftover.exists(),
            "a `.raw.tmp.<pid>` sidecar is not a `{{id}}.raw` and must not be swept"
        );
    }

    /// The RB5009 has no battery-backed RTC: after a router reboot the
    /// container can start with a clock behind files written before it. Seeding
    /// must fall back to "due" there rather than computing a nonsense age —
    /// refetching once too often is harmless; skipping refreshes is not.
    #[tokio::test]
    async fn a_clock_behind_the_cached_copy_leaves_the_list_due() {
        let data_dir = tempfile::tempdir().unwrap();
        let configured = local_lists(data_dir.path(), 1);
        cache::write(data_dir.path(), "l0", "||stale.example.com^\n")
            .await
            .unwrap();
        cache::set_modified(
            data_dir.path(),
            "l0",
            SystemTime::now() + Duration::from_secs(3600),
        );
        let manager =
            ListManager::new(&config_with(configured), data_dir.path().to_path_buf()).unwrap();

        manager.boot().await;
        let before = manager.compile_count();

        manager.refresh_due_lists().await;

        assert_eq!(
            manager.compile_count() - before,
            1,
            "an unusable mtime must leave the list due, not silently seeded"
        );
    }

    /// `fastadhunter_ruleset_compile_duration_seconds` reported a hardcoded
    /// zero from the day it shipped until this was wired up, which made every
    /// statement about compile cost in PERFORMANCE.md unverifiable on the
    /// device. The gauge reads whatever this accessor returns, so a regression
    /// to zero here silently restores that blind spot — hence a test rather
    /// than trust.
    #[tokio::test]
    async fn a_compile_records_how_long_it_took() {
        let data_dir = tempfile::tempdir().unwrap();
        let configured = local_lists(data_dir.path(), 2);
        let manager =
            ListManager::new(&config_with(configured), data_dir.path().to_path_buf()).unwrap();

        assert_eq!(
            manager.last_compile_duration(),
            Duration::ZERO,
            "nothing has been compiled yet"
        );

        manager.boot().await;

        assert!(
            manager.last_compile_duration() > Duration::ZERO,
            "boot compiles, so the duration must be a real measurement"
        );
        // Sanity bound rather than a budget: this compiles two tiny lists, so
        // anything near a second means the clock is being read wrong (µs/ms
        // confusion is the failure this catches, not slowness).
        assert!(
            manager.last_compile_duration() < Duration::from_secs(30),
            "implausible compile duration {:?} — check the unit conversion",
            manager.last_compile_duration()
        );

        let first = manager.last_compile_duration();
        manager.refresh_due_lists().await;
        assert!(
            manager.last_compile_duration() > Duration::ZERO,
            "the scheduler's compile must overwrite the boot measurement, not clear it"
        );
        let _ = first;
    }

    #[tokio::test]
    async fn refresh_of_unknown_list_id_errors() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager =
            ListManager::new(&config_with(vec![]), data_dir.path().to_path_buf()).unwrap();
        let err = manager.refresh_list("does-not-exist").await.unwrap_err();
        assert!(matches!(err, LifecycleError::UnknownList(_)));
    }

    #[tokio::test]
    async fn disabled_list_is_excluded_from_compiled_matcher() {
        let data_dir = tempfile::tempdir().unwrap();
        cache::write(data_dir.path(), "off", "||ads.example.com^\n")
            .await
            .unwrap();
        let config = config_with(vec![RuleListConfig {
            id: "off".to_string(),
            url: "https://example.invalid".to_string(),
            enabled: false,
            refresh_hours: None,
        }]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        manager.boot().await;

        assert_eq!(
            manager.matcher().lookup("ads.example.com", &QueryType::A),
            MatchDecision::Pass
        );
    }

    #[tokio::test]
    async fn enabling_a_disabled_list_swaps_it_into_the_ruleset_immediately() {
        let data_dir = tempfile::tempdir().unwrap();
        cache::write(data_dir.path(), "off", "||ads.example.com^\n")
            .await
            .unwrap();
        let config = config_with(vec![RuleListConfig {
            id: "off".to_string(),
            url: "https://example.invalid".to_string(),
            enabled: false,
            refresh_hours: None,
        }]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();
        manager.boot().await;

        let view = manager
            .update_list(
                "off",
                &ListPatch {
                    enabled: Some(true),
                    refresh_hours: None,
                },
            )
            .await
            .unwrap();

        assert!(view.enabled);
        assert!(
            matches!(
                manager.matcher().lookup("ads.example.com", &QueryType::A),
                MatchDecision::Block(_)
            ),
            "enabling must recompile without waiting for a refresh"
        );
    }

    #[tokio::test]
    async fn add_list_registers_it_and_rejects_a_duplicate_id() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager =
            ListManager::new(&config_with(vec![]), data_dir.path().to_path_buf()).unwrap();

        let view = manager
            .add_list(&list("extra", "https://example.invalid"))
            .unwrap();
        assert_eq!(view.id, "extra");
        assert_eq!(manager.lists().len(), 1);
        assert_eq!(
            manager.status("extra").unwrap().last_result,
            RefreshResult::NeverAttempted
        );

        assert!(matches!(
            manager.add_list(&list("extra", "https://other.invalid")),
            Err(LifecycleError::DuplicateList(_))
        ));
    }

    /// Answers every hostname with 127.0.0.1, which the system resolver will
    /// not do for a `.invalid` name — so a fetch that succeeds proves the
    /// injected resolver, not `/etc/resolv.conf`, did the work.
    struct LoopbackResolver;

    impl HostResolver for LoopbackResolver {
        fn resolve(&self, _host: String) -> Resolving {
            Box::pin(async { Ok(vec![std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)]) })
        }
    }

    #[tokio::test]
    async fn list_downloads_go_through_the_injected_resolver() {
        let data_dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve_once(listener, "||ads.invalid^\n"));

        // `.invalid` is reserved as never-resolvable (RFC 2606), so the system
        // resolver cannot reach this — only LoopbackResolver can.
        let url = format!("http://lists.example.invalid:{port}/");
        let manager = ListManager::with_resolver(
            &config_with(vec![list("blocklist", &url)]),
            data_dir.path().to_path_buf(),
            Arc::new(LoopbackResolver),
        )
        .unwrap();

        let stats = manager.refresh_list("blocklist").await.unwrap();
        assert_eq!(stats.active, 1);
    }

    #[tokio::test]
    async fn compiled_counts_track_the_serving_ruleset_not_the_last_refresh() {
        let data_dir = tempfile::tempdir().unwrap();
        let url = local_server("||ads.invalid^\n").await;
        let manager = ListManager::with_resolver(
            &config_with(vec![list("blocklist", &url)]),
            data_dir.path().to_path_buf(),
            Arc::new(LoopbackResolver),
        )
        .unwrap();

        manager.refresh_list("blocklist").await.unwrap();
        let compiled = manager.status("blocklist").unwrap().compiled.unwrap();
        assert_eq!(compiled.active, 1);

        // The one-shot server is gone, so this refresh cannot succeed — but the
        // rule it already compiled keeps blocking, and must keep being counted.
        assert!(manager.refresh_list("blocklist").await.is_err());
        let status = manager.status("blocklist").unwrap();
        assert!(matches!(status.last_result, RefreshResult::Failed(_)));
        assert_eq!(
            status.compiled.unwrap().active,
            1,
            "a failed fetch leaves the previous ruleset serving"
        );

        // Disabling *does* remove it from the ruleset, so now the count drops.
        manager
            .update_list(
                "blocklist",
                &ListPatch {
                    enabled: Some(false),
                    refresh_hours: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(manager.status("blocklist").unwrap().compiled, None);
    }

    #[tokio::test]
    async fn remove_list_drops_its_rules_status_and_cached_copy() {
        let data_dir = tempfile::tempdir().unwrap();
        cache::write(data_dir.path(), "gone", "||ads.example.com^\n")
            .await
            .unwrap();
        let config = config_with(vec![list("gone", "https://example.invalid")]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();
        manager.boot().await;
        assert!(matches!(
            manager.matcher().lookup("ads.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));

        manager.remove_list("gone").await.unwrap();

        assert!(manager.lists().is_empty());
        assert!(manager.status("gone").is_none());
        assert_eq!(
            manager.matcher().lookup("ads.example.com", &QueryType::A),
            MatchDecision::Pass
        );
        assert!(
            cache::read(data_dir.path(), "gone").await.is_none(),
            "the /data copy must go with the list"
        );
        assert!(matches!(
            manager.remove_list("gone").await,
            Err(LifecycleError::UnknownList(_))
        ));
    }

    #[tokio::test]
    async fn patching_refresh_hours_is_reflected_without_touching_the_ruleset() {
        let data_dir = tempfile::tempdir().unwrap();
        let config = config_with(vec![list("a", "https://example.invalid")]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        let view = manager
            .update_list(
                "a",
                &ListPatch {
                    enabled: None,
                    refresh_hours: Some(Some(6)),
                },
            )
            .await
            .unwrap();
        assert_eq!(view.refresh_hours, Some(6));

        // Clearing the override returns the list to the global default.
        let view = manager
            .update_list(
                "a",
                &ListPatch {
                    enabled: None,
                    refresh_hours: Some(None),
                },
            )
            .await
            .unwrap();
        assert_eq!(view.refresh_hours, None);
    }

    #[tokio::test]
    async fn set_user_rules_recompiles_and_swaps() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager =
            ListManager::new(&config_with(vec![]), data_dir.path().to_path_buf()).unwrap();

        let stats = manager
            .set_user_rules("||ads.example.com^\n".to_string())
            .await;

        assert_eq!(stats.active, 1);
        assert!(matches!(
            manager.matcher().lookup("ads.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));
        assert_eq!(
            cache::read(data_dir.path(), USER_RULES_ID).await.as_deref(),
            Some("||ads.example.com^\n")
        );
    }

    #[tokio::test]
    async fn local_file_source_is_read_and_reflected_on_refresh() {
        let data_dir = tempfile::tempdir().unwrap();
        tokio::fs::write(data_dir.path().join("custom.txt"), "block.example.net\n")
            .await
            .unwrap();
        let config = config_with(vec![list("local", "custom.txt")]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        manager.refresh_list("local").await.unwrap();

        assert!(matches!(
            manager.matcher().lookup("block.example.net", &QueryType::A),
            MatchDecision::Block(_)
        ));
    }

    #[tokio::test]
    async fn concurrent_lookups_during_repeated_swaps_never_panic_or_see_a_bad_state() {
        let data_dir = tempfile::tempdir().unwrap();
        let config = config_with(vec![list("oisd-basic", "https://unused.invalid")]);
        let manager = Arc::new(ListManager::new(&config, data_dir.path().to_path_buf()).unwrap());

        // Seed an initial ruleset directly via user rules (no network needed)
        // and keep swapping it while many readers hammer the lookup path —
        // arc-swap must never let a reader observe a torn/invalid Matcher.
        manager
            .set_user_rules("||ads.example.com^\n".to_string())
            .await;

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut readers = Vec::new();
        for _ in 0..8 {
            let manager = Arc::clone(&manager);
            let stop = Arc::clone(&stop);
            readers.push(std::thread::spawn(move || {
                let mut iterations = 0u64;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = manager.matcher().lookup("ads.example.com", &QueryType::A);
                    iterations += 1;
                }
                assert!(iterations > 0);
            }));
        }

        for i in 0..200 {
            manager
                .set_user_rules(format!("||ads.example.com^\n||extra{i}.example.com^\n"))
                .await;
        }

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for reader in readers {
            reader.join().unwrap();
        }

        assert!(matches!(
            manager.matcher().lookup("ads.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_refreshes_only_enabled_lists() {
        let data_dir = tempfile::tempdir().unwrap();
        tokio::fs::write(data_dir.path().join("a.txt"), "block.example.net\n")
            .await
            .unwrap();
        tokio::fs::write(data_dir.path().join("b.txt"), "other.example.net\n")
            .await
            .unwrap();
        let config = config_with(vec![
            list("a", "a.txt"),
            RuleListConfig {
                id: "b".to_string(),
                url: "b.txt".to_string(),
                enabled: false,
                refresh_hours: None,
            },
        ]);
        let manager = Arc::new(ListManager::new(&config, data_dir.path().to_path_buf()).unwrap());

        manager.refresh_due_lists().await;

        assert!(matches!(
            manager.status("a").unwrap().last_result,
            RefreshResult::Ok(_)
        ));
        assert_eq!(
            manager.status("b").unwrap().last_result,
            RefreshResult::NeverAttempted,
            "a disabled list must never be fetched"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_skips_a_list_until_its_interval_elapses() {
        let data_dir = tempfile::tempdir().unwrap();
        tokio::fs::write(data_dir.path().join("a.txt"), "block.example.net\n")
            .await
            .unwrap();
        // 2h override against the 24h default: the pass at +3h is due, and
        // the intermediate pass at +1h is not.
        let config = config_with(vec![RuleListConfig {
            id: "a".to_string(),
            url: "a.txt".to_string(),
            enabled: true,
            refresh_hours: Some(2),
        }]);
        let manager = Arc::new(ListManager::new(&config, data_dir.path().to_path_buf()).unwrap());

        manager.refresh_due_lists().await;
        let first = manager.status("a").unwrap().last_refreshed.unwrap();

        tokio::time::advance(Duration::from_secs(3600)).await;
        manager.refresh_due_lists().await;
        assert_eq!(
            manager.status("a").unwrap().last_refreshed.unwrap(),
            first,
            "1h into a 2h interval is not due yet"
        );

        tokio::time::advance(Duration::from_secs(2 * 3600)).await;
        manager.refresh_due_lists().await;
        assert!(
            manager.status("a").unwrap().last_refreshed.unwrap() > first,
            "3h into a 2h interval must refresh"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_list_added_at_runtime_is_immediately_due() {
        let data_dir = tempfile::tempdir().unwrap();
        tokio::fs::write(data_dir.path().join("late.txt"), "late.example.net\n")
            .await
            .unwrap();
        let manager = Arc::new(
            ListManager::new(&config_with(vec![]), data_dir.path().to_path_buf()).unwrap(),
        );
        manager.refresh_due_lists().await;

        manager.add_list(&list("late", "late.txt")).unwrap();
        manager.refresh_due_lists().await;

        assert!(matches!(
            manager.matcher().lookup("late.example.net", &QueryType::A),
            MatchDecision::Block(_)
        ));
    }

    #[tokio::test]
    async fn oversized_remote_list_is_rejected() {
        let url = local_server("this body is well over the cap\n").await;
        let source = ListSource::from_url(&url, std::path::Path::new("/data"));
        let http = reqwest::Client::new();
        let err = source
            .fetch(&http, Duration::from_secs(5), 16)
            .await
            .unwrap_err();
        assert!(matches!(err, LifecycleError::TooLarge { limit: 16, .. }));
    }

    #[tokio::test]
    async fn oversized_local_list_is_rejected_before_reading() {
        let data_dir = tempfile::tempdir().unwrap();
        tokio::fs::write(
            data_dir.path().join("big.txt"),
            "this body is well over the cap\n",
        )
        .await
        .unwrap();
        let source = ListSource::from_url("big.txt", data_dir.path());
        let http = reqwest::Client::new();
        let err = source
            .fetch(&http, Duration::from_secs(5), 16)
            .await
            .unwrap_err();
        assert!(matches!(err, LifecycleError::TooLarge { limit: 16, .. }));
    }

    #[tokio::test]
    async fn refresh_of_disabled_list_is_rejected_without_fetching() {
        let data_dir = tempfile::tempdir().unwrap();
        let config = config_with(vec![RuleListConfig {
            id: "off".to_string(),
            // Unroutable url: reaching the network here would fail the test
            // slowly instead of erroring fast.
            url: "https://example.invalid".to_string(),
            enabled: false,
            refresh_hours: None,
        }]);
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();

        let err = manager.refresh_list("off").await.unwrap_err();
        assert!(matches!(err, LifecycleError::ListDisabled(_)));
    }

    #[tokio::test]
    async fn zero_refresh_hours_is_clamped_so_the_scheduler_cannot_panic() {
        let data_dir = tempfile::tempdir().unwrap();
        let config = RulesConfig {
            refresh_hours_default: 0,
            lists: vec![list("a", "https://a.invalid")],
        };
        let manager = ListManager::new(&config, data_dir.path().to_path_buf()).unwrap();
        let entry = manager.find("a").unwrap();
        assert_eq!(
            entry.interval(manager.default_refresh_hours),
            Duration::from_secs(3600),
            "zero interval must clamp to the 1h floor (an unclamped 0 would refresh every tick)"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_first_refresh_lands_promptly_after_boot() {
        let data_dir = tempfile::tempdir().unwrap();
        tokio::fs::write(data_dir.path().join("custom.txt"), "block.example.net\n")
            .await
            .unwrap();
        let config = config_with(vec![list("local", "custom.txt")]);
        let manager = Arc::new(ListManager::new(&config, data_dir.path().to_path_buf()).unwrap());

        let handle = manager.spawn_scheduler();

        // The refresh does real (fast) file I/O on the blocking pool; give it
        // real time to land, bounded by a real-clock deadline. No virtual-time
        // advance is needed — the scheduler's first tick fires immediately.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(status) = manager.status("local") {
                if matches!(status.last_result, RefreshResult::Ok(_)) {
                    break;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "first refresh did not land promptly after boot"
            );
            tokio::task::yield_now().await;
        }

        assert!(matches!(
            manager.matcher().lookup("block.example.net", &QueryType::A),
            MatchDecision::Block(_)
        ));
        handle.abort();
    }

    #[tokio::test]
    async fn user_rules_round_trip_through_the_getter() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager =
            ListManager::new(&config_with(vec![]), data_dir.path().to_path_buf()).unwrap();

        assert_eq!(manager.user_rules().await, None);

        manager
            .set_user_rules("||ads.example.com^\n".to_string())
            .await;
        assert_eq!(
            manager.user_rules().await.as_deref(),
            Some("||ads.example.com^\n")
        );
    }
}
