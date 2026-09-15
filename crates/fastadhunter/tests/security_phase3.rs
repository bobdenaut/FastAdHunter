mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::HeaderMap;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use common::{
    a_query, await_event, bind_origin, boot_full, boot_full_in, client_config_trusting,
    client_hello, connect_events, contains, decode_base64, insecure_client_config, login_cookie,
    password_from_log, pseudo_random_payload, raw_exchange, resolve, run_raw_origin,
    run_tls_origin, self_signed_origin, skip_origin_message, tls_connect_from, FullMode, Instance,
    Needles, AD_HOST, ORIGIN_PORT, PAGE_HOST,
};

const LOCAL: Ipv4Addr = Ipv4Addr::LOCALHOST;
const OTHER_CLIENT: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);

enum Body {
    Empty,
    Json(Value),
    Dns,
}

enum Auth<'a> {
    None,
    Bearer(&'a str),
    Cookie(&'a str),
}

impl Auth<'_> {
    fn label(&self) -> &'static str {
        match self {
            Auth::None => "anonymous",
            Auth::Bearer(_) => "bearer",
            Auth::Cookie(_) => "cookie",
        }
    }
}

fn full(origin_ip: Ipv4Addr, clients: &[&str]) -> FullMode {
    FullMode {
        origin_ip,
        clients: clients.iter().map(|client| client.to_string()).collect(),
        api_tls: true,
        upstream_root: None,
        https_limits: None,
    }
}

fn documented_routes(ca_pem: &str, ca_key_pem: &str) -> Vec<(Method, String, Body)> {
    let mut routes = vec![
        (Method::GET, "/health".to_string(), Body::Empty),
        (Method::GET, "/dns-query".to_string(), Body::Dns),
        (Method::POST, "/dns-query".to_string(), Body::Dns),
        (Method::GET, "/api/v1/telemetry".to_string(), Body::Empty),
        (Method::GET, "/api/v1/stats".to_string(), Body::Empty),
        (
            Method::GET,
            "/api/v1/history/summary".to_string(),
            Body::Empty,
        ),
        (
            Method::GET,
            "/api/v1/history/perf?from=0&to=4102444800".to_string(),
            Body::Empty,
        ),
        (Method::GET, "/api/v1/history/top".to_string(), Body::Empty),
        (Method::GET, "/api/v1/clients".to_string(), Body::Empty),
        (
            Method::PUT,
            "/api/v1/clients/127.0.0.1".to_string(),
            Body::Json(json!({ "name": "probe" })),
        ),
        (Method::GET, "/api/v1/cache".to_string(), Body::Empty),
        (Method::POST, "/api/v1/cache/clean".to_string(), Body::Empty),
        (Method::GET, "/api/v1/lists".to_string(), Body::Empty),
        (
            Method::POST,
            "/api/v1/lists".to_string(),
            Body::Json(json!({})),
        ),
        (
            Method::PATCH,
            "/api/v1/lists/nope".to_string(),
            Body::Json(json!({ "enabled": false })),
        ),
        (
            Method::DELETE,
            "/api/v1/lists/nope".to_string(),
            Body::Empty,
        ),
        (
            Method::POST,
            "/api/v1/lists/nope/refresh".to_string(),
            Body::Empty,
        ),
        (
            Method::POST,
            "/api/v1/lists/refresh".to_string(),
            Body::Empty,
        ),
        (Method::GET, "/api/v1/rules/user".to_string(), Body::Empty),
        (
            Method::PUT,
            "/api/v1/rules/user".to_string(),
            Body::Json(json!({ "rules": ["||probe.example^"] })),
        ),
        (
            Method::POST,
            "/api/v1/rules/test".to_string(),
            Body::Json(json!({ "domain": "probe.example" })),
        ),
        (Method::GET, "/api/v1/policies".to_string(), Body::Empty),
        (
            Method::POST,
            "/api/v1/policies".to_string(),
            Body::Json(json!({})),
        ),
        (
            Method::PATCH,
            "/api/v1/policies/nope".to_string(),
            Body::Json(json!({})),
        ),
        (
            Method::DELETE,
            "/api/v1/policies/nope".to_string(),
            Body::Empty,
        ),
        (
            Method::GET,
            "/api/v1/clients/127.0.0.1/policy".to_string(),
            Body::Empty,
        ),
        (
            Method::PUT,
            "/api/v1/clients/127.0.0.1/policy".to_string(),
            Body::Json(json!({ "policy": "nope" })),
        ),
        (
            Method::DELETE,
            "/api/v1/clients/127.0.0.1/policy".to_string(),
            Body::Empty,
        ),
        (Method::GET, "/api/v1/config".to_string(), Body::Empty),
        (
            Method::POST,
            "/api/v1/config".to_string(),
            Body::Json(json!({})),
        ),
        (Method::GET, "/api/v1/events".to_string(), Body::Empty),
        (Method::GET, "/api/v1/debug/memory".to_string(), Body::Empty),
        (Method::GET, "/api/v1/certificates".to_string(), Body::Empty),
        (
            Method::POST,
            "/api/v1/certificates/ca/generate".to_string(),
            Body::Json(json!({ "confirm": false })),
        ),
        (
            Method::GET,
            "/api/v1/certificates/ca/export?format=pem".to_string(),
            Body::Empty,
        ),
        (
            Method::GET,
            "/api/v1/certificates/ca/export?format=der".to_string(),
            Body::Empty,
        ),
        (
            Method::GET,
            "/api/v1/certificates/ca/export?format=key".to_string(),
            Body::Empty,
        ),
        (
            Method::GET,
            "/api/v1/certificates/ca/export?format=pem&include_key=true".to_string(),
            Body::Empty,
        ),
        (
            Method::GET,
            "/api/v1/certificates/ca/key".to_string(),
            Body::Empty,
        ),
        (
            Method::GET,
            "/api/v1/certificates/key".to_string(),
            Body::Empty,
        ),
        (
            Method::GET,
            "/api/v1/certificates/ca-key.pem".to_string(),
            Body::Empty,
        ),
        (
            Method::GET,
            "/api/v1/certificates/export".to_string(),
            Body::Empty,
        ),
        (
            Method::POST,
            "/api/v1/certificates/import".to_string(),
            Body::Json(json!({
                "cert_pem": format!("{ca_pem}{ca_key_pem}"),
                "key_pem": "not a key"
            })),
        ),
        (
            Method::POST,
            "/api/v1/auth/login".to_string(),
            Body::Json(json!({ "password": "wrong" })),
        ),
        (
            Method::POST,
            "/api/v1/auth/password".to_string(),
            Body::Json(json!({
                "current_password": "wrong",
                "new_password": "irrelevant-value-1234"
            })),
        ),
        (Method::GET, "/api/v1/dns-query".to_string(), Body::Dns),
        (Method::GET, "/api/dns-query".to_string(), Body::Dns),
        (Method::GET, "/api/v1/nope".to_string(), Body::Empty),
        (Method::GET, "/api/".to_string(), Body::Empty),
    ];
    for path in static_and_traversal_paths() {
        routes.push((Method::GET, path.to_string(), Body::Empty));
    }
    routes.push((Method::POST, "/api/v1/auth/logout".to_string(), Body::Empty));
    routes
}

fn static_and_traversal_paths() -> [&'static str; 20] {
    [
        "/",
        "/index.html",
        "/assets/",
        "/assets/nope.js",
        "/config/ca-key.pem",
        "/ca-key.pem",
        "/../config/ca-key.pem",
        "/..%2fconfig%2fca-key.pem",
        "/%2e%2e/config/ca-key.pem",
        "/assets/../../config/ca-key.pem",
        "/assets/..%2f..%2fconfig%2fca-key.pem",
        "/assets/..%5c..%5cconfig%5cca-key.pem",
        "/assets/..\\..\\config\\ca-key.pem",
        "/web/../config/ca-key.pem",
        "/api/v1/../../config/ca-key.pem",
        "/api/v1/certificates/ca/../../../../config/ca-key.pem",
        "/config/api-key.pem",
        "/config/fastadhunter.toml",
        "/config/apikey",
        "/data/session-secret",
    ]
}

async fn send(
    instance: &Instance,
    auth: &Auth<'_>,
    method: &Method,
    path: &str,
    body: &Body,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    if *method == Method::GET && static_and_traversal_paths().contains(&path) {
        return send_raw(instance, auth, path).await;
    }
    let url = format!("{}{path}", instance.base);
    let mut request = instance.http.request(method.clone(), &url);
    request = match auth {
        Auth::None => request,
        Auth::Bearer(key) => request.bearer_auth(key),
        Auth::Cookie(cookie) => request.header("cookie", *cookie),
    };
    request = match body {
        Body::Empty => request,
        Body::Json(value) => request.json(value),
        Body::Dns if *method == Method::GET => {
            let query = a_query("probe.example");
            let encoded = base64url_unpadded(&query);
            instance
                .http
                .request(method.clone(), format!("{url}?dns={encoded}"))
        }
        Body::Dns => request
            .header("content-type", "application/dns-message")
            .body(a_query("probe.example")),
    };
    if let (Body::Dns, Auth::Bearer(key)) = (body, auth) {
        request = request.bearer_auth(key);
    }
    if let (Body::Dns, Auth::Cookie(cookie)) = (body, auth) {
        request = request.header("cookie", *cookie);
    }
    let response = request
        .send()
        .await
        .unwrap_or_else(|err| panic!("{method} {path} [{}]: {err}", auth.label()));
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.bytes().await.expect("response body").to_vec();
    (status, headers, bytes)
}

async fn send_raw(
    instance: &Instance,
    auth: &Auth<'_>,
    path: &str,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut stream = tls_connect_from(
        None,
        instance.ports.api,
        "127.0.0.1",
        Arc::new(insecure_client_config()),
    )
    .await
    .expect("TLS to the API listener");
    let credential = match auth {
        Auth::None => String::new(),
        Auth::Bearer(key) => format!("Authorization: Bearer {key}\r\n"),
        Auth::Cookie(cookie) => format!("Cookie: {cookie}\r\n"),
    };
    let request = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n{credential}\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("send the raw request");
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut raw))
        .await
        .expect("the API closes an HTTP/1.0 exchange within 10 s")
        .ok();
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("GET {path}: no HTTP head in {} bytes", raw.len()));
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let body = raw[split + 4..].to_vec();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .and_then(|code| StatusCode::from_u16(code).ok())
        .unwrap_or_else(|| panic!("GET {path}: no status line in {head:?}"));
    let mut headers = HeaderMap::new();
    for line in head.lines().skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(name.trim().as_bytes()),
                reqwest::header::HeaderValue::from_str(value.trim()),
            ) {
                headers.insert(name, value);
            }
        }
    }
    (status, headers, body)
}

fn base64url_unpadded(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut word = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            word |= u32::from(*byte) << (16 - 8 * index);
        }
        for index in 0..=chunk.len() {
            let sextet = (word >> (18 - 6 * index)) & 0x3F;
            out.push(ALPHABET[sextet as usize] as char);
        }
    }
    out
}

fn read_config(instance: &Instance, file: &str) -> String {
    std::fs::read_to_string(instance.config_dir.path().join(file))
        .unwrap_or_else(|err| panic!("reading {file} from the config volume: {err}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ca_key_unreachable_via_every_route() {
    walk_every_route_for_key_material(Ipv4Addr::new(127, 0, 0, 20), &[], "clients: []").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ca_key_unreachable_via_every_route_with_a_client_listed() {
    walk_every_route_for_key_material(
        Ipv4Addr::new(127, 0, 0, 28),
        &["127.0.0.1"],
        "clients: [127.0.0.1]",
    )
    .await;
}

async fn walk_every_route_for_key_material(origin_ip: Ipv4Addr, clients: &[&str], mode: &str) {
    let instance = boot_full(full(origin_ip, clients)).await;
    instance.generate_ca().await;

    let ca_key_pem = read_config(&instance, "ca-key.pem");
    let api_key_pem = read_config(&instance, "api-key.pem");
    let ca_pem = read_config(&instance, "ca-cert.pem");
    let needles = [
        Needles::from_pem("ca-key.pem", &ca_key_pem),
        Needles::from_pem("api-key.pem", &api_key_pem),
    ];
    for needle in &needles {
        assert!(
            needle.found_in(ca_key_pem.as_bytes()).is_some()
                || needle.found_in(api_key_pem.as_bytes()).is_some(),
            "{}: the detector must recognise the key it was built from",
            needle.label
        );
    }

    let password = password_from_log(&instance.engine_log())
        .expect("the first boot logs the generated dashboard password once");
    let cookie = login_cookie(&instance, &password).await;

    let routes = documented_routes(&ca_pem, &ca_key_pem);
    let mut leaks = Vec::new();
    let mut walked = 0usize;
    for auth in [
        Auth::None,
        Auth::Bearer(&instance.key),
        Auth::Cookie(&cookie),
    ] {
        for (method, path, body) in &routes {
            let (status, _, bytes) = send(&instance, &auth, method, path, body).await;
            walked += 1;
            for needle in &needles {
                if let Some(what) = needle.found_in(&bytes) {
                    leaks.push(format!(
                        "{method} {path} [{}] -> {status}: {what} of {}",
                        auth.label(),
                        needle.label
                    ));
                }
            }
        }
    }

    let (status, _, bytes) = send(
        &instance,
        &Auth::Bearer(&instance.key),
        &Method::POST,
        "/api/v1/auth/logout-all",
        &Body::Empty,
    )
    .await;
    assert!(status.is_success(), "logout-all answered {status}");
    walked += 1;
    let (status, _, rotated) = send(
        &instance,
        &Auth::Bearer(&instance.key),
        &Method::POST,
        "/api/v1/config/apikey/rotate",
        &Body::Empty,
    )
    .await;
    assert!(status.is_success(), "apikey rotate answered {status}");
    walked += 1;
    for body in [&bytes, &rotated] {
        for needle in &needles {
            if let Some(what) = needle.found_in(body) {
                leaks.push(format!("logout-all/rotate: {what} of {}", needle.label));
            }
        }
    }

    assert!(
        leaks.is_empty(),
        "private key material reached a response body:\n{}",
        leaks.join("\n")
    );
    assert!(
        walked >= 3 * routes.len(),
        "the walk must cover every documented route under every credential"
    );
    println!(
        "ca_key_unreachable_via_every_route [{mode}]: {walked} requests over {} routes x 3 \
         credentials, 0 leaks",
        routes.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn non_listed_client_is_never_minted_a_leaf() {
    let origin_ip = Ipv4Addr::new(127, 0, 0, 21);
    let Some(listener) = bind_origin(origin_ip).await else {
        eprintln!("{}", skip_origin_message(origin_ip));
        return;
    };
    let (origin_cert, origin_key) = self_signed_origin(&[PAGE_HOST]);
    let payload = Arc::new(pseudo_random_payload(64 * 1024, 21));
    let origin = run_tls_origin(
        listener,
        origin_cert.clone(),
        origin_key,
        Arc::clone(&payload),
    );

    let instance = boot_full(full(origin_ip, &["127.0.0.2"])).await;
    let ca_der = instance.generate_ca().await;
    let mut events = connect_events(&instance.base, &instance.key).await;
    let https = instance.ports.https();

    let mut spliced = tls_connect_from(
        Some(LOCAL),
        https,
        PAGE_HOST,
        client_config_trusting(origin_cert.to_vec()),
    )
    .await
    .expect("a non-listed client trusting the origin completes the origin's handshake through the splice");
    let observed = spliced
        .get_ref()
        .1
        .peer_certificates()
        .expect("the origin presented a chain")
        .first()
        .expect("a leaf")
        .to_vec();
    assert_eq!(
        observed,
        origin_cert.to_vec(),
        "the certificate the non-listed client sees must be the origin's own, never a minted leaf"
    );
    let mut relayed = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), spliced.read_to_end(&mut relayed))
        .await
        .expect("the origin closes within 10 s")
        .ok();
    assert_eq!(relayed, *payload, "the spliced payload is byte-identical");

    let event = await_event(&mut events, "https-sni for the non-listed client", |data| {
        data["kind"] == "https-sni" && data["domain"] == PAGE_HOST
    })
    .await;
    assert_eq!(event["verdict"], "pass");
    assert_eq!(event["client"], "127.0.0.1");

    let ours = tls_connect_from(
        Some(LOCAL),
        https,
        PAGE_HOST,
        client_config_trusting(ca_der.clone()),
    )
    .await;
    assert!(
        ours.is_err(),
        "a non-listed client trusting only our CA must fail: nothing on its path presents our leaf"
    );

    let listed = tls_connect_from(
        Some(OTHER_CLIENT),
        https,
        PAGE_HOST,
        client_config_trusting(origin_cert.to_vec()),
    )
    .await;
    assert!(
        listed.is_err(),
        "the listed client is never spliced: its origin-trusting handshake cannot complete"
    );
    let event = await_event(&mut events, "https for the listed client", |data| {
        data["kind"] == "https" && data["client"] == "127.0.0.2"
    })
    .await;
    assert_eq!(event["domain"], PAGE_HOST);

    let certificates = instance.certificates().await;
    assert_eq!(
        certificates["leaf_cache"]["minted_total"], 0,
        "no leaf is ever minted for a non-listed client, and the listed one fails upstream \
         verification before a mint: {certificates}"
    );
    assert_eq!(certificates["leaf_cache"]["size"], 0);
    assert!(
        origin.handshakes.load(Ordering::Relaxed) >= 1,
        "the spliced client handshook with the origin itself"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_upstream_cert_is_not_masked() {
    let origin_ip = Ipv4Addr::new(127, 0, 0, 22);
    let Some(listener) = bind_origin(origin_ip).await else {
        eprintln!("{}", skip_origin_message(origin_ip));
        return;
    };
    let (origin_cert, origin_key) = self_signed_origin(&[PAGE_HOST]);
    let origin = run_tls_origin(
        listener,
        origin_cert.clone(),
        origin_key,
        Arc::new(pseudo_random_payload(1024, 22)),
    );

    let instance = boot_full(full(origin_ip, &["127.0.0.1"])).await;
    let ca_der = instance.generate_ca().await;
    let mut events = connect_events(&instance.base, &instance.key).await;
    let https = instance.ports.https();

    let outcome = tls_connect_from(
        Some(LOCAL),
        https,
        PAGE_HOST,
        client_config_trusting(ca_der.clone()),
    )
    .await;
    assert!(
        outcome.is_err(),
        "an origin the binary cannot verify must never be re-signed into a success"
    );
    let event = await_event(&mut events, "https 526", |data| {
        data["kind"] == "https" && data["domain"] == PAGE_HOST
    })
    .await;
    assert_eq!(
        event["status"], 526,
        "the failure is reported as an upstream certificate failure, not masked: {event}"
    );
    assert_eq!(event["bytes"], 0);

    let anything = tls_connect_from(
        Some(LOCAL),
        https,
        PAGE_HOST,
        Arc::new(insecure_client_config()),
    )
    .await;
    assert!(
        anything.is_err(),
        "even a client that would accept any certificate sees no ServerHello: the connection is \
         closed before our handshake"
    );

    let certificates = instance.certificates().await;
    assert_eq!(
        certificates["leaf_cache"]["minted_total"], 0,
        "verification precedes minting, so an unverifiable origin never costs a leaf"
    );
    assert!(
        origin.accepts.load(Ordering::Relaxed) >= 2,
        "the binary contacted the origin to verify it on every attempt"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bad_origin_certificate_is_relayed_untouched_when_interception_is_off() {
    let origin_ip = Ipv4Addr::new(127, 0, 0, 24);
    let Some(listener) = bind_origin(origin_ip).await else {
        eprintln!("{}", skip_origin_message(origin_ip));
        return;
    };
    let (origin_cert, origin_key) = self_signed_origin(&[PAGE_HOST]);
    let origin = run_tls_origin(
        listener,
        origin_cert.clone(),
        origin_key,
        Arc::new(pseudo_random_payload(1024, 24)),
    );

    let instance = boot_full(full(origin_ip, &[])).await;
    let ca_der = instance.generate_ca().await;
    let mut events = connect_events(&instance.base, &instance.key).await;
    let https = instance.ports.https();

    let trusting_the_origin = tls_connect_from(
        Some(LOCAL),
        https,
        PAGE_HOST,
        client_config_trusting(origin_cert.to_vec()),
    )
    .await;
    assert!(
        trusting_the_origin.is_ok(),
        "a client trusting the origin's own certificate must complete the handshake through the \
         splice: anything else means the certificate was replaced"
    );

    let trusting_our_ca = tls_connect_from(
        Some(LOCAL),
        https,
        PAGE_HOST,
        client_config_trusting(ca_der.clone()),
    )
    .await;
    assert!(
        trusting_our_ca.is_err(),
        "trusting our CA must not be enough: with nobody listed, nothing is re-signed and the \
         client sees the origin's own certificate"
    );

    let event = await_event(&mut events, "https-sni pass", |data| {
        data["kind"] == "https-sni" && data["domain"] == PAGE_HOST
    })
    .await;
    assert_eq!(
        event["verdict"], "pass",
        "the SNI verdict decides the connection; the origin's certificate is not our business: \
         {event}"
    );

    let certificates = instance.certificates().await;
    assert_eq!(
        certificates["leaf_cache"]["minted_total"], 0,
        "an origin the binary would refuse under interception costs no leaf when nobody is listed"
    );
    assert!(
        origin.accepts.load(Ordering::Relaxed) >= 2,
        "both connections must reach the origin: the splice relays, it does not verify"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exports_contain_no_private_material() {
    let instance = boot_full(full(Ipv4Addr::new(127, 0, 0, 23), &[])).await;
    let ca_der = instance.generate_ca().await;
    let ca_key_pem = read_config(&instance, "ca-key.pem");
    let ca_needles = Needles::from_pem("ca-key.pem", &ca_key_pem);

    let pem_bytes = instance.export_ca("pem").await;
    let pem = String::from_utf8(pem_bytes.clone()).expect("PEM is text");
    assert_eq!(
        pem.matches("-----BEGIN CERTIFICATE-----").count(),
        1,
        "exactly one certificate block: {pem}"
    );
    assert!(
        !pem.contains("PRIVATE KEY"),
        "the PEM export carries no key block"
    );
    let pem_body: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    assert_eq!(
        decode_base64(pem_body.as_bytes()),
        ca_der,
        "the PEM and DER exports encode the same certificate"
    );
    assert!(ca_needles.found_in(&pem_bytes).is_none());
    assert!(ca_needles.found_in(&ca_der).is_none());
    client_config_trusting(ca_der.clone());

    let status = instance.certificates().await;
    assert!(
        ca_needles.found_in(status.to_string().as_bytes()).is_none(),
        "the status document carries no key material"
    );

    let (server_cert, server_key) = self_signed_origin(&["fastadhunter"]);
    let server_cert_pem = pem_block("CERTIFICATE", server_cert.as_ref());
    let server_key_pem = pem_block("PRIVATE KEY", server_key.secret_der());
    let import_needles = Needles::from_pem("imported api-key.pem", &server_key_pem);
    let imported = instance
        .http
        .post(format!("{}/api/v1/certificates/import", instance.base))
        .bearer_auth(&instance.key)
        .json(&json!({ "cert_pem": server_cert_pem, "key_pem": server_key_pem }))
        .send()
        .await
        .expect("import an API pair");
    let import_status = imported.status();
    let import_body = imported.bytes().await.expect("import body").to_vec();
    assert_eq!(
        import_status,
        200,
        "a valid pair imports: {}",
        String::from_utf8_lossy(&import_body)
    );
    assert!(
        import_needles.found_in(&import_body).is_none(),
        "the import response echoes no key"
    );
    let stored_cert = read_config(&instance, "api-cert.pem");
    assert!(
        import_needles.found_in(stored_cert.as_bytes()).is_none()
            && !stored_cert.contains("PRIVATE KEY"),
        "the stored certificate file holds no key material"
    );
    let stored_key = read_config(&instance, "api-key.pem");
    assert!(
        import_needles.found_in(stored_key.as_bytes()).is_some(),
        "the imported key landed in api-key.pem"
    );

    let status = instance.certificates().await;
    assert_eq!(status["api_certificate"]["source"], "imported");
    assert!(status.to_string().find("PRIVATE").is_none());
    assert!(import_needles
        .found_in(status.to_string().as_bytes())
        .is_none());

    let config = common::get_json(
        &instance.http,
        &instance.base,
        &instance.key,
        "/api/v1/config",
    )
    .await;
    let config_text = config.to_string();
    assert!(ca_needles.found_in(config_text.as_bytes()).is_none());
    assert!(import_needles.found_in(config_text.as_bytes()).is_none());
}

fn pem_block(label: &str, der: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut body = String::with_capacity(der.len().div_ceil(3) * 4);
    for chunk in der.chunks(3) {
        let mut word = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            word |= u32::from(*byte) << (16 - 8 * index);
        }
        for index in 0..4 {
            if index <= chunk.len() {
                let sextet = (word >> (18 - 6 * index)) & 0x3F;
                body.push(ALPHABET[sextet as usize] as char);
            } else {
                body.push('=');
            }
        }
    }
    let mut out = format!("-----BEGIN {label}-----\n");
    for line in body.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(line).expect("ascii"));
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn splice_is_byte_identical_when_interception_is_off() {
    let origin_ip = Ipv4Addr::new(127, 0, 0, 25);
    let Some(listener) = bind_origin(origin_ip).await else {
        eprintln!("{}", skip_origin_message(origin_ip));
        return;
    };
    let payload = Arc::new(pseudo_random_payload(256 * 1024, 25));
    let accepts = run_raw_origin(listener, Arc::clone(&payload));

    let instance = boot_full(full(origin_ip, &[])).await;
    let mut events = connect_events(&instance.base, &instance.key).await;
    let hello = client_hello(PAGE_HOST);
    let origin_addr = SocketAddr::from((origin_ip, ORIGIN_PORT));
    let splice_addr = SocketAddr::from((LOCAL, instance.ports.https()));

    for sample in 0..3 {
        let direct = raw_exchange(origin_addr, &hello).await;
        let spliced = raw_exchange(splice_addr, &hello).await;
        assert_eq!(
            direct.len(),
            payload.len(),
            "sample {sample}: direct length"
        );
        assert_eq!(
            spliced.len(),
            direct.len(),
            "sample {sample}: the splice relays every byte"
        );
        assert!(
            spliced == direct,
            "sample {sample}: the spliced stream differs from the direct one"
        );
        let event = await_event(&mut events, "https-sni pass", |data| {
            data["kind"] == "https-sni" && data["domain"] == PAGE_HOST
        })
        .await;
        assert_eq!(event["verdict"], "pass");
        assert_eq!(event["bytes"], payload.len() as u64);
    }
    assert_eq!(accepts.load(Ordering::Relaxed), 6);

    let certificates = instance.certificates().await;
    assert_eq!(certificates["leaf_cache"]["minted_total"], 0);
    assert_eq!(certificates["ca"]["present"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dns_query_is_the_only_new_unauthenticated_route() {
    let instance = boot_full(full(Ipv4Addr::new(127, 0, 0, 26), &[])).await;
    instance.generate_ca().await;
    let ca_pem = read_config(&instance, "ca-cert.pem");
    let certificates = instance.certificates().await;
    assert_eq!(certificates["dot"]["state"], "listening", "{certificates}");
    assert_eq!(
        certificates["dot"]["address"],
        format!("127.0.0.1:{}", instance.ports.dot),
        "{certificates}"
    );

    let mut answered_without_credentials = Vec::new();
    let mut wrong = Vec::new();
    for (method, path, body) in documented_routes(&ca_pem, "-----BEGIN NONE-----\nAAAA\n") {
        let (status, headers, bytes) = send(&instance, &Auth::None, &method, &path, &body).await;
        let bare = path.split('?').next().unwrap_or(&path);
        if bare.starts_with("/api/") {
            if bare == "/api/v1/auth/login" {
                if status == StatusCode::NOT_FOUND {
                    wrong.push(format!("{method} {path}: login must exist"));
                }
                continue;
            }
            if status != StatusCode::UNAUTHORIZED {
                wrong.push(format!(
                    "{method} {path}: expected 401 without credentials, got {status}"
                ));
            }
            continue;
        }
        if status == StatusCode::UNAUTHORIZED {
            continue;
        }
        let content_type = headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        answered_without_credentials.push((method.clone(), bare.to_string(), status, content_type));
        if bare == "/dns-query" {
            assert_eq!(status, 200, "{method} {path}");
            assert_eq!(headers["content-type"], "application/dns-message");
            assert!(!bytes.is_empty());
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    let mut public: Vec<String> = answered_without_credentials
        .iter()
        .map(|(_, path, _, _)| path.clone())
        .filter(|path| {
            !path.starts_with('/')
                || path.starts_with("/api")
                || path == "/health"
                || path == "/dns-query"
        })
        .collect();
    public.sort();
    public.dedup();
    assert_eq!(
        public,
        vec!["/dns-query".to_string(), "/health".to_string()],
        "outside the static dashboard, only /health and /dns-query answer without credentials"
    );
    let dashboard: Vec<&(Method, String, StatusCode, String)> = answered_without_credentials
        .iter()
        .filter(|(_, path, _, _)| path != "/health" && path != "/dns-query")
        .collect();
    for (method, path, status, content_type) in &dashboard {
        assert!(
            !content_type.starts_with("application/json") || *status == StatusCode::NOT_FOUND,
            "{method} {path} answered {status} {content_type} without credentials: a static path \
             must never serve API JSON"
        );
        assert!(
            !path.starts_with("/config")
                || *status != StatusCode::OK
                || content_type.starts_with("text/html"),
            "{method} {path}: a /config-shaped path may only fall back to the dashboard shell"
        );
    }
    let static_served = dashboard
        .iter()
        .filter(|(_, _, status, _)| *status == StatusCode::OK)
        .count();
    println!(
        "static dashboard probes: {static_served} of {} answered 200 — {}",
        dashboard.len(),
        if static_served == 0 {
            "no web root on this box, so the two static-path assertions above were vacuous \
             (X5); the on-device curl walk is the closing evidence"
        } else {
            "a web root was present, so the static-path assertions above exercised the file server"
        }
    );

    let plaintext = plaintext_dns_on_the_dot_port(instance.ports.dot).await;
    assert!(
        plaintext.is_none(),
        "plaintext DNS on the DoT port must never be answered: {plaintext:?}"
    );

    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");
    std::fs::write(
        config_dir.path().join("api-cert.pem"),
        "not a certificate\n",
    )
    .expect("seed");
    std::fs::write(config_dir.path().join("api-key.pem"), "not a key\n").expect("seed");
    let closed = boot_full_in(
        config_dir,
        data_dir,
        FullMode {
            origin_ip: Ipv4Addr::new(127, 0, 0, 27),
            clients: Vec::new(),
            api_tls: false,
            upstream_root: None,
            https_limits: None,
        },
    )
    .await;
    assert!(closed.base.starts_with("http://"));
    let answer = resolve(closed.ports.dns, "allowed.example.com").await;
    assert_eq!(
        answer.a_records,
        vec![Ipv4Addr::new(127, 0, 0, 27)],
        ":53 keeps resolving while the DoT listener is closed"
    );
    let free = std::net::TcpListener::bind((LOCAL, closed.ports.dot));
    assert!(
        free.is_ok(),
        "nothing may listen on the DoT port when the API pair is unloadable and api.tls = false"
    );
    let plaintext = plaintext_dns_on_the_dot_port(closed.ports.dot).await;
    assert!(
        plaintext.is_none(),
        "a closed DoT port never answers plaintext"
    );
    let log = closed.engine_log();
    assert!(
        log.contains("dot_enabled = true"),
        "the closed posture is logged once, naming the setting: {log}"
    );
    let doh = closed
        .http
        .post(format!("{}/dns-query", closed.base))
        .header("content-type", "application/dns-message")
        .body(a_query(AD_HOST))
        .send()
        .await
        .expect("POST /dns-query over plain HTTP");
    assert_ne!(
        doh.headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/dns-message"),
        "DoH is HTTPS-only: with api.tls = false the route is absent"
    );
    let health = closed
        .http
        .get(format!("{}/health", closed.base))
        .send()
        .await
        .expect("GET /health");
    let health: Value = health.json().await.expect("health json");
    assert!(
        health.get("dot").is_none() && health.get("checks").is_none(),
        "/health stays status, version and uptime only (SECURITY.md): {health}"
    );
    let certificates = closed.certificates().await;
    assert_eq!(
        certificates["dot"]["state"], "closed",
        "the closed DoT listener is reported on the certificates document (p3-05 N3): \
         {certificates}"
    );
    assert!(
        certificates["dot"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("certificate pair")),
        "the reason names the unloadable API pair: {certificates}"
    );
    assert!(certificates["dot"].get("address").is_none());
}

async fn plaintext_dns_on_the_dot_port(port: u16) -> Option<Vec<u8>> {
    let Ok(Ok(mut stream)) = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect((LOCAL, port)),
    )
    .await
    else {
        return None;
    };
    let query = a_query(AD_HOST);
    let len = u16::try_from(query.len()).expect("short").to_be_bytes();
    if stream.write_all(&len).await.is_err() || stream.write_all(&query).await.is_err() {
        return None;
    }
    let mut buffer = vec![0u8; 4096];
    match tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buffer)).await {
        Ok(Ok(read)) if read >= 2 && buffer[0] != 0x15 => {
            let answer = buffer[..read].to_vec();
            (contains(&answer, &query[2..4]) || read > 12).then_some(answer)
        }
        _ => None,
    }
}
