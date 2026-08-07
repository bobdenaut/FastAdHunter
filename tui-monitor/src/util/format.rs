//! Number and string rendering. Pure functions, testable without a terminal.

use std::borrow::Cow;

/// `part / whole` in percent, `0.0` rather than `NaN` on an empty denominator.
pub fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64) * 100.0
    }
}

pub fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// `1043886` → `1,043,886`, at any magnitude.
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Bytes at a human scale — `27.1 MB`, `1.2 GB`.
pub fn bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut scaled = value as f64;
    let mut unit = 0;
    while scaled >= 1024.0 && unit < UNITS.len() - 1 {
        scaled /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{scaled:.1} {}", UNITS[unit])
    }
}

/// At most `max` **characters**, with `…` when something was cut. Counting
/// characters rather than bytes is what keeps an internationalised domain from
/// panicking the panel that lists it.
///
/// Borrowed when nothing was cut, which is the common case: most domains and
/// every healthy status string already fit, and this runs per row per frame.
pub fn truncate(text: &str, max: usize) -> Cow<'_, str> {
    if text.chars().count() <= max {
        return Cow::Borrowed(text);
    }
    let keep = max.saturating_sub(1);
    Cow::Owned(
        text.chars()
            .take(keep)
            .chain(std::iter::once('…'))
            .collect(),
    )
}

/// RouterOS's uptime spelling, spaced for reading: `2d23h57m20s` becomes
/// `2d 23h 57m 20s`. The device's figure is passed through rather than parsed,
/// so this only breaks the run where a unit letter meets the next digit — which
/// also spaces the `3d04:12:55` spelling the same device uses elsewhere.
pub fn spaced_uptime(raw: &str) -> Cow<'_, str> {
    let breaks_here =
        |(unit, next): (char, char)| unit.is_ascii_alphabetic() && next.is_ascii_digit();
    let pairs = || raw.chars().zip(raw.chars().skip(1));

    if !pairs().any(breaks_here) {
        return Cow::Borrowed(raw);
    }

    let mut out = String::with_capacity(raw.len() + 4);
    for pair in pairs() {
        out.push(pair.0);
        if breaks_here(pair) {
            out.push(' ');
        }
    }
    // `zip` stops one short, so the final character is still owed.
    out.extend(raw.chars().last());
    Cow::Owned(out)
}

/// A duration in milliseconds, rendered at the precision it deserves: a DNS
/// answer served from cache is tens of microseconds.
pub fn millis(value_ms: f64) -> String {
    if value_ms >= 100.0 {
        format!("{value_ms:.1}")
    } else {
        format!("{value_ms:.3}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_groups_at_every_magnitude() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(42), "42");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(798_287), "798,287");
        assert_eq!(thousands(1_043_886), "1,043,886");
        assert_eq!(thousands(12_345_678_901), "12,345,678,901");
    }

    #[test]
    fn truncate_cuts_on_character_boundaries() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
    }

    /// A byte-indexed cut would land mid-character here and panic.
    #[test]
    fn truncate_survives_a_multibyte_domain() {
        assert_eq!(truncate("bücher.münchen.example", 8), "bücher.…");
        assert_eq!(truncate("日本語ドメイン.example", 4), "日本語…");
    }

    #[test]
    fn percent_of_nothing_is_zero_not_nan() {
        assert_eq!(percent(0, 0), 0.0);
        assert_eq!(percent(1, 4), 25.0);
    }

    /// RouterOS emits several spellings depending on version and magnitude, and
    /// this must not mangle any of them — it only inserts, never reorders.
    #[test]
    fn uptime_is_spaced_at_each_unit_boundary() {
        assert_eq!(spaced_uptime("2d23h57m20s"), "2d 23h 57m 20s");
        assert_eq!(spaced_uptime("1w2d3h4m5s"), "1w 2d 3h 4m 5s");
        assert_eq!(spaced_uptime("3d04:12:55"), "3d 04:12:55");
        assert_eq!(spaced_uptime("57m20s"), "57m 20s");
    }

    /// Nothing to break apart means nothing to allocate.
    #[test]
    fn an_uptime_with_one_unit_is_returned_untouched() {
        for raw in ["20s", "5m", "", "00:00:00"] {
            assert!(
                matches!(spaced_uptime(raw), std::borrow::Cow::Borrowed(_)),
                "{raw:?}"
            );
            assert_eq!(spaced_uptime(raw), raw);
        }
    }

    #[test]
    fn bytes_scale_to_the_largest_whole_unit() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(27_052_081), "25.8 MB");
    }
}
