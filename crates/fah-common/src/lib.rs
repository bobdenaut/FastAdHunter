//! Shared error types and small utilities used across the workspace (ARCHITECTURE.md L1).

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
}
