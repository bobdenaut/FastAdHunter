//! p2-12 attribution probe: where the compile transient's memory goes.
//!
//! Three counters over the production allocator (mimalloc), deliberately
//! different:
//!   LIVE / PEAK    logical live heap — bytes *requested*, a realloc counted as
//!                  its result.
//!   USABLE / PEAK  the same allocations at the size mimalloc actually hands
//!                  out (`mi_usable_size`). The difference is the size-class
//!                  rounding term, measured rather than modelled.
//!   PEAK_MOVED     LIVE, but a realloc that *moved* (returned pointer differs)
//!                  counts both buffers at the crossover, because they were.

use fah_rules::{parse_rule_list, MatcherBuilder, PolicySet};
use mimalloc::MiMalloc;
use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Arc;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static PEAK_MOVED: AtomicUsize = AtomicUsize::new(0);
static USABLE: AtomicUsize = AtomicUsize::new(0);
static USABLE_PEAK: AtomicUsize = AtomicUsize::new(0);
/// Copying reallocs above 1 MiB — the ones that could matter to a peak.
static MOVES: AtomicUsize = AtomicUsize::new(0);
static MOVED_BYTES: AtomicUsize = AtomicUsize::new(0);
static WORST_MOVE: AtomicUsize = AtomicUsize::new(0);
/// Live allocation count, so the rounding term can be read per allocation.
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static ALLOCS_PEAK: AtomicUsize = AtomicUsize::new(0);

/// SAFETY: `ptr` was returned by mimalloc and is still live.
unsafe fn usable(ptr: *mut u8) -> usize {
    unsafe { libmimalloc_sys::mi_usable_size(ptr.cast()) }
}

struct Counting;

// SAFETY: every method forwards to `MiMalloc`, a sound `GlobalAlloc`. The
// counters are pure side effects: they never touch the returned memory and
// never alter a layout, so the allocator contract is preserved.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { MiMalloc.alloc(layout) };
        if !ptr.is_null() {
            let now = LIVE.fetch_add(layout.size(), Relaxed) + layout.size();
            PEAK.fetch_max(now, Relaxed);
            PEAK_MOVED.fetch_max(now, Relaxed);
            let got = unsafe { usable(ptr) };
            let used = USABLE.fetch_add(got, Relaxed) + got;
            USABLE_PEAK.fetch_max(used, Relaxed);
            let count = ALLOCS.fetch_add(1, Relaxed) + 1;
            ALLOCS_PEAK.fetch_max(count, Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        USABLE.fetch_sub(unsafe { usable(ptr) }, Relaxed);
        ALLOCS.fetch_sub(1, Relaxed);
        unsafe { MiMalloc.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        let was_usable = unsafe { usable(ptr) };
        let out = unsafe { MiMalloc.realloc(ptr, layout, new) };
        if out.is_null() {
            return out;
        }
        if out != ptr {
            // Both buffers were live while the bytes were copied across.
            PEAK_MOVED.fetch_max(LIVE.load(Relaxed) + new, Relaxed);
            if new > 1 << 20 {
                MOVES.fetch_add(1, Relaxed);
                MOVED_BYTES.fetch_add(new, Relaxed);
                WORST_MOVE.fetch_max(layout.size().min(new), Relaxed);
            }
        }
        let now = if new >= layout.size() {
            LIVE.fetch_add(new - layout.size(), Relaxed) + (new - layout.size())
        } else {
            LIVE.fetch_sub(layout.size() - new, Relaxed) - (layout.size() - new)
        };
        PEAK.fetch_max(now, Relaxed);
        PEAK_MOVED.fetch_max(now, Relaxed);

        let got = unsafe { usable(out) };
        let used = if got >= was_usable {
            USABLE.fetch_add(got - was_usable, Relaxed) + (got - was_usable)
        } else {
            USABLE.fetch_sub(was_usable - got, Relaxed) - (was_usable - got)
        };
        USABLE_PEAK.fetch_max(used, Relaxed);
        out
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1_048_576.0
}
fn live() -> usize {
    LIVE.load(Relaxed)
}
fn live_usable() -> usize {
    USABLE.load(Relaxed)
}
/// `requested`, `usable`, and what the rounding costs as a percentage.
fn term(label: &str, requested: usize, usable: usize) {
    println!(
        "{label:<29}: {:8.2} MB requested, {:8.2} MB usable  (+{:.2}, {:+.1} %)",
        mb(requested),
        mb(usable),
        mb(usable - requested),
        (usable as f64 / requested as f64 - 1.0) * 100.0
    );
}
fn arm() {
    PEAK.store(live(), Relaxed);
    PEAK_MOVED.store(live(), Relaxed);
    USABLE_PEAK.store(USABLE.load(Relaxed), Relaxed);
}
fn peaks() -> (usize, usize, usize) {
    (
        PEAK.load(Relaxed),
        PEAK_MOVED.load(Relaxed),
        USABLE_PEAK.load(Relaxed),
    )
}

/// The same ceiling `MatcherBuilder::with_capacity` is fed in production.
fn upper_bound(text: &str) -> usize {
    text.lines()
        .map(|line| {
            let line = line.trim_start();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                0
            } else {
                line.split_whitespace().count()
            }
        })
        .sum()
}

/// Deployment order, as `GET /api/v1/lists` reports it.
const ORDER: [&str; 17] = [
    "big.oisd.nl",
    "filter_1",
    "filter_2",
    "filter_3",
    "filter_11",
    "filter_18",
    "filter_30",
    "filter_43",
    "filter_48",
    "filter_50",
    "filter_59",
    "filter_63",
    "dyndns",
    "hosts",
    "spy",
    "doh-vpn-proxy-bypass",
    "user-rules",
];

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("corpus dir"));
    let read = |id: &str| std::fs::read_to_string(dir.join(format!("{id}.raw"))).unwrap();

    // `deployed` is the order `entries` happens to hold; the other two bracket
    // what any other insertion order would cost.
    let mode = std::env::args().nth(2).unwrap_or_else(|| "deployed".to_string());
    let mut order: Vec<&str> = ORDER.to_vec();
    let size = |id: &str| std::fs::metadata(dir.join(format!("{id}.raw"))).unwrap().len();
    match mode.as_str() {
        "largest-first" => order.sort_by_key(|id| std::cmp::Reverse(size(id))),
        "smallest-first" => order.sort_by_key(|id| size(id)),
        _ => {}
    }
    println!("order: {mode}");

    println!(
        "size_of::<ParsedRule>() = {}",
        std::mem::size_of::<fah_rules::ParsedRule>()
    );

    // The ruleset ArcSwap still holds while the replacement compiles.
    let old = {
        let policies = PolicySet::single_default();
        let bound: usize = ORDER.iter().map(|id| upper_bound(&read(id))).sum();
        let mut builder = MatcherBuilder::with_capacity(bound);
        builder.set_policy_universe(policies.universe());
        for id in ORDER {
            let parsed = parse_rule_list(&read(id));
            builder.add_parsed_list_masked(Arc::from(id), &parsed, policies.mask_for_list(id));
        }
        builder.build()
    };
    let (base, base_usable) = (live(), live_usable());
    println!();
    term("old ruleset, held by ArcSwap", base, base_usable);
    println!(
        "  ({} rules in {} live allocations, heap_bytes() {:.2} MB)\n",
        old.len(),
        ALLOCS.load(Relaxed),
        mb(old.heap_bytes())
    );

    // Every list text is read before the compile loop, and all stay resident.
    let mut texts: Vec<(Arc<str>, String)> = Vec::new();
    for id in &order {
        texts.push((Arc::from(*id), read(id)));
    }
    let (after_texts, texts_usable) = (live(), live_usable());
    term(
        "+ all 17 raw texts",
        after_texts - base,
        texts_usable - base_usable,
    );

    let bound: usize = texts.iter().map(|(_, text)| upper_bound(text)).sum();
    let policies = PolicySet::single_default();
    let mut builder = MatcherBuilder::with_capacity(bound);
    builder.set_policy_universe(policies.universe());
    let (after_dedup, dedup_usable) = (live(), live_usable());
    term(
        "+ dedup index",
        after_dedup - after_texts,
        dedup_usable - texts_usable,
    );
    println!("  (bound {bound} rules)\n");

    println!(
        "{:<22} {:>7} {:>8} {:>8} {:>8} {:>8} {:>9} {:>8}",
        "list", "rules", "vec cap", "Arc<str>", "parsed", "peak", "peak+move", "usable"
    );

    let (mut worst, mut worst_where) = (0usize, String::new());
    let (mut worst_moved, mut worst_usable) = (0usize, 0usize);
    let mut big_parse: Option<(usize, usize, usize, usize)> = None;

    for (id, text) in &texts {
        let (before_parse, before_usable) = (live(), live_usable());
        arm();
        let parsed = parse_rule_list(text);
        let (after_parse, after_usable) = (live(), live_usable());
        let (parse_peak, parse_moved, parse_usable) = peaks();
        if id.as_ref() == "big.oisd.nl" {
            big_parse = Some((
                after_parse - before_parse,
                after_usable - before_usable,
                parsed.rules.capacity(),
                parsed.rules.len(),
            ));
        }

        let spine = parsed.rules.capacity() * std::mem::size_of::<fah_rules::ParsedRule>();
        let payload = (after_parse - before_parse).saturating_sub(spine);
        let rules = parsed.rules.len();

        arm();
        builder.add_parsed_list_masked(id.clone(), &parsed, policies.mask_for_list(id));
        let (add_peak, add_moved, add_usable) = peaks();
        drop(parsed);

        let list_peak = parse_peak.max(add_peak);
        if list_peak > worst {
            worst = list_peak;
            worst_where = format!(
                "{id} ({})",
                if add_peak > parse_peak {
                    "add_parsed_list_masked"
                } else {
                    "parse_rule_list"
                }
            );
        }
        worst_moved = worst_moved.max(parse_moved.max(add_moved));
        worst_usable = worst_usable.max(parse_usable.max(add_usable));

        println!(
            "{:<22} {:>7} {:8.2} {:8.2} {:8.2} {:8.2} {:9.2} {:8.2}",
            id,
            rules,
            mb(spine),
            mb(payload),
            mb(after_parse - before_parse),
            mb(list_peak),
            mb(parse_moved.max(add_moved)),
            mb(parse_usable.max(add_usable))
        );
    }

    println!(
        "\npeak, requested bytes        : {:8.2} MB  inside {worst_where}",
        mb(worst)
    );
    println!(
        "peak, counting moved buffers : {:8.2} MB  (+{:.2} over requested)",
        mb(worst_moved),
        mb(worst_moved - worst)
    );
    println!(
        "peak, mimalloc usable bytes  : {:8.2} MB  (+{:.2} size-class rounding, {} live allocs)",
        mb(worst_usable),
        mb(worst_usable.saturating_sub(worst)),
        ALLOCS_PEAK.load(Relaxed)
    );
    println!(
        "  copying reallocs >1 MiB    : {} moves, {:.2} MB copied, worst single {:.2} MB",
        MOVES.load(Relaxed),
        mb(MOVED_BYTES.load(Relaxed)),
        mb(WORST_MOVE.load(Relaxed))
    );

    // The dominant term, split into the parts a change could remove.
    if let Some((requested, usable, capacity, rules)) = big_parse {
        let width = std::mem::size_of::<fah_rules::ParsedRule>();
        println!("\nbig.oisd.nl ParsedRuleList, the peak term:");
        term("  whole", requested, usable);
        println!(
            "  Vec spine                    : {:8.2} MB  ({capacity} cap x {width} B)",
            mb(capacity * width)
        );
        println!(
            "    of which used              : {:8.2} MB  ({rules} rules)",
            mb(rules * width)
        );
        println!(
            "    of which doubling slack    : {:8.2} MB",
            mb((capacity - rules) * width)
        );
        println!(
            "  Arc<str> domains             : {:8.2} MB requested, {:8.2} MB usable  ({:.1} B/rule requested, {:.1} usable)",
            mb(requested - capacity * width),
            mb(usable - capacity * width),
            (requested - capacity * width) as f64 / rules as f64,
            (usable - capacity * width) as f64 / rules as f64,
        );
    }

    let before_build = live();
    let matcher = builder.build();
    println!(
        "\nbuild()                      : {:8.2} -> {:8.2} MB   new ruleset {:.2} MB, {} rules",
        mb(before_build),
        mb(live()),
        mb(matcher.heap_bytes()),
        matcher.len()
    );
    drop(old);
    println!("after swap_in drops the old  : {:8.2} MB", mb(live()));
    std::hint::black_box(&matcher);
}
