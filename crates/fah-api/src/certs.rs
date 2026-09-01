use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::rejection::JsonRejection;
use axum::extract::{Query, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use axum::Json;
use fah_certs::{
    validate_server_pair, ApiPairSource, CaInstalled, CaParams, CaSummary, CertError, CertStatus,
    CertStore, LeafCacheStats,
};
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::timestamp;

pub(crate) const MAX_BODY_BYTES: usize = 256 * 1024;

const MIN_VALIDITY_DAYS: i64 = 1;
const MAX_VALIDITY_DAYS: i64 = 7_300;
const MAX_COMMON_NAME_BYTES: usize = 64;

const PEM_CONTENT_TYPE: &str = "application/x-pem-file";
const DER_CONTENT_TYPE: &str = "application/pkix-cert";
const PEM_DISPOSITION: &str = "attachment; filename=\"fastadhunter-ca.pem\"";
const DER_DISPOSITION: &str = "attachment; filename=\"fastadhunter-ca.crt\"";

#[derive(Debug, Serialize)]
pub(crate) struct CertificatesResponse {
    pub(crate) ca: CaResponse,
    pub(crate) api_certificate: ApiCertificateResponse,
    pub(crate) leaf_cache: LeafCacheResponse,
}

#[derive(Debug, Serialize)]
pub(crate) struct CaResponse {
    pub(crate) present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fingerprint_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) not_before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) not_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) subject: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ApiCertificateResponse {
    pub(crate) source: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct LeafCacheResponse {
    pub(crate) size: usize,
    pub(crate) capacity: usize,
    pub(crate) inflight: usize,
    pub(crate) hits: u64,
    pub(crate) unwarmed_misses: u64,
    pub(crate) prewarm_hits: u64,
    pub(crate) coalesced: u64,
    pub(crate) minted_total: u64,
    pub(crate) evictions: u64,
    pub(crate) superseded: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct GenerateCaResponse {
    pub(crate) ca: CaResponse,
    pub(crate) archived_previous: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ImportResponse {
    pub(crate) applied: bool,
    pub(crate) restart_required: bool,
    pub(crate) source: &'static str,
}

#[derive(Deserialize)]
pub(crate) struct GenerateCaRequest {
    #[serde(default)]
    pub(crate) confirm: bool,
    pub(crate) common_name: Option<String>,
    pub(crate) validity_days: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct ImportRequest {
    pub(crate) format: Option<String>,
    pub(crate) cert_pem: Option<String>,
    pub(crate) key_pem: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ExportParams {
    pub(crate) format: Option<String>,
}

enum Exported {
    Pem(String),
    Der(Vec<u8>),
}

impl CaResponse {
    fn absent() -> Self {
        Self {
            present: false,
            fingerprint_sha256: None,
            not_before: None,
            not_after: None,
            subject: None,
        }
    }

    fn of(summary: &CaSummary) -> Self {
        Self {
            present: true,
            fingerprint_sha256: Some(summary.fingerprint_sha256.clone()),
            not_before: Some(timestamp::to_rfc3339(instant(summary.not_before))),
            not_after: Some(timestamp::to_rfc3339(instant(summary.not_after))),
            subject: Some(summary.subject.clone()),
        }
    }
}

impl From<LeafCacheStats> for LeafCacheResponse {
    fn from(stats: LeafCacheStats) -> Self {
        Self {
            size: stats.size,
            capacity: stats.capacity,
            inflight: stats.inflight,
            hits: stats.hits,
            unwarmed_misses: stats.unwarmed_misses,
            prewarm_hits: stats.prewarm_hits,
            coalesced: stats.coalesced,
            minted_total: stats.minted_total,
            evictions: stats.evictions,
            superseded: stats.superseded,
        }
    }
}

impl From<CertStatus> for CertificatesResponse {
    fn from(status: CertStatus) -> Self {
        Self {
            ca: status
                .ca
                .as_ref()
                .map_or_else(CaResponse::absent, CaResponse::of),
            api_certificate: ApiCertificateResponse {
                source: source_name(status.api_pair),
            },
            leaf_cache: status.leaves.into(),
        }
    }
}

fn source_name(source: ApiPairSource) -> &'static str {
    match source {
        ApiPairSource::SelfSigned => "self_signed",
        ApiPairSource::Imported => "imported",
    }
}

fn instant(seconds: i64) -> SystemTime {
    match u64::try_from(seconds) {
        Ok(seconds) => UNIX_EPOCH + Duration::from_secs(seconds),
        Err(_) => UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs()),
    }
}

fn store(state: &AppState) -> ApiResult<&Arc<CertStore>> {
    state.certs.as_ref().ok_or_else(|| ApiError::Unavailable {
        message: "the certificate store did not open at start-up; repair /config and \
                  restart — the container log names the file and the reason"
            .to_string(),
        retry_after: None,
    })
}

async fn on_blocking<T: Send + 'static>(
    store: &Arc<CertStore>,
    work: impl FnOnce(&CertStore) -> T + Send + 'static,
) -> ApiResult<T> {
    let store = Arc::clone(store);
    tokio::task::spawn_blocking(move || work(&store))
        .await
        .map_err(join_error)
}

fn join_error(error: tokio::task::JoinError) -> ApiError {
    tracing::error!(%error, "the certificate task did not complete");
    ApiError::Internal(
        "the certificate task did not complete; the container log has the reason".to_string(),
    )
}

fn body_error(rejection: JsonRejection) -> ApiError {
    match rejection {
        JsonRejection::JsonDataError(_)
        | JsonRejection::JsonSyntaxError(_)
        | JsonRejection::MissingJsonContentType(_) => ApiError::BadRequest(
            "the request body must be JSON in the documented shape".to_string(),
        ),
        _ => ApiError::BadRequest(format!(
            "the request body must be JSON of at most {MAX_BODY_BYTES} bytes"
        )),
    }
}

fn opaque_internal(what: &str, error: &CertError) -> ApiError {
    tracing::error!(%error, "{what}");
    ApiError::Internal(format!(
        "{what} failed; the container log names the file and the reason"
    ))
}

pub(crate) fn generate_error(error: CertError) -> ApiError {
    match error {
        CertError::Generate(source) => {
            tracing::warn!(error = %source, "the certificate builder refused the parameters");
            ApiError::ValidationFailed(
                "common_name: the certificate builder refused these parameters".to_string(),
            )
        }
        full @ CertError::ArchiveFull { .. } => archive_full(&full),
        other => opaque_internal("generating the certificate authority", &other),
    }
}

fn archive_full(error: &CertError) -> ApiError {
    ApiError::Conflict(format!("archive_full: {error}"))
}

pub(crate) fn import_error(error: CertError) -> ApiError {
    match error {
        CertError::Parse { .. } => ApiError::ValidationFailed(
            "parse: the certificate or the private key is not readable PEM".to_string(),
        ),
        CertError::Expired { not_after } => ApiError::ValidationFailed(format!(
            "expired: the certificate expired at {}",
            timestamp::to_rfc3339(instant(not_after))
        )),
        CertError::NotYetValid { not_before } => ApiError::ValidationFailed(format!(
            "not_yet_valid: the certificate is not valid before {}",
            timestamp::to_rfc3339(instant(not_before))
        )),
        CertError::KeyMismatch => ApiError::ValidationFailed(
            "key_mismatch: the private key does not match the certificate".to_string(),
        ),
        CertError::NotACa => ApiError::ValidationFailed(
            "not_a_ca: the certificate is not a certificate authority".to_string(),
        ),
        full @ CertError::ArchiveFull { .. } => archive_full(&full),
        other => opaque_internal("importing the certificate pair", &other),
    }
}

pub(crate) async fn status(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<CertificatesResponse>> {
    let store = store(&state)?;
    let status = on_blocking(store, CertStore::status).await?;
    Ok(Json(status.into()))
}

pub(crate) async fn generate_ca(
    State(state): State<Arc<AppState>>,
    body: Result<Json<GenerateCaRequest>, JsonRejection>,
) -> ApiResult<Json<GenerateCaResponse>> {
    let store = store(&state)?;
    let Json(request) = body.map_err(body_error)?;
    if !request.confirm {
        return Err(ApiError::BadRequest(
            "confirm: true is required — regenerating the authority invalidates every \
             client that trusts the current one"
                .to_string(),
        ));
    }
    let params = ca_params(&request)?;

    let CaInstalled {
        summary,
        archived_previous,
    } = on_blocking(store, move |store| store.generate_ca(&params))
        .await?
        .map_err(generate_error)?;

    tracing::info!(
        fingerprint = %summary.fingerprint_sha256,
        archived_previous,
        "generated a certificate authority"
    );

    Ok(Json(GenerateCaResponse {
        ca: CaResponse::of(&summary),
        archived_previous,
    }))
}

pub(crate) async fn export_ca(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExportParams>,
) -> ApiResult<Response> {
    let store = store(&state)?;
    let der = match params.format.as_deref().unwrap_or("pem") {
        "pem" => false,
        "der" => true,
        _ => {
            return Err(ApiError::ValidationFailed(
                "format: expected pem or der".to_string(),
            ))
        }
    };

    let exported = on_blocking(store, move |store| match der {
        true => store.ca_public_der().map(Exported::Der),
        false => store.ca_public_pem().map(Exported::Pem),
    })
    .await?;

    match exported {
        Some(Exported::Pem(text)) => Ok((
            [
                (CONTENT_TYPE, PEM_CONTENT_TYPE),
                (CONTENT_DISPOSITION, PEM_DISPOSITION),
            ],
            text,
        )
            .into_response()),
        Some(Exported::Der(bytes)) => Ok((
            [
                (CONTENT_TYPE, DER_CONTENT_TYPE),
                (CONTENT_DISPOSITION, DER_DISPOSITION),
            ],
            bytes,
        )
            .into_response()),
        None => Err(ApiError::NotFound(
            "no certificate authority exists; generate one first".to_string(),
        )),
    }
}

pub(crate) async fn import(
    State(state): State<Arc<AppState>>,
    body: Result<Json<ImportRequest>, JsonRejection>,
) -> ApiResult<Json<ImportResponse>> {
    let store = store(&state)?;
    let Json(request) = body.map_err(body_error)?;
    let (cert_pem, key_pem) = pem_fields(request)?;

    on_blocking(store, move |store| {
        validate_server_pair(&cert_pem, &key_pem).and_then(|pair| store.install_api_pair(&pair))
    })
    .await?
    .map_err(import_error)?;

    tracing::info!("imported an API server certificate pair; it applies on the next restart");

    Ok(Json(ImportResponse {
        applied: false,
        restart_required: true,
        source: "imported",
    }))
}

fn pem_fields(request: ImportRequest) -> ApiResult<(String, String)> {
    match request.format.as_deref().unwrap_or("pem") {
        "pem" => {}
        "pfx" | "pkcs12" => {
            return Err(ApiError::ValidationFailed(
                "unsupported_format: PKCS#12/PFX import is not offered; convert with \
                 openssl pkcs12 -in cert.pfx -out cert.pem -nodes and import the PEM"
                    .to_string(),
            ))
        }
        _ => {
            return Err(ApiError::ValidationFailed(
                "unsupported_format: expected pem".to_string(),
            ))
        }
    }

    let cert_pem = request.cert_pem.unwrap_or_default();
    let key_pem = request.key_pem.unwrap_or_default();
    if cert_pem.trim().is_empty() || key_pem.trim().is_empty() {
        return Err(ApiError::ValidationFailed(
            "parse: cert_pem and key_pem are both required and must be PEM".to_string(),
        ));
    }
    Ok((cert_pem, key_pem))
}

fn ca_params(request: &GenerateCaRequest) -> ApiResult<CaParams> {
    let mut params = CaParams::default();
    if let Some(common_name) = &request.common_name {
        let trimmed = common_name.trim();
        let printable = !trimmed.chars().any(char::is_control);
        if trimmed.is_empty() || trimmed.len() > MAX_COMMON_NAME_BYTES || !printable {
            return Err(ApiError::ValidationFailed(format!(
                "common_name: 1 to {MAX_COMMON_NAME_BYTES} bytes, no control characters"
            )));
        }
        params.common_name = trimmed.to_string();
    }
    if let Some(days) = request.validity_days {
        if !(MIN_VALIDITY_DAYS..=MAX_VALIDITY_DAYS).contains(&days) {
            return Err(ApiError::ValidationFailed(format!(
                "validity_days: expected {MIN_VALIDITY_DAYS} to {MAX_VALIDITY_DAYS}"
            )));
        }
        params.validity_days = days;
    }
    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> CaSummary {
        CaSummary {
            fingerprint_sha256: "AB:CD".to_string(),
            not_before: 1_784_000_463,
            not_after: 1_784_000_464,
            subject: "CN=FastAdHunter CA".to_string(),
        }
    }

    fn stats() -> LeafCacheStats {
        LeafCacheStats {
            size: 3,
            capacity: 512,
            inflight: 1,
            hits: 4,
            unwarmed_misses: 5,
            prewarm_hits: 6,
            coalesced: 7,
            minted_total: 8,
            evictions: 9,
            superseded: 10,
        }
    }

    fn json_of<T: Serialize>(value: &T) -> serde_json::Value {
        serde_json::to_value(value).unwrap()
    }

    #[test]
    fn a_status_without_an_authority_reports_present_false_and_nothing_else() {
        let body = json_of(&CertificatesResponse::from(CertStatus {
            ca: None,
            leaves: stats(),
            api_pair: ApiPairSource::SelfSigned,
        }));

        assert_eq!(body["ca"], serde_json::json!({ "present": false }));
        assert_eq!(body["api_certificate"]["source"], "self_signed");
        assert_eq!(
            body["leaf_cache"],
            serde_json::json!({
                "size": 3,
                "capacity": 512,
                "inflight": 1,
                "hits": 4,
                "unwarmed_misses": 5,
                "prewarm_hits": 6,
                "coalesced": 7,
                "minted_total": 8,
                "evictions": 9,
                "superseded": 10,
            })
        );
    }

    #[test]
    fn a_status_with_an_authority_renders_rfc3339_and_the_imported_marker() {
        let body = json_of(&CertificatesResponse::from(CertStatus {
            ca: Some(summary()),
            leaves: stats(),
            api_pair: ApiPairSource::Imported,
        }));

        assert_eq!(
            body["ca"],
            serde_json::json!({
                "present": true,
                "fingerprint_sha256": "AB:CD",
                "not_before": "2026-07-14T03:41:03Z",
                "not_after": "2026-07-14T03:41:04Z",
                "subject": "CN=FastAdHunter CA",
            })
        );
        assert_eq!(body["api_certificate"]["source"], "imported");
    }

    #[test]
    fn the_generate_and_import_responses_carry_the_documented_fields() {
        let generated = json_of(&GenerateCaResponse {
            ca: CaResponse::of(&summary()),
            archived_previous: true,
        });
        assert_eq!(
            generated,
            serde_json::json!({
                "ca": {
                    "present": true,
                    "fingerprint_sha256": "AB:CD",
                    "not_before": "2026-07-14T03:41:03Z",
                    "not_after": "2026-07-14T03:41:04Z",
                    "subject": "CN=FastAdHunter CA",
                },
                "archived_previous": true,
            })
        );

        let imported = json_of(&ImportResponse {
            applied: false,
            restart_required: true,
            source: "imported",
        });
        assert_eq!(
            imported,
            serde_json::json!({
                "applied": false,
                "restart_required": true,
                "source": "imported",
            })
        );
    }

    #[test]
    fn every_rejection_the_validator_can_return_maps_to_its_named_422() {
        let cases = [
            (
                CertError::Parse {
                    what: "the certificate",
                    detail: "no CERTIFICATE block".to_string(),
                },
                "parse:",
            ),
            (CertError::Expired { not_after: 0 }, "expired:"),
            (CertError::NotYetValid { not_before: 0 }, "not_yet_valid:"),
            (CertError::KeyMismatch, "key_mismatch:"),
            (CertError::NotACa, "not_a_ca:"),
        ];

        for (error, prefix) in cases {
            match import_error(error) {
                ApiError::ValidationFailed(message) => {
                    assert!(message.starts_with(prefix), "{message}")
                }
                other => panic!("expected a 422 for {prefix}, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_failure_that_is_not_the_callers_fault_is_a_500_that_names_no_path() {
        for error in [
            import_error(CertError::Io {
                path: "/config/api-key.pem".into(),
                source: std::io::Error::other("disk full"),
            }),
            generate_error(CertError::Io {
                path: "/config/ca-key.pem".into(),
                source: std::io::Error::other("disk full"),
            }),
        ] {
            match error {
                ApiError::Internal(message) => {
                    assert!(!message.contains("/config"), "{message}");
                    assert!(!message.contains("disk full"), "{message}");
                }
                other => panic!("expected a 500, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_full_archive_is_a_409_on_both_routes_naming_the_directory_not_the_path() {
        for mapped in [
            generate_error(CertError::ArchiveFull {
                archive: "ca-archive",
                limit: 8,
            }),
            import_error(CertError::ArchiveFull {
                archive: "api-archive",
                limit: 8,
            }),
        ] {
            match mapped {
                ApiError::Conflict(message) => {
                    assert!(message.starts_with("archive_full:"), "{message}");
                    assert!(message.contains("-archive"), "{message}");
                    assert!(!message.contains("/config/"), "{message}");
                }
                other => panic!("expected a 409, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn a_task_that_panics_answers_a_500_that_carries_no_panic_payload() {
        let error = tokio::task::spawn_blocking(|| panic!("/config/api-key.pem: disk full"))
            .await
            .unwrap_err();
        assert!(error.is_panic());
        match join_error(error) {
            ApiError::Internal(message) => {
                assert!(!message.contains("/config"), "{message}");
                assert!(!message.contains("disk full"), "{message}");
            }
            other => panic!("expected a 500, got {other:?}"),
        }
    }

    #[test]
    fn parameters_the_certificate_builder_refuses_are_the_callers_fault() {
        let refused = generate_error(CertError::Generate(rcgen::Error::CouldNotParseCertificate));
        match refused {
            ApiError::ValidationFailed(message) => {
                assert!(message.starts_with("common_name:"), "{message}")
            }
            other => panic!("expected a 422, got {other:?}"),
        }
    }

    #[test]
    fn pfx_is_refused_with_the_conversion_command_before_any_field_is_read() {
        let request = ImportRequest {
            format: Some("pfx".to_string()),
            cert_pem: None,
            key_pem: None,
        };
        match pem_fields(request) {
            Err(ApiError::ValidationFailed(message)) => {
                assert!(message.starts_with("unsupported_format:"), "{message}");
                assert!(message.contains("openssl pkcs12"), "{message}");
            }
            other => panic!("expected a 422, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_or_blank_pem_field_is_a_parse_rejection() {
        for (cert, key) in [
            (None, Some("key".to_string())),
            (Some("cert".to_string()), None),
            (Some("   ".to_string()), Some("key".to_string())),
        ] {
            let request = ImportRequest {
                format: None,
                cert_pem: cert,
                key_pem: key,
            };
            match pem_fields(request) {
                Err(ApiError::ValidationFailed(message)) => {
                    assert!(message.starts_with("parse:"), "{message}")
                }
                other => panic!("expected a 422, got {other:?}"),
            }
        }
    }

    #[test]
    fn generation_parameters_default_and_are_range_checked() {
        let default = ca_params(&GenerateCaRequest {
            confirm: true,
            common_name: None,
            validity_days: None,
        })
        .unwrap();
        assert_eq!(default, CaParams::default());

        let custom = ca_params(&GenerateCaRequest {
            confirm: true,
            common_name: Some("  Household CA  ".to_string()),
            validity_days: Some(30),
        })
        .unwrap();
        assert_eq!(custom.common_name, "Household CA");
        assert_eq!(custom.validity_days, 30);

        for (name, days) in [
            (None, Some(0)),
            (None, Some(MAX_VALIDITY_DAYS + 1)),
            (Some(String::new()), None),
            (Some("x".repeat(MAX_COMMON_NAME_BYTES + 1)), None),
            (Some("é".repeat(MAX_COMMON_NAME_BYTES / 2 + 1)), None),
            (Some("Household\u{0}CA".to_string()), None),
        ] {
            let request = GenerateCaRequest {
                confirm: true,
                common_name: name,
                validity_days: days,
            };
            assert!(matches!(
                ca_params(&request),
                Err(ApiError::ValidationFailed(_))
            ));
        }
    }

    #[test]
    fn a_pre_epoch_validity_still_formats_instead_of_panicking() {
        assert_eq!(
            timestamp::to_rfc3339(instant(-1)),
            "1969-12-31T23:59:59Z".to_string()
        );
    }
}
