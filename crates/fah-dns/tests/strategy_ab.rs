mod support;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fah_config::{
    DnsCacheConfig, DnsConfig, DnsListenConfig, DnsUpstreamsConfig, RulesConfig, UpstreamProtocol,
    UpstreamServerConfig, UpstreamStrategy,
};
use fah_dns::{Pipeline, Server, UpstreamPool, UpstreamStatus, DEFAULT_REFRESH_CLAIM_LEASE};
use fah_rules::ListManager;
use hickory_proto::op::{Message, Query as WireQuery, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use tokio::net::UdpSocket;

use support::mock_upstream::{Behaviour, Distribution, MockConfig, MockUpstream, Phase};

const ENDPOINTS: usize = 4;
const TIMEOUT_MS: u32 = 800;
const PENALTY_FAILURES: u32 = 2;
const RATE_QPS: u64 = 50;
const TICK_MS: u64 = 20;
const SAMPLE_MS: u64 = 100;
const SCENARIO: Duration = Duration::from_secs(20);
const RECOVERY_DEAD: Duration = Duration::from_secs(15);
const RECOVERY_TOTAL: Duration = Duration::from_secs(60);
const DRAIN: Duration = Duration::from_millis(4_500);
const WINDOW: Duration = Duration::from_secs(5);

const SCENARIOS: [(&str, [bool; ENDPOINTS]); 5] = [
    ("0of4-dead", [false, false, false, false]),
    ("1of4-dead-first", [true, false, false, false]),
    ("1of4-dead-last", [false, false, false, true]),
    ("2of4-dead", [true, true, false, false]),
    ("4of4-dead", [true, true, true, true]),
];

fn healthy_latency() -> Distribution {
    Distribution::LogNormal {
        median: Duration::from_millis(15),
        sigma: 0.3,
    }
}

fn timeout() -> Duration {
    Duration::from_millis(u64::from(TIMEOUT_MS))
}

fn healthy() -> MockConfig {
    MockConfig::answering().with_latency(healthy_latency())
}

fn dead() -> MockConfig {
    MockConfig::black_hole(timeout())
}

fn revives_after(dead_for: Duration) -> MockConfig {
    MockConfig::scripted(vec![
        Phase::new(dead_for, Behaviour::BlackHole),
        Phase::new(Duration::MAX, Behaviour::Answer),
    ])
    .with_dead_hold(timeout())
    .with_latency(healthy_latency())
}

fn constant(dead_flags: [bool; ENDPOINTS]) -> Vec<MockConfig> {
    dead_flags
        .iter()
        .map(|&is_dead| if is_dead { dead() } else { healthy() })
        .collect()
}

fn recovery() -> Vec<MockConfig> {
    vec![
        revives_after(RECOVERY_DEAD),
        healthy(),
        healthy(),
        healthy(),
    ]
}

fn strategy_name(strategy: UpstreamStrategy) -> &'static str {
    match strategy {
        UpstreamStrategy::Adaptive => "adaptive",
    }
}

fn udp_server(addr: SocketAddr) -> UpstreamServerConfig {
    UpstreamServerConfig {
        address: addr.to_string(),
        protocol: UpstreamProtocol::Udp,
        hostname: None,
    }
}

async fn start_server(
    servers: Vec<UpstreamServerConfig>,
    strategy: UpstreamStrategy,
) -> (Server, UpstreamPool, tempfile::TempDir) {
    let data_dir = tempfile::tempdir().unwrap();
    let rules = Arc::new(
        ListManager::new(
            &RulesConfig {
                refresh_hours_default: 24,
                lists: vec![],
            },
            data_dir.path().to_path_buf(),
        )
        .unwrap(),
    );
    let upstreams = DnsUpstreamsConfig {
        strategy,
        timeout_ms: TIMEOUT_MS,
        penalty_failures: PENALTY_FAILURES,
        servers,
    };
    let pool = UpstreamPool::from_config(&upstreams).unwrap();
    let (events, mut drain) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move { while drain.recv().await.is_some() {} });
    let pipeline = Arc::new(Pipeline::new(
        rules,
        pool.clone(),
        10,
        &DnsCacheConfig::default(),
        DEFAULT_REFRESH_CLAIM_LEASE,
        events,
    ));
    let config = DnsConfig {
        listen: DnsListenConfig {
            address: "127.0.0.1".to_string(),
            port: 0,
            dot_enabled: false,
            ..Default::default()
        },
        ..DnsConfig::default()
    };
    let mut server = Server::bind(&config).await.unwrap();
    server.serve(pipeline, None);
    (server, pool, data_dir)
}

fn query_bytes(id: u16, name: &str) -> Vec<u8> {
    let mut message = Message::query();
    message.metadata.id = id;
    message.add_query(WireQuery::query(
        Name::from_ascii(name).unwrap(),
        RecordType::A,
    ));
    message.to_vec().unwrap()
}

struct Reply {
    at: Duration,
    latency: Duration,
    rcode: ResponseCode,
}

struct Sample {
    at: Duration,
    datagrams: [u64; ENDPOINTS],
    answered: [u64; ENDPOINTS],
}

struct Endpoint {
    datagrams: u64,
    answered: u64,
    status: UpstreamStatus,
}

struct Run {
    strategy: UpstreamStrategy,
    sent: u64,
    replies: Vec<Reply>,
    samples: Vec<Sample>,
    endpoints: Vec<Endpoint>,
    revival: Option<Duration>,
}

fn snapshot(mocks: &[MockUpstream], at: Duration) -> Sample {
    let mut datagrams = [0u64; ENDPOINTS];
    let mut answered = [0u64; ENDPOINTS];
    for (index, mock) in mocks.iter().enumerate() {
        datagrams[index] = mock.stats.datagrams();
        answered[index] = mock.stats.answered();
    }
    Sample {
        at,
        datagrams,
        answered,
    }
}

async fn run(
    strategy: UpstreamStrategy,
    configs: Vec<MockConfig>,
    duration: Duration,
    revives_at: Option<Duration>,
    label: &str,
) -> Run {
    let mut mocks = Vec::with_capacity(ENDPOINTS);
    for config in configs {
        mocks.push(MockUpstream::spawn(config).await);
    }
    let mocks = Arc::new(mocks);
    let servers = mocks.iter().map(|mock| udp_server(mock.addr)).collect();
    let (server, pool, _data_dir) = start_server(servers, strategy).await;
    let client = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let target = server.udp_addr();
    let started = Instant::now();
    let revival = revives_at.map(|dead_for| {
        let revive_at = mocks[0].started + dead_for;
        revive_at.saturating_duration_since(started)
    });

    let sends: Arc<Mutex<Vec<Option<Instant>>>> = Arc::new(Mutex::new(vec![None; 65_536]));
    let replies: Arc<Mutex<Vec<Reply>>> = Arc::new(Mutex::new(Vec::new()));
    let receiver = {
        let client = Arc::clone(&client);
        let sends = Arc::clone(&sends);
        let replies = Arc::clone(&replies);
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            loop {
                let Ok(len) = client.recv(&mut buf).await else {
                    return;
                };
                let Ok(reply) = Message::from_vec(&buf[..len]) else {
                    continue;
                };
                let sent_at = sends.lock().unwrap()[usize::from(reply.metadata.id)];
                if let Some(sent_at) = sent_at {
                    replies.lock().unwrap().push(Reply {
                        at: started.elapsed(),
                        latency: sent_at.elapsed(),
                        rcode: reply.metadata.response_code,
                    });
                }
            }
        })
    };

    let stop = Arc::new(AtomicBool::new(false));
    let samples: Arc<Mutex<Vec<Sample>>> = Arc::new(Mutex::new(Vec::new()));
    let sampler = {
        let stop = Arc::clone(&stop);
        let samples = Arc::clone(&samples);
        let mocks = Arc::clone(&mocks);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(SAMPLE_MS));
            while !stop.load(Ordering::Relaxed) {
                ticker.tick().await;
                let sample = snapshot(&mocks, started.elapsed());
                samples.lock().unwrap().push(sample);
            }
        })
    };

    let per_tick = RATE_QPS * TICK_MS / 1000;
    let ticks = duration.as_millis() as u64 / TICK_MS;
    let mut ticker = tokio::time::interval(Duration::from_millis(TICK_MS));
    let mut sent = 0u64;
    for _ in 0..ticks {
        ticker.tick().await;
        for _ in 0..per_tick {
            sent += 1;
            let id = u16::try_from(sent).expect("fewer than 65536 queries per run");
            let name = format!("q{sent}.{label}.ab.example.");
            sends.lock().unwrap()[usize::from(id)] = Some(Instant::now());
            client
                .send_to(&query_bytes(id, &name), target)
                .await
                .unwrap();
        }
    }
    tokio::time::sleep(DRAIN).await;

    stop.store(true, Ordering::Relaxed);
    sampler.await.unwrap();
    receiver.abort();
    let status = pool.status();
    let endpoints = mocks
        .iter()
        .zip(status)
        .map(|(mock, status)| Endpoint {
            datagrams: mock.stats.datagrams(),
            answered: mock.stats.answered(),
            status,
        })
        .collect();
    server.shutdown();

    let replies = std::mem::take(&mut *replies.lock().unwrap());
    let samples = std::mem::take(&mut *samples.lock().unwrap());
    Run {
        strategy,
        sent,
        replies,
        samples,
        endpoints,
        revival,
    }
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * quantile).round() as usize;
    sorted[index]
}

fn latencies_ms(replies: &[Reply]) -> Vec<f64> {
    let mut values: Vec<f64> = replies
        .iter()
        .map(|reply| reply.latency.as_secs_f64() * 1_000.0)
        .collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values
}

fn report(scenario: &str, run: &Run) {
    let sorted = latencies_ms(&run.replies);
    let answered = run.replies.len() as u64;
    let noerror = run
        .replies
        .iter()
        .filter(|reply| reply.rcode == ResponseCode::NoError)
        .count();
    let servfail = run
        .replies
        .iter()
        .filter(|reply| reply.rcode == ResponseCode::ServFail)
        .count();
    let over_1s = sorted.iter().filter(|&&ms| ms >= 1_000.0).count();
    let over_2s = sorted.iter().filter(|&&ms| ms >= 2_000.0).count();
    let paid_tax = sorted
        .iter()
        .filter(|&&ms| ms >= f64::from(TIMEOUT_MS))
        .count();
    let mean = if sorted.is_empty() {
        0.0
    } else {
        sorted.iter().sum::<f64>() / sorted.len() as f64
    };
    println!(
        "AB {scenario} {}: sent={} answered={} unanswered={} noerror={noerror} servfail={servfail} \
         p50={:.1} p95={:.1} p99={:.1} max={:.1} mean={:.1} ms | >=timeout={paid_tax} >=1s={over_1s} >=2s={over_2s}",
        strategy_name(run.strategy),
        run.sent,
        answered,
        run.sent - answered,
        percentile(&sorted, 0.50),
        percentile(&sorted, 0.95),
        percentile(&sorted, 0.99),
        sorted.last().copied().unwrap_or(0.0),
        mean,
    );
    let total_datagrams: u64 = run
        .endpoints
        .iter()
        .map(|endpoint| endpoint.datagrams)
        .sum();
    for (index, endpoint) in run.endpoints.iter().enumerate() {
        let share = if total_datagrams == 0 {
            0.0
        } else {
            endpoint.datagrams as f64 * 100.0 / total_datagrams as f64
        };
        let success = if endpoint.datagrams == 0 {
            0.0
        } else {
            endpoint.answered as f64 * 100.0 / endpoint.datagrams as f64
        };
        println!(
            "   U{}: datagrams={} ({share:.1}%) answered={} success={success:.1}% | pool attempts={} failures={} penalties={} probes={} probe_ok={} state={:?} round={}",
            index + 1,
            endpoint.datagrams,
            endpoint.answered,
            endpoint.status.attempts,
            endpoint.status.failures,
            endpoint.status.penalties,
            endpoint.status.probes,
            endpoint.status.probe_successes,
            endpoint.status.state,
            endpoint.status.penalty_round,
        );
    }
}

fn sample_at(samples: &[Sample], at: Duration) -> Option<&Sample> {
    samples.iter().take_while(|sample| sample.at <= at).last()
}

fn report_recovery(run: &Run) {
    let Some(revival) = run.revival else {
        return;
    };
    let baseline = sample_at(&run.samples, revival).or_else(|| run.samples.first());
    let Some(baseline) = baseline else {
        return;
    };
    let first_datagram = run
        .samples
        .iter()
        .find(|sample| sample.at > revival && sample.datagrams[0] > baseline.datagrams[0])
        .map(|sample| sample.at - revival);
    let first_success = run
        .samples
        .iter()
        .find(|sample| sample.at > revival && sample.answered[0] > baseline.answered[0])
        .map(|sample| sample.at - revival);
    println!(
        "AB recovery {}: U1 revived at {:.1}s; first datagram to U1 after revival +{} s; first answer from U1 after revival +{} s (sampled every {SAMPLE_MS} ms)",
        strategy_name(run.strategy),
        revival.as_secs_f64(),
        first_datagram.map_or("never".to_string(), |d| format!("{:.1}", d.as_secs_f64())),
        first_success.map_or("never".to_string(), |d| format!("{:.1}", d.as_secs_f64())),
    );
    let windows = (RECOVERY_TOTAL.as_secs() / WINDOW.as_secs()) as usize;
    println!("   window        n   p50 ms   p95 ms   U1 share  U1 answered");
    for window in 0..windows {
        let from = WINDOW * window as u32;
        let to = from + WINDOW;
        let in_window: Vec<f64> = {
            let mut values: Vec<f64> = run
                .replies
                .iter()
                .filter(|reply| reply.at >= from && reply.at < to)
                .map(|reply| reply.latency.as_secs_f64() * 1_000.0)
                .collect();
            values.sort_by(|a, b| a.partial_cmp(b).unwrap());
            values
        };
        let zero = Sample {
            at: Duration::ZERO,
            datagrams: [0; ENDPOINTS],
            answered: [0; ENDPOINTS],
        };
        let start = sample_at(&run.samples, from).unwrap_or(&zero);
        let (share, u1_answered) = match sample_at(&run.samples, to) {
            Some(end) => {
                let total: u64 = (0..ENDPOINTS)
                    .map(|index| end.datagrams[index] - start.datagrams[index])
                    .sum();
                let u1 = end.datagrams[0] - start.datagrams[0];
                let share = if total == 0 {
                    0.0
                } else {
                    u1 as f64 * 100.0 / total as f64
                };
                (share, end.answered[0] - start.answered[0])
            }
            None => (0.0, 0),
        };
        println!(
            "   {:>2}-{:<2}s  {:>5}  {:>7.1}  {:>7.1}  {:>7.1}%  {:>5}",
            from.as_secs(),
            to.as_secs(),
            in_window.len(),
            percentile(&in_window, 0.50),
            percentile(&in_window, 0.95),
            share,
            u1_answered,
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn strategy_ab_constant_failures() {
    println!(
        "AB constant failures: {ENDPOINTS} UDP upstreams, timeout_ms={TIMEOUT_MS}, penalty_failures={PENALTY_FAILURES}, {RATE_QPS} qps for {} s per arm, healthy latency lognormal median 15 ms sigma 0.3, dead = black hole",
        SCENARIO.as_secs()
    );
    for (name, dead_flags) in SCENARIOS {
        for strategy in [UpstreamStrategy::Adaptive] {
            let label = format!("{name}-{}", strategy_name(strategy));
            let outcome = run(strategy, constant(dead_flags), SCENARIO, None, &label).await;
            report(name, &outcome);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn strategy_ab_recovery() {
    println!(
        "AB recovery: U1 black-holed for {} s then answering, U2-U4 healthy, {RATE_QPS} qps for {} s per arm, timeout_ms={TIMEOUT_MS}, penalty_failures={PENALTY_FAILURES}",
        RECOVERY_DEAD.as_secs(),
        RECOVERY_TOTAL.as_secs()
    );
    for strategy in [UpstreamStrategy::Adaptive] {
        let label = format!("recovery-{}", strategy_name(strategy));
        let outcome = run(
            strategy,
            recovery(),
            RECOVERY_TOTAL,
            Some(RECOVERY_DEAD),
            &label,
        )
        .await;
        report("recovery", &outcome);
        report_recovery(&outcome);
    }
}
