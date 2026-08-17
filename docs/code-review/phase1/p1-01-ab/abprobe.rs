//! p1-01 A/B probe: startup-phase timings and compiled-output equivalence over
//! a real list corpus. Uses only API present both before and after the p1-01
//! review fixes, so the same file compiles in either checkout.
//!
//! Lives here rather than in the crate: it is an instrument for one review, not
//! a shipped example. To run it, copy it into both checkouts first.
//!
//! ```text
//! git worktree add <baseline-dir> <pre-change-rev> --detach
//! cp abprobe.rs {.,<baseline-dir>}/crates/fah-rules/examples/abprobe.rs
//! cargo build --release -p fah-rules --example abprobe   # in each
//! pwsh run-ab.ps1 -Corpus <corpus-dir> -Mode phases -Iterations 7
//! pwsh run-ab.ps1 -Corpus <corpus-dir> -Mode boot   -Iterations 7
//! python analyze.py run2-phases.txt run2-boot.txt
//! ```
//!
//! The corpus is one `.raw` per list, named by list id — refetch the deployed
//! set from the URLs in the newest `soak-*/lists-*.json`.
//!
//! `phases` splits parse (by detected format) from build; `boot` runs the real
//! `ListManager::boot()`, the only arm that exercises the pre-parse rule
//! ceiling. One cycle per process, so peak RSS belongs to one compile.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fah_config::{RuleListConfig, RulesConfig};
use fah_rules::{detect_format, parse_rule_list, ListManager, MatcherBuilder, RuleFormat};

/// The allocator the product uses. A library must not pick one, so the example
/// does it here rather than fah-rules doing it for every dependent.
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Dedup-index size for `phases`. A constant, not the real pre-parse ceiling:
/// that function is crate-private, and holding it fixed keeps a rehash storm
/// out of the build arm. `boot` measures the real path.
const DEDUP_HINT: usize = 1_200_000;

fn main() {
    let mut args = std::env::args().skip(1);
    let corpus = args.next().expect("usage: abprobe <corpus-dir> <mode>");
    let mode = args.next().unwrap_or_else(|| "phases".to_string());

    let sources = read_corpus(Path::new(&corpus));
    println!("[ab] mode={mode}");
    println!("[ab] lists={}", sources.len());
    println!(
        "[ab] corpus_bytes={}",
        sources.iter().map(|(_, text)| text.len()).sum::<usize>()
    );

    match mode.as_str() {
        "phases" => phases(&sources),
        "boot" => boot(&sources),
        other => panic!("unknown mode {other}"),
    }

    // Peak working set is read by the runner from outside; it is a high-water
    // mark, so any read during this window sees the whole run.
    println!("[ab] peak_window_open");
    std::thread::sleep(Duration::from_millis(1500));
    println!("[ab] done");
}

/// Sorted so the compiled ruleset — and every figure below — cannot depend on
/// directory iteration order.
fn read_corpus(dir: &Path) -> Vec<(String, String)> {
    let mut sources: Vec<(String, String)> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("corpus dir {}: {err}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "raw"))
        .map(|path| {
            let id = path
                .file_stem()
                .expect("list file has a stem")
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            (id, text)
        })
        .collect();
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!sources.is_empty(), "no .raw lists in {}", dir.display());
    sources
}

/// FNV-1a over every corpus byte — the control arm. No change under review can
/// touch it, so a move here is the box drifting, not the code.
fn control(sources: &[(String, String)]) -> Duration {
    let started = Instant::now();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (_, text) in sources {
        for &byte in text.as_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let elapsed = started.elapsed();
    println!("[ab] control_hash={hash:x}");
    elapsed
}

fn phases(sources: &[(String, String)]) {
    println!(
        "[ab] control_ms={:.3}",
        control(sources).as_secs_f64() * 1e3
    );

    let mut parse = [Duration::ZERO; 3];
    let mut lists = [0usize; 3];
    let mut add = Duration::ZERO;
    let (mut active, mut url, mut inactive, mut errors) = (0usize, 0usize, 0usize, 0u64);

    let mut builder = MatcherBuilder::with_capacity(DEDUP_HINT);
    for (id, text) in sources {
        let slot = match detect_format(text) {
            RuleFormat::Hosts => 0,
            RuleFormat::Adblock => 1,
            RuleFormat::PlainDomainList => 2,
        };

        let started = Instant::now();
        let parsed = parse_rule_list(text);
        parse[slot] += started.elapsed();
        lists[slot] += 1;

        // Counted here rather than kept: the compile path drops each parsed
        // list before the next, so holding all 16 would misreport peak RSS.
        active += parsed.active_count();
        url += parsed.url_count();
        inactive += parsed.inactive_count();
        errors += u64::from(parsed.parse_errors);

        let started = Instant::now();
        builder.add_parsed_list(Arc::from(id.as_str()), &parsed);
        add += started.elapsed();
    }

    let started = Instant::now();
    let matcher = builder.build();
    let build = started.elapsed();

    let parse_total: Duration = parse.iter().sum();
    println!("[ab] parse_total_ms={:.3}", ms(parse_total));
    println!("[ab] parse_hosts_ms={:.3} n={}", ms(parse[0]), lists[0]);
    println!("[ab] parse_adblock_ms={:.3} n={}", ms(parse[1]), lists[1]);
    println!("[ab] parse_plain_ms={:.3} n={}", ms(parse[2]), lists[2]);
    println!("[ab] add_ms={:.3}", ms(add));
    println!("[ab] build_ms={:.3}", ms(build));
    println!("[ab] compile_total_ms={:.3}", ms(parse_total + add + build));
    println!("[ab] parsed_active={active} parsed_url={url} parsed_inactive={inactive}");
    println!("[ab] parse_errors={errors}");
    println!("[ab] compiled_rules={}", matcher.len());
    println!("[ab] compiled_url_rules={}", matcher.url_len());
    println!("[ab] compiled_heap_bytes={}", matcher.heap_bytes());
}

/// The production path: `ListManager::new` + `boot()` over `/data`-cached
/// copies. The only arm that includes the pre-parse rule ceiling.
fn boot(sources: &[(String, String)]) {
    let data_dir = tempfile::tempdir().expect("temp data dir");
    let lists_dir = data_dir.path().join("lists");
    std::fs::create_dir_all(&lists_dir).expect("lists dir");
    for (id, text) in sources {
        std::fs::write(lists_dir.join(format!("{id}.raw")), text).expect("seed cache");
    }

    // `.invalid` so boot cannot be tempted onto the network; the URL is
    // identity only, the cache file is keyed by id.
    let config = RulesConfig {
        refresh_hours_default: 48,
        lists: sources
            .iter()
            .map(|(id, _)| RuleListConfig {
                id: id.clone(),
                url: format!("https://{id}.invalid/list.txt"),
                enabled: true,
                refresh_hours: None,
            })
            .collect(),
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");

    let path: PathBuf = data_dir.path().to_path_buf();
    let started = Instant::now();
    let manager = ListManager::new(&config, path).expect("list manager");
    runtime.block_on(manager.boot());
    let boot = started.elapsed();

    println!("[ab] boot_ms={:.3}", ms(boot));
    println!("[ab] compiled_rules={}", manager.matcher().len());
    println!("[ab] compiled_url_rules={}", manager.matcher().url_len());
    println!(
        "[ab] compiled_heap_bytes={}",
        manager.matcher().heap_bytes()
    );
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}
