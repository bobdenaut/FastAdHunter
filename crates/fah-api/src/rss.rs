//! Process resident-set size for `GET /api/v1/debug/memory`.
//!
//! Linux-only by reading `/proc/self/status` — the deployment target is a
//! Linux container (ARCHITECTURE.md §Docker), and RSS is the figure the
//! PERFORMANCE.md memory budget is stated against. Anywhere else there is no
//! procfs, and the field is served as `null` rather than a guess.

/// Current RSS in bytes, or `None` where it cannot be read.
#[cfg(target_os = "linux")]
pub fn process_rss() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    parse_vmrss(&status)
}

#[cfg(not(target_os = "linux"))]
pub fn process_rss() -> Option<u64> {
    None
}

/// `VmRSS:    48588 kB` → bytes. Kept separate from the file read so the
/// parsing is testable on every platform.
#[allow(dead_code, reason = "only called on the Linux deployment target")]
fn parse_vmrss(status: &str) -> Option<u64> {
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vmrss_line_parses_to_bytes() {
        let status = "VmPeak:\t   50000 kB\nVmRSS:\t   48588 kB\nVmData:\t  1000 kB\n";
        assert_eq!(parse_vmrss(status), Some(48_588 * 1024));
    }

    #[test]
    fn a_status_without_vmrss_yields_none() {
        assert_eq!(parse_vmrss("Name:\tfastadhunter\n"), None);
        assert_eq!(parse_vmrss("VmRSS:\tgarbage kB\n"), None);
    }
}
