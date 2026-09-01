use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CertError {
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("generating certificate: {0}")]
    Generate(#[source] rcgen::Error),
    #[error("{path} contains no {what}")]
    Empty { path: PathBuf, what: &'static str },
    #[error("building TLS config from {cert} and {key}: {source}")]
    Config {
        cert: PathBuf,
        key: PathBuf,
        #[source]
        source: Box<rustls::Error>,
    },
    #[error(
        "{present} exists but {missing} does not; \
         move both aside together to regenerate the pair"
    )]
    IncompletePair { present: PathBuf, missing: PathBuf },
    #[error("parsing {what}: {detail}")]
    Parse { what: &'static str, detail: String },
    #[error("the certificate expired at {not_after}")]
    Expired { not_after: i64 },
    #[error("the certificate is not valid before {not_before}")]
    NotYetValid { not_before: i64 },
    #[error("the private key does not match the certificate")]
    KeyMismatch,
    #[error("the certificate is not a CA certificate")]
    NotACa,
    #[error("no certificate authority exists; generate one first")]
    NoCa,
    #[error("the host is empty or longer than a DNS name may be")]
    InvalidHost,
    #[error(
        "{archive} already holds {limit} retired pairs; \
         move some out of /config before replacing this pair"
    )]
    ArchiveFull { archive: &'static str, limit: usize },
}

impl CertError {
    pub(crate) fn parse(what: &'static str, detail: impl std::fmt::Display) -> Self {
        Self::Parse {
            what,
            detail: detail.to_string(),
        }
    }
}
