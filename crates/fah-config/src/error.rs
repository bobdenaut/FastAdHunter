use std::path::PathBuf;

use thiserror::Error;

/// Errors from loading, parsing, validating or writing FastAdHunter config.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read/write config file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse config: {source}")]
    Parse {
        #[source]
        #[from]
        source: toml::de::Error,
    },

    #[error("failed to serialize default config to TOML: {source}")]
    Serialize {
        #[source]
        #[from]
        source: toml::ser::Error,
    },

    #[error(
        "environment variable {var} does not map to a known config key (`{path}`); array-of-tables \
         sections (dns.upstreams.servers, rules.lists) can only be edited via the config file"
    )]
    UnknownEnvKey { var: String, path: String },

    #[error("environment variable {var}={value:?} must be {expected} (config key `{path}`)")]
    InvalidEnvValue {
        var: String,
        path: String,
        value: String,
        expected: &'static str,
    },

    #[error("invalid value for `{key}`: {message}")]
    Validation { key: &'static str, message: String },
}
