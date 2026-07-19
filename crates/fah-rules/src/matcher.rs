//! Compiled matcher: DNS-applicable rules -> a compact structure answering
//! verdicts within PERFORMANCE.md budgets (1M domains <= 40MB, verdict < 1ms
//! p99, allocation-free lookup, O(labels)).
//!
//! # Layout (why this shape)
//!
//! PERFORMANCE.md forbids per-string control-block overhead and pointer
//! chasing on a 1.4 GHz ARM core. So domains are **not** stored as one
//! `Arc<str>` each (1M of those is ~36 MB of control blocks alone, before any
//! index). Instead:
//!
//! - `arena` — every rule domain (already lowercased by parser normalization),
//!   concatenated into one contiguous allocation. No per-domain allocator
//!   overhead. Hashing and comparison are case-insensitive regardless, so a
//!   caller feeding [`MatcherBuilder::add_rule`] mixed-case domains still
//!   matches — only the reported [`DecisiveRule`] text would echo that case.
//! - `records` — a fixed 8-byte [`Record`] per rule (offset+len into the arena,
//!   flags, list id). Contiguous, cache-friendly.
//! - `slots` — a flat open-addressing hash table of `u32` record indices
//!   (`EMPTY` sentinel). Domain -> record(s); duplicates simply occupy extra
//!   slots. No `Box<str>` keys (which would double the domain bytes).
//!
//! `$dnstype` masks and `$dnsrewrite` payloads are rare, so they live in side
//! maps keyed by record index rather than bloating every [`Record`].
//!
//! # Lookup contract
//!
//! [`Matcher::lookup`] is the hot path and is **allocation-free**: it returns a
//! [`MatchDecision`] carrying a compact [`RuleRef`] (a record index), never a
//! `String`/`Arc`. Materializing the human-readable [`DecisiveRule`] (which
//! allocates the rule text) happens only on the block/allow path via
//! [`Matcher::decisive_rule`] — never for the common `Pass`.

use std::collections::HashMap;

use fah_model::{DecisiveRule, QueryType, Verdict};

use crate::rule::{DomainRule, RuleAction};

const EMPTY: u32 = u32::MAX;

// Record flag bits.
const FLAG_ALLOW: u8 = 1 << 0;
const FLAG_SUBDOMAINS: u8 = 1 << 1;
const FLAG_DNSTYPE: u8 = 1 << 2;
const FLAG_REWRITE: u8 = 1 << 3;

/// One compiled rule: where its domain lives in the arena, plus flags and the
/// owning list. Exactly 8 bytes so 1M rules cost 8 MB (PERFORMANCE.md budget).
#[derive(Clone, Copy)]
#[repr(C)]
struct Record {
    dom_off: u32,
    dom_len: u8,
    flags: u8,
    list_id: u16,
}

impl Record {
    fn is_allow(&self) -> bool {
        self.flags & FLAG_ALLOW != 0
    }
    fn include_subdomains(&self) -> bool {
        self.flags & FLAG_SUBDOMAINS != 0
    }
    fn has_dnstype(&self) -> bool {
        self.flags & FLAG_DNSTYPE != 0
    }
    fn has_rewrite(&self) -> bool {
        self.flags & FLAG_REWRITE != 0
    }
}

/// A compact, `Copy` handle to the rule that decided a [`MatchDecision`].
/// Resolve to a human-readable [`DecisiveRule`] with [`Matcher::decisive_rule`]
/// only when reporting (query log / `rules/test`) — that step allocates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleRef(u32);

/// The allocation-free result of [`Matcher::lookup`]. Mirrors [`Verdict`] but
/// carries a [`RuleRef`] instead of an allocated [`DecisiveRule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchDecision {
    Allow(RuleRef),
    Block(RuleRef),
    Pass,
}

/// DNS record types the matcher understands for `$dnstype` filtering. The
/// table position doubles as a bit index into the mask; unknown types map to
/// no bit (a `$dnstype`-restricted rule then never matches them —
/// conservative, never over-blocks a type we can't reason about).
///
/// Case-insensitive without allocating: this runs on the lookup hot path via
/// [`qtype_bit`], so `to_ascii_uppercase()` (which builds a `String`) is
/// forbidden here.
fn rrtype_bit(name: &str) -> u32 {
    const TYPES: [&str; 15] = [
        "A", "AAAA", "HTTPS", "SVCB", "CNAME", "MX", "TXT", "NS", "PTR", "SRV", "SOA", "CAA", "DS",
        "DNSKEY", "NAPTR",
    ];
    TYPES
        .iter()
        .position(|t| name.eq_ignore_ascii_case(t))
        .map_or(0, |i| 1 << i)
}

/// Every bit [`rrtype_bit`] can produce — the "all known types" mask that
/// negated `$dnstype` values subtract from.
const KNOWN_TYPES_MASK: u32 = (1 << 15) - 1;

fn qtype_bit(qtype: &QueryType) -> u32 {
    match qtype {
        QueryType::A => rrtype_bit("A"),
        QueryType::Aaaa => rrtype_bit("AAAA"),
        QueryType::Other(name) => rrtype_bit(name),
    }
}

/// `$dnstype=A|AAAA` (or comma-separated) -> a bitmask over [`rrtype_bit`].
///
/// AdGuard negation is supported: `$dnstype=~A` means "every type except A".
/// Purely negated values subtract from [`KNOWN_TYPES_MASK`]; mixed values
/// resolve as `positive & !negated`. Types this matcher doesn't know carry no
/// bit, so a negated rule still never matches an unrecognized query type
/// (fails open — under-blocks rather than over-blocks).
fn parse_dnstype_mask(raw: &str) -> u32 {
    let mut positive = 0u32;
    let mut negated = 0u32;
    for token in raw.split(['|', ',']).map(str::trim) {
        if let Some(name) = token.strip_prefix('~') {
            negated |= rrtype_bit(name.trim());
        } else if !token.is_empty() {
            positive |= rrtype_bit(token);
        }
    }
    if positive == 0 && negated != 0 {
        KNOWN_TYPES_MASK & !negated
    } else {
        positive & !negated
    }
}

/// FNV-1a over ASCII-lowercased bytes: case-insensitive hashing so a query's
/// case never matters and matches the lowercased arena. No external dep, and
/// no allocation.
fn hash_domain(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b.to_ascii_lowercase() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Lemire's fastrange: maps a 64-bit hash into `[0, cap)` with one 128-bit
/// multiply and shift — no modulo, and no power-of-two requirement. That lets
/// the table be sized to the exact rule count / load factor instead of rounding
/// up to the next power of two, which for 1M rules would waste ~4 MB of empty
/// slots (PERFORMANCE.md 40 MB budget).
#[inline]
fn fastrange(hash: u64, cap: usize) -> usize {
    ((hash as u128 * cap as u128) >> 64) as usize
}

/// Builds a [`Matcher`] from DNS-applicable rules. Rules are added per list;
/// the open-addressing table is sized and filled once at [`MatcherBuilder::build`].
#[derive(Default)]
pub struct MatcherBuilder {
    arena: Vec<u8>,
    records: Vec<Record>,
    lists: Vec<std::sync::Arc<str>>,
    dnstype: HashMap<u32, (u32, std::sync::Arc<str>)>,
    rewrite: HashMap<u32, std::sync::Arc<str>>,
}

impl MatcherBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a rule list by name, returning its id for [`Self::add_rule`].
    pub fn add_list(&mut self, name: impl Into<std::sync::Arc<str>>) -> u16 {
        let id = u16::try_from(self.lists.len()).expect("at most 65_535 rule lists");
        self.lists.push(name.into());
        id
    }

    /// Registers a list by name and adds every DNS-applicable (active) rule
    /// from its parsed form. Inactive rules are ignored — they carry no
    /// verdict in this phase (RULE_ENGINE.md) and their raw text is not
    /// retained, keeping the compiled structure within budget.
    pub fn add_parsed_list(
        &mut self,
        name: impl Into<std::sync::Arc<str>>,
        list: &crate::rule_list::ParsedRuleList,
    ) {
        let list_id = self.add_list(name);
        for rule in &list.rules {
            if let crate::rule::RuleKind::Active(domain_rule) = &rule.kind {
                self.add_rule(list_id, domain_rule);
            }
        }
    }

    /// Adds one DNS-applicable rule to the given list. Domains longer than 255
    /// bytes (impossible for a valid DNS name, max 253) are skipped.
    pub fn add_rule(&mut self, list_id: u16, rule: &DomainRule) {
        let domain = rule.domain.as_bytes();
        let Ok(dom_len) = u8::try_from(domain.len()) else {
            return;
        };
        if dom_len == 0 {
            return;
        }
        let dom_off = u32::try_from(self.arena.len()).expect("arena within 4 GiB");
        let rec_idx = u32::try_from(self.records.len()).expect("at most u32::MAX rules");

        let mut flags = 0u8;
        if rule.action == RuleAction::Allow {
            flags |= FLAG_ALLOW;
        }
        if rule.include_subdomains {
            flags |= FLAG_SUBDOMAINS;
        }
        if let Some(raw) = &rule.dns_types {
            flags |= FLAG_DNSTYPE;
            self.dnstype
                .insert(rec_idx, (parse_dnstype_mask(raw), raw.clone()));
        }
        if let Some(raw) = &rule.dns_rewrite {
            flags |= FLAG_REWRITE;
            self.rewrite.insert(rec_idx, raw.clone());
        }

        self.arena.extend_from_slice(domain);
        self.records.push(Record {
            dom_off,
            dom_len,
            flags,
            list_id,
        });
    }

    pub fn build(self) -> Matcher {
        let count = self.records.len();
        // Load factor ~0.7 keeps probe chains short. Exact (non-power-of-two)
        // capacity via fastrange avoids rounding up ~4 MB of empty slots.
        let cap = (count.saturating_mul(10) / 7).max(8);
        let mut slots = vec![EMPTY; cap];

        for (idx, rec) in self.records.iter().enumerate() {
            let domain =
                &self.arena[rec.dom_off as usize..rec.dom_off as usize + rec.dom_len as usize];
            let mut slot = fastrange(hash_domain(domain), cap);
            while slots[slot] != EMPTY {
                slot += 1;
                if slot == cap {
                    slot = 0;
                }
            }
            slots[slot] = idx as u32;
        }

        Matcher {
            arena: self.arena.into_boxed_slice(),
            records: self.records.into_boxed_slice(),
            slots: slots.into_boxed_slice(),
            slot_cap: cap,
            lists: self.lists.into_boxed_slice(),
            dnstype: self.dnstype,
            rewrite: self.rewrite,
        }
    }
}

/// The compiled, immutable matcher. Swapped atomically on reload (p1-03); the
/// hot path only ever reads it, never locks it (PERFORMANCE.md).
pub struct Matcher {
    arena: Box<[u8]>,
    records: Box<[Record]>,
    slots: Box<[u32]>,
    slot_cap: usize,
    lists: Box<[std::sync::Arc<str>]>,
    dnstype: HashMap<u32, (u32, std::sync::Arc<str>)>,
    rewrite: HashMap<u32, std::sync::Arc<str>>,
}

impl Matcher {
    /// Number of compiled rules.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    fn domain_of(&self, idx: u32) -> &[u8] {
        let rec = &self.records[idx as usize];
        &self.arena[rec.dom_off as usize..rec.dom_off as usize + rec.dom_len as usize]
    }

    /// Answers a verdict for `domain`/`qtype`. **Allocation-free** — the hot
    /// path (PERFORMANCE.md). Precedence: allow > block > pass; the most
    /// specific matching rule of the winning class is decisive.
    ///
    /// Walks the query's label suffixes most-specific first. The first `allow`
    /// found wins outright (nothing beats allow), so it returns immediately.
    /// A `block` is remembered but never returned early — a less specific
    /// `allow` at a parent label must still be able to override it.
    pub fn lookup(&self, domain: &str, qtype: &QueryType) -> MatchDecision {
        let query = domain.strip_suffix('.').unwrap_or(domain).as_bytes();
        let qbit = qtype_bit(qtype);
        let mut blocked: Option<u32> = None;

        let mut start = 0usize;
        loop {
            let suffix = &query[start..];
            let is_subdomain_level = suffix.len() != query.len();

            let mut slot = fastrange(hash_domain(suffix), self.slot_cap);
            while self.slots[slot] != EMPTY {
                let idx = self.slots[slot];
                if self.domain_of(idx).eq_ignore_ascii_case(suffix) {
                    let rec = &self.records[idx as usize];
                    let applies = (!is_subdomain_level || rec.include_subdomains())
                        && (!rec.has_dnstype() || self.dnstype[&idx].0 & qbit != 0);
                    if applies {
                        if rec.is_allow() {
                            return MatchDecision::Allow(RuleRef(idx));
                        } else if blocked.is_none() {
                            blocked = Some(idx);
                        }
                    }
                }
                slot += 1;
                if slot == self.slot_cap {
                    slot = 0;
                }
            }

            // Advance to the parent label (drop the leftmost label).
            match suffix.iter().position(|&b| b == b'.') {
                Some(rel) => start += rel + 1,
                None => break,
            }
        }

        match blocked {
            Some(idx) => MatchDecision::Block(RuleRef(idx)),
            None => MatchDecision::Pass,
        }
    }

    /// `$dnsrewrite` payload for a decided rule, if any — used by fah-dns for
    /// answer synthesis (interpretation is out of scope here). Borrow, no alloc.
    pub fn rewrite(&self, r: RuleRef) -> Option<&str> {
        let rec = &self.records[r.0 as usize];
        if rec.has_rewrite() {
            self.rewrite.get(&r.0).map(|s| &**s)
        } else {
            None
        }
    }

    /// Materializes the human-readable decisive rule + owning list for the
    /// query log / `rules/test`. Allocates the reconstructed rule text — call
    /// only off the hot path (block/allow, never `Pass`).
    pub fn decisive_rule(&self, r: RuleRef) -> DecisiveRule {
        let rec = &self.records[r.0 as usize];
        let domain = std::str::from_utf8(self.domain_of(r.0)).unwrap_or("");
        let mut text = String::with_capacity(domain.len() + 6);
        if rec.is_allow() {
            text.push_str("@@");
        }
        text.push_str("||");
        text.push_str(domain);
        text.push('^');
        // AdGuard option syntax: one `$`, further options comma-separated.
        let mut sep = '$';
        if let Some((_, raw)) = self.dnstype.get(&r.0) {
            text.push(sep);
            text.push_str("dnstype=");
            text.push_str(raw);
            sep = ',';
        }
        if let Some(raw) = self.rewrite.get(&r.0) {
            text.push(sep);
            text.push_str("dnsrewrite=");
            text.push_str(raw);
        }
        DecisiveRule::new(self.lists[rec.list_id as usize].clone(), text)
    }

    /// Convenience: full [`Verdict`] with materialized decisive rule. Allocates
    /// on allow/block — for tests and `rules/test`, not the hot path.
    pub fn verdict(&self, domain: &str, qtype: &QueryType) -> Verdict {
        match self.lookup(domain, qtype) {
            MatchDecision::Allow(r) => Verdict::Allow(self.decisive_rule(r)),
            MatchDecision::Block(r) => Verdict::Block(self.decisive_rule(r)),
            MatchDecision::Pass => Verdict::Pass,
        }
    }

    /// Approximate resident bytes of the compiled structure — the figure the
    /// PERFORMANCE.md `<= 40 MB / 1M domains` budget is measured against.
    pub fn heap_bytes(&self) -> usize {
        let arc_overhead = 16; // control block, approx
        let lists: usize = self.lists.iter().map(|s| s.len() + arc_overhead).sum();
        let dnstype: usize = self
            .dnstype
            .values()
            .map(|(_, raw)| 4 + 4 + arc_overhead + raw.len() + 8)
            .sum();
        let rewrite: usize = self
            .rewrite
            .values()
            .map(|raw| 4 + arc_overhead + raw.len() + 8)
            .sum();
        self.arena.len()
            + self.records.len() * std::mem::size_of::<Record>()
            + self.slots.len() * std::mem::size_of::<u32>()
            + lists
            + dnstype
            + rewrite
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::DomainRule;
    use std::sync::Arc;

    fn block(domain: &str) -> DomainRule {
        DomainRule {
            domain: Arc::from(domain),
            action: RuleAction::Block,
            include_subdomains: true,
            dns_types: None,
            dns_rewrite: None,
        }
    }

    fn allow(domain: &str) -> DomainRule {
        DomainRule {
            action: RuleAction::Allow,
            ..block(domain)
        }
    }

    fn matcher_with(rules: &[(&str, DomainRule)]) -> Matcher {
        let mut b = MatcherBuilder::new();
        let list = b.add_list("test");
        for (_, rule) in rules {
            b.add_rule(list, rule);
        }
        b.build()
    }

    fn decision(m: &Matcher, domain: &str) -> MatchDecision {
        m.lookup(domain, &QueryType::A)
    }

    #[test]
    fn exact_domain_blocks() {
        let m = matcher_with(&[("", block("ads.example.com"))]);
        assert!(matches!(
            decision(&m, "ads.example.com"),
            MatchDecision::Block(_)
        ));
    }

    #[test]
    fn subdomain_of_block_rule_blocks() {
        let m = matcher_with(&[("", block("example.com"))]);
        assert!(matches!(
            decision(&m, "tracker.ads.example.com"),
            MatchDecision::Block(_)
        ));
    }

    #[test]
    fn unrelated_domain_passes() {
        let m = matcher_with(&[("", block("example.com"))]);
        assert_eq!(decision(&m, "example.org"), MatchDecision::Pass);
    }

    #[test]
    fn parent_of_block_rule_passes() {
        // Rule on ads.example.com must not block its parent example.com.
        let m = matcher_with(&[("", block("ads.example.com"))]);
        assert_eq!(decision(&m, "example.com"), MatchDecision::Pass);
    }

    #[test]
    fn sibling_label_prefix_does_not_falsely_match() {
        // "notexample.com" must not match a rule for "example.com".
        let m = matcher_with(&[("", block("example.com"))]);
        assert_eq!(decision(&m, "notexample.com"), MatchDecision::Pass);
    }

    #[test]
    fn allow_beats_block_same_domain() {
        let m = matcher_with(&[("", block("example.com")), ("", allow("example.com"))]);
        assert!(matches!(
            decision(&m, "example.com"),
            MatchDecision::Allow(_)
        ));
    }

    #[test]
    fn less_specific_allow_overrides_more_specific_block() {
        // block ads.example.com, allow example.com -> allow wins (allow > block).
        let m = matcher_with(&[("", block("ads.example.com")), ("", allow("example.com"))]);
        assert!(matches!(
            decision(&m, "ads.example.com"),
            MatchDecision::Allow(_)
        ));
    }

    #[test]
    fn case_insensitive_lookup() {
        let m = matcher_with(&[("", block("ads.example.com"))]);
        assert!(matches!(
            decision(&m, "ADS.Example.COM"),
            MatchDecision::Block(_)
        ));
    }

    #[test]
    fn trailing_dot_is_ignored() {
        let m = matcher_with(&[("", block("ads.example.com"))]);
        assert!(matches!(
            decision(&m, "ads.example.com."),
            MatchDecision::Block(_)
        ));
    }

    #[test]
    fn dnstype_restricts_matching_qtype() {
        let rule = DomainRule {
            dns_types: Some(Arc::from("A")),
            ..block("ads.example.com")
        };
        let m = matcher_with(&[("", rule)]);
        assert!(matches!(
            m.lookup("ads.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));
        // AAAA query is outside the rule's $dnstype=A -> not blocked.
        assert_eq!(
            m.lookup("ads.example.com", &QueryType::Aaaa),
            MatchDecision::Pass
        );
    }

    #[test]
    fn dnstype_negation_matches_everything_except_listed() {
        // $dnstype=~A: rule applies to every known type except A.
        let rule = DomainRule {
            dns_types: Some(Arc::from("~A")),
            ..block("ads.example.com")
        };
        let m = matcher_with(&[("", rule)]);
        assert_eq!(
            m.lookup("ads.example.com", &QueryType::A),
            MatchDecision::Pass
        );
        assert!(matches!(
            m.lookup("ads.example.com", &QueryType::Aaaa),
            MatchDecision::Block(_)
        ));
        assert!(matches!(
            m.lookup("ads.example.com", &QueryType::Other("HTTPS".into())),
            MatchDecision::Block(_)
        ));
    }

    #[test]
    fn other_qtype_name_is_case_insensitive() {
        let rule = DomainRule {
            dns_types: Some(Arc::from("HTTPS")),
            ..block("ads.example.com")
        };
        let m = matcher_with(&[("", rule)]);
        assert!(matches!(
            m.lookup("ads.example.com", &QueryType::Other("https".into())),
            MatchDecision::Block(_)
        ));
    }

    #[test]
    fn dnsrewrite_payload_is_carried() {
        let rule = DomainRule {
            dns_rewrite: Some(Arc::from("0.0.0.0")),
            ..block("rewrite.example.com")
        };
        let m = matcher_with(&[("", rule)]);
        let MatchDecision::Block(r) = m.lookup("rewrite.example.com", &QueryType::A) else {
            panic!("expected block");
        };
        assert_eq!(m.rewrite(r), Some("0.0.0.0"));
    }

    #[test]
    fn decisive_rule_reports_list_and_reconstructed_text() {
        let mut b = MatcherBuilder::new();
        let list = b.add_list("oisd");
        b.add_rule(list, &block("ads.example.com"));
        let m = b.build();
        let MatchDecision::Block(r) = m.lookup("ads.example.com", &QueryType::A) else {
            panic!("expected block");
        };
        let decisive = m.decisive_rule(r);
        assert_eq!(&*decisive.list, "oisd");
        assert_eq!(&*decisive.rule, "||ads.example.com^");
    }

    #[test]
    fn decisive_rule_separates_multiple_options_with_comma() {
        // AdGuard syntax: one `$`, options comma-separated.
        let rule = DomainRule {
            dns_types: Some(Arc::from("A")),
            dns_rewrite: Some(Arc::from("0.0.0.0")),
            ..block("ads.example.com")
        };
        let m = matcher_with(&[("", rule)]);
        let MatchDecision::Block(r) = m.lookup("ads.example.com", &QueryType::A) else {
            panic!("expected block");
        };
        assert_eq!(
            &*m.decisive_rule(r).rule,
            "||ads.example.com^$dnstype=A,dnsrewrite=0.0.0.0"
        );
    }

    #[test]
    fn verdict_materializes_allow() {
        let m = matcher_with(&[("", allow("cdn.example.com"))]);
        match m.verdict("cdn.example.com", &QueryType::A) {
            Verdict::Allow(d) => assert_eq!(&*d.rule, "@@||cdn.example.com^"),
            other => panic!("expected allow, got {other:?}"),
        }
    }

    #[test]
    fn empty_matcher_passes_everything() {
        let m = matcher_with(&[]);
        assert_eq!(decision(&m, "anything.example.com"), MatchDecision::Pass);
        assert!(m.is_empty());
    }
}
