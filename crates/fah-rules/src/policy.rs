//! Compiled Policies (CONTEXT.md §Policy): which policy a client is judged
//! under right now, and which rules that policy is allowed to see.
//!
//! # Why one ruleset and not one per policy
//!
//! A policy is a subset of the configured rule lists, so the obvious shape is
//! one compiled [`crate::Matcher`] per policy. Measured, that costs the whole
//! corpus again per policy — 12.099 MiB for two policies over four lists where
//! their union is 6.839 MiB, which at deployed scale (21.9 MiB, ~24 MiB of
//! headroom against PERFORMANCE.md's 128 MB budget) overruns on the *second*
//! policy.
//!
//! So the union is compiled once and every rule carries a `u16` mask of the
//! policies that can see it. That is ~2 MiB at 1.06 M rules — **flat**, not per
//! policy — and it is allocated only when a second policy exists, so a
//! zero-config deployment carries no masks at all.
//!
//! The mask has to be per *rule* rather than per *list* because deduplication
//! collapses a rule appearing in several lists into one record attributed to
//! the first (`Matcher` §Deduplication). Keying policy membership off that one
//! attribution would hide the rule from every policy that enabled only the
//! other list, so [`crate::MatcherBuilder`] unions the masks of every list a
//! duplicate arrives from.

use std::net::IpAddr;
use std::sync::Arc;

use fah_config::{parse_days, parse_time_of_day, AssignmentConfig, PolicyConfig, PosixTz};
use fah_model::{Assignment, ClientSelector, Policy, PolicyId, Schedule};

/// Every policy bit set — what a rule carries when no policies are configured,
/// and the sentinel [`crate::MatcherBuilder`] uses to detect that it need not
/// allocate a mask array at all.
pub(crate) const ALL_POLICIES: u16 = u16::MAX;

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("invalid timezone: {0}")]
    Timezone(String),
    #[error("at most {max} policies may exist including the default; got {got}")]
    TooMany { max: usize, got: usize },
    #[error("policy {policy:?}: {message}")]
    Assignment { policy: String, message: String },
}

/// One policy, compiled.
#[derive(Debug, Clone, PartialEq)]
struct CompiledPolicy {
    policy: Policy,
    /// `None` means every enabled list — the default policy's corpus.
    lists: Option<Vec<Arc<str>>>,
}

#[derive(Debug, Clone, PartialEq)]
struct CompiledAssignment {
    assignment: Assignment,
    policy: PolicyId,
}

/// The compiled policy configuration. Immutable and swapped wholesale with the
/// ruleset it was compiled against, like [`crate::Matcher`] itself.
#[derive(Debug, Clone, PartialEq)]
pub struct PolicySet {
    timezone: PosixTz,
    policies: Vec<CompiledPolicy>,
    /// Sorted most-specific first, so [`Self::resolve`] returns on the first
    /// match instead of scoring every assignment.
    assignments: Vec<CompiledAssignment>,
}

impl Default for PolicySet {
    fn default() -> Self {
        PolicySet::single_default()
    }
}

impl PolicySet {
    /// The zero-config policy set: one policy, every enabled list, no
    /// assignments. What every deployment had before Policies existed.
    pub fn single_default() -> PolicySet {
        PolicySet {
            timezone: PosixTz::UTC,
            policies: vec![CompiledPolicy {
                policy: Policy {
                    id: "default".to_string(),
                    name: "Default".to_string(),
                    lists: Vec::new(),
                    blocking_mode: None,
                },
                lists: None,
            }],
            assignments: Vec::new(),
        }
    }

    /// Compiles `[schedule]` + `[[policies]]`.
    ///
    /// Everything here was already checked by `Config::validate`; this repeats
    /// the parsing because the config crate cannot hand over `fah-model` types
    /// (sibling L1) and a second parse of ~10 short strings at compile time is
    /// cheaper than the machinery to avoid it.
    pub fn from_config(
        timezone: &str,
        policies: &[PolicyConfig],
    ) -> Result<PolicySet, PolicyError> {
        let timezone =
            PosixTz::parse(timezone).map_err(|err| PolicyError::Timezone(err.to_string()))?;

        if policies.len() + 1 > PolicyId::MAX {
            return Err(PolicyError::TooMany {
                max: PolicyId::MAX,
                got: policies.len() + 1,
            });
        }

        let mut set = PolicySet::single_default();
        set.timezone = timezone;

        for (index, config) in policies.iter().enumerate() {
            // +1: index 0 is the default policy, which is never configured.
            let id = PolicyId((index + 1) as u8);
            for assignment in &config.assignments {
                set.assignments.push(CompiledAssignment {
                    assignment: compile_assignment(&config.id, assignment)?,
                    policy: id,
                });
            }
            set.policies.push(CompiledPolicy {
                policy: Policy {
                    id: config.id.clone(),
                    name: config.name.clone().unwrap_or_else(|| config.id.clone()),
                    lists: config.lists.clone().unwrap_or_default(),
                    blocking_mode: config.blocking_mode.clone(),
                },
                lists: config
                    .lists
                    .as_ref()
                    .map(|lists| lists.iter().map(|list| Arc::from(list.as_str())).collect()),
            });
        }

        // Most specific first. `sort_by` is stable, so two assignments of equal
        // specificity keep config order and the earlier one wins — which is the
        // only tie-break an operator can predict from the file.
        set.assignments.sort_by(|a, b| {
            b.assignment
                .client
                .specificity()
                .cmp(&a.assignment.client.specificity())
        });

        Ok(set)
    }

    /// How many policies exist, the default included.
    pub fn len(&self) -> usize {
        self.policies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
    }

    /// True when nothing but the default policy exists, which is when the
    /// compiled ruleset can skip its per-rule masks entirely.
    pub fn is_default_only(&self) -> bool {
        self.policies.len() <= 1
    }

    pub fn timezone(&self) -> PosixTz {
        self.timezone
    }

    /// The policy with this id, if it is defined.
    pub fn id_of(&self, id: &str) -> Option<PolicyId> {
        self.policies
            .iter()
            .position(|compiled| compiled.policy.id == id)
            .map(|index| PolicyId(index as u8))
    }

    pub fn get(&self, id: PolicyId) -> Option<&Policy> {
        self.policies
            .get(id.index())
            .map(|compiled| &compiled.policy)
    }

    /// The mask of policies that can see rules from `list` — what
    /// [`crate::MatcherBuilder::add_parsed_list_masked`] stamps on each record.
    ///
    /// The default policy's bit is always set: it is every enabled list by
    /// definition, and a list an operator disabled never reaches the compile.
    pub fn mask_for_list(&self, list: &str) -> u16 {
        if self.is_default_only() {
            return ALL_POLICIES;
        }
        let mut mask = PolicyId::DEFAULT.bit();
        for (index, compiled) in self.policies.iter().enumerate().skip(1) {
            let sees = match &compiled.lists {
                // A policy that names no lists inherits the whole corpus, so a
                // policy overriding only a setting need not restate them.
                None => true,
                Some(lists) => lists.iter().any(|name| &**name == list),
            };
            if sees {
                mask |= PolicyId(index as u8).bit();
            }
        }
        mask
    }

    /// Which policy judges this client at `now_unix_seconds`.
    ///
    /// The first matching assignment wins, most specific first. An assignment
    /// whose schedule is not currently active does not match, and the search
    /// **continues** to the next one — so a device with "kids on school nights"
    /// still falls back to whatever subnet-wide assignment covers it, and only
    /// reaches [`PolicyId::DEFAULT`] when nothing at all applies.
    ///
    /// The alternative — an inactive schedule short-circuiting straight to the
    /// default — reads well for a single device ("unrestricted the rest of the
    /// week") and is the wrong safety default: it would let a scheduled
    /// override silently exempt that one device from a policy its operator
    /// applied to the whole subnet, in the hours the override is *not* in
    /// force. Fewer restrictions than were configured is the failure worth
    /// avoiding.
    pub fn resolve(&self, ip: IpAddr, name: Option<&str>, now_unix_seconds: i64) -> PolicyId {
        if self.assignments.is_empty() {
            return PolicyId::DEFAULT;
        }
        let local = self.timezone.local(now_unix_seconds);
        for compiled in &self.assignments {
            if !compiled.assignment.client.matches(ip, name) {
                continue;
            }
            let active = match compiled.assignment.schedule {
                None => true,
                Some(schedule) => schedule.contains(local.weekday, local.minute_of_day),
            };
            if active {
                return compiled.policy;
            }
        }
        PolicyId::DEFAULT
    }
}

fn compile_assignment(policy: &str, config: &AssignmentConfig) -> Result<Assignment, PolicyError> {
    let fail = |message: String| PolicyError::Assignment {
        policy: policy.to_string(),
        message,
    };

    let client = parse_selector(&config.client).ok_or_else(|| {
        fail(format!(
            "{:?} is not an address, prefix or name",
            config.client
        ))
    })?;

    let schedule = match (&config.start, &config.end) {
        (Some(start), Some(end)) => {
            let days = match &config.days {
                Some(days) => parse_days(days).map_err(fail)?,
                None => Schedule::ALL_DAYS,
            };
            Some(Schedule {
                days,
                start: parse_time_of_day(start).map_err(fail)?,
                end: parse_time_of_day(end).map_err(fail)?,
            })
        }
        (None, None) => {
            // `days` without times is still a schedule: whole days.
            match &config.days {
                Some(days) => Some(Schedule {
                    days: parse_days(days).map_err(fail)?,
                    start: 0,
                    end: 1440,
                }),
                None => None,
            }
        }
        _ => return Err(fail("a schedule needs both start and end".to_string())),
    };

    Ok(Assignment {
        client,
        policy: policy.to_string(),
        schedule,
    })
}

/// `192.168.1.50`, `192.168.1.0/24`, or a client name. A `/` commits the value
/// to being a prefix, so a malformed address cannot quietly become a name that
/// never matches anything.
pub(crate) fn parse_selector(spec: &str) -> Option<ClientSelector> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    if let Some((address, prefix)) = spec.split_once('/') {
        let address: IpAddr = address.parse().ok()?;
        let prefix_len: u8 = prefix.parse().ok()?;
        let max = if address.is_ipv4() { 32 } else { 128 };
        if prefix_len > max {
            return None;
        }
        return Some(ClientSelector::Network {
            address,
            prefix_len,
        });
    }
    match spec.parse::<IpAddr>() {
        Ok(address) => Some(ClientSelector::Ip(address)),
        Err(_) => Some(ClientSelector::Name(spec.to_string())),
    }
}

/// A compiled `$client=` payload — a rule scoped to particular clients, which
/// RULE_ENGINE.md describes as an inline per-client policy.
///
/// Stored in a side map keyed by record index rather than in the record, like
/// `$dnstype` and `$dnsrewrite`: the public lists carry essentially none of
/// these (0 across the deployed corpus, 1 apiece in EasyList and EasyPrivacy),
/// so the cost belongs on the rules that use it, not on every record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientScope {
    /// The payload as written, for `decisive_rule` to echo back.
    pub(crate) raw: Arc<str>,
    terms: Vec<ScopeTerm>,
    has_positive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScopeTerm {
    negated: bool,
    selector: ClientSelector,
}

impl ClientScope {
    /// Compiles `a|b|~c`. Returns `None` when nothing in the payload parses —
    /// the caller then drops the rule rather than applying it without its
    /// restriction, which would widen a one-device rule to the whole network.
    pub(crate) fn parse(raw: &str) -> Option<ClientScope> {
        let mut terms = Vec::new();
        let mut has_positive = false;
        for term in raw.split('|') {
            let term = term.trim();
            if term.is_empty() {
                continue;
            }
            let (negated, spec) = match term.strip_prefix('~') {
                Some(rest) => (true, rest),
                None => (false, term),
            };
            let selector = parse_selector(spec)?;
            has_positive |= !negated;
            terms.push(ScopeTerm { negated, selector });
        }
        if terms.is_empty() {
            return None;
        }
        Some(ClientScope {
            raw: Arc::from(raw),
            terms,
            has_positive,
        })
    }

    /// Bytes this scope holds, for `Matcher::heap_bytes`.
    pub(crate) fn heap_bytes(&self) -> usize {
        self.raw.len()
            + self.terms.capacity() * std::mem::size_of::<ScopeTerm>()
            + self
                .terms
                .iter()
                .map(|term| match &term.selector {
                    ClientSelector::Name(name) => name.capacity(),
                    _ => 0,
                })
                .sum::<usize>()
    }

    /// Whether a rule so scoped applies to this client.
    ///
    /// An unidentified client (no address — the API's `rules/test`, or any
    /// caller with no request behind it) matches no selector, so a rule with
    /// positive terms does not apply to it and one with only negations does.
    /// That is the same reading `ClientSelector::matches` gives, and it errs
    /// toward the rule's own default in both directions.
    pub(crate) fn admits(&self, ip: Option<IpAddr>, name: Option<&str>) -> bool {
        let matches = |term: &ScopeTerm| {
            ip.is_some_and(|ip| term.selector.matches(ip, name))
                || (ip.is_none() && matches!(&term.selector, ClientSelector::Name(_)))
                    && term.selector.matches(UNSPECIFIED, name)
        };

        for term in &self.terms {
            if term.negated && matches(term) {
                return false;
            }
        }
        if !self.has_positive {
            return true;
        }
        self.terms.iter().any(|term| !term.negated && matches(term))
    }
}

/// Stands in for "no address" when only a name is being compared, so a named
/// client with no address still matches a `$client=laptop` rule.
const UNSPECIFIED: IpAddr = IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn policy(id: &str, lists: Option<Vec<&str>>) -> PolicyConfig {
        PolicyConfig {
            id: id.to_string(),
            name: None,
            lists: lists.map(|lists| lists.iter().map(|l| l.to_string()).collect()),
            blocking_mode: None,
            assignments: Vec::new(),
        }
    }

    fn assign(client: &str) -> AssignmentConfig {
        AssignmentConfig {
            client: client.to_string(),
            days: None,
            start: None,
            end: None,
        }
    }

    /// 2026-08-01T12:00:00Z — a Saturday.
    const SATURDAY_NOON: i64 = 1_785_585_600;
    /// 2026-08-03T22:00:00Z — a Monday.
    const MONDAY_EVENING: i64 = 1_785_794_400;

    #[test]
    fn zero_config_is_one_policy_that_sees_everything() {
        let set = PolicySet::single_default();
        assert!(set.is_default_only());
        assert_eq!(set.len(), 1);
        assert_eq!(set.mask_for_list("anything"), ALL_POLICIES);
        assert_eq!(
            set.resolve(ip("192.168.1.50"), None, SATURDAY_NOON),
            PolicyId::DEFAULT
        );
    }

    #[test]
    fn a_policy_sees_only_the_lists_it_names_and_default_sees_all() {
        let set =
            PolicySet::from_config("UTC", &[policy("kids", Some(vec!["oisd", "adult"]))]).unwrap();

        let kids = set.id_of("kids").unwrap();
        assert_eq!(kids, PolicyId(1));
        // Default's bit is set on every list; kids' only on the two it names.
        assert_eq!(
            set.mask_for_list("oisd"),
            PolicyId::DEFAULT.bit() | kids.bit()
        );
        assert_eq!(set.mask_for_list("easylist"), PolicyId::DEFAULT.bit());
    }

    /// A policy that names no lists inherits the whole corpus, so overriding
    /// only a schedule does not mean restating every list.
    #[test]
    fn a_policy_without_lists_inherits_every_list() {
        let set = PolicySet::from_config("UTC", &[policy("guest", None)]).unwrap();
        let guest = set.id_of("guest").unwrap();
        assert_eq!(
            set.mask_for_list("whatever"),
            PolicyId::DEFAULT.bit() | guest.bit()
        );
    }

    #[test]
    fn the_most_specific_assignment_wins_regardless_of_config_order() {
        let mut subnet = policy("guest", None);
        subnet.assignments.push(assign("192.168.1.0/24"));
        let mut host = policy("kids", None);
        host.assignments.push(assign("192.168.1.50"));

        // Subnet is configured first; the host assignment must still win.
        let set = PolicySet::from_config("UTC", &[subnet, host]).unwrap();
        assert_eq!(
            set.resolve(ip("192.168.1.50"), None, SATURDAY_NOON),
            set.id_of("kids").unwrap()
        );
        assert_eq!(
            set.resolve(ip("192.168.1.51"), None, SATURDAY_NOON),
            set.id_of("guest").unwrap()
        );
    }

    /// An expired window hands the client to the next assignment that covers
    /// it, not to the default — the subnet-wide policy its operator configured
    /// must not go inert for the one device carrying a scheduled override.
    #[test]
    fn an_inactive_schedule_falls_back_to_the_next_matching_assignment() {
        let mut kids = policy("kids", None);
        kids.assignments.push(AssignmentConfig {
            client: "192.168.1.50".to_string(),
            days: Some("mon-fri".to_string()),
            start: Some("21:00".to_string()),
            end: Some("07:00".to_string()),
        });
        let mut guest = policy("guest", None);
        guest.assignments.push(assign("192.168.1.0/24"));

        let set = PolicySet::from_config("UTC", &[kids, guest]).unwrap();
        let client = ip("192.168.1.50");

        // Monday 22:00 UTC is inside the window.
        assert_eq!(
            set.resolve(client, None, MONDAY_EVENING),
            set.id_of("kids").unwrap()
        );
        // Saturday noon is not, so the subnet assignment takes over.
        assert_eq!(
            set.resolve(client, None, SATURDAY_NOON),
            set.id_of("guest").unwrap()
        );
        // A client no assignment covers still gets the default.
        assert_eq!(
            set.resolve(ip("10.9.9.9"), None, SATURDAY_NOON),
            PolicyId::DEFAULT
        );
    }

    /// With nothing broader to fall back to, an inactive window does mean the
    /// default — the "unrestricted the rest of the week" case.
    #[test]
    fn an_inactive_schedule_with_no_other_assignment_is_the_default() {
        let mut kids = policy("kids", None);
        kids.assignments.push(AssignmentConfig {
            client: "192.168.1.50".to_string(),
            days: Some("mon-fri".to_string()),
            start: Some("21:00".to_string()),
            end: Some("07:00".to_string()),
        });
        let set = PolicySet::from_config("UTC", &[kids]).unwrap();
        assert_eq!(
            set.resolve(ip("192.168.1.50"), None, SATURDAY_NOON),
            PolicyId::DEFAULT
        );
    }

    /// The same instant lands inside or outside a window depending only on the
    /// timezone, which is the whole reason `[schedule] timezone` exists.
    #[test]
    fn the_timezone_moves_the_window() {
        let mut kids = policy("kids", None);
        kids.assignments.push(AssignmentConfig {
            client: "192.168.1.50".to_string(),
            days: Some("daily".to_string()),
            start: Some("23:00".to_string()),
            end: Some("23:30".to_string()),
        });

        let utc = PolicySet::from_config("UTC", std::slice::from_ref(&kids)).unwrap();
        // 2026-08-01T20:10:00Z: 20:10 in UTC, and 23:10 in Bucharest — which in
        // August is EEST at UTC+3, not the +2 the standard-time name suggests.
        let instant = 1_785_615_000;
        assert_eq!(
            utc.resolve(ip("192.168.1.50"), None, instant),
            PolicyId::DEFAULT
        );

        let bucharest = PolicySet::from_config("EET-2EEST,M3.5.0/3,M10.5.0/4", &[kids]).unwrap();
        assert_eq!(
            bucharest.resolve(ip("192.168.1.50"), None, instant),
            bucharest.id_of("kids").unwrap()
        );
    }

    /// DST moves the window against UTC, which is the whole point of reading
    /// schedules in a zone rather than in the container's clock. The same
    /// 19:30 UTC is inside a 21:00–22:00 local window in February and outside
    /// it in August, because Bucharest is +2 then and +3 then.
    #[test]
    fn a_schedule_follows_the_dst_offset_not_utc() {
        let mut kids = policy("kids", None);
        kids.assignments.push(AssignmentConfig {
            client: "192.168.1.50".to_string(),
            days: Some("daily".to_string()),
            start: Some("21:00".to_string()),
            end: Some("22:00".to_string()),
        });
        let set = PolicySet::from_config("EET-2EEST,M3.5.0/3,M10.5.0/4", &[kids]).unwrap();
        let client = ip("192.168.1.50");
        let kids_id = set.id_of("kids").unwrap();

        // 2026-02-01T19:30:00Z — 21:30 in standard time.
        assert_eq!(set.resolve(client, None, 1_769_974_200), kids_id);
        // 2026-08-01T19:30:00Z — 22:30 in summer time, past the window.
        assert_eq!(set.resolve(client, None, 1_785_612_600), PolicyId::DEFAULT);
    }

    /// `days` on its own is a whole-day schedule — the "weekends only" case,
    /// which would otherwise need a 00:00–24:00 window spelled out.
    #[test]
    fn days_without_times_is_a_whole_day_window() {
        let mut weekend = policy("weekend", None);
        weekend.assignments.push(AssignmentConfig {
            client: "192.168.1.50".to_string(),
            days: Some("sat,sun".to_string()),
            start: None,
            end: None,
        });
        let set = PolicySet::from_config("UTC", &[weekend]).unwrap();
        let client = ip("192.168.1.50");
        assert_eq!(
            set.resolve(client, None, SATURDAY_NOON),
            set.id_of("weekend").unwrap()
        );
        assert_eq!(set.resolve(client, None, MONDAY_EVENING), PolicyId::DEFAULT);
    }

    #[test]
    fn a_name_selector_matches_the_client_name_case_insensitively() {
        let mut kids = policy("kids", None);
        kids.assignments.push(assign("Laptop"));
        let set = PolicySet::from_config("UTC", &[kids]).unwrap();
        assert_eq!(
            set.resolve(ip("10.0.0.9"), Some("laptop"), SATURDAY_NOON),
            set.id_of("kids").unwrap()
        );
        assert_eq!(
            set.resolve(ip("10.0.0.9"), Some("desktop"), SATURDAY_NOON),
            PolicyId::DEFAULT
        );
    }

    #[test]
    fn too_many_policies_is_an_error_not_a_truncation() {
        let policies: Vec<PolicyConfig> = (0..PolicyId::MAX)
            .map(|index| policy(&format!("p{index}"), None))
            .collect();
        assert!(matches!(
            PolicySet::from_config("UTC", &policies),
            Err(PolicyError::TooMany { .. })
        ));
    }

    #[test]
    fn a_bad_timezone_is_an_error() {
        assert!(matches!(
            PolicySet::from_config("not a timezone", &[]),
            Err(PolicyError::Timezone(_))
        ));
    }

    #[test]
    fn client_scopes_honour_positives_and_negations() {
        let scope = ClientScope::parse("192.168.1.50|10.0.0.0/8").unwrap();
        assert!(scope.admits(Some(ip("192.168.1.50")), None));
        assert!(scope.admits(Some(ip("10.1.2.3")), None));
        assert!(!scope.admits(Some(ip("192.168.1.51")), None));

        let except = ClientScope::parse("~192.168.1.50").unwrap();
        assert!(!except.admits(Some(ip("192.168.1.50")), None));
        assert!(except.admits(Some(ip("192.168.1.51")), None));
    }

    #[test]
    fn a_client_scope_that_parses_to_nothing_is_rejected() {
        assert!(ClientScope::parse("").is_none());
        assert!(ClientScope::parse("|||").is_none());
        // One malformed prefix poisons the whole payload: applying the rest
        // would apply the rule more widely than it was written.
        assert!(ClientScope::parse("192.168.1.0/99").is_none());
    }

    /// A rule scoped by name still applies to a client identified only by name.
    #[test]
    fn a_named_scope_matches_without_an_address() {
        let scope = ClientScope::parse("laptop").unwrap();
        assert!(scope.admits(None, Some("laptop")));
        assert!(!scope.admits(None, Some("desktop")));
        assert!(!scope.admits(None, None));
    }
}
