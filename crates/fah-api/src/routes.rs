//! Every endpoint API.md documents. Each handler is thin: parse, call a
//! handle, map to the wire shape — all the logic lives in `fah-rules` and
//! behind the [`crate::ports`] traits.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use fah_config::RuleListConfig;
use fah_model::{HistoryRange, HistoryResolution, TopKind};
use fah_rules::{ListPatch, ListStatus, RefreshResult};

use crate::error::{ApiError, ApiResult};
use crate::events::{self, Event};
use crate::ports::{HistorySource, QueryLogRequest, VerdictFilter};
use crate::state::AppState;
use crate::timestamp;
use crate::wire::*;

/// `GET /api/v1/queries` pagination bounds (API.md: "limit (default 100, max
/// 1000)").
const DEFAULT_QUERY_LIMIT: usize = 100;
const MAX_QUERY_LIMIT: usize = 1000;

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

pub fn router(state: Arc<AppState>) -> Router {
    let v1 = Router::new()
        .route("/stats", get(stats))
        .route("/queries", get(queries))
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
        .route("/rules/user", get(get_user_rules).put(put_user_rules))
        .route("/rules/test", post(test_rule))
        .route("/cache", get(cache_stats))
        .route("/cache/clean", post(cache_clean))
        .route("/config", get(get_config).post(post_config))
        .route("/config/apikey/rotate", post(rotate_api_key))
        .route("/debug/memory", get(debug_memory))
        .route("/events", get(events_socket));

    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .nest("/api/v1", v1)
        .fallback(not_found)
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            crate::auth::require_api_key,
        ))
        .with_state(state)
}

async fn not_found() -> ApiError {
    ApiError::NotFound("no such endpoint".to_string())
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

async fn metrics(State(state): State<Arc<AppState>>) -> Response {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.telemetry.prometheus_text(),
    )
        .into_response()
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

/// Where the RAM goes, for chasing the PERFORMANCE.md budget on-device:
/// the compiled ruleset (the largest resident thing), the DNS cache, and the
/// process RSS the container reports. The gap between RSS and the parts is
/// runtime + allocator-retained memory.
async fn debug_memory(State(state): State<Arc<AppState>>) -> Json<MemoryResponse> {
    let cache = state.cache.stats();
    Json(MemoryResponse {
        ruleset_bytes: state.rules.matcher().heap_bytes() as u64,
        cache_entries: cache.entries,
        cache_estimated_bytes: cache.estimated_bytes,
        process_rss: crate::rss::process_rss(),
    })
}

// ─── Statistics & query log ────────────────────────────────────────────

async fn stats(State(state): State<Arc<AppState>>) -> Json<StatsResponse> {
    Json(state.stats.overview(SystemTime::now()).into())
}

async fn queries(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Json<QueryPageResponse>> {
    let request = parse_query_params(&params)?;
    Ok(Json(state.stats.queries(&request).into()))
}

fn parse_query_params(params: &HashMap<String, String>) -> ApiResult<QueryLogRequest> {
    let limit = match params.get("limit") {
        Some(raw) => raw
            .parse::<usize>()
            .map_err(|_| ApiError::BadRequest(format!("limit must be a number, got {raw:?}")))?
            .clamp(1, MAX_QUERY_LIMIT),
        None => DEFAULT_QUERY_LIMIT,
    };

    let client = match params.get("client") {
        Some(raw) => Some(
            raw.parse::<IpAddr>()
                .map_err(|_| ApiError::BadRequest(format!("client must be an IP, got {raw:?}")))?,
        ),
        None => None,
    };

    let verdict = match params.get("verdict").map(String::as_str) {
        Some("allow") => Some(VerdictFilter::Allow),
        Some("block") => Some(VerdictFilter::Block),
        Some("pass") => Some(VerdictFilter::Pass),
        Some(other) => {
            return Err(ApiError::BadRequest(format!(
                "verdict must be one of allow|block|pass, got {other:?}"
            )))
        }
        None => None,
    };

    Ok(QueryLogRequest {
        limit,
        cursor: params.get("cursor").cloned(),
        client,
        domain: params.get("domain").cloned(),
        verdict,
        from: timestamp_param(params, "from")?,
        to: timestamp_param(params, "to")?,
    })
}

/// An RFC 3339 `from`/`to` query parameter — shared by the query log and the
/// history endpoints, which document the same spelling.
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
/// (same contract as `GET /api/v1/queries`' `limit`).
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

async fn clients(State(state): State<Arc<AppState>>) -> Json<ClientsResponse> {
    Json(ClientsResponse {
        items: state
            .stats
            .clients(SystemTime::now())
            .into_iter()
            .map(Into::into)
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

    state
        .stats
        .set_client_name(ip, name)
        .map(|entry| Json(entry.into()))
        .ok_or_else(|| ApiError::NotFound(format!("no client seen at {ip}")))
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
    let last_status = match &status.last_result {
        RefreshResult::Ok(_) => "ok",
        RefreshResult::Failed(_) => "failed",
        RefreshResult::NeverAttempted => "never",
    };
    // Counts describe the ruleset that is *serving*, not the last refresh
    // attempt. A failed refresh keeps the previous ruleset live
    // (RULE_ENGINE.md failure policy), so `last_status: "failed"` alongside a
    // non-zero `rules_total` is the correct — and operationally important —
    // report: the fetch broke, protection did not.
    let (active, inactive) = status
        .compiled
        .as_ref()
        .map_or((0, 0), |stats| (stats.active, stats.inactive));
    ListResponse {
        id: entry.id,
        url: entry.url,
        format: "auto",
        enabled: entry.enabled,
        refresh_hours: entry.refresh_hours.unwrap_or(default_hours),
        last_refresh: status.last_refreshed,
        last_status,
        rules_total: active + inactive,
        rules_active_dns: active,
        rules_inactive: inactive,
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
            Ok(_) => "ok",
            Err(err) => {
                // A manual refresh is what an operator reaches for when a list
                // is failing, so this line has to name the actual cause.
                let error = fah_common::error_chain(&err);
                tracing::warn!(list = %id, %error, "manual list refresh failed");
                "failed"
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
            match outcome.result {
                Ok(stats) => {
                    refreshed += 1;
                    state.events.publish(Event::ListRefreshed {
                        id: id.clone(),
                        status: "ok",
                    });
                    ListRefreshResult {
                        id,
                        status: "ok",
                        rules_active_dns: Some(stats.active),
                        error: None,
                    }
                }
                Err(error) => {
                    failed += 1;
                    tracing::warn!(list = %id, %error, "list refresh failed in refresh-all");
                    state.events.publish(Event::ListRefreshed {
                        id: id.clone(),
                        status: "failed",
                    });
                    ListRefreshResult {
                        id,
                        status: "failed",
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

    if let Some(message) = validate_user_rules(&text) {
        return Err(ApiError::ValidationFailed(message));
    }

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

    let verdict = state.rules.matcher().verdict(&domain, &qtype);
    let (verdict, rule, list) = match verdict {
        fah_model::Verdict::Block(decisive) => (
            "block",
            Some(decisive.rule.to_string()),
            Some(decisive.list.to_string()),
        ),
        fah_model::Verdict::Allow(decisive) => (
            "allow",
            Some(decisive.rule.to_string()),
            Some(decisive.list.to_string()),
        ),
        fah_model::Verdict::Pass => ("pass", None, None),
    };
    Ok(Json(RuleTestResponse {
        verdict,
        rule,
        list,
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

// ─── Events ────────────────────────────────────────────────────────────

async fn events_socket(State(state): State<Arc<AppState>>, upgrade: WebSocketUpgrade) -> Response {
    let receiver = state.events.subscribe();
    let stats = Arc::clone(&state.stats);
    upgrade.on_upgrade(move |socket| events::run_socket(socket, receiver, stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_defaults_and_is_capped_at_the_documented_maximum() {
        let empty = HashMap::new();
        assert_eq!(
            parse_query_params(&empty).unwrap().limit,
            DEFAULT_QUERY_LIMIT
        );

        let params = HashMap::from([("limit".to_string(), "5000".to_string())]);
        assert_eq!(parse_query_params(&params).unwrap().limit, MAX_QUERY_LIMIT);

        let params = HashMap::from([("limit".to_string(), "0".to_string())]);
        assert_eq!(parse_query_params(&params).unwrap().limit, 1);
    }

    #[test]
    fn bad_filter_values_are_rejected_rather_than_ignored() {
        for (key, value) in [
            ("limit", "many"),
            ("client", "not-an-ip"),
            ("verdict", "maybe"),
            ("from", "yesterday"),
        ] {
            let params = HashMap::from([(key.to_string(), value.to_string())]);
            assert!(
                matches!(parse_query_params(&params), Err(ApiError::BadRequest(_))),
                "{key}={value} must be rejected"
            );
        }
    }

    #[test]
    fn filters_parse_into_the_request() {
        let params = HashMap::from([
            ("client".to_string(), "192.168.1.10".to_string()),
            ("domain".to_string(), "ads".to_string()),
            ("verdict".to_string(), "block".to_string()),
            ("from".to_string(), "1970-01-01T00:00:00Z".to_string()),
            ("cursor".to_string(), "42".to_string()),
        ]);
        let request = parse_query_params(&params).unwrap();

        assert_eq!(request.client, Some("192.168.1.10".parse().unwrap()));
        assert_eq!(request.domain.as_deref(), Some("ads"));
        assert_eq!(request.verdict, Some(VerdictFilter::Block));
        assert_eq!(request.from, Some(SystemTime::UNIX_EPOCH));
        assert_eq!(request.cursor.as_deref(), Some("42"));
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
        assert_eq!(response.rules_total, 214_001);
        assert_eq!(response.rules_active_dns, 198_500);
        assert_eq!(response.rules_inactive, 15_501);
        assert_eq!(response.format, "auto");
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
