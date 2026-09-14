#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hickory_proto::op::{Message, OpCode, ResponseCode};
use hickory_proto::rr::rdata::{A, AAAA};
use hickory_proto::rr::{RData, Record, RecordType};
use rand::Rng;
use tokio::net::UdpSocket;

#[derive(Clone, Copy, Debug)]
pub enum Distribution {
    Fixed(Duration),
    Uniform { low: Duration, high: Duration },
    LogNormal { median: Duration, sigma: f64 },
}

impl Distribution {
    pub fn sample(self) -> Duration {
        match self {
            Distribution::Fixed(value) => value,
            Distribution::Uniform { low, high } => {
                let span = u64::try_from(high.saturating_sub(low).as_nanos()).unwrap_or(u64::MAX);
                if span == 0 {
                    low
                } else {
                    low + Duration::from_nanos(rand::rng().random_range(0..=span))
                }
            }
            Distribution::LogNormal { median, sigma } => {
                let uniform: f64 = rand::rng().random::<f64>().max(f64::MIN_POSITIVE);
                let angle: f64 = rand::rng().random();
                let normal = (-2.0 * uniform.ln()).sqrt() * (std::f64::consts::TAU * angle).cos();
                median.mul_f64((sigma * normal).exp().clamp(0.05, 20.0))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behaviour {
    Answer,
    BlackHole,
    AnswerV4Only,
}

#[derive(Clone, Copy, Debug)]
pub struct Phase {
    pub duration: Duration,
    pub behaviour: Behaviour,
}

impl Phase {
    pub fn new(duration: Duration, behaviour: Behaviour) -> Self {
        Self {
            duration,
            behaviour,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MockConfig {
    pub latency: Distribution,
    pub loss: f32,
    pub refuse: bool,
    pub rcode: Option<ResponseCode>,
    pub script: Vec<Phase>,
    pub ttl: u32,
    pub dead_hold: Duration,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            latency: Distribution::Fixed(Duration::ZERO),
            loss: 0.0,
            refuse: false,
            rcode: None,
            script: Vec::new(),
            ttl: 300,
            dead_hold: Duration::ZERO,
        }
    }
}

impl MockConfig {
    pub fn answering() -> Self {
        Self::default()
    }

    pub fn black_hole(dead_hold: Duration) -> Self {
        Self {
            script: vec![Phase::new(Duration::MAX, Behaviour::BlackHole)],
            dead_hold,
            ..Self::default()
        }
    }

    pub fn answering_v4_only() -> Self {
        Self {
            script: vec![Phase::new(Duration::MAX, Behaviour::AnswerV4Only)],
            ..Self::default()
        }
    }

    pub fn rcode(code: ResponseCode) -> Self {
        Self {
            rcode: Some(code),
            ..Self::default()
        }
    }

    pub fn refused() -> Self {
        Self {
            refuse: true,
            ..Self::default()
        }
    }

    pub fn scripted(script: Vec<Phase>) -> Self {
        Self {
            script,
            ..Self::default()
        }
    }

    pub fn with_ttl(mut self, ttl: u32) -> Self {
        self.ttl = ttl;
        self
    }

    pub fn with_dead_hold(mut self, dead_hold: Duration) -> Self {
        self.dead_hold = dead_hold;
        self
    }

    pub fn with_latency(mut self, latency: Distribution) -> Self {
        self.latency = latency;
        self
    }

    pub fn behaviour_at(&self, elapsed: Duration) -> Behaviour {
        let mut boundary = Duration::ZERO;
        for phase in &self.script {
            boundary = boundary.saturating_add(phase.duration);
            if elapsed < boundary {
                return phase.behaviour;
            }
        }
        match self.script.last() {
            Some(phase) => phase.behaviour,
            None => Behaviour::Answer,
        }
    }

    pub fn script_length(&self) -> Duration {
        self.script.iter().fold(Duration::ZERO, |total, phase| {
            total.saturating_add(phase.duration)
        })
    }
}

#[derive(Debug, Default)]
pub struct MockStats {
    datagrams: AtomicU64,
    answered: AtomicU64,
    lost: AtomicU64,
    black_holed: AtomicU64,
    dead_in_flight: AtomicU64,
    dead_high_water: AtomicU64,
}

impl MockStats {
    pub fn datagrams(&self) -> u64 {
        self.datagrams.load(Ordering::Relaxed)
    }

    pub fn answered(&self) -> u64 {
        self.answered.load(Ordering::Relaxed)
    }

    pub fn lost(&self) -> u64 {
        self.lost.load(Ordering::Relaxed)
    }

    pub fn black_holed(&self) -> u64 {
        self.black_holed.load(Ordering::Relaxed)
    }

    pub fn dead_high_water(&self) -> u64 {
        self.dead_high_water.load(Ordering::Relaxed)
    }

    fn enter_dead(&self) {
        let in_flight = self.dead_in_flight.fetch_add(1, Ordering::Relaxed) + 1;
        self.dead_high_water.fetch_max(in_flight, Ordering::Relaxed);
    }

    fn leave_dead(&self) {
        self.dead_in_flight.fetch_sub(1, Ordering::Relaxed);
    }
}

pub struct MockUpstream {
    pub addr: SocketAddr,
    pub stats: Arc<MockStats>,
    pub started: Instant,
    config: MockConfig,
}

impl MockUpstream {
    pub async fn spawn(config: MockConfig) -> Self {
        Self::spawn_on("127.0.0.1:0", config).await
    }

    pub async fn spawn_on(bind: &str, config: MockConfig) -> Self {
        let socket = UdpSocket::bind(bind).await.unwrap();
        let addr = socket.local_addr().unwrap();
        let stats = Arc::new(MockStats::default());
        let started = Instant::now();

        if config.refuse {
            drop(socket);
            return Self {
                addr,
                stats,
                started,
                config,
            };
        }

        let socket = Arc::new(socket);
        let serving = Arc::clone(&socket);
        let counters = Arc::clone(&stats);
        let script = config.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                let Ok((len, client)) = serving.recv_from(&mut buf).await else {
                    return;
                };
                counters.datagrams.fetch_add(1, Ordering::Relaxed);
                let Ok(request) = Message::from_vec(&buf[..len]) else {
                    continue;
                };
                let behaviour = script.behaviour_at(started.elapsed());
                let wanted = request
                    .queries
                    .first()
                    .map_or(RecordType::A, hickory_proto::op::Query::query_type);
                let silent = behaviour == Behaviour::BlackHole
                    || (behaviour == Behaviour::AnswerV4Only && wanted != RecordType::A);

                if silent {
                    counters.black_holed.fetch_add(1, Ordering::Relaxed);
                    counters.enter_dead();
                    let counters = Arc::clone(&counters);
                    let hold = script.dead_hold;
                    tokio::spawn(async move {
                        tokio::time::sleep(hold).await;
                        counters.leave_dead();
                    });
                    continue;
                }
                if script.loss > 0.0 && rand::rng().random::<f32>() < script.loss {
                    counters.lost.fetch_add(1, Ordering::Relaxed);
                    continue;
                }

                let bytes = reply(&request, script.rcode, script.ttl).to_vec().unwrap();
                let delay = script.latency.sample();
                let sending = Arc::clone(&serving);
                let counters = Arc::clone(&counters);
                tokio::spawn(async move {
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    if sending.send_to(&bytes, client).await.is_ok() {
                        counters.answered.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });

        Self {
            addr,
            stats,
            started,
            config,
        }
    }

    pub fn behaviour_now(&self) -> Behaviour {
        self.config.behaviour_at(self.started.elapsed())
    }

    pub fn config(&self) -> &MockConfig {
        &self.config
    }
}

pub async fn closed_tcp_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 1))
}

fn reply(request: &Message, rcode: Option<ResponseCode>, ttl: u32) -> Message {
    let mut response = Message::response(request.metadata.id, OpCode::Query);
    response.metadata.response_code = rcode.unwrap_or(ResponseCode::NoError);
    response.metadata.recursion_available = true;
    response.queries = request.queries.clone();
    if rcode.is_some() {
        return response;
    }
    for query in &request.queries {
        let data = match query.query_type() {
            RecordType::A => RData::A(A(std::net::Ipv4Addr::new(93, 184, 216, 34))),
            RecordType::AAAA => RData::AAAA(AAAA(std::net::Ipv6Addr::new(
                0x2606, 0x2800, 0x220, 1, 0x248, 0x1893, 0x25c8, 0x1946,
            ))),
            _ => continue,
        };
        response.add_answer(Record::from_rdata(query.name().clone(), ttl, data));
    }
    response
}
