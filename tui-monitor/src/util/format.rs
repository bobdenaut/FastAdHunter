//! Number and string rendering. Pure functions, testable without a terminal.

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
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(1);
    text.chars()
        .take(keep)
        .chain(std::iter::once('…'))
        .collect()
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

    #[test]
    fn bytes_scale_to_the_largest_whole_unit() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(27_052_081), "25.8 MB");
    }
}
