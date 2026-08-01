//! Compiled matcher: DNS-applicable rules -> a compact structure answering
//! verdicts within PERFORMANCE.md budgets (1M domains <= 40MB, verdict < 1ms
//! p99, allocation-free lookup, O(labels)).
//!
//! # Layout (why this shape)
//!
//! PERFORMANCE.md forbids per-string control-block overhead and pointer
//! chasing on the RB5009, whose measured single-threaded throughput is roughly
//! 9× slower than the dev box. So domains are **not** stored as one
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
//!   (`EMPTY` sentinel). Domain -> record(s); several rules for the same
//!   domain occupy several slots. No `Box<str>` keys (which would double the
//!   domain bytes).
//!
//! `$dnstype` masks and `$dnsrewrite` payloads are rare, so they live in side
//! maps keyed by record index rather than bloating every [`Record`].
//!
//! # Deduplication (build time only)
//!
//! Lists overlap heavily — AdGuard's `filter_48` and HaGeZi's `pro` are nearly
//! the same corpus — and every duplicate used to cost arena bytes, an 8-byte
//! [`Record`] and ~1.43 slots. [`MatcherBuilder::add_rule`] therefore drops a
//! rule whose **full identity** (domain, action, subdomain flag, `$dnstype`,
//! `$dnsrewrite`; the owning list is attribution, not identity) already exists
//! — before its bytes are appended, so nothing has to be reclaimed later.
//!
//! The index doing that is a transient open-addressing table of *record
//! indices* ([`MatcherBuilder::dedup`]) — ~8 MB at 1M rules, dropped at
//! [`MatcherBuilder::build`]. The arena stays the single source of truth: the
//! identity hash only picks the probe start, and every accept/reject is a real
//! byte-and-field comparison against arena + [`Record::flags`] + the side
//! maps, so a hash collision can never silently drop a distinct rule. Output
//! is decided by insertion order (first list to supply a rule wins, compile
//! order = enabled lists then user rules), never by probe order, so the table's
//! capacity cannot change the compiled result.
//!
//! # Lookup contract
//!
//! [`Matcher::lookup`] is the hot path and is **allocation-free**: it returns a
//! [`MatchDecision`] carrying a compact [`RuleRef`] (a record index), never a
//! `String`/`Arc`. Materializing the human-readable [`DecisiveRule`] (which
//! allocates the rule text) happens only on the block/allow path via
//! [`Matcher::decisive_rule`] — never for the common `Pass`. Dedup is a
//! build-time step and touches none of it.

use std::collections::HashMap;

use fah_model::{DecisiveRule, HttpRequest, PolicyId, QueryType, Verdict};

use crate::policy::{ClientScope, ALL_POLICIES};
use crate::rule::{DomainRule, RuleAction, UrlRule};
use crate::url_matcher::{UrlDecision, UrlIndex, UrlIndexBuilder};

pub(crate) const EMPTY: u32 = u32::MAX;

// Record flag bits.
const FLAG_ALLOW: u8 = 1 << 0;
const FLAG_SUBDOMAINS: u8 = 1 << 1;
const FLAG_DNSTYPE: u8 = 1 << 2;
const FLAG_REWRITE: u8 = 1 << 3;
const FLAG_CLIENT: u8 = 1 << 4;

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
    fn has_client(&self) -> bool {
        self.flags & FLAG_CLIENT != 0
    }
}

/// Who is asking, and under which policy — everything a rule may be scoped to
/// beyond the question itself.
///
/// [`Default`] is the pre-Policies behaviour: the default policy, and a client
/// the caller cannot name. `Matcher::lookup` and `Matcher::lookup_http` use it,
/// so every existing caller keeps its signature and its meaning; p2-06 is what
/// threads a real context through the two pipelines.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientContext<'a> {
    pub policy: PolicyId,
    /// The client's source address, when there is a request behind the lookup.
    pub ip: Option<std::net::IpAddr>,
    /// The client's user-assigned name (CONTEXT.md §Client), if it has one.
    pub name: Option<&'a str>,
}

impl ClientContext<'_> {
    /// The policy bit tested against a rule's mask.
    #[inline]
    fn policy_bit(&self) -> u16 {
        self.policy.bit()
    }
}

/// Marks a [`RuleRef`] as pointing into the URL tier rather than the domain
/// one. A tag bit rather than a second ref type: both tiers answer the same
/// [`MatchDecision`], so a caller never has to know which one decided, and
/// [`Matcher::decisive_rule`] dispatches on it. Rule counts are bounded by the
/// 40 MB budget long before 2^31, so the bit is never contested.
const URL_TIER: u32 = 1 << 31;

/// A compact, `Copy` handle to the rule that decided a [`MatchDecision`].
/// Resolve to a human-readable [`DecisiveRule`] with [`Matcher::decisive_rule`]
/// only when reporting (query log / `rules/test`) — that step allocates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleRef(u32);

impl RuleRef {
    /// True when the deciding rule is a URL-tier rule (RULE_ENGINE.md: HTTP
    /// matching), false when it is a domain rule.
    pub fn is_url_rule(&self) -> bool {
        self.0 & URL_TIER != 0
    }

    fn index(&self) -> u32 {
        self.0 & !URL_TIER
    }
}

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

/// Continues an FNV-1a chain over `bytes` **verbatim** — no case folding.
/// Used only to extend [`hash_domain`] with the rest of a rule's identity for
/// dedup; option payloads (`$dnstype`, `$dnsrewrite`) are compared
/// byte-exactly, so they must hash byte-exactly too.
fn hash_extend(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Hash of a rule's full identity — domain (case-folded, as the arena stores
/// it), flags, and any option payloads. Only ever a probe accelerator: the
/// dedup path confirms every candidate with a real comparison, so two
/// identities colliding here costs one extra comparison and nothing else.
fn identity_hash(
    domain: &[u8],
    flags: u8,
    dns_types: Option<&str>,
    rewrite: Option<&str>,
    client: Option<&str>,
) -> u64 {
    let mut h = hash_extend(hash_domain(domain), &[flags]);
    for raw in [dns_types, rewrite, client].into_iter().flatten() {
        h = hash_extend(h, raw.as_bytes());
    }
    h
}

/// Where an incoming rule lands in the transient dedup index.
enum DedupSlot {
    /// Free slot for a rule not yet compiled.
    Vacant(usize),
    /// The record index already carrying this exact rule.
    Duplicate(u32),
}

/// Slot count for the compiled `slots` table: a ~0.7 load factor, which keeps
/// probe chains short without spending memory the 40 MB budget needs. Never
/// zero, so probing always has somewhere to land.
fn table_slots(entries: usize) -> usize {
    (entries.saturating_mul(10) / 7).max(8)
}

/// Slot count for the *transient* dedup index — a 0.5 load factor, looser than
/// the compiled table's 0.7 on purpose, and the measured optimum.
///
/// Every insert during a compile is an *unsuccessful* search, which for linear
/// probing is the expensive case (~6 probes at 0.7 against ~2.5 at 0.5), and
/// each occupied slot walked past may pull a `Record` and its arena bytes in
/// from DRAM to be compared. Measured on a 1M-unique-rule build, pinned:
/// **135.7 ms at 0.7, 93.3 ms at 0.5** — against 41.6 ms for a build that does
/// no dedup at all. Storing a 32-bit hash tag beside each index to reject
/// collisions without touching the arena was tried and *rejected*: it doubled
/// the slot to 8 bytes and measured 124 ms at 0.7 load, worse than the plain
/// 4-byte index at 0.5 for 1.4× the compile-time memory. What remains is
/// essentially one random memory access per rule, which is the floor for
/// membership-testing 1M rules against a 1M-entry index.
///
/// Cost of the looser factor: 4 bytes per extra slot for the duration of one
/// compile (~8 MB at 1M rules, freed at `build()`).
fn dedup_slots(entries: usize) -> usize {
    entries.saturating_mul(2).max(8)
}

/// Ceiling on the *up-front* dedup index [`MatcherBuilder::with_capacity`] will
/// pre-allocate, regardless of the `expected_rules` ceiling it is handed.
///
/// `expected_rules` comes from [`crate::parser::rule_upper_bound`], a pre-parse
/// *token* count — a correct ceiling, but adversarially loose: a 64 MiB list
/// (the `MAX_LIST_BYTES` cap) of single-char space-separated tokens is ~33.5M
/// "rules" that parse to **nothing**, which would size the transient index at
/// ~268 MB before parsing discovers there are no real rules. On the 1 GB RB5009
/// a few such lists abort the process at `handle_alloc_error`. This clamp bounds
/// the pre-allocation to ~32 MB; [`MatcherBuilder::reserve_dedup`] still grows
/// the index if a (unreachable-on-device) corpus really exceeds it, and the
/// output is capacity-inert (determinism test), so clamping changes nothing
/// observable. 4M rules already implies a compiled matcher well past the 40 MB
/// budget, so the clamp never binds a legitimate corpus.
const MAX_PREALLOC_RULES: usize = 4_000_000;

/// Lemire's fastrange: maps a 64-bit hash into `[0, cap)` with one 128-bit
/// multiply and shift — no modulo, and no power-of-two requirement. That lets
/// the table be sized to the exact rule count / load factor instead of rounding
/// up to the next power of two, which for 1M rules would waste ~4 MB of empty
/// slots (PERFORMANCE.md 40 MB budget).
#[inline]
pub(crate) fn fastrange(hash: u64, cap: usize) -> usize {
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
    /// Compiled `$client=` scopes, keyed by record index. A side map like the
    /// two above and for the same reason: the deployed corpus carries zero of
    /// these and the public lists one apiece, so the cost belongs on the rules
    /// that use it rather than on every [`Record`].
    clients: HashMap<u32, ClientScope>,
    /// Which policies can see each list's rules, parallel to `lists`.
    list_policy: Vec<u16>,
    /// Which policies can see each *record*, parallel to `records`. Not derived
    /// from `list_policy` at lookup time because deduplication collapses a rule
    /// arriving from several lists into one record attributed to the first —
    /// so this is the union of every contributing list's mask.
    record_policy: Vec<u16>,
    /// Transient dedup index: an open-addressing table of *record indices*
    /// keyed by [`identity_hash`], never owned copies of the rules. Dropped
    /// with the builder at [`Self::build`], so it costs compile-time memory
    /// only — ~8 MB at 1M distinct rules, against the ~60–70 MB an owned
    /// `HashSet` of keys would have pinned for the same job, and it pins no
    /// `Arc` refcounts.
    dedup: Vec<u32>,
    duplicates_removed: usize,
    /// The URL tier (p2-03). Built alongside the domain tier from the same
    /// parsed lists, so one `add_parsed_list` fills both and the two can never
    /// disagree about which lists are loaded.
    url: UrlIndexBuilder,
}

impl MatcherBuilder {
    /// A builder whose dedup index grows on demand. Fine for tests, the API's
    /// small ad-hoc builds and an empty ruleset; the compile path uses
    /// [`Self::with_capacity`] so the index is allocated exactly once.
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-sizes the transient dedup index for `expected_rules` — one
    /// allocation up front, freed at [`Self::build`], with no
    /// grow-and-rehash churn part-way through a 1M-rule compile. The caller
    /// passes a *ceiling* on the rule count (`parser::rule_upper_bound`);
    /// overshooting wastes 4 bytes per unused slot, undershooting only costs
    /// the one rehash this parameter exists to avoid. Capacity is a
    /// performance knob and nothing else: the compiled arena, records and
    /// slots come out byte-identical whatever it is set to.
    ///
    /// The pre-allocation is clamped at [`MAX_PREALLOC_RULES`] so an
    /// adversarially inflated ceiling (a hostile list of tokens that parse to
    /// nothing) cannot force a multi-hundred-MB allocation before parsing runs;
    /// [`Self::reserve_dedup`] grows the index for the rare real overflow.
    pub fn with_capacity(expected_rules: usize) -> Self {
        Self {
            dedup: vec![EMPTY; dedup_slots(expected_rules.min(MAX_PREALLOC_RULES))],
            ..Self::default()
        }
    }

    /// Registers a rule list by name, returning its id for [`Self::add_rule`].
    /// The list is visible to every policy; [`Self::add_list_masked`] narrows
    /// it.
    pub fn add_list(&mut self, name: impl Into<std::sync::Arc<str>>) -> u16 {
        self.add_list_masked(name, ALL_POLICIES)
    }

    /// Registers a rule list visible only to the policies in `policy_mask`
    /// (`PolicySet::mask_for_list`).
    pub fn add_list_masked(
        &mut self,
        name: impl Into<std::sync::Arc<str>>,
        policy_mask: u16,
    ) -> u16 {
        let id = u16::try_from(self.lists.len()).expect("at most 65_535 rule lists");
        self.lists.push(name.into());
        self.list_policy.push(policy_mask);
        id
    }

    /// Registers a list by name and adds every rule from its parsed form that
    /// some tier can answer: domain rules to the domain tier, URL rules to the
    /// URL tier. Inactive rules are still ignored — they carry no verdict
    /// (RULE_ENGINE.md) and retain no text to compile.
    pub fn add_parsed_list(
        &mut self,
        name: impl Into<std::sync::Arc<str>>,
        list: &crate::rule_list::ParsedRuleList,
    ) {
        self.add_parsed_list_masked(name, list, ALL_POLICIES);
    }

    /// [`Self::add_parsed_list`], with the list visible only to the policies in
    /// `policy_mask`.
    pub fn add_parsed_list_masked(
        &mut self,
        name: impl Into<std::sync::Arc<str>>,
        list: &crate::rule_list::ParsedRuleList,
        policy_mask: u16,
    ) {
        let list_id = self.add_list_masked(name, policy_mask);
        for rule in &list.rules {
            match &rule.kind {
                crate::rule::RuleKind::Active(domain_rule) => self.add_rule(list_id, domain_rule),
                crate::rule::RuleKind::Url(url_rule) => self.add_url_rule(list_id, url_rule),
                crate::rule::RuleKind::Inactive(_) => {}
            }
        }
    }

    /// Adds one request-applicable rule to the URL tier.
    pub fn add_url_rule(&mut self, list_id: u16, rule: &UrlRule) {
        self.url
            .add(list_id, rule, self.list_policy[list_id as usize]);
    }

    /// Adds one DNS-applicable rule to the given list. Domains longer than 255
    /// bytes (impossible for a valid DNS name, max 253) are skipped, and so is
    /// a rule whose full identity was already added — see the module's
    /// "Deduplication" section. A duplicate returns before anything is
    /// appended, so its arena bytes, record and slot are never allocated at
    /// all; the first list to supply the rule keeps the attribution.
    pub fn add_rule(&mut self, list_id: u16, rule: &DomainRule) {
        let domain = rule.domain.as_bytes();
        let Ok(dom_len) = u8::try_from(domain.len()) else {
            return;
        };
        if dom_len == 0 {
            return;
        }

        let mut flags = 0u8;
        if rule.action == RuleAction::Allow {
            flags |= FLAG_ALLOW;
        }
        if rule.include_subdomains {
            flags |= FLAG_SUBDOMAINS;
        }
        if rule.dns_types.is_some() {
            flags |= FLAG_DNSTYPE;
        }
        if rule.dns_rewrite.is_some() {
            flags |= FLAG_REWRITE;
        }

        // A `$client` payload the parser accepted but that compiles to no
        // selector would leave the rule applying to everyone. Dropped instead:
        // the restriction is part of the rule, not decoration on it.
        let scope = match &rule.client {
            None => None,
            Some(raw) => match ClientScope::parse(raw) {
                Some(scope) => {
                    flags |= FLAG_CLIENT;
                    Some(scope)
                }
                None => return,
            },
        };

        let policy_mask = self.list_policy[list_id as usize];
        let slot = match self.dedup_slot_for(rule, domain, flags, scope.as_ref()) {
            DedupSlot::Vacant(slot) => slot,
            // The rule is already compiled, from another list. Attribution
            // stays with the first list to supply it, but *visibility* is the
            // union: a policy that enabled only this list must still see it.
            DedupSlot::Duplicate(existing) => {
                self.record_policy[existing as usize] |= policy_mask;
                self.duplicates_removed += 1;
                return;
            }
        };

        let dom_off = u32::try_from(self.arena.len()).expect("arena within 4 GiB");
        let rec_idx = u32::try_from(self.records.len()).expect("at most u32::MAX rules");
        if let Some(raw) = &rule.dns_types {
            self.dnstype
                .insert(rec_idx, (parse_dnstype_mask(raw), raw.clone()));
        }
        if let Some(raw) = &rule.dns_rewrite {
            self.rewrite.insert(rec_idx, raw.clone());
        }
        if let Some(scope) = scope {
            self.clients.insert(rec_idx, scope);
        }

        self.arena.extend_from_slice(domain);
        self.records.push(Record {
            dom_off,
            dom_len,
            flags,
            list_id,
        });
        self.record_policy.push(policy_mask);
        self.dedup[slot] = rec_idx;
    }

    /// Probes the dedup index for `rule`'s identity: where the new record index
    /// goes, or which record already carries this rule. The hash picks where to
    /// start looking; acceptance is always [`Self::identity_matches`] reading
    /// the arena back, never the hash.
    fn dedup_slot_for(
        &mut self,
        rule: &DomainRule,
        domain: &[u8],
        flags: u8,
        scope: Option<&ClientScope>,
    ) -> DedupSlot {
        self.reserve_dedup();
        let cap = self.dedup.len();
        let mut slot = fastrange(
            identity_hash(
                domain,
                flags,
                rule.dns_types.as_deref(),
                rule.dns_rewrite.as_deref(),
                scope.map(|scope| &*scope.raw),
            ),
            cap,
        );
        // `reserve_dedup` keeps the table under its load factor, so an empty
        // slot always exists and this walk always terminates.
        while self.dedup[slot] != EMPTY {
            if self.identity_matches(self.dedup[slot], rule, domain, flags, scope) {
                return DedupSlot::Duplicate(self.dedup[slot]);
            }
            slot += 1;
            if slot == cap {
                slot = 0;
            }
        }
        DedupSlot::Vacant(slot)
    }

    /// Full-identity comparison of an already-compiled record against an
    /// incoming rule, read back from the arena, the record's flags and (only
    /// when the flags say so) the option side maps. `list_id` is deliberately
    /// not compared: attribution is informational, never part of a verdict.
    fn identity_matches(
        &self,
        idx: u32,
        rule: &DomainRule,
        domain: &[u8],
        flags: u8,
        scope: Option<&ClientScope>,
    ) -> bool {
        let rec = &self.records[idx as usize];
        // The side-map indexing below is guarded by these flag bits; the
        // invariant is that `add_rule` inserts the map entry for every record
        // that sets the flag. Enforce it in debug rather than only arguing it.
        debug_assert!(
            !rec.has_dnstype() || self.dnstype.contains_key(&idx),
            "FLAG_DNSTYPE record {idx} is missing its dnstype side-map entry"
        );
        debug_assert!(
            !rec.has_rewrite() || self.rewrite.contains_key(&idx),
            "FLAG_REWRITE record {idx} is missing its rewrite side-map entry"
        );
        if rec.flags != flags {
            return false;
        }
        let stored = &self.arena[rec.dom_off as usize..rec.dom_off as usize + rec.dom_len as usize];
        if !stored.eq_ignore_ascii_case(domain) {
            return false;
        }
        // Equal flags mean both sides agree on whether each option is
        // present, so `unwrap_or_default` here is unreachable, not a fallback.
        if rec.has_dnstype() && &*self.dnstype[&idx].1 != rule.dns_types.as_deref().unwrap_or("") {
            return false;
        }
        if rec.has_rewrite() && &*self.rewrite[&idx] != rule.dns_rewrite.as_deref().unwrap_or("") {
            return false;
        }
        // Two rules that differ only in who they apply to are two rules.
        if rec.has_client() && &*self.clients[&idx].raw != scope.map(|s| &*s.raw).unwrap_or("") {
            return false;
        }
        true
    }

    /// Keeps the dedup index under its load factor. A builder from
    /// [`Self::with_capacity`] was sized for the whole compile and never
    /// enters the rehash branch; one from [`Self::new`] grows here instead.
    /// Rehashing changes where indices sit, never which rules survive — the
    /// table is membership-only and is never iterated to produce output.
    fn reserve_dedup(&mut self) {
        let needed = self.records.len() + 1;
        if needed * 2 <= self.dedup.len() {
            return;
        }
        // Doubling in *entry* terms, so an un-hinted builder pays an
        // amortized-constant number of rehashes rather than one per insert.
        let mut grown = vec![EMPTY; dedup_slots(needed * 2)];
        let cap = grown.len();
        for (idx, rec) in self.records.iter().enumerate() {
            let domain =
                &self.arena[rec.dom_off as usize..rec.dom_off as usize + rec.dom_len as usize];
            let idx = idx as u32;
            let mut slot = fastrange(
                identity_hash(
                    domain,
                    rec.flags,
                    self.dnstype.get(&idx).map(|(_, raw)| raw.as_ref()),
                    self.rewrite.get(&idx).map(|raw| raw.as_ref()),
                    self.clients.get(&idx).map(|scope| &*scope.raw),
                ),
                cap,
            );
            while grown[slot] != EMPTY {
                slot += 1;
                if slot == cap {
                    slot = 0;
                }
            }
            grown[slot] = idx;
        }
        self.dedup = grown;
    }

    /// How many rules were dropped as exact duplicates of one already added.
    pub fn duplicates_removed(&self) -> usize {
        self.duplicates_removed
    }

    pub fn build(self) -> Matcher {
        let count = self.records.len();
        // Load factor ~0.7 keeps probe chains short. Exact (non-power-of-two)
        // capacity via fastrange avoids rounding up ~4 MB of empty slots.
        let cap = table_slots(count);
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

        // A mask array is only worth its bytes once some rule is invisible to
        // some policy, and an empty array is the flag that skips the test on
        // the hot path. The comparison is against the union of every list's
        // mask rather than against `ALL_POLICIES`: with three policies all
        // enabling every list the masks are `0b111`, which filters nothing but
        // is not the sentinel. Both the zero-config case and that one drop the
        // array, so neither pays the ~1 MB nor the per-candidate check.
        let full = self.list_policy.iter().fold(0u16, |acc, &mask| acc | mask);
        let policy_mask = if self.record_policy.iter().all(|&mask| mask == full) {
            Box::default()
        } else {
            self.record_policy.into_boxed_slice()
        };

        // `self.dedup` is dropped here with the builder: the dedup index is
        // compile-time working memory and never rides along with the matcher.
        Matcher {
            arena: self.arena.into_boxed_slice(),
            records: self.records.into_boxed_slice(),
            slots: slots.into_boxed_slice(),
            slot_cap: cap,
            lists: self.lists.into_boxed_slice(),
            dnstype: self.dnstype,
            rewrite: self.rewrite,
            clients: self.clients,
            policy_mask,
            duplicates_removed: self.duplicates_removed,
            url: self.url.build(full),
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
    clients: HashMap<u32, ClientScope>,
    /// Which policies can see each record. **Empty when every rule is visible
    /// to every policy**, which is both the zero-config case and the signal to
    /// skip the check entirely.
    policy_mask: Box<[u16]>,
    duplicates_removed: usize,
    url: UrlIndex,
}

impl Matcher {
    /// Number of compiled rules — **distinct** rules, since p1.5-05: two lists
    /// carrying the same rule contribute one record, not two.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// How many rules the compile dropped as exact duplicates of one already
    /// present, across every list in it. Reported through metrics and the
    /// lists API so an overlapping pair of lists is visible as overlap rather
    /// than as a mysteriously small rule count.
    pub fn duplicates_removed(&self) -> usize {
        self.duplicates_removed
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
        self.lookup_in(domain, qtype, &ClientContext::default())
    }

    /// [`Self::lookup`] under a policy, for a named client — the entry point
    /// p2-06 threads through the DNS pipeline. Rules from a list the policy
    /// does not enable, and `$client` rules scoped to somebody else, do not
    /// participate in the decision at all: they are filtered *during* the walk,
    /// not after it, so an excluded list's exception cannot suppress a block
    /// the policy should still see.
    pub fn lookup_in(
        &self,
        domain: &str,
        qtype: &QueryType,
        ctx: &ClientContext<'_>,
    ) -> MatchDecision {
        self.lookup_domain(domain, Some(qtype_bit(qtype)), ctx)
    }

    /// Whether `idx` participates in a decision made for `ctx`.
    #[inline]
    fn visible(&self, idx: u32, ctx: &ClientContext<'_>) -> bool {
        if !self.policy_mask.is_empty() && self.policy_mask[idx as usize] & ctx.policy_bit() == 0 {
            return false;
        }
        true
    }

    /// The `$client` half, split out because it is reached only by the handful
    /// of records that carry a scope.
    #[inline]
    fn client_admits(&self, idx: u32, ctx: &ClientContext<'_>) -> bool {
        self.clients[&idx].admits(ctx.ip, ctx.name)
    }

    /// The domain walk shared by both entry points. `qbit` is the query type's
    /// bit for a DNS question, or `None` for an HTTP request — which has no
    /// record type, so a `$dnstype`-restricted rule simply does not apply to
    /// it. Answering otherwise would let `$dnstype=A` decide a request that
    /// never asked a DNS question.
    fn lookup_domain(
        &self,
        domain: &str,
        qbit: Option<u32>,
        ctx: &ClientContext<'_>,
    ) -> MatchDecision {
        let query = domain.strip_suffix('.').unwrap_or(domain).as_bytes();
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
                    let type_applies = !rec.has_dnstype()
                        || qbit.is_some_and(|bit| self.dnstype[&idx].0 & bit != 0);
                    // `$dnsrewrite` synthesizes a DNS *answer*. An HTTP request
                    // asked no DNS question, so there is nothing to rewrite —
                    // and reading the rule as a plain block would refuse a
                    // fetch it never said to refuse (`$dnsrewrite=1.2.3.4` is a
                    // redirect, not a denial). Same reasoning as `$dnstype`
                    // above, and the same `qbit.is_none()` signal.
                    let dns_only = rec.has_rewrite() && qbit.is_none();
                    let applies = (!is_subdomain_level || rec.include_subdomains())
                        && type_applies
                        && !dns_only
                        && self.visible(idx, ctx)
                        && (!rec.has_client() || self.client_admits(idx, ctx));
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

    /// Answers a verdict for one HTTP request. **Allocation-free**, like its
    /// DNS counterpart, and the second typed entry point over the same
    /// compiled ruleset (Phase-2 CLAUDE.md: one matcher, one typed interface
    /// per request model, no trait objects on the hot path).
    ///
    /// **Both tiers are consulted.** A URL rule can decide it, and so can a
    /// plain domain rule: `||ads.example.com^` blocks the *name*, and a request
    /// addressed to that name is exactly what it blocks. Ignoring the domain
    /// tier here would mean a host blocked for DNS was still fetched over HTTP
    /// whenever the client resolved it some other way. Precedence is unchanged
    /// and spans both tiers — **any** allow beats **any** block, so an
    /// `@@||cdn.example.com^` exception overrides a URL-tier block just as it
    /// overrides a domain-tier one.
    pub fn lookup_http(&self, request: &HttpRequest<'_>) -> MatchDecision {
        self.lookup_http_in(request, &ClientContext::default())
    }

    /// [`Self::lookup_http`] under a policy, for a named client. The HTTP half
    /// of [`Self::lookup_in`], and the entry point p2-06 threads through the
    /// proxy.
    pub fn lookup_http_in(
        &self,
        request: &HttpRequest<'_>,
        ctx: &ClientContext<'_>,
    ) -> MatchDecision {
        let mut blocked = match self.url.lookup(request, ctx) {
            UrlDecision::Allow(index) => return MatchDecision::Allow(RuleRef(index | URL_TIER)),
            UrlDecision::Block(index) => Some(RuleRef(index | URL_TIER)),
            UrlDecision::Pass => None,
        };
        match self.lookup_domain(request.host, None, ctx) {
            MatchDecision::Allow(rule) => return MatchDecision::Allow(rule),
            MatchDecision::Block(rule) => blocked = blocked.or(Some(rule)),
            MatchDecision::Pass => {}
        }
        match blocked {
            Some(rule) => MatchDecision::Block(rule),
            None => MatchDecision::Pass,
        }
    }

    /// Convenience: full [`Verdict`] for a request, with the decisive rule
    /// materialized. Allocates on allow/block — for tests and `rules/test`,
    /// not the hot path.
    pub fn verdict_http(&self, request: &HttpRequest<'_>) -> Verdict {
        match self.lookup_http(request) {
            MatchDecision::Allow(rule) => Verdict::Allow(self.decisive_rule(rule)),
            MatchDecision::Block(rule) => Verdict::Block(self.decisive_rule(rule)),
            MatchDecision::Pass => Verdict::Pass,
        }
    }

    /// Number of compiled URL-tier rules, distinct like the domain tier's.
    /// Reported separately from [`Self::len`] because the API's
    /// `compiled_rules` has always meant "rules that answer a domain
    /// question", and widening it silently would change what every existing
    /// metric and list count means.
    pub fn url_len(&self) -> usize {
        self.url.len()
    }

    /// URL-tier rules dropped as exact duplicates. Separate from
    /// [`Self::duplicates_removed`] for the same reason as [`Self::url_len`]:
    /// RULE_ENGINE.md pins an arithmetic identity on the domain-tier figure.
    pub fn url_duplicates_removed(&self) -> usize {
        self.url.duplicates_removed()
    }

    /// URL rules no token could file, and which are therefore checked on every
    /// request. Exposed so a bench can assert the token index is still doing
    /// its job rather than silently degrading to a linear scan.
    pub fn url_unindexed(&self) -> usize {
        self.url.unindexed_len()
    }

    /// Keys in the URL tier's n-gram index — the rules reachable only by
    /// sliding a window along each URL token. Exposed alongside
    /// [`Self::url_unindexed`] because the two trade against each other: this
    /// is where the rules that used to inflate that count now live.
    pub fn url_ngram_keys(&self) -> usize {
        self.url.ngram_keys()
    }

    /// URL-tier lookups that hit the matcher's work allowance and stopped
    /// early, leaving some rule unenforced for that request. Zero under any
    /// traffic that is not deliberately shaped to feed a backtracking pattern;
    /// exposed so the binary can surface it, because nothing else would ever
    /// reveal a rule that quietly stopped firing.
    pub fn url_budget_exhausted(&self) -> u64 {
        self.url.budget_exhausted()
    }

    /// Resident bytes of the compiled URL tier alone — p2-03's acceptance
    /// criterion asks for this as an absolute number.
    pub fn url_heap_bytes(&self) -> usize {
        self.url.heap_bytes()
    }

    /// `$dnsrewrite` payload for a decided rule, if any — used by fah-dns for
    /// answer synthesis (interpretation is out of scope here). Borrow, no alloc.
    pub fn rewrite(&self, r: RuleRef) -> Option<&str> {
        if r.is_url_rule() {
            return None;
        }
        let index = r.index();
        let rec = &self.records[index as usize];
        if rec.has_rewrite() {
            self.rewrite.get(&index).map(|s| &**s)
        } else {
            None
        }
    }

    /// Materializes the human-readable decisive rule + owning list for the
    /// query log / `rules/test`. Allocates the reconstructed rule text — call
    /// only off the hot path (block/allow, never `Pass`).
    pub fn decisive_rule(&self, r: RuleRef) -> DecisiveRule {
        let index = r.index();
        if r.is_url_rule() {
            let list = self.lists[self.url.list_id(index) as usize].clone();
            return DecisiveRule::new(list, self.url.rule_text(index));
        }
        let rec = &self.records[index as usize];
        let domain = std::str::from_utf8(self.domain_of(index)).unwrap_or("");
        let mut text = String::with_capacity(domain.len() + 6);
        if rec.is_allow() {
            text.push_str("@@");
        }
        text.push_str("||");
        text.push_str(domain);
        text.push('^');
        // AdGuard option syntax: one `$`, further options comma-separated.
        let mut sep = '$';
        if let Some((_, raw)) = self.dnstype.get(&index) {
            text.push(sep);
            text.push_str("dnstype=");
            text.push_str(raw);
            sep = ',';
        }
        if let Some(raw) = self.rewrite.get(&index) {
            text.push(sep);
            text.push_str("dnsrewrite=");
            text.push_str(raw);
            sep = ',';
        }
        if let Some(scope) = self.clients.get(&index) {
            text.push(sep);
            text.push_str("client=");
            text.push_str(&scope.raw);
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
    ///
    /// `len()` rather than `capacity()` throughout is exact, not an oversight:
    /// `build` converts each of these to `Box<[T]>` via `into_boxed_slice()`,
    /// which is the shrink — there is no spare capacity left to miss, and
    /// `Box<[T]>` has no `capacity()` to call. Likewise `Arc<str>` is exactly
    /// sized.
    ///
    /// This is a floor on what the compiled structure *asked for*, and it must
    /// stay one. The gap to RSS is allocator slack, and it stays in
    /// `MemoryBreakdown::residual` where it can be watched as a trend — no
    /// counter names it directly, the allocator's own commit figure being a
    /// lifetime high-water mark rather than a live one. Folding slack in here
    /// would make a component that should be flat between recompiles track
    /// allocator state, and break the leak signal that depends on exactly that
    /// flatness (p2-07).
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
        let clients: usize = self
            .clients
            .values()
            .map(|scope| 4 + arc_overhead + scope.heap_bytes() + 8)
            .sum();
        self.arena.len()
            + self.records.len() * std::mem::size_of::<Record>()
            + self.slots.len() * std::mem::size_of::<u32>()
            + self.policy_mask.len() * std::mem::size_of::<u16>()
            + lists
            + dnstype
            + rewrite
            + clients
            + self.url.heap_bytes()
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
            client: None,
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

    /// `$dnsrewrite` synthesizes a DNS answer; it is not a block. Reading it as
    /// one on the HTTP path refused fetches the rule never said to refuse —
    /// `$dnsrewrite=1.2.3.4` is a redirect.
    #[test]
    fn a_dnsrewrite_rule_does_not_decide_an_http_request() {
        let rule = DomainRule {
            dns_rewrite: Some(Arc::from("1.2.3.4")),
            ..block("rewrite.example.com")
        };
        let m = matcher_with(&[("", rule)]);
        // Still decisive for the DNS question it was written for.
        assert!(matches!(
            m.lookup("rewrite.example.com", &QueryType::A),
            MatchDecision::Block(_)
        ));
        let request = HttpRequest {
            url: "http://rewrite.example.com/x",
            host: "rewrite.example.com",
            method: "GET",
            resource_type: fah_model::ResourceType::Unknown,
            document_host: None,
        };
        assert_eq!(m.lookup_http(&request), MatchDecision::Pass);
    }

    /// A plain block rule *does* still decide a request — the exclusion above
    /// is about `$dnsrewrite` specifically, not about the domain tier.
    #[test]
    fn a_plain_domain_block_still_decides_an_http_request() {
        let m = matcher_with(&[("", block("ads.example.com"))]);
        let request = HttpRequest {
            url: "http://ads.example.com/x",
            host: "ads.example.com",
            method: "GET",
            resource_type: fah_model::ResourceType::Unknown,
            document_host: None,
        };
        assert!(matches!(m.lookup_http(&request), MatchDecision::Block(_)));
    }

    #[test]
    fn empty_matcher_passes_everything() {
        let m = matcher_with(&[]);
        assert_eq!(decision(&m, "anything.example.com"), MatchDecision::Pass);
        assert!(m.is_empty());
    }

    // ─── Deduplication (p1.5-05) ──────────────────────────────────────────

    /// Compiles the given (list name, rule) pairs, each rule attributed to its
    /// own named list — the cross-list overlap the dedup work exists for.
    fn build_across_lists(rules: &[(&str, DomainRule)], capacity: Option<usize>) -> Matcher {
        let mut b = match capacity {
            Some(cap) => MatcherBuilder::with_capacity(cap),
            None => MatcherBuilder::new(),
        };
        let mut ids: Vec<(&str, u16)> = Vec::new();
        for (list, rule) in rules {
            let id = match ids.iter().find(|(name, _)| name == list) {
                Some((_, id)) => *id,
                None => {
                    let id = b.add_list(*list);
                    ids.push((list, id));
                    id
                }
            };
            b.add_rule(id, rule);
        }
        b.build()
    }

    /// One compiled record, flattened for comparison.
    type RecordPrint = (u32, u8, u8, u16);

    /// Everything the compiled output consists of, in order — what a
    /// determinism assertion has to compare.
    struct Fingerprint {
        arena: Vec<u8>,
        records: Vec<RecordPrint>,
        slots: usize,
    }

    impl PartialEq for Fingerprint {
        fn eq(&self, other: &Self) -> bool {
            self.arena == other.arena && self.records == other.records && self.slots == other.slots
        }
    }

    impl std::fmt::Debug for Fingerprint {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "Fingerprint {{ arena: {} bytes, records: {}, slots: {} }}",
                self.arena.len(),
                self.records.len(),
                self.slots
            )
        }
    }

    fn fingerprint(m: &Matcher) -> Fingerprint {
        Fingerprint {
            arena: m.arena.to_vec(),
            records: m
                .records
                .iter()
                .map(|r| (r.dom_off, r.dom_len, r.flags, r.list_id))
                .collect(),
            slots: m.slots.len(),
        }
    }

    #[test]
    fn the_same_rule_in_two_lists_is_compiled_once() {
        let m = build_across_lists(
            &[
                ("adguard", block("ads.example.com")),
                ("hagezi", block("ads.example.com")),
            ],
            None,
        );
        assert_eq!(m.len(), 1, "an identical rule must not occupy two records");
        assert_eq!(m.duplicates_removed(), 1);
        // Verdict unchanged, attributed to the first list that supplied it.
        let MatchDecision::Block(r) = decision(&m, "ads.example.com") else {
            panic!("expected block");
        };
        assert_eq!(&*m.decisive_rule(r).list, "adguard");
    }

    #[test]
    fn an_exact_duplicate_within_one_list_collapses_too() {
        let m = matcher_with(&[
            ("", block("ads.example.com")),
            ("", block("ads.example.com")),
            ("", block("other.example.com")),
        ]);
        assert_eq!(m.len(), 2);
        assert_eq!(m.duplicates_removed(), 1);
    }

    #[test]
    fn block_in_one_list_and_allow_in_another_both_survive() {
        // Different actions are different identities: collapsing them would
        // silently change the verdict, which allow > block then decides.
        let m = build_across_lists(
            &[
                ("blocklist", block("example.com")),
                ("allowlist", allow("example.com")),
            ],
            None,
        );
        assert_eq!(m.len(), 2);
        assert_eq!(m.duplicates_removed(), 0);
        assert!(matches!(
            decision(&m, "example.com"),
            MatchDecision::Allow(_)
        ));
    }

    #[test]
    fn rules_differing_only_in_an_option_are_distinct_identities() {
        let plain = block("ads.example.com");
        let typed = DomainRule {
            dns_types: Some(Arc::from("A")),
            ..block("ads.example.com")
        };
        let rewritten = DomainRule {
            dns_rewrite: Some(Arc::from("0.0.0.0")),
            ..block("ads.example.com")
        };
        let exact = DomainRule {
            include_subdomains: false,
            ..block("ads.example.com")
        };
        let m = matcher_with(&[
            ("", plain),
            ("", typed),
            ("", rewritten),
            ("", exact),
            // …and one true duplicate of the $dnstype rule, which must go.
            (
                "",
                DomainRule {
                    dns_types: Some(Arc::from("A")),
                    ..block("ads.example.com")
                },
            ),
        ]);
        assert_eq!(m.len(), 4, "only the identical pair may collapse");
        assert_eq!(m.duplicates_removed(), 1);
    }

    #[test]
    fn a_domain_that_differs_only_in_case_is_the_same_rule() {
        // Lookup is case-insensitive, so these two would always decide
        // identically — keeping both would be pure waste.
        let m = matcher_with(&[
            ("", block("Ads.Example.COM")),
            ("", block("ads.example.com")),
        ]);
        assert_eq!(m.len(), 1);
        assert_eq!(m.duplicates_removed(), 1);
        assert!(matches!(
            decision(&m, "ads.example.com"),
            MatchDecision::Block(_)
        ));
    }

    #[test]
    fn compiling_the_same_rules_twice_is_byte_identical_whatever_the_capacity() {
        // Determinism: the dedup index is membership-only and never iterated
        // to produce output, so its capacity is a performance knob and must
        // leave the compiled arena/records/slots untouched.
        let rules: Vec<(&str, DomainRule)> = vec![
            ("a", block("ads.example.com")),
            ("a", allow("cdn.example.com")),
            ("b", block("ads.example.com")),
            ("b", block("tracker.example.org")),
            (
                "b",
                DomainRule {
                    dns_types: Some(Arc::from("AAAA")),
                    ..block("ads.example.com")
                },
            ),
            ("c", block("tracker.example.org")),
        ];

        let baseline = fingerprint(&build_across_lists(&rules, None));
        for capacity in [Some(0), Some(1), Some(rules.len()), Some(100_000), None] {
            let other = build_across_lists(&rules, capacity);
            assert_eq!(
                fingerprint(&other),
                baseline,
                "capacity {capacity:?} changed the compiled output"
            );
            assert_eq!(other.duplicates_removed(), 2);
        }
    }

    #[test]
    fn an_unhinted_builder_grows_its_index_without_losing_or_inventing_rules() {
        // `MatcherBuilder::new()` starts with a small index and rehashes as it
        // fills; rehashing moves indices around but must not change which
        // rules survive.
        let mut b = MatcherBuilder::new();
        let list = b.add_list("big");
        for i in 0..5_000 {
            b.add_rule(list, &block(&format!("host{i}.example.com")));
            b.add_rule(list, &block(&format!("host{i}.example.com")));
        }
        let m = b.build();
        assert_eq!(m.len(), 5_000);
        assert_eq!(m.duplicates_removed(), 5_000);
        assert!(matches!(
            decision(&m, "host4999.example.com"),
            MatchDecision::Block(_)
        ));
        assert!(matches!(
            decision(&m, "deep.sub.host0.example.com"),
            MatchDecision::Block(_)
        ));
    }

    #[test]
    fn dedup_shrinks_the_compiled_footprint_of_overlapping_lists() {
        let rules: Vec<(&str, DomainRule)> = (0..1_000)
            .map(|i| ("a", block(&format!("ads{i}.example.com"))))
            .chain((0..1_000).map(|i| ("b", block(&format!("ads{i}.example.com")))))
            .collect();
        let m = build_across_lists(&rules, Some(rules.len()));
        assert_eq!(m.len(), 1_000);
        assert_eq!(m.duplicates_removed(), 1_000);
        let single = build_across_lists(&rules[..1_000], Some(1_000));
        assert_eq!(
            (m.arena.len(), m.records.len(), m.slots.len()),
            (single.arena.len(), single.records.len(), single.slots.len()),
            "a fully overlapping second list must add no arena, records or slots"
        );
        // The only thing the second list does add is its own name in `lists`.
        assert!(m.heap_bytes() - single.heap_bytes() < 64);
    }

    #[test]
    fn with_capacity_clamps_the_preallocation_against_an_inflated_ceiling() {
        // `rule_upper_bound` is a pre-parse token count a hostile list can
        // inflate to tens of millions of "rules" that parse to nothing. The
        // transient index must be sized from the clamp, never from that
        // unbounded ceiling, so the up-front allocation can't OOM the box.
        let clamped = MatcherBuilder::with_capacity(usize::MAX);
        assert_eq!(
            clamped.dedup.len(),
            dedup_slots(MAX_PREALLOC_RULES),
            "an inflated ceiling must clamp to MAX_PREALLOC_RULES"
        );
        // A ceiling under the clamp is still honored exactly.
        let exact = MatcherBuilder::with_capacity(1_000);
        assert_eq!(exact.dedup.len(), dedup_slots(1_000));
    }

    #[test]
    fn an_adversarial_list_body_cannot_inflate_the_dedup_allocation() {
        // The real attack path end-to-end: a list body of single-char,
        // space-separated tokens is a valid `rule_upper_bound` input that parses
        // to ~nothing, yet its token ceiling is enormous. `with_capacity` must
        // size the transient index from the clamp, never from that ceiling —
        // this is what stops a 64 MiB hostile list from forcing a ~268 MB alloc.
        let garbage = "a ".repeat(5_000_000); // ~5M tokens, above the 4M clamp
        let ceiling = crate::parser::rule_upper_bound(&garbage);
        assert!(
            ceiling > MAX_PREALLOC_RULES,
            "test input must exceed the clamp to be meaningful (got {ceiling})"
        );
        let builder = MatcherBuilder::with_capacity(ceiling);
        assert_eq!(
            builder.dedup.len(),
            dedup_slots(MAX_PREALLOC_RULES),
            "a {ceiling}-token ceiling must clamp, not size the index for the whole ceiling"
        );
    }

    #[test]
    fn a_clamped_builder_still_compiles_every_distinct_rule() {
        // Correctness is independent of the clamp: even were the real rule
        // count to exceed MAX_PREALLOC_RULES, reserve_dedup grows the index.
        // Proven here in miniature by pinning a tiny "clamp" via a small hint
        // and overflowing it — the grown path must lose or invent nothing.
        let mut b = MatcherBuilder::with_capacity(4); // deliberately far too small
        let list = b.add_list("l");
        for i in 0..2_000 {
            b.add_rule(list, &block(&format!("host{i}.example.com")));
            b.add_rule(list, &block(&format!("host{i}.example.com")));
        }
        let m = b.build();
        assert_eq!(m.len(), 2_000);
        assert_eq!(m.duplicates_removed(), 2_000);
        assert!(matches!(
            decision(&m, "host1999.example.com"),
            MatchDecision::Block(_)
        ));
    }
}
