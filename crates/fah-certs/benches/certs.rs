use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use fah_certs::{CaParams, CertStore};

const HOSTS: usize = 4096;

fn store_with_ca() -> (tempfile::TempDir, CertStore) {
    fah_certs::install_crypto_provider();
    let dir = tempfile::tempdir().unwrap();
    let store = CertStore::open(dir.path()).unwrap();
    store.generate_ca(&CaParams::default()).unwrap();
    (dir, store)
}

fn certs_mint(c: &mut Criterion) {
    let (_dir, store) = store_with_ca();
    let hosts = (0..HOSTS)
        .map(|index| format!("host{index}.example"))
        .collect::<Vec<_>>();
    let mut next = 0usize;
    c.bench_function("certs_mint", |b| {
        b.iter(|| {
            let host = &hosts[next % HOSTS];
            next += 1;
            black_box(store.prewarm(black_box(host)).unwrap());
        })
    });
}

fn certs_cache_hit(c: &mut Criterion) {
    let (_dir, store) = store_with_ca();
    store.prewarm("cached.example").unwrap();
    c.bench_function("certs_cache_hit", |b| {
        b.iter(|| black_box(store.cached_leaf(black_box("cached.example")).unwrap()))
    });
}

fn certs_prewarm_warm(c: &mut Criterion) {
    let (_dir, store) = store_with_ca();
    store.prewarm("cached.example").unwrap();
    c.bench_function("certs_prewarm_warm", |b| {
        b.iter(|| black_box(store.prewarm(black_box("cached.example")).unwrap()))
    });
}

const ZIPF_HOSTS: usize = 4096;
const ZIPF_REPLAY: usize = 100_000;
const ZIPF_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

struct Zipf {
    cdf: Vec<f64>,
    state: u64,
}

impl Zipf {
    fn new(hosts: usize, seed: u64) -> Self {
        let mut cdf = Vec::with_capacity(hosts);
        let mut total = 0.0;
        for rank in 1..=hosts {
            total += 1.0 / rank as f64;
            cdf.push(total);
        }
        for value in &mut cdf {
            *value /= total;
        }
        Self { cdf, state: seed }
    }

    fn next_rank(&mut self) -> usize {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        let unit = (self.state >> 11) as f64 / (1u64 << 53) as f64;
        self.cdf
            .partition_point(|&p| p < unit)
            .min(self.cdf.len() - 1)
    }
}

fn certs_replay_zipf(c: &mut Criterion) {
    let (_dir, store) = store_with_ca();
    let hosts = (0..ZIPF_HOSTS)
        .map(|index| format!("host{index}.example"))
        .collect::<Vec<_>>();
    let mut zipf = Zipf::new(ZIPF_HOSTS, ZIPF_SEED);
    for _ in 0..ZIPF_REPLAY {
        store.prewarm(&hosts[zipf.next_rank()]).unwrap();
    }
    let stats = store.leaf_cache_stats();
    let handshakes = stats.prewarm_hits + stats.minted_total;
    println!(
        "certs_replay_zipf: {ZIPF_REPLAY} handshakes, Zipf(s=1) over {ZIPF_HOSTS} hosts, \
         LRU {}: prewarm_hits={} minted_total={} evictions={} hit_rate={:.4}",
        stats.capacity,
        stats.prewarm_hits,
        stats.minted_total,
        stats.evictions,
        stats.prewarm_hits as f64 / handshakes as f64
    );
    c.bench_function("certs_replay_zipf", |b| {
        b.iter(|| black_box(store.prewarm(black_box(&hosts[zipf.next_rank()])).unwrap()))
    });
}

criterion_group!(
    benches,
    certs_mint,
    certs_cache_hit,
    certs_prewarm_warm,
    certs_replay_zipf
);
criterion_main!(benches);
