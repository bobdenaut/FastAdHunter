//! Cost of the p2-07 accounting walk, measured rather than assumed.
//!
//! `cargo test -p fah-stats --release --test heap_cost -- --nocapture --ignored`
//!
//! `#[ignore]` because it prints a timing rather than asserting one: a wall
//! clock on a shared dev box is not a gate. The number it produces is what
//! decides whether a structure keeps a running total instead of being walked.
//!
//! The figure it used to justify was the query log's ring, which p2-09 removed
//! along with the log; what remains are the aggregates and the client registry.

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Instant, SystemTime};

use fah_config::{HistoryConfig, StatsConfig};
use fah_model::{Query, QueryEvent, QueryType, Verdict};
use fah_stats::Stats;

#[tokio::test]
#[ignore = "timing measurement, not an assertion"]
async fn measure_heap_walk_cost() {
    let dir = tempfile::tempdir().unwrap();
    let stats = Stats::new(
        &StatsConfig::default(),
        &HistoryConfig::default(),
        dir.path().to_path_buf(),
    );
    stats.boot().await;

    // Saturate both counted structures, at realistic domain lengths.
    for i in 0..20_000u32 {
        let client = IpAddr::V4(Ipv4Addr::new(192, 168, (i / 256) as u8, (i % 256) as u8));
        stats.record(QueryEvent::new(
            Query::new(
                format!("sub{i}.some-tracker-domain-{i}.example.com"),
                QueryType::A,
                client,
                SystemTime::now(),
            ),
            Verdict::Pass,
            std::time::Duration::from_micros(100),
            false,
            true,
            false,
        ));
    }

    let heap = stats.heap();
    println!(
        "\naggregates {} bytes, clients {} bytes, total {}",
        heap.aggregates,
        heap.clients,
        heap.total()
    );

    for round in 1..=3 {
        let iterations = 200;
        let start = Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(stats.heap());
        }
        println!(
            "round {round}: heap() = {:?} per call",
            start.elapsed() / iterations
        );
    }
}
