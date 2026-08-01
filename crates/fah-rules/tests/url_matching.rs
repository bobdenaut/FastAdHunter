//! End-to-end URL-tier tests (p2-03): real EasyList lines in, request verdicts
//! out. The unit tests in `url_matcher.rs` pin the matcher's mechanics against
//! hand-built rules; this file pins the whole chain — detection, parsing,
//! retention, compilation, lookup — against text taken verbatim from the list
//! the phase exists to support.

use fah_model::{HttpRequest, ResourceType, Verdict};
use fah_rules::{parse_rule_list, Matcher, MatcherBuilder};

fn compile(text: &str) -> Matcher {
    let parsed = parse_rule_list(text);
    let mut builder = MatcherBuilder::new();
    builder.add_parsed_list("test", &parsed);
    builder.build()
}

/// A request with the fields most tests do not care about already filled in.
fn get<'a>(url: &'a str, host: &'a str) -> HttpRequest<'a> {
    HttpRequest {
        url,
        host,
        method: "GET",
        resource_type: ResourceType::Unknown,
        document_host: None,
    }
}

fn blocked(matcher: &Matcher, request: &HttpRequest<'_>) -> bool {
    matches!(matcher.verdict_http(request), Verdict::Block(_))
}

/// The excerpt is the real head of EasyList — the lines that used to defeat
/// format detection, and whose text used to be discarded even once detection
/// worked. Every rule below is verbatim from `fixtures/easylist_head.txt`.
#[test]
fn real_easylist_rules_decide_real_requests() {
    let matcher = compile(include_str!("fixtures/easylist_head.txt"));
    assert_eq!(matcher.url_len(), 30, "every excerpt line must compile");
    assert_eq!(matcher.len(), 0, "none of them is a DNS rule");

    // `&rb=&uuid=$third-party`
    let mut request = get("http://ads.example.com/px?&rb=&uuid=9f2", "ads.example.com");
    request.document_host = Some("news.other.org");
    assert!(blocked(&matcher, &request));
    // …the same URL first-party is not third-party traffic.
    request.document_host = Some("www.example.com");
    assert!(!blocked(&matcher, &request));

    // `.ashx?AdID=`  — no options, so it applies to any request.
    assert!(blocked(
        &matcher,
        &get(
            "http://shop.example.com/serve.ashx?AdID=17",
            "shop.example.com"
        )
    ));
    assert!(!blocked(
        &matcher,
        &get(
            "http://shop.example.com/serve.ashx?ItemID=17",
            "shop.example.com"
        )
    ));

    // `.club/js/popunder.js$script`
    let mut request = get("http://a.club/js/popunder.js", "a.club");
    request.resource_type = ResourceType::Script;
    assert!(blocked(&matcher, &request));
    request.resource_type = ResourceType::Image;
    assert!(
        !blocked(&matcher, &request),
        "$script must not decide an image request"
    );

    // `-ad-manager/$~stylesheet` — negation, folded at compile time.
    let mut request = get("http://x.example.com/-ad-manager/go", "x.example.com");
    request.resource_type = ResourceType::Script;
    assert!(blocked(&matcher, &request));
    request.resource_type = ResourceType::Stylesheet;
    assert!(!blocked(&matcher, &request), "`~stylesheet` excludes it");
}

/// `$domain=` is evaluated against the *document*, not the request host —
/// getting that backwards would silently invert every one of these rules.
#[test]
fn the_domain_option_is_judged_on_the_document_host() {
    // Verbatim EasyList: `-ads/assets/$script,domain=~web-ads.org`
    let matcher = compile("! t\n-ads/assets/$script,domain=~web-ads.org\n");
    let mut request = get("http://cdn.example.com/-ads/assets/x.js", "cdn.example.com");
    request.resource_type = ResourceType::Script;

    request.document_host = Some("blog.example.com");
    assert!(blocked(&matcher, &request));
    request.document_host = Some("web-ads.org");
    assert!(
        !blocked(&matcher, &request),
        "the `~` entry vetoes the rule"
    );
    request.document_host = Some("docs.web-ads.org");
    assert!(
        !blocked(&matcher, &request),
        "and so does anything under it"
    );
}

/// Precedence is the phase's most load-bearing property: an exception must
/// win over a block from either tier, or enabling a list breaks a site.
#[test]
fn exceptions_beat_blocks_across_both_tiers() {
    // URL exception over a URL block.
    let matcher = compile("! t\n||example.com/assets/\n@@||example.com/assets/app.js\n");
    assert!(blocked(
        &matcher,
        &get("http://example.com/assets/ads.js", "example.com")
    ));
    assert!(matches!(
        matcher.verdict_http(&get("http://example.com/assets/app.js", "example.com")),
        Verdict::Allow(_)
    ));

    // A *domain* exception must also override a URL-tier block: the two tiers
    // share one precedence order, they are not consulted independently.
    let matcher = compile("! t\n||example.com/assets/\n@@||example.com^\n");
    assert!(matches!(
        matcher.verdict_http(&get("http://example.com/assets/ads.js", "example.com")),
        Verdict::Allow(_)
    ));
}

/// A rule about a *name* still decides a request addressed to that name — the
/// HTTP entry point consults both tiers.
#[test]
fn a_domain_rule_blocks_an_http_request_to_that_host() {
    let matcher = compile("! t\n||ads.example.com^\n");
    assert_eq!(matcher.url_len(), 0);
    assert!(blocked(
        &matcher,
        &get("http://ads.example.com/anything", "ads.example.com")
    ));
    assert!(
        blocked(
            &matcher,
            &get("http://deep.ads.example.com/x", "deep.ads.example.com")
        ),
        "subdomain semantics carry over to HTTP"
    );
    assert!(!blocked(
        &matcher,
        &get("http://example.com/x", "example.com")
    ));
}

/// A `$dnstype`-restricted rule is answering a question a request never asks.
/// Applying it to HTTP would let `$dnstype=A` decide a fetch.
#[test]
fn a_dnstype_restricted_rule_does_not_decide_a_request() {
    let matcher = compile("! t\n||ads.example.com^$dnstype=A\n");
    assert_eq!(matcher.len(), 1);
    assert!(!blocked(
        &matcher,
        &get("http://ads.example.com/x", "ads.example.com")
    ));
}

/// The decisive rule is reported back in canonical syntax, from the compiled
/// record — there is no retained line to echo.
#[test]
fn a_url_verdict_names_the_rule_and_its_list() {
    let matcher = compile("! t\n||ads.example.com^*/pixel.gif$third-party\n");
    let mut request = get("http://ads.example.com/a/pixel.gif", "ads.example.com");
    request.document_host = Some("news.other.org");
    match matcher.verdict_http(&request) {
        Verdict::Block(decisive) => {
            assert_eq!(&*decisive.list, "test");
            assert_eq!(&*decisive.rule, "||ads.example.com^*/pixel.gif$third-party");
        }
        other => panic!("expected a block, got {other:?}"),
    }
}

/// Nothing in the URL tier may change what a DNS query resolves to.
#[test]
fn adding_url_rules_leaves_dns_verdicts_untouched() {
    use fah_model::QueryType;

    let dns_only = compile("! t\n||ads.example.com^\n");
    let mixed = compile(
        "! t\n||ads.example.com^\n||ads.example.com^*/pixel.gif$third-party\n/banner/*/ad.js\n",
    );
    for domain in [
        "ads.example.com",
        "deep.ads.example.com",
        "example.com",
        "banner.example.org",
    ] {
        assert_eq!(
            dns_only.verdict(domain, &QueryType::A),
            mixed.verdict(domain, &QueryType::A),
            "the URL tier must not move a DNS verdict for {domain}"
        );
    }
    assert_eq!(mixed.url_len(), 2);
}
