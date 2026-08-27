//! Every endpoint API.md documents. Each handler is thin: parse, call a
//! handle, map to the wire shape — all the logic lives in `fah-rules` and
//! behind the [`crate::ports`] traits.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, Path, Query, State, WebSocketUpgrade};
use axum::http::header::{CACHE_CONTROL, HOST, ORIGIN, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put, MethodRouter};
use axum::{Extension, Json, Router};
use fah_config::{AssignmentConfig, Config, PolicyConfig, RuleListConfig};
use fah_model::{HistoryRange, HistoryResolution, TopKind};
use fah_rules::{ListPatch, ListStatus, RefreshResult};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::auth::AuthMethod;
use crate::error::{ApiError, ApiResult};
use crate::events::{self, Event};
use crate::password::{self, Argon2Permit, RateDecision};
use crate::ports::{ClientEntry, HistorySource};
use crate::session;
use crate::state::AppState;
use crate::telemetry::{MemorySnapshot, TelemetryResponse, TelemetrySnapshot};
use crate::timestamp;
use crate::wire::*;

/// `GET /api/v1/history/*` bounds (API.md §History). The summary budget covers
/// 90 days of hourly points (2160) whole, so the default chart is never
/// silently thinned; the perf series is far denser (1440 samples/day at the
/// 60 s cadence) and is decimated by default, with `stride` saying so.
const DEFAULT_SUMMARY_POINTS: usize = 5_000;
const MAX_SUMMARY_POINTS: usize = 10_000;
const DEFAULT_PERF_POINTS: usize = 1_000;
const MAX_PERF_POINTS: usize = 5_000;
const DEFAULT_TOP_N: usize = 10;
const MAX_TOP_N: usize = 100;

/// Default windows when `from`/`to` are omitted. Top-N is stored per completed
/// day, so a 24h default would routinely serve one file or none — a week is the
/// smallest window that reliably has something in it.
const DEFAULT_SERIES_WINDOW: Duration = Duration::from_secs(24 * 3600);
const DEFAULT_TOP_WINDOW: Duration = Duration::from_secs(7 * 24 * 3600);

/// What an unassigned client is reported as (CONTEXT.md §Policy). Spelled once
/// so the stats row, the `?policy=` filter and `rules/test` agree.
const DEFAULT_POLICY: &str = "default";

pub fn router(state: Arc<AppState>) -> Router {
    let v1 = Router::new()
        .route("/stats", get(stats))
        .route("/telemetry", get(telemetry))
        .route("/clients", get(clients))
        .route("/history/summary", get(history_summary))
        .route("/history/perf", get(history_perf))
        .route("/history/top", get(history_top))
        .route("/clients/{ip}", put(set_client_name))
        .route("/lists", get(lists).post(create_list))
        // Static segment, registered before the `{id}` param so a list can
        // never be named `refresh` and shadow the refresh-all route.
        .route("/lists/refresh", post(refresh_all_lists))
        .route(
            "/lists/{id}",
            axum::routing::patch(patch_list).delete(delete_list),
        )
        .route("/lists/{id}/refresh", post(refresh_list))
        .route("/policies", get(policies).post(create_policy))
        .route(
            "/policies/{id}",
            axum::routing::patch(patch_policy).delete(delete_policy),
        )
        .route(
            "/clients/{ip}/policy",
            get(get_client_policy)
                .put(set_client_policy)
                .delete(clear_client_policy),
        )
        .route("/rules/user", get(get_user_rules).put(put_user_rules))
        .route("/rules/test", post(test_rule))
        .route("/cache", get(cache_stats))
        .route("/cache/clean", post(cache_clean))
        .route("/config", get(get_config).post(post_config))
        .route("/config/apikey/rotate", post(rotate_api_key))
        .route("/debug/memory", get(debug_memory))
        .route("/events", get(events_socket))
        .route("/auth/login", no_store(post(auth_login)))
        .route("/auth/logout", no_store(post(auth_logout)))
        .route("/auth/logout-all", no_store(post(auth_logout_all)))
        .route("/auth/password", no_store(post(auth_password)))
        .fallback(not_found);

    let api = Router::new().nest("/v1", v1).fallback(not_found);

    Router::new()
        .route("/health", get(health))
        .route("/api/", any(not_found))
        .nest("/api", api)
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            crate::auth::require_auth,
        ))
        .with_state(state)
        .merge(crate::web::mounted())
}

async fn not_found() -> ApiError {
    ApiError::NotFound("no such endpoint".to_string())
}

fn no_store(method: MethodRouter<Arc<AppState>>) -> MethodRouter<Arc<AppState>> {
    method.layer(SetResponseHeaderLayer::if_not_present(
        CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    ))
}

// ─── Health & telemetry ────────────────────────────────────────────────

async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: if state.telemetry.degraded() {
            "degraded"
        } else {
            "ok"
        },
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: state.uptime_seconds(),
    })
}

// ─── Cache & memory ────────────────────────────────────────────────────

async fn cache_stats(State(state): State<Arc<AppState>>) -> Json<CacheStatsResponse> {
    Json(state.cache.stats().into())
}

async fn cache_clean(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CacheCleanParams>,
) -> Json<CacheCleanResponse> {
    Json(state.cache.clean(params.stale).into())
}

/// Where the RAM goes, for chasing the PERFORMANCE.md budget on-device: every
/// bounded component reports its own heap, and `residual_bytes` is what RSS
/// holds beyond them — binary pages, thread stacks, the tokio runtime and
/// allocator retention.
///
/// The residual is the number to watch: growth there while the components stay
/// flat is the leak signal, because the growth you legitimately expect has been
/// subtracted out (p2-07).
///
/// The allocator counters do **not** split it further. `allocator_committed_*`
/// is a lifetime high-water mark that routinely exceeds `process_rss`, so
/// nothing derived from it describes memory currently held — see
/// `MemoryResponse::allocator_committed_bytes`. `minor_page_faults` is the one
/// to pair with the residual: a rising fault rate at flat RSS is purge thrash,
/// not a leak.
async fn debug_memory(State(state): State<Arc<AppState>>) -> Json<DebugMemoryResponse> {
    // Same gathering site `/telemetry`'s memory block uses, so the two cannot
    // report a different RSS or residual for the same instant — and `accounted`
    // / `residual` stay defined once, in `fah_model::MemoryBreakdown` (p2-07).
    // Not the whole `TelemetrySnapshot`: the engine read is pure waste here.
    let snapshot = MemorySnapshot::collect(&state);
    Json(DebugMemoryResponse::of(
        snapshot.breakdown(),
        snapshot.cache_entries(),
    ))
}

/// The whole engine state in one JSON request: ruleset, lifetime counters,
/// per-stage latency totals, upstreams, cache and memory (see
/// [`crate::telemetry`]).
///
/// Everything here is produced by FastAdHunter or by the kernel, so the shape
/// is a stable contract. Allocator-specific figures stay on `/debug/memory`,
/// which promises nothing — swapping the allocator must not break a dashboard.
async fn telemetry(State(state): State<Arc<AppState>>) -> Json<TelemetryResponse> {
    Json(TelemetrySnapshot::collect(&state).into())
}

// ─── Statistics ────────────────────────────────────────────────────────

async fn stats(State(state): State<Arc<AppState>>) -> Json<StatsResponse> {
    Json(state.stats.overview(SystemTime::now()).into())
}

/// An RFC 3339 `from`/`to` query parameter, as the history endpoints document
/// it.
fn timestamp_param(params: &HashMap<String, String>, key: &str) -> ApiResult<Option<SystemTime>> {
    match params.get(key) {
        Some(raw) => timestamp::from_rfc3339(raw).map(Some).ok_or_else(|| {
            ApiError::BadRequest(format!("{key} must be an RFC 3339 timestamp, got {raw:?}"))
        }),
        None => Ok(None),
    }
}

// ─── History (persisted series) ────────────────────────────────────────

async fn history_summary(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Json<HistorySummaryResponse>> {
    let range = parse_range(&params, DEFAULT_SERIES_WINDOW)?;
    let (resolution, label) = match params.get("resolution").map(String::as_str) {
        None | Some("hour") => (HistoryResolution::Hour, "hour"),
        Some("day") => (HistoryResolution::Day, "day"),
        Some(other) => {
            return Err(ApiError::BadRequest(format!(
                "resolution must be one of hour|day, got {other:?}"
            )))
        }
    };
    let max_points = parse_bounded(
        &params,
        "max_points",
        DEFAULT_SUMMARY_POINTS,
        MAX_SUMMARY_POINTS,
    )?;

    let series = read_history(&state.history, move |history| {
        history.summary(range, resolution, max_points)
    })
    .await?;
    Ok(Json(HistorySummaryResponse::new(
        label, range.from, range.to, series,
    )))
}

async fn history_perf(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Json<HistoryPerfResponse>> {
    let range = parse_range(&params, DEFAULT_SERIES_WINDOW)?;
    let fields = parse_perf_fields(&params)?;
    let max_points = parse_bounded(&params, "max_points", DEFAULT_PERF_POINTS, MAX_PERF_POINTS)?;

    let series = read_history(&state.history, move |history| {
        history.perf(range, max_points)
    })
    .await?;
    Ok(Json(HistoryPerfResponse::new(
        range.from, range.to, series, fields,
    )))
}

async fn history_top(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Json<HistoryTopResponse>> {
    let range = parse_range(&params, DEFAULT_TOP_WINDOW)?;
    let (kind, label) = match params.get("kind").map(String::as_str) {
        None | Some("blocked") => (TopKind::Blocked, "blocked"),
        Some("queried") => (TopKind::Queried, "queried"),
        Some("clients") => (TopKind::Clients, "clients"),
        Some(other) => {
            return Err(ApiError::BadRequest(format!(
                "kind must be one of blocked|queried|clients, got {other:?}"
            )))
        }
    };
    let limit = parse_bounded(&params, "n", DEFAULT_TOP_N, MAX_TOP_N)?;

    let items = read_history(&state.history, move |history| {
        history.top(range, kind, limit)
    })
    .await?;
    Ok(Json(HistoryTopResponse::new(
        label, range.from, range.to, items,
    )))
}

/// Runs a history read on a blocking thread. The port is `std::fs` by design
/// ([`HistorySource`]): scanning a 90-day range on a runtime worker would park
/// every other request behind it.
async fn read_history<T: Send + 'static>(
    history: &Arc<dyn HistorySource>,
    read: impl FnOnce(&dyn HistorySource) -> std::io::Result<T> + Send + 'static,
) -> ApiResult<T> {
    let history = Arc::clone(history);
    tokio::task::spawn_blocking(move || read(history.as_ref()))
        .await
        .map_err(|err| ApiError::Internal(format!("history read task failed: {err}")))?
        .map_err(|err| ApiError::Internal(format!("reading /data/history: {err}")))
}

/// The `from`/`to` window of a history request. `to` defaults to now and `from`
/// to `to - default_window`, so a parameterless call still charts something.
///
/// An empty or inverted window is a `400`, not an empty `200`: a range with no
/// data in it is a normal answer, but a range that *cannot* contain data is a
/// mistake in the request, and reporting the two identically would leave a
/// caller staring at an empty chart looking for the outage.
fn parse_range(
    params: &HashMap<String, String>,
    default_window: Duration,
) -> ApiResult<HistoryRange> {
    let to = timestamp_param(params, "to")?.unwrap_or_else(SystemTime::now);
    let from = match timestamp_param(params, "from")? {
        Some(from) => from,
        None => to
            .checked_sub(default_window)
            .unwrap_or(SystemTime::UNIX_EPOCH),
    };
    if from >= to {
        return Err(ApiError::BadRequest(
            "from must be earlier than to".to_string(),
        ));
    }
    Ok(HistoryRange { from, to })
}

/// A positive integer query parameter, clamped to its documented ceiling
/// (the same clamping contract every bounded list parameter uses).
fn parse_bounded(
    params: &HashMap<String, String>,
    key: &str,
    default: usize,
    max: usize,
) -> ApiResult<usize> {
    match params.get(key) {
        Some(raw) => Ok(raw
            .parse::<usize>()
            .map_err(|_| ApiError::BadRequest(format!("{key} must be a number, got {raw:?}")))?
            .clamp(1, max)),
        None => Ok(default),
    }
}

/// `?fields=` for the perf series: a comma-separated subset of the response
/// keys. Omitted — or present but empty — means everything; an unknown name is
/// rejected rather than ignored, so a typo cannot silently drop the series a
/// chart was asking for.
fn parse_perf_fields(params: &HashMap<String, String>) -> ApiResult<PerfFields> {
    let Some(raw) = params.get("fields") else {
        return Ok(PerfFields::ALL);
    };
    let mut fields = PerfFields::NONE;
    let mut named_any = false;
    for name in raw
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        if !fields.enable(name) {
            return Err(ApiError::BadRequest(format!(
                "unknown field {name:?}; fields must be a comma-separated subset of {}",
                PerfFields::NAMES.join(", ")
            )));
        }
        named_any = true;
    }
    Ok(if named_any { fields } else { PerfFields::ALL })
}

// ─── Clients ───────────────────────────────────────────────────────────

const DIRECT_ASSIGNMENT: &str = "direct";

fn client_response(
    entry: ClientEntry,
    policy: String,
    assignment_source: Option<&'static str>,
) -> ClientResponse {
    ClientResponse {
        policy,
        assignment_source,
        ip: entry.ip,
        name: entry.name,
        first_seen: entry.first_seen,
        last_seen: entry.last_seen,
        queries_24h: entry.queries_24h,
        blocked_24h: entry.blocked_24h,
    }
}

async fn clients(State(state): State<Arc<AppState>>) -> Json<ClientsResponse> {
    let resolver = PolicyResolver::build(&state);
    Json(ClientsResponse {
        items: state
            .stats
            .clients(SystemTime::now())
            .into_iter()
            .map(|entry| {
                let ip = entry.ip;
                client_response(
                    entry,
                    resolver.policy_of(ip),
                    resolver.is_direct(ip).then_some(DIRECT_ASSIGNMENT),
                )
            })
            .collect(),
    })
}

async fn set_client_name(
    State(state): State<Arc<AppState>>,
    Path(ip): Path<String>,
    Json(body): Json<ClientNameRequest>,
) -> ApiResult<Json<ClientResponse>> {
    let ip: IpAddr = ip
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("{ip:?} is not an IP address")))?;

    // An empty name is a clear, not a client literally named "".
    let name = body.name.filter(|name| !name.trim().is_empty());

    let entry = state
        .stats
        .set_client_name(ip, name)
        .ok_or_else(|| ApiError::NotFound(format!("no client seen at {ip}")))?;
    // A rename can move the client into or out of a name assignment, and the
    // snapshot resolved names when it was built.
    republish_policies(&state);

    let config = state.config.current();
    let key = assignment_key(ip);
    Ok(Json(client_response(
        entry,
        policy_in_force(&state.policies.current(), ip),
        direct_assignment(&config, &key).map(|_| DIRECT_ASSIGNMENT),
    )))
}

// ─── Rule lists ────────────────────────────────────────────────────────

async fn lists(State(state): State<Arc<AppState>>) -> Json<ListsResponse> {
    let default_hours = state.config.current().rules.refresh_hours_default;
    let statuses = state.rules.statuses();

    let items = state
        .rules
        .lists()
        .into_iter()
        .map(|entry| {
            let status = statuses.get(entry.id.as_str()).cloned().unwrap_or_default();
            list_response(entry, &status, default_hours)
        })
        .collect();
    // Ruleset-wide totals live on the envelope, not on an item: after the
    // merge dedups identical rules, "how many rules are actually compiled"
    // is a property of the combination of lists, not of any one of them.
    let matcher = state.rules.matcher();
    Json(ListsResponse {
        items,
        compiled_rules: matcher.len(),
        duplicates_removed: matcher.duplicates_removed(),
    })
}

fn list_response(
    entry: fah_rules::ListEntryView,
    status: &ListStatus,
    default_hours: u32,
) -> ListResponse {
    // `degraded` is a *fetch that succeeded* over a list whose lines mostly
    // failed to parse — the signature of a format misdetection. It reports as
    // its own status because `ok` hid exactly this: a list yielding 83 rules
    // and 69,514 errors used to be indistinguishable from a healthy one.
    let last_status = status_label(&status.last_result);
    // Counts describe the ruleset that is *serving*, not the last refresh
    // attempt. A failed refresh keeps the previous ruleset live
    // (RULE_ENGINE.md failure policy), so `last_status: "failed"` alongside a
    // non-zero `rules_total` is the correct — and operationally important —
    // report: the fetch broke, protection did not.
    let (active, url_active, inactive, parse_errors) =
        status.compiled.as_ref().map_or((0, 0, 0, 0), |stats| {
            (stats.active, stats.url, stats.inactive, stats.parse_errors)
        });
    ListResponse {
        id: entry.id,
        url: entry.url,
        format: "auto",
        enabled: entry.enabled,
        refresh_hours: entry.refresh_hours.unwrap_or(default_hours),
        last_refresh: status.last_refreshed,
        last_status,
        rules_total: active + url_active + inactive,
        rules_active_dns: active,
        rules_active_url: url_active,
        rules_inactive: inactive,
        parse_errors,
        last_error: status.last_result.failure_message(),
    }
}

fn status_label(result: &RefreshResult) -> &'static str {
    match result {
        RefreshResult::Ok(stats) if stats.looks_misparsed() => "degraded",
        RefreshResult::Ok(_) => "ok",
        RefreshResult::Failed(_) => "failed",
        RefreshResult::Rejected(_) => "rejected",
        RefreshResult::NeverAttempted => "never",
    }
}

async fn create_list(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateListRequest>,
) -> ApiResult<(StatusCode, Json<ListResponse>)> {
    // API.md offers either a remote `url` or a mounted-file `path`; the
    // lifecycle's `ListSource` distinguishes them by the string itself, so
    // both land in the same field.
    let source = match (body.url.as_deref(), body.path.as_deref()) {
        (Some(url), None) => url.to_string(),
        (None, Some(path)) => path.to_string(),
        (Some(_), Some(_)) => {
            return Err(ApiError::ValidationFailed(
                "provide either url or path, not both".to_string(),
            ))
        }
        (None, None) => {
            return Err(ApiError::ValidationFailed(
                "one of url or path is required".to_string(),
            ))
        }
    };

    // A mounted file's path joins onto the data dir, where `..` segments
    // would escape it. The caller is the authenticated admin, so this is
    // hardening, not an auth boundary — but traversal must die here, before
    // any `Path::join`.
    if let Some(path) = body.path.as_deref() {
        if std::path::Path::new(path)
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(ApiError::ValidationFailed(
                "path must not contain '..'".to_string(),
            ));
        }
    }

    let (id, id_was_derived) = match body.id {
        Some(id) => (id, false),
        None => (derive_id(&source), true),
    };
    if id.is_empty() {
        return Err(ApiError::ValidationFailed(
            "could not derive a list id; provide one explicitly".to_string(),
        ));
    }
    validate_list_id(&id)?;

    let config = RuleListConfig {
        id,
        url: source,
        enabled: body.enabled,
        refresh_hours: body.refresh_hours,
    };

    let _guard = state.list_mutations.lock().await;
    let current = configured_lists(&state);
    if current.iter().any(|list| list.id == config.id) {
        // Derivation collides across sources that merely share a filename —
        // StevenBlack's `hosts` and 1Hosts' `Xtra/hosts.txt` both derive to
        // `hosts`. Say so, rather than leaving the caller to guess why an id
        // they never chose is taken.
        let hint = if id_was_derived {
            " — id was derived from the source; pass an explicit `id` to disambiguate"
        } else {
            ""
        };
        return Err(ApiError::Conflict(format!(
            "list {} already exists{hint}",
            config.id
        )));
    }
    // Two ids over one source would fetch, cache and compile it twice — on a
    // 1 GB box that is a silent doubling of the largest thing in memory.
    if let Some(existing) = current.iter().find(|list| list.url == config.url) {
        return Err(ApiError::Conflict(format!(
            "{} is already configured as list {}",
            config.url, existing.id
        )));
    }
    // The new set is the current one plus this entry — borrowed, not moved in,
    // so `config` is still ours to hand to the engine below.
    persist_lists(&state, current.iter().chain([&config]))?;

    let entry = match state.rules.add_list(&config) {
        Ok(entry) => entry,
        Err(err) => {
            // The file already promises this list but the engine refused it:
            // put the file back so the two never disagree (API.md
            // §Persistence). Best-effort — a failure here means the write
            // path itself is broken, which the returned 500 already conveys.
            let _ = persist_lists(&state, &current);
            return Err(match err {
                fah_rules::LifecycleError::DuplicateList(id) => {
                    ApiError::Conflict(format!("list {id} already exists"))
                }
                other => ApiError::Internal(other.to_string()),
            });
        }
    };

    let default_hours = state.config.current().rules.refresh_hours_default;
    Ok((
        StatusCode::CREATED,
        Json(list_response(entry, &ListStatus::default(), default_hours)),
    ))
}

/// A readable, stable id from a URL or path: the file stem where there is
/// one, else the host. `https://small.oisd.nl` → `small.oisd.nl`;
/// `/data/lists/local.txt` → `local`. Lowercased, so a derived id always
/// satisfies [`validate_list_id`]'s alphabet.
fn derive_id(source: &str) -> String {
    let trimmed = source.trim_end_matches('/');
    let tail = trimmed.rsplit('/').next().unwrap_or(trimmed);
    let stem = tail.split('?').next().unwrap_or(tail);
    let stem = stem.strip_suffix(".txt").unwrap_or(stem);
    if stem.is_empty() || stem.contains(':') {
        // No path segment at all (a bare `https://host`): fall back to the
        // host, which is what the remaining text is.
        return trimmed
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_ascii_lowercase();
    }
    stem.to_ascii_lowercase()
}

/// List ids become file names (`/data/lists/{id}.raw`), TOML keys and API
/// path segments, so the accepted alphabet is locked down at the boundary:
/// lowercase alphanumerics plus `.`/`_`/`-`, not starting with a dot. Rules
/// out traversal (`../`), separators, and ids that would surprise in a file
/// listing or a metrics label.
fn validate_list_id(id: &str) -> ApiResult<()> {
    let alphabet_ok = id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'));
    if alphabet_ok && !id.starts_with('.') {
        return Ok(());
    }
    Err(ApiError::ValidationFailed(format!(
        "invalid list id {id:?}: use lowercase letters, digits, '.', '_' or '-', \
         and do not start with '.'"
    )))
}

async fn patch_list(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<PatchListRequest>,
) -> ApiResult<Json<ListResponse>> {
    let patch = ListPatch {
        enabled: body.enabled,
        refresh_hours: body.refresh_hours,
    };

    let _guard = state.list_mutations.lock().await;
    let previous = configured_lists(&state);
    let mut next = previous.clone();
    let target = next
        .iter_mut()
        .find(|list| list.id == id)
        .ok_or_else(|| ApiError::NotFound(format!("no such list: {id}")))?;
    if let Some(enabled) = patch.enabled {
        target.enabled = enabled;
    }
    if let Some(refresh_hours) = patch.refresh_hours {
        target.refresh_hours = refresh_hours;
    }
    persist_lists(&state, &next)?;

    let entry = match state.rules.update_list(&id, &patch).await {
        Ok(entry) => entry,
        Err(err) => {
            // Persisted but not applied: restore the file so it keeps
            // describing the running engine (API.md §Persistence).
            let _ = persist_lists(&state, &previous);
            return Err(unknown_list(&id, err));
        }
    };

    let default_hours = state.config.current().rules.refresh_hours_default;
    let status = state.rules.status(&id).unwrap_or_default();
    Ok(Json(list_response(entry, &status, default_hours)))
}

async fn delete_list(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let _guard = state.list_mutations.lock().await;
    let previous = configured_lists(&state);
    let mut next = previous.clone();
    next.retain(|list| list.id != id);
    if next.len() == previous.len() {
        return Err(ApiError::NotFound(format!("no such list: {id}")));
    }
    persist_lists(&state, &next)?;

    if let Err(err) = state.rules.remove_list(&id).await {
        // Persisted but not applied: restore the file so it keeps describing
        // the running engine (API.md §Persistence).
        let _ = persist_lists(&state, &previous);
        return Err(unknown_list(&id, err));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The configured list set as it currently stands in the engine — the shape
/// `[[rules.lists]]` wants, so a mutation is "read this, change it, write it
/// back".
fn configured_lists(state: &AppState) -> Vec<RuleListConfig> {
    state
        .rules
        .lists()
        .into_iter()
        .map(|view| RuleListConfig {
            id: view.id,
            url: view.url,
            enabled: view.enabled,
            refresh_hours: view.refresh_hours,
        })
        .collect()
}

/// Writes a list set back to `fastadhunter.toml` through the same validated,
/// atomic path `POST /api/v1/config` uses.
///
/// Callers persist *before* touching the `ListManager`: a list that lives only
/// in memory serves traffic until the next restart and then silently vanishes,
/// taking its rules with it, so the durable record has to be the thing that
/// can fail. Writing first means a failed write leaves the engine and the file
/// still agreeing — the mutation simply did not happen.
/// Takes references rather than an owned set so a caller adding an entry can
/// lend the one it still needs afterwards.
fn persist_lists<'a>(
    state: &AppState,
    lists: impl IntoIterator<Item = &'a RuleListConfig>,
) -> ApiResult<()> {
    let lists: Vec<&RuleListConfig> = lists.into_iter().collect();
    let patch = serde_json::json!({ "rules": { "lists": lists } });
    state
        .config
        .apply_patch(&patch)
        .map_err(|err| ApiError::Internal(format!("persisting the list set: {err}")))?;
    Ok(())
}

/// `202 Accepted`: the refresh runs in the background and its outcome shows
/// up in `last_status` (API.md) and as a `list_refreshed` event.
async fn refresh_list(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if state.rules.status(&id).is_none() {
        return Err(ApiError::NotFound(format!("no such list: {id}")));
    }

    let rules = Arc::clone(&state.rules);
    let events = state.events.clone();
    tokio::spawn(async move {
        let status = match rules.refresh_list(&id).await {
            Ok(stats) => status_label(&RefreshResult::Ok(stats)),
            Err(err) => {
                // A manual refresh is what an operator reaches for when a list
                // is failing, so this line has to name the actual cause.
                let result = RefreshResult::from_error(&err);
                let error = result.failure_message().unwrap_or_default();
                tracing::warn!(list = %id, %error, "manual list refresh failed");
                status_label(&result)
            }
        };
        events.publish(Event::ListRefreshed { id, status });
    });

    Ok(StatusCode::ACCEPTED)
}

/// `POST /api/v1/lists/refresh`: refresh every enabled list in one pass and
/// recompile the ruleset once. **Synchronous**, unlike the per-list `202`: the
/// caller wants to know the result, so this blocks until the whole batch has
/// been fetched and the ruleset rebuilt, then returns which lists refreshed and
/// which failed. Best-effort — one dead source never aborts the batch
/// (RULE_ENGINE.md failure policy), and each list still emits the same
/// `list_refreshed` event a dashboard listens for.
async fn refresh_all_lists(State(state): State<Arc<AppState>>) -> Json<RefreshAllResponse> {
    let outcomes = state.rules.refresh_all().await;

    let mut refreshed = 0usize;
    let mut failed = 0usize;
    let results = outcomes
        .into_iter()
        .map(|outcome| {
            let id = outcome.id.to_string();
            let status = status_label(&outcome.result);
            state.events.publish(Event::ListRefreshed {
                id: id.clone(),
                status,
            });
            match outcome.result {
                RefreshResult::Ok(stats) => {
                    refreshed += 1;
                    ListRefreshResult {
                        id,
                        status,
                        rules_active_dns: Some(stats.active),
                        error: None,
                    }
                }
                result => {
                    failed += 1;
                    let error = result.failure_message().unwrap_or_default();
                    tracing::warn!(list = %id, %error, "list refresh failed in refresh-all");
                    ListRefreshResult {
                        id,
                        status,
                        rules_active_dns: None,
                        error: Some(error),
                    }
                }
            }
        })
        .collect();

    Json(RefreshAllResponse {
        refreshed,
        failed,
        results,
    })
}

fn unknown_list(id: &str, err: fah_rules::LifecycleError) -> ApiError {
    match err {
        fah_rules::LifecycleError::UnknownList(_) => {
            ApiError::NotFound(format!("no such list: {id}"))
        }
        other => ApiError::Internal(other.to_string()),
    }
}

// ─── Policies ──────────────────────────────────────────────────────────

async fn policies(State(state): State<Arc<AppState>>) -> Json<PoliciesResponse> {
    let config = state.config.current();
    Json(PoliciesResponse {
        timezone: config.schedule.timezone.clone(),
        items: config.policies.iter().map(policy_response).collect(),
        active_assignments: state.policies.current().len(),
    })
}

fn policy_response(config: &PolicyConfig) -> PolicyResponse {
    PolicyResponse {
        id: config.id.clone(),
        name: config.name.clone().unwrap_or_else(|| config.id.clone()),
        lists: config.lists.clone(),
        blocking_mode: config.blocking_mode.clone(),
        assignments: config.assignments.iter().map(assignment_response).collect(),
    }
}

fn assignment_response(config: &AssignmentConfig) -> AssignmentResponse {
    AssignmentResponse {
        client: config.client.clone(),
        days: config.days.clone(),
        start: config.start.clone(),
        end: config.end.clone(),
    }
}

fn assignment_config(body: &AssignmentResponse) -> AssignmentConfig {
    AssignmentConfig {
        client: body.client.clone(),
        days: body.days.clone(),
        start: body.start.clone(),
        end: body.end.clone(),
    }
}

async fn create_policy(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreatePolicyRequest>,
) -> ApiResult<(StatusCode, Json<PolicyResponse>)> {
    validate_policy_id(&body.id)?;

    let _guard = state.policy_mutations.lock().await;
    let mut next = state.config.current().policies.clone();
    if next.iter().any(|policy| policy.id == body.id) {
        return Err(ApiError::Conflict(format!(
            "policy {} already exists",
            body.id
        )));
    }
    next.push(PolicyConfig {
        id: body.id,
        name: body.name,
        lists: body.lists,
        blocking_mode: body.blocking_mode,
        assignments: body.assignments.iter().map(assignment_config).collect(),
    });

    // A new policy adds a bit to every rule's mask, so the ruleset has to be
    // rebuilt before it can decide anything.
    apply_policies(&state, next, Recompile::Yes).await?;
    let config = state.config.current();
    let created = config
        .policies
        .last()
        .expect("the policy just persisted is present");
    Ok((StatusCode::CREATED, Json(policy_response(created))))
}

async fn patch_policy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<PatchPolicyRequest>,
) -> ApiResult<Json<PolicyResponse>> {
    let _guard = state.policy_mutations.lock().await;
    let mut next = state.config.current().policies.clone();
    let target = next
        .iter_mut()
        .find(|policy| policy.id == id)
        .ok_or_else(|| ApiError::NotFound(format!("no such policy: {id}")))?;

    if let Some(name) = body.name {
        target.name = Some(name);
    }
    // Only a `lists` change moves the masks; everything else here is a
    // read of the policy set, not of the compiled ruleset.
    let mut recompile = Recompile::No;
    if let Some(lists) = body.lists {
        if target.lists != lists {
            recompile = Recompile::Yes;
        }
        target.lists = lists;
    }
    if let Some(mode) = body.blocking_mode {
        target.blocking_mode = mode;
    }
    if let Some(assignments) = body.assignments {
        target.assignments = assignments.iter().map(assignment_config).collect();
    }

    apply_policies(&state, next, recompile).await?;
    let config = state.config.current();
    let updated = config
        .policies
        .iter()
        .find(|policy| policy.id == id)
        .expect("the policy just persisted is present");
    Ok(Json(policy_response(updated)))
}

async fn delete_policy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let _guard = state.policy_mutations.lock().await;
    let mut next = state.config.current().policies.clone();
    let before = next.len();
    next.retain(|policy| policy.id != id);
    if next.len() == before {
        return Err(ApiError::NotFound(format!("no such policy: {id}")));
    }

    apply_policies(&state, next, Recompile::Yes).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_client_policy(
    State(state): State<Arc<AppState>>,
    Path(ip): Path<String>,
) -> ApiResult<Json<ClientPolicyResponse>> {
    let ip = parse_ip(&ip)?;
    Ok(Json(client_policy_response(&state, ip)))
}

async fn set_client_policy(
    State(state): State<Arc<AppState>>,
    Path(ip): Path<String>,
    Json(body): Json<ClientPolicyRequest>,
) -> ApiResult<Json<ClientPolicyResponse>> {
    let ip = parse_ip(&ip)?;
    let assignment = AssignmentResponse {
        client: ip.to_string(),
        days: body.days,
        start: body.start,
        end: body.end,
    };

    let _guard = state.policy_mutations.lock().await;
    let mut next = state.config.current().policies.clone();
    // One assignment per address: the same client listed under two policies
    // would resolve by config order, which is not something an operator
    // asking "put this device on kids" is choosing.
    for policy in &mut next {
        policy
            .assignments
            .retain(|existing| existing.client != assignment.client);
    }
    let target = next
        .iter_mut()
        .find(|policy| policy.id == body.policy)
        .ok_or_else(|| ApiError::NotFound(format!("no such policy: {}", body.policy)))?;
    target.assignments.push(assignment_config(&assignment));

    // An assignment changes no mask — no recompile, so this is live in
    // milliseconds rather than seconds.
    apply_policies(&state, next, Recompile::No).await?;
    Ok(Json(client_policy_response(&state, ip)))
}

async fn clear_client_policy(
    State(state): State<Arc<AppState>>,
    Path(ip): Path<String>,
) -> ApiResult<StatusCode> {
    let ip = parse_ip(&ip)?;
    let client = ip.to_string();

    let _guard = state.policy_mutations.lock().await;
    let mut next = state.config.current().policies.clone();
    let before: usize = next.iter().map(|policy| policy.assignments.len()).sum();
    for policy in &mut next {
        policy
            .assignments
            .retain(|existing| existing.client != client);
    }
    let after: usize = next.iter().map(|policy| policy.assignments.len()).sum();
    if before == after {
        return Err(ApiError::NotFound(format!("no assignment for {ip}")));
    }

    apply_policies(&state, next, Recompile::No).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn assignment_key(ip: IpAddr) -> String {
    ip.to_string()
}

fn policy_in_force(active: &fah_rules::ActivePolicies, ip: IpAddr) -> String {
    active
        .id_of(active.policy_for(ip))
        .map_or_else(|| DEFAULT_POLICY.to_string(), |id| id.to_string())
}

fn direct_assignment<'a>(config: &'a Config, key: &str) -> Option<&'a AssignmentConfig> {
    config
        .policies
        .iter()
        .flat_map(|policy| &policy.assignments)
        .find(|assignment| assignment.client == key)
}

struct PolicyResolver {
    active: Arc<fah_rules::ActivePolicies>,
    direct: HashSet<String>,
}

impl PolicyResolver {
    fn build(state: &AppState) -> Self {
        let direct = state
            .config
            .current()
            .policies
            .iter()
            .flat_map(|policy| &policy.assignments)
            .map(|assignment| assignment.client.clone())
            .collect();
        Self {
            active: state.policies.current(),
            direct,
        }
    }

    fn policy_of(&self, ip: IpAddr) -> String {
        policy_in_force(&self.active, ip)
    }

    fn is_direct(&self, ip: IpAddr) -> bool {
        self.direct.contains(&assignment_key(ip))
    }
}

fn client_policy_response(state: &AppState, ip: IpAddr) -> ClientPolicyResponse {
    let config = state.config.current();
    ClientPolicyResponse {
        ip,
        policy: policy_in_force(&state.policies.current(), ip),
        assignment: direct_assignment(&config, &assignment_key(ip)).map(assignment_response),
    }
}

/// Whether a policy edit moved the per-rule masks and so needs the ruleset
/// rebuilt. Seconds of ARM CPU, so it is stated at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recompile {
    Yes,
    No,
}

/// Persists a policy set and applies it — the single write path behind every
/// handler above.
///
/// Compiles before persisting, so a set the engine would reject never reaches
/// the file. Publishing the snapshot is last and unconditional: an assignment
/// is live when this returns.
async fn apply_policies(
    state: &AppState,
    policies: Vec<PolicyConfig>,
    recompile: Recompile,
) -> ApiResult<()> {
    let timezone = state.config.current().schedule.timezone.clone();
    let compiled = fah_rules::PolicySet::from_config(&timezone, &policies)
        .map_err(|err| ApiError::ValidationFailed(err.to_string()))?;

    let patch = serde_json::json!({ "policies": policies });
    state
        .config
        .apply_patch(&patch)
        .map_err(|err| ApiError::ValidationFailed(err.to_string()))?;

    state.rules.set_policies(compiled);
    if recompile == Recompile::Yes {
        state.rules.recompile().await;
    }
    republish_policies(state);
    Ok(())
}

/// Rebuilds the live client → policy snapshot. Also called after a timezone
/// change and after a client is renamed, both of which move assignments
/// without touching the policy set.
pub(crate) fn republish_policies(state: &AppState) {
    state
        .policies
        .refresh(&state.rules.policies(), &state.stats.named_clients());
}

/// Policy ids appear in TOML keys, API paths and metric labels, so the
/// alphabet is locked down at the boundary like a list id's.
fn validate_policy_id(id: &str) -> ApiResult<()> {
    if id == DEFAULT_POLICY {
        return Err(ApiError::ValidationFailed(format!(
            "{DEFAULT_POLICY:?} names the implicit policy every unassigned client \
             already gets and cannot be redefined"
        )));
    }
    let alphabet_ok = !id.is_empty()
        && id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        });
    if alphabet_ok && !id.starts_with('.') {
        return Ok(());
    }
    Err(ApiError::ValidationFailed(format!(
        "invalid policy id {id:?}: use lowercase letters, digits, '.', '_' or '-', \
         and do not start with '.'"
    )))
}

fn parse_ip(raw: &str) -> ApiResult<IpAddr> {
    raw.parse()
        .map_err(|_| ApiError::BadRequest(format!("{raw:?} is not an IP address")))
}

// ─── Rules ─────────────────────────────────────────────────────────────

async fn get_user_rules(State(state): State<Arc<AppState>>) -> Json<UserRulesBody> {
    let text = state.rules.user_rules().await.unwrap_or_default();
    Json(UserRulesBody {
        rules: text.lines().map(str::to_string).collect(),
    })
}

async fn put_user_rules(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UserRulesBody>,
) -> ApiResult<Json<UserRulesBody>> {
    let sent = body
        .rules
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(message) = validate_user_rules(&sent) {
        return Err(ApiError::ValidationFailed(message));
    }

    // Drop exact-duplicate rule lines (keep the first, preserve order). Storing
    // the same rule twice only clutters the list — the matcher already dedups,
    // so the copy blocks nothing new (a self-duplicate, unlike a user rule that
    // overlaps a *list*, carries no resilience benefit to justify keeping it).
    // Blanks and comments have no matcher identity, so they pass through as-is.
    let mut seen = std::collections::HashSet::new();
    let mut rules = Vec::with_capacity(body.rules.len());
    for rule in body.rules {
        let key = rule.trim();
        let is_rule = !key.is_empty() && !key.starts_with('#') && !key.starts_with('!');
        if is_rule && !seen.insert(key.to_string()) {
            continue;
        }
        rules.push(rule);
    }

    let text = rules
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");

    // Trailing newline so appending later never joins two rules onto a line.
    state.rules.set_user_rules(format!("{text}\n")).await;

    Ok(Json(UserRulesBody { rules }))
}

/// Per-line validation for `PUT /api/v1/rules/user` (API.md: "invalid lines →
/// 422 validation_failed with per-line messages").
///
/// A rule *list* never rejects — RULE_ENGINE.md has unparseable lines skipped
/// and counted so a bad upstream list cannot drop protection. Rules typed by
/// hand are the opposite case: silently dropping one would leave the user
/// believing they are protected, so any parse error fails the whole request.
///
/// The block is parsed as one unit, not line by line: format detection needs
/// the whole text. `/ads/banner.gif` alone looks like a malformed domain, but
/// among adblock rules it is a valid (DNS-inactive) URL pattern.
fn validate_user_rules(text: &str) -> Option<String> {
    let parsed = fah_rules::parse_rule_list(text);
    if parsed.parse_errors == 0 {
        return None;
    }

    let lines: Vec<&str> = text.lines().collect();
    let mut problems: Vec<String> = parsed
        .parse_error_lines
        .iter()
        .map(|number| {
            let content = lines.get(*number as usize - 1).unwrap_or(&"").trim();
            format!("line {number}: invalid rule syntax: {content:?}")
        })
        .collect();

    // `parse_error_lines` is capped; say so rather than under-report.
    let unreported = parsed.parse_errors as usize - problems.len();
    if unreported > 0 {
        problems.push(format!("and {unreported} more invalid line(s)"));
    }
    Some(problems.join("; "))
}

async fn test_rule(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RuleTestRequest>,
) -> ApiResult<Json<RuleTestResponse>> {
    let domain = body.domain.trim().to_ascii_lowercase();
    if domain.is_empty() {
        return Err(ApiError::ValidationFailed("domain is required".to_string()));
    }
    let qtype = body
        .qtype
        .as_deref()
        .map_or(fah_model::QueryType::A, parse_qtype);

    // Whose view to answer from. An explicit `policy` wins over the client's
    // assignment, so "what would kids see?" is answerable without a device.
    let matcher = state.rules.matcher();
    let active = state.policies.current();
    let mut ctx = match body.client.as_deref().map(str::trim) {
        Some(client) if !client.is_empty() => match client.parse::<IpAddr>() {
            Ok(ip) => matcher.context_for(ip, &active),
            // Not an address, so it is a client name — which selects nothing by
            // itself, but does satisfy a `$client=<name>` rule.
            Err(_) => fah_rules::ClientContext {
                name: Some(client),
                ..fah_rules::ClientContext::default()
            },
        },
        _ => fah_rules::ClientContext::default(),
    };
    if let Some(wanted) = body.policy.as_deref() {
        ctx.policy = match wanted {
            DEFAULT_POLICY => fah_model::PolicyId::DEFAULT,
            id => state
                .rules
                .policies()
                .id_of(id)
                .ok_or_else(|| ApiError::ValidationFailed(format!("no such policy: {id}")))?,
        };
    }

    let decided_by = active
        .id_of(ctx.policy)
        .map_or_else(|| DEFAULT_POLICY.to_string(), |id| id.to_string());
    let (verdict, rule, list) = match matcher.lookup_in(&domain, &qtype, &ctx) {
        fah_rules::MatchDecision::Block(rule) => {
            let decisive = matcher.decisive_rule(rule);
            (
                "block",
                Some(decisive.rule.to_string()),
                Some(decisive.list.to_string()),
            )
        }
        fah_rules::MatchDecision::Allow(rule) => {
            let decisive = matcher.decisive_rule(rule);
            (
                "allow",
                Some(decisive.rule.to_string()),
                Some(decisive.list.to_string()),
            )
        }
        fah_rules::MatchDecision::Pass => ("pass", None, None),
    };
    Ok(Json(RuleTestResponse {
        verdict,
        rule,
        list,
        policy: decided_by,
    }))
}

// ─── Configuration ─────────────────────────────────────────────────────

/// The effective merged config. Nothing here is secret — the API key and TLS
/// private key live in their own `/config` files and never enter this tree
/// (CONFIGURATION.md: "api key: stored in /config, never in this file's
/// plaintext sections"), so "secrets redacted" holds by construction.
async fn get_config(State(state): State<Arc<AppState>>) -> Json<fah_config::Config> {
    Json(state.config.current().as_ref().clone())
}

async fn post_config(
    State(state): State<Arc<AppState>>,
    Json(patch): Json<serde_json::Value>,
) -> ApiResult<Json<ConfigUpdateResponse>> {
    // `[[rules.lists]]` is owned by the `/lists` endpoints. They apply a change
    // live — fetch, recompile, atomic swap — *and* write it back to the TOML
    // through `persist_lists`. Accepting the array here as well would give one
    // piece of state two writers with no reconciliation between them: this
    // handler never reloads the `ListManager`, so the engine would keep serving
    // the old set, and the next `/lists` mutation reads its set from the engine
    // and persists *that* over the file — silently reverting the patch. One
    // owner instead: the TOML is the boot source and the durable record,
    // `/lists` is the runtime API (CONFIGURATION.md §Rule lists).
    //
    // `persist_lists` is unaffected: it calls `ConfigStore::apply_patch`
    // directly, not this handler.
    if patch
        .get("rules")
        .and_then(|rules| rules.get("lists"))
        .is_some()
    {
        return Err(ApiError::ValidationFailed(
            "rules.lists is not settable here: rule lists are managed by the \
             /api/v1/lists endpoints (POST, PATCH, DELETE), which apply live \
             with no restart and write the TOML back for you"
                .to_string(),
        ));
    }
    // `[[policies]]` has the same two-writer problem, and the same one owner:
    // `/policies` recompiles the masks when it has to, which this handler
    // cannot do.
    if patch.get("policies").is_some() {
        return Err(ApiError::ValidationFailed(
            "policies is not settable here: policies are managed by the \
             /api/v1/policies endpoints (POST, PATCH, DELETE) and \
             /api/v1/clients/{ip}/policy, which apply live and write the TOML \
             back for you"
                .to_string(),
        ));
    }

    if patch.get("auth").is_some() {
        return Err(ApiError::ValidationFailed(
            "auth is not settable here: the dashboard password is changed through \
             POST /api/v1/auth/password, which requires the current password and \
             invalidates every existing session. The Argon2id hash lives in \
             /config/auth-hash and never travels through this endpoint"
                .to_string(),
        ));
    }

    let outcome = state
        .config
        .apply_patch(&patch)
        .map_err(|err| ApiError::ValidationFailed(err.to_string()))?;

    // Push the runtime-class `[history]` fields into the writers so a retention
    // or enable change is live on the next prune/flush — no restart (hard rule
    // 3). Idempotent and cheap (two atomic stores), so it runs after every
    // successful apply rather than diffing the patch for these keys.
    let history = &state.config.current().history;
    state
        .stats
        .apply_history_config(history.enabled, history.retention_days);

    // `[schedule] timezone` decides when a window is open, so a change has to
    // recompile the policy set and republish. No ruleset rebuild: the masks
    // do not depend on the clock. Idempotent, so it runs after every apply.
    let config = state.config.current();
    match fah_rules::PolicySet::from_config(&config.schedule.timezone, &config.policies) {
        Ok(policies) => {
            state.rules.set_policies(policies);
            republish_policies(&state);
        }
        Err(err) => {
            // `Config::validate` already passed, so this is a bug rather than
            // operator error — and the running policies are left untouched.
            tracing::error!(error = %err, "policies failed to recompile after a config patch");
        }
    }

    state.events.publish(Event::ConfigChanged {
        restart_required: outcome.restart_required,
    });

    Ok(Json(ConfigUpdateResponse {
        applied: outcome.applied,
        restart_required: outcome.restart_required,
    }))
}

async fn rotate_api_key(State(state): State<Arc<AppState>>) -> ApiResult<Json<ApiKeyResponse>> {
    let api_key = state
        .keys
        .rotate()
        .map_err(|err| ApiError::Internal(format!("persisting the new API key: {err}")))?;
    Ok(Json(ApiKeyResponse { api_key }))
}

fn no_content_with_cookie(cookie: String) -> Response {
    match HeaderValue::from_str(&cookie) {
        Ok(value) => {
            let mut response = StatusCode::NO_CONTENT.into_response();
            response.headers_mut().insert(SET_COOKIE, value);
            response
        }
        Err(_) => ApiError::Internal("building the session cookie".to_string()).into_response(),
    }
}

fn spend_argon2(state: &AppState, client: IpAddr) -> ApiResult<Argon2Permit> {
    match state.auth.check_rate(client) {
        RateDecision::Limited { retry_after } => Err(ApiError::RateLimited {
            message: "too many password attempts; try again shortly".to_string(),
            retry_after,
        }),
        RateDecision::Allowed => state.auth.try_argon2_permit().ok_or(ApiError::Unavailable {
            message: "password verification is busy; retry in a moment".to_string(),
            retry_after: Some(1),
        }),
    }
}

async fn auth_login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Result<Json<LoginRequest>, JsonRejection>,
) -> ApiResult<Response> {
    if !state.tls {
        return Err(ApiError::Unavailable {
            message: "session authentication requires TLS: set [api] tls = true and \
                      restart. Bearer-key authentication is unaffected"
                .to_string(),
            retry_after: None,
        });
    }

    let Json(request) = body.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let permit = spend_argon2(&state, peer.ip())?;

    let minted = state
        .auth
        .verify_and_mint(permit, request.password)
        .await
        .map_err(ApiError::Internal)?;

    match minted {
        Some(token) => Ok(no_content_with_cookie(session::set_cookie(&token))),
        None => Err(ApiError::Unauthorized),
    }
}

async fn auth_logout() -> Response {
    no_content_with_cookie(session::clear_cookie())
}

async fn auth_logout_all(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    state
        .auth
        .rotate_secret()
        .await
        .map_err(|err| ApiError::Internal(format!("rotating the session secret: {err}")))?;
    Ok(no_content_with_cookie(session::clear_cookie()))
}

async fn auth_password(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Result<Json<PasswordChangeRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let Json(request) = body.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    if request.new_password.chars().count() < password::MIN_PASSWORD_CHARS {
        return Err(ApiError::ValidationFailed(format!(
            "new_password must be at least {} characters",
            password::MIN_PASSWORD_CHARS
        )));
    }

    let permit = spend_argon2(&state, peer.ip())?;
    let (correct, permit) = state
        .auth
        .verify_only(permit, request.current_password)
        .await
        .map_err(ApiError::Internal)?;
    if !correct {
        return Err(ApiError::Unauthorized);
    }

    let new_hash = password::hash_password_off_runtime(permit, request.new_password)
        .await
        .map_err(ApiError::Internal)?;

    state
        .auth
        .replace_password(new_hash)
        .await
        .map_err(|err| ApiError::Internal(format!("persisting the new password: {err}")))?;

    state.events.publish(Event::ConfigChanged {
        restart_required: false,
    });

    Ok(no_content_with_cookie(session::clear_cookie()))
}

// ─── Events ────────────────────────────────────────────────────────────

async fn events_socket(
    State(state): State<Arc<AppState>>,
    method: Option<Extension<AuthMethod>>,
    headers: HeaderMap,
    uri: Uri,
    upgrade: WebSocketUpgrade,
) -> Response {
    let method = method.map_or(AuthMethod::Session, |Extension(method)| method);
    if method == AuthMethod::Session && !same_origin(&headers, &uri, state.tls) {
        return ApiError::Unauthorized.into_response();
    }

    let (receiver, subscription) = state.events.subscribe_socket();
    let stats = Arc::clone(&state.stats);
    upgrade
        .max_message_size(events::MAX_CLIENT_MESSAGE_BYTES)
        .max_frame_size(events::MAX_CLIENT_MESSAGE_BYTES)
        .on_upgrade(move |socket| events::run_socket(socket, receiver, stats, subscription))
}

fn same_origin(headers: &HeaderMap, uri: &Uri, tls: bool) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case(if tls { "https" } else { "http" }) {
        return false;
    }
    let authority = authority.strip_suffix('/').unwrap_or(authority);
    if authority.contains('/') || authority.contains('@') {
        return false;
    }

    let target = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .or_else(|| uri.authority().map(|authority| authority.as_str()));
    let Some(target) = target else {
        return false;
    };

    let (origin_host, origin_port) = split_authority(authority, tls);
    let (target_host, target_port) = split_authority(target, tls);
    origin_host.eq_ignore_ascii_case(target_host) && origin_port == target_port
}

fn split_authority(authority: &str, tls: bool) -> (&str, u16) {
    let default = if tls { 443 } else { 80 };
    if let Some(rest) = authority.strip_prefix('[') {
        if let Some((host, tail)) = rest.split_once(']') {
            let port = tail
                .strip_prefix(':')
                .and_then(|port| port.parse().ok())
                .unwrap_or(default);
            return (host, port);
        }
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().unwrap_or(default)),
        None => (authority, default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(origin: Option<&str>, host: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(origin) = origin {
            headers.insert(ORIGIN, HeaderValue::from_str(origin).unwrap());
        }
        if let Some(host) = host {
            headers.insert(HOST, HeaderValue::from_str(host).unwrap());
        }
        headers
    }

    #[test]
    fn an_origin_matching_the_requests_own_target_is_accepted() {
        let uri = Uri::from_static("/api/v1/events");
        for (origin, host) in [
            ("https://fah.lan:8443", "fah.lan:8443"),
            ("https://FAH.lan:8443", "fah.lan:8443"),
            ("https://fah.lan:8443/", "fah.lan:8443"),
            ("https://fah.lan", "fah.lan:443"),
            ("https://fah.lan:443", "fah.lan"),
            ("https://[fd00::1]:8443", "[fd00::1]:8443"),
        ] {
            assert!(
                same_origin(&headers(Some(origin), Some(host)), &uri, true),
                "{origin} against {host}"
            );
        }
    }

    #[test]
    fn a_foreign_missing_or_downgraded_origin_is_rejected() {
        let uri = Uri::from_static("/api/v1/events");
        for (origin, host) in [
            (Some("https://evil.example.com"), Some("fah.lan:8443")),
            (Some("https://fah.lan:9443"), Some("fah.lan:8443")),
            (Some("http://fah.lan:8443"), Some("fah.lan:8443")),
            (Some("https://user@fah.lan:8443"), Some("fah.lan:8443")),
            (Some("null"), Some("fah.lan:8443")),
            (None, Some("fah.lan:8443")),
            (Some("https://fah.lan:8443"), None),
        ] {
            assert!(
                !same_origin(&headers(origin, host), &uri, true),
                "{origin:?} against {host:?}"
            );
        }
    }

    #[test]
    fn the_h2_authority_stands_in_when_no_host_header_is_present() {
        let uri = Uri::from_static("https://fah.lan:8443/api/v1/events");
        assert!(same_origin(
            &headers(Some("https://fah.lan:8443"), None),
            &uri,
            true
        ));
        assert!(!same_origin(
            &headers(Some("https://other.lan:8443"), None),
            &uri,
            true
        ));
    }

    #[test]
    fn the_scheme_follows_api_tls_rather_than_the_presented_origin() {
        let uri = Uri::from_static("/api/v1/events");
        assert!(same_origin(
            &headers(Some("http://fah.lan:8080"), Some("fah.lan:8080")),
            &uri,
            false
        ));
        assert!(!same_origin(
            &headers(Some("https://fah.lan:8080"), Some("fah.lan:8080")),
            &uri,
            false
        ));
    }

    #[test]
    fn a_bad_timestamp_is_rejected_rather_than_ignored() {
        let params = HashMap::from([("from".to_string(), "yesterday".to_string())]);
        assert!(matches!(
            timestamp_param(&params, "from"),
            Err(ApiError::BadRequest(_))
        ));
        assert_eq!(timestamp_param(&HashMap::new(), "from").unwrap(), None);
    }

    #[test]
    fn a_timestamp_parses_into_the_range() {
        let params = HashMap::from([("from".to_string(), "1970-01-01T00:00:00Z".to_string())]);
        assert_eq!(
            timestamp_param(&params, "from").unwrap(),
            Some(SystemTime::UNIX_EPOCH)
        );
    }

    #[test]
    fn user_rule_validation_reports_the_offending_line_numbers() {
        // `||^` has an empty domain between the anchor and the separator —
        // the parser counts that as a parse error.
        let message = validate_user_rules("||ads.example.com^\n||^\n@@||good.example.com^")
            .expect("line 2 is invalid");
        assert!(message.contains("line 2"), "got: {message}");
        assert!(message.contains("||^"), "the offending text is quoted back");
        assert!(!message.contains("line 1"));
        assert!(!message.contains("line 3"));
    }

    #[test]
    fn blank_and_comment_lines_are_not_validation_failures() {
        let text = "\n   \n# a comment\n! easylist-style comment\n||ads.example.com^";
        assert_eq!(validate_user_rules(text), None);
    }

    #[test]
    fn valid_but_dns_inactive_rules_are_accepted() {
        // Cosmetic and URL-pattern rules parse fine, they are just inactive
        // for DNS (ADR-0003) — rejecting them would break pasting a list.
        // Validating line-by-line would misread `/ads/banner.gif` as a
        // malformed domain; in context it is a valid adblock URL pattern.
        let text = "||ads.example.com^\nexample.com##.ad-banner\n/ads/banner.gif";
        assert_eq!(validate_user_rules(text), None);
    }

    #[test]
    fn every_reported_line_number_indexes_the_line_it_names() {
        let text = "||ads.example.com^\n||^\n||good.example.com^\n||^";
        let message = validate_user_rules(text).unwrap();
        assert!(message.contains("line 2"));
        assert!(message.contains("line 4"));
    }

    #[test]
    fn list_ids_derive_readably_from_urls_and_paths() {
        assert_eq!(derive_id("https://small.oisd.nl"), "small.oisd.nl");
        assert_eq!(
            derive_id("https://example.org/oisd-basic.txt"),
            "oisd-basic"
        );
        assert_eq!(derive_id("/data/lists/local.txt"), "local");
        assert_eq!(derive_id("https://example.org/lists/"), "lists");
        // Lowercased so every derived id passes `validate_list_id`.
        assert_eq!(derive_id("https://example.org/Xtra/Hosts.txt"), "hosts");
    }

    #[test]
    fn list_ids_are_locked_to_a_filesystem_safe_alphabet() {
        for ok in ["oisd-basic", "small.oisd.nl", "list_2", "a"] {
            assert!(validate_list_id(ok).is_ok(), "{ok:?} must be accepted");
        }
        // Ids become `/data/lists/{id}.raw` — traversal, separators, case
        // and leading dots all die at the boundary.
        for bad in ["../apikey", "a/b", "a\\b", "Hosts", ".hidden", "a b"] {
            assert!(
                matches!(validate_list_id(bad), Err(ApiError::ValidationFailed(_))),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn list_response_joins_the_entry_with_its_refresh_outcome() {
        let entry = fah_rules::ListEntryView {
            id: "oisd-basic".to_string(),
            url: "https://small.oisd.nl".to_string(),
            enabled: true,
            refresh_hours: None,
        };
        let stats = fah_rules::RefreshStats {
            active: 198_500,
            url: 9_181,
            inactive: 15_501,
            parse_errors: 0,
        };
        let status = ListStatus {
            last_refreshed: Some(SystemTime::UNIX_EPOCH),
            last_result: RefreshResult::Ok(stats.clone()),
            compiled: Some(stats),
        };

        let response = list_response(entry, &status, 24);
        assert_eq!(
            response.refresh_hours, 24,
            "falls back to the global default"
        );
        assert_eq!(response.last_status, "ok");
        assert_eq!(response.rules_total, 223_182);
        assert_eq!(response.rules_active_dns, 198_500);
        assert_eq!(response.rules_active_url, 9_181);
        assert_eq!(response.rules_inactive, 15_501);
        assert_eq!(response.format, "auto");
        assert_eq!(response.parse_errors, 0);
        assert_eq!(response.last_error, None);
    }

    #[test]
    fn a_rejected_body_reports_its_reason_and_the_rules_that_keep_serving() {
        let entry = fah_rules::ListEntryView {
            id: "hosts".to_string(),
            url: "https://example.org/hosts".to_string(),
            enabled: true,
            refresh_hours: None,
        };
        let status = ListStatus {
            last_refreshed: None,
            last_result: RefreshResult::Rejected("misparse: 69514 errors, 83 rules".to_string()),
            compiled: Some(fah_rules::RefreshStats {
                active: 55_866,
                url: 0,
                inactive: 0,
                parse_errors: 7,
            }),
        };

        let response = list_response(entry, &status, 24);
        assert_eq!(response.last_status, "rejected");
        assert_eq!(
            response.last_error.as_deref(),
            Some("rejected: misparse: 69514 errors, 83 rules")
        );
        assert_eq!(
            response.parse_errors, 7,
            "parse_errors describes the copy that is serving, not the refused body"
        );
        assert_eq!(
            response.rules_total, 55_866,
            "a refused body leaves the last-good ruleset in place"
        );
    }

    #[test]
    fn a_failed_refresh_still_reports_the_rules_that_are_serving() {
        let entry = fah_rules::ListEntryView {
            id: "hosts".to_string(),
            url: "https://example.org/hosts".to_string(),
            enabled: true,
            refresh_hours: None,
        };
        // The shape after a boot-from-cache followed by a refresh that could
        // not reach the network: the previous ruleset is still live.
        let status = ListStatus {
            last_refreshed: None,
            last_result: RefreshResult::Failed("dns error: EAI_AGAIN".to_string()),
            compiled: Some(fah_rules::RefreshStats {
                active: 55_866,
                url: 0,
                inactive: 0,
                parse_errors: 0,
            }),
        };

        let response = list_response(entry, &status, 24);
        assert_eq!(response.last_status, "failed", "the fetch did fail");
        assert_eq!(
            response.rules_total, 55_866,
            "but those rules are still blocking — reporting 0 would say the \
             list is not protecting anything, which is false"
        );
        assert_eq!(
            response.last_error.as_deref(),
            Some("dns error: EAI_AGAIN"),
            "the cause has to reach the API, not only the log"
        );
    }

    #[test]
    fn a_never_refreshed_list_reports_never_with_no_timestamp() {
        let entry = fah_rules::ListEntryView {
            id: "new".to_string(),
            url: "https://example.org/l.txt".to_string(),
            enabled: true,
            refresh_hours: Some(6),
        };
        let response = list_response(entry, &ListStatus::default(), 24);
        assert_eq!(response.last_status, "never");
        assert_eq!(response.last_refresh, None);
        assert_eq!(response.refresh_hours, 6, "the per-list override wins");
        assert_eq!(response.rules_total, 0);
    }
}
