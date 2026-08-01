//! The URL tier: compiled request-level rules (RULE_ENGINE.md: HTTP matching).
//!
//! # Layout (why this shape)
//!
//! The same reasoning as the domain tier, one level up. A URL rule's pattern
//! *is* the rule — unlike a domain rule there is nothing to reconstruct it
//! from — so the text has to be stored, and storing 22k `Arc<str>` would spend
//! more on control blocks than on patterns. Instead:
//!
//! - `patterns` — every pattern concatenated into one contiguous allocation.
//! - `domains` — a second arena for the `$domain=` payloads, which roughly half
//!   of EasyList's URL rules carry and the rest do not pay for.
//! - `records` — a fixed **16-byte** [`Record`] per rule: two arena offsets,
//!   two lengths, the resource-type mask and the flags. Contiguous, so a
//!   candidate check is one cache line rather than a pointer chase.
//! - `list_ids` — attribution, in a parallel array rather than in [`Record`].
//!   It is read only when a rule has already decided a request, so keeping it
//!   out of the record is what holds the hot struct at 16 bytes.
//!
//! `$method=` lives in a side map: it is rare enough that a field per record
//! would cost more than the whole option does.
//!
//! # The token index, and why a linear scan is not an option
//!
//! Checking every URL rule against every request is ~22k pattern matches per
//! request. So each rule is filed under **one** token — a literal run of
//! `[a-z0-9_%]` its pattern requires — and a lookup only checks rules filed
//! under a token the URL actually contains. This is the adblock-rust approach.
//!
//! Three details make it correct rather than merely fast:
//!
//! - **A token's *boundedness* decides how it can be looked up.** A pattern
//!   token that could match part of a URL token is not findable by an exact
//!   probe: `*track` matches `mytrack`, whose only token is `mytrack`. So the
//!   pattern's delimiters — a separator, or an anchor at the pattern's edge,
//!   never a `*` — decide the key kind: bounded both sides is an exact key,
//!   left-only a prefix key, right-only a suffix key. See [`KeyKind`].
//! - **The rarest key wins.** Filing `||ads.example.com/track` under `com`
//!   would put it in a bucket half the corpus shares. Frequencies are counted
//!   across the whole compile, then each rule takes its least common key.
//! - **A lookup probes each URL token three ways** — as itself, by its first
//!   [`MIN_TOKEN_LEN`] bytes, and by its last — matching the three key kinds.
//!
//! Rules whose every token is unbounded on both sides go to
//! [`UrlIndex::unindexed`] and are checked on every lookup. On EasyList +
//! EasyPrivacy that is 77 of 18,778; a test pins the share, because a large
//! one would quietly turn every lookup into a linear scan.
//!
//! # Matching
//!
//! [`match_from`] is an iterative two-pointer wildcard matcher over `*` and
//! `^` — **no regex**, no backtracking stack, no allocation (PERFORMANCE.md).
//! A `/regex/` literal is refused at parse time rather than matched here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use fah_model::HttpRequest;

use crate::matcher::{fastrange, EMPTY};
use crate::resource::bit_for_request;
use crate::rule::{Party, RuleAction, UrlAnchor, UrlRule};

// Record flag bits.
const FLAG_ALLOW: u16 = 1 << 0;
const FLAG_END_ANCHORED: u16 = 1 << 1;
const FLAG_MATCH_CASE: u16 = 1 << 2;
const FLAG_METHODS: u16 = 1 << 3;
const ANCHOR_SHIFT: u16 = 4;
const ANCHOR_MASK: u16 = 0b11 << ANCHOR_SHIFT;
const PARTY_SHIFT: u16 = 6;
const PARTY_MASK: u16 = 0b11 << PARTY_SHIFT;

const ANCHOR_DOMAIN: u16 = 0;
const ANCHOR_START: u16 = 1;
const ANCHOR_ANYWHERE: u16 = 2;

const PARTY_ANY: u16 = 0;
const PARTY_THIRD: u16 = 1;
const PARTY_FIRST: u16 = 2;

/// Shortest token worth filing a rule under. Below this, tokens are so common
/// that the bucket approaches the whole corpus and the index stops paying for
/// itself.
const MIN_TOKEN_LEN: usize = 3;

/// Backstop allowance for a whole lookup, in [`Budget`] units. Purely a safety
/// net behind [`rule_step_cap`], which is what actually bounds the shape of the
/// cost; on the real corpus a lookup spends four orders of magnitude less.
const LOOKUP_STEP_CAP: usize = 8_000_000;

/// Work allowance for **one rule** against one URL, in [`Budget`] units.
///
/// [`match_from`] backtracks over `*`, and an unanchored pattern is retried at
/// every start offset whose first byte could match, so the product is
/// O(url² × pattern). That is not theoretical: a rule shaped `aaa…a*aaa…ab`
/// against a URL of `a`s measured **313 ms for a single request** — five orders
/// past the budget, driven by a URL the client chooses. The URL is
/// attacker-controlled even when the rule is not.
///
/// A unit is *one attempted match position*: an entry into [`match_from`], or
/// one widening of a `*`. Both are bounded by the URL length for any honest
/// pattern — a leading-`*` rule widens at most once per URL byte, an
/// unanchored one is entered at most once per URL byte — so `2 × url_len`
/// leaves a rule its full legitimate cost and nothing beyond it. What it
/// removes is the *product* of the two, which is the square.
///
/// Charging per position rather than per byte compared is deliberate: the byte
/// loop is the hot one, and metering it there measured **+28 % on the real
/// EasyList corpus**. Here the branch runs once per position, and the same
/// bound holds.
fn rule_step_cap(url_len: usize) -> usize {
    url_len.saturating_mul(2).saturating_add(64)
}

/// The work allowance for one lookup, and whether it ran out.
///
/// `floor` is re-armed per rule by [`Budget::open`], so one pathological rule
/// spends its own allowance and not the lookup's. Exhaustion is remembered
/// rather than inferred from `remaining == 0`, since a lookup may legitimately
/// spend its last unit.
struct Budget {
    remaining: usize,
    floor: usize,
    exhausted: bool,
}

impl Budget {
    fn new() -> Self {
        Self {
            remaining: LOOKUP_STEP_CAP,
            floor: 0,
            exhausted: false,
        }
    }

    /// Re-arms the per-rule allowance before a rule is matched.
    #[inline]
    fn open(&mut self, url_len: usize) {
        self.floor = self.remaining.saturating_sub(rule_step_cap(url_len));
    }

    /// Charges one attempted match position. `false` means the allowance is
    /// gone and the caller must stop — reported as "no match", which
    /// under-blocks.
    #[inline]
    fn spend(&mut self) -> bool {
        if self.remaining <= self.floor {
            self.exhausted = true;
            return false;
        }
        self.remaining -= 1;
        true
    }
}

/// One compiled URL rule. Exactly 16 bytes — the figure the p2-03 headroom
/// model was built on.
#[derive(Clone, Copy)]
#[repr(C)]
struct Record {
    pat_off: u32,
    dom_off: u32,
    pat_len: u16,
    dom_len: u16,
    /// Resource types the rule applies to, negation already folded in. Zero
    /// means "any type" (see [`crate::rule::UrlRule::resource_types`]).
    types: u16,
    flags: u16,
}

impl Record {
    fn is_allow(&self) -> bool {
        self.flags & FLAG_ALLOW != 0
    }
    fn end_anchored(&self) -> bool {
        self.flags & FLAG_END_ANCHORED != 0
    }
    fn match_case(&self) -> bool {
        self.flags & FLAG_MATCH_CASE != 0
    }
    fn has_methods(&self) -> bool {
        self.flags & FLAG_METHODS != 0
    }
    fn anchor(&self) -> u16 {
        (self.flags & ANCHOR_MASK) >> ANCHOR_SHIFT
    }
    fn party(&self) -> u16 {
        (self.flags & PARTY_MASK) >> PARTY_SHIFT
    }
}

fn flags_of(rule: &UrlRule) -> u16 {
    let mut flags = 0u16;
    if rule.action == RuleAction::Allow {
        flags |= FLAG_ALLOW;
    }
    if rule.end_anchored {
        flags |= FLAG_END_ANCHORED;
    }
    if rule.match_case {
        flags |= FLAG_MATCH_CASE;
    }
    if rule.methods.is_some() {
        flags |= FLAG_METHODS;
    }
    let anchor = match rule.anchor {
        UrlAnchor::Domain => ANCHOR_DOMAIN,
        UrlAnchor::Start => ANCHOR_START,
        UrlAnchor::Anywhere => ANCHOR_ANYWHERE,
    };
    let party = match rule.party {
        Party::Any => PARTY_ANY,
        Party::Third => PARTY_THIRD,
        Party::First => PARTY_FIRST,
    };
    flags | (anchor << ANCHOR_SHIFT) | (party << PARTY_SHIFT)
}

// ─── Tokenization ─────────────────────────────────────────────────────────

/// A URL/pattern token character. Everything else is a delimiter, which is
/// also what `^` matches — so token boundaries and separator boundaries are
/// the same notion, deliberately.
#[inline]
fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'%'
}

/// `^` matches one separator character, or the end of the URL. The set is the
/// adblock one: anything that is not a letter, digit, `_`, `-`, `.` or `%`.
#[inline]
fn is_separator(b: u8) -> bool {
    !(b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b'%')
}

/// How a pattern token relates to the URL token that must contain it.
///
/// A token delimited on both sides *is* a URL token, and can be looked up
/// exactly. One delimited on the left only (`-adserver` with nothing after it)
/// can be any *prefix* of a URL token, and one delimited on the right only
/// (`*trackpixel/`) any *suffix*. Those two used to be unusable, which left
/// 254 of EasyList + EasyPrivacy's 18,778 rules to be scanned on every single
/// request — measured at 87 % of the lookup's cost. Indexing them under their
/// first / last [`MIN_TOKEN_LEN`] bytes reaches them in one probe instead.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    Exact,
    Prefix,
    Suffix,
}

impl KeyKind {
    /// Distinct FNV seeds, so `Exact("ads")` and `Prefix("ads")` are different
    /// keys and a rule filed as one is never reached by a probe for the other.
    fn seed(self) -> u64 {
        match self {
            KeyKind::Exact => 0xcbf2_9ce4_8422_2325,
            KeyKind::Prefix => 0x9e37_79b9_7f4a_7c15,
            KeyKind::Suffix => 0x517c_c1b7_2722_0a95,
        }
    }
}

/// FNV-1a over ASCII-lowercased bytes, seeded by the key kind. Case-folded on
/// both sides so a `$match-case` rule is still *found* by the index — its case
/// sensitivity is enforced by the full comparison afterwards, not by which
/// bucket it is in.
fn hash_key(kind: KeyKind, bytes: &[u8]) -> u64 {
    let mut h = kind.seed();
    for &b in bytes {
        h ^= b.to_ascii_lowercase() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Calls `visit` with every token span in `bytes`, stopping early the moment
/// `visit` returns `false` — a lookup that has already found an allow, or has
/// run out of budget, has no reason to keep tokenizing the rest of the URL.
fn for_each_token(bytes: &[u8], mut visit: impl FnMut(usize, usize) -> bool) {
    let mut at = 0;
    while at < bytes.len() {
        if !is_token_byte(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_token_byte(bytes[at]) {
            at += 1;
        }
        if !visit(start, at) {
            return;
        }
    }
}

/// Calls `visit` with every index key `pattern` offers, and the length of the
/// token each came from — longer tokens are more selective, which is how ties
/// in frequency are broken.
///
/// A token unbounded on *both* sides yields no key: the URL token merely
/// contains it, and neither a prefix nor a suffix probe would find it.
fn for_each_index_key(
    pattern: &[u8],
    anchor: u16,
    end_anchored: bool,
    mut visit: impl FnMut(KeyKind, &[u8], usize),
) {
    for_each_token(pattern, |start, end| {
        let token = &pattern[start..end];
        if token.len() < MIN_TOKEN_LEN {
            return true;
        }
        // `*` is not a delimiter: it can consume URL bytes that would fuse
        // into this token. An anchor at the pattern edge is one, because the
        // URL boundary it pins is a token boundary.
        let left_bounded = if start == 0 {
            anchor != ANCHOR_ANYWHERE
        } else {
            pattern[start - 1] != b'*'
        };
        let right_bounded = if end == pattern.len() {
            end_anchored
        } else {
            pattern[end] != b'*'
        };
        match (left_bounded, right_bounded) {
            (true, true) => visit(KeyKind::Exact, token, token.len()),
            (true, false) => visit(KeyKind::Prefix, &token[..MIN_TOKEN_LEN], token.len()),
            (false, true) => visit(
                KeyKind::Suffix,
                &token[token.len() - MIN_TOKEN_LEN..],
                token.len(),
            ),
            (false, false) => {}
        }
        true
    });
}

// ─── Pattern matching ─────────────────────────────────────────────────────

#[inline]
fn byte_matches(pattern_byte: u8, text_byte: u8, match_case: bool) -> bool {
    if match_case {
        pattern_byte == text_byte
    } else {
        pattern_byte == text_byte.to_ascii_lowercase()
    }
}

/// Matches `pattern` against `text` beginning at `start`.
///
/// Iterative two-pointer with a single backtrack point, so a pattern with many
/// `*` costs time but never stack. `^` matches one separator **or** the end of
/// the text, which is what makes `||example.com^` match `http://example.com`.
///
/// Entering the matcher costs one [`Budget`] unit, and so does each widening of
/// a `*` — the two places the work can multiply. The byte loop between them is
/// bounded by the pattern length and is left unmetered, because metering it
/// measured +28 % on the real corpus for a bound this already gives.
fn match_from(
    pattern: &[u8],
    text: &[u8],
    start: usize,
    end_anchored: bool,
    match_case: bool,
    budget: &mut Budget,
) -> bool {
    if !budget.spend() {
        return false;
    }
    let mut p = 0usize;
    let mut t = start;
    // (pattern index just after the `*`, text index that `*` was last tried at)
    let mut star: Option<(usize, usize)> = None;

    loop {
        if p == pattern.len() {
            if !end_anchored || t == text.len() {
                return true;
            }
        } else {
            let pb = pattern[p];
            if pb == b'*' {
                star = Some((p + 1, t));
                p += 1;
                continue;
            }
            if pb == b'^' {
                if t == text.len() {
                    // End of URL is a separator; it consumes nothing.
                    p += 1;
                    continue;
                }
                if is_separator(text[t]) {
                    p += 1;
                    t += 1;
                    continue;
                }
            } else if t < text.len() && byte_matches(pb, text[t], match_case) {
                p += 1;
                t += 1;
                continue;
            }
        }
        // Mismatch, or a complete-but-unanchored match: widen the last `*`.
        // This is the loop that can run once per text byte *per entry*, so it
        // is the second place the allowance is charged.
        match star {
            Some((after, tried)) if tried < text.len() && budget.spend() => {
                star = Some((after, tried + 1));
                p = after;
                t = tried + 1;
            }
            _ => return false,
        }
    }
}

/// Where the host sits inside an absolute URL. Falls back to the whole string
/// when there is no scheme, so a bare `host/path` still anchors correctly.
fn host_span(url: &[u8]) -> (usize, usize) {
    let start = url
        .windows(3)
        .position(|window| window == b"://")
        .map_or(0, |at| at + 3);
    let end = url[start..]
        .iter()
        .position(|&b| matches!(b, b'/' | b'?' | b'#'))
        .map_or(url.len(), |offset| start + offset);
    (start, end)
}

// ─── Request-level predicates ─────────────────────────────────────────────

/// The registrable-domain approximation: the last two labels.
///
/// **This is not a Public Suffix List.** `a.co.uk` and `b.co.uk` both reduce to
/// `co.uk` and are therefore judged first-party to each other, which a PSL
/// would get right. Carrying a PSL costs a dependency, ~200 KB of tables and a
/// refresh story, against a `$third-party` option that only narrows rules —
/// so the error direction is under-blocking, never over-blocking. Revisit with
/// measurements if `$third-party` accuracy is ever shown to matter.
fn registrable(host: &str) -> &str {
    let bytes = host.as_bytes();
    let mut seen = 0;
    for index in (0..bytes.len()).rev() {
        if bytes[index] == b'.' {
            seen += 1;
            if seen == 2 {
                return &host[index + 1..];
            }
        }
    }
    host
}

/// A request is third-party when it leaves the document's registrable domain.
/// With no referer there is no document to be third to, so it is first-party.
fn is_third_party(request: &HttpRequest<'_>) -> bool {
    match request.document_host {
        Some(document) => !registrable(document).eq_ignore_ascii_case(registrable(request.host)),
        None => false,
    }
}

/// Does `host` fall under a `$domain=` entry — the entry itself, or anything
/// below it?
///
/// Bytes rather than `&str` on both sides: every comparison here is
/// ASCII-case-insensitive anyway, and taking the arena slice as `&str` would
/// put a UTF-8 validation of the whole `$domain=` payload on the hot path — and
/// would have to decide what an invalid one means, where the only cheap answer
/// ("") makes the rule apply *everywhere*.
fn host_under(host: &[u8], entry: &[u8]) -> bool {
    // `example.*` — uBO's "this name under any public suffix" form. Without a
    // PSL the tail is bounded to at most two labels, the same approximation
    // `registrable` already makes; without this arm the entry matches nothing
    // at all and the rule silently never fires.
    if let Some(stem) = entry.strip_suffix(b".*") {
        return host_under_any_suffix(host, stem);
    }
    if host.eq_ignore_ascii_case(entry) {
        return true;
    }
    host.len() > entry.len()
        && host[host.len() - entry.len() - 1] == b'.'
        && host[host.len() - entry.len()..].eq_ignore_ascii_case(entry)
}

/// `stem` appearing at a label boundary of `host` with one or two labels after
/// it — `example.*` against `example.com`, `www.example.co.uk`, but not
/// `example.com.evil.net`.
fn host_under_any_suffix(host: &[u8], stem: &[u8]) -> bool {
    if stem.is_empty() {
        return false;
    }
    let mut at = 0;
    loop {
        let candidate = &host[at..];
        if candidate.len() > stem.len()
            && candidate[stem.len()] == b'.'
            && candidate[..stem.len()].eq_ignore_ascii_case(stem)
        {
            let tail = &candidate[stem.len() + 1..];
            if !tail.is_empty() && tail.iter().filter(|&&byte| byte == b'.').count() < 2 {
                return true;
            }
        }
        match candidate.iter().position(|&byte| byte == b'.') {
            Some(offset) => at += offset + 1,
            None => return false,
        }
    }
}

/// `$domain=a.com|~b.com` against the document's host. Positive entries are a
/// whitelist, negated ones a veto; with no positives, anything not vetoed
/// passes.
fn domains_apply(raw: &[u8], document_host: Option<&str>) -> bool {
    let mut has_positive = false;
    let mut positive_hit = false;
    for entry in raw.split(|&byte| byte == b'|') {
        let (entry, negated) = match entry.strip_prefix(b"~") {
            Some(rest) => (rest, true),
            None => (entry, false),
        };
        if entry.is_empty() {
            continue;
        }
        let hit = document_host.is_some_and(|host| host_under(host.as_bytes(), entry));
        if negated {
            if hit {
                return false;
            }
        } else {
            has_positive = true;
            positive_hit |= hit;
        }
    }
    !has_positive || positive_hit
}

/// `$method=get|post`, or `$method=~post`. Same shape as `$domain=`.
fn methods_apply(raw: &str, method: &str) -> bool {
    let mut has_positive = false;
    let mut positive_hit = false;
    for entry in raw.split('|') {
        let (entry, negated) = match entry.strip_prefix('~') {
            Some(rest) => (rest, true),
            None => (entry, false),
        };
        if entry.is_empty() {
            continue;
        }
        let hit = method.eq_ignore_ascii_case(entry);
        if negated {
            if hit {
                return false;
            }
        } else {
            has_positive = true;
            positive_hit |= hit;
        }
    }
    !has_positive || positive_hit
}

// ─── Build ────────────────────────────────────────────────────────────────

/// Accumulates URL rules; [`UrlIndexBuilder::build`] computes the token index
/// once, when every rule's token frequency is known.
#[derive(Default)]
pub(crate) struct UrlIndexBuilder {
    patterns: Vec<u8>,
    domains: Vec<u8>,
    records: Vec<Record>,
    list_ids: Vec<u16>,
    methods: HashMap<u32, Arc<str>>,
    /// Identity hash -> record indices. Build-time only, dropped at
    /// [`Self::build`]; every candidate is confirmed by real comparison, so a
    /// collision costs a comparison and never a dropped rule.
    dedup: HashMap<u64, Vec<u32>>,
    duplicates_removed: usize,
}

impl UrlIndexBuilder {
    pub(crate) fn add(&mut self, list_id: u16, rule: &UrlRule) {
        let pattern = rule.pattern.as_bytes();
        let Ok(pat_len) = u16::try_from(pattern.len()) else {
            return;
        };
        if pat_len == 0 {
            return;
        }
        let domains = rule.domains.as_deref().unwrap_or("").as_bytes();
        let Ok(dom_len) = u16::try_from(domains.len()) else {
            return;
        };
        let flags = flags_of(rule);

        let identity = self.identity_hash(pattern, domains, flags, rule);
        if let Some(existing) = self.dedup.get(&identity) {
            if existing
                .iter()
                .any(|&idx| self.identity_matches(idx, pattern, domains, flags, rule))
            {
                self.duplicates_removed += 1;
                return;
            }
        }

        let index = u32::try_from(self.records.len()).expect("at most u32::MAX URL rules");
        let pat_off = u32::try_from(self.patterns.len()).expect("pattern arena within 4 GiB");
        let dom_off = u32::try_from(self.domains.len()).expect("domain arena within 4 GiB");
        self.patterns.extend_from_slice(pattern);
        self.domains.extend_from_slice(domains);
        if let Some(raw) = &rule.methods {
            self.methods.insert(index, raw.clone());
        }
        self.records.push(Record {
            pat_off,
            dom_off,
            pat_len,
            dom_len,
            types: rule.resource_types,
            flags,
        });
        self.list_ids.push(list_id);
        self.dedup.entry(identity).or_default().push(index);
    }

    fn identity_hash(&self, pattern: &[u8], domains: &[u8], flags: u16, rule: &UrlRule) -> u64 {
        let mut h = hash_key(KeyKind::Exact, pattern);
        for bytes in [
            domains,
            &flags.to_le_bytes()[..],
            &rule.resource_types.to_le_bytes()[..],
        ] {
            for &b in bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    fn identity_matches(
        &self,
        index: u32,
        pattern: &[u8],
        domains: &[u8],
        flags: u16,
        rule: &UrlRule,
    ) -> bool {
        let record = &self.records[index as usize];
        record.flags == flags
            && record.types == rule.resource_types
            && self.pattern_of(record) == pattern
            && self.domains_of(record) == domains
            && self.methods.get(&index).map(|raw| &**raw) == rule.methods.as_deref()
    }

    fn pattern_of(&self, record: &Record) -> &[u8] {
        let start = record.pat_off as usize;
        &self.patterns[start..start + record.pat_len as usize]
    }

    fn domains_of(&self, record: &Record) -> &[u8] {
        let start = record.dom_off as usize;
        &self.domains[start..start + record.dom_len as usize]
    }

    pub(crate) fn build(self) -> UrlIndex {
        // Pass 1 — how common is each key across the whole compile?
        let mut frequency: HashMap<u64, u32> = HashMap::new();
        for record in &self.records {
            for_each_index_key(
                self.pattern_of(record),
                record.anchor(),
                record.end_anchored(),
                |kind, key, _| {
                    *frequency.entry(hash_key(kind, key)).or_insert(0) += 1;
                },
            );
        }

        // Pass 2 — file each rule under its rarest key; the longest source
        // token breaks ties, being the more selective of two equally rare ones.
        let mut filed: Vec<(u64, u32)> = Vec::with_capacity(self.records.len());
        let mut unindexed: Vec<u32> = Vec::new();
        for (index, record) in self.records.iter().enumerate() {
            let mut best: Option<(u32, usize, u64)> = None;
            for_each_index_key(
                self.pattern_of(record),
                record.anchor(),
                record.end_anchored(),
                |kind, key, token_len| {
                    let hash = hash_key(kind, key);
                    let count = frequency.get(&hash).copied().unwrap_or(u32::MAX);
                    let better = match best {
                        None => true,
                        Some((best_count, best_len, _)) => {
                            count < best_count || (count == best_count && token_len > best_len)
                        }
                    };
                    if better {
                        best = Some((count, token_len, hash));
                    }
                },
            );
            match best {
                Some((_, _, hash)) => filed.push((hash, index as u32)),
                None => unindexed.push(index as u32),
            }
        }

        // CSR: sort by key, then one contiguous run of rule ids per key.
        filed.sort_unstable();
        let mut bucket_hash: Vec<u64> = Vec::new();
        let mut bucket_start: Vec<u32> = Vec::new();
        let mut bucket_rules: Vec<u32> = Vec::with_capacity(filed.len());
        for (hash, rule) in filed {
            if bucket_hash.last() != Some(&hash) {
                bucket_hash.push(hash);
                bucket_start.push(bucket_rules.len() as u32);
            }
            bucket_rules.push(rule);
        }
        bucket_start.push(bucket_rules.len() as u32);

        // Open-addressing table over the buckets, ~0.7 load factor, sized
        // exactly via fastrange rather than rounded to a power of two.
        let slot_cap = (bucket_hash.len().saturating_mul(10) / 7).max(8);
        let mut slots = vec![EMPTY; slot_cap];
        for (bucket, &hash) in bucket_hash.iter().enumerate() {
            let mut slot = fastrange(hash, slot_cap);
            while slots[slot] != EMPTY {
                slot += 1;
                if slot == slot_cap {
                    slot = 0;
                }
            }
            slots[slot] = bucket as u32;
        }

        UrlIndex {
            patterns: self.patterns.into_boxed_slice(),
            domains: self.domains.into_boxed_slice(),
            records: self.records.into_boxed_slice(),
            list_ids: self.list_ids.into_boxed_slice(),
            methods: self.methods,
            slots: slots.into_boxed_slice(),
            slot_cap,
            bucket_hash: bucket_hash.into_boxed_slice(),
            bucket_start: bucket_start.into_boxed_slice(),
            bucket_rules: bucket_rules.into_boxed_slice(),
            unindexed: unindexed.into_boxed_slice(),
            duplicates_removed: self.duplicates_removed,
            budget_exhausted: AtomicU64::new(0),
        }
    }
}

/// What a URL-tier lookup found: the winning record index, or nothing.
pub(crate) enum UrlDecision {
    Allow(u32),
    Block(u32),
    Pass,
}

/// Everything one lookup derives from the request once and then reuses for
/// every candidate rule, plus the running work allowance.
///
/// A struct rather than eight positional parameters: `check` is called from
/// three places and every one of them was passing the same list through
/// unchanged, which is how a caller ends up handing `host_end` where
/// `host_start` was wanted.
struct Probe<'a> {
    request: &'a HttpRequest<'a>,
    url: &'a [u8],
    host_start: usize,
    host_end: usize,
    type_bit: u16,
    third_party: bool,
    budget: Budget,
}

impl<'a> Probe<'a> {
    fn new(request: &'a HttpRequest<'a>) -> Self {
        let url = request.url.as_bytes();
        let (host_start, host_end) = host_span(url);
        Self {
            request,
            url,
            host_start,
            host_end,
            type_bit: bit_for_request(request.resource_type),
            third_party: is_third_party(request),
            budget: Budget::new(),
        }
    }
}

/// The compiled, immutable URL tier. Read-only on the hot path, swapped
/// atomically with the rest of the ruleset.
pub(crate) struct UrlIndex {
    patterns: Box<[u8]>,
    domains: Box<[u8]>,
    records: Box<[Record]>,
    list_ids: Box<[u16]>,
    methods: HashMap<u32, Arc<str>>,
    slots: Box<[u32]>,
    slot_cap: usize,
    bucket_hash: Box<[u64]>,
    bucket_start: Box<[u32]>,
    bucket_rules: Box<[u32]>,
    /// Rules whose pattern offers no token a URL tokenizer would reproduce.
    unindexed: Box<[u32]>,
    duplicates_removed: usize,
    /// Lookups that hit [`LOOKUP_STEP_CAP`] or a rule's [`rule_step_cap`] and
    /// therefore stopped matching early. Interior mutability on an otherwise
    /// immutable structure, and only ever touched on the exhausted path.
    budget_exhausted: AtomicU64,
}

impl UrlIndex {
    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn duplicates_removed(&self) -> usize {
        self.duplicates_removed
    }

    /// How many rules no token could file. Small by construction; a test pins
    /// the share against a real corpus, because a large one would quietly turn
    /// every lookup into a linear scan.
    pub(crate) fn unindexed_len(&self) -> usize {
        self.unindexed.len()
    }

    /// Lookups cut short by the work allowance. Non-zero means some rule stopped
    /// being enforced for some request — a signal, not a statistic.
    pub(crate) fn budget_exhausted(&self) -> u64 {
        self.budget_exhausted.load(Ordering::Relaxed)
    }

    pub(crate) fn list_id(&self, index: u32) -> u16 {
        self.list_ids[index as usize]
    }

    fn pattern_of(&self, index: u32) -> &[u8] {
        let record = &self.records[index as usize];
        let start = record.pat_off as usize;
        &self.patterns[start..start + record.pat_len as usize]
    }

    fn domains_of(&self, index: u32) -> &[u8] {
        let record = &self.records[index as usize];
        let start = record.dom_off as usize;
        &self.domains[start..start + record.dom_len as usize]
    }

    /// Answers a verdict for one request. **Allocation-free** — the hot path.
    ///
    /// Precedence matches the domain tier: an allow wins outright and returns
    /// immediately; a block is remembered in case an allow turns up later.
    pub(crate) fn lookup(&self, request: &HttpRequest<'_>) -> UrlDecision {
        let mut probe = Probe::new(request);
        let mut blocked: Option<u32> = None;
        let mut found: Option<u32> = None;

        for &index in self.unindexed.iter() {
            match self.check(index, &mut probe) {
                Some(true) => {
                    found = Some(index);
                    break;
                }
                Some(false) => blocked = blocked.or(Some(index)),
                None => {}
            }
        }

        // Probe every token the URL offers, three ways: as itself, and as the
        // prefix/suffix keys that reach rules whose own token is only a part of
        // a URL token. A rule is filed under exactly one key, so it is reached
        // at most once per URL token.
        if found.is_none() {
            for_each_token(probe.url, |start, end| {
                let token = &probe.url[start..end];
                if token.len() < MIN_TOKEN_LEN {
                    return true;
                }
                let keys = [
                    (KeyKind::Exact, token),
                    (KeyKind::Prefix, &token[..MIN_TOKEN_LEN]),
                    (KeyKind::Suffix, &token[token.len() - MIN_TOKEN_LEN..]),
                ];
                for (kind, key) in keys {
                    let Some(bucket) = self.bucket_of(hash_key(kind, key)) else {
                        continue;
                    };
                    self.check_bucket(bucket, &mut probe, &mut found, &mut blocked);
                    if found.is_some() {
                        return false;
                    }
                }
                true
            });
        }

        if probe.budget.exhausted {
            // Rare enough to be an operator signal rather than a hot-path cost:
            // it takes a pattern engineered for backtracking plus a URL chosen
            // to feed it. Counted because the consequence is a silently
            // unenforced rule, which nothing else would ever show.
            self.budget_exhausted.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(index) = found {
            return UrlDecision::Allow(index);
        }
        match blocked {
            Some(index) => UrlDecision::Block(index),
            None => UrlDecision::Pass,
        }
    }

    /// Runs every rule in one bucket against the request, folding the outcome
    /// into the running allow/block state.
    fn check_bucket(
        &self,
        bucket: usize,
        probe: &mut Probe<'_>,
        found: &mut Option<u32>,
        blocked: &mut Option<u32>,
    ) {
        let from = self.bucket_start[bucket] as usize;
        let to = self.bucket_start[bucket + 1] as usize;
        for &index in &self.bucket_rules[from..to] {
            match self.check(index, probe) {
                Some(true) => {
                    *found = Some(index);
                    return;
                }
                Some(false) => *blocked = blocked.or(Some(index)),
                None => {}
            }
        }
    }

    /// Every rule checked, index bypassed — the oracle the token index is
    /// tested against. Test-only: this is the O(rules) cost the index exists to
    /// avoid.
    #[cfg(test)]
    fn lookup_scanning(&self, request: &HttpRequest<'_>) -> UrlDecision {
        let mut probe = Probe::new(request);
        let mut blocked = None;
        for index in 0..self.records.len() as u32 {
            match self.check(index, &mut probe) {
                Some(true) => return UrlDecision::Allow(index),
                Some(false) => blocked = blocked.or(Some(index)),
                None => {}
            }
        }
        match blocked {
            Some(index) => UrlDecision::Block(index),
            None => UrlDecision::Pass,
        }
    }

    fn bucket_of(&self, hash: u64) -> Option<usize> {
        if self.bucket_hash.is_empty() {
            return None;
        }
        let mut slot = fastrange(hash, self.slot_cap);
        while self.slots[slot] != EMPTY {
            let bucket = self.slots[slot] as usize;
            if self.bucket_hash[bucket] == hash {
                return Some(bucket);
            }
            slot += 1;
            if slot == self.slot_cap {
                slot = 0;
            }
        }
        None
    }

    /// `Some(is_allow)` when the rule applies to this request, `None` when it
    /// does not. Ordered cheapest-predicate-first: the pattern match is the
    /// only part that walks bytes, so everything that can rule the record out
    /// runs before it.
    fn check(&self, index: u32, probe: &mut Probe<'_>) -> Option<bool> {
        let record = &self.records[index as usize];

        if record.types != 0 && record.types & probe.type_bit == 0 {
            // A type the proxy could not determine carries no bit, so every
            // type-restricted rule misses it. That is right for a **block** —
            // guessing `$script` wrong would refuse a resource nobody asked to
            // refuse — but backwards for an **exception**: declining
            // `@@…/assets/$script` leaves the block it exists to override
            // standing, which over-blocks the exact resource the list author
            // permitted. Both directions therefore under-block, which is the
            // safe one. (Verified: with `ResourceType::Unknown` and
            // `||cdn.example.com^` + `@@||cdn.example.com/assets/$script`, the
            // request was blocked before this branch existed.)
            if probe.type_bit != 0 || !record.is_allow() {
                return None;
            }
        }
        match record.party() {
            PARTY_THIRD if !probe.third_party => return None,
            PARTY_FIRST if probe.third_party => return None,
            _ => {}
        }
        if record.has_methods() {
            let raw = self.methods.get(&index).map(|raw| &**raw).unwrap_or("");
            if !methods_apply(raw, probe.request.method) {
                return None;
            }
        }
        if record.dom_len != 0
            && !domains_apply(self.domains_of(index), probe.request.document_host)
        {
            return None;
        }

        let pattern = self.pattern_of(index);
        let end_anchored = record.end_anchored();
        let match_case = record.match_case();
        let url = probe.url;
        // Re-arm this rule's share of the work allowance, so one pathological
        // pattern cannot spend the whole lookup's.
        probe.budget.open(url.len());
        let budget = &mut probe.budget;
        let matched = match record.anchor() {
            ANCHOR_START => match_from(pattern, url, 0, end_anchored, match_case, budget),
            ANCHOR_DOMAIN => {
                // `||` anchors at the host, or at any label boundary inside it
                // — so `||example.com` matches `sub.example.com` but never
                // `notexample.com`.
                let mut at = probe.host_start;
                loop {
                    if match_from(pattern, url, at, end_anchored, match_case, budget) {
                        break true;
                    }
                    match url[at..probe.host_end].iter().position(|&b| b == b'.') {
                        Some(offset) => at += offset + 1,
                        None => break false,
                    }
                }
            }
            // An unanchored pattern may start anywhere, but trying every offset
            // costs O(url × pattern) on rules that decide nothing — and the
            // unindexed set pays it on *every* request. So only offsets where
            // the pattern's first element could match are tried; the rest are
            // rejected on one byte comparison.
            _ => match pattern.first() {
                // A leading `*` already tries every start via backtracking.
                Some(b'*') => match_from(pattern, url, 0, end_anchored, match_case, budget),
                // `^` matches a separator or the end of the URL.
                // Each offset that survives the one-byte filter enters
                // `match_from`, which charges the allowance itself — the scan
                // that rejects the rest is a single byte compare and is left
                // unmetered.
                Some(b'^') => (0..=url.len()).any(|at| {
                    (at == url.len() || is_separator(url[at]))
                        && match_from(pattern, url, at, end_anchored, match_case, budget)
                }),
                // A literal first byte cannot match at the end of the URL, so
                // the range stops one short of it.
                Some(&first) => (0..url.len()).any(|at| {
                    byte_matches(first, url[at], match_case)
                        && match_from(pattern, url, at, end_anchored, match_case, budget)
                }),
                None => true,
            },
        };

        matched.then_some(record.is_allow())
    }

    /// Reconstructs the rule's canonical text for the query log. Allocates —
    /// call only once a rule has decided a request, never on `Pass`.
    pub(crate) fn rule_text(&self, index: u32) -> String {
        let record = &self.records[index as usize];
        let pattern = std::str::from_utf8(self.pattern_of(index)).unwrap_or("");
        let mut text = String::with_capacity(pattern.len() + 16);
        if record.is_allow() {
            text.push_str("@@");
        }
        match record.anchor() {
            ANCHOR_DOMAIN => text.push_str("||"),
            ANCHOR_START => text.push('|'),
            _ => {}
        }
        text.push_str(pattern);
        if record.end_anchored() {
            text.push('|');
        }

        let mut separator = '$';
        let mut option = |text: &mut String, body: &str| {
            text.push(separator);
            text.push_str(body);
            separator = ',';
        };
        if record.types != 0 {
            append_types(&mut text, record.types, &mut option);
        }
        match record.party() {
            PARTY_THIRD => option(&mut text, "third-party"),
            PARTY_FIRST => option(&mut text, "first-party"),
            _ => {}
        }
        if record.match_case() {
            option(&mut text, "match-case");
        }
        if record.dom_len != 0 {
            let domains = String::from_utf8_lossy(self.domains_of(index));
            option(&mut text, &format!("domain={domains}"));
        }
        if let Some(raw) = self.methods.get(&index) {
            option(&mut text, &format!("method={raw}"));
        }
        text
    }

    /// Approximate resident bytes — the figure p2-03's headroom criterion is
    /// measured against. Same convention as the domain tier: what the compiled
    /// structure asked for, not RSS.
    pub(crate) fn heap_bytes(&self) -> usize {
        let arc_overhead = 16;
        let methods: usize = self
            .methods
            .values()
            .map(|raw| 4 + arc_overhead + raw.len() + 8)
            .sum();
        self.patterns.len()
            + self.domains.len()
            + self.records.len() * std::mem::size_of::<Record>()
            + self.list_ids.len() * std::mem::size_of::<u16>()
            + self.slots.len() * std::mem::size_of::<u32>()
            + self.bucket_hash.len() * std::mem::size_of::<u64>()
            + self.bucket_start.len() * std::mem::size_of::<u32>()
            + self.bucket_rules.len() * std::mem::size_of::<u32>()
            + self.unindexed.len() * std::mem::size_of::<u32>()
            + methods
    }
}

/// Writes a resource-type mask back as options, preferring the negated form
/// when that is the shorter truth (`~script` rather than eleven positives).
fn append_types(text: &mut String, types: u16, option: &mut impl FnMut(&mut String, &str)) {
    const NAMES: [&str; 12] = [
        "document",
        "subdocument",
        "script",
        "stylesheet",
        "image",
        "font",
        "media",
        "xmlhttprequest",
        "websocket",
        "ping",
        "object",
        "other",
    ];
    let negate = types.count_ones() > NAMES.len() as u32 / 2;
    for (index, name) in NAMES.iter().enumerate() {
        let present = types & (1 << index) != 0;
        if present == negate {
            continue;
        }
        if negate {
            option(text, &format!("~{name}"));
        } else {
            option(text, name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fah_model::ResourceType;

    fn rule(pattern: &str) -> UrlRule {
        UrlRule {
            pattern: Arc::from(pattern),
            action: RuleAction::Block,
            anchor: UrlAnchor::Anywhere,
            end_anchored: false,
            match_case: false,
            party: Party::Any,
            resource_types: 0,
            domains: None,
            methods: None,
        }
    }

    fn request<'a>(url: &'a str, host: &'a str) -> HttpRequest<'a> {
        HttpRequest {
            url,
            host,
            method: "GET",
            resource_type: ResourceType::Unknown,
            document_host: None,
        }
    }

    fn index_of(rules: Vec<UrlRule>) -> UrlIndex {
        let mut builder = UrlIndexBuilder::default();
        for rule in &rules {
            builder.add(0, rule);
        }
        builder.build()
    }

    fn blocks(index: &UrlIndex, request: &HttpRequest<'_>) -> bool {
        matches!(index.lookup(request), UrlDecision::Block(_))
    }

    // ─── The matcher itself ───────────────────────────────────────────────

    #[test]
    fn a_substring_pattern_matches_anywhere_in_the_url() {
        let index = index_of(vec![rule("/ads/banner")]);
        assert!(blocks(
            &index,
            &request("http://example.com/img/ads/banner.gif", "example.com")
        ));
        assert!(!blocks(
            &index,
            &request("http://example.com/img/banner.gif", "example.com")
        ));
    }

    #[test]
    fn a_wildcard_spans_any_run_of_bytes() {
        let index = index_of(vec![rule("/banner/*/ad.js")]);
        assert!(blocks(
            &index,
            &request(
                "http://x.example.com/banner/deep/nested/ad.js",
                "x.example.com"
            )
        ));
        assert!(!blocks(
            &index,
            &request("http://x.example.com/banner/ad.css", "x.example.com")
        ));
    }

    #[test]
    fn the_separator_matches_a_delimiter_or_the_end_of_the_url() {
        let index = index_of(vec![UrlRule {
            anchor: UrlAnchor::Domain,
            ..rule("ads.example.com^")
        }]);
        // `^` consumes the `/`.
        assert!(blocks(
            &index,
            &request("http://ads.example.com/pixel", "ads.example.com")
        ));
        // …and matches end-of-URL, consuming nothing.
        assert!(blocks(
            &index,
            &request("http://ads.example.com", "ads.example.com")
        ));
        // But a longer label is not a separator boundary.
        assert!(!blocks(
            &index,
            &request(
                "http://ads.example.community/pixel",
                "ads.example.community"
            )
        ));
    }

    #[test]
    fn the_domain_anchor_matches_label_boundaries_only() {
        let index = index_of(vec![UrlRule {
            anchor: UrlAnchor::Domain,
            ..rule("example.com/track")
        }]);
        assert!(blocks(
            &index,
            &request("http://example.com/track", "example.com")
        ));
        assert!(blocks(
            &index,
            &request("http://deep.sub.example.com/track", "deep.sub.example.com")
        ));
        assert!(
            !blocks(
                &index,
                &request("http://notexample.com/track", "notexample.com")
            ),
            "`||` must anchor at a label boundary, not any substring"
        );
    }

    #[test]
    fn the_start_anchor_matches_only_at_position_zero() {
        let index = index_of(vec![UrlRule {
            anchor: UrlAnchor::Start,
            ..rule("http://ads.example.com/")
        }]);
        assert!(blocks(
            &index,
            &request("http://ads.example.com/pixel", "ads.example.com")
        ));
        assert!(!blocks(
            &index,
            &request(
                "http://example.com/?u=http://ads.example.com/",
                "example.com"
            )
        ));
    }

    #[test]
    fn the_end_anchor_requires_the_pattern_to_reach_the_url_end() {
        let index = index_of(vec![UrlRule {
            end_anchored: true,
            ..rule("/ad.js")
        }]);
        assert!(blocks(
            &index,
            &request("http://example.com/ad.js", "example.com")
        ));
        assert!(!blocks(
            &index,
            &request("http://example.com/ad.js?v=2", "example.com")
        ));
    }

    #[test]
    fn matching_is_case_insensitive_unless_the_rule_says_otherwise() {
        let folded = index_of(vec![rule("/track/pixel")]);
        assert!(blocks(
            &folded,
            &request("http://example.com/TRACK/Pixel", "example.com")
        ));

        let cased = index_of(vec![UrlRule {
            match_case: true,
            ..rule("/Track/Pixel")
        }]);
        assert!(blocks(
            &cased,
            &request("http://example.com/Track/Pixel", "example.com")
        ));
        assert!(!blocks(
            &cased,
            &request("http://example.com/track/pixel", "example.com")
        ));
    }

    #[test]
    fn a_pattern_of_only_wildcards_still_terminates() {
        let index = index_of(vec![rule("/a***b")]);
        assert!(blocks(&index, &request("http://e.com/aXXb", "e.com")));
        assert!(!blocks(&index, &request("http://e.com/aXX", "e.com")));
    }

    // ─── Precedence ───────────────────────────────────────────────────────

    #[test]
    fn an_exception_beats_a_block_however_they_are_ordered() {
        for reversed in [false, true] {
            let mut rules = vec![
                rule("/ads/banner"),
                UrlRule {
                    action: RuleAction::Allow,
                    ..rule("/ads/banner")
                },
            ];
            if reversed {
                rules.reverse();
            }
            let index = index_of(rules);
            assert!(matches!(
                index.lookup(&request("http://e.com/ads/banner.gif", "e.com")),
                UrlDecision::Allow(_)
            ));
        }
    }

    #[test]
    fn nothing_matching_is_a_pass() {
        let index = index_of(vec![rule("/ads/banner")]);
        assert!(matches!(
            index.lookup(&request("http://e.com/index.html", "e.com")),
            UrlDecision::Pass
        ));
    }

    // ─── Options ──────────────────────────────────────────────────────────

    #[test]
    fn a_type_restricted_rule_ignores_other_types() {
        let script = crate::resource::bit_for_option("script").unwrap();
        let index = index_of(vec![UrlRule {
            resource_types: script,
            ..rule("/widget")
        }]);
        let mut req = request("http://e.com/widget.js", "e.com");
        req.resource_type = ResourceType::Script;
        assert!(blocks(&index, &req));
        req.resource_type = ResourceType::Image;
        assert!(!blocks(&index, &req));
        // An undetermined type is never caught by a type-restricted rule.
        req.resource_type = ResourceType::Unknown;
        assert!(!blocks(&index, &req));
    }

    #[test]
    fn third_party_needs_a_referer_from_another_registrable_domain() {
        let index = index_of(vec![UrlRule {
            party: Party::Third,
            ..rule("/pixel.gif")
        }]);
        let mut req = request("http://ads.example.com/pixel.gif", "ads.example.com");
        assert!(!blocks(&index, &req), "no referer means first-party");

        req.document_host = Some("www.example.com");
        assert!(!blocks(&index, &req), "same site is not third-party");

        req.document_host = Some("news.other.org");
        assert!(blocks(&index, &req));
    }

    #[test]
    fn first_party_is_the_exact_complement() {
        let index = index_of(vec![UrlRule {
            party: Party::First,
            ..rule("/pixel.gif")
        }]);
        let mut req = request("http://ads.example.com/pixel.gif", "ads.example.com");
        req.document_host = Some("www.example.com");
        assert!(blocks(&index, &req));
        req.document_host = Some("news.other.org");
        assert!(!blocks(&index, &req));
    }

    #[test]
    fn domain_option_whitelists_and_vetoes() {
        let index = index_of(vec![UrlRule {
            domains: Some(Arc::from("news.org|~sport.news.org")),
            ..rule("/pixel.gif")
        }]);
        let mut req = request("http://ads.example.com/pixel.gif", "ads.example.com");

        req.document_host = Some("news.org");
        assert!(blocks(&index, &req));
        req.document_host = Some("world.news.org");
        assert!(blocks(&index, &req), "subdomains fall under the entry");
        req.document_host = Some("sport.news.org");
        assert!(!blocks(&index, &req), "an explicit `~` entry vetoes");
        req.document_host = Some("other.com");
        assert!(!blocks(&index, &req));
        req.document_host = None;
        assert!(
            !blocks(&index, &req),
            "no document cannot satisfy a positive entry"
        );
    }

    /// `$domain=example.*` is uBO's "under any public suffix" form. Without
    /// this arm the entry matched no host at all, so the rule compiled, counted
    /// as active, and silently never fired.
    #[test]
    fn a_wildcard_domain_entry_matches_any_public_suffix() {
        let index = index_of(vec![UrlRule {
            domains: Some(Arc::from("example.*")),
            ..rule("/pixel.gif")
        }]);
        let mut req = request("http://ads.other.com/pixel.gif", "ads.other.com");
        for document in ["example.com", "example.co.uk", "www.example.com"] {
            req.document_host = Some(document);
            assert!(blocks(&index, &req), "{document} is under example.*");
        }
        for document in ["notexample.com", "example.com.evil.net", "other.com"] {
            req.document_host = Some(document);
            assert!(!blocks(&index, &req), "{document} is not under example.*");
        }
    }

    /// A type-restricted **exception** must still apply when the proxy could
    /// not determine the type. Declining it is not the conservative choice: it
    /// leaves whatever block the exception exists to override standing, which
    /// over-blocks. A type-restricted **block** still declines.
    #[test]
    fn an_undetermined_type_declines_a_typed_block_but_not_a_typed_exception() {
        let script = crate::resource::bit_for_option("script").unwrap();
        let index = index_of(vec![
            UrlRule {
                resource_types: script,
                ..rule("/assets/")
            },
            UrlRule {
                action: RuleAction::Allow,
                resource_types: script,
                ..rule("/assets/")
            },
        ]);
        let mut req = request("http://cdn.example.com/assets/a.js", "cdn.example.com");
        req.resource_type = ResourceType::Unknown;
        assert!(
            matches!(index.lookup(&req), UrlDecision::Allow(_)),
            "the exception must survive an undetermined type"
        );

        let blocks_only = index_of(vec![UrlRule {
            resource_types: script,
            ..rule("/assets/")
        }]);
        assert!(
            !blocks(&blocks_only, &req),
            "a typed block must still decline an undetermined type"
        );
    }

    #[test]
    fn a_purely_negated_domain_option_applies_everywhere_else() {
        let index = index_of(vec![UrlRule {
            domains: Some(Arc::from("~safe.org")),
            ..rule("/pixel.gif")
        }]);
        let mut req = request("http://ads.example.com/pixel.gif", "ads.example.com");
        req.document_host = Some("safe.org");
        assert!(!blocks(&index, &req));
        req.document_host = Some("elsewhere.com");
        assert!(blocks(&index, &req));
    }

    #[test]
    fn method_option_selects_and_vetoes() {
        let index = index_of(vec![UrlRule {
            methods: Some(Arc::from("POST")),
            ..rule("/collect")
        }]);
        let mut req = request("http://e.com/collect", "e.com");
        assert!(!blocks(&index, &req));
        req.method = "POST";
        assert!(blocks(&index, &req));
    }

    // ─── Index integrity ──────────────────────────────────────────────────

    /// The property the whole index rests on: **the index must decide exactly
    /// what a full scan decides.** Every key kind is represented, and every URL
    /// is checked both ways — an indexing bug shows up as a rule that silently
    /// stops firing, which no latency bench and no fixture test would catch.
    #[test]
    fn the_index_decides_exactly_what_a_full_scan_decides() {
        let patterns = [
            // Exact keys: bounded on both sides.
            "/ads/banner",
            "tracker.example.com/collect",
            "&utm_source=",
            "^adserver^",
            "/pixel.gif",
            "/a/b",
            // Prefix keys: bounded left, open right.
            "/adserver",
            "&campaign_id",
            "-metrics",
            // Suffix keys: open left, bounded right.
            "*trackpixel/",
            "*_analytics.js",
            // Unindexable: open on both sides.
            "*banneroid*",
            "*promo",
        ];
        let rules: Vec<UrlRule> = patterns.iter().map(|p| rule(p)).collect();
        let index = index_of(rules);

        let urls = [
            "http://example.com/img/ads/banner.gif",
            "http://tracker.example.com/collect?id=1",
            "http://example.com/p?&utm_source=x",
            "http://example.com/adserver/x",
            "http://example.com/pixel.gif",
            "http://example.com/a/b",
            // A prefix key's URL token is *longer* than the pattern token.
            "http://example.com/adserver12/beacon",
            "http://example.com/p?&campaign_id_v2=7",
            "http://example.com/-metrics-collector/x",
            // A suffix key's URL token is longer on the left.
            "http://cdn.example.com/mytrackpixel/1",
            "http://cdn.example.com/vendor_analytics.js",
            // Unindexable rules must still fire, via the fallback set.
            "http://cdn.example.com/xbanneroidx/1",
            "http://cdn.example.com/megapromo",
            // …and plenty that match nothing.
            "http://example.com/nothing/here",
            "http://www.wikipedia.org/wiki/Rust",
            "http://example.com/trackpixel",
        ];
        for url in urls {
            let req = request(url, "example.com");
            let indexed = index.lookup(&req);
            let scanned = index.lookup_scanning(&req);
            assert_eq!(
                matches!(indexed, UrlDecision::Block(_)),
                matches!(scanned, UrlDecision::Block(_)),
                "index and full scan disagree on {url}"
            );
        }
    }

    /// A token unbounded on **one** side is still usable — as a prefix or a
    /// suffix key. Only one unbounded on both sides falls back to the scan.
    #[test]
    fn one_sided_tokens_are_indexed_and_only_two_sided_ones_fall_back() {
        assert_eq!(index_of(vec![rule("/adserver")]).unindexed_len(), 0);
        assert_eq!(index_of(vec![rule("*trackpixel/")]).unindexed_len(), 0);
        assert_eq!(
            index_of(vec![rule("*trackpixel")]).unindexed_len(),
            1,
            "open on both sides: no key can reach it"
        );

        // …and being indexed must not change the verdict.
        let index = index_of(vec![rule("/adserver")]);
        assert!(blocks(
            &index,
            &request("http://e.com/adserver99/x", "e.com")
        ));
        let index = index_of(vec![rule("*trackpixel/")]);
        assert!(blocks(
            &index,
            &request("http://e.com/mytrackpixel/1", "e.com")
        ));
    }

    /// The fixture list above covers the shapes we thought of. This covers the
    /// ones we did not: random patterns over an alphabet of anchors,
    /// separators, wildcards and letters, each ruleset checked both ways.
    #[test]
    fn the_index_agrees_with_a_full_scan_on_random_rulesets() {
        // xorshift64, so a failure is reproducible from the seed alone.
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        const ALPHABET: &[u8] = b"ad/.-_*^&=?xyz1";

        for round in 0..200 {
            let rules: Vec<UrlRule> = (0..12)
                .map(|_| {
                    let pattern: String = (0..1 + next() as usize % 10)
                        .map(|_| ALPHABET[next() as usize % ALPHABET.len()] as char)
                        .collect();
                    UrlRule {
                        anchor: match next() % 3 {
                            0 => UrlAnchor::Anywhere,
                            1 => UrlAnchor::Start,
                            _ => UrlAnchor::Domain,
                        },
                        end_anchored: next() % 4 == 0,
                        action: if next() % 5 == 0 {
                            RuleAction::Allow
                        } else {
                            RuleAction::Block
                        },
                        ..rule(&pattern)
                    }
                })
                .collect();
            let index = index_of(rules.clone());

            for _ in 0..40 {
                let tail: String = (0..1 + next() as usize % 24)
                    .map(|_| ALPHABET[next() as usize % (ALPHABET.len() - 2)] as char)
                    .filter(|byte| *byte != '*' && *byte != '^')
                    .collect();
                let url = format!("http://host.example.com/{tail}");
                let req = request(&url, "host.example.com");
                let outcome = |decision: UrlDecision| match decision {
                    UrlDecision::Allow(_) => 2,
                    UrlDecision::Block(_) => 1,
                    UrlDecision::Pass => 0,
                };
                assert_eq!(
                    outcome(index.lookup(&req)),
                    outcome(index.lookup_scanning(&req)),
                    "round {round}: index and full scan disagree on {url}\npatterns: {:?}",
                    rules.iter().map(|r| &*r.pattern).collect::<Vec<_>>()
                );
            }
        }
    }

    // ─── The work allowance ───────────────────────────────────────────────

    /// A pattern engineered for backtracking, against a URL chosen to feed it,
    /// measured **313 ms for one request** before the allowance existed — six
    /// orders past the `< 1 ms` budget, driven entirely by client-chosen input.
    #[test]
    fn a_pathological_pattern_cannot_spend_an_unbounded_lookup() {
        let pattern = format!("{}*{}b", "a".repeat(40), "a".repeat(40));
        let index = index_of(vec![rule(&pattern)]);
        let url = format!("http://h.example.com/{}", "a".repeat(4000));
        let req = request(&url, "h.example.com");

        let start = std::time::Instant::now();
        let decision = index.lookup(&req);
        let elapsed = start.elapsed();

        assert!(matches!(decision, UrlDecision::Pass));
        assert_eq!(
            index.budget_exhausted(),
            1,
            "the lookup must report that it stopped short, not fail silently"
        );
        // Deliberately loose: this runs in a debug build on unknown hardware.
        // It is three orders below the unbounded cost and that is the claim.
        assert!(elapsed.as_millis() < 200, "took {elapsed:?}");
    }

    /// The allowance must be invisible to real rules on a long URL — an 8 KB
    /// query string is ordinary traffic, not an attack.
    #[test]
    fn a_long_url_still_matches_an_ordinary_rule() {
        let index = index_of(vec![rule("/trackpixel")]);
        let url = format!("http://h.example.com/{}/trackpixel", "x".repeat(8000));
        let req = request(&url, "h.example.com");
        assert!(blocks(&index, &req));
        assert_eq!(
            index.budget_exhausted(),
            0,
            "an ordinary rule on a long URL must not be cut short"
        );
    }

    /// The three key kinds share one table, so `Exact("ads")` must not be
    /// reachable by a prefix probe for `ads` — that would file a rule in a
    /// bucket it can never legitimately be found through, and mask real bugs.
    #[test]
    fn the_key_kinds_do_not_collide_with_each_other() {
        let bytes = b"ads";
        let exact = hash_key(KeyKind::Exact, bytes);
        let prefix = hash_key(KeyKind::Prefix, bytes);
        let suffix = hash_key(KeyKind::Suffix, bytes);
        assert_ne!(exact, prefix);
        assert_ne!(exact, suffix);
        assert_ne!(prefix, suffix);
    }

    #[test]
    fn an_identical_rule_from_two_lists_is_compiled_once() {
        let mut builder = UrlIndexBuilder::default();
        builder.add(0, &rule("/ads/banner"));
        builder.add(1, &rule("/ads/banner"));
        builder.add(1, &rule("/ads/other"));
        let index = builder.build();
        assert_eq!(index.len(), 2);
        assert_eq!(index.duplicates_removed(), 1);
        // Attribution stays with the first list to supply it.
        let UrlDecision::Block(idx) = index.lookup(&request("http://e.com/ads/banner", "e.com"))
        else {
            panic!("expected a block");
        };
        assert_eq!(index.list_id(idx), 0);
    }

    #[test]
    fn rules_differing_only_in_an_option_are_distinct() {
        let mut builder = UrlIndexBuilder::default();
        builder.add(0, &rule("/ads/banner"));
        builder.add(
            0,
            &UrlRule {
                party: Party::Third,
                ..rule("/ads/banner")
            },
        );
        builder.add(
            0,
            &UrlRule {
                domains: Some(Arc::from("news.org")),
                ..rule("/ads/banner")
            },
        );
        let index = builder.build();
        assert_eq!(index.len(), 3);
        assert_eq!(index.duplicates_removed(), 0);
    }

    #[test]
    fn an_empty_index_passes_everything() {
        let index = index_of(vec![]);
        assert_eq!(index.len(), 0);
        assert!(matches!(
            index.lookup(&request("http://e.com/ads/banner", "e.com")),
            UrlDecision::Pass
        ));
    }

    // ─── Reconstruction ───────────────────────────────────────────────────

    #[test]
    fn rule_text_round_trips_the_canonical_form() {
        let index = index_of(vec![UrlRule {
            anchor: UrlAnchor::Domain,
            party: Party::Third,
            domains: Some(Arc::from("news.org")),
            ..rule("ads.example.com^*/pixel.gif")
        }]);
        assert_eq!(
            index.rule_text(0),
            "||ads.example.com^*/pixel.gif$third-party,domain=news.org"
        );
    }

    #[test]
    fn rule_text_marks_exceptions_and_anchors() {
        let index = index_of(vec![UrlRule {
            action: RuleAction::Allow,
            anchor: UrlAnchor::Start,
            end_anchored: true,
            ..rule("http://cdn.example.com/app.js")
        }]);
        assert_eq!(index.rule_text(0), "@@|http://cdn.example.com/app.js|");
    }

    #[test]
    fn rule_text_prefers_the_negated_form_for_a_near_complete_type_set() {
        let script = crate::resource::bit_for_option("script").unwrap();
        let index = index_of(vec![UrlRule {
            resource_types: crate::resource::ALL_TYPES & !script,
            ..rule("/widget")
        }]);
        assert_eq!(index.rule_text(0), "/widget$~script");
    }

    // ─── Helpers ──────────────────────────────────────────────────────────

    #[test]
    fn host_span_finds_the_authority() {
        let url = "http://ads.example.com:8080/path?q=1";
        let (start, end) = host_span(url.as_bytes());
        assert_eq!(&url[start..end], "ads.example.com:8080");
        let bare = "example.com/x";
        let (start, end) = host_span(bare.as_bytes());
        assert_eq!(&bare[start..end], "example.com");
    }

    #[test]
    fn registrable_takes_the_last_two_labels() {
        assert_eq!(registrable("a.b.example.com"), "example.com");
        assert_eq!(registrable("example.com"), "example.com");
        assert_eq!(registrable("localhost"), "localhost");
    }
}
