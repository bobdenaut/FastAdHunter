//! On-device HTTP proxy probe (p2-08).
//!
//! Answers what the dev box cannot: **what does the proxy add per request on
//! the RB5009** — head path, opaque body relay, and the two rule-attached paths
//! (a request that passes, a request blocked on the head).
//!
//! The arms mirror `benches/proxy.rs` so the x86 and ARM figures describe the
//! same work, and it is deliberately not criterion: the router has no shell, no
//! writable cwd and no way to collect a `target/criterion` tree, so the probe
//! ships as a container whose entrypoint is the measurement and prints to
//! stdout, which RouterOS copies into its log.
//!
//! **What this measures and what it does not.** Client, proxy and origin all
//! run in this process over loopback, so the figure is FastAdHunter's own cost
//! on this CPU — not the deployed path, which adds veth, dst-nat, conntrack and
//! the real origin's RTT. The deployed path is proved by the nat counters and a
//! browsing check, not here.
//!
//! ```sh
//! # dev box; the corpus is optional and only sizes the rules arms
//! cargo run --release -p fah-http --example httpbench -- .probe-corpus/production
//! ```

use std::convert::Infallible;
use std::hint::black_box;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use fah_common::egress::{AllowedNet, DestinationPolicy};
use fah_common::resolve::{HostResolver, Resolving};
use fah_rules::{parse_rule_list, Matcher, MatcherBuilder};
use http_body_util::{BodyExt, Full};
use hyper::header::HOST;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;

use fah_http::{Proxy, Ruleset};

/// Matches the shipped binary, because a probe measuring the router has to
/// measure the allocator the router runs. Same reasoning as
/// `fah-rules/examples/urlbench.rs`.
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Wall time per arm — long enough that the run describes a steady state
/// rather than whatever the governor was doing at startup.
const ARM_TARGET: Duration = Duration::from_secs(8);

/// One batch aims for this long, so the `Instant::now()` pair is amortised.
const BATCH_TARGET_NANOS: u128 = 1_000_000;

/// Head-path payload: small on purpose, so the arm measures the head.
const PAYLOAD: &[u8] = b"HTTP pass-through benchmark payload; small on purpose.";

/// Same spread as `benches/proxy.rs`: a small asset, a photo, a video chunk.
const BODY_SIZES: [(usize, &str); 3] = [
    (8 * 1024, "8KiB"),
    (1024 * 1024, "1MiB"),
    (8 * 1024 * 1024, "8MiB"),
];

/// The path the probe's own rule blocks. Not in any real list, so the block arm
/// is deterministic whether or not a corpus was baked in.
const BLOCKED_PATH: &str = "/probe-blocked.js";

/// Resolution is a fixed answer — the measurement is the proxy's work, not a
/// resolver's.
struct FixedResolver(Vec<IpAddr>);

impl HostResolver for FixedResolver {
    fn resolve(&self, _host: String) -> Resolving {
        let addresses = self.0.clone();
        Box::pin(async move { Ok(addresses) })
    }
}

/// A compiled ruleset that never swaps — the production `ListManager` without
/// the lifecycle.
struct StaticRuleset(Arc<Matcher>);

impl Ruleset for StaticRuleset {
    fn matcher(&self) -> Arc<Matcher> {
        Arc::clone(&self.0)
    }
}

fn main() {
    println!("[probe] fah httpbench — p2-08 on-device HTTP proxy cost");
    report_cpu();

    let rt = Runtime::new().unwrap();
    let matcher = build_matcher();

    println!(
        "[probe] --- measuring, ~{} s per arm ---",
        ARM_TARGET.as_secs()
    );

    // Head path, no rules: the p2-02 pair, and the baseline every later arm is
    // a delta against.
    head_arms(&rt, "head", None);
    // Head path with the ruleset attached: the delta is p2-04's verdict.
    head_arms(&rt, "head_rules", Some(Arc::clone(&matcher)));
    blocked_arm(&rt, Arc::clone(&matcher));

    for (size, label) in BODY_SIZES {
        body_arms(&rt, size, label);
    }

    report_cpu();
    println!("[probe] done");
}

/// Reads every `.raw`/`.txt` list in the directory given on the command line,
/// and always appends the probe's own blocking rule.
///
/// The corpus is optional and only sizes the URL tier: a lookup against 715
/// rules costs less than one against 18,781, so the arms are reported together
/// with what was loaded rather than as a single number.
fn build_matcher() -> Arc<Matcher> {
    let mut builder = MatcherBuilder::new();
    let mut loaded = 0usize;
    let mut url_rules = 0usize;

    for dir in std::env::args().skip(1) {
        let entries =
            std::fs::read_dir(&dir).unwrap_or_else(|err| panic!("corpus dir {dir}: {err}"));
        let mut paths: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext == "raw" || ext == "txt")
            })
            .collect();
        // Sorted so the compiled ruleset does not depend on iteration order.
        paths.sort();
        for path in paths {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into(),
            );
            let list = parse_rule_list(&text);
            url_rules += list.url_count();
            builder.add_parsed_list(name.as_str(), &list);
            loaded += 1;
        }
    }

    let probe = parse_rule_list(&format!("{BLOCKED_PATH}\n"));
    assert_eq!(
        probe.url_count(),
        1,
        "the probe rule must compile as a URL rule"
    );
    builder.add_parsed_list("probe", &probe);
    url_rules += 1;

    let matcher = builder.build();
    println!(
        "[probe] ruleset : {loaded} corpus list(s), {url_rules} url rules parsed, \
         {} compiled ({} unindexed), matcher {:.2} MiB",
        matcher.url_len(),
        matcher.url_unindexed(),
        matcher.heap_bytes() as f64 / (1024.0 * 1024.0)
    );
    Arc::new(matcher)
}

/// Direct and proxied arms over the same origin, in that order: an idle
/// keep-alive connection would not survive the arm before it, so the proxied
/// connection is opened only once the direct arm is finished.
fn head_arms(rt: &Runtime, label: &str, rules: Option<Arc<Matcher>>) {
    let body = Bytes::from_static(PAYLOAD);
    let origin = rt.block_on(origin_serving(body));
    let proxy = rt.block_on(proxy_in_front_of(origin, rules));
    let host = format!("origin.test:{}", origin.port());

    if label == "head" {
        measure_requests(rt, "head_direct", origin, &host, "/resource");
    }
    measure_requests(rt, &format!("{label}_proxied"), proxy, &host, "/resource");
}

/// The blocked path never reaches the origin: the verdict is taken on the head,
/// so this arm is the answer plus the response synthesis, with no resolve and
/// no upstream connection.
fn blocked_arm(rt: &Runtime, matcher: Arc<Matcher>) {
    let origin = rt.block_on(origin_serving(Bytes::from_static(PAYLOAD)));
    let proxy = rt.block_on(proxy_in_front_of(origin, Some(matcher)));
    let host = format!("origin.test:{}", origin.port());
    measure_requests(rt, "head_blocked", proxy, &host, BLOCKED_PATH);
}

/// Opaque bodies: the verdict is taken on the head, then the bytes are relayed
/// untouched. A cost that scales with transfer size widens the gap between the
/// two arms; which cost it is, this probe cannot say — see PERFORMANCE.md.
fn body_arms(rt: &Runtime, size: usize, label: &str) {
    let origin = rt.block_on(origin_serving(Bytes::from(vec![b'x'; size])));
    let proxy = rt.block_on(proxy_in_front_of(origin, None));
    let host = format!("origin.test:{}", origin.port());

    measure_requests(rt, &format!("body_{label}_direct"), origin, &host, "/asset");
    measure_requests(rt, &format!("body_{label}_proxied"), proxy, &host, "/asset");
}

/// Allocated once and cloned per response: `Bytes` clones are a refcount bump,
/// so the origin's own cost stays out of the measurement at every size.
async fn origin_serving(body: Bytes) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let body = body.clone();
            tokio::spawn(async move {
                let service = service_fn(move |_request: Request<hyper::body::Incoming>| {
                    let body = body.clone();
                    async move { Ok::<_, Infallible>(Response::new(Full::new(body))) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .timer(TokioTimer::new())
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    addr
}

async fn proxy_in_front_of(origin: SocketAddr, rules: Option<Arc<Matcher>>) -> SocketAddr {
    let mut proxy = Proxy::new(
        Arc::new(FixedResolver(vec![origin.ip()])),
        // Loopback is denied by default; the probe's origin is on it, so the
        // exception is what a real internal-service allow-list would look like.
        DestinationPolicy::new(
            origin.port(),
            vec![AllowedNet::host("127.0.0.1".parse().unwrap())],
        ),
        origin.port(),
        // Generous: at the shipped 10 s default the header timeout closes the
        // connection mid-run, which is the slowloris bound doing its job.
        Duration::from_secs(120),
        Duration::from_secs(120),
        8,
        false,
    );
    if let Some(matcher) = rules {
        proxy = proxy.with_rules(Arc::new(StaticRuleset(matcher)));
    }
    let proxy = Arc::new(proxy);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                return;
            };
            let proxy = Arc::clone(&proxy);
            tokio::spawn(async move { proxy.serve_connection(stream, peer).await });
        }
    });
    addr
}

async fn connect(addr: SocketAddr) -> hyper::client::conn::http1::SendRequest<Full<Bytes>> {
    let stream = TcpStream::connect(addr).await.unwrap();
    stream.set_nodelay(true).unwrap();
    let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    sender
}

fn request(host: &str, path: &str) -> Request<Full<Bytes>> {
    Request::builder()
        .uri(path)
        .header(HOST, host)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

/// One arm: connect, warm the connection (which also pays for the upstream one
/// every measured iteration then reuses), then time round trips.
fn measure_requests(rt: &Runtime, name: &str, addr: SocketAddr, host: &str, path: &str) {
    let mut sender = rt.block_on(connect(addr));
    rt.block_on(async {
        sender.ready().await.unwrap();
        let response = sender.send_request(request(host, path)).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        println!(
            "[probe] {name:<22} warm-up: {} status, {} body bytes",
            status.as_u16(),
            bytes.len()
        );
    });

    measure(name, || {
        rt.block_on(async {
            // Reused across iterations, so wait for capacity rather than
            // assuming the connection is idle.
            sender.ready().await.unwrap();
            let response = sender.send_request(request(host, path)).await.unwrap();
            black_box(response.into_body().collect().await.unwrap().to_bytes());
        });
    });
}

/// Time `op` and print a distribution — byte-identical to `urlbench`'s harness,
/// so the two probes' output can be read the same way.
fn measure(name: &str, mut op: impl FnMut()) {
    let probe = Instant::now();
    let mut warm = 0u64;
    while probe.elapsed() < Duration::from_millis(500) {
        op();
        warm += 1;
    }
    let per_nanos = (probe.elapsed().as_nanos() / u128::from(warm.max(1))).max(1);
    let inner = (BATCH_TARGET_NANOS / per_nanos).clamp(1, 100_000) as u64;
    let batches = (ARM_TARGET.as_nanos() / (per_nanos * u128::from(inner))).clamp(20, 5_000) as u64;

    let mut samples = Vec::with_capacity(batches as usize);
    for _ in 0..batches {
        let started = Instant::now();
        for _ in 0..inner {
            op();
        }
        samples.push(started.elapsed().as_nanos() as f64 / inner as f64);
    }
    samples.sort_by(f64::total_cmp);

    let n = samples.len();
    let mean = samples.iter().sum::<f64>() / n as f64;
    let pick = |q: f64| samples[((n as f64 * q) as usize).min(n - 1)];
    println!(
        "[bench] {name:<22} min {:>10.3} us  p50 {:>10.3}  mean {:>10.3}  p99 {:>10.3}  max {:>10.3}   ({n} batches x {inner})",
        samples[0] / 1000.0,
        pick(0.50) / 1000.0,
        mean / 1000.0,
        pick(0.99) / 1000.0,
        samples[n - 1] / 1000.0,
    );
}

/// Informational only — the reported frequency is not a calibration input
/// (`docs/measurement-traps.md`); the ~9× x86 → RB5009 factor is.
fn report_cpu() {
    let freq = std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq")
        .ok()
        .map_or_else(
            || "unavailable".to_string(),
            |khz| {
                khz.trim().parse::<f64>().map_or_else(
                    |_| khz.trim().to_string(),
                    |k| format!("{:.0} MHz", k / 1000.0),
                )
            },
        );
    println!("[probe] cpu0 scaling_cur_freq: {freq} (informational — not a calibration input)");
}
