//! On-device URL-tier probe (p2-08).
//!
//! Answers one question the dev box cannot: **what does a URL lookup cost on
//! the RB5009 in the worst case — long URLs against the rules no token could
//! index — and does that cost justify building a substring index?**
//!
//! Deliberately *not* criterion. This runs as the entrypoint of a distroless
//! throwaway container on a router with no shell, no writable cwd and no way
//! to collect a `target/criterion` tree; the only channel back is stdout, which
//! RouterOS copies into its log. So: fixed arms, plain text out, exit.
//!
//! The arms mirror `benches/url_matcher.rs` exactly. That is the whole point —
//! the same harness runs on x86 and on ARM over the same corpus, so the ratio
//! between them is a measurement rather than a comparison of two harnesses.
//!
//! ```sh
//! # dev box, against the lists copied off the router
//! cargo run --release -p fah-rules --example urlbench -- ./corpus
//! ```
//!
//! In the container the corpus is baked in at `/corpus` and no argument is
//! passed.

use std::hint::black_box;
use std::time::{Duration, Instant};

use fah_model::{HttpRequest, ResourceType};
use fah_rules::{parse_rule_list, MatcherBuilder, RuleKind};

/// Matches the shipped binary (`crates/fastadhunter/src/allocator.rs`), because
/// a probe measuring the router has to measure the allocator the router runs.
/// Lookup itself is allocation-free — `tests/url_lookup_alloc.rs` asserts that,
/// so the bench arms are unaffected either way — but parse and compile are
/// reported here too, and under musl's default malloc those would be a figure
/// production never pays.
///
/// This is an example target, not the library: the "fah-rules must not
/// participate in allocator selection" rule in `Cargo.toml` is about what the
/// crate imposes on its dependents, and an example imposes nothing.
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Wall time each arm aims to spend. Two jobs, and the second is why it is not
/// smaller: it has to hold the core busy long enough for RouterOS's frequency
/// scaling to settle, because the 0.2.9 soak caught the RB5009 idling at
/// 350 MHz against a 1.4 GHz nominal. A probe that finishes before the core
/// clocks up reports a 4x-pessimistic figure and would "justify" an index
/// nothing needs.
const ARM_TARGET: Duration = Duration::from_secs(8);

/// Batch size is chosen so one batch takes about this long, amortising the
/// `Instant::now()` pair across enough iterations that clock overhead does not
/// show up in a 3 us measurement.
const BATCH_TARGET_NANOS: u128 = 1_000_000;

fn main() {
    println!("[probe] fah urlbench — p2-08 worst-case URL lookup");
    report_cpu();

    // Explicit dirs on the command line, else every subdirectory of `/corpus`.
    // Two corpora are measured on purpose, and the pair is the finding: the
    // deployed lists compile 3 unindexed rules, real EasyList + EasyPrivacy
    // compile 77, and the unindexed scan is the only term that grows with URL
    // length. One number alone would answer the wrong question.
    let mut dirs: Vec<String> = std::env::args().skip(1).collect();
    if dirs.is_empty() {
        dirs = std::fs::read_dir("/corpus")
            .unwrap_or_else(|err| panic!("/corpus: {err}"))
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.path().display().to_string())
            .collect();
        dirs.sort();
    }
    assert!(!dirs.is_empty(), "no corpus directories to measure");

    for dir in &dirs {
        println!("\n[probe] ================ corpus: {dir} ================");
        run_corpus(dir);
    }
    report_cpu();
    println!("[probe] done");
}

fn run_corpus(dir: &str) {
    let started = Instant::now();
    let mut sources: Vec<(String, String)> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("corpus dir {dir}: {err}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "raw" || ext == "txt")
        })
        .map(|path| {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into(),
            );
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            (name, text)
        })
        .collect();
    // Sorted so the compiled ruleset — and therefore every figure below — does
    // not depend on directory iteration order.
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    let read_time = started.elapsed();
    assert!(!sources.is_empty(), "no .raw/.txt lists found in {dir}");

    let started = Instant::now();
    let parsed: Vec<_> = sources
        .iter()
        .map(|(name, text)| (name.clone(), parse_rule_list(text)))
        .collect();
    let parse_time = started.elapsed();

    let started = Instant::now();
    let mut builder = MatcherBuilder::new();
    for (name, list) in &parsed {
        builder.add_parsed_list(name.as_str(), list);
    }
    let matcher = builder.build();
    let compile_time = started.elapsed();

    let url_rules: usize = parsed.iter().map(|(_, list)| list.url_count()).sum();
    let dns_rules: usize = parsed.iter().map(|(_, list)| list.active_count()).sum();
    let inactive: usize = parsed.iter().map(|(_, list)| list.inactive_count()).sum();
    let retained: usize = parsed
        .iter()
        .flat_map(|(_, list)| list.rules.iter())
        .filter_map(|rule| match &rule.kind {
            RuleKind::Url(url) => Some(
                url.pattern.len()
                    + url.domains.as_deref().map_or(0, str::len)
                    + url.methods.as_deref().map_or(0, str::len),
            ),
            _ => None,
        })
        .sum();

    println!("[probe] lists   : {}", sources.len());
    for (name, list) in &parsed {
        println!(
            "[probe]   {name:<28} dns={:<7} url={:<6} inactive={}",
            list.active_count(),
            list.url_count(),
            list.inactive_count()
        );
    }
    println!("[probe] parsed  : {dns_rules} dns, {url_rules} url, {inactive} inactive");
    println!(
        "[probe] compiled: {} url rules ({} duplicates removed), {} UNINDEXED",
        matcher.url_len(),
        matcher.url_duplicates_removed(),
        matcher.url_unindexed()
    );
    println!(
        "[probe] heap    : url tier {} bytes ({:.2} MiB), whole matcher {:.2} MiB, retained rule text {retained} bytes",
        matcher.url_heap_bytes(),
        matcher.url_heap_bytes() as f64 / (1024.0 * 1024.0),
        matcher.heap_bytes() as f64 / (1024.0 * 1024.0)
    );
    println!(
        "[probe] startup : read {:.3} s, parse {:.3} s, compile {:.3} s",
        read_time.as_secs_f64(),
        parse_time.as_secs_f64(),
        compile_time.as_secs_f64()
    );
    println!(
        "[probe] --- measuring, ~{} s per arm ---",
        ARM_TARGET.as_secs()
    );

    let requests = requests();
    measure("mixed_requests", || {
        for request in &requests {
            black_box(matcher.lookup_http(black_box(request)));
        }
    });
    measure("single_pass_request", || {
        black_box(matcher.lookup_http(black_box(&requests[1])));
    });

    // The sweep p2-08 exists for. x86 reference (p2-03 review): 3.09 us short,
    // 556 us at 8 KiB. Only the unindexed rules grow with URL length, so the
    // shape of this curve — flat, linear, or worse — is the argument for or
    // against a substring index.
    for length in [64usize, 1024, 4096, 8192] {
        let url = long_url(length);
        let request = HttpRequest {
            url: &url,
            host: "cdn.example.com",
            method: "GET",
            resource_type: ResourceType::Script,
            document_host: Some("news.other.org"),
        };
        measure(&format!("long_url_{length}b"), || {
            black_box(matcher.lookup_http(black_box(&request)));
        });
    }

    println!(
        "[probe] budget exhaustions during this corpus: {}",
        matcher.url_budget_exhausted()
    );
}

/// Same mix as `benches/url_matcher.rs`, real-corpus variant.
fn requests<'a>() -> Vec<HttpRequest<'a>> {
    vec![
        HttpRequest {
            url: "http://www.example.com/pagead/js/adsbygoogle.js",
            host: "www.example.com",
            method: "GET",
            resource_type: ResourceType::Script,
            document_host: Some("news.other.org"),
        },
        HttpRequest {
            url: "http://www.wikipedia.org/wiki/Rust_(programming_language)",
            host: "www.wikipedia.org",
            method: "GET",
            resource_type: ResourceType::Document,
            document_host: None,
        },
        HttpRequest {
            url: "http://static.example.net/assets/app.7f3c2b.js",
            host: "static.example.net",
            method: "GET",
            resource_type: ResourceType::Script,
            document_host: Some("www.example.net"),
        },
        HttpRequest {
            url: "http://img.example.org/photos/2026/07/header.jpg?w=1200",
            host: "img.example.org",
            method: "GET",
            resource_type: ResourceType::Image,
            document_host: Some("blog.example.org"),
        },
    ]
}

/// A URL of `length` bytes matching nothing — the honest worst case, since a
/// lookup that finds an allow returns early while one that finds nothing has
/// walked every candidate. Token-shaped, so the tokenizer does real work.
/// Byte-identical to the bench's generator.
fn long_url(length: usize) -> String {
    let mut url = String::with_capacity(length + 32);
    url.push_str("http://cdn.example.com/p?");
    let mut n = 0u32;
    while url.len() < length {
        url.push_str(&format!("k{n}=v{n}&"));
        n += 1;
    }
    url.truncate(length.max(url.find('?').unwrap_or(0) + 1));
    url
}

/// Time `op` and print a distribution. Batched: one timed batch runs `inner`
/// iterations, so the clock pair is amortised rather than measured. The
/// reported statistics are over batch means, which suits a deterministic
/// operation where the spread is machine noise rather than workload variance —
/// `min` is therefore the cleanest estimate of true cost, and the tail says how
/// noisy the device was.
fn measure(name: &str, mut op: impl FnMut()) {
    // Warm up caches and branch predictors, and give the governor a reason to
    // raise the clock before anything is recorded.
    let probe = Instant::now();
    let mut warm = 0u64;
    while probe.elapsed() < Duration::from_millis(500) {
        op();
        warm += 1;
    }
    let per_nanos = (probe.elapsed().as_nanos() / u128::from(warm.max(1))).max(1);
    let inner = (BATCH_TARGET_NANOS / per_nanos).clamp(1, 100_000) as u64;
    let batches = (ARM_TARGET.as_nanos() / (per_nanos * u128::from(inner))).clamp(20, 5_000) as u64;

    let mut samples = Vec::with_capacity(batches as usize);
    for _ in 0..batches {
        let started = Instant::now();
        for _ in 0..inner {
            op();
        }
        samples.push(started.elapsed().as_nanos() as f64 / inner as f64);
    }
    samples.sort_by(f64::total_cmp);

    let n = samples.len();
    let mean = samples.iter().sum::<f64>() / n as f64;
    let pick = |q: f64| samples[((n as f64 * q) as usize).min(n - 1)];
    println!(
        "[bench] {name:<22} min {:>10.3} us  p50 {:>10.3}  mean {:>10.3}  p99 {:>10.3}  max {:>10.3}   ({n} batches x {inner})",
        samples[0] / 1000.0,
        pick(0.50) / 1000.0,
        mean / 1000.0,
        pick(0.99) / 1000.0,
        samples[n - 1] / 1000.0,
    );
}

/// Whatever the kernel will admit about the core this is running on. Printed
/// before and after the run: if the two differ, the governor moved mid-probe
/// and the numbers need reading with that in mind.
fn report_cpu() {
    let freq = std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq")
        .ok()
        .map_or_else(
            || "unavailable".to_string(),
            |khz| {
                khz.trim().parse::<f64>().map_or_else(
                    |_| khz.trim().to_string(),
                    |k| format!("{:.0} MHz", k / 1000.0),
                )
            },
        );
    println!("[probe] cpu0 scaling_cur_freq: {freq}");
}
