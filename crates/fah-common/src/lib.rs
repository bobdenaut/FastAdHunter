//! Shared error types and small utilities used across the workspace (ARCHITECTURE.md L1).

pub mod connections;
pub mod egress;
pub mod histogram;
pub mod listen;
pub mod process;
pub mod resolve;
pub mod retry;
pub mod throttle;

use thiserror::Error;

/// Shared error type across the workspace.
#[derive(Debug, Error)]
pub enum FahError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Message(String),
}

pub type Result<T> = std::result::Result<T, FahError>;

/// Formats an error together with its whole `source()` chain.
///
/// By convention an error's `Display` describes only its own layer, so the
/// cause is lost the moment the error becomes a string — which is how a DNS
/// failure, a refused connection and a rejected certificate all end up logged
/// as the same sentence. The chain has to be walked while the error is still
/// typed, so a caller that logs or stores an error message should walk it
/// here rather than reach for `to_string()`.
///
/// A layer whose own `Display` already interpolates its source (thiserror's
/// `{source}`) is not repeated.
pub fn error_chain(err: &dyn std::error::Error) -> String {
    let mut out = err.to_string();
    let mut next = err.source();
    while let Some(cause) = next {
        let text = cause.to_string();
        if !out.ends_with(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        next = cause.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_converts_via_from() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err: FahError = io_err.into();
        assert_eq!(err.to_string(), "I/O error: missing");
    }

    #[test]
    fn message_variant_displays_message() {
        let err = FahError::Message("boom".to_string());
        assert_eq!(err.to_string(), "boom");
    }

    #[derive(Debug, Error)]
    #[error("outer")]
    struct Outer(#[source] Middle);

    #[derive(Debug, Error)]
    #[error("middle")]
    struct Middle(#[source] std::io::Error);

    /// The whole point: the layer that names the actual cause is the innermost
    /// one, and it is the only one worth reading.
    #[test]
    fn every_cause_in_the_chain_is_reported() {
        let err = Outer(Middle(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "connection refused",
        )));
        assert_eq!(error_chain(&err), "outer: middle: connection refused");
    }

    #[test]
    fn a_lone_error_is_just_its_own_message() {
        assert_eq!(error_chain(&FahError::Message("boom".to_string())), "boom");
    }

    /// `LifecycleError::Fetch` is written as `"fetch {url} failed: {source}"`,
    /// so its source is already in the text — appending it again would read as
    /// a stutter.
    #[test]
    fn a_source_already_interpolated_by_display_is_not_repeated() {
        #[derive(Debug, Error)]
        #[error("wrapper: {0}")]
        struct Interpolating(#[source] Middle);

        let err = Interpolating(Middle(std::io::Error::other("no route to host")));
        assert_eq!(error_chain(&err), "wrapper: middle: no route to host");
    }
}
