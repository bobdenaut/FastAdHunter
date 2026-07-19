//! Resident set size, read fresh on every `/metrics` scrape rather than
//! cached — it changes independently of any query traffic, so there's no
//! event to update it on. `/proc` parsing needs no dependency (the container
//! target is always Linux — CONFIGURATION.md/ARCHITECTURE.md: distroless
//! static musl binary), so this is hand-rolled rather than pulling in
//! `sysinfo` for one gauge.

/// Current process RSS in bytes, or `0` if it can't be determined (a
/// non-Linux dev machine, or a malformed/unreadable `/proc/self/status`).
pub fn resident_memory_bytes() -> u64 {
    #[cfg(target_os = "linux")]
    {
        read_vm_rss_kb().map(|kb| kb * 1024).unwrap_or(0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

#[cfg(target_os = "linux")]
fn read_vm_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    parse_vm_rss_kb(&status)
}

#[cfg(target_os = "linux")]
fn parse_vm_rss_kb(status: &str) -> Option<u64> {
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn parses_the_vm_rss_line() {
        let status = "VmPeak:\t   12345 kB\nVmRSS:\t    6789 kB\nVmData:\t   111 kB\n";
        assert_eq!(parse_vm_rss_kb(status), Some(6789));
    }

    #[test]
    fn missing_line_yields_none() {
        assert_eq!(parse_vm_rss_kb("VmPeak:\t 1 kB\n"), None);
    }

    #[test]
    fn a_running_process_reports_nonzero_rss() {
        assert!(resident_memory_bytes() > 0);
    }
}
