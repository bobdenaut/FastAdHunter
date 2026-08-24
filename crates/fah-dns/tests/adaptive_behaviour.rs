mod support;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fah_config::{
    DnsCacheConfig, DnsUpstreamsConfig, RulesConfig, UpstreamProtocol, UpstreamServerConfig,
    UpstreamStrategy,
};
use fah_dns::{
    Forwarder, Pipeline, Policy, Transport, UpstreamPool, UpstreamStatus, ATTEMPT_LEGS,
    DEFAULT_REFRESH_CLAIM_LEASE,
};
use fah_model::UpstreamState;
use fah_rules::ListManager;
use hickory_proto::op::{Message, Query as WireQuery, ResponseCode};
use hickory_proto::rr::{Name, RecordType};

use support::mock_upstream::{
    closed_tcp_addr, Behaviour, MockConfig, MockStats, MockUpstream, Phase,
};

const TIMEOUT_MS: u32 = 50;
const PENALTY_BASE_MS: u64 = 1_500;
const MAX_RATIO_LOW: u64 = 3_750;
const MAX_RATIO_SHIPPED: u64 = 18_750;
const MAX_RATIO_HIGH: u64 = 56_250;
const ENDPOINTS: usize = 2;
const CADENCE_MS: u64 = 50;
const TAIL_UNITS: f64 = 1.5;
const REFRESH_FAILURE_COOLDOWN_SECONDS: u64 = 30;

fn attempt_bound_ms() -> u64 {
    u64::from(ATTEMPT_LEGS) * u64::from(TIMEOUT_MS)
}

fn leg() -> Duration {
    Duration::from_millis(u64::from(TIMEOUT_MS))
}

fn nominal_penalty_ms(round: u8, base: u64, max: u64) -> u64 {
    let mut nominal = base;
    for _ in 1..round.max(1) {
        if nominal >= max {
            break;
        }
        nominal = nominal.saturating_mul(2);
    }
    nominal.min(max)
}

fn udp_config(addr: SocketAddr) -> UpstreamServerConfig {
    UpstreamServerConfig {
        address: addr.to_string(),
        protocol: UpstreamProtocol::Udp,
        hostname: None,
    }
}

fn dot_config(addr: SocketAddr) -> UpstreamServerConfig {
    UpstreamServerConfig {
        address: addr.to_string(),
        protocol: UpstreamProtocol::Dot,
        hostname: Some("closed.example.invalid".to_string()),
    }
}

fn build_pool(
    servers: Vec<UpstreamServerConfig>,
    strategy: UpstreamStrategy,
    penalty_failures: u32,
    penalty_max_ms: u64,
) -> UpstreamPool {
    let config = DnsUpstreamsConfig {
        strategy,
        timeout_ms: TIMEOUT_MS,
        penalty_failures,
        servers,
    };
    UpstreamPool::with_policy(
        &config,
        Policy {
            penalty_failures: u8::try_from(penalty_failures).unwrap(),
            penalty_base_ms: PENALTY_BASE_MS,
            penalty_max_ms,
        },
    )
    .unwrap()
}

fn strategy_name(strategy: UpstreamStrategy) -> &'static str {
    match strategy {
        UpstreamStrategy::Adaptive => "adaptive",
        UpstreamStrategy::Fallback => "fallback",
    }
}

fn query_for(name: &str) -> Message {
    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_ascii(name).unwrap(),
        RecordType::A,
    ));
    message.metadata.recursion_desired = true;
    message
}

fn dead_mock() -> MockConfig {
    MockConfig::black_hole(leg())
}

fn serial_requested() -> bool {
    if std::env::var("RUST_TEST_THREADS").is_ok_and(|value| value.trim() == "1") {
        return true;
    }
    let mut args = std::env::args();
    while let Some(argument) = args.next() {
        if let Some(value) = argument.strip_prefix("--test-threads=") {
            return value.trim() == "1";
        }
        if argument == "--test-threads" {
            return args.next().is_some_and(|value| value.trim() == "1");
        }
    }
    std::thread::available_parallelism().is_ok_and(|count| count.get() == 1)
}

fn require_serial(arm: &str) {
    assert!(
        serial_requested(),
        "{arm}: every latency figure in this arm assumes it runs alone; \
         re-run with `-- --include-ignored --test-threads=1` (or RUST_TEST_THREADS=1)"
    );
}

fn tolerance(queries: usize) -> u64 {
    ((queries as f64) * 0.001).max(1.0) as u64
}

fn assert_answered(label: &str, report: &Metrics) {
    let unanswered = report.queries - report.answered;
    let allowed = tolerance(report.queries) as usize;
    if unanswered > 0 {
        println!(
            "G3 unanswered {label} unanswered={unanswered} of {} allowed={allowed}",
            report.queries
        );
    }
    assert!(
        unanswered <= allowed,
        "{label}: {unanswered} of {} queries went unanswered, at most {allowed} tolerated",
        report.queries
    );
}

fn assert_one_probe_per_query(label: &str, samples: &[Sample]) {
    for (index, sample) in samples.iter().enumerate() {
        assert!(
            sample.probes_total() <= 1,
            "{label}: query {index} carried {} probes, at most one is allowed",
            sample.probes_total()
        );
    }
}

#[derive(Clone, Copy)]
struct Sample {
    at: Duration,
    latency: Duration,
    ok: bool,
    attempts: [u64; ENDPOINTS],
    probes: [u64; ENDPOINTS],
    probe_successes: [u64; ENDPOINTS],
    penalties: [u64; ENDPOINTS],
    failures: [u64; ENDPOINTS],
    datagrams: [u64; ENDPOINTS],
    round: [u8; ENDPOINTS],
    state: [UpstreamState; ENDPOINTS],
    consecutive_failures: [u64; ENDPOINTS],
}

impl Sample {
    fn probes_total(&self) -> u64 {
        self.probes.iter().sum()
    }

    fn penalties_total(&self) -> u64 {
        self.penalties.iter().sum()
    }

    fn datagrams_total(&self) -> u64 {
        self.datagrams.iter().sum()
    }

    fn latency_ms(&self) -> f64 {
        self.latency.as_secs_f64() * 1_000.0
    }

    fn paid(&self) -> bool {
        self.latency >= leg()
    }
}

struct Runner<'a> {
    pool: &'a UpstreamPool,
    mocks: [Arc<MockStats>; ENDPOINTS],
    previous: Vec<UpstreamStatus>,
    seen: [u64; ENDPOINTS],
    started: Instant,
}

impl<'a> Runner<'a> {
    fn new(pool: &'a UpstreamPool, mocks: [Arc<MockStats>; ENDPOINTS]) -> Self {
        let previous = pool.status();
        let seen = [mocks[0].datagrams(), mocks[1].datagrams()];
        Self {
            pool,
            mocks,
            previous,
            seen,
            started: Instant::now(),
        }
    }

    async fn query(&mut self, name: &str) -> Sample {
        let request = query_for(name);
        let at = self.started.elapsed();
        let begin = Instant::now();
        let ok = self.pool.forward(&request).await.is_ok();
        let latency = begin.elapsed();
        self.close(at, latency, ok)
    }

    async fn resolve(&mut self, host: &str) -> Sample {
        let at = self.started.elapsed();
        let begin = Instant::now();
        let ok = self.pool.resolve_host(host).await.is_ok();
        let latency = begin.elapsed();
        self.close(at, latency, ok)
    }

    fn close(&mut self, at: Duration, latency: Duration, ok: bool) -> Sample {
        let current = self.pool.status();
        let mut sample = Sample {
            at,
            latency,
            ok,
            attempts: [0; ENDPOINTS],
            probes: [0; ENDPOINTS],
            probe_successes: [0; ENDPOINTS],
            penalties: [0; ENDPOINTS],
            failures: [0; ENDPOINTS],
            datagrams: [0; ENDPOINTS],
            round: [0; ENDPOINTS],
            state: [UpstreamState::Healthy; ENDPOINTS],
            consecutive_failures: [0; ENDPOINTS],
        };
        for (endpoint, now) in current.iter().enumerate().take(ENDPOINTS) {
            let was = &self.previous[endpoint];
            sample.attempts[endpoint] = now.attempts - was.attempts;
            sample.probes[endpoint] = now.probes - was.probes;
            sample.probe_successes[endpoint] = now.probe_successes - was.probe_successes;
            sample.penalties[endpoint] = now.penalties - was.penalties;
            sample.failures[endpoint] = now.failures - was.failures;
            sample.round[endpoint] = now.penalty_round;
            sample.state[endpoint] = now.state;
            sample.consecutive_failures[endpoint] = now.consecutive_failures;
            let datagrams = self.mocks[endpoint].datagrams();
            sample.datagrams[endpoint] = datagrams - self.seen[endpoint];
            self.seen[endpoint] = datagrams;
        }
        self.previous = current;
        sample
    }
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (quantile * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn sorted_ms(samples: &[Sample]) -> Vec<f64> {
    let mut values: Vec<f64> = samples.iter().map(Sample::latency_ms).collect();
    values.sort_by(f64::total_cmp);
    values
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn low(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().copied().fold(f64::INFINITY, f64::min)
    }
}

fn high(values: &[f64]) -> f64 {
    values.iter().copied().fold(0.0, f64::max)
}

struct Metrics {
    label: String,
    queries: usize,
    answered: usize,
    p50_ms: f64,
    p99_ms: f64,
    first_ten_ms: Vec<f64>,
    non_probe_paying_ms: Vec<f64>,
    non_probe_first_ms: Vec<f64>,
    stalled_ms: Vec<f64>,
    probe_paying_ms: Vec<f64>,
    healthy_p50_ms: f64,
    healthy_n: usize,
    first_penalty_index: Option<usize>,
    time_to_penalize: Option<Duration>,
    tax_free_share: f64,
    queries_after_penalty: usize,
    queries_without_packet: usize,
    attempts: [u64; ENDPOINTS],
    probes: [u64; ENDPOINTS],
    probe_successes: [u64; ENDPOINTS],
    penalties: [u64; ENDPOINTS],
    failures: [u64; ENDPOINTS],
    datagrams: [u64; ENDPOINTS],
    attempts_per_query: f64,
    state: [UpstreamState; ENDPOINTS],
    round: [u8; ENDPOINTS],
    consecutive_failures: [u64; ENDPOINTS],
}

impl Metrics {
    fn paying(&self) -> usize {
        self.non_probe_paying_ms.len() + self.probe_paying_ms.len()
    }

    fn non_probe_paying(&self) -> usize {
        self.non_probe_paying_ms.len()
    }

    fn non_probe_paying_first(&self) -> usize {
        self.non_probe_first_ms.len()
    }

    fn stalled(&self) -> usize {
        self.stalled_ms.len()
    }

    fn probe_paying(&self) -> usize {
        self.probe_paying_ms.len()
    }

    fn tax_ms(&self, values: &[f64]) -> f64 {
        Self::tax_against(self.healthy_p50_ms, values)
    }

    fn tax_against(baseline: f64, values: &[f64]) -> f64 {
        values.iter().map(|value| (value - baseline).max(0.0)).sum()
    }

    fn non_probe_first_tax_against(&self, baseline: f64) -> f64 {
        Self::tax_against(baseline, &self.non_probe_first_ms)
    }

    fn probe_tax_against(&self, baseline: f64) -> f64 {
        Self::tax_against(baseline, &self.probe_paying_ms)
    }

    fn non_probe_tax_ms(&self) -> f64 {
        self.tax_ms(&self.non_probe_paying_ms)
    }

    fn probe_tax_ms(&self) -> f64 {
        self.tax_ms(&self.probe_paying_ms)
    }

    fn tax_field(&self, tax_ms: f64) -> String {
        if self.healthy_n == 0 {
            "n/a".to_string()
        } else {
            format!("{tax_ms:.0}")
        }
    }

    fn all_paying_ms(&self) -> Vec<f64> {
        let mut all = self.non_probe_paying_ms.clone();
        all.extend_from_slice(&self.probe_paying_ms);
        all
    }

    fn report(&self) {
        let first: Vec<String> = self
            .first_ten_ms
            .iter()
            .map(|value| format!("{value:.1}"))
            .collect();
        let paying = self.all_paying_ms();
        println!(
            "G3 {} queries={} answered={} p50_ms={:.2} p99_ms={:.2} healthy_p50_ms={:.2} \
             healthy_n={} \
             paying={} non_probe_paying={} non_probe_paying_first={} stalled={} \
             probe_paying={} paying_mean_ms={:.2} \
             paying_min_ms={:.2} paying_max_ms={:.2} non_probe_tax_ms={} probe_tax_ms={} \
             attempts={:?} attempts_per_query={:.4} probes={:?} probe_successes={:?} \
             penalties={:?} failures={:?} datagrams={:?} state={:?} round={:?} \
             consecutive_failures={:?} first_penalty_index={:?} time_to_penalize_ms={:?} \
             queries_after_penalty={} \
             tax_free_share={:.5} queries_without_packet={} first_ten_ms=[{}]",
            self.label,
            self.queries,
            self.answered,
            self.p50_ms,
            self.p99_ms,
            self.healthy_p50_ms,
            self.healthy_n,
            self.paying(),
            self.non_probe_paying(),
            self.non_probe_paying_first(),
            self.stalled(),
            self.probe_paying(),
            mean(&paying),
            low(&paying),
            high(&paying),
            self.tax_field(self.non_probe_tax_ms()),
            self.tax_field(self.probe_tax_ms()),
            self.attempts,
            self.attempts_per_query,
            self.probes,
            self.probe_successes,
            self.penalties,
            self.failures,
            self.datagrams,
            self.state,
            self.round,
            self.consecutive_failures,
            self.first_penalty_index,
            self.time_to_penalize.map(|at| at.as_millis()),
            self.queries_after_penalty,
            self.tax_free_share,
            self.queries_without_packet,
            first.join(",")
        );
    }
}

fn metrics(label: &str, samples: &[Sample]) -> Metrics {
    let sorted = sorted_ms(samples);
    let mut healthy: Vec<f64> = samples
        .iter()
        .filter(|sample| !sample.paid())
        .map(Sample::latency_ms)
        .collect();
    healthy.sort_by(f64::total_cmp);
    let first_penalty_index = samples
        .iter()
        .position(|sample| sample.penalties_total() > 0);
    let after = match first_penalty_index {
        Some(index) => samples.len().saturating_sub(index + 1),
        None => samples.len(),
    };
    let tax_free = match first_penalty_index {
        Some(index) => samples[index + 1..]
            .iter()
            .filter(|sample| !sample.paid())
            .count(),
        None => samples.iter().filter(|sample| !sample.paid()).count(),
    };
    let mut totals = Metrics {
        label: label.to_string(),
        queries: samples.len(),
        answered: samples.iter().filter(|sample| sample.ok).count(),
        p50_ms: percentile(&sorted, 0.50),
        p99_ms: percentile(&sorted, 0.99),
        first_ten_ms: samples.iter().take(10).map(Sample::latency_ms).collect(),
        non_probe_paying_ms: samples
            .iter()
            .filter(|sample| sample.paid() && sample.probes_total() == 0)
            .map(Sample::latency_ms)
            .collect(),
        non_probe_first_ms: samples
            .iter()
            .filter(|sample| sample.paid() && sample.probes_total() == 0 && sample.attempts[0] > 0)
            .map(Sample::latency_ms)
            .collect(),
        stalled_ms: samples
            .iter()
            .filter(|sample| sample.paid() && sample.probes_total() == 0 && sample.attempts[0] == 0)
            .map(Sample::latency_ms)
            .collect(),
        probe_paying_ms: samples
            .iter()
            .filter(|sample| sample.paid() && sample.probes_total() > 0)
            .map(Sample::latency_ms)
            .collect(),
        healthy_p50_ms: percentile(&healthy, 0.50),
        healthy_n: healthy.len(),
        first_penalty_index,
        time_to_penalize: first_penalty_index
            .map(|index| samples[index].at + samples[index].latency),
        tax_free_share: if after == 0 {
            0.0
        } else {
            tax_free as f64 / after as f64
        },
        queries_after_penalty: after,
        queries_without_packet: samples
            .iter()
            .filter(|sample| sample.datagrams_total() == 0)
            .count(),
        attempts: [0; ENDPOINTS],
        probes: [0; ENDPOINTS],
        probe_successes: [0; ENDPOINTS],
        penalties: [0; ENDPOINTS],
        failures: [0; ENDPOINTS],
        datagrams: [0; ENDPOINTS],
        attempts_per_query: 0.0,
        state: [UpstreamState::Healthy; ENDPOINTS],
        round: [0; ENDPOINTS],
        consecutive_failures: [0; ENDPOINTS],
    };
    for sample in samples {
        for endpoint in 0..ENDPOINTS {
            totals.attempts[endpoint] += sample.attempts[endpoint];
            totals.probes[endpoint] += sample.probes[endpoint];
            totals.probe_successes[endpoint] += sample.probe_successes[endpoint];
            totals.penalties[endpoint] += sample.penalties[endpoint];
            totals.failures[endpoint] += sample.failures[endpoint];
            totals.datagrams[endpoint] += sample.datagrams[endpoint];
        }
    }
    if let Some(last) = samples.last() {
        totals.state = last.state;
        totals.round = last.round;
        totals.consecutive_failures = last.consecutive_failures;
    }
    let attempts: u64 = totals.attempts.iter().sum();
    totals.attempts_per_query = if samples.is_empty() {
        0.0
    } else {
        attempts as f64 / samples.len() as f64
    };
    totals
}

fn net_avoided(label: &str, fallback_paying: u64, adaptive_paying: u64, failed_probes: u64) {
    let bound = i64::try_from(attempt_bound_ms()).unwrap();
    let gross =
        (i64::try_from(fallback_paying).unwrap() - i64::try_from(adaptive_paying).unwrap()) * bound;
    let cost = i64::try_from(failed_probes).unwrap() * bound;
    println!(
        "G3 net-cost {label} fallback_paying={fallback_paying} adaptive_paying={adaptive_paying} \
         failed_probes={failed_probes} attempt_bound_ms={bound} gross_ms={gross} \
         probe_cost_ms={cost} net_avoided_ms={}",
        gross - cost
    );
}

fn net_avoided_measured(label: &str, baseline_ms: f64, fallback: &Metrics, adaptive: &Metrics) {
    let fallback_tax_ms = fallback.non_probe_first_tax_against(baseline_ms);
    let adaptive_tax_ms = adaptive.non_probe_first_tax_against(baseline_ms);
    let probe_tax_ms = adaptive.probe_tax_against(baseline_ms);
    println!(
        "G3 net-cost-measured {label} shared_baseline_ms={baseline_ms:.2} \
         fallback_paying={} adaptive_non_probe_paying={} failed_probes={} \
         fallback_tax_ms={fallback_tax_ms:.0} adaptive_non_probe_tax_ms={adaptive_tax_ms:.0} \
         probe_tax_ms={probe_tax_ms:.0} net_avoided_ms={:.0}",
        fallback.non_probe_paying_first(),
        adaptive.non_probe_paying_first(),
        adaptive.probe_paying(),
        fallback_tax_ms - adaptive_tax_ms - probe_tax_ms
    );
}

async fn sequential(runner: &mut Runner<'_>, prefix: &str, count: usize) -> Vec<Sample> {
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        samples.push(runner.query(&format!("{prefix}{index}.g3.test.")).await);
    }
    samples
}

async fn sequential_paced(
    runner: &mut Runner<'_>,
    prefix: &str,
    count: usize,
    cadence: Duration,
) -> Vec<Sample> {
    let mut ticker = tokio::time::interval(cadence);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        ticker.tick().await;
        samples.push(runner.query(&format!("{prefix}{index}.g3.test.")).await);
    }
    samples
}

async fn paced_until(
    runner: &mut Runner<'_>,
    prefix: &str,
    cadence: Duration,
    until: Duration,
) -> Vec<Sample> {
    let mut ticker = tokio::time::interval(cadence);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let expected = (until.as_secs_f64() / cadence.as_secs_f64()) as usize;
    let mut samples = Vec::with_capacity(expected);
    let started = Instant::now();
    let mut index = 0usize;
    while started.elapsed() < until {
        ticker.tick().await;
        samples.push(runner.query(&format!("{prefix}{index}.g3.test.")).await);
        index += 1;
    }
    samples
}

async fn run_b1(
    strategy: UpstreamStrategy,
    penalty_failures: u32,
    penalty_max_ms: u64,
    count: usize,
    ratio: &str,
    cadence: Duration,
) -> Metrics {
    let dead = MockUpstream::spawn(dead_mock()).await;
    let live = MockUpstream::spawn(MockConfig::answering()).await;
    let pool = build_pool(
        vec![udp_config(dead.addr), udp_config(live.addr)],
        strategy,
        penalty_failures,
        penalty_max_ms,
    );
    let mut runner = Runner::new(&pool, [Arc::clone(&dead.stats), Arc::clone(&live.stats)]);
    let started = Instant::now();
    let samples = if cadence.is_zero() {
        sequential(&mut runner, "b1-", count).await
    } else {
        sequential_paced(&mut runner, "b1-", count, cadence).await
    };
    let elapsed = started.elapsed();
    let label = format!(
        "B.1 strategy={} penalty_failures={penalty_failures} ratio={ratio} cadence_ms={} elapsed_ms={}",
        strategy_name(strategy),
        cadence.as_millis(),
        elapsed.as_millis()
    );
    let report = metrics(&label, &samples);
    report.report();

    assert_answered(&label, &report);
    if strategy == UpstreamStrategy::Adaptive {
        assert!(
            report.non_probe_paying_first() <= penalty_failures as usize,
            "{label}: {} non-probe queries paid at the dead endpoint, at most {penalty_failures} allowed",
            report.non_probe_paying_first()
        );
        assert_eq!(
            report.probe_paying() as u64,
            report.probes[0],
            "{label}: each probe against a black hole pays exactly once"
        );
        assert_eq!(
            report.probe_successes[0], 0,
            "{label}: no probe can succeed against a black hole"
        );
        assert_eq!(
            report.penalties[1], 0,
            "{label}: the healthy endpoint must never be penalized"
        );
        assert!(
            report.p50_ms < f64::from(TIMEOUT_MS),
            "{label}: the median query must answer at the healthy endpoint's RTT"
        );
        assert_eq!(
            report.probes[0] + 1,
            report.penalties[0],
            "{label}: exactly one probe must be claimed per penalty window"
        );
        assert_one_probe_per_query(&label, &samples);
    } else {
        assert_eq!(
            report.non_probe_paying_first(),
            report.queries,
            "{label}: every fallback query pays one leg at the dead endpoint"
        );
    }
    report
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn b1_black_hole() {
    require_serial("B.1");
    let count = 10_000;
    let mut shipped = Vec::new();
    let cadence = Duration::from_millis(5);
    for penalty_failures in [1, 2, 3] {
        shipped.push(
            run_b1(
                UpstreamStrategy::Adaptive,
                penalty_failures,
                MAX_RATIO_SHIPPED,
                count,
                "12.5",
                cadence,
            )
            .await,
        );
    }
    for (ratio, penalty_max_ms) in [("2.5", MAX_RATIO_LOW), ("37.5", MAX_RATIO_HIGH)] {
        let report = run_b1(
            UpstreamStrategy::Adaptive,
            2,
            penalty_max_ms,
            count,
            ratio,
            cadence,
        )
        .await;
        println!(
            "G3 tax-free B.1 ratio={ratio} penalties={} probes={} tax_free_share={:.5}",
            report.penalties[0], report.probes[0], report.tax_free_share
        );
    }
    let fallback = run_b1(
        UpstreamStrategy::Fallback,
        2,
        MAX_RATIO_SHIPPED,
        count,
        "12.5",
        Duration::ZERO,
    )
    .await;

    let adaptive = &shipped[1];
    println!(
        "G3 tax-free B.1 ratio=12.5 penalties={} probes={} tax_free_share={:.5}",
        adaptive.penalties[0], adaptive.probes[0], adaptive.tax_free_share
    );
    net_avoided(
        "client-forwards B.1 ratio=12.5 penalty_failures=2",
        fallback.non_probe_paying_first() as u64,
        adaptive.non_probe_paying_first() as u64,
        adaptive.probe_paying() as u64,
    );
    net_avoided_measured(
        "client-forwards B.1 ratio=12.5 penalty_failures=2",
        adaptive.healthy_p50_ms,
        &fallback,
        adaptive,
    );
}

async fn run_b2_dot(strategy: UpstreamStrategy, penalty_failures: u32) -> Metrics {
    let refused = closed_tcp_addr().await;
    let live = MockUpstream::spawn(MockConfig::answering()).await;
    let pool = build_pool(
        vec![dot_config(refused), udp_config(live.addr)],
        strategy,
        penalty_failures,
        MAX_RATIO_SHIPPED,
    );
    let empty = Arc::new(MockStats::default());
    let mut runner = Runner::new(&pool, [empty, Arc::clone(&live.stats)]);
    let samples = sequential(&mut runner, "b2dot-", 20).await;
    let label = format!(
        "B.2-dot strategy={} penalty_failures={penalty_failures}",
        strategy_name(strategy)
    );
    let report = metrics(&label, &samples);
    report.report();
    assert_answered(&label, &report);
    if strategy == UpstreamStrategy::Adaptive {
        let classification = if report.attempts[0] == 1 {
            "hard_failure_on_first_attempt"
        } else {
            "soft_failure_needed_the_threshold"
        };
        println!(
            "G3 B.2-dot-criterion penalty_failures={penalty_failures} \
             penalized_after_attempts={} classification={classification} \
             paid_at_refused={} stalled={} \
             penalized_after_one_attempt={} at_most_one_paid_at_refused={}",
            report.attempts[0],
            report.non_probe_paying_first(),
            report.stalled(),
            report.attempts[0] == 1,
            report.non_probe_paying_first() <= 1
        );
        assert!(
            report.attempts[0] >= 1 && report.attempts[0] <= u64::from(penalty_failures),
            "{label}: the endpoint must be Penalized and skipped within its threshold, \
             {} attempts reached it",
            report.attempts[0]
        );
        assert!(
            report.non_probe_paying_first() <= penalty_failures as usize,
            "{label}: {} queries paid at the refused endpoint, at most {penalty_failures} allowed",
            report.non_probe_paying_first()
        );
        assert_eq!(
            report.state[0],
            UpstreamState::Penalized,
            "{label}: the refused endpoint must end Penalized"
        );
    }
    report
}

async fn b2_refusal_probe() {
    let refused = closed_tcp_addr().await;
    let started = Instant::now();
    let raw = tokio::net::TcpStream::connect(refused).await.unwrap_err();
    println!(
        "G3 B.2-probe raw_tcp_kind={:?} raw_tcp_elapsed_ms={:.2}",
        raw.kind(),
        started.elapsed().as_secs_f64() * 1_000.0
    );

    let pool = build_pool(
        vec![dot_config(refused)],
        UpstreamStrategy::Adaptive,
        2,
        MAX_RATIO_SHIPPED,
    );
    let started = Instant::now();
    let err = pool
        .forward(&query_for("probe.g3.test."))
        .await
        .expect_err("a closed TCP port cannot answer");
    println!(
        "G3 B.2-probe dot_kind={:?} dot_elapsed_ms={:.2} dot_error={err}",
        err.kind(),
        started.elapsed().as_secs_f64() * 1_000.0
    );

    let closed_udp = MockUpstream::spawn(MockConfig::refused()).await;
    let pool = build_pool(
        vec![udp_config(closed_udp.addr)],
        UpstreamStrategy::Adaptive,
        2,
        MAX_RATIO_SHIPPED,
    );
    let started = Instant::now();
    let result = pool.forward(&query_for("probe.g3.test.")).await;
    println!(
        "G3 B.2-probe os={} udp_kind={:?} udp_elapsed_ms={:.2}",
        std::env::consts::OS,
        result.as_ref().err().map(std::io::Error::kind),
        started.elapsed().as_secs_f64() * 1_000.0
    );
}

async fn b2_refused_endpoint_dot() {
    for penalty_failures in [1, 2, 3] {
        run_b2_dot(UpstreamStrategy::Fallback, penalty_failures).await;
        run_b2_dot(UpstreamStrategy::Adaptive, penalty_failures).await;
    }
}

#[cfg(target_os = "linux")]
async fn run_b2_udp(strategy: UpstreamStrategy, penalty_failures: u32) -> Metrics {
    let refused = MockUpstream::spawn(MockConfig::refused()).await;
    let live = MockUpstream::spawn(MockConfig::answering()).await;
    let pool = build_pool(
        vec![udp_config(refused.addr), udp_config(live.addr)],
        strategy,
        penalty_failures,
        MAX_RATIO_SHIPPED,
    );
    let mut runner = Runner::new(&pool, [Arc::clone(&refused.stats), Arc::clone(&live.stats)]);
    let samples = sequential(&mut runner, "b2udp-", 20).await;
    let label = format!(
        "B.2-udp strategy={} penalty_failures={penalty_failures}",
        strategy_name(strategy)
    );
    let report = metrics(&label, &samples);
    report.report();
    assert_answered(&label, &report);
    if strategy == UpstreamStrategy::Adaptive {
        assert_eq!(
            report.first_penalty_index,
            Some(0),
            "{label}: ECONNREFUSED penalizes on the first attempt"
        );
        assert!(
            report.non_probe_paying_first() <= 1,
            "{label}: {} queries paid at the refused endpoint, at most one may, stalled={}",
            report.non_probe_paying_first(),
            report.stalled()
        );
    }
    report
}

#[cfg(target_os = "linux")]
async fn b2_refused_endpoint_udp() {
    for penalty_failures in [1, 2, 3] {
        run_b2_udp(UpstreamStrategy::Fallback, penalty_failures).await;
        run_b2_udp(UpstreamStrategy::Adaptive, penalty_failures).await;
    }
}

async fn run_b3(strategy: UpstreamStrategy, count: usize) -> Metrics {
    let first = MockUpstream::spawn(MockConfig::answering()).await;
    let second = MockUpstream::spawn(MockConfig::answering()).await;
    let pool = build_pool(
        vec![udp_config(first.addr), udp_config(second.addr)],
        strategy,
        2,
        MAX_RATIO_SHIPPED,
    );
    let mut runner = Runner::new(&pool, [Arc::clone(&first.stats), Arc::clone(&second.stats)]);
    let samples = sequential(&mut runner, "b3-", count).await;
    let label = format!("B.3 strategy={}", strategy_name(strategy));
    let report = metrics(&label, &samples);
    report.report();
    assert_answered(&label, &report);
    assert_eq!(
        report.penalties,
        [0, 0],
        "{label}: a healthy link never penalizes"
    );
    assert_eq!(
        report.probes,
        [0, 0],
        "{label}: a healthy link never probes"
    );
    assert_eq!(
        report.state,
        [UpstreamState::Healthy; ENDPOINTS],
        "{label}: both endpoints stay Healthy"
    );
    report
}

async fn b3_healthy_control() {
    let count = 400;
    let fallback = run_b3(UpstreamStrategy::Fallback, count).await;
    let adaptive = run_b3(UpstreamStrategy::Adaptive, count).await;
    let allowed = tolerance(count);
    let attempt_gap = fallback.attempts[0].abs_diff(adaptive.attempts[0])
        + fallback.attempts[1].abs_diff(adaptive.attempts[1]);
    let datagram_gap = fallback.datagrams[0].abs_diff(adaptive.datagrams[0])
        + fallback.datagrams[1].abs_diff(adaptive.datagrams[1]);
    println!(
        "G3 B.3-control fallback_attempts={:?} adaptive_attempts={:?} attempt_gap={attempt_gap} \
         fallback_datagrams={:?} adaptive_datagrams={:?} datagram_gap={datagram_gap} \
         allowed={allowed}",
        fallback.attempts, adaptive.attempts, fallback.datagrams, adaptive.datagrams
    );
    assert!(
        attempt_gap <= allowed,
        "B.3: attempts differ by {attempt_gap} between strategies on a healthy link"
    );
    assert!(
        datagram_gap <= allowed,
        "B.3: packets on the wire differ by {datagram_gap} between strategies"
    );
}

async fn run_b4(strategy: UpstreamStrategy, minimum: usize) -> Metrics {
    let first = MockUpstream::spawn(dead_mock()).await;
    let second = MockUpstream::spawn(dead_mock()).await;
    let pool = build_pool(
        vec![udp_config(first.addr), udp_config(second.addr)],
        strategy,
        2,
        MAX_RATIO_SHIPPED,
    );
    let mut runner = Runner::new(&pool, [Arc::clone(&first.stats), Arc::clone(&second.stats)]);
    let adaptive = strategy == UpstreamStrategy::Adaptive;
    let cap = Duration::from_millis(PENALTY_BASE_MS * 4);
    let started = Instant::now();
    let mut samples: Vec<Sample> = Vec::with_capacity(minimum);
    let mut probed = [0u64; ENDPOINTS];
    let mut index = 0usize;
    loop {
        let sample = runner.query(&format!("b4-{index}.g3.test.")).await;
        for (total, claimed) in probed.iter_mut().zip(sample.probes.iter()) {
            *total += *claimed;
        }
        samples.push(sample);
        index += 1;
        let both_probed = probed.iter().all(|count| *count > 0);
        if index >= minimum && (!adaptive || both_probed || started.elapsed() >= cap) {
            break;
        }
    }
    let elapsed = started.elapsed();
    let label = format!("B.4 strategy={}", strategy_name(strategy));
    let report = metrics(&label, &samples);
    report.report();

    assert_eq!(
        report.queries_without_packet, 0,
        "{label}: the S1.3 invariant forbids a query that sends no packet"
    );
    assert_eq!(report.answered, 0, "{label}: both endpoints are dead");
    let attempts: u64 = report.attempts.iter().sum();
    let datagrams: u64 = report.datagrams.iter().sum();
    assert_eq!(
        attempts, datagrams,
        "{label}: every counted attempt must be a datagram the mock saw"
    );

    if strategy == UpstreamStrategy::Adaptive {
        let mut round = [0u8; ENDPOINTS];
        let mut state = [UpstreamState::Healthy; ENDPOINTS];
        let mut forced_on_penalized = 0usize;
        for (index, sample) in samples.iter().enumerate() {
            for endpoint in 0..ENDPOINTS {
                if state[endpoint] == UpstreamState::Penalized && sample.probes[endpoint] == 0 {
                    forced_on_penalized += 1;
                    assert_eq!(
                        sample.penalties[endpoint], 0,
                        "{label}: query {index} forced Penalized endpoint {endpoint} and re-penalized it"
                    );
                    assert_eq!(
                        sample.round[endpoint], round[endpoint],
                        "{label}: query {index} forced endpoint {endpoint} and moved penalty_round"
                    );
                }
                round[endpoint] = sample.round[endpoint];
                state[endpoint] = sample.state[endpoint];
            }
        }
        println!(
            "G3 B.4 forced_on_penalized_words={forced_on_penalized} queries={} \
             elapsed_ms={} probe_deadline_cap_ms={}",
            samples.len(),
            elapsed.as_millis(),
            cap.as_millis()
        );
        assert!(
            report.probes[0] > 0 && report.probes[1] > 0,
            "{label}: both endpoints must be probed within {}, probes={:?} after {} queries",
            cap.as_millis(),
            report.probes,
            samples.len()
        );
    }
    report
}

async fn b4_all_dead() {
    run_b4(UpstreamStrategy::Fallback, 40).await;
    run_b4(UpstreamStrategy::Adaptive, 40).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn g3_default_gate_arms() {
    b2_refusal_probe().await;
    b2_refused_endpoint_dot().await;
    #[cfg(target_os = "linux")]
    b2_refused_endpoint_udp().await;
    b3_healthy_control().await;
    b4_all_dead().await;
    b6_rcode_isolation().await;
}

struct RecoveryScript {
    phases: Vec<(&'static str, Duration, Behaviour)>,
}

impl RecoveryScript {
    fn build(penalty_max_ms: u64, tail_units: f64) -> Self {
        let unit = |units: f64| Duration::from_secs_f64(penalty_max_ms as f64 * units / 1_000.0);
        let mut phases = vec![
            ("healthy_pre", unit(0.4), Behaviour::Answer),
            ("black_hole", unit(2.0), Behaviour::BlackHole),
            ("healthy_mid", unit(0.6), Behaviour::Answer),
        ];
        let down = ["flap_down_1", "flap_down_2", "flap_down_3"];
        let up = ["flap_up_1", "flap_up_2", "flap_up_3"];
        for cycle in 0..3 {
            phases.push((down[cycle], unit(0.1), Behaviour::BlackHole));
            phases.push((up[cycle], unit(0.1), Behaviour::Answer));
        }
        phases.push(("healthy_tail", unit(tail_units), Behaviour::Answer));
        phases.push(("black_hole_tail", unit(0.1), Behaviour::BlackHole));
        Self { phases }
    }

    fn script(&self) -> Vec<Phase> {
        self.phases
            .iter()
            .map(|(_, duration, behaviour)| Phase::new(*duration, *behaviour))
            .collect()
    }

    fn total(&self) -> Duration {
        self.phases
            .iter()
            .fold(Duration::ZERO, |sum, (_, duration, _)| sum + *duration)
    }

    fn start_of(&self, name: &str) -> Duration {
        let mut boundary = Duration::ZERO;
        for (phase, duration, _) in &self.phases {
            if *phase == name {
                return boundary;
            }
            boundary += *duration;
        }
        boundary
    }

    fn name_at(&self, elapsed: Duration) -> &'static str {
        let mut boundary = Duration::ZERO;
        for (phase, duration, _) in &self.phases {
            boundary += *duration;
            if elapsed < boundary {
                return phase;
            }
        }
        self.phases.last().map_or("after", |(name, _, _)| name)
    }
}

async fn run_b5(
    strategy: UpstreamStrategy,
    penalty_max_ms: u64,
    ratio: &str,
    tail_units: f64,
) -> Metrics {
    let script = RecoveryScript::build(penalty_max_ms, tail_units);
    let flaky =
        MockUpstream::spawn(MockConfig::scripted(script.script()).with_dead_hold(leg())).await;
    let live = MockUpstream::spawn(MockConfig::answering()).await;
    let pool = build_pool(
        vec![udp_config(flaky.addr), udp_config(live.addr)],
        strategy,
        2,
        penalty_max_ms,
    );
    let cadence = Duration::from_millis(CADENCE_MS);
    let offset = flaky.started.elapsed();
    let until = script.total().saturating_sub(offset);
    let mut runner = Runner::new(&pool, [Arc::clone(&flaky.stats), Arc::clone(&live.stats)]);
    let samples = paced_until(&mut runner, "b5-", cadence, until).await;
    let label = format!(
        "B.5 strategy={} ratio={ratio} tail_units={tail_units}",
        strategy_name(strategy)
    );
    let report = metrics(&label, &samples);
    report.report();

    let mut black_hole_ms: Vec<f64> = Vec::new();
    let mut flapping_ms: Vec<f64> = Vec::new();
    for sample in &samples {
        match script.name_at(sample.at + offset) {
            "black_hole" => black_hole_ms.push(sample.latency_ms()),
            name if name.starts_with("flap_") => flapping_ms.push(sample.latency_ms()),
            _ => {}
        }
    }
    black_hole_ms.sort_by(f64::total_cmp);
    flapping_ms.sort_by(f64::total_cmp);
    let p99_black_hole = percentile(&black_hole_ms, 0.99);
    let p99_flapping = percentile(&flapping_ms, 0.99);

    let healthy_mid_start = script.start_of("healthy_mid");
    let flap_start = script.start_of("flap_down_1");
    let adaptive = strategy == UpstreamStrategy::Adaptive;
    let recovered_at = samples
        .iter()
        .find(|sample| {
            adaptive
                && sample.at + offset >= healthy_mid_start
                && sample.state[0] == UpstreamState::Healthy
        })
        .map(|sample| sample.at + offset);
    let recovery = recovered_at.map(|at| at.saturating_sub(healthy_mid_start));
    let first_probe_after_return = samples
        .iter()
        .find(|sample| adaptive && sample.at + offset >= healthy_mid_start && sample.probes[0] > 0)
        .map(|sample| (sample.at + offset).saturating_sub(healthy_mid_start));
    let secondary_always_healthy = samples
        .iter()
        .all(|sample| sample.state[1] == UpstreamState::Healthy);

    let mut healthy_since: Option<Duration> = None;
    let mut previous_state = UpstreamState::Healthy;
    let mut penalty_rounds: Vec<(&'static str, u8)> = Vec::new();
    let mut final_penalty: Option<(u8, Option<Duration>)> = None;
    let mut round_at_flap_start = 0u8;
    for sample in &samples {
        let at = sample.at + offset;
        if at < flap_start {
            round_at_flap_start = sample.round[0];
        }
        if sample.penalties[0] > 0 {
            penalty_rounds.push((script.name_at(at), sample.round[0]));
            let stretch = if previous_state == UpstreamState::Healthy {
                healthy_since.map(|since| at.saturating_sub(since))
            } else {
                None
            };
            final_penalty = Some((sample.round[0], stretch));
        }
        if sample.state[0] == UpstreamState::Healthy {
            if previous_state != UpstreamState::Healthy {
                healthy_since = Some(at);
            }
        } else {
            healthy_since = None;
        }
        previous_state = sample.state[0];
    }

    let flap_probes: u64 = samples
        .iter()
        .filter(|sample| script.name_at(sample.at + offset).starts_with("flap_"))
        .map(|sample| sample.probes[0])
        .sum();
    let flap_duration_ms =
        u64::try_from((script.start_of("healthy_tail") - flap_start).as_millis())
            .unwrap_or(u64::MAX);
    let nominal_ms = nominal_penalty_ms(round_at_flap_start, PENALTY_BASE_MS, penalty_max_ms);
    let flap_windows = flap_duration_ms.div_ceil(nominal_ms.max(1)).max(1);
    let tail_penalty = penalty_rounds
        .iter()
        .filter(|(phase, _)| *phase == "black_hole_tail")
        .map(|(_, round)| *round)
        .next_back();

    println!(
        "G3 recovery {label} penalty_max_ms={penalty_max_ms} queries={} \
         recovery_ms={:?} first_probe_after_return_ms={:?} \
         secondary_always_healthy={secondary_always_healthy} \
         dead_high_water={} p99_black_hole_ms={p99_black_hole:.2} \
         p99_flapping_ms={p99_flapping:.2} flap_probes={flap_probes} \
         flap_windows={flap_windows} round_at_flap_start={round_at_flap_start} \
         penalty_rounds={penalty_rounds:?} tail_penalty_round={tail_penalty:?} \
         healthy_stretch_before_final_penalty_ms={:?} reset_threshold_ms={penalty_max_ms}",
        samples.len(),
        recovery.map(|value| value.as_millis()),
        first_probe_after_return.map(|value| value.as_millis()),
        flaky.stats.dead_high_water(),
        final_penalty.map(|(_, stretch)| stretch.map(|value| value.as_millis())),
    );

    if strategy == UpstreamStrategy::Adaptive {
        assert_one_probe_per_query(&label, &samples);
        assert!(
            secondary_always_healthy,
            "{label}: the secondary must stay Healthy throughout"
        );
        assert!(
            flaky.stats.dead_high_water() <= 1,
            "{label}: more than one request was outstanding at the dead endpoint ({})",
            flaky.stats.dead_high_water()
        );
        let recovered = recovery.expect("the endpoint must return to Healthy");
        let budget = Duration::from_millis(penalty_max_ms + CADENCE_MS);
        let claimed =
            first_probe_after_return.expect("a probe must be claimed after the endpoint returns");
        let jittered = Duration::from_millis(penalty_max_ms * 125 / 100 + CADENCE_MS);
        println!(
            "G3 recovery {label} first_probe_ms={} nominal_budget_ms={} jittered_budget_ms={} \
             within_nominal={} within_jittered={}",
            claimed.as_millis(),
            budget.as_millis(),
            jittered.as_millis(),
            claimed <= budget,
            claimed <= jittered
        );
        assert!(
            claimed <= jittered,
            "{label}: the first probe after the return came {claimed:?} late, \
             jitter-aware budget {jittered:?}"
        );
        let stayed_up = recovered_at.is_some_and(|at| at <= flap_start);
        if stayed_up {
            assert!(
                recovered <= jittered,
                "{label}: recovery took {recovered:?}, jitter-aware budget {jittered:?}"
            );
        } else {
            println!(
                "G3 recovery {label} recovery_criterion=deferred recovery_ms={} \
                 first_probe_after_return_ms={} budget_ms={} \
                 healthy_window_ms={} reason=probe_landed_after_the_endpoint_went_dark_again",
                recovered.as_millis(),
                claimed.as_millis(),
                budget.as_millis(),
                (flap_start - healthy_mid_start).as_millis()
            );
        }
        let mut previous = 0u8;
        for (phase, round) in &penalty_rounds {
            if *phase == "black_hole_tail" || *phase == "healthy_tail" {
                continue;
            }
            assert!(
                *round > previous || *round == 15,
                "{label}: penalty_round must climb, saw {round} after {previous}"
            );
            previous = *round;
        }
        assert!(
            flap_probes <= flap_windows,
            "{label}: {flap_probes} probes over {flap_windows} penalty windows in the flapping phase"
        );
        if flapping_ms.len() >= 100 && black_hole_ms.len() >= 100 {
            assert!(
                p99_flapping <= 1.1 * p99_black_hole,
                "{label}: p99_flapping {p99_flapping:.2} exceeds 1.1 x p99_black_hole {p99_black_hole:.2}"
            );
        } else {
            println!(
                "G3 recovery {label} p99_comparison=undecidable flapping_n={} black_hole_n={}",
                flapping_ms.len(),
                black_hole_ms.len()
            );
        }
        if let Some((round, Some(stretch))) = final_penalty {
            if stretch >= Duration::from_millis(penalty_max_ms) {
                assert_eq!(
                    round, 1,
                    "{label}: a penalty after {stretch:?} of continuous health must reset the round"
                );
            } else {
                assert!(
                    round > 1,
                    "{label}: a penalty after only {stretch:?} of health must not reset the round"
                );
            }
        }
    }
    report
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn b5_recovery_and_flapping() {
    require_serial("B.5");
    run_b5(
        UpstreamStrategy::Fallback,
        MAX_RATIO_SHIPPED,
        "12.5",
        TAIL_UNITS,
    )
    .await;
    for (ratio, penalty_max_ms) in [
        ("2.5", MAX_RATIO_LOW),
        ("12.5", MAX_RATIO_SHIPPED),
        ("37.5", MAX_RATIO_HIGH),
    ] {
        run_b5(
            UpstreamStrategy::Adaptive,
            penalty_max_ms,
            ratio,
            TAIL_UNITS,
        )
        .await;
    }
}

async fn run_b6(strategy: UpstreamStrategy, count: usize) -> Metrics {
    let servfail = MockUpstream::spawn(MockConfig::rcode(ResponseCode::ServFail)).await;
    let live = MockUpstream::spawn(MockConfig::answering()).await;
    let pool = build_pool(
        vec![udp_config(servfail.addr), udp_config(live.addr)],
        strategy,
        2,
        MAX_RATIO_SHIPPED,
    );
    let mut runner = Runner::new(
        &pool,
        [Arc::clone(&servfail.stats), Arc::clone(&live.stats)],
    );
    let samples = sequential(&mut runner, "b6-", count).await;
    let label = format!("B.6 strategy={}", strategy_name(strategy));
    let report = metrics(&label, &samples);
    report.report();

    let allowed = tolerance(count);
    assert_eq!(
        report.attempts[0], count as u64,
        "{label}: every query must reach the first endpoint"
    );
    println!(
        "G3 B.6-walk strategy={} attempts={:?} failures={:?} allowed_stalls={allowed}",
        strategy_name(strategy),
        report.attempts,
        report.failures
    );
    assert!(
        report.attempts[1] <= allowed,
        "{label}: an RCODE answer must end the walk at the first endpoint, \
         {} queries continued",
        report.attempts[1]
    );
    assert!(
        report.failures[0] <= allowed && report.failures[1] <= allowed,
        "{label}: an RCODE is not a transport failure, failures={:?}",
        report.failures
    );
    assert_eq!(
        report.penalties,
        [0, 0],
        "{label}: an RCODE never penalizes"
    );
    assert_eq!(report.probes, [0, 0], "{label}: an RCODE never probes");
    assert_eq!(
        report.state,
        [UpstreamState::Healthy; ENDPOINTS],
        "{label}: state is unchanged end to end"
    );
    assert_eq!(
        report.consecutive_failures,
        [0, 0],
        "{label}: consecutive_failures is unchanged end to end"
    );
    report
}

async fn b6_rcode_isolation() {
    run_b6(UpstreamStrategy::Fallback, 1_000).await;
    run_b6(UpstreamStrategy::Adaptive, 1_000).await;
}

async fn run_b7_first_pass(strategy: UpstreamStrategy, calls: usize) -> Metrics {
    let first = MockUpstream::spawn(MockConfig::answering_v4_only().with_dead_hold(leg())).await;
    let second = MockUpstream::spawn(MockConfig::answering_v4_only().with_dead_hold(leg())).await;
    let pool = build_pool(
        vec![udp_config(first.addr), udp_config(second.addr)],
        strategy,
        2,
        MAX_RATIO_SHIPPED,
    );
    let mut runner = Runner::new(&pool, [Arc::clone(&first.stats), Arc::clone(&second.stats)]);
    let mut samples = Vec::with_capacity(calls);
    for index in 0..calls {
        samples.push(runner.resolve(&format!("host{index}.g3.test")).await);
    }
    let label = format!("B.7-pass1 strategy={}", strategy_name(strategy));
    let report = metrics(&label, &samples);
    report.report();
    assert_answered(&label, &report);
    if strategy == UpstreamStrategy::Adaptive {
        assert_eq!(
            report.attempts,
            [0, 0],
            "{label}: resolve_host runs in Ignore mode and moves no counter"
        );
        assert_eq!(
            report.failures,
            [0, 0],
            "{label}: Ignore mode records no failure"
        );
        assert_eq!(
            report.penalties,
            [0, 0],
            "{label}: Ignore mode never penalizes"
        );
        assert_eq!(
            report.probes,
            [0, 0],
            "{label}: Ignore mode never claims a probe"
        );
        assert_eq!(
            report.state,
            [UpstreamState::Healthy; ENDPOINTS],
            "{label}: Ignore mode leaves state alone"
        );
    }
    report
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn b7_resolve_host_isolation() {
    require_serial("B.7");
    run_b7_first_pass(UpstreamStrategy::Fallback, 100).await;
    run_b7_first_pass(UpstreamStrategy::Adaptive, 100).await;

    let dead = MockUpstream::spawn(dead_mock()).await;
    let live = MockUpstream::spawn(MockConfig::answering()).await;
    let pool = build_pool(
        vec![udp_config(dead.addr), udp_config(live.addr)],
        UpstreamStrategy::Adaptive,
        2,
        MAX_RATIO_SHIPPED,
    );
    let mut runner = Runner::new(&pool, [Arc::clone(&dead.stats), Arc::clone(&live.stats)]);
    let warmup = sequential(&mut runner, "b7warm-", 4).await;
    let warm = metrics("B.7-pass2-warmup strategy=adaptive", &warmup);
    warm.report();
    assert_eq!(
        warm.state[0],
        UpstreamState::Penalized,
        "B.7 pass 2 needs a Penalized primary"
    );

    tokio::time::sleep(Duration::from_millis(PENALTY_BASE_MS * 5 / 4 + 200)).await;

    let before = pool.status();
    let mut samples = Vec::with_capacity(20);
    for index in 0..20 {
        samples.push(runner.resolve(&format!("due{index}.g3.test")).await);
    }
    let report = metrics("B.7-pass2 strategy=adaptive", &samples);
    report.report();
    let after = pool.status();
    assert_eq!(
        report.probes,
        [0, 0],
        "B.7 pass 2: Ignore must not claim the due probe"
    );
    assert_eq!(report.attempts, [0, 0], "B.7 pass 2: no counter moves");
    assert_eq!(
        before[0].penalty_round, after[0].penalty_round,
        "B.7 pass 2: the due word's penalty_round moved"
    );
    assert_eq!(
        before[0].penalties, after[0].penalties,
        "B.7 pass 2: the due word's deadline moved"
    );
    assert_eq!(
        after[0].state,
        UpstreamState::Penalized,
        "B.7 pass 2: the due word left Penalized"
    );
}

struct SwrHarness {
    pipeline: Pipeline<UpstreamPool>,
    _workers: Vec<tokio::task::JoinHandle<()>>,
    _dir: tempfile::TempDir,
}

async fn swr_harness(pool: UpstreamPool, workers: u32) -> SwrHarness {
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(
        ListManager::new(
            &RulesConfig {
                refresh_hours_default: 24,
                lists: vec![],
            },
            dir.path().to_path_buf(),
        )
        .unwrap(),
    );
    manager.set_user_rules(String::new()).await;
    let (tx, mut rx) = tokio::sync::mpsc::channel(1_024);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let cache = DnsCacheConfig {
        min_ttl_seconds: 0,
        serve_stale: true,
        swr_workers: workers,
        cleanup_interval_seconds: 0,
        ..DnsCacheConfig::default()
    };
    let pipeline = Pipeline::new(manager, pool, 10, &cache, DEFAULT_REFRESH_CLAIM_LEASE, tx);
    let workers = pipeline.spawn_swr_workers();
    SwrHarness {
        pipeline,
        _workers: workers,
        _dir: dir,
    }
}

async fn client_query(harness: &SwrHarness, name: &str) {
    let bytes = query_for(name).to_vec().unwrap();
    let _response = harness
        .pipeline
        .handle(&bytes, IpAddr::V4(Ipv4Addr::LOCALHOST), Transport::Udp)
        .await;
}

struct SwrPass {
    refreshes: u64,
    attempts: [u64; ENDPOINTS],
    probes: [u64; ENDPOINTS],
}

async fn run_b8_swr_only(strategy: UpstreamStrategy, keys: usize, spacing: Duration) -> SwrPass {
    let dead = MockUpstream::spawn(dead_mock()).await;
    let live = MockUpstream::spawn(MockConfig::answering().with_ttl(1)).await;
    let pool = build_pool(
        vec![udp_config(dead.addr), udp_config(live.addr)],
        strategy,
        2,
        MAX_RATIO_SHIPPED,
    );
    let harness = swr_harness(pool.clone(), 3).await;

    let names: Vec<String> = (0..keys)
        .map(|index| format!("swr{index}.g3.test."))
        .collect();
    for name in &names {
        client_query(&harness, name).await;
    }
    tokio::time::sleep(Duration::from_millis(1_200)).await;

    let before_status = pool.status();
    let before_swr = harness.pipeline.swr_stats();
    let started = Instant::now();
    for name in &names {
        client_query(&harness, name).await;
        tokio::time::sleep(spacing).await;
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    let duration = started.elapsed();
    let after_status = pool.status();
    let after_swr = harness.pipeline.swr_stats();

    let mut attempts = [0u64; ENDPOINTS];
    let mut probes = [0u64; ENDPOINTS];
    for endpoint in 0..ENDPOINTS {
        attempts[endpoint] = after_status[endpoint].attempts - before_status[endpoint].attempts;
        probes[endpoint] = after_status[endpoint].probes - before_status[endpoint].probes;
    }
    let enqueued = after_swr.enqueued - before_swr.enqueued;
    let dropped = after_swr.dropped - before_swr.dropped;
    let failed = after_swr.failed - before_swr.failed;
    let completed = after_swr.completed - before_swr.completed;
    let windows = duration.as_secs() / REFRESH_FAILURE_COOLDOWN_SECONDS + 1;
    let bound = keys as u64 * windows;
    let label = strategy_name(strategy);

    println!(
        "G3 B.8-swr-only strategy={label} stale_keys={keys} duration_ms={} enqueued={enqueued} \
         dropped={dropped} failed={failed} completed={completed} enqueue_bound={bound} \
         attempts={attempts:?} probes={probes:?} state={:?} cooldown_seconds={}",
        duration.as_millis(),
        after_status[0].state,
        REFRESH_FAILURE_COOLDOWN_SECONDS
    );

    assert!(
        enqueued <= bound,
        "B.8 {label}: {enqueued} refreshes enqueued exceeds the {bound} the cooldown allows"
    );
    assert_eq!(
        attempts[1],
        enqueued - dropped,
        "B.8 {label}: only SWR refreshes may reach the healthy endpoint in this pass"
    );
    if strategy == UpstreamStrategy::Adaptive {
        assert!(
            probes[0] > 0,
            "B.8 adaptive: SWR must claim at least one probe against the dead endpoint"
        );
        assert_eq!(
            after_status[0].probe_successes, before_status[0].probe_successes,
            "B.8 adaptive: no probe can succeed against a black hole"
        );
    }

    SwrPass {
        refreshes: completed + failed,
        attempts,
        probes,
    }
}

async fn run_b8_client_only(strategy: UpstreamStrategy, queries: usize, spacing: Duration) {
    let dead = MockUpstream::spawn(dead_mock()).await;
    let live = MockUpstream::spawn(MockConfig::answering()).await;
    let pool = build_pool(
        vec![udp_config(dead.addr), udp_config(live.addr)],
        strategy,
        2,
        MAX_RATIO_SHIPPED,
    );
    let harness = swr_harness(pool.clone(), 3).await;
    for index in 0..queries {
        client_query(&harness, &format!("fresh{index}.g3.test.")).await;
        tokio::time::sleep(spacing).await;
    }
    let status = pool.status();
    let swr = harness.pipeline.swr_stats();
    let label = strategy_name(strategy);
    println!(
        "G3 B.8-client-only strategy={label} queries={queries} enqueued={} attempts=[{},{}] \
         probes=[{},{}] state={:?}",
        swr.enqueued,
        status[0].attempts,
        status[1].attempts,
        status[0].probes,
        status[1].probes,
        status[0].state
    );
    assert_eq!(
        swr.enqueued, 0,
        "B.8 client-only: never-cached names must not enqueue a refresh"
    );
    if strategy == UpstreamStrategy::Adaptive {
        assert!(
            status[0].probes > 0,
            "B.8 client-only adaptive: the client must carry the probes in this pass"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn b8_swr_interaction() {
    require_serial("B.8");
    let spacing = Duration::from_millis(40);
    let fallback = run_b8_swr_only(UpstreamStrategy::Fallback, 100, spacing).await;
    let adaptive = run_b8_swr_only(UpstreamStrategy::Adaptive, 100, spacing).await;
    run_b8_client_only(UpstreamStrategy::Fallback, 100, spacing).await;
    run_b8_client_only(UpstreamStrategy::Adaptive, 100, spacing).await;

    println!(
        "G3 B.8 refreshes fallback={} adaptive={}",
        fallback.refreshes, adaptive.refreshes
    );
    net_avoided(
        "swr-refreshes B.8 ratio=12.5 penalty_failures=2",
        fallback.attempts[0],
        adaptive.attempts[0] - adaptive.probes[0],
        adaptive.probes[0],
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn b1_pacing_dry_run() {
    require_serial("B.1-dry-run");
    let report = run_b1(
        UpstreamStrategy::Adaptive,
        2,
        MAX_RATIO_LOW,
        100,
        "2.5-dry",
        Duration::from_millis(80),
    )
    .await;
    println!(
        "G3 B.1-dry-run penalties={} probes={} failed_probes={} tax_free_share={:.5} \
         queries_after_penalty={} state={:?} round={:?}",
        report.penalties[0],
        report.probes[0],
        report.probe_paying(),
        report.tax_free_share,
        report.queries_after_penalty,
        report.state[0],
        report.round[0]
    );
    assert!(
        report.probes[0] > 0,
        "dry run must cross at least one penalty deadline, probes={}",
        report.probes[0]
    );
}
