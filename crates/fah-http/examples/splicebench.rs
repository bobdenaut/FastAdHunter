use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fah_common::egress::{AllowedNet, DestinationPolicy};
use fah_common::resolve::{HostResolver, Resolving};
use fah_config::{HttpsConfig, HttpsListenConfig};
use fah_http::{ProxyCounters, TlsProxy, TlsServer};
use fah_model::Event;
use fah_rules::{parse_rule_list, Matcher, MatcherBuilder};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;

#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;
const CANDIDATES_KIB: [(usize, usize); 5] = [(16, 16), (16, 32), (16, 64), (16, 128), (64, 64)];
const DEFAULT_REPS: usize = 5;
const DEFAULT_SIZE_MIB: usize = 64;
const DEFAULT_BUDGET_MIB: usize = 32;
const MAX_CONNECTIONS: usize = 1024;
const DRAIN_CHUNK: usize = 64 * KIB;
const SPLICE_HOST: &str = "origin.test";
const RULES: &str = "||ads.example^\n\
    ||tracker.example^\n\
    ||metrics.example^$script\n\
    /track.js\n\
    /banner.\n\
    ||cdn.example/analytics/\n";

struct Options {
    reps: usize,
    size: usize,
    budget: usize,
}

fn options() -> Options {
    let mut options = Options {
        reps: DEFAULT_REPS,
        size: DEFAULT_SIZE_MIB * MIB,
        budget: DEFAULT_BUDGET_MIB * MIB,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or_else(|| panic!("{flag} needs a number"));
        match flag.as_str() {
            "--reps" => options.reps = value,
            "--size-mib" => options.size = value * MIB,
            "--budget-mib" => options.budget = value * MIB,
            other => panic!("unknown flag {other}; flags: --reps --size-mib --budget-mib"),
        }
    }
    options
}

struct FixedResolver(Vec<IpAddr>);

impl HostResolver for FixedResolver {
    fn resolve(&self, _host: String) -> Resolving {
        let addresses = self.0.clone();
        Box::pin(async move { Ok(addresses) })
    }
}

struct FixedRules(Arc<Matcher>);

impl fah_http::Ruleset for FixedRules {
    fn matcher(&self) -> Arc<Matcher> {
        Arc::clone(&self.0)
    }
}

fn rules() -> Arc<dyn fah_http::Ruleset> {
    let parsed = parse_rule_list(RULES);
    let mut builder = MatcherBuilder::new();
    builder.add_parsed_list("splicebench", &parsed);
    Arc::new(FixedRules(Arc::new(builder.build())))
}

fn drained_events() -> tokio::sync::mpsc::Sender<Event> {
    let (events, mut drain) = tokio::sync::mpsc::channel(1024);
    tokio::spawn(async move { while drain.recv().await.is_some() {} });
    events
}

fn client_hello(host: &str) -> Vec<u8> {
    let mut entry = vec![0u8];
    entry.extend_from_slice(&(host.len() as u16).to_be_bytes());
    entry.extend_from_slice(host.as_bytes());
    let mut sni = Vec::new();
    sni.extend_from_slice(&(entry.len() as u16).to_be_bytes());
    sni.extend_from_slice(&entry);
    let mut extensions = Vec::new();
    extensions.extend_from_slice(&0u16.to_be_bytes());
    extensions.extend_from_slice(&(sni.len() as u16).to_be_bytes());
    extensions.extend_from_slice(&sni);
    let mut body = Vec::new();
    body.extend_from_slice(&[0x03, 0x03]);
    body.extend_from_slice(&[0x42; 32]);
    body.push(0);
    body.extend_from_slice(&2u16.to_be_bytes());
    body.extend_from_slice(&[0x13, 0x01]);
    body.push(1);
    body.push(0);
    body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    body.extend_from_slice(&extensions);
    let mut handshake = vec![0x01u8];
    handshake.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    handshake.extend_from_slice(&body);
    let mut record = vec![0x16u8, 0x03, 0x01];
    record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
    record.extend_from_slice(&handshake);
    record
}

async fn raw_origin(payload: Arc<Vec<u8>>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let payload = Arc::clone(&payload);
            tokio::spawn(async move {
                let (mut reader, mut writer) = stream.into_split();
                tokio::spawn(async move {
                    let mut sink = tokio::io::sink();
                    let _ = tokio::io::copy(&mut reader, &mut sink).await;
                });
                let _ = writer.write_all(&payload).await;
                let _ = writer.shutdown().await;
            });
        }
    });
    addr
}

async fn splice_in_front_of(
    origin: SocketAddr,
    up: usize,
    down: usize,
) -> (SocketAddr, Arc<ProxyCounters>) {
    let proxy = Arc::new(
        TlsProxy::new(
            Arc::new(FixedResolver(vec![origin.ip()])),
            DestinationPolicy::new(
                origin.port(),
                vec![AllowedNet::host("127.0.0.1".parse().unwrap())],
            ),
            origin.port(),
            Duration::from_secs(120),
            Duration::from_secs(120),
            fah_config::NoSni::Pass,
        )
        .with_splice_buffers(up, down)
        .with_rules(rules())
        .with_events(drained_events()),
    );
    let counters = proxy.counters();
    let config = HttpsConfig {
        listen: HttpsListenConfig {
            address: "127.0.0.1".to_string(),
            port: 0,
        },
        ..HttpsConfig::default()
    };
    let mut server = TlsServer::bind(&config).await.unwrap();
    let addr = server.local_addr();
    server.serve(proxy);
    (addr, counters)
}

struct Timing {
    steady: Duration,
    total: Duration,
}

async fn drain(addr: SocketAddr, hello: Option<&[u8]>, size: usize) -> Timing {
    let started = Instant::now();
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.set_nodelay(true).unwrap();
    if let Some(hello) = hello {
        stream.write_all(hello).await.unwrap();
    }
    let mut buf = vec![0u8; DRAIN_CHUNK];
    let mut received = 0usize;
    let mut first = None;
    while received < size {
        let read = stream.read(&mut buf).await.unwrap();
        assert!(read > 0, "origin closed after {received} of {size} bytes");
        if first.is_none() {
            first = Some(Instant::now());
        }
        received += read;
    }
    let done = Instant::now();
    Timing {
        steady: done - first.unwrap(),
        total: done - started,
    }
}

fn mib_per_s(bytes: usize, elapsed: Duration) -> f64 {
    bytes as f64 / MIB as f64 / elapsed.as_secs_f64()
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sorted[(sorted.len() - 1) / 2]
}

fn min(values: &[f64]) -> f64 {
    values.iter().cloned().fold(f64::INFINITY, f64::min)
}

fn max(values: &[f64]) -> f64 {
    values.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
}

struct Candidate {
    up: usize,
    down: usize,
    proxy: SocketAddr,
    counters: Arc<ProxyCounters>,
    steady: Vec<f64>,
    total: Vec<f64>,
}

impl Candidate {
    fn worst_case(&self) -> usize {
        (self.up + self.down) * MAX_CONNECTIONS
    }
}

fn main() {
    let options = options();
    let rt = Runtime::new().unwrap();
    let hello = client_hello(SPLICE_HOST);
    let payload = Arc::new(vec![b'x'; options.size]);
    let origin = rt.block_on(raw_origin(Arc::clone(&payload)));
    let mut candidates: Vec<Candidate> = CANDIDATES_KIB
        .iter()
        .map(|&(up_kib, down_kib)| {
            let (up, down) = (up_kib * KIB, down_kib * KIB);
            let (proxy, counters) = rt.block_on(splice_in_front_of(origin, up, down));
            Candidate {
                up,
                down,
                proxy,
                counters,
                steady: Vec::with_capacity(options.reps),
                total: Vec::with_capacity(options.reps),
            }
        })
        .collect();
    let mut loopback_origin = Vec::with_capacity(options.reps);

    println!(
        "splicebench: {} MiB per connection, {} reps, candidates interleaved per rep, \
         budget {} MiB at max_connections = {MAX_CONNECTIONS}, allocator mimalloc",
        options.size / MIB,
        options.reps,
        options.budget / MIB
    );
    rt.block_on(drain(origin, None, options.size));
    for candidate in &candidates {
        rt.block_on(drain(candidate.proxy, Some(&hello), options.size));
    }

    for rep in 1..=options.reps {
        let timing = rt.block_on(drain(origin, None, options.size));
        let rate = mib_per_s(options.size, timing.steady);
        loopback_origin.push(rate);
        println!("rep={rep} arm=loopback_origin steady_mib_s={rate:.1}");
        for candidate in candidates.iter_mut() {
            let timing = rt.block_on(drain(candidate.proxy, Some(&hello), options.size));
            let steady = mib_per_s(options.size, timing.steady);
            let total = mib_per_s(options.size, timing.total);
            candidate.steady.push(steady);
            candidate.total.push(total);
            println!(
                "rep={rep} arm=splice up_kib={} down_kib={} steady_mib_s={steady:.1} total_mib_s={total:.1}",
                candidate.up / KIB,
                candidate.down / KIB
            );
        }
    }

    println!(
        "candidate up_kib down_kib n median_mib_s min max median_total worst_case_mib in_budget"
    );
    println!(
        "loopback_origin - - {} {:.1} {:.1} {:.1} - - -",
        loopback_origin.len(),
        median(&loopback_origin),
        min(&loopback_origin),
        max(&loopback_origin)
    );
    for candidate in &candidates {
        println!(
            "splice {} {} {} {:.1} {:.1} {:.1} {:.1} {} {}",
            candidate.up / KIB,
            candidate.down / KIB,
            candidate.steady.len(),
            median(&candidate.steady),
            min(&candidate.steady),
            max(&candidate.steady),
            median(&candidate.total),
            candidate.worst_case() / MIB,
            candidate.worst_case() <= options.budget
        );
    }

    let best = candidates
        .iter()
        .map(|c| median(&c.steady))
        .fold(f64::NEG_INFINITY, f64::max);
    let pick = candidates
        .iter()
        .filter(|c| c.worst_case() <= options.budget && median(&c.steady) >= 0.9 * best)
        .min_by_key(|c| c.up + c.down);
    match pick {
        Some(c) => println!(
            "pick (smallest in-budget candidate with median >= 0.9 x best {best:.1}): up_kib={} down_kib={} median_mib_s={:.1}",
            c.up / KIB,
            c.down / KIB,
            median(&c.steady)
        ),
        None => println!("pick: none — no in-budget candidate reaches 0.9 x best {best:.1}"),
    }
    let sym = candidates
        .iter()
        .find(|c| c.up == 64 * KIB && c.down == 64 * KIB);
    let asym = candidates
        .iter()
        .find(|c| c.up == 16 * KIB && c.down == 64 * KIB);
    if let (Some(sym), Some(asym)) = (sym, asym) {
        let gain = median(&sym.steady) / median(&asym.steady) - 1.0;
        println!(
            "up sensitivity: 64/64 over 16/64 = {:+.1}% (up stays 16 unless > +10%)",
            gain * 100.0
        );
    }

    for candidate in &candidates {
        let stats = candidate.counters.snapshot();
        println!(
            "counters up_kib={} down_kib={}: connections={} requests={} blocked={} \
             refused_destination={} resolve_failures={} dropped_events={}",
            candidate.up / KIB,
            candidate.down / KIB,
            stats.connections,
            stats.requests,
            stats.blocked,
            stats.refused_destination,
            stats.resolve_failures,
            stats.dropped_events
        );
    }
}
