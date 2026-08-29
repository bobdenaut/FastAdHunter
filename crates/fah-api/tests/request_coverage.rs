//! Guard: `requests/` exercises every route the API actually serves.
//!
//! `requests/README.md` claims coverage of every endpoint in API.md, and that
//! claim had already gone stale — `/api/v1/policies*` and
//! `/api/v1/clients/{ip}/policy` shipped with no file at all. A prose claim
//! about a directory rots silently; enumerating the router against it does not.
//!
//! This checks that a route is *reachable* from some request, not that its
//! failure cases are covered. It is a floor under the claim, not a substitute
//! for reading the files.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// Routes deliberately outside the suite, each with the reason the README
/// gives. Adding an entry here is a decision; forgetting a file is not.
const UNCOVERED: [(&str, &str); 2] = [
    (
        "/api/v1/events",
        "a WebSocket — the REST Client extension cannot open one \
         (requests/README.md §Not covered)",
    ),
    (
        "/api/",
        "not an endpoint — the trailing-slash form axum's `nest(\"/api\")` \
         cannot match, kept inside the JSON world instead of falling to the \
         SPA shell (review finding R2)",
    ),
];

#[test]
fn every_route_the_api_serves_has_a_request_in_the_suite() {
    let routes = routes_from_source();
    // The scrape is textual, so a refactor that moves the router could quietly
    // find nothing and pass. A floor turns that into a failure.
    assert!(
        routes.len() >= 20,
        "only {} routes scraped — has router() moved?",
        routes.len()
    );

    let exercised = paths_in_request_files();
    assert!(
        exercised.len() >= 20,
        "only {} request paths found — has requests/ moved?",
        exercised.len()
    );

    let missing: Vec<&String> = routes
        .iter()
        .filter(|route| !UNCOVERED.iter().any(|(path, _)| path == *route))
        .filter(|route| !exercised.iter().any(|used| covers(route, used)))
        .collect();

    assert!(
        missing.is_empty(),
        "these routes have no request in requests/, so the README's coverage \
         claim is false: {missing:#?}"
    );
}

/// Every route the router registers, fully qualified. Scraped from the source
/// rather than from a live `Router`, because axum exposes no way to enumerate
/// one.
fn routes_from_source() -> BTreeSet<String> {
    let source = std::fs::read_to_string(repo_path(["crates", "fah-api", "src", "routes.rs"]))
        .expect("crates/fah-api/src/routes.rs");

    // The `/api/v1` group is one `let v1 = Router::new()…;` statement with no
    // semicolon inside it, so the first `;` is exactly its end. Everything
    // after is mounted at the root.
    let (nested, root) = source
        .split_once("let v1 = Router::new()")
        .and_then(|(_, rest)| rest.split_once(';'))
        .expect("router() no longer starts with `let v1 = Router::new()`");

    let mut routes = BTreeSet::new();
    for (block, prefix) in [(nested, "/api/v1"), (root, "")] {
        for literal in block.split(".route(").skip(1) {
            let path = literal
                .trim_start()
                .strip_prefix('"')
                .and_then(|rest| rest.split_once('"'))
                .map(|(path, _)| path);
            if let Some(path) = path {
                routes.insert(format!("{prefix}{path}"));
            }
        }
    }
    routes
}

/// Every path any `.http` file issues a request against, with `{{base}}`
/// expanded and the query string dropped.
fn paths_in_request_files() -> BTreeSet<String> {
    const METHODS: [&str; 5] = ["GET ", "POST ", "PUT ", "PATCH ", "DELETE "];

    let dir = repo_path(["requests"]);
    let mut paths = BTreeSet::new();

    for entry in std::fs::read_dir(&dir).expect("requests/") {
        let file = entry.expect("a directory entry").path();
        if file.extension().and_then(|e| e.to_str()) != Some("http") {
            continue;
        }
        let text = std::fs::read_to_string(&file).expect("a request file");

        // `@base = {{host}}/api/v1` — read it rather than assuming it, so a
        // file that mounts somewhere else is still resolved correctly.
        let base = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("@base"))
            .and_then(|rest| rest.split_once('='))
            .map(|(_, value)| value.trim().replace("{{host}}", ""))
            .unwrap_or_default();

        for line in text.lines() {
            let line = line.trim();
            let Some(method) = METHODS.iter().find(|m| line.starts_with(**m)) else {
                continue;
            };
            let target = line[method.len()..].trim();
            let path = target
                .replace("{{base}}", &base)
                .replace("{{host}}", "")
                .split(['?', '#'])
                .next()
                .unwrap_or_default()
                .to_string();
            if path.starts_with('/') {
                paths.insert(path);
            }
        }
    }
    paths
}

/// Whether `used` is a request against `route`, treating `{param}` segments as
/// wildcards. Segment-wise, so `/cache` is not satisfied by `/cache/clean`.
fn covers(route: &str, used: &str) -> bool {
    let route: Vec<&str> = route.split('/').collect();
    let used: Vec<&str> = used.split('/').collect();

    route.len() == used.len()
        && route
            .iter()
            .zip(&used)
            .all(|(segment, actual)| segment.starts_with('{') || segment == actual)
}

fn repo_path<const N: usize>(parts: [&str; N]) -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/fah-api.
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.extend(parts);
    path
}

/// The exclusion list is a decision record, not a mute button: an entry that
/// no longer names a real route is a stale excuse hiding a gap.
#[test]
fn every_documented_exclusion_still_names_a_real_route() {
    let routes = routes_from_source();

    for (path, reason) in UNCOVERED {
        assert!(
            routes.contains(path),
            "{path} is excluded from requests/ ({reason}) but is no longer a route"
        );
    }
}
