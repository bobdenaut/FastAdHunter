//! Policy, Schedule and Assignment — the runtime shapes behind CONTEXT.md's
//! **Policy**: a named bundle of rule lists and settings assignable to clients
//! or schedules.
//!
//! Data only, like everything in this crate: which policy a client gets, and
//! whether a schedule is active right now, are decided in `fah-rules` — the one
//! crate both pipelines already share (CLAUDE.md hard rule 2).

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// Which policy a request is judged under. An index, not a name: it is the bit
/// position in the compiled ruleset's per-rule policy mask, so a lookup tests
/// membership with a shift and an `and` rather than a string compare.
///
/// [`PolicyId::DEFAULT`] is always present and always index 0 — a deployment
/// that defines no policies still has one, which is why zero-config users see
/// no behaviour change (CONFIGURATION.md §Policies).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PolicyId(pub u8);

impl PolicyId {
    /// Every enabled list, no overrides — what every client got before
    /// Policies existed, and what an unassigned client still gets.
    pub const DEFAULT: PolicyId = PolicyId(0);

    /// The ceiling on distinct policies, including the default.
    ///
    /// The compiled ruleset carries one `u16` mask per rule so that two
    /// policies can share one compiled corpus instead of each holding their
    /// own — at deployed scale (1.06 M rules) that mask array is ~2 MiB *flat*,
    /// against ~17 MiB for every additional unshared copy. 16 is what a `u16`
    /// buys, and a household reaches five or six (default, kids, guest, iot,
    /// work) long before it reaches this.
    pub const MAX: usize = 16;

    pub fn bit(self) -> u16 {
        1u16 << self.0
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl Default for PolicyId {
    fn default() -> Self {
        PolicyId::DEFAULT
    }
}

/// A named bundle of rule lists plus the settings that override the globals
/// while it is in force.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    /// Stable identifier, referenced by assignments and reported in events.
    pub id: String,
    /// Human label for the dashboard. Free text; never matched on.
    pub name: String,
    /// Ids of the rule lists this policy enables, naming `[[rules.lists]]`
    /// entries. A policy is a *subset* of the configured lists, never a source
    /// of new ones — otherwise "which lists exist" would have two owners
    /// (CONFIGURATION.md §Who owns the list set).
    pub lists: Vec<String>,
    /// Per-policy override of `[dns.blocking] mode`. **No consumer until
    /// p2-06** — enforcement is that task, and declaring the field here without
    /// saying so is how a setting that does nothing survives review.
    pub blocking_mode: Option<String>,
}

/// A recurring weekly window in the timezone from `[schedule] timezone`.
///
/// Deliberately not a date range: "school nights 21:00–07:00" is the shape
/// parental control needs, and an absolute range would need a calendar nothing
/// else in the system has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    /// Bitmask of the days the window **starts** on; bit 0 = Sunday, matching
    /// the POSIX weekday numbering `fah_config::PosixTz` reports.
    ///
    /// A window that wraps midnight belongs to the day it opened: `mon-fri
    /// 21:00–07:00` covers Saturday 00:00–07:00 because it started on Friday,
    /// and does *not* cover Monday 00:00–07:00.
    pub days: u8,
    /// Minutes since local midnight, inclusive.
    pub start: u16,
    /// Minutes since local midnight, exclusive. `end <= start` wraps midnight,
    /// and `end == start` therefore means a window a full day long — not an
    /// empty one.
    pub end: u16,
}

impl Schedule {
    pub const ALL_DAYS: u8 = 0b0111_1111;

    /// True when `(weekday, minute_of_day)` falls inside the window.
    ///
    /// Pure integer arithmetic over an already-localized time — the timezone
    /// was applied by the caller, so this is testable without a clock and
    /// without a timezone (the acceptance criterion in p2-05).
    pub fn contains(&self, weekday: u8, minute_of_day: u16) -> bool {
        let on = |day: u8| self.days & (1 << (day % 7)) != 0;
        if self.start < self.end {
            on(weekday) && minute_of_day >= self.start && minute_of_day < self.end
        } else {
            // Wrapped: either late on a day the window opens, or early on the
            // day after one.
            (on(weekday) && minute_of_day >= self.start)
                || (on((weekday + 6) % 7) && minute_of_day < self.end)
        }
    }
}

/// Which client a policy applies to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientSelector {
    /// One address.
    Ip(IpAddr),
    /// A prefix, so a whole subnet can share a policy.
    Network { address: IpAddr, prefix_len: u8 },
    /// The client's user-assigned name (CONTEXT.md §Client). Compared
    /// case-insensitively; a client that has not been named never matches.
    Name(String),
}

impl ClientSelector {
    /// Whether this selector names the given client.
    pub fn matches(&self, ip: IpAddr, name: Option<&str>) -> bool {
        match self {
            ClientSelector::Ip(wanted) => *wanted == ip,
            ClientSelector::Network {
                address,
                prefix_len,
            } => network_contains(*address, *prefix_len, ip),
            ClientSelector::Name(wanted) => {
                name.is_some_and(|name| name.eq_ignore_ascii_case(wanted))
            }
        }
    }

    /// How specific this selector is, for resolving a client matched by more
    /// than one assignment. A name is the most deliberate statement an operator
    /// can make, then a single address, then the longest prefix.
    pub fn specificity(&self) -> u32 {
        match self {
            ClientSelector::Name(_) => 1_000,
            ClientSelector::Ip(_) => 900,
            ClientSelector::Network { prefix_len, .. } => *prefix_len as u32,
        }
    }
}

/// True when `ip` falls inside `address/prefix_len`. Mixed families never
/// match — a v4 client is not in a v6 prefix, however the bits line up.
fn network_contains(address: IpAddr, prefix_len: u8, ip: IpAddr) -> bool {
    fn covered(network: &[u8], candidate: &[u8], prefix_len: u8) -> bool {
        if prefix_len as usize > network.len() * 8 {
            return false;
        }
        let whole = (prefix_len / 8) as usize;
        if network[..whole] != candidate[..whole] {
            return false;
        }
        let bits = prefix_len % 8;
        if bits == 0 {
            return true;
        }
        let mask = 0xffu8 << (8 - bits);
        network[whole] & mask == candidate[whole] & mask
    }

    match (address, ip) {
        (IpAddr::V4(network), IpAddr::V4(candidate)) => {
            covered(&network.octets(), &candidate.octets(), prefix_len)
        }
        (IpAddr::V6(network), IpAddr::V6(candidate)) => {
            covered(&network.octets(), &candidate.octets(), prefix_len)
        }
        _ => false,
    }
}

/// Binds a client to a policy, optionally only while a schedule is active.
///
/// An assignment whose schedule is not active simply does not apply; which
/// policy the client then gets is `fah_rules::PolicySet::resolve`'s business —
/// it keeps looking, so a scheduled per-device override cannot exempt that
/// device from a broader assignment during the hours it is not in force.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub client: ClientSelector,
    /// The [`Policy::id`] this assignment selects.
    pub policy: String,
    pub schedule: Option<Schedule>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: u8 = 1;
    const TUE: u8 = 2;
    const FRI: u8 = 5;
    const SAT: u8 = 6;
    const SUN: u8 = 0;

    fn school_nights() -> Schedule {
        Schedule {
            // Monday through Friday.
            days: 0b0011_1110,
            start: 21 * 60,
            end: 7 * 60,
        }
    }

    #[test]
    fn a_wrapping_window_belongs_to_the_day_it_opened() {
        let schedule = school_nights();
        assert!(schedule.contains(FRI, 22 * 60));
        // Saturday morning is covered because Friday opened the window.
        assert!(schedule.contains(SAT, 3 * 60));
        // Saturday night is not: the window does not open on Saturday.
        assert!(!schedule.contains(SAT, 22 * 60));
        // Monday morning is not: Sunday never opened it.
        assert!(!schedule.contains(MON, 3 * 60));
    }

    #[test]
    fn window_bounds_are_start_inclusive_and_end_exclusive() {
        let schedule = school_nights();
        assert!(!schedule.contains(MON, 20 * 60 + 59));
        assert!(schedule.contains(MON, 21 * 60));
        assert!(schedule.contains(SAT, 6 * 60 + 59));
        assert!(!schedule.contains(SAT, 7 * 60));
    }

    #[test]
    fn a_same_day_window_does_not_wrap() {
        let schedule = Schedule {
            days: Schedule::ALL_DAYS,
            start: 9 * 60,
            end: 17 * 60,
        };
        assert!(schedule.contains(SUN, 12 * 60));
        assert!(!schedule.contains(SUN, 8 * 60));
        assert!(!schedule.contains(SUN, 17 * 60));
    }

    /// `start == end` is a window a full day long, not an empty one: it opens
    /// at 09:00 and closes at 09:00 the next day, so with every day selected it
    /// covers everything. Pinned because the alternative reading — "empty" —
    /// is the one a reader expects, and the wrap arithmetic quietly gives the
    /// other.
    #[test]
    fn start_equal_to_end_is_a_full_day_window() {
        let schedule = Schedule {
            days: Schedule::ALL_DAYS,
            start: 9 * 60,
            end: 9 * 60,
        };
        assert!(schedule.contains(SUN, 9 * 60));
        assert!(schedule.contains(SUN, 23 * 60));
        // 08:59 on Sunday is still inside the window Saturday opened.
        assert!(schedule.contains(SUN, 9 * 60 - 1));

        // With only one day selected, the 24 hours are the ones that day opens.
        let monday_only = Schedule {
            days: 1 << MON,
            ..schedule
        };
        assert!(monday_only.contains(MON, 9 * 60));
        // Tuesday morning is the tail of Monday's window.
        assert!(monday_only.contains(TUE, 8 * 60));
        assert!(!monday_only.contains(TUE, 10 * 60));
        assert!(!monday_only.contains(SUN, 12 * 60));
    }

    #[test]
    fn selectors_match_addresses_names_and_prefixes() {
        let ip: IpAddr = "192.168.1.50".parse().unwrap();
        assert!(ClientSelector::Ip(ip).matches(ip, None));
        assert!(!ClientSelector::Ip(ip).matches("192.168.1.51".parse().unwrap(), None));

        let net = ClientSelector::Network {
            address: "192.168.1.0".parse().unwrap(),
            prefix_len: 24,
        };
        assert!(net.matches(ip, None));
        assert!(!net.matches("192.168.2.50".parse().unwrap(), None));

        let named = ClientSelector::Name("Laptop".to_string());
        assert!(named.matches(ip, Some("laptop")));
        assert!(!named.matches(ip, None));
    }

    /// A v4 address is not inside a v6 prefix even when the leading bytes
    /// would compare equal.
    #[test]
    fn a_prefix_never_matches_across_families() {
        let net = ClientSelector::Network {
            address: "::".parse().unwrap(),
            prefix_len: 0,
        };
        assert!(!net.matches("0.0.0.0".parse().unwrap(), None));
        assert!(net.matches("::1".parse().unwrap(), None));
    }

    #[test]
    fn a_prefix_longer_than_the_family_matches_nothing() {
        let net = ClientSelector::Network {
            address: "192.168.1.0".parse().unwrap(),
            prefix_len: 40,
        };
        assert!(!net.matches("192.168.1.50".parse().unwrap(), None));
    }

    #[test]
    fn a_name_outranks_an_address_which_outranks_a_prefix() {
        let name = ClientSelector::Name("laptop".to_string()).specificity();
        let ip = ClientSelector::Ip("192.168.1.50".parse().unwrap()).specificity();
        let host_route = ClientSelector::Network {
            address: "192.168.1.50".parse().unwrap(),
            prefix_len: 32,
        }
        .specificity();
        let subnet = ClientSelector::Network {
            address: "192.168.1.0".parse().unwrap(),
            prefix_len: 24,
        }
        .specificity();
        assert!(name > ip && ip > host_route && host_route > subnet);
    }

    #[test]
    fn the_default_policy_is_index_zero() {
        assert_eq!(PolicyId::default(), PolicyId::DEFAULT);
        assert_eq!(PolicyId::DEFAULT.bit(), 1);
        assert_eq!(PolicyId(3).bit(), 8);
    }
}
