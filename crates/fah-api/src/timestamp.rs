//! RFC 3339 conversion for the wire. Every timestamp API.md shows is an RFC
//! 3339 string (`"2026-07-17T10:41:03.412Z"`), while the crates behind the
//! ports speak `SystemTime` — serde would render that as
//! `{secs_since_epoch, nanos_since_epoch}`, so the mapping happens here,
//! once, on the way in and out.

use std::time::SystemTime;

use serde::Serializer;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Formats as RFC 3339 in UTC. A time outside the representable range (only
/// reachable from a corrupted persisted value) falls back to the epoch
/// rather than failing a whole response.
pub fn to_rfc3339(at: SystemTime) -> String {
    OffsetDateTime::from(at)
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// Parses the `from`/`to` range parameters of `GET /api/v1/history/*`.
pub fn from_rfc3339(text: &str) -> Option<SystemTime> {
    OffsetDateTime::parse(text, &Rfc3339)
        .ok()
        .map(SystemTime::from)
}

/// `#[serde(serialize_with = "…")]` hook for `SystemTime` fields.
pub fn serialize<S: Serializer>(at: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&to_rfc3339(*at))
}

/// Same, for `Option<SystemTime>` — `null` when absent (API.md's
/// `last_refresh` before a list's first successful refresh).
pub fn serialize_option<S: Serializer>(
    at: &Option<SystemTime>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match at {
        Some(at) => serializer.serialize_str(&to_rfc3339(*at)),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn epoch_formats_as_documented_utc_shape() {
        assert_eq!(to_rfc3339(SystemTime::UNIX_EPOCH), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn round_trips_through_parse() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_784_000_463);
        let text = to_rfc3339(at);
        assert_eq!(from_rfc3339(&text), Some(at));
    }

    #[test]
    fn rejects_a_non_rfc3339_string() {
        assert_eq!(from_rfc3339("yesterday"), None);
        assert_eq!(from_rfc3339("2026-07-17"), None);
    }
}
