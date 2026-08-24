use std::cell::Cell;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use fah_dns::{
    pack, record, select, unpack, Candidate, Health, Outcome, Policy, Selected, State, Transition,
};

const NOW_MS: u64 = 1_000_000_000;
const FUTURE_MS: u64 = NOW_MS + 60_000;
const PAST_MS: u64 = NOW_MS - 1;

const UPSTREAM_TIMEOUT_MS: u64 = 800;
const POLICY: Policy = Policy::from_timeout(UPSTREAM_TIMEOUT_MS, 3);

const RACERS: usize = 4;

fn endpoints(count: usize) -> Vec<Health> {
    (0..count).map(|_| Health::new()).collect()
}

fn penalize_prefix(health: &[Health], count: usize) {
    for (index, endpoint) in health.iter().enumerate().take(count) {
        endpoint.state.store(pack(
            State::Penalized,
            1,
            POLICY.penalty_failures,
            FUTURE_MS + index as u64,
        ));
    }
}

#[inline(never)]
fn select_claim(health: &[Health]) -> Option<Candidate> {
    select(health, 0, || NOW_MS, true)
}

#[inline(never)]
fn select_noop(health: &[Health]) -> Option<Candidate> {
    black_box(health[0].state.load());
    Some(Candidate {
        id: 0,
        selected: Selected::Healthy,
    })
}

#[inline(never)]
fn record_success(health: &Health) -> Transition {
    record(health, Outcome::Success, || NOW_MS, &POLICY)
}

fn clock_reads(health: &[Health]) -> usize {
    let reads = Cell::new(0usize);
    let picked = select(
        health,
        0,
        || {
            reads.set(reads.get() + 1);
            NOW_MS
        },
        true,
    );
    assert!(picked.is_some(), "every arm must select an endpoint");
    reads.get()
}

fn bench_select(c: &mut Criterion) {
    let two = endpoints(2);
    let eight = endpoints(8);
    let first_seven = endpoints(8);
    penalize_prefix(&first_seven, 7);
    let all_eight = endpoints(8);
    penalize_prefix(&all_eight, 8);

    assert_eq!(clock_reads(&two), 0, "M.1 must not evaluate the clock");
    assert_eq!(clock_reads(&eight), 0, "M.2 must not evaluate the clock");
    assert_eq!(
        clock_reads(&first_seven),
        1,
        "M.3 must evaluate the clock exactly once"
    );
    assert_eq!(
        clock_reads(&all_eight),
        1,
        "M.4 must evaluate the clock exactly once"
    );
    assert_eq!(
        select_claim(&first_seven).unwrap().selected,
        Selected::Healthy
    );
    assert_eq!(select_claim(&all_eight).unwrap().selected, Selected::Forced);

    let mut group = c.benchmark_group("select");
    group.bench_function("2_healthy", |b| {
        b.iter(|| select_claim(black_box(&two)));
    });
    group.bench_function("8_healthy", |b| {
        b.iter(|| select_claim(black_box(&eight)));
    });
    group.bench_function("8_first_7_penalized", |b| {
        b.iter(|| select_claim(black_box(&first_seven)));
    });
    group.bench_function("8_all_penalized", |b| {
        b.iter(|| select_claim(black_box(&all_eight)));
    });
    group.bench_function("noop", |b| {
        b.iter(|| select_noop(black_box(&two)));
    });
    group.finish();
}

fn bench_update(c: &mut Criterion) {
    let health = Health::new();
    let before = health.state.load();
    assert_eq!(record_success(&health), Transition::NoChange);
    assert_eq!(
        health.state.load(),
        before,
        "M.5 must not store on a clean Healthy word"
    );

    let mut group = c.benchmark_group("update");
    group.bench_function("success", |b| {
        b.iter(|| record_success(black_box(&health)));
    });
    group.finish();
}

fn bench_transition(c: &mut Criterion) {
    let health = Health::new();
    let armed = pack(State::Healthy, 0, POLICY.penalty_failures - 1, NOW_MS);
    health.state.store(armed);
    assert_eq!(
        unpack(
            match record(&health, Outcome::HardFailure, || NOW_MS, &POLICY) {
                Transition::Store { word, .. } => word,
                Transition::NoChange => panic!("M.6 must cross the penalty threshold"),
            }
        )
        .state,
        State::Penalized
    );

    let mut group = c.benchmark_group("transition");
    group.bench_function("penalize", |b| {
        b.iter_batched(
            || health.state.store(armed),
            |()| record(black_box(&health), Outcome::HardFailure, || NOW_MS, &POLICY),
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

struct Race {
    health: Vec<Health>,
    start: Barrier,
    done: Barrier,
    winners: AtomicUsize,
    racing: AtomicBool,
    stop: AtomicBool,
}

impl Race {
    fn new() -> Self {
        Self {
            health: endpoints(1),
            start: Barrier::new(RACERS + 1),
            done: Barrier::new(RACERS + 1),
            winners: AtomicUsize::new(0),
            racing: AtomicBool::new(true),
            stop: AtomicBool::new(false),
        }
    }

    fn round(&self) -> usize {
        self.health[0]
            .state
            .store(pack(State::Penalized, 1, POLICY.penalty_failures, PAST_MS));
        self.winners.store(0, Ordering::Relaxed);
        self.start.wait();
        self.done.wait();
        self.winners.load(Ordering::Relaxed)
    }
}

fn spawn_racers(race: &Arc<Race>) -> Vec<thread::JoinHandle<()>> {
    (0..RACERS)
        .map(|_| {
            let race = Arc::clone(race);
            thread::spawn(move || loop {
                race.start.wait();
                if race.stop.load(Ordering::Relaxed) {
                    return;
                }
                if race.racing.load(Ordering::Relaxed) {
                    if let Some(candidate) = select(&race.health, 0, || NOW_MS, true) {
                        if candidate.selected == Selected::Probe {
                            race.winners.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                race.done.wait();
            })
        })
        .collect()
}

fn bench_claim_probe(c: &mut Criterion) {
    let race = Arc::new(Race::new());
    let racers = spawn_racers(&race);

    assert_eq!(race.round(), 1, "M.7 must produce exactly one claim");

    let mut group = c.benchmark_group("transition");
    group.bench_function("claim_probe", |b| {
        b.iter(|| assert_eq!(race.round(), 1));
    });
    race.racing.store(false, Ordering::Relaxed);
    group.bench_function("claim_probe_harness_only", |b| {
        b.iter(|| assert_eq!(race.round(), 0));
    });
    group.finish();

    race.stop.store(true, Ordering::Relaxed);
    race.start.wait();
    for racer in racers {
        racer.join().unwrap();
    }
}

criterion_group!(
    benches,
    bench_select,
    bench_update,
    bench_transition,
    bench_claim_probe
);
criterion_main!(benches);
