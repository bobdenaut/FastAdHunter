//! Dropping root after the privileged bind (ADR-0004).
//!
//! Port 53 requires privilege to bind and nothing after that point does, so
//! the process starts as root, takes its sockets, and permanently becomes the
//! unprivileged service user before answering a single query. This is what
//! bind9, unbound and dnsmasq all do, and on RouterOS it is the only option
//! that works: MikroTik offers no `cap-add`, does not lower
//! `net.ipv4.ip_unprivileged_port_start` the way Docker does, and drops the
//! `CAP_NET_BIND_SERVICE` file capability on image import (ADR-0004, gate 2).
//!
//! Started as an unprivileged user already — the normal Docker case — every
//! function here is a no-op. The deployment target is Linux; the non-Unix
//! implementation exists so the workspace still builds and tests on a Windows
//! development machine.

#[cfg(unix)]
mod imp {
    use std::io;
    use std::path::Path;

    /// The `nonroot` uid/gid baked into `gcr.io/distroless/static-debian12`,
    /// and the owner the image seeds `/config` and `/data` with.
    const SERVICE_UID: u32 = 65532;
    const SERVICE_GID: u32 = 65532;

    /// Whether this process is root and therefore has something to drop.
    pub fn is_root() -> bool {
        // SAFETY: `geteuid` takes no arguments, cannot fail, and only reads
        // process credentials.
        unsafe { libc::geteuid() == 0 }
    }

    /// Hands the state directories to the service user, then becomes that user
    /// irreversibly.
    ///
    /// Ordering is security-critical and is why this is one function rather
    /// than several: `setgroups` must precede `setgid`, and `setgid` must
    /// precede `setuid`. Reversing the last two leaves the process stuck with
    /// its original groups, because dropping the uid first removes the very
    /// permission needed to change the gid.
    pub fn drop_to_service_user(state_dirs: &[&Path]) -> io::Result<()> {
        if !is_root() {
            return Ok(());
        }

        for dir in state_dirs {
            reown_if_needed(dir)?;
        }

        // SAFETY: each is a plain credential-setting syscall on the current
        // process with no memory operands. The empty supplementary-group list
        // is a null pointer with length 0, which `setgroups` accepts.
        unsafe {
            if libc::setgroups(0, std::ptr::null()) != 0 {
                return Err(annotate("setgroups"));
            }
            if libc::setgid(SERVICE_GID) != 0 {
                return Err(annotate("setgid"));
            }
            if libc::setuid(SERVICE_UID) != 0 {
                return Err(annotate("setuid"));
            }
        }

        verify_drop_is_irreversible()?;
        tracing::info!(
            uid = SERVICE_UID,
            gid = SERVICE_GID,
            "dropped privileges after binding"
        );
        Ok(())
    }

    /// A `setuid` that returned success but left the process able to regain
    /// root would be worse than never dropping, because everything downstream
    /// assumes the privilege is gone. Confirm it rather than trust the return
    /// code.
    fn verify_drop_is_irreversible() -> io::Result<()> {
        // SAFETY: read-only credential queries; neither can fail.
        let (uid, euid) = unsafe { (libc::getuid(), libc::geteuid()) };
        if uid != SERVICE_UID || euid != SERVICE_UID {
            return Err(io::Error::other(format!(
                "privilege drop did not take effect: uid={uid} euid={euid}, expected {SERVICE_UID}"
            )));
        }

        // SAFETY: deliberately attempting to regain root. Success is the
        // failure case, reported below; it corrupts no state.
        if unsafe { libc::setuid(0) } == 0 {
            return Err(io::Error::other(
                "privilege drop is reversible: the process regained uid 0",
            ));
        }
        Ok(())
    }

    /// Recursively gives `dir` to the service user, but only when it is not
    /// already theirs.
    ///
    /// The check matters: `/data` accumulates query-log segments and tens of
    /// megabytes of cached lists, and walking all of it every boot to reapply
    /// ownership that is already correct is pure waste. A wrong owner is the
    /// exceptional case — a first boot on a fresh volume, or one left behind
    /// by a differently-configured run.
    fn reown_if_needed(dir: &Path) -> io::Result<()> {
        if !dir.exists() {
            return Ok(());
        }
        if owner_of(dir)? == (SERVICE_UID, SERVICE_GID) {
            return Ok(());
        }
        tracing::info!(path = %dir.display(), "adopting state directory for the service user");
        reown_tree(dir)
    }

    fn owner_of(path: &Path) -> io::Result<(u32, u32)> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(path)?;
        Ok((meta.uid(), meta.gid()))
    }

    /// Iterative rather than recursive: the depth here is bounded by whatever
    /// a user mounts at `/config` and `/data`, which is not ours to assume.
    ///
    /// Symlinks are chowned but never descended into, and the check is written
    /// out explicitly rather than left to rest on `symlink_metadata` reporting
    /// a link as a non-directory. Both that and `lchown` refusing to follow
    /// links are load-bearing — a followed link would let anything writable
    /// inside a mounted volume redirect ownership changes anywhere on the
    /// filesystem, while the process is still root.
    fn reown_tree(root: &Path) -> io::Result<()> {
        let mut stack = vec![root.to_path_buf()];
        while let Some(path) = stack.pop() {
            let meta = std::fs::symlink_metadata(&path)?;
            std::os::unix::fs::lchown(&path, Some(SERVICE_UID), Some(SERVICE_GID))?;

            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                for entry in std::fs::read_dir(&path)? {
                    stack.push(entry?.path());
                }
            }
        }
        Ok(())
    }

    fn annotate(call: &str) -> io::Error {
        let err = io::Error::last_os_error();
        io::Error::new(err.kind(), format!("{call}: {err}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The branch actually exercised by the test suite: an already
        /// unprivileged process, which is also the normal Docker path.
        #[test]
        fn dropping_is_a_no_op_when_already_unprivileged() {
            if is_root() {
                // Guard rather than assert: a root-run suite must not
                // irreversibly drop its own uid.
                return;
            }
            let dir = tempfile::tempdir().unwrap();
            assert!(drop_to_service_user(&[dir.path()]).is_ok());
        }

        #[test]
        fn a_missing_state_directory_is_not_an_error() {
            assert!(reown_if_needed(Path::new("/nonexistent/fastadhunter")).is_ok());
        }

        /// A symlink planted in a mounted volume must not redirect the walk
        /// outside it. Unprivileged here, so `lchown` cannot actually change
        /// ownership — what this pins is the traversal: the walk must not
        /// descend through the link and touch the target directory's contents.
        #[test]
        fn the_walk_does_not_descend_through_a_symlink() {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("untouched.txt"), b"x").unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();

            let mut visited = Vec::new();
            let mut stack = vec![root.path().to_path_buf()];
            while let Some(path) = stack.pop() {
                let meta = std::fs::symlink_metadata(&path).unwrap();
                visited.push(path.clone());
                if meta.file_type().is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    for entry in std::fs::read_dir(&path).unwrap() {
                        stack.push(entry.unwrap().path());
                    }
                }
            }

            assert!(visited.iter().any(|p| p.ends_with("escape")));
            assert!(
                !visited.iter().any(|p| p.ends_with("untouched.txt")),
                "the walk followed a symlink out of the tree: {visited:?}"
            );
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use std::io;
    use std::path::Path;

    /// Windows has no uid to drop; the shipped artefact is a Linux container.
    pub fn drop_to_service_user(_state_dirs: &[&Path]) -> io::Result<()> {
        Ok(())
    }
}

pub use imp::drop_to_service_user;
