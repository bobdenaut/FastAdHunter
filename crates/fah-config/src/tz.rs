//! POSIX TZ strings — the timezone a Schedule's wall-clock times are read in
//! (CONFIGURATION.md `[schedule] timezone`).
//!
//! # Why this and not a timezone database
//!
//! A schedule says "school nights 21:00–07:00". That is *local* time, and the
//! container has no local time to read: the image is distroless and ships no
//! `/usr/share/zoneinfo`, so every clock in the process is UTC. Something has
//! to state the offset, and it has to state DST too — a fixed offset silently
//! fires a 21:00 schedule at 20:00 for half the year.
//!
//! A POSIX TZ string states both in ~40 bytes: `EET-2EEST,M3.5.0/3,M10.5.0/4`
//! is "2 h east of UTC, DST from the last Sunday in March at 03:00 to the last
//! Sunday in October at 04:00". It is what RouterOS and most embedded gear
//! already speak, it needs no bundled IANA database (~1 MB of static data in an
//! image that currently has none) and no new dependency against the fixed tech
//! stack (CLAUDE.md §Environment notes).
//!
//! What it gives up: zones whose rules are not a recurring `nth weekday of
//! month` pattern, and historical transitions. Neither matters for "is this
//! client's schedule active *now*", which is the only question asked of it.
//!
//! # Grammar accepted
//!
//! ```text
//! std offset [ dst [offset] , start[/time] , end[/time] ]
//! ```
//!
//! - `std` / `dst` — 3+ letters (`EET`), or bracket-quoted (`<+0530>`). The
//!   names are parsed and discarded: nothing in FastAdHunter displays them.
//! - `offset` — `[+|-]hh[:mm[:ss]]`, **POSIX-signed**, i.e. positive is *west*
//!   of Greenwich. `EET-2` is UTC+2. This inversion is the single most common
//!   way to write one of these strings backwards, so [`PosixTz::parse`] keeps
//!   the sign convention at the boundary and stores seconds *east* of UTC.
//! - `start` / `end` — `Mm.w.d`: month `m` 1–12, week `w` 1–5 (5 = last, even
//!   in a month with four), weekday `d` 0–6 with 0 = Sunday. `/time` defaults
//!   to 02:00:00 as POSIX requires.
//!
//! The `Jn` and `n` (Julian day) transition forms are **rejected** rather than
//! misread. No zone in current use needs them, and accepting a form we cannot
//! evaluate would move a schedule by a day without saying so.

use std::fmt;

/// A parsed POSIX TZ string. Immutable, `Copy`-cheap, and evaluated with pure
/// integer arithmetic — [`PosixTz::local`] takes no locks and allocates
/// nothing, because a per-query schedule check runs on the same path a verdict
/// does (PERFORMANCE.md §Hot path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosixTz {
    /// Seconds **east** of UTC in standard time, already sign-corrected from
    /// POSIX's west-positive convention.
    std_offset: i32,
    dst: Option<Dst>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Dst {
    /// Seconds east of UTC while DST is in effect. Defaults to standard + 1 h.
    offset: i32,
    start: Transition,
    end: Transition,
}

/// One `Mm.w.d/time` rule: "the `week`-th `weekday` of `month`, at `seconds`
/// past local midnight".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Transition {
    month: u8,
    week: u8,
    weekday: u8,
    seconds: i32,
}

/// Local wall-clock time, reduced to exactly what a Schedule compares against.
/// No date: a schedule is a weekday plus a time of day, and carrying a full
/// civil date would invite comparisons the model does not support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    /// 0 = Sunday … 6 = Saturday, matching the POSIX `d` field so the schedule
    /// bitmask and the transition rule cannot disagree about what day 0 is.
    pub weekday: u8,
    /// Minutes since local midnight, 0–1439.
    pub minute_of_day: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TzError {
    message: String,
}

impl fmt::Display for TzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TzError {}

fn err(message: impl Into<String>) -> TzError {
    TzError {
        message: message.into(),
    }
}

impl PosixTz {
    /// UTC — the default, and what an operator who never sets a timezone gets.
    pub const UTC: PosixTz = PosixTz {
        std_offset: 0,
        dst: None,
    };

    /// Parses a POSIX TZ string. `"UTC"` and any other DST-less form yield a
    /// zone with a fixed offset; a malformed one is rejected here, at config
    /// load, rather than on the evening months later when a schedule first
    /// consults it.
    pub fn parse(spec: &str) -> Result<PosixTz, TzError> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err(err("timezone must not be empty (use \"UTC\")"));
        }

        let mut rest = read_name(spec)?;
        // POSIX requires an offset after the standard name, but `TZ=UTC` is
        // universally spelled without one and is this project's default. A
        // bare name is read as offset zero rather than rejected.
        let std_offset = if rest.is_empty() {
            0
        } else {
            let (offset, after) = read_offset(rest)?;
            rest = after;
            offset
        };

        if rest.is_empty() {
            return Ok(PosixTz {
                std_offset,
                dst: None,
            });
        }

        // A DST name may be followed by its own offset; when it is absent POSIX
        // defines it as one hour ahead of standard time.
        rest = read_name(rest)?;
        let dst_offset = if rest.starts_with(',') {
            std_offset + 3600
        } else {
            let (offset, after) = read_offset(rest)?;
            rest = after;
            offset
        };

        let Some(rules) = rest.strip_prefix(',') else {
            return Err(err(format!(
                "a DST name needs its transition rules: expected \",start,end\" after it, found {rest:?}"
            )));
        };
        let Some((start, end)) = rules.split_once(',') else {
            return Err(err(
                "DST needs two transition rules separated by a comma, e.g. \",M3.5.0/3,M10.5.0/4\"",
            ));
        };

        Ok(PosixTz {
            std_offset,
            dst: Some(Dst {
                offset: dst_offset,
                start: parse_transition(start)?,
                end: parse_transition(end)?,
            }),
        })
    }

    /// Seconds east of UTC in effect at `unix_seconds`.
    pub fn offset_at(&self, unix_seconds: i64) -> i32 {
        let Some(dst) = self.dst else {
            return self.std_offset;
        };

        // The year is taken in standard local time. A transition can only be
        // misattributed within an hour of New Year, and no DST rule in the
        // grammar lands there (the `M` form names a month and a weekday).
        let year = civil_from_days(div_floor(unix_seconds + self.std_offset as i64, 86_400)).0;

        // Each rule's `/time` is local time *in the offset in force just before
        // that transition*: standard time entering DST, DST time leaving it.
        // Reading both in standard time shifts the autumn switch by an hour.
        let start = transition_instant(dst.start, year, self.std_offset);
        let end = transition_instant(dst.end, year, dst.offset);

        let in_dst = if start <= end {
            // Northern hemisphere: DST is the interval inside the year.
            unix_seconds >= start && unix_seconds < end
        } else {
            // Southern hemisphere: DST wraps the year boundary.
            unix_seconds >= start || unix_seconds < end
        };

        if in_dst {
            dst.offset
        } else {
            self.std_offset
        }
    }

    /// Local weekday and time of day at `unix_seconds`.
    pub fn local(&self, unix_seconds: i64) -> LocalTime {
        let local = unix_seconds + self.offset_at(unix_seconds) as i64;
        let days = div_floor(local, 86_400);
        let seconds_of_day = local - days * 86_400;
        LocalTime {
            // 1970-01-01 was a Thursday, which is 4 with Sunday as 0.
            weekday: (days + 4).rem_euclid(7) as u8,
            minute_of_day: (seconds_of_day / 60) as u16,
        }
    }
}

impl Default for PosixTz {
    fn default() -> Self {
        PosixTz::UTC
    }
}

/// Skips a zone name — either 3+ letters, or anything bracket-quoted — and
/// returns what follows. The name itself is never used.
fn read_name(spec: &str) -> Result<&str, TzError> {
    if let Some(rest) = spec.strip_prefix('<') {
        let Some(close) = rest.find('>') else {
            return Err(err("unterminated <...> zone name"));
        };
        if close == 0 {
            return Err(err("empty <> zone name"));
        }
        return Ok(&rest[close + 1..]);
    }

    let len = spec
        .bytes()
        .take_while(|byte| byte.is_ascii_alphabetic())
        .count();
    if len < 3 {
        return Err(err(format!(
            "zone name must be 3+ letters or <quoted>, found {:?}",
            &spec[..spec.len().min(8)]
        )));
    }
    Ok(&spec[len..])
}

/// Reads `[+|-]hh[:mm[:ss]]` and returns **seconds east of UTC**, inverting
/// POSIX's west-positive sign.
fn read_offset(spec: &str) -> Result<(i32, &str), TzError> {
    let (negated, rest) = match spec.as_bytes().first() {
        Some(b'-') => (true, &spec[1..]),
        Some(b'+') => (false, &spec[1..]),
        _ => (false, spec),
    };

    let mut parts = [0i32; 3];
    let mut rest = rest;
    for (index, part) in parts.iter_mut().enumerate() {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            if index == 0 {
                return Err(err(format!(
                    "expected an offset like \"-2\" or \"+05:30\", found {:?}",
                    &rest[..rest.len().min(8)]
                )));
            }
            break;
        }
        *part = rest[..digits].parse::<i32>().map_err(|_| {
            err(format!(
                "offset field {:?} does not fit in a timezone offset",
                &rest[..digits]
            ))
        })?;
        rest = &rest[digits..];
        match rest.strip_prefix(':') {
            Some(after) if index < 2 => rest = after,
            _ => break,
        }
    }

    let [hours, minutes, seconds] = parts;
    if hours > 24 || minutes > 59 || seconds > 59 {
        return Err(err(format!(
            "offset {hours}:{minutes}:{seconds} is out of range (max 24:59:59)"
        )));
    }

    let magnitude = hours * 3600 + minutes * 60 + seconds;
    // POSIX counts west as positive; everything downstream counts east.
    Ok((if negated { magnitude } else { -magnitude }, rest))
}

fn parse_transition(spec: &str) -> Result<Transition, TzError> {
    let (date, time) = match spec.split_once('/') {
        Some((date, time)) => (date, Some(time)),
        None => (spec, None),
    };

    let Some(fields) = date.strip_prefix('M') else {
        return Err(err(format!(
            "transition {date:?} is not supported: use the Mmonth.week.day form, e.g. \"M3.5.0\" \
             (the Julian-day forms Jn and n are rejected rather than misread)"
        )));
    };

    let mut parts = fields.split('.');
    let mut next = |what: &str| -> Result<u8, TzError> {
        parts
            .next()
            .and_then(|value| value.parse::<u8>().ok())
            .ok_or_else(|| err(format!("transition {date:?} has no {what}")))
    };
    let month = next("month")?;
    let week = next("week")?;
    let weekday = next("weekday")?;
    if parts.next().is_some() {
        return Err(err(format!("transition {date:?} has trailing fields")));
    }
    if !(1..=12).contains(&month) || !(1..=5).contains(&week) || weekday > 6 {
        return Err(err(format!(
            "transition {date:?} is out of range: month 1-12, week 1-5, weekday 0-6 (0 = Sunday)"
        )));
    }

    // POSIX: the transition happens at 02:00:00 local unless stated. The time
    // may exceed 24 h or be negative in the wider POSIX grammar; the schedule
    // model has no use for that, so it is bounded to a real time of day.
    let seconds = match time {
        None => 2 * 3600,
        Some(time) => {
            let (offset, rest) = read_offset(time)?;
            if !rest.is_empty() {
                return Err(err(format!("trailing {rest:?} after transition time")));
            }
            // `read_offset` inverts the sign for zone offsets; a transition
            // time is a plain clock reading, so invert it back.
            -offset
        }
    };
    if !(0..86_400).contains(&seconds) {
        return Err(err(format!(
            "transition time in {spec:?} must be within a single day"
        )));
    }

    Ok(Transition {
        month,
        week,
        weekday,
        seconds,
    })
}

/// The UTC instant of `rule` in `year`, given the offset in force immediately
/// before it.
fn transition_instant(rule: Transition, year: i32, offset_before: i32) -> i64 {
    let first = days_from_civil(year, rule.month, 1);
    // 1970-01-01 was a Thursday (4 with Sunday as 0).
    let first_weekday = (first + 4).rem_euclid(7) as u8;
    let to_first_match = (rule.weekday + 7 - first_weekday) % 7;

    let mut day = to_first_match as i64 + (rule.week as i64 - 1) * 7;
    // Week 5 means "the last one", which is only the fifth when the month has
    // one; otherwise fall back a week until the date is still in the month.
    let month_len = days_in_month(year, rule.month) as i64;
    while day >= month_len {
        day -= 7;
    }

    (first + day) * 86_400 + rule.seconds as i64 - offset_before as i64
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u8) -> u8 {
    const LENGTHS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if month == 2 && is_leap(year) {
        29
    } else {
        LENGTHS[month as usize - 1]
    }
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's `days_from_civil`,
/// exact for the whole `i32` year range and branch-free apart from the leap
/// adjustment).
fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = if month <= 2 { year - 1 } else { year } as i64;
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = month as i64;
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let m = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * m + 2) / 5 + 1) as u8;
    let month = if m < 10 { m + 3 } else { m - 9 } as u8;
    (
        (if month <= 2 { year + 1 } else { year }) as i32,
        month,
        day,
    )
}

/// `i64::div_floor` is unstable; local times before 1970 and the seconds-of-day
/// arithmetic both need floor semantics, not truncation toward zero.
fn div_floor(value: i64, divisor: i64) -> i64 {
    let quotient = value / divisor;
    if value % divisor != 0 && (value < 0) != (divisor < 0) {
        quotient - 1
    } else {
        quotient
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `EET-2EEST,M3.5.0/3,M10.5.0/4` — Bucharest, and the string this module
    /// was written against.
    const BUCHAREST: &str = "EET-2EEST,M3.5.0/3,M10.5.0/4";

    fn at(tz: &PosixTz, unix: i64) -> (u8, u16) {
        let local = tz.local(unix);
        (local.weekday, local.minute_of_day)
    }

    /// 2026-08-01T12:00:00Z, the day this was written.
    const AUG_1_2026_NOON_UTC: i64 = 1_785_585_600;

    #[test]
    fn utc_is_the_default_and_parses() {
        assert_eq!(PosixTz::default(), PosixTz::UTC);
        assert_eq!(PosixTz::parse("UTC").unwrap(), PosixTz::UTC);
        assert_eq!(PosixTz::parse("GMT0").unwrap(), PosixTz::UTC);
        // A bare name is offset zero. `EET` alone really means +2, but the
        // rule that would reject it also rejects `UTC`, and a zone written
        // without its offset has not stated one.
        assert_eq!(PosixTz::parse("EET").unwrap(), PosixTz::UTC);
        assert_eq!(PosixTz::UTC.offset_at(AUG_1_2026_NOON_UTC), 0);
    }

    /// The sign inversion is the most likely way to get one of these wrong, so
    /// it gets its own test: `EET-2` is *east* of Greenwich.
    #[test]
    fn posix_offsets_are_west_positive() {
        assert_eq!(PosixTz::parse("EET-2").unwrap().offset_at(0), 2 * 3600);
        assert_eq!(PosixTz::parse("EST5").unwrap().offset_at(0), -5 * 3600);
        assert_eq!(PosixTz::parse("<+0530>-5:30").unwrap().offset_at(0), 19_800);
    }

    #[test]
    fn a_dst_offset_defaults_to_one_hour_ahead() {
        let tz = PosixTz::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        // Mid-July is inside DST for any northern rule.
        let july = 1_784_000_000; // 2026-07-13T...Z
        assert_eq!(tz.offset_at(july), 2 * 3600);
    }

    /// Spring forward: 2026-03-29, last Sunday in March, 03:00 local standard
    /// (01:00 UTC). The wall clock jumps 03:00 -> 04:00.
    #[test]
    fn spring_forward_skips_an_hour_of_wall_clock() {
        let tz = PosixTz::parse(BUCHAREST).unwrap();
        let transition = 1_774_746_000; // 2026-03-29T01:00:00Z

        assert_eq!(tz.offset_at(transition - 1), 2 * 3600);
        assert_eq!(tz.offset_at(transition), 3 * 3600);
        // One second before: Sunday 02:59. One second after: Sunday 04:00.
        assert_eq!(at(&tz, transition - 1), (0, 2 * 60 + 59));
        assert_eq!(at(&tz, transition), (0, 4 * 60));
    }

    /// Fall back: 2026-10-25, last Sunday in October, 04:00 local *DST*
    /// (01:00 UTC). Reading the rule in standard time instead would move this
    /// by an hour, which is the bug this test exists to catch.
    #[test]
    fn fall_back_repeats_an_hour_of_wall_clock() {
        let tz = PosixTz::parse(BUCHAREST).unwrap();
        let transition = 1_792_890_000; // 2026-10-25T01:00:00Z

        assert_eq!(tz.offset_at(transition - 1), 3 * 3600);
        assert_eq!(tz.offset_at(transition), 2 * 3600);
        // 03:59 DST, then 03:00 standard — the same wall clock hour twice.
        assert_eq!(at(&tz, transition - 1), (0, 3 * 60 + 59));
        assert_eq!(at(&tz, transition), (0, 3 * 60));
    }

    /// A southern-hemisphere zone's DST wraps the year boundary, so the
    /// "inside the interval" test has to invert.
    #[test]
    fn southern_hemisphere_dst_wraps_the_year() {
        // New Zealand: DST from the last Sunday in September to the first
        // Sunday in April.
        let tz = PosixTz::parse("NZST-12NZDT,M9.5.0,M4.1.0/3").unwrap();
        let january = 1_767_225_600; // 2026-01-01T00:00:00Z — inside DST
        let july = 1_784_000_000; // 2026-07-13 — outside it
        assert_eq!(tz.offset_at(january), 13 * 3600);
        assert_eq!(tz.offset_at(july), 12 * 3600);
    }

    /// Week 5 means "the last", including in a month whose weekday occurs only
    /// four times.
    #[test]
    fn week_five_means_the_last_occurrence() {
        // November 2026 has four Sundays (1, 8, 15, 22, 29 — five, in fact),
        // so use February 2026: Sundays fall on 1, 8, 15, 22 only.
        let tz = PosixTz::parse("XXX0YYY,M2.5.0,M11.1.0").unwrap();
        // 2026-02-22T02:00:00Z is the last Sunday of February at 02:00.
        let last_sunday_february = 1_771_725_600;
        assert_eq!(tz.offset_at(last_sunday_february - 1), 0);
        assert_eq!(tz.offset_at(last_sunday_february), 3600);
    }

    #[test]
    fn weekday_and_minute_of_day_are_local() {
        let tz = PosixTz::parse(BUCHAREST).unwrap();
        // 2026-08-01 is a Saturday; 12:00 UTC is 15:00 in Bucharest DST.
        assert_eq!(at(&tz, AUG_1_2026_NOON_UTC), (6, 15 * 60));
    }

    #[test]
    fn civil_conversions_round_trip_across_leap_years() {
        for (year, month, day) in [
            (1970, 1, 1),
            (2000, 2, 29),
            (2026, 8, 1),
            (2100, 3, 1),
            (1969, 12, 31),
        ] {
            let days = days_from_civil(year, month, day);
            assert_eq!(civil_from_days(days), (year, month, day));
        }
    }

    #[test]
    fn malformed_strings_are_rejected_at_parse() {
        for spec in [
            "",
            "ET-2",                      // name too short
            "EET-2EEST",                 // DST without transitions
            "EET-2EEST,M3.5.0",          // only one transition
            "EET-2EEST,J89,J300",        // Julian form, deliberately rejected
            "EET-2EEST,M13.5.0,M10.5.0", // month out of range
            "EET-2EEST,M3.6.0,M10.5.0",  // week out of range
            "EET-2EEST,M3.5.7,M10.5.0",  // weekday out of range
            "EET-99",                    // offset out of range
            "<+0530-5:30",               // unterminated quote
        ] {
            assert!(
                PosixTz::parse(spec).is_err(),
                "{spec:?} should not have parsed"
            );
        }
    }
}
