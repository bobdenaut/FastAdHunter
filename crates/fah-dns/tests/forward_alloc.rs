use std::alloc::{GlobalAlloc, Layout};
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use fah_config::{
    DnsCacheConfig, DnsUpstreamsConfig, RulesConfig, UpstreamProtocol, UpstreamServerConfig,
    UpstreamStrategy,
};
use fah_dns::{ForwardOutcome, Forwarder, Pipeline, Transport, UpstreamPool};
use fah_rules::ListManager;
use hickory_proto::op::{Message, OpCode, Query as WireQuery, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, Record, RecordType};
use mimalloc::MiMalloc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

const TRANSPORTS: [Transport; 4] = [
    Transport::Udp,
    Transport::Tcp,
    Transport::Dot,
    Transport::Doh,
];

const JITTER_ALLOWANCE: usize = 4;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static SERIAL: Mutex<()> = Mutex::new(());

struct Counting;

// SAFETY: every method forwards to `MiMalloc`, a sound `GlobalAlloc`; the counters never touch the returned memory nor alter the layout.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: `layout` satisfies the trait contract at the call site and is forwarded verbatim.
        unsafe { MiMalloc.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` come from a prior `alloc` with the same layout, as the trait requires.
        unsafe { MiMalloc.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size, Ordering::Relaxed);
        // SAFETY: `ptr`/`layout`/`new_size` satisfy the trait contract at the call site.
        unsafe { MiMalloc.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn a_query() -> Message {
    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_ascii("example.com.").unwrap(),
        RecordType::A,
    ));
    message
}

async fn answering_server() -> SocketAddr {
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

fn pool_for(addr: SocketAddr, strategy: UpstreamStrategy) -> UpstreamPool {
    UpstreamPool::from_config(&DnsUpstreamsConfig {
        strategy,
        timeout_ms: 2_000,
        servers: vec![UpstreamServerConfig {
            address: addr.to_string(),
            protocol: UpstreamProtocol::Udp,
            hostname: None,
        }],
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn warm_adaptive_forwards_allocate_a_steady_amount() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    const FORWARDS: usize = 64;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let addr = rt.block_on(answering_server());
    let query = a_query();

    let pool = pool_for(addr, UpstreamStrategy::Adaptive);
    for _ in 0..FORWARDS {
        rt.block_on(pool.forward(black_box(&query))).unwrap();
    }

    let mut measured = Vec::new();
    for _ in 0..2 {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        for _ in 0..FORWARDS {
            rt.block_on(pool.forward(black_box(&query))).unwrap();
        }
        measured.push(ALLOCATIONS.load(Ordering::Relaxed) - before);
    }

    println!(
        "forward/allocations over {FORWARDS} warm forwards: first batch {} second batch {}",
        measured[0], measured[1]
    );

    assert!(
        measured[1] <= measured[0] + JITTER_ALLOWANCE,
        "{FORWARDS} warm adaptive forwards allocated {} then {}; the walk must not accumulate. \
         This checks the absence of growth, not bit-for-bit equality between two measurements a \
         scheduler and socket readiness both touch: one leaked allocation per forward would show \
         as +{FORWARDS} here, so a difference within {JITTER_ALLOWANCE} is noise",
        measured[0],
        measured[1]
    );
}

#[derive(Clone)]
struct StubForwarder;

impl Forwarder for StubForwarder {
    async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
        let mut response = Message::response(request.metadata.id, request.metadata.op_code);
        response.metadata.response_code = ResponseCode::NoError;
        Ok(ForwardOutcome::new(response, 0))
    }
}

fn raw_query(name: &str) -> Vec<u8> {
    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_ascii(name).unwrap(),
        RecordType::A,
    ));
    message.to_vec().unwrap()
}

#[derive(Clone)]
struct AnsweringStub;

impl Forwarder for AnsweringStub {
    async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
        let mut response = Message::response(request.metadata.id, request.metadata.op_code);
        response.metadata.response_code = ResponseCode::NoError;
        response.queries = request.queries.clone();
        if let Some(query) = request.queries.first() {
            response.add_answer(Record::from_rdata(
                query.name().clone(),
                300,
                RData::A(A::new(93, 184, 216, 34)),
            ));
        }
        Ok(ForwardOutcome::new(response, 0))
    }
}

async fn pipeline_blocking_one_domain<F: Forwarder>(
    data_dir: &Path,
    forwarder: F,
) -> (Pipeline<F>, mpsc::Receiver<fah_model::Event>) {
    let rules = Arc::new(
        ListManager::new(
            &RulesConfig {
                refresh_hours_default: 24,
                lists: vec![],
            },
            data_dir.to_path_buf(),
        )
        .unwrap(),
    );
    rules
        .set_user_rules("||blocked.example.com^\n".to_string())
        .await;
    let (events, receiver) = mpsc::channel(1);
    let pipeline = Pipeline::new(
        rules,
        forwarder,
        10,
        &DnsCacheConfig::default(),
        fah_dns::DEFAULT_REFRESH_CLAIM_LEASE,
        events,
    );
    (pipeline, receiver)
}

#[test]
fn warm_pipeline_handles_allocate_a_steady_amount() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    const HANDLES: usize = 64;
    const CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50));

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let (pipeline, _events) =
        rt.block_on(pipeline_blocking_one_domain(data_dir.path(), StubForwarder));

    let cases = [
        ("blocked, inline name", "blocked.example.com.", 13),
        (
            "blocked, heap name",
            "a-very-long-subdomain-label-here.blocked.example.com.",
            19,
        ),
        ("cache hit, inline name", "example.org.", 10),
        (
            "cache hit, heap name",
            "a-very-long-subdomain-label-here.allowed.example.org.",
            16,
        ),
    ];
    for (label, name, ceiling_per_handle) in cases {
        let raw = raw_query(name);
        for transport in TRANSPORTS {
            for _ in 0..HANDLES {
                rt.block_on(pipeline.handle(black_box(&raw), CLIENT, transport))
                    .unwrap();
            }

            let mut measured = Vec::new();
            for _ in 0..2 {
                let before = ALLOCATIONS.load(Ordering::Relaxed);
                for _ in 0..HANDLES {
                    rt.block_on(pipeline.handle(black_box(&raw), CLIENT, transport))
                        .unwrap();
                }
                measured.push(ALLOCATIONS.load(Ordering::Relaxed) - before);
            }

            println!(
                "handle/allocations over {HANDLES} warm handles ({label}, {transport:?}): first batch {} second batch {}",
                measured[0], measured[1]
            );

            assert!(
                measured[1] <= measured[0] + JITTER_ALLOWANCE,
                "{HANDLES} warm handles ({label}, {transport:?}) allocated {} then {}; the \
                 pipeline must not accumulate. This checks the absence of growth, not \
                 bit-for-bit equality between two measurements a scheduler touches: one leaked \
                 allocation per handle would show as +{HANDLES} here, so a difference within \
                 {JITTER_ALLOWANCE} is noise",
                measured[0],
                measured[1]
            );
            assert!(
                measured[1] <= HANDLES * ceiling_per_handle + JITTER_ALLOWANCE,
                "{HANDLES} warm handles ({label}, {transport:?}) allocated {}; the ceiling is \
                 {ceiling_per_handle} per handle plus {JITTER_ALLOWANCE}, because one leaked \
                 allocation per handle would show as +{HANDLES} here",
                measured[1]
            );
        }
    }
}

#[test]
fn warm_pipeline_misses_stay_under_the_ceiling() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    const HANDLES: usize = 64;
    const CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50));

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let (pipeline, _events) =
        rt.block_on(pipeline_blocking_one_domain(data_dir.path(), AnsweringStub));

    let cases = [
        ("miss, inline name", "m{i:04}.example.net.", 17),
        (
            "miss, heap name",
            "a-very-long-subdomain-label-here-{i:04}.miss.example.net.",
            24,
        ),
    ];
    let mut results = Vec::new();
    for (label, pattern, ceiling_per_handle) in cases {
        let raws: Vec<Vec<u8>> = (0..HANDLES * 3 * TRANSPORTS.len())
            .map(|i| raw_query(&pattern.replace("{i:04}", &format!("{i:04}"))))
            .collect();
        let mut next = raws.iter();
        for transport in TRANSPORTS {
            for _ in 0..HANDLES {
                rt.block_on(pipeline.handle(black_box(next.next().unwrap()), CLIENT, transport))
                    .unwrap();
            }

            let mut measured = Vec::new();
            let mut bytes = Vec::new();
            for _ in 0..2 {
                let before = ALLOCATIONS.load(Ordering::Relaxed);
                let bytes_before = BYTES.load(Ordering::Relaxed);
                for _ in 0..HANDLES {
                    rt.block_on(pipeline.handle(
                        black_box(next.next().unwrap()),
                        CLIENT,
                        transport,
                    ))
                    .unwrap();
                }
                measured.push(ALLOCATIONS.load(Ordering::Relaxed) - before);
                bytes.push(BYTES.load(Ordering::Relaxed) - bytes_before);
            }

            println!(
                "handle/allocations over {HANDLES} warm misses ({label}, {transport:?}): first batch {} ({} bytes) second batch {} ({} bytes)",
                measured[0], bytes[0], measured[1], bytes[1]
            );
            results.push((label, transport, ceiling_per_handle, measured));
        }
    }

    for (label, transport, ceiling_per_handle, measured) in results {
        for allocations in measured {
            assert!(
                allocations <= HANDLES * ceiling_per_handle + JITTER_ALLOWANCE,
                "{HANDLES} warm misses ({label}, {transport:?}) allocated {allocations}; the \
                 ceiling is {ceiling_per_handle} per handle plus {JITTER_ALLOWANCE}, because one \
                 leaked allocation per handle would show as +{HANDLES} here"
            );
        }
    }
}
