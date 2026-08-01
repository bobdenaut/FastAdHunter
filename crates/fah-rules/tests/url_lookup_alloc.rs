//! `Matcher::lookup_http` must be allocation-free (p2-03 acceptance criterion,
//! PERFORMANCE.md: the hot path allocates nothing).
//!
//! The domain tier earned that guarantee by returning a `RuleRef` instead of a
//! `DecisiveRule`; the URL tier has more ways to lose it — a `$domain=` split,
//! a lowercased URL, a `Vec` of candidate rules would each be invisible in a
//! latency bench and fatal at 20k requests/s. So it is asserted directly: a
//! counting allocator, a warm matcher, and a hard zero.
//!
//! Kept in its own test binary with one test, so no other test allocates on a
//! second thread while the counter is being read.

use fah_model::{HttpRequest, ResourceType};
use fah_rules::{parse_rule_list, MatcherBuilder};
use mimalloc::MiMalloc;
use std::alloc::{GlobalAlloc, Layout};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

/// Counts calls, not bytes: one allocation of any size is the failure, so the
/// count is the sharper instrument. Delegates to the production allocator.
struct Counting;

// SAFETY: every method forwards to `MiMalloc`, a sound `GlobalAlloc`. The
// counter is a pure side effect that never touches the returned memory nor
// alters the layout, so the allocator contract is preserved unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: `layout` is a valid, non-zero layout per the trait contract;
        // forwarded verbatim to the delegate.
        unsafe { MiMalloc.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` come straight from a prior `alloc` call with
        // the same layout, as the trait requires; forwarded verbatim.
        unsafe { MiMalloc.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: forwarded verbatim; `ptr`/`layout`/`new_size` satisfy the
        // trait contract at the call site.
        unsafe { MiMalloc.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn an_http_lookup_allocates_nothing_whatever_it_decides() {
    let parsed = parse_rule_list(include_str!("fixtures/easylist_head.txt"));
    let mut builder = MatcherBuilder::new();
    builder.add_parsed_list("easylist", &parsed);
    builder.add_parsed_list(
        "extra",
        &parse_rule_list("! t\n||ads.example.com^\n@@||cdn.example.com/app.js\n"),
    );
    let matcher = builder.build();

    // A spread over every outcome and every predicate: blocked by the URL
    // tier, blocked by the domain tier, allowed, passed — plus the option
    // paths (`$domain=`, `$third-party`, `$script`) that are the ones most
    // likely to reach for a `split` or a `to_lowercase`.
    let cases: [(&str, &str, ResourceType, Option<&str>); 6] = [
        (
            "http://ads.example.com/px?&rb=&uuid=9f2",
            "ads.example.com",
            ResourceType::Image,
            Some("news.other.org"),
        ),
        (
            "http://cdn.example.com/-ads/assets/x.js",
            "cdn.example.com",
            ResourceType::Script,
            Some("blog.example.com"),
        ),
        (
            "http://cdn.example.com/app.js",
            "cdn.example.com",
            ResourceType::Script,
            None,
        ),
        (
            "http://ads.example.com/whatever",
            "ads.example.com",
            ResourceType::Document,
            None,
        ),
        (
            "http://www.example.org/index.html",
            "www.example.org",
            ResourceType::Document,
            None,
        ),
        (
            "http://shop.example.com/serve.ashx?AdID=17",
            "shop.example.com",
            ResourceType::Other,
            Some("shop.example.com"),
        ),
    ];

    // Warm every path once *before* counting: the first call through a lazily
    // initialized allocator arena would otherwise be charged to the lookup.
    for (url, host, resource_type, document_host) in cases {
        black_box(matcher.lookup_http(&HttpRequest {
            url,
            host,
            method: "GET",
            resource_type,
            document_host,
        }));
    }

    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for _ in 0..1_000 {
        for (url, host, resource_type, document_host) in cases {
            black_box(matcher.lookup_http(&HttpRequest {
                url,
                host,
                method: "GET",
                resource_type,
                document_host,
            }));
        }
    }
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;

    assert_eq!(
        allocations, 0,
        "6000 HTTP lookups made {allocations} allocations; the hot path must make none"
    );
}
