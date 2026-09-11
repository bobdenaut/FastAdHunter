use std::alloc::{GlobalAlloc, Layout};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fah_config::{
    DnsCacheConfig, DnsConfig, DnsListenConfig, DnsUpstreamsConfig, RulesConfig, UpstreamProtocol,
    UpstreamServerConfig, UpstreamStrategy,
};
use fah_dns::{Pipeline, Server, UpstreamPool, DEFAULT_REFRESH_CLAIM_LEASE};
use fah_rules::ListManager;
use hickory_proto::op::{Message, OpCode, Query as WireQuery, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use mimalloc::MiMalloc;
use tokio::net::UdpSocket;
use tokio::runtime::Handle;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct Counting;

fn grow(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
}

// SAFETY: every method forwards to `MiMalloc`, a sound `GlobalAlloc`; the counters never touch the returned memory nor alter the layout.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        // SAFETY: `layout` satisfies the trait contract at the call site and is forwarded verbatim.
        unsafe { MiMalloc.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: `ptr`/`layout` come from a prior `alloc` with the same layout, as the trait requires.
        unsafe { MiMalloc.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        grow(new_size);
        // SAFETY: `ptr`/`layout`/`new_size` satisfy the trait contract at the call site.
        unsafe { MiMalloc.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const FLOOD_SECONDS: u64 = 8;
const TICK_MS: u64 = 10;
const DRAIN: Duration = Duration::from_secs(6);

fn udp_server(addr: SocketAddr) -> UpstreamServerConfig {
    UpstreamServerConfig {
        address: addr.to_string(),
        protocol: UpstreamProtocol::Udp,
        hostname: None,
    }
}

async fn black_hole() -> (UdpSocket, SocketAddr) {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    (socket, addr)
}

async fn answering() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 512];
        loop {
            let (len, client) = socket.recv_from(&mut buf).await.unwrap();
            let request = Message::from_vec(&buf[..len]).unwrap();
            let mut response = Message::response(request.metadata.id, OpCode::Query);
            response.metadata.response_code = ResponseCode::NoError;
            response.queries = request.queries.clone();
            socket
                .send_to(&response.to_vec().unwrap(), client)
                .await
                .unwrap();
        }
    });
    addr
}

async fn start_server(
    servers: Vec<UpstreamServerConfig>,
    strategy: UpstreamStrategy,
) -> (Server, tempfile::TempDir) {
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
        timeout_ms: 800,
        penalty_failures: 2,
        servers,
    };
    let pool = UpstreamPool::from_config(&upstreams).unwrap();
    let (events, mut drain) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move { while drain.recv().await.is_some() {} });
    let pipeline = Arc::new(Pipeline::new(
        rules,
        pool,
        10,
        &DnsCacheConfig::default(),
        DEFAULT_REFRESH_CLAIM_LEASE,
        events,
    ));
    let config = DnsConfig {
        listen: DnsListenConfig {
            address: "127.0.0.1".to_string(),
            port: 0,
        },
        ..DnsConfig::default()
    };
    let mut server = Server::bind(&config).await.unwrap();
    server.serve(pipeline);
    (server, data_dir)
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

#[cfg(windows)]
fn rss_bytes() -> Option<u64> {
    let output = std::process::Command::new("tasklist")
        .args([
            "/FI",
            &format!("PID eq {}", std::process::id()),
            "/FO",
            "CSV",
            "/NH",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let field = text.trim().trim_end_matches('"').rsplit('"').next()?;
    let kib: u64 = field
        .trim_end_matches(" K")
        .replace(['.', ',', '\u{a0}'], "")
        .trim()
        .parse()
        .ok()?;
    Some(kib * 1024)
}

#[cfg(not(windows))]
fn rss_bytes() -> Option<u64> {
    fah_common::process::resident_bytes()
}

struct Arm {
    label: &'static str,
    rate: u64,
    walk: Duration,
    sent: u64,
    replies: u64,
    servfail: u64,
    baseline_tasks: usize,
    peak_tasks: usize,
    baseline_live: usize,
    peak_live: usize,
    baseline_rss: Option<u64>,
    peak_rss: Option<u64>,
    allocations: usize,
}

impl Arm {
    fn inflight(&self) -> usize {
        self.peak_tasks.saturating_sub(self.baseline_tasks)
    }

    fn heap_growth(&self) -> usize {
        self.peak_live.saturating_sub(self.baseline_live)
    }

    fn print(&self) {
        let inflight = self.inflight();
        let per_query = self.heap_growth().checked_div(inflight).unwrap_or(0);
        let rss_growth = match (self.baseline_rss, self.peak_rss) {
            (Some(base), Some(peak)) => format!("{}", peak.saturating_sub(base) / 1024 / 1024),
            _ => "n/a".to_string(),
        };
        println!(
            "F2 {label} rate={rate} walk_ms={walk} sent={sent} replies={replies} servfail={servfail} \
             peak_inflight={inflight} expected_inflight={expected} heap_growth_kib={heap} \
             heap_per_inflight_bytes={per_query} rss_growth_mib={rss_growth} allocs_per_query={allocs}",
            label = self.label,
            rate = self.rate,
            walk = self.walk.as_millis(),
            sent = self.sent,
            replies = self.replies,
            servfail = self.servfail,
            expected = (self.rate as f64 * self.walk.as_secs_f64()).round() as u64,
            heap = self.heap_growth() / 1024,
            allocs = self
                .allocations
                .checked_div(self.sent as usize)
                .unwrap_or(0),
        );
    }
}

async fn measure_walk(client: &UdpSocket, server: SocketAddr) -> (Duration, ResponseCode) {
    let query = query_bytes(1, "walk.f2.example.");
    let started = Instant::now();
    client.send_to(&query, server).await.unwrap();
    let mut buf = [0u8; 512];
    let len = tokio::time::timeout(Duration::from_secs(30), client.recv(&mut buf))
        .await
        .unwrap()
        .unwrap();
    let reply = Message::from_vec(&buf[..len]).unwrap();
    (started.elapsed(), reply.metadata.response_code)
}

async fn run_arm(label: &'static str, rate: u64, server: SocketAddr) -> Arm {
    let client = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let (walk, walk_rcode) = measure_walk(&client, server).await;
    println!(
        "F2 {label} probe rcode={walk_rcode:?} walk_ms={}",
        walk.as_millis()
    );
    tokio::time::sleep(Duration::from_millis(200)).await;

    let replies = Arc::new(AtomicU64::new(0));
    let servfail = Arc::new(AtomicU64::new(0));
    let receiver = {
        let client = Arc::clone(&client);
        let replies = Arc::clone(&replies);
        let servfail = Arc::clone(&servfail);
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            loop {
                let Ok(len) = client.recv(&mut buf).await else {
                    return;
                };
                replies.fetch_add(1, Ordering::Relaxed);
                if Message::from_vec(&buf[..len])
                    .is_ok_and(|reply| reply.metadata.response_code == ResponseCode::ServFail)
                {
                    servfail.fetch_add(1, Ordering::Relaxed);
                }
            }
        })
    };

    let baseline_tasks = Handle::current().metrics().num_alive_tasks();
    let baseline_live = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline_live, Ordering::Relaxed);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let baseline_rss = rss_bytes();

    let stop = Arc::new(AtomicBool::new(false));
    let peak_tasks = Arc::new(AtomicUsize::new(0));
    let sampler = {
        let stop = Arc::clone(&stop);
        let peak_tasks = Arc::clone(&peak_tasks);
        let handle = Handle::current();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(20));
            while !stop.load(Ordering::Relaxed) {
                ticker.tick().await;
                peak_tasks.fetch_max(handle.metrics().num_alive_tasks(), Ordering::Relaxed);
            }
        })
    };
    let peak_rss = Arc::new(AtomicU64::new(0));
    let rss_sampler = {
        let stop = Arc::clone(&stop);
        let peak_rss = Arc::clone(&peak_rss);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Some(rss) = rss_bytes() {
                    peak_rss.fetch_max(rss, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
    };

    let per_tick = rate * TICK_MS / 1000;
    let ticks = FLOOD_SECONDS * 1000 / TICK_MS;
    let mut ticker = tokio::time::interval(Duration::from_millis(TICK_MS));
    let mut sent = 0u64;
    for _ in 0..ticks {
        ticker.tick().await;
        for _ in 0..per_tick {
            sent += 1;
            let name = format!("q{sent}.{label}.f2.example.");
            let query = query_bytes((sent % 65_535) as u16 + 1, &name);
            client.send_to(&query, server).await.unwrap();
        }
    }
    tokio::time::sleep(walk + DRAIN).await;

    stop.store(true, Ordering::Relaxed);
    sampler.await.unwrap();
    rss_sampler.join().unwrap();
    receiver.abort();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);

    Arm {
        label,
        rate,
        walk,
        sent,
        replies: replies.load(Ordering::Relaxed),
        servfail: servfail.load(Ordering::Relaxed),
        baseline_tasks,
        peak_tasks: peak_tasks.load(Ordering::Relaxed),
        baseline_live,
        peak_live: PEAK.load(Ordering::Relaxed),
        baseline_rss,
        peak_rss: match peak_rss.load(Ordering::Relaxed) {
            0 => None,
            bytes => Some(bytes),
        },
        allocations,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn udp_inflight_cost_under_upstream_outage() {
    let mut holes = Vec::new();
    for _ in 0..4 {
        holes.push(black_hole().await);
    }
    let dead_servers = |count: usize| holes[..count].iter().map(|(_, addr)| udp_server(*addr));
    let (dead2, _dead2_dir) =
        start_server(dead_servers(2).collect(), UpstreamStrategy::Fallback).await;
    let (dead4, _dead4_dir) =
        start_server(dead_servers(4).collect(), UpstreamStrategy::Fallback).await;
    let (adaptive4, _adaptive4_dir) =
        start_server(dead_servers(4).collect(), UpstreamStrategy::Adaptive).await;

    let answering_addr = answering().await;
    let (alive, _alive_dir) =
        start_server(vec![udp_server(answering_addr)], UpstreamStrategy::Fallback).await;

    let mut arms = Vec::new();
    arms.push(run_arm("control", 1000, alive.udp_addr()).await);
    for rate in [100, 300, 1000, 3000] {
        arms.push(run_arm("fallback2", rate, dead2.udp_addr()).await);
    }
    arms.push(run_arm("fallback4", 1000, dead4.udp_addr()).await);
    arms.push(run_arm("adaptive4", 1000, adaptive4.udp_addr()).await);
    arms.push(run_arm("control", 1000, alive.udp_addr()).await);

    println!("F2 summary (dev box, timeout_ms=800, UDP black-hole upstreams)");
    for arm in &arms {
        arm.print();
    }
    dead2.shutdown();
    dead4.shutdown();
    adaptive4.shutdown();
    alive.shutdown();
}
