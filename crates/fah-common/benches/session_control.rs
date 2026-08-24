use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};

const ROUNDS: u64 = 256;
const SEED: u64 = 0x2545_F491_4F6C_DD1D;
const WALK_WORDS: u64 = 4096;
const WALK_STRIDE: usize = 8;

fn integer_mix(mut state: u64) -> u64 {
    for _ in 0..ROUNDS {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
    }
    state
}

fn l1_walk(words: &[u64]) -> u64 {
    let mut sum = 0u64;
    let mut index = 0usize;
    while index < words.len() {
        sum = sum.wrapping_add(words[index]);
        index += WALK_STRIDE;
    }
    sum
}

fn bench_session_control(c: &mut Criterion) {
    let words: Vec<u64> = (0..WALK_WORDS).collect();

    let mut group = c.benchmark_group("session_control");
    group.bench_function("integer_mix", |b| {
        b.iter(|| integer_mix(black_box(SEED)));
    });
    group.bench_function("l1_walk", |b| {
        b.iter(|| l1_walk(black_box(&words)));
    });
    group.finish();
}

criterion_group!(benches, bench_session_control);
criterion_main!(benches);
