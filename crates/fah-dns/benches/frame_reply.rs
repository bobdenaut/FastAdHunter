use std::alloc::{GlobalAlloc, Layout};
use std::hint::black_box;
use std::io::IoSlice;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use mimalloc::MiMalloc;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

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

const SIZES: [usize; 7] = [64, 256, 500, 512, 1024, 4096, 16384];

const TOKIO_MAX_VECTOR_ELEMENTS: usize = 64;

const HICKORY_INITIAL_CAPACITY: usize = 512;

fn hickory_capacity(len: usize) -> usize {
    let mut capacity = HICKORY_INITIAL_CAPACITY;
    while capacity < len {
        capacity *= 2;
    }
    capacity
}

fn reply_with_spare_capacity(len: usize) -> Vec<u8> {
    let mut capacity = hickory_capacity(len);
    if capacity < len + 2 {
        capacity *= 2;
    }
    let mut reply = Vec::with_capacity(capacity);
    reply.resize(len, 0x2a);
    reply
}

fn reply_with_exact_capacity(len: usize) -> Vec<u8> {
    let mut reply = Vec::with_capacity(len);
    reply.resize(len, 0x2a);
    reply
}

fn frame_by_splice(reply: &mut Vec<u8>) {
    let len = u16::try_from(reply.len()).unwrap_or(u16::MAX).to_be_bytes();
    reply.splice(0..0, len);
}

fn frame_by_iovec(len_buf: &mut [u8; 2], reply: &[u8]) -> usize {
    *len_buf = u16::try_from(reply.len()).unwrap_or(u16::MAX).to_be_bytes();
    let mut slices = [IoSlice::new(&[]); TOKIO_MAX_VECTOR_ELEMENTS];
    slices[0] = IoSlice::new(&len_buf[..]);
    slices[1] = IoSlice::new(reply);
    black_box(&slices);
    slices[0].len() + slices[1].len()
}

fn probe_splice(shape: &str, len: usize, mut reply: Vec<u8>) {
    let capacity_before = reply.capacity();
    let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
    let bytes_before = BYTES.load(Ordering::Relaxed);
    frame_by_splice(&mut reply);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations_before;
    let bytes = BYTES.load(Ordering::Relaxed) - bytes_before;
    let capacity_after = reply.capacity();
    println!(
        "| splice | {shape} | {len} | {capacity_before} | {capacity_after} | {allocations} | {bytes} |"
    );
    black_box(reply);
}

fn probe_iovec(shape: &str, len: usize, reply: Vec<u8>) {
    let capacity_before = reply.capacity();
    let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
    let bytes_before = BYTES.load(Ordering::Relaxed);
    let mut len_buf = [0u8; 2];
    let written = frame_by_iovec(&mut len_buf, &reply);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations_before;
    let bytes = BYTES.load(Ordering::Relaxed) - bytes_before;
    assert_eq!(written, len + 2);
    println!(
        "| iovec | {shape} | {len} | {capacity_before} | {capacity_before} | {allocations} | {bytes} |"
    );
    black_box(reply);
}

fn print_allocation_probe() {
    println!("\n[F3] one framing operation per row, mimalloc, counting allocator\n");
    println!("| path | shape | reply len | cap before | cap after | allocations | bytes |");
    println!("| ---- | ----- | --------- | ---------- | --------- | ----------- | ----- |");
    for len in SIZES {
        probe_splice("spare", len, reply_with_spare_capacity(len));
        probe_splice("exact", len, reply_with_exact_capacity(len));
        probe_iovec("spare", len, reply_with_spare_capacity(len));
        probe_iovec("exact", len, reply_with_exact_capacity(len));
    }
    println!();
}

fn batch_size(len: usize) -> BatchSize {
    if len >= 4096 {
        BatchSize::LargeInput
    } else {
        BatchSize::SmallInput
    }
}

fn bench_frame_reply(c: &mut Criterion) {
    print_allocation_probe();

    let mut group = c.benchmark_group("tcp_frame_reply");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));

    for len in SIZES {
        group.bench_with_input(
            BenchmarkId::new("splice_spare_capacity", len),
            &len,
            |b, &len| {
                b.iter_batched(
                    || reply_with_spare_capacity(len),
                    |mut reply| {
                        frame_by_splice(&mut reply);
                        reply
                    },
                    batch_size(len),
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("splice_exact_capacity", len),
            &len,
            |b, &len| {
                b.iter_batched(
                    || reply_with_exact_capacity(len),
                    |mut reply| {
                        frame_by_splice(&mut reply);
                        reply
                    },
                    batch_size(len),
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("iovec_spare_capacity", len),
            &len,
            |b, &len| {
                b.iter_batched(
                    || reply_with_spare_capacity(len),
                    |reply| {
                        let mut len_buf = [0u8; 2];
                        let written = frame_by_iovec(&mut len_buf, &reply);
                        (reply, written)
                    },
                    batch_size(len),
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("iovec_exact_capacity", len),
            &len,
            |b, &len| {
                b.iter_batched(
                    || reply_with_exact_capacity(len),
                    |reply| {
                        let mut len_buf = [0u8; 2];
                        let written = frame_by_iovec(&mut len_buf, &reply);
                        (reply, written)
                    },
                    batch_size(len),
                );
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_frame_reply);
criterion_main!(benches);
