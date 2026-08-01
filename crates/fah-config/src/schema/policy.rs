//! `[schedule]` and `[[policies]]` (CONFIGURATION.md §Policies).
//!
//! Mirrored rather than reused from `fah_model::Policy`: sibling L1 crates
//! never import each other (CLAUDE.md hard rule 1), which is the same reason
//! [`super::EngineMode`] is a local copy of `fah_model::OperatingMode`.
//! `fah-rules` sits above both and converts.
//!
//! The two sections are separate because `[[policies]]` is an array of tables
//! and cannot also hold a scalar. `timezone` belongs to schedules anyway — it
//! is what turns "21:00" into an instant, and nothing else in the config reads
//! a clock.

use serde::{Deserialize, Serialize};

/// `[schedule]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ScheduleConfig {
    /// POSIX TZ string; see [`crate::PosixTz`] for the grammar and for why
    /// this is not an IANA name. Defaults to UTC, which is also the only clock
    /// the distroless image has.
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            timezone: default_timezone(),
        }
    }
}

fn default_timezone() -> String {
    "UTC".to_string()
}

/// One `[[policies]]` entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    pub id: String,
    /// Human label; defaults to the id so a minimal entry is two lines.
    #[serde(default)]
    pub name: Option<String>,
    /// Ids of `[[rules.lists]]` entries this policy enables. Absent means
    /// **every enabled list** — the same corpus the default policy uses, which
    /// makes a policy that only overrides a setting or a schedule expressible
    /// without restating the list set.
    #[serde(default)]
    pub lists: Option<Vec<String>>,
    /// Per-policy override of `[dns.blocking] mode`. **No consumer until
    /// p2-06.**
    #[serde(default)]
    pub blocking_mode: Option<String>,
    /// Clients this policy applies to, nested inside the policy they select
    /// rather than in a separate top-level array — an assignment has no
    /// meaning apart from its policy, and nesting removes the dangling
    /// `policy = "..."` back-reference that a flat array would need.
    #[serde(default)]
    pub assignments: Vec<AssignmentConfig>,
}

/// One `[[policies.assignments]]` entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentConfig {
    /// `192.168.1.50`, `192.168.1.0/24`, or a client name (CONTEXT.md
    /// §Client). An address or prefix is recognized by parsing; anything else
    /// is a name.
    pub client: String,
    /// `mon-fri`, `sat,sun`, `daily`, or a single day. Absent means every day.
    #[serde(default)]
    pub days: Option<String>,
    /// `HH:MM` local time. `start` and `end` are set together or not at all;
    /// absent means the assignment is always in force.
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
}

/// Parses `days` into a weekday bitmask, bit 0 = Sunday — the numbering
/// [`crate::PosixTz`] reports, so the mask and the clock cannot disagree about
/// which day is which.
///
/// Accepts `daily`, a comma-separated list (`sat,sun`), and inclusive ranges
/// (`mon-fri`, and `fri-mon` wrapping through the weekend). Lives here, beside
/// the schema it validates, so `Config::validate` rejects a typo while the
/// operator is editing rather than on the evening the schedule first matters.
pub fn parse_days(spec: &str) -> Result<u8, String> {
    const NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

    let day = |name: &str| -> Result<u8, String> {
        let name = name.trim().to_ascii_lowercase();
        // Accept the long forms too: an operator writing "monday" is not
        // making a mistake worth an error message.
        NAMES
            .iter()
            .position(|short| name.starts_with(short) && name.len() <= 9)
            .map(|index| index as u8)
            .ok_or_else(|| format!("unknown day {name:?} (use sun..sat, or \"daily\")"))
    };

    let spec = spec.trim();
    if spec.eq_ignore_ascii_case("daily") || spec.eq_ignore_ascii_case("all") {
        return Ok(fah_schedule_all_days());
    }

    let mut mask = 0u8;
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err("empty entry in days".to_string());
        }
        match part.split_once('-') {
            Some((from, to)) => {
                let (from, to) = (day(from)?, day(to)?);
                // Inclusive, and wrapping: "fri-mon" is Fri, Sat, Sun, Mon.
                let span = (to + 7 - from) % 7;
                for step in 0..=span {
                    mask |= 1 << ((from + step) % 7);
                }
            }
            None => mask |= 1 << day(part)?,
        }
    }
    Ok(mask)
}

/// Every day — the same constant as `fah_model::Schedule::ALL_DAYS`, which
/// this crate cannot name (sibling L1).
fn fah_schedule_all_days() -> u8 {
    0b0111_1111
}

/// Parses `HH:MM` into minutes since local midnight. `24:00` is accepted as an
/// end bound meaning "midnight", which is otherwise unwritable: `00:00` would
/// wrap the window to the previous day.
pub fn parse_time_of_day(spec: &str) -> Result<u16, String> {
    let spec = spec.trim();
    let Some((hours, minutes)) = spec.split_once(':') else {
        return Err(format!("time {spec:?} must be HH:MM"));
    };
    let hours: u16 = hours
        .trim()
        .parse()
        .map_err(|_| format!("time {spec:?} has a non-numeric hour"))?;
    let minutes: u16 = minutes
        .trim()
        .parse()
        .map_err(|_| format!("time {spec:?} has a non-numeric minute"))?;
    if minutes > 59 || hours > 24 || (hours == 24 && minutes > 0) {
        return Err(format!("time {spec:?} is not a time of day (00:00-24:00)"));
    }
    Ok((hours * 60 + minutes) % 1440)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_specs_cover_lists_ranges_and_wraps() {
        assert_eq!(parse_days("daily").unwrap(), 0b0111_1111);
        assert_eq!(parse_days("sun").unwrap(), 0b0000_0001);
        assert_eq!(parse_days("mon-fri").unwrap(), 0b0011_1110);
        assert_eq!(parse_days("sat,sun").unwrap(), 0b0100_0001);
        // Wrapping range: Fri, Sat, Sun, Mon.
        assert_eq!(parse_days("fri-mon").unwrap(), 0b0110_0011);
        assert_eq!(parse_days("Monday, Wednesday").unwrap(), 0b0000_1010);
        assert!(parse_days("funday").is_err());
        assert!(parse_days("mon,,fri").is_err());
    }

    #[test]
    fn times_parse_and_reject_nonsense() {
        assert_eq!(parse_time_of_day("21:00").unwrap(), 21 * 60);
        assert_eq!(parse_time_of_day("07:30").unwrap(), 7 * 60 + 30);
        // 24:00 is the only way to write "the end of the day".
        assert_eq!(parse_time_of_day("24:00").unwrap(), 0);
        for bad in ["2100", "25:00", "21:60", "21:xx", "24:01"] {
            assert!(
                parse_time_of_day(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn a_minimal_policy_needs_only_an_id() {
        let policy: PolicyConfig = toml::from_str("id = \"kids\"").unwrap();
        assert_eq!(policy.id, "kids");
        assert!(policy.name.is_none());
        assert!(policy.lists.is_none());
        assert!(policy.assignments.is_empty());
    }

    #[test]
    fn the_timezone_defaults_to_utc() {
        assert_eq!(ScheduleConfig::default().timezone, "UTC");
    }

    #[test]
    fn a_typo_in_an_assignment_is_rejected_rather_than_ignored() {
        let toml = "client = \"192.168.1.50\"\nstartt = \"21:00\"\n";
        assert!(toml::from_str::<AssignmentConfig>(toml).is_err());
    }
}
