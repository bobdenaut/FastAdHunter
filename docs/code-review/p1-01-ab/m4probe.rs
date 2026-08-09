//! p1-01 M4 sizing: what boxing the rare `ParsedRule` payloads would buy.
//!
//! Two questions the estimate in `p1-01-review.md` assumed rather than measured:
//!   OCCUPANCY  how many rules actually set the three `Option<Arc<str>>` — the
//!              boxing is only near-free if that is ~0.
//!   LAYOUT     what the compiler really produces for the proposed shape, taken
//!              from mirror types rather than from hand arithmetic.

use fah_rules::{parse_rule_list, InactiveReason, RuleAction, RuleKind, UrlAnchor, UrlRule};
use std::sync::Arc;

// ---- the proposed post-M4 shapes, mirrored so `size_of` is measured ----

#[allow(dead_code)]
struct DomainOpts {
    dns_types: Option<Arc<str>>,
    dns_rewrite: Option<Arc<str>>,
    client: Option<Arc<str>>,
}

#[allow(dead_code)]
struct DomainRuleM4 {
    domain: Arc<str>,
    action: RuleAction,
    include_subdomains: bool,
    /// `None` on every rule carrying no `$dnstype`/`$dnsrewrite`/`$client`.
    opts: Option<Box<DomainOpts>>,
}

#[allow(dead_code)]
enum RuleKindM4 {
    Active(DomainRuleM4),
    Url(Box<UrlRule>),
    Inactive(InactiveReason),
}

#[allow(dead_code)]
struct ParsedRuleM4 {
    kind: RuleKindM4,
}

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

#[derive(Default)]
struct Counts {
    rules: usize,
    active: usize,
    url: usize,
    inactive: usize,
    /// Active rules setting at least one of the three options — one `Box` each.
    active_with_opts: usize,
    dns_types: usize,
    dns_rewrite: usize,
    client: usize,
    /// URL rules setting each option; they ride along inside the boxed `UrlRule`.
    url_domains: usize,
    url_methods: usize,
    url_client: usize,
    /// The `Vec<ParsedRule>` capacity this list reached, in elements.
    capacity: usize,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.rules += other.rules;
        self.active += other.active;
        self.url += other.url;
        self.inactive += other.inactive;
        self.active_with_opts += other.active_with_opts;
        self.dns_types += other.dns_types;
        self.dns_rewrite += other.dns_rewrite;
        self.client += other.client;
        self.url_domains += other.url_domains;
        self.url_methods += other.url_methods;
        self.url_client += other.url_client;
        self.capacity += other.capacity;
    }
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn count(text: &str) -> Counts {
    let parsed = parse_rule_list(text);
    let mut c = Counts {
        capacity: parsed.rules.capacity(),
        ..Counts::default()
    };
    for rule in &parsed.rules {
        c.rules += 1;
        match &rule.kind {
            RuleKind::Active(d) => {
                c.active += 1;
                let mut any = false;
                if d.dns_types.is_some() {
                    c.dns_types += 1;
                    any = true;
                }
                if d.dns_rewrite.is_some() {
                    c.dns_rewrite += 1;
                    any = true;
                }
                if d.client.is_some() {
                    c.client += 1;
                    any = true;
                }
                if any {
                    c.active_with_opts += 1;
                }
            }
            RuleKind::Url(u) => {
                c.url += 1;
                c.url_domains += usize::from(u.domains.is_some());
                c.url_methods += usize::from(u.methods.is_some());
                c.url_client += usize::from(u.client.is_some());
            }
            RuleKind::Inactive(_) => c.inactive += 1,
        }
    }
    c
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("corpus dir"));
    let read = |id: &str| std::fs::read_to_string(dir.join(format!("{id}.raw"))).unwrap();

    let now = std::mem::size_of::<fah_rules::ParsedRule>();
    let after = std::mem::size_of::<ParsedRuleM4>();
    println!("layout");
    println!("  size_of::<ParsedRule>()      now {now:3} B   after M4 {after:3} B   delta {:+} B", after as isize - now as isize);
    println!(
        "    RuleKind                   now {:3} B   after M4 {:3} B",
        std::mem::size_of::<RuleKind>(),
        std::mem::size_of::<RuleKindM4>()
    );
    println!(
        "    DomainRule                 now {:3} B   after M4 {:3} B   (boxed opts {:3} B)",
        std::mem::size_of::<fah_rules::DomainRule>(),
        std::mem::size_of::<DomainRuleM4>(),
        std::mem::size_of::<DomainOpts>()
    );
    println!(
        "    UrlRule                    now {:3} B   after M4 {:3} B   (behind a Box)",
        std::mem::size_of::<UrlRule>(),
        std::mem::size_of::<Box<UrlRule>>()
    );
    println!(
        "    UrlAnchor {} B, RuleAction {} B, InactiveReason {} B",
        std::mem::size_of::<UrlAnchor>(),
        std::mem::size_of::<RuleAction>(),
        std::mem::size_of::<InactiveReason>()
    );

    println!(
        "\n{:<22} {:>8} {:>8} {:>7} {:>8} {:>7} {:>8} {:>8} {:>8}",
        "list", "rules", "active", "url", "inactive", "opts", "vec cap", "now MB", "M4 MB"
    );

    let mut total = Counts::default();
    let mut biggest: (usize, String, usize) = (0, String::new(), 0);
    for id in ORDER {
        let c = count(&read(id));
        println!(
            "{:<22} {:>8} {:>8} {:>7} {:>8} {:>7} {:>8} {:>8.2} {:>8.2}",
            id,
            c.rules,
            c.active,
            c.url,
            c.inactive,
            c.active_with_opts,
            c.capacity,
            mb(c.capacity * now),
            mb(c.capacity * after)
        );
        if c.capacity > biggest.0 {
            biggest = (c.capacity, id.to_string(), c.rules);
        }
        total.add(&c);
    }

    println!(
        "{:<22} {:>8} {:>8} {:>7} {:>8} {:>7} {:>8} {:>8.2} {:>8.2}",
        "TOTAL",
        total.rules,
        total.active,
        total.url,
        total.inactive,
        total.active_with_opts,
        total.capacity,
        mb(total.capacity * now),
        mb(total.capacity * after)
    );

    println!("\noccupancy of the fields M4 boxes");
    let pct = |n: usize, of: usize| {
        if of == 0 {
            0.0
        } else {
            n as f64 / of as f64 * 100.0
        }
    };
    println!(
        "  active rules with any option : {:>8} of {:>8}  ({:.4} %)  <- one Box each",
        total.active_with_opts,
        total.active,
        pct(total.active_with_opts, total.active)
    );
    println!(
        "    $dnstype                   : {:>8}   $dnsrewrite {:>8}   $client {:>8}",
        total.dns_types, total.dns_rewrite, total.client
    );
    println!(
        "  url rules                    : {:>8} of {:>8}  ({:.2} %)  <- one Box each",
        total.url,
        total.rules,
        pct(total.url, total.rules)
    );
    println!(
        "    $domain {:>7}   $method {:>7}   $client {:>7}",
        total.url_domains, total.url_methods, total.url_client
    );

    // The peak term is the largest single list: lists are parsed one at a time.
    let (cap, ref id, rules) = biggest;
    println!("\npeak term — the largest single list is {id} ({rules} rules, {cap} cap)");
    println!(
        "  Vec spine                    : {:8.2} -> {:8.2} MB   ({:+.2} MB)",
        mb(cap * now),
        mb(cap * after),
        mb(cap * after) - mb(cap * now)
    );
    let boxes = total.active_with_opts + total.url;
    println!(
        "  new allocations, whole corpus: {boxes} boxes ({:.2} % of {} rules)",
        pct(boxes, total.rules),
        total.rules
    );
}
