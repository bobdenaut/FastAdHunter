//! Process readings from procfs (ARCHITECTURE.md L1).
//!
//! Here because two crates need RSS and neither may import the other: `fah-api`
//! serves it live, the binary samples it for the perf series and the metrics
//! snapshot. It was hand-parsed in two places before this. `fah-metrics` does
//! **not** read it — every figure it renders arrives through `Metrics::set_*`,
//! so one scrape stays internally consistent.

/// Current RSS in bytes, or `None` where it cannot be read.
///
/// `None` rather than `0` off Linux: a fabricated zero charts as a real
/// measurement.
#[cfg(target_os = "linux")]
pub fn resident_bytes() -> Option<u64> {
    parse_vm_rss(&std::fs::read_to_string("/proc/self/status").ok()?)
}

#[cfg(not(target_os = "linux"))]
pub fn resident_bytes() -> Option<u64> {
    None
}

/// `VmRSS:` line → bytes. Split from the file read so it is testable off the
/// deployment target.
#[allow(dead_code, reason = "only called on the Linux deployment target")]
fn parse_vm_rss(status: &str) -> Option<u64> {
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vm_rss_line_parses_to_bytes() {
        let status = "VmPeak:\t   50000 kB\nVmRSS:\t   48588 kB\nVmData:\t  1000 kB\n";
        assert_eq!(parse_vm_rss(status), Some(48_588 * 1024));
    }

    #[test]
    fn a_status_without_a_readable_vm_rss_yields_none() {
        assert_eq!(parse_vm_rss("Name:\tfastadhunter\n"), None);
        assert_eq!(parse_vm_rss("VmRSS:\tgarbage kB\n"), None);
        assert_eq!(parse_vm_rss("VmRSS:\n"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_running_process_reports_nonzero_rss() {
        assert!(resident_bytes().is_some_and(|bytes| bytes > 0));
    }
}
