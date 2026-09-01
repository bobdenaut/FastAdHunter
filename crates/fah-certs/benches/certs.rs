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

criterion_group!(benches, certs_mint, certs_cache_hit, certs_prewarm_warm);
criterion_main!(benches);
