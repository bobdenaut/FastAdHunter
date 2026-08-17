//! URL-tier benches vs the p2-03 budgets: request verdict < 1 ms p99
//! (allocation-freedom is asserted separately, in
//! `tests/url_lookup_alloc.rs`), and the compiled URL matcher's heap recorded
//! as an absolute number against the ~1.03 MiB the headroom model predicted
//! (`docs/code-review/phase2/p2-03-headroom-and-parser-findings.md`).
//!
//! **Corpus.** Real EasyList cannot live in the repo — it is 2 MB, GPLv3, and
//! changes daily — so the default corpus is synthetic and shaped like it. Point
//! `FAH_URL_CORPUS` at real lists (`;`-separated paths) to measure the real
//! thing:
//!
//! ```sh
//! FAH_URL_CORPUS="easylist.txt;easyprivacy.txt" cargo bench -p fah-rules --bench url_matcher
//! ```
//!
//! Both modes print the same figures, so the synthetic run is a regression
//! guard and the real run is the number of record.

use std::hint::black_box;
use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use fah_model::{HttpRequest, ResourceType};
use fah_rules::{parse_rule_list, MatcherBuilder};

/// Rule count of the synthetic corpus — EasyList + EasyPrivacy's measured URL
/// tier is 22,020 rules, so the fallback lands in the same order of magnitude.
const SYNTHETIC_RULES: usize = 22_000;

/// A deterministic pseudo-random generator, matching `benches/matcher.rs`.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// EasyList's actual pattern shapes, in roughly its proportions: domain-anchored
/// paths, bare substrings, wildcards, and the option tails that dominate the
/// side arena (`$third-party`, `$script`, `$domain=`).
fn synthetic_corpus() -> String {
    let mut lcg = Lcg(0x5eed_1234);
    let mut text = String::with_capacity(SYNTHETIC_RULES * 48);
    text.push_str("! Title: synthetic url corpus\n");
    for n in 0..SYNTHETIC_RULES as u64 {
        let r = lcg.next();
        match n % 6 {
            0 => text.push_str(&format!("||ads{n}.example.com^*/pixel.gif$third-party\n")),
            1 => text.push_str(&format!("/adserver{n}/banner$image\n")),
            2 => text.push_str(&format!(
                "-tracking{n}-/$script,domain=~publisher{}.org\n",
                r % 500
            )),
            3 => text.push_str(&format!("||cdn{n}.metrics.net/collect?id=\n")),
            4 => text.push_str(&format!("&utm_campaign{n}=$third-party\n")),
            _ => text.push_str(&format!("||track{n}.example.org^$xmlhttprequest\n")),
        }
    }
    text
}

/// Every configured corpus as (name, text). Real lists when `FAH_URL_CORPUS`
/// names them, otherwise the synthetic one.
fn corpora() -> Vec<(String, String)> {
    match std::env::var("FAH_URL_CORPUS") {
        Ok(paths) if !paths.trim().is_empty() => paths
            .split(';')
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(|path| {
                let text = std::fs::read_to_string(path)
                    .unwrap_or_else(|err| panic!("FAH_URL_CORPUS entry {path}: {err}"));
                (path.to_string(), text)
            })
            .collect(),
        _ => vec![("synthetic".to_string(), synthetic_corpus())],
    }
}

/// A request mix that exercises both outcomes and both tiers: a lookup that
/// finds nothing is the common case and must not be the fast case only because
/// it short-circuits.
fn requests<'a>(corpus_is_real: bool) -> Vec<HttpRequest<'a>> {
    let blocked_url = if corpus_is_real {
        "http://www.example.com/pagead/js/adsbygoogle.js"
    } else {
        "http://ads12.example.com/a/pixel.gif"
    };
    vec![
        HttpRequest {
            url: blocked_url,
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

/// A URL of `length` bytes that matches nothing — the honest worst case, since
/// a lookup that finds an allow returns early and a lookup that finds nothing
/// has walked every candidate. Token-shaped rather than one long run of a
/// single byte, so the tokenizer and the index probes do their real work.
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

fn bench_url_matcher(c: &mut Criterion) {
    let corpora = corpora();
    let corpus_is_real = std::env::var("FAH_URL_CORPUS").is_ok();

    // Parse and compile timings are reported separately: retention changed the
    // parse half (one `Arc<str>` per URL rule), the index build is new work.
    let started = Instant::now();
    let parsed: Vec<_> = corpora
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
    let retained_bytes: usize = parsed
        .iter()
        .flat_map(|(_, list)| list.rules.iter())
        .filter_map(|rule| match &rule.kind {
            fah_rules::RuleKind::Url(url) => Some(
                url.pattern.len()
                    + url.domains.as_deref().map_or(0, str::len)
                    + url.methods.as_deref().map_or(0, str::len),
            ),
            _ => None,
        })
        .sum();

    let url_heap = matcher.url_heap_bytes();
    println!(
        "\n[url tier] corpus: {}\n\
         [url tier] parsed  : {dns_rules} dns, {url_rules} url, {inactive} inactive\n\
         [url tier] compiled: {} url rules ({} duplicates removed), {} unindexed\n\
         [url tier] heap    : {url_heap} bytes ({:.2} MiB)\n\
         [url tier] retained: {retained_bytes} bytes of rule text ({} Arc allocations)\n\
         [url tier] parse   : {:.3} s   compile: {:.3} s\n\
         [url tier] whole matcher heap: {:.2} MiB\n",
        corpora
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        matcher.url_len(),
        matcher.url_duplicates_removed(),
        matcher.url_unindexed(),
        url_heap as f64 / (1024.0 * 1024.0),
        url_rules,
        parse_time.as_secs_f64(),
        compile_time.as_secs_f64(),
        matcher.heap_bytes() as f64 / (1024.0 * 1024.0),
    );

    let requests = requests(corpus_is_real);

    let mut group = c.benchmark_group("url_verdict");
    group.bench_function("mixed_requests", |b| {
        b.iter(|| {
            for request in &requests {
                black_box(matcher.lookup_http(black_box(request)));
            }
        })
    });
    group.bench_function("single_pass_request", |b| {
        let request = &requests[1];
        b.iter(|| black_box(matcher.lookup_http(black_box(request))))
    });

    // **The worst case, and the one figure a dev box cannot settle** (p2-08).
    //
    // Rules no token could file are checked on every lookup, and that is the
    // only term that grows with URL length — the p2-03 review measured the
    // whole lookup at 3.09 µs for a short URL and 556 µs at 8 KiB, on x86. The
    // RB5009's core is several times slower, so whether a substring index is
    // worth its automaton is decided by *this* sweep run on device, not here.
    //
    // Long URLs are ordinary traffic, not an attack: OAuth redirects, ad-tech
    // beacons and analytics payloads routinely carry multi-KB query strings.
    for length in [64usize, 1024, 4096, 8192] {
        let url = long_url(length);
        let request = HttpRequest {
            url: &url,
            host: "cdn.example.com",
            method: "GET",
            resource_type: ResourceType::Script,
            document_host: Some("news.other.org"),
        };
        group.bench_function(format!("long_url_{length}b"), |b| {
            b.iter(|| black_box(matcher.lookup_http(black_box(&request))))
        });
    }
    group.finish();

    // Retention's own cost, isolated: parsing is the phase that dominates
    // startup, and `ParsedRule`'s comment demands a number before that path
    // starts allocating per rule again.
    let mut group = c.benchmark_group("url_parse");
    group.sample_size(10);
    for (name, text) in &corpora {
        group.bench_function(name.as_str(), |b| {
            b.iter(|| black_box(parse_rule_list(black_box(text))))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_url_matcher);
criterion_main!(benches);
