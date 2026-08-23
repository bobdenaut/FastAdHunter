use std::io;
use std::sync::atomic::{AtomicU64, Ordering};

use hickory_proto::op::Message;

use super::ATTEMPT_LEGS;

const STATE_MASK: u64 = 0b11;
const ROUND_SHIFT: u32 = 2;
const ROUND_MASK: u64 = 0b1111;
const ROUND_MAX: u8 = 15;
const CF_SHIFT: u32 = 6;
const CF_MASK: u64 = 0xFF;
const TS_SHIFT: u32 = 14;
const TS_MASK: u64 = (1 << 50) - 1;

const PENALTY_MAX_MS: u64 = 300_000;
const PENALTY_BASE_FACTOR: u64 = 10;

#[derive(Debug, Default)]
pub struct PackedWord(AtomicU64);

impl PackedWord {
    pub fn load(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn store(&self, word: u64) {
        self.0.store(word, Ordering::Relaxed);
    }

    pub fn compare_exchange_weak(&self, current: u64, new: u64) -> Result<u64, u64> {
        self.0
            .compare_exchange_weak(current, new, Ordering::Relaxed, Ordering::Relaxed)
    }
}

#[repr(align(64))]
#[derive(Debug, Default)]
pub struct Health {
    pub state: PackedWord,
    pub attempts: AtomicU64,
    pub failures: AtomicU64,
    pub penalties: AtomicU64,
    pub probes: AtomicU64,
    pub probe_successes: AtomicU64,
    pub penalized_ms_total: AtomicU64,
}

const _: () = assert!(std::mem::size_of::<Health>() == 64);

impl Health {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Healthy = 0,
    Penalized = 1,
    Probing = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word {
    pub state: State,
    pub penalty_round: u8,
    pub consecutive_failures: u8,
    pub timestamp_ms: u64,
}

impl Word {
    pub fn deadline_passed(self, now_ms: u64) -> bool {
        self.timestamp_ms <= now_ms
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success,
    HardFailure,
    PathFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportKind {
    Plain,
    Encrypted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthMode {
    Record,
    Ignore,
}

pub fn classify(transport: TransportKind, result: &io::Result<Message>) -> Outcome {
    let Err(err) = result else {
        return Outcome::Success;
    };
    match err.kind() {
        io::ErrorKind::ConnectionRefused
        | io::ErrorKind::NetworkUnreachable
        | io::ErrorKind::HostUnreachable => Outcome::PathFailure,
        io::ErrorKind::InvalidData => match transport {
            TransportKind::Encrypted => Outcome::PathFailure,
            TransportKind::Plain => Outcome::HardFailure,
        },
        _ => Outcome::HardFailure,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    pub penalty_failures: u8,
    pub penalty_base_ms: u64,
    pub penalty_max_ms: u64,
}

impl Policy {
    pub fn from_timeout(timeout_ms: u64, penalty_failures: u8) -> Self {
        let attempt_bound_ms = u64::from(ATTEMPT_LEGS) * timeout_ms;
        Self {
            penalty_failures,
            penalty_base_ms: PENALTY_BASE_FACTOR * attempt_bound_ms,
            penalty_max_ms: PENALTY_MAX_MS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    NoChange,
    Store {
        word: u64,
        closed_run: Option<u8>,
        penalty_applied: Option<u64>,
    },
}

pub fn pack(state: State, penalty_round: u8, consecutive_failures: u8, timestamp_ms: u64) -> u64 {
    (state as u64)
        | ((u64::from(penalty_round) & ROUND_MASK) << ROUND_SHIFT)
        | ((u64::from(consecutive_failures) & CF_MASK) << CF_SHIFT)
        | ((timestamp_ms & TS_MASK) << TS_SHIFT)
}

pub fn unpack(word: u64) -> Word {
    let bits = word & STATE_MASK;
    debug_assert!(bits != 3, "state bits 3 must never be written");
    let state = match bits {
        1 => State::Penalized,
        2 => State::Probing,
        _ => State::Healthy,
    };
    Word {
        state,
        penalty_round: u8::try_from((word >> ROUND_SHIFT) & ROUND_MASK).unwrap_or(0),
        consecutive_failures: u8::try_from((word >> CF_SHIFT) & CF_MASK).unwrap_or(0),
        timestamp_ms: word >> TS_SHIFT,
    }
}

pub fn penalty(round: u8, now_ms: u64, policy: &Policy) -> u64 {
    debug_assert!(round >= 1, "penalty is defined for round >= 1 only");
    let nominal = nominal_penalty(round, policy);
    let bits = now_ms & 0xFF;
    let percent = 75 + bits * 50 / 255;
    u64::try_from(u128::from(nominal) * u128::from(percent) / 100).unwrap_or(u64::MAX)
}

fn nominal_penalty(round: u8, policy: &Policy) -> u64 {
    policy
        .penalty_base_ms
        .checked_shl(u32::from(round.saturating_sub(1)))
        .unwrap_or(u64::MAX)
        .min(policy.penalty_max_ms)
}

fn next_round(w: Word, now_ms: u64, policy: &Policy) -> u8 {
    if w.state == State::Healthy && now_ms.saturating_sub(w.timestamp_ms) >= policy.penalty_max_ms {
        1
    } else {
        w.penalty_round.saturating_add(1).min(ROUND_MAX)
    }
}

fn penalize(w: Word, consecutive_failures: u8, now_ms: u64, policy: &Policy) -> Transition {
    let round = next_round(w, now_ms, policy);
    let deadline = now_ms.saturating_add(penalty(round, now_ms, policy));
    Transition::Store {
        word: pack(State::Penalized, round, consecutive_failures, deadline),
        closed_run: None,
        penalty_applied: Some(nominal_penalty(round, policy)),
    }
}

struct LazyNow<F> {
    clock: Option<F>,
    sampled: Option<u64>,
}

impl<F: FnOnce() -> u64> LazyNow<F> {
    fn new(clock: F) -> Self {
        Self {
            clock: Some(clock),
            sampled: None,
        }
    }

    fn get(&mut self) -> u64 {
        match self.sampled {
            Some(now_ms) => now_ms,
            None => {
                let now_ms = self.clock.take().expect("clock sampled at most once")();
                self.sampled = Some(now_ms);
                now_ms
            }
        }
    }
}

pub fn next_word(word: u64, outcome: Outcome, now_ms: u64, policy: &Policy) -> Transition {
    next_word_lazy(word, outcome, &mut LazyNow::new(|| now_ms), policy)
}

fn next_word_lazy<F: FnOnce() -> u64>(
    word: u64,
    outcome: Outcome,
    now: &mut LazyNow<F>,
    policy: &Policy,
) -> Transition {
    let w = unpack(word);
    match (w.state, outcome) {
        (State::Healthy, Outcome::Success) => {
            if w.consecutive_failures == 0 {
                Transition::NoChange
            } else {
                Transition::Store {
                    word: pack(State::Healthy, w.penalty_round, 0, w.timestamp_ms),
                    closed_run: Some(w.consecutive_failures),
                    penalty_applied: None,
                }
            }
        }
        (State::Penalized | State::Probing, Outcome::Success) => Transition::Store {
            word: pack(State::Healthy, w.penalty_round, 0, now.get()),
            closed_run: (w.consecutive_failures > 0).then_some(w.consecutive_failures),
            penalty_applied: None,
        },
        (State::Healthy, Outcome::HardFailure) => {
            let cf = w.consecutive_failures.saturating_add(1);
            if cf >= policy.penalty_failures {
                penalize(w, cf, now.get(), policy)
            } else {
                Transition::Store {
                    word: pack(State::Healthy, w.penalty_round, cf, w.timestamp_ms),
                    closed_run: None,
                    penalty_applied: None,
                }
            }
        }
        (State::Healthy, Outcome::PathFailure) => penalize(
            w,
            w.consecutive_failures.saturating_add(1),
            now.get(),
            policy,
        ),
        (State::Penalized, Outcome::HardFailure | Outcome::PathFailure) => {
            let cf = w.consecutive_failures.saturating_add(1);
            if cf == w.consecutive_failures {
                Transition::NoChange
            } else {
                Transition::Store {
                    word: pack(State::Penalized, w.penalty_round, cf, w.timestamp_ms),
                    closed_run: None,
                    penalty_applied: None,
                }
            }
        }
        (State::Probing, Outcome::HardFailure | Outcome::PathFailure) => penalize(
            w,
            w.consecutive_failures.saturating_add(1),
            now.get(),
            policy,
        ),
    }
}

pub fn record<F: FnOnce() -> u64>(
    health: &Health,
    outcome: Outcome,
    now: F,
    policy: &Policy,
) -> Transition {
    let mut now = LazyNow::new(now);
    let mut old = health.state.load();
    loop {
        let transition = next_word_lazy(old, outcome, &mut now, policy);
        let Transition::Store { word, .. } = transition else {
            return transition;
        };
        match health.state.compare_exchange_weak(old, word) {
            Ok(_) => return transition,
            Err(current) => old = current,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selected {
    Healthy,
    Probe,
    Forced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: u8,
    pub selected: Selected,
}

pub fn select(
    health: &[Health],
    start: usize,
    now: impl FnOnce() -> u64,
    claim: bool,
) -> Option<Candidate> {
    let mut clock = Some(now);
    let mut now_ms: Option<u64> = None;
    let mut forced: Option<(u8, u64)> = None;

    for (index, endpoint) in health.iter().enumerate().skip(start) {
        let id = u8::try_from(index).unwrap_or(u8::MAX);
        let word = endpoint.state.load();
        let w = unpack(word);
        match w.state {
            State::Healthy => {
                return Some(Candidate {
                    id,
                    selected: Selected::Healthy,
                })
            }
            State::Penalized if claim => {
                let sampled = *now_ms.get_or_insert_with(|| {
                    clock.take().expect("clock evaluated at most once per pass")()
                });
                if w.deadline_passed(sampled) {
                    let probing = pack(
                        State::Probing,
                        w.penalty_round,
                        w.consecutive_failures,
                        w.timestamp_ms,
                    );
                    if endpoint.state.compare_exchange_weak(word, probing).is_ok() {
                        return Some(Candidate {
                            id,
                            selected: Selected::Probe,
                        });
                    }
                }
            }
            State::Penalized => {}
            State::Probing => {}
        }
        if forced.is_none_or(|(_, earliest)| w.timestamp_ms < earliest) {
            forced = Some((id, w.timestamp_ms));
        }
    }

    forced.map(|(id, _)| Candidate {
        id,
        selected: Selected::Forced,
    })
}

pub(crate) struct ProbeGuard<'a> {
    health: &'a Health,
    claimed: u64,
    armed: bool,
}

impl<'a> ProbeGuard<'a> {
    pub(crate) fn new(health: &'a Health) -> Self {
        Self {
            health,
            claimed: health.state.load(),
            armed: true,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProbeGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let w = unpack(self.claimed);
        if w.state != State::Probing {
            return;
        }
        let restored = pack(
            State::Penalized,
            w.penalty_round,
            w.consecutive_failures,
            w.timestamp_ms,
        );
        let mut current = self.claimed;
        while current == self.claimed {
            match self.health.state.compare_exchange_weak(current, restored) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const fn policy(penalty_failures: u8, penalty_base_ms: u64, penalty_max_ms: u64) -> Policy {
        Policy {
            penalty_failures,
            penalty_base_ms,
            penalty_max_ms,
        }
    }

    fn assert_in_jitter_band(value: u64, nominal: u64) {
        assert!(
            value >= nominal * 75 / 100,
            "{value} below band of {nominal}"
        );
        assert!(
            value <= nominal * 125 / 100,
            "{value} above band of {nominal}"
        );
    }

    fn store(transition: Transition) -> (Word, Option<u8>, Option<u64>) {
        match transition {
            Transition::Store {
                word,
                closed_run,
                penalty_applied,
            } => (unpack(word), closed_run, penalty_applied),
            Transition::NoChange => panic!("expected Transition::Store"),
        }
    }

    #[test]
    fn new_health_is_healthy_at_epoch() {
        let health = Health::new();
        let w = unpack(health.state.load());
        assert_eq!(
            w,
            Word {
                state: State::Healthy,
                penalty_round: 0,
                consecutive_failures: 0,
                timestamp_ms: 0,
            }
        );
    }

    #[test]
    fn packed_word_round_trips() {
        let words = [
            pack(State::Healthy, 0, 0, 0),
            pack(State::Penalized, 15, 255, (1 << 50) - 1),
            pack(State::Probing, 7, 42, 123_456),
            pack(State::Penalized, 1, 2, 3),
        ];
        for word in words {
            let w = unpack(word);
            assert_eq!(
                pack(
                    w.state,
                    w.penalty_round,
                    w.consecutive_failures,
                    w.timestamp_ms
                ),
                word
            );
        }
        let w = unpack(pack(State::Penalized, 15, 255, (1 << 50) - 1));
        assert_eq!(w.state, State::Penalized);
        assert_eq!(w.penalty_round, 15);
        assert_eq!(w.consecutive_failures, 255);
        assert_eq!(w.timestamp_ms, (1 << 50) - 1);
    }

    #[test]
    fn pack_masks_timestamp_to_fifty_bits() {
        let w = unpack(pack(State::Healthy, 0, 0, u64::MAX));
        assert_eq!(w.timestamp_ms, (1 << 50) - 1);
        assert_eq!(w.state, State::Healthy);
        assert_eq!(w.penalty_round, 0);
        assert_eq!(w.consecutive_failures, 0);
    }

    #[test]
    fn unpack_maps_state_bits_three_to_healthy() {
        if cfg!(debug_assertions) {
            return;
        }
        assert_eq!(unpack(0b11).state, State::Healthy);
    }

    #[test]
    fn deadline_passes_at_exactly_now() {
        let w = unpack(pack(State::Penalized, 1, 0, 100));
        assert!(w.deadline_passed(100));
        assert!(w.deadline_passed(101));
        assert!(!w.deadline_passed(99));
    }

    #[test]
    fn policy_from_timeout_derives_base_from_attempt_bound() {
        let p = Policy::from_timeout(800, 2);
        assert_eq!(p.penalty_failures, 2);
        assert_eq!(p.penalty_base_ms, 24_000);
        assert_eq!(p.penalty_max_ms, 300_000);
    }

    #[test]
    fn healthy_success_with_no_failures_stores_nothing() {
        for p in [policy(1, 7, 300), policy(2, 1_000, 16_000)] {
            let health = Health::new();
            health.state.store(pack(State::Healthy, 3, 0, 77));
            let before = health.state.load();
            assert_eq!(
                record(&health, Outcome::Success, || 42, &p),
                Transition::NoChange
            );
            assert_eq!(health.state.load(), before);
        }
    }

    #[test]
    fn healthy_success_clears_failures_and_closes_run() {
        for p in [policy(4, 1_000, 16_000), policy(2, 40, 640)] {
            let t = next_word(pack(State::Healthy, 2, 3, 77), Outcome::Success, 999, &p);
            let (w, closed_run, penalty_applied) = store(t);
            assert_eq!(closed_run, Some(3));
            assert_eq!(penalty_applied, None);
            assert_eq!(w.state, State::Healthy);
            assert_eq!(w.consecutive_failures, 0);
            assert_eq!(w.penalty_round, 2);
            assert_eq!(w.timestamp_ms, 77);
        }
    }

    #[test]
    fn healthy_hard_failure_below_threshold_counts_only() {
        for p in [policy(2, 1_000, 16_000), policy(255, 40, 640)] {
            let t = next_word(
                pack(State::Healthy, 1, 0, 50),
                Outcome::HardFailure,
                9_999,
                &p,
            );
            let (w, closed_run, penalty_applied) = store(t);
            assert_eq!(closed_run, None);
            assert_eq!(penalty_applied, None);
            assert_eq!(w.state, State::Healthy);
            assert_eq!(w.consecutive_failures, 1);
            assert_eq!(w.penalty_round, 1);
            assert_eq!(w.timestamp_ms, 50);
        }
    }

    #[test]
    fn healthy_hard_failure_at_threshold_penalizes() {
        for p in [policy(2, 1_000, 16_000), policy(2, 500, 8_000)] {
            let health = Health::new();
            health.state.store(pack(State::Healthy, 0, 1, 0));
            let now = 5_000;
            let t = record(&health, Outcome::HardFailure, || now, &p);
            let (w, closed_run, penalty_applied) = store(t);
            let Transition::Store { word, .. } = t else {
                unreachable!()
            };
            assert_eq!(word, health.state.load());
            assert_eq!(closed_run, None);
            assert_eq!(penalty_applied, Some(p.penalty_base_ms));
            assert_eq!(w.state, State::Penalized);
            assert_eq!(w.consecutive_failures, 2);
            assert_eq!(w.penalty_round, 1);
            assert!(w.timestamp_ms > now);
            assert_in_jitter_band(w.timestamp_ms - now, p.penalty_base_ms);
        }
    }

    #[test]
    fn healthy_path_failure_penalizes_immediately() {
        let p = policy(5, 1_000, 16_000);
        let now = 200;
        let t = next_word(pack(State::Healthy, 0, 0, 0), Outcome::PathFailure, now, &p);
        let (w, closed_run, penalty_applied) = store(t);
        assert_eq!(closed_run, None);
        assert_eq!(penalty_applied, Some(1_000));
        assert_eq!(w.state, State::Penalized);
        assert_eq!(w.consecutive_failures, 1);
        assert_eq!(w.penalty_round, 1);
        assert_in_jitter_band(w.timestamp_ms - now, 1_000);
    }

    #[test]
    fn penalized_success_restores_healthy_round_kept() {
        for p in [policy(2, 1_000, 16_000), policy(3, 40, 640)] {
            let t = next_word(pack(State::Penalized, 3, 5, 500), Outcome::Success, 900, &p);
            let (w, closed_run, penalty_applied) = store(t);
            assert_eq!(closed_run, Some(5));
            assert_eq!(penalty_applied, None);
            assert_eq!(w.state, State::Healthy);
            assert_eq!(w.consecutive_failures, 0);
            assert_eq!(w.penalty_round, 3);
            assert_eq!(w.timestamp_ms, 900);
        }
    }

    #[test]
    fn penalized_failure_leaves_deadline_and_round() {
        let p = policy(2, 1_000, 16_000);
        for outcome in [Outcome::HardFailure, Outcome::PathFailure] {
            let t = next_word(pack(State::Penalized, 3, 5, 500), outcome, 999, &p);
            let (w, closed_run, penalty_applied) = store(t);
            assert_eq!(closed_run, None);
            assert_eq!(penalty_applied, None);
            assert_eq!(w.state, State::Penalized);
            assert_eq!(w.consecutive_failures, 6);
            assert_eq!(w.penalty_round, 3);
            assert_eq!(w.timestamp_ms, 500);
        }
    }

    #[test]
    fn penalized_failure_at_saturation_changes_nothing() {
        let p = policy(2, 1_000, 16_000);
        for outcome in [Outcome::HardFailure, Outcome::PathFailure] {
            assert_eq!(
                next_word(pack(State::Penalized, 3, 255, 500), outcome, 999, &p),
                Transition::NoChange
            );
        }
    }

    #[test]
    fn probing_success_restores_healthy() {
        let p = policy(2, 1_000, 16_000);
        let t = next_word(pack(State::Probing, 4, 7, 300), Outcome::Success, 350, &p);
        let (w, closed_run, penalty_applied) = store(t);
        assert_eq!(closed_run, Some(7));
        assert_eq!(penalty_applied, None);
        assert_eq!(w.state, State::Healthy);
        assert_eq!(w.consecutive_failures, 0);
        assert_eq!(w.penalty_round, 4);
        assert_eq!(w.timestamp_ms, 350);
    }

    #[test]
    fn probing_failure_penalizes_deeper() {
        let p = policy(2, 1_000, 16_000);
        let old_deadline = 300;
        let now = 400;
        let t = next_word(
            pack(State::Probing, 2, 4, old_deadline),
            Outcome::HardFailure,
            now,
            &p,
        );
        let (w, closed_run, penalty_applied) = store(t);
        assert_eq!(closed_run, None);
        assert_eq!(penalty_applied, Some(4_000));
        assert_eq!(w.state, State::Penalized);
        assert_eq!(w.consecutive_failures, 5);
        assert_eq!(w.penalty_round, 3);
        assert!(w.timestamp_ms > old_deadline);
        assert_in_jitter_band(w.timestamp_ms - now, 4_000);
    }

    #[test]
    fn probing_failure_saturates_round_and_failures() {
        let p = policy(2, 1_000, 16_000);
        let t = next_word(
            pack(State::Probing, 15, 255, 10),
            Outcome::PathFailure,
            20,
            &p,
        );
        let (w, closed_run, penalty_applied) = store(t);
        assert_eq!(closed_run, None);
        assert_eq!(penalty_applied, Some(16_000));
        assert_eq!(w.state, State::Penalized);
        assert_eq!(w.consecutive_failures, 255);
        assert_eq!(w.penalty_round, 15);
        assert_in_jitter_band(w.timestamp_ms - 20, 16_000);
    }

    #[test]
    fn next_round_resets_only_after_continuous_healthy_max() {
        let p = policy(2, 1_000, 16_000);
        let healthy = Word {
            state: State::Healthy,
            penalty_round: 5,
            consecutive_failures: 0,
            timestamp_ms: 1_000,
        };
        assert_eq!(next_round(healthy, 1_000 + 15_999, &p), 6);
        assert_eq!(next_round(healthy, 1_000 + 16_000, &p), 1);
        let sampled_ahead = Word {
            timestamp_ms: 2_001,
            ..healthy
        };
        assert_eq!(next_round(sampled_ahead, 2_000, &p), 6);
        let saturated = Word {
            penalty_round: 15,
            ..healthy
        };
        assert_eq!(next_round(saturated, 1_500, &p), 15);
    }

    #[test]
    fn penalty_doubles_per_round_and_caps() {
        let p = policy(2, 1_000, 16_000);
        for now in [0_u64, 100, 255] {
            let mut nominal = 1_000_u64;
            for round in 1..=15_u8 {
                let expected = nominal.min(16_000);
                assert_in_jitter_band(penalty(round, now, &p), expected);
                nominal = nominal.saturating_mul(2);
            }
        }
    }

    #[test]
    fn penalty_jitter_spans_the_band_from_the_low_bits() {
        let p = policy(2, 1_000, 16_000);
        assert_eq!(penalty(1, 0, &p), 750);
        assert_eq!(penalty(1, 255, &p), 1_250);
        assert_eq!(penalty(1, 256, &p), 750);
    }

    #[test]
    fn concurrent_saturated_failures_leave_word_intact() {
        let p = policy(2, 1_000, 16_000);
        let health = Health::new();
        let initial = pack(State::Penalized, 3, 255, 12_345);
        health.state.store(initial);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..1_000 {
                        record(&health, Outcome::HardFailure, || 99_999, &p);
                    }
                });
            }
        });
        let w = unpack(health.state.load());
        assert_eq!(w.state, State::Penalized);
        assert_eq!(w.penalty_round, 3);
        assert_eq!(w.consecutive_failures, 255);
        assert_eq!(w.timestamp_ms, 12_345);
    }

    #[test]
    fn contended_stores_never_corrupt_timestamp_or_round() {
        let p = policy(255, 1_000, 16_000);
        let health = Health::new();
        let initial = pack(State::Healthy, 9, 1, 12_345);
        health.state.store(initial);
        const THREADS: u8 = 8;
        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                scope.spawn(|| {
                    for _ in 0..1_000 {
                        record(&health, Outcome::HardFailure, || 99_999, &p);
                        record(&health, Outcome::Success, || 99_999, &p);
                    }
                });
            }
        });
        let w = unpack(health.state.load());
        assert_eq!(w.state, State::Healthy);
        assert_eq!(w.penalty_round, 9);
        assert_eq!(w.timestamp_ms, 12_345);
        assert!(w.consecutive_failures <= THREADS);
    }

    #[test]
    fn contended_failures_saturate_exactly_once() {
        let p = policy(255, 1_000, 16_000);
        let health = Health::new();
        health.state.store(pack(State::Healthy, 4, 250, 777));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..1_000 {
                        record(&health, Outcome::HardFailure, || 5_000, &p);
                    }
                });
            }
        });
        let w = unpack(health.state.load());
        assert_eq!(w.state, State::Penalized);
        assert_eq!(w.consecutive_failures, 255);
        assert_eq!(w.penalty_round, 5);
        assert_in_jitter_band(w.timestamp_ms - 5_000, 16_000);
    }

    #[test]
    fn sequential_probe_failures_keep_round_saturated() {
        let p = policy(2, 1_000, 16_000);
        let health = Health::new();
        health.state.store(pack(State::Penalized, 15, 255, 10));
        for _ in 0..16 {
            let w = unpack(health.state.load());
            health.state.store(pack(
                State::Probing,
                w.penalty_round,
                w.consecutive_failures,
                w.timestamp_ms,
            ));
            record(&health, Outcome::HardFailure, || 1_000, &p);
            let after = unpack(health.state.load());
            assert_eq!(after.state, State::Penalized);
            assert_eq!(after.penalty_round, 15);
            assert_eq!(after.consecutive_failures, 255);
        }
    }

    fn endpoints<const N: usize>() -> [Health; N] {
        std::array::from_fn(|_| Health::new())
    }

    fn snapshot(health: &Health) -> [u64; 7] {
        [
            health.state.load(),
            health.attempts.load(Ordering::Relaxed),
            health.failures.load(Ordering::Relaxed),
            health.penalties.load(Ordering::Relaxed),
            health.probes.load(Ordering::Relaxed),
            health.probe_successes.load(Ordering::Relaxed),
            health.penalized_ms_total.load(Ordering::Relaxed),
        ]
    }

    fn counted_clock(now_ms: u64, calls: &Cell<u32>) -> impl FnOnce() -> u64 + '_ {
        move || {
            calls.set(calls.get() + 1);
            now_ms
        }
    }

    fn healthy(id: u8) -> Option<Candidate> {
        Some(Candidate {
            id,
            selected: Selected::Healthy,
        })
    }

    fn probe(id: u8) -> Option<Candidate> {
        Some(Candidate {
            id,
            selected: Selected::Probe,
        })
    }

    fn forced(id: u8) -> Option<Candidate> {
        Some(Candidate {
            id,
            selected: Selected::Forced,
        })
    }

    #[test]
    fn boot_selects_the_first_endpoint_and_never_reads_the_clock() {
        let calls = Cell::new(0);
        let two: [Health; 2] = endpoints();
        let eight: [Health; 8] = endpoints();
        for _ in 0..4 {
            assert_eq!(
                select(&two, 0, counted_clock(1_000, &calls), true),
                healthy(0)
            );
            assert_eq!(
                select(&eight, 0, counted_clock(1_000, &calls), true),
                healthy(0)
            );
        }
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn first_healthy_behind_penalized_reads_the_clock_once() {
        let health: [Health; 4] = endpoints();
        for endpoint in &health[..3] {
            endpoint.state.store(pack(State::Penalized, 1, 1, 5_000));
        }
        let calls = Cell::new(0);
        assert_eq!(
            select(&health, 0, counted_clock(100, &calls), true),
            healthy(3)
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn due_endpoint_ahead_of_a_healthy_one_is_probed() {
        let health: [Health; 2] = endpoints();
        health[0].state.store(pack(State::Penalized, 3, 7, 100));
        let before = snapshot(&health[1]);
        assert_eq!(claim_probe(&health, 100), probe(0));
        let w = unpack(health[0].state.load());
        assert_eq!(w.state, State::Probing);
        assert_eq!(w.penalty_round, 3);
        assert_eq!(w.consecutive_failures, 7);
        assert_eq!(w.timestamp_ms, 100);
        assert_eq!(health[0].probes.load(Ordering::Relaxed), 0);
        assert_eq!(snapshot(&health[1]), before);
    }

    #[test]
    fn due_endpoint_behind_a_healthy_one_is_not_reached() {
        let health: [Health; 2] = endpoints();
        health[1].state.store(pack(State::Penalized, 2, 4, 100));
        let before = snapshot(&health[1]);
        let calls = Cell::new(0);
        assert_eq!(
            select(&health, 0, counted_clock(999, &calls), true),
            healthy(0)
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(snapshot(&health[1]), before);
    }

    #[test]
    fn all_penalized_forces_the_earliest_deadline_and_stores_nothing() {
        let health: [Health; 3] = endpoints();
        health[0].state.store(pack(State::Penalized, 2, 3, 900));
        health[1].state.store(pack(State::Penalized, 1, 1, 400));
        health[2].state.store(pack(State::Penalized, 4, 9, 700));
        let before: [[u64; 7]; 3] = std::array::from_fn(|i| snapshot(&health[i]));

        assert_eq!(select(&health, 0, || 100, true), forced(1));
        for (index, expected) in before.iter().enumerate() {
            assert_eq!(&snapshot(&health[index]), expected);
        }

        let p = policy(2, 1_000, 16_000);
        record(&health[1], Outcome::HardFailure, || 100, &p);
        let w = unpack(health[1].state.load());
        assert_eq!(w.state, State::Penalized);
        assert_eq!(w.penalty_round, 1);
        assert_eq!(w.timestamp_ms, 400);
        assert_eq!(w.consecutive_failures, 2);
    }

    #[test]
    fn forced_candidate_may_be_a_probing_endpoint() {
        let health: [Health; 2] = endpoints();
        health[0].state.store(pack(State::Penalized, 1, 1, 900));
        health[1].state.store(pack(State::Probing, 2, 5, 400));
        let before: [[u64; 7]; 2] = std::array::from_fn(|i| snapshot(&health[i]));
        assert_eq!(select(&health, 0, || 100, true), forced(1));
        for (index, expected) in before.iter().enumerate() {
            assert_eq!(&snapshot(&health[index]), expected);
        }
    }

    #[test]
    fn exactly_one_thread_claims_the_probe_with_no_other_endpoint() {
        let health: [Health; 1] = endpoints();
        let results = race_for_the_claim(&health, pack(State::Penalized, 1, 2, 500), 500);
        assert_eq!(results.iter().filter(|c| **c == probe(0)).count(), 1);
        assert_eq!(results.iter().filter(|c| **c == forced(0)).count(), 7);
        let w = unpack(health[0].state.load());
        assert_eq!(w.state, State::Probing);
        assert_eq!(w.penalty_round, 1);
        assert_eq!(w.consecutive_failures, 2);
        assert_eq!(w.timestamp_ms, 500);
    }

    #[test]
    fn exactly_one_thread_claims_the_probe_ahead_of_a_healthy_endpoint() {
        let health: [Health; 2] = endpoints();
        let results = race_for_the_claim(&health, pack(State::Penalized, 1, 2, 500), 500);
        assert_eq!(results.iter().filter(|c| **c == probe(0)).count(), 1);
        assert_eq!(results.iter().filter(|c| **c == healthy(1)).count(), 7);
        assert_eq!(unpack(health[0].state.load()).state, State::Probing);
    }

    const CLAIM_ROUNDS: u32 = 8;

    fn claim_probe(health: &[Health], now_ms: u64) -> Option<Candidate> {
        for _ in 0..CLAIM_ROUNDS {
            let candidate = select(health, 0, || now_ms, true);
            if candidate.is_some_and(|c| c.selected == Selected::Probe) {
                return candidate;
            }
        }
        panic!("no probe claimed in {CLAIM_ROUNDS} passes");
    }

    fn race_for_the_claim(health: &[Health], word: u64, now_ms: u64) -> Vec<Option<Candidate>> {
        for _ in 0..CLAIM_ROUNDS {
            health[0].state.store(word);
            let results = race(health, now_ms);
            if results.iter().any(|c| *c == probe(0)) {
                return results;
            }
        }
        panic!("no thread claimed the probe in {CLAIM_ROUNDS} rounds");
    }

    fn race(health: &[Health], now_ms: u64) -> Vec<Option<Candidate>> {
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(move || select(health, 0, move || now_ms, true)))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("selector thread panicked"))
                .collect()
        })
    }

    #[test]
    fn without_claim_a_due_endpoint_is_skipped_and_untouched() {
        let health: [Health; 2] = endpoints();
        health[0].state.store(pack(State::Penalized, 3, 7, 100));
        let before = snapshot(&health[0]);
        assert_eq!(select(&health, 0, || 100, false), healthy(1));
        assert_eq!(snapshot(&health[0]), before);

        let lone: [Health; 1] = endpoints();
        lone[0].state.store(pack(State::Penalized, 3, 7, 100));
        let before = snapshot(&lone[0]);
        assert_eq!(select(&lone, 0, || 100, false), forced(0));
        assert_eq!(snapshot(&lone[0]), before);

        let both: [Health; 2] = endpoints();
        both[0].state.store(pack(State::Penalized, 1, 1, 300));
        both[1].state.store(pack(State::Penalized, 1, 1, 200));
        let before: [[u64; 7]; 2] = std::array::from_fn(|i| snapshot(&both[i]));
        assert_eq!(select(&both, 0, || 1_000, false), forced(1));
        for (index, expected) in before.iter().enumerate() {
            assert_eq!(&snapshot(&both[index]), expected);
        }
    }

    #[test]
    fn a_deadline_equal_to_now_is_due_and_one_millisecond_later_is_not() {
        let due: [Health; 1] = endpoints();
        due[0].state.store(pack(State::Penalized, 1, 1, 1_000));
        assert_eq!(claim_probe(&due, 1_000), probe(0));

        let pending: [Health; 1] = endpoints();
        pending[0].state.store(pack(State::Penalized, 1, 1, 1_001));
        let before = snapshot(&pending[0]);
        assert_eq!(select(&pending, 0, || 1_000, true), forced(0));
        assert_eq!(snapshot(&pending[0]), before);
    }

    #[test]
    fn continuation_starts_after_the_failed_endpoint() {
        let health: [Health; 2] = endpoints();
        assert_eq!(select(&health, 1, || 0, true), healthy(1));
        assert_eq!(select(&health, 2, || 0, true), None);
        assert_eq!(select(&health, 9, || 0, true), None);
        let empty: [Health; 0] = endpoints();
        assert_eq!(select(&empty, 0, || 0, true), None);
    }

    #[test]
    fn a_pass_over_pending_and_probing_words_reaches_the_healthy_one() {
        let health: [Health; 3] = endpoints();
        health[0].state.store(pack(State::Penalized, 1, 1, 9_000));
        health[1].state.store(pack(State::Probing, 2, 2, 8_000));
        let calls = Cell::new(0);
        assert_eq!(
            select(&health, 0, counted_clock(100, &calls), true),
            healthy(2)
        );
        assert_eq!(calls.get(), 1);

        let pair: [Health; 2] = endpoints();
        pair[0].state.store(pack(State::Penalized, 1, 1, 9_000));
        pair[1].state.store(pack(State::Probing, 2, 2, 8_000));
        assert_eq!(select(&pair, 0, || 100, true), forced(1));
    }

    fn answer() -> io::Result<Message> {
        Ok(Message::query())
    }

    fn failure(kind: io::ErrorKind) -> io::Result<Message> {
        Err(io::Error::new(kind, "classification test"))
    }

    #[test]
    fn an_answer_is_a_success_on_both_transports() {
        for transport in [TransportKind::Plain, TransportKind::Encrypted] {
            assert_eq!(classify(transport, &answer()), Outcome::Success);
        }
    }

    #[test]
    fn unreachable_and_refused_are_path_failures_on_both_transports() {
        for kind in [
            io::ErrorKind::ConnectionRefused,
            io::ErrorKind::NetworkUnreachable,
            io::ErrorKind::HostUnreachable,
        ] {
            for transport in [TransportKind::Plain, TransportKind::Encrypted] {
                assert_eq!(
                    classify(transport, &failure(kind)),
                    Outcome::PathFailure,
                    "{kind:?} on {transport:?}"
                );
            }
        }
    }

    #[test]
    fn invalid_data_is_a_path_failure_only_on_the_encrypted_transport() {
        assert_eq!(
            classify(
                TransportKind::Encrypted,
                &failure(io::ErrorKind::InvalidData)
            ),
            Outcome::PathFailure
        );
        assert_eq!(
            classify(TransportKind::Plain, &failure(io::ErrorKind::InvalidData)),
            Outcome::HardFailure
        );
    }

    #[test]
    fn every_other_kind_is_a_hard_failure_on_both_transports() {
        for kind in [
            io::ErrorKind::TimedOut,
            io::ErrorKind::Other,
            io::ErrorKind::NotFound,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::ConnectionAborted,
            io::ErrorKind::AddrNotAvailable,
            io::ErrorKind::InvalidInput,
            io::ErrorKind::OutOfMemory,
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::UnexpectedEof,
        ] {
            for transport in [TransportKind::Plain, TransportKind::Encrypted] {
                assert_eq!(
                    classify(transport, &failure(kind)),
                    Outcome::HardFailure,
                    "{kind:?} on {transport:?}"
                );
            }
        }
    }
}
