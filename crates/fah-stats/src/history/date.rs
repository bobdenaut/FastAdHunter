//! Dependency-free calendar math for human-readable history filenames
//! (`YYYY-MM-DD`), shared by the rollup and perf writers. Howard Hinnant's
//! public-domain civil-date algorithm — no date crate pulled in for a filename.

/// `YYYY-MM-DD` for a day-epoch (days since 1970-01-01).
pub(crate) fn date_string(day_epoch: u64) -> String {
    let (y, m, d) = civil_from_days(day_epoch as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parses `YYYY-MM-DD` back to a day-epoch, rejecting malformed or
/// out-of-range dates.
pub(crate) fn parse_day(date: &str) -> Option<u64> {
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    days_from_civil(y, m, d).try_into().ok()
}

/// Days since 1970-01-01 → `(year, month, day)`. Howard Hinnant's
/// public-domain civil-date algorithm.
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Inverse of [`civil_from_days`]: `(year, month, day)` → days since epoch.
pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * u64::from(mp) + 2) / 5 + u64::from(d) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe as i64 - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_date_round_trips_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        // Leap day.
        assert_eq!(
            days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28),
            2
        );
        for day in 0..40_000i64 {
            let (y, m, d) = civil_from_days(day);
            assert_eq!(days_from_civil(y, m, d), day);
        }
    }

    #[test]
    fn date_string_and_parse_are_inverse() {
        for day in [0u64, 1, 18_262, 20_291, 25_000] {
            let s = date_string(day);
            assert_eq!(parse_day(&s), Some(day), "round-trip failed for {s}");
        }
        assert_eq!(date_string(0), "1970-01-01");
    }

    #[test]
    fn parse_day_rejects_malformed_names() {
        assert_eq!(parse_day("2025-13-01"), None); // month out of range
        assert_eq!(parse_day("2025-01"), None); // too few parts
        assert_eq!(parse_day("2025-01-01-01"), None); // too many parts
        assert_eq!(parse_day("garbage"), None);
    }
}
