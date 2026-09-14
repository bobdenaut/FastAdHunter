mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use hickory_proto::op::ResponseCode;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use common::{
    await_event, bind_origin, boot_full, client_config_trusting, connect_events, get_json,
    insecure_client_config, put_user_rules, resolve, resolve_doh_post, resolve_dot, resolve_tcp,
    run_tls_origin, self_signed_origin, skip_origin_message, tls_connect_from, FullMode, AD_HOST,
    DOMAIN_LANE_LOG, DOT_HOSTNAME, FULL_MODE_HTTP_RUNTIMES, PAGE_HOST,
};

const ORIGIN_IP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 40);
const ALLOWED_HOST: &str = "allowed.example.com";
const ORIGIN_PAYLOAD: &[u8] = b"origin payload, relayed byte for byte\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_shipped_configuration_blocks_at_every_layer() {
    let (origin_cert, origin_key) = self_signed_origin(&[PAGE_HOST]);
    let origin = match bind_origin(ORIGIN_IP).await {
        Some(listener) => Some(run_tls_origin(
            listener,
            origin_cert.clone(),
            origin_key,
            Arc::new(ORIGIN_PAYLOAD.to_vec()),
        )),
        None => {
            eprintln!(
                "{} The splice leg is skipped, and with it the only assertion that tells this \
                 build apart from an intercepting one.",
                skip_origin_message(ORIGIN_IP)
            );
            None
        }
    };

    let instance = boot_full(FullMode {
        origin_ip: ORIGIN_IP,
        clients: Vec::new(),
        api_tls: true,
        upstream_root: None,
    })
    .await;

    let log = instance.engine_log();
    assert!(
        log.contains(DOMAIN_LANE_LOG),
        "0/6 lane: with runtime.http_runtimes = {FULL_MODE_HTTP_RUNTIMES} the HTTPS listener \
         must feed the HTTP allocation domains, not the shared runtime\
         \n--- engine log ---\n{log}"
    );

    let ca_der = instance.generate_ca().await;
    let mut events = connect_events(&instance.base, &instance.key).await;
    put_user_rules(
        &instance.http,
        &instance.base,
        &instance.key,
        &[&format!("||{AD_HOST}^"), &format!("||{PAGE_HOST}/track.js")],
    )
    .await;

    let blocked = resolve(instance.ports.dns, AD_HOST).await;
    assert_eq!(blocked.rcode, ResponseCode::NoError);
    assert_eq!(
        blocked.a_records,
        vec![Ipv4Addr::UNSPECIFIED],
        "1/6 DNS over UDP: the blocked domain is answered with the null IP"
    );
    let allowed = resolve(instance.ports.dns, ALLOWED_HOST).await;
    assert_eq!(
        allowed.a_records,
        vec![ORIGIN_IP],
        "1/6 DNS over UDP: an allowed domain is forwarded — the control that says the null IP \
         above is a verdict and not a failure"
    );

    let blocked = resolve_tcp(instance.ports.dns, AD_HOST).await;
    assert_eq!(blocked.rcode, ResponseCode::NoError);
    assert_eq!(
        blocked.a_records,
        vec![Ipv4Addr::UNSPECIFIED],
        "2/6 DNS over TCP: the same verdict arrives over the length-prefixed transport"
    );
    let allowed = resolve_tcp(instance.ports.dns, ALLOWED_HOST).await;
    assert_eq!(
        allowed.a_records,
        vec![ORIGIN_IP],
        "2/6 DNS over TCP: an allowed domain is forwarded"
    );

    let script = fetch_http(instance.ports.http(), PAGE_HOST, "/track.js").await;
    assert_eq!(
        script.0, 200,
        "3/6 HTTP: a blocked script collapses to an empty 200"
    );
    assert!(
        script.1.is_empty(),
        "3/6 HTTP: the blocked script body is empty, got {} bytes",
        script.1.len()
    );

    let https = instance.ports.https();
    let refused = tls_connect_from(None, https, AD_HOST, Arc::new(insecure_client_config())).await;
    assert!(
        refused.is_err(),
        "4/6 SNI: a blocked SNI is closed before any ServerHello"
    );
    let event = await_event(&mut events, "https-sni block", |data| {
        data["kind"] == "https-sni" && data["domain"] == AD_HOST
    })
    .await;
    assert_eq!(event["verdict"], "block", "4/6 SNI: {event}");
    assert_eq!(
        event["bytes"], 0,
        "4/6 SNI: a blocked SNI costs the origin nothing: {event}"
    );

    if let Some(origin) = &origin {
        let spliced = tls_connect_from(None, https, PAGE_HOST, Arc::new(insecure_client_config()))
            .await
            .expect(
                "4/6 SNI: an allowed SNI must complete against the origin through the splice. \
                 Two things break this: the origin is not up, or this build terminated TLS and \
                 then refused the origin's self-signed certificate — which is what an \
                 intercepting build does with no upstream root configured",
            );
        let presented = spliced
            .get_ref()
            .1
            .peer_certificates()
            .expect("4/6 SNI: the handshake presented a chain")
            .to_vec();
        assert_eq!(
            presented[0].as_ref(),
            origin_cert.as_ref(),
            "4/6 SNI: **the assertion that says this build does not intercept.** The client must \
             receive the origin's own certificate, byte for byte. A minted leaf here means the \
             allocation domain terminated TLS, which the empty client list forbids"
        );
        let mut relayed = Vec::new();
        let mut spliced = spliced;
        tokio::time::timeout(Duration::from_secs(10), spliced.read_to_end(&mut relayed))
            .await
            .expect("4/6 SNI: the spliced session ends within 10 s")
            .ok();
        assert_eq!(
            relayed, ORIGIN_PAYLOAD,
            "4/6 SNI: the splice relays the origin's bytes unchanged"
        );
        assert!(
            origin.handshakes.load(Ordering::Relaxed) >= 1,
            "4/6 SNI: the handshake terminated at the origin, not before it"
        );
        await_event(&mut events, "https-sni pass", |data| {
            data["kind"] == "https-sni" && data["domain"] == PAGE_HOST
        })
        .await;
    }

    let blocked = resolve_dot(
        instance.ports.dot,
        AD_HOST,
        client_config_trusting(ca_der.clone()),
        DOT_HOSTNAME,
    )
    .await;
    assert_eq!(
        blocked.a_records,
        vec![Ipv4Addr::UNSPECIFIED],
        "5/6 DoT: the blocked domain is blocked over DoT, against a leaf minted for \
         {DOT_HOSTNAME} under the exported CA"
    );
    let allowed = resolve_dot(
        instance.ports.dot,
        ALLOWED_HOST,
        client_config_trusting(ca_der),
        DOT_HOSTNAME,
    )
    .await;
    assert_eq!(
        allowed.a_records,
        vec![ORIGIN_IP],
        "5/6 DoT: an allowed domain is forwarded over DoT"
    );

    let blocked = resolve_doh_post(&instance.http, &instance.base, AD_HOST).await;
    assert_eq!(
        blocked.a_records,
        vec![Ipv4Addr::UNSPECIFIED],
        "6/6 DoH: the blocked domain is blocked over DoH, on the API server that is also \
         serving the dashboard"
    );
    let allowed = resolve_doh_post(&instance.http, &instance.base, ALLOWED_HOST).await;
    assert_eq!(
        allowed.a_records,
        vec![ORIGIN_IP],
        "6/6 DoH: an allowed domain is forwarded over DoH"
    );

    let certificates = instance.certificates().await;
    assert_eq!(
        certificates["leaf_cache"]["minted_total"], 1,
        "shipped: exactly one leaf exists and it is the DoT hostname's. A second one would mean \
         a domain was terminated: {certificates}"
    );
    assert_eq!(
        certificates["leaf_cache"]["unwarmed_misses"], 0,
        "shipped: the second DoT handshake hit the cache: {certificates}"
    );
    assert_eq!(
        certificates["dot"]["state"], "listening",
        "shipped: the DoT listener reports itself: {certificates}"
    );

    let telemetry = get_json(
        &instance.http,
        &instance.base,
        &instance.key,
        "/api/v1/telemetry",
    )
    .await;
    let https_counters = &telemetry["listeners"]["https"];
    assert_eq!(
        https_counters["handshakes_completed"]
            .as_u64()
            .unwrap_or_default(),
        0,
        "shipped: the allocation domains terminated no TLS. Only intercept.rs raises this \
         counter, so a non-zero value means the empty client list did not hold: {https_counters}"
    );
    assert!(
        https_counters["blocked"].as_u64().unwrap_or_default() >= 1,
        "shipped: the SNI block above reached the listener's counters: {https_counters}"
    );
    assert_eq!(
        https_counters["non_tls"].as_u64().unwrap_or_default(),
        0,
        "shipped: nothing in this scenario spoke plaintext at the HTTPS port, so a non-zero \
         value means the two listener counter sets reached the API transposed: {https_counters}"
    );
}

async fn fetch_http(proxy_port: u16, host: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, proxy_port)))
        .await
        .expect("connect to the HTTP proxy");
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.expect("send");
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut raw))
        .await
        .expect("the proxy closes within 10 s")
        .ok();
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").expect("an HTTP head");
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .expect("a status code");
    (status, body.to_string())
}
