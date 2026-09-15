use std::thread;
use std::time::Instant;

use aws_lc_rs::aead::{
    Aad, LessSafeKey, Nonce, UnboundKey, AES_128_GCM, AES_256_GCM, CHACHA20_POLY1305, NONCE_LEN,
};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const RECORD: usize = 16 * 1024;
const MIB: usize = 1024 * 1024;
const GIGABIT_MIB_PER_S: f64 = 119.209;

struct Algorithm {
    name: &'static str,
    algorithm: &'static aws_lc_rs::aead::Algorithm,
    key_len: usize,
}

const ALGORITHMS: [Algorithm; 3] = [
    Algorithm {
        name: "aes-128-gcm",
        algorithm: &AES_128_GCM,
        key_len: 16,
    },
    Algorithm {
        name: "aes-256-gcm",
        algorithm: &AES_256_GCM,
        key_len: 32,
    },
    Algorithm {
        name: "chacha20-poly1305",
        algorithm: &CHACHA20_POLY1305,
        key_len: 32,
    },
];

fn key_for(spec: &Algorithm) -> LessSafeKey {
    let material = vec![0x5au8; spec.key_len];
    let unbound =
        UnboundKey::new(spec.algorithm, &material).expect("key material of the right len");
    LessSafeKey::new(unbound)
}

fn nonce_from(counter: u64) -> Nonce {
    let mut bytes = [0u8; NONCE_LEN];
    bytes[4..].copy_from_slice(&counter.to_be_bytes());
    Nonce::assume_unique_for_key(bytes)
}

fn seal_rounds(spec: &Algorithm, records: usize) -> f64 {
    let key = key_for(spec);
    let mut buffer = vec![0x11u8; RECORD];
    buffer.reserve(32);
    let started = Instant::now();
    for counter in 0..records {
        buffer.truncate(RECORD);
        key.seal_in_place_append_tag(nonce_from(counter as u64), Aad::empty(), &mut buffer)
            .expect("seal");
    }
    started.elapsed().as_secs_f64()
}

fn sealed_record(spec: &Algorithm) -> Vec<u8> {
    let key = key_for(spec);
    let mut sealed = vec![0x11u8; RECORD];
    key.seal_in_place_append_tag(nonce_from(0), Aad::empty(), &mut sealed)
        .expect("seal the record every round reopens");
    sealed
}

fn copy_rounds(spec: &Algorithm, records: usize) -> f64 {
    let sealed = sealed_record(spec);
    let mut buffer = Vec::with_capacity(sealed.len());
    let started = Instant::now();
    for _ in 0..records {
        buffer.clear();
        buffer.extend_from_slice(&sealed);
        std::hint::black_box(&buffer);
    }
    started.elapsed().as_secs_f64()
}

fn open_rounds(spec: &Algorithm, records: usize) -> f64 {
    let key = key_for(spec);
    let sealed = sealed_record(spec);
    let mut buffer = Vec::with_capacity(sealed.len());
    let started = Instant::now();
    for _ in 0..records {
        buffer.clear();
        buffer.extend_from_slice(&sealed);
        key.open_in_place(nonce_from(0), Aad::empty(), &mut buffer)
            .expect("open");
    }
    started.elapsed().as_secs_f64()
}

fn report(spec: &Algorithm, operation: &str, threads: usize, mib: f64, seconds: f64) {
    let per_mib_ms = seconds * 1000.0 / mib;
    let mib_per_s = mib / seconds;
    let gigabit = mib_per_s / GIGABIT_MIB_PER_S;
    println!(
        "aeadbench: alg={} op={} threads={} mib={:.0} ms_per_mib={:.3} mib_per_s={:.1} \
         gigabit_headroom={:.2}x",
        spec.name, operation, threads, mib, per_mib_ms, mib_per_s, gigabit
    );
}

fn run(spec: &Algorithm, threads: usize, total_mib: usize) {
    let records_per_thread = total_mib * MIB / RECORD;
    let mib = (records_per_thread * threads * RECORD) as f64 / MIB as f64;

    for (operation, rounds) in [
        ("seal", seal_rounds as fn(&Algorithm, usize) -> f64),
        ("open", open_rounds as fn(&Algorithm, usize) -> f64),
        ("copy-control", copy_rounds as fn(&Algorithm, usize) -> f64),
    ] {
        let started = Instant::now();
        if threads == 1 {
            rounds(spec, records_per_thread);
        } else {
            thread::scope(|scope| {
                for _ in 0..threads {
                    scope.spawn(|| rounds(spec, records_per_thread));
                }
            });
        }
        report(
            spec,
            operation,
            threads,
            mib,
            started.elapsed().as_secs_f64(),
        );
    }
}

fn main() {
    let total_mib: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(256);
    let cores = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);

    println!("aeadbench: record={RECORD} total_mib_per_thread={total_mib} cores={cores}");
    println!("aeadbench: one gigabit is {GIGABIT_MIB_PER_S:.1} MiB/s, so gigabit_headroom is how many times over this arm alone could carry it");
    println!("aeadbench: open includes one copy of the sealed record, which open_in_place destroys; subtract the copy-control arm for the AEAD alone");

    for spec in &ALGORITHMS {
        run(spec, 1, total_mib.max(16));
    }
    if cores > 1 {
        for spec in &ALGORITHMS {
            run(spec, cores, total_mib.max(16));
        }
    }
    println!("aeadbench: done");
}
