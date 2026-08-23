use std::alloc::{GlobalAlloc, Layout};
use std::hint::black_box;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};

use fah_config::{DnsUpstreamsConfig, UpstreamProtocol, UpstreamServerConfig, UpstreamStrategy};
use fah_dns::{Forwarder, UpstreamPool};
use hickory_proto::op::{Message, OpCode, Query as WireQuery, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use mimalloc::MiMalloc;
use tokio::net::UdpSocket;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// SAFETY: every method forwards to `MiMalloc`, a sound `GlobalAlloc`; the counter never touches the returned memory nor alters the layout.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: `layout` satisfies the trait contract at the call site and is forwarded verbatim.
        unsafe { MiMalloc.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` come from a prior `alloc` with the same layout, as the trait requires.
        unsafe { MiMalloc.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
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
fn adaptive_forwards_allocate_exactly_as_much_as_fallback_forwards() {
    const FORWARDS: usize = 64;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let addr = rt.block_on(answering_server());
    let query = a_query();

    let mut measured = Vec::new();
    for strategy in [UpstreamStrategy::Fallback, UpstreamStrategy::Adaptive] {
        let pool = pool_for(addr, strategy);
        for _ in 0..FORWARDS {
            rt.block_on(pool.forward(black_box(&query))).unwrap();
        }

        let before = ALLOCATIONS.load(Ordering::Relaxed);
        for _ in 0..FORWARDS {
            rt.block_on(pool.forward(black_box(&query))).unwrap();
        }
        measured.push(ALLOCATIONS.load(Ordering::Relaxed) - before);
    }

    assert_eq!(
        measured[1], measured[0],
        "{FORWARDS} adaptive forwards allocated {} against fallback's {}; \
         Stage 1 must add none",
        measured[1], measured[0]
    );
}
