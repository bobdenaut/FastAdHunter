//! Where a rule list's raw text comes from (RULE_ENGINE.md §Sources) and how
//! to fetch it. Fetching is the only I/O the Rule Engine performs — always
//! off the hot path, driven by [`super::ListManager`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::LifecycleError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Validators {
    pub(super) etag: Option<String>,
    pub(super) last_modified: Option<String>,
}

impl Validators {
    pub(super) fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }
}

pub(super) enum FetchOutcome {
    NotModified,
    Body {
        text: String,
        validators: Validators,
    },
}

/// One list's origin: a remote URL or a file under the manager's data
/// directory (RULE_ENGINE.md: remote lists vs local lists). Detected from the
/// configured `url` string — `http://`/`https://` is remote, anything else is
/// a path relative to `/data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ListSource {
    Remote(String),
    LocalFile(PathBuf),
}

impl ListSource {
    pub(super) fn from_url(url: &str, data_dir: &Path) -> Self {
        if url.starts_with("http://") || url.starts_with("https://") {
            ListSource::Remote(url.to_string())
        } else {
            ListSource::LocalFile(data_dir.join(url))
        }
    }

    /// Fetches the list's current raw text. The only failure modes reported
    /// here are I/O (network or filesystem) and an over-`max_bytes` payload —
    /// RULE_ENGINE.md's "unparseable lines never reject a list" means parsing
    /// itself cannot fail. The size cap enforces hard rule 4 (bounded
    /// everything): a rogue or misconfigured source must not be able to grow
    /// memory without limit, so remote bodies are streamed chunk-by-chunk and
    /// abandoned the moment they exceed the cap rather than buffered whole.
    pub(super) async fn fetch(
        &self,
        http: &reqwest::Client,
        timeout: Duration,
        max_bytes: usize,
        cached: &Validators,
    ) -> Result<FetchOutcome, LifecycleError> {
        match self {
            ListSource::Remote(url) => {
                let mut request = http.get(url).timeout(timeout);
                if let Some(etag) = &cached.etag {
                    request = request.header(reqwest::header::IF_NONE_MATCH, etag);
                }
                if let Some(when) = &cached.last_modified {
                    request = request.header(reqwest::header::IF_MODIFIED_SINCE, when);
                }
                let response = request
                    .send()
                    .await
                    .map_err(|source| LifecycleError::Fetch {
                        url: url.clone(),
                        source,
                    })?;
                if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                    return Ok(FetchOutcome::NotModified);
                }
                let header = |name: reqwest::header::HeaderName| {
                    response
                        .headers()
                        .get(name)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned)
                };
                let validators = Validators {
                    etag: header(reqwest::header::ETAG),
                    last_modified: header(reqwest::header::LAST_MODIFIED),
                };
                let mut response =
                    response
                        .error_for_status()
                        .map_err(|source| LifecycleError::Fetch {
                            url: url.clone(),
                            source,
                        })?;

                // Grown by doubling rather than pre-sized from `Content-Length`.
                // A declared length is server-controlled and unverified, so
                // trusting it would let one header buy an allocation up to
                // `max_bytes` for a body that never arrives — replacing a cost
                // paid in bytes on the wire with a free one. Pre-sizing from a
                // ceiling this crate picks is worth doing for the transient peak
                // (the last doubling holds both buffers); tracked as a follow-up
                // in `plan/wip/phase2/CLAUDE.md`, along with rejecting an
                // over-`max_bytes` `Content-Length` before transferring at all.
                let mut body: Vec<u8> = Vec::new();
                while let Some(chunk) =
                    response
                        .chunk()
                        .await
                        .map_err(|source| LifecycleError::Fetch {
                            url: url.clone(),
                            source,
                        })?
                {
                    if body.len() + chunk.len() > max_bytes {
                        return Err(LifecycleError::TooLarge {
                            origin: url.clone(),
                            limit: max_bytes,
                        });
                    }
                    body.extend_from_slice(&chunk);
                }
                // Rule lists are ASCII/UTF-8 in practice; lossy replacement of
                // stray bytes just turns the affected lines into parse errors.
                let text = String::from_utf8(body)
                    .unwrap_or_else(|err| String::from_utf8_lossy(err.as_bytes()).into_owned());
                Ok(FetchOutcome::Body { text, validators })
            }
            ListSource::LocalFile(path) => {
                let len = tokio::fs::metadata(path)
                    .await
                    .map_err(|source| LifecycleError::LocalRead {
                        path: path.clone(),
                        source,
                    })?
                    .len();
                if len > max_bytes as u64 {
                    return Err(LifecycleError::TooLarge {
                        origin: path.display().to_string(),
                        limit: max_bytes,
                    });
                }
                let text = tokio::fs::read_to_string(path).await.map_err(|source| {
                    LifecycleError::LocalRead {
                        path: path.clone(),
                        source,
                    }
                })?;
                Ok(FetchOutcome::Body {
                    text,
                    validators: Validators::default(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_url_is_remote() {
        let source = ListSource::from_url("https://small.oisd.nl", Path::new("/data"));
        assert_eq!(source, ListSource::Remote("https://small.oisd.nl".into()));
    }

    #[test]
    fn bare_name_is_local_under_data_dir() {
        let source = ListSource::from_url("custom.txt", Path::new("/data"));
        assert_eq!(
            source,
            ListSource::LocalFile(PathBuf::from("/data/custom.txt"))
        );
    }
}
