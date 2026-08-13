//! Process readings from procfs (ARCHITECTURE.md L1).
//!
//! Here because two crates need RSS and neither may import the other: `fah-api`
//! serves it live, the binary samples it for the perf series and the metrics
//! snapshot. It was hand-parsed in two places before this. `fah-metrics` does
//! **not** read it — every figure it renders arrives through `Metrics::set_*`,
//! so one scrape stays internally consistent.

/// RSS and the parts the kernel breaks it into, all from one read so they
/// describe the same instant.
///
/// `anon` and `file` separate a heap that is holding memory from page cache the
/// kernel charges to this process. They are `Option` on their own because
/// `RssAnon:`/`RssFile:` postdate `VmRSS:` — a kernel that reports only the
/// total is served an absent split, never a fabricated one.
///
/// **`total` is `anon + file + RssShmem`, not `anon + file`.** Shared memory is
/// a third bucket and is deliberately not a field here: `total − anon − file`
/// yields it from what is already served, so storing it would be a fourth
/// number that can disagree with the three it is derived from. A gap between
/// the halves and the total is shared memory, never a parse error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resident {
    /// `VmRSS:` — the total, and the only field guaranteed present.
    pub total: u64,
    /// `RssAnon:` — heap, thread stacks, anything not backed by a file.
    pub anon: Option<u64>,
    /// `RssFile:` — file-backed pages: the binary's text, mapped `/data` reads.
    pub file: Option<u64>,
}

/// One `/proc/self/status` read, parsed into [`Resident`], or `None` where it
/// cannot be read.
///
/// `None` rather than `0` off Linux: a fabricated zero charts as a real
/// measurement.
#[cfg(target_os = "linux")]
pub fn resident() -> Option<Resident> {
    parse_resident(&std::fs::read_to_string("/proc/self/status").ok()?)
}

#[cfg(not(target_os = "linux"))]
pub fn resident() -> Option<Resident> {
    None
}

/// Current RSS in bytes, or `None` where it cannot be read. For callers that
/// need the total alone; [`resident`] costs the same read and carries the split.
pub fn resident_bytes() -> Option<u64> {
    resident().map(|resident| resident.total)
}

/// Parses the three `Rss*`/`VmRSS` lines. Split from the file read so it is
/// testable off the deployment target.
#[allow(dead_code, reason = "only called on the Linux deployment target")]
fn parse_resident(status: &str) -> Option<Resident> {
    Some(Resident {
        total: parse_kib_line(status, "VmRSS:")?,
        anon: parse_kib_line(status, "RssAnon:"),
        file: parse_kib_line(status, "RssFile:"),
    })
}

/// One `Key:\t<n> kB` line → bytes.
#[allow(dead_code, reason = "only called on the Linux deployment target")]
fn parse_kib_line(status: &str, key: &str) -> Option<u64> {
    let line = status.lines().find(|line| line.starts_with(key))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped like the real file: `RssShmem` is present and non-zero, so a
    /// reader assuming `anon + file == total` is wrong here as it is on the
    /// device.
    const STATUS: &str = "VmPeak:\t   50000 kB\nVmRSS:\t   48588 kB\n\
         RssAnon:\t   30000 kB\nRssFile:\t   18000 kB\nRssShmem:\t 588 kB\n\
         VmData:\t  1000 kB\n";

    #[test]
    fn the_vm_rss_line_parses_to_bytes() {
        assert_eq!(parse_resident(STATUS).unwrap().total, 48_588 * 1024);
    }

    #[test]
    fn the_split_parses_beside_the_total() {
        let resident = parse_resident(STATUS).unwrap();
        assert_eq!(resident.anon, Some(30_000 * 1024));
        assert_eq!(resident.file, Some(18_000 * 1024));
    }

    /// The gap between the halves and the total is shared memory, which is why
    /// no `shmem` field exists — it is derivable, and a stored fourth number
    /// could disagree with the three it comes from.
    #[test]
    fn what_the_two_halves_do_not_cover_is_shared_memory() {
        let resident = parse_resident(STATUS).unwrap();
        let shmem = resident.total - resident.anon.unwrap() - resident.file.unwrap();
        assert_eq!(shmem, 588 * 1024);
    }

    /// `RssAnon:`/`RssFile:` postdate `VmRSS:`. A kernel reporting only the
    /// total must still give a usable reading, with the split absent rather
    /// than zero — zero would chart as "no heap".
    #[test]
    fn a_kernel_without_the_split_still_reports_the_total() {
        let resident = parse_resident("VmRSS:\t   48588 kB\n").unwrap();
        assert_eq!(resident.total, 48_588 * 1024);
        assert_eq!(resident.anon, None);
        assert_eq!(resident.file, None);
    }

    #[test]
    fn a_status_without_a_readable_vm_rss_yields_none() {
        assert!(parse_resident("Name:\tfastadhunter\n").is_none());
        assert!(parse_resident("VmRSS:\tgarbage kB\n").is_none());
        assert!(parse_resident("VmRSS:\n").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_running_process_reports_nonzero_rss() {
        assert!(resident_bytes().is_some_and(|bytes| bytes > 0));
    }
}
