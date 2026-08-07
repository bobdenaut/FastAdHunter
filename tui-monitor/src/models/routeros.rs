//! RouterOS REST (`/rest/system/resource`, `/rest/container`) — read-only.
//!
//! RouterOS renders most numeric fields as **strings** (`"free-memory":
//! "786432000"`) but not all of them, and `/rest/system/resource` answers with
//! a bare object where `/rest/container` answers with an array. Both quirks are
//! absorbed here, at the boundary, so the rest of the program sees `Option<u64>`.

use serde::{Deserialize, Deserializer};

/// One response that may arrive either bare or wrapped in a single-element
/// array, depending on the path.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    One(Box<T>),
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    pub fn into_first(self) -> Option<T> {
        match self {
            Self::One(value) => Some(*value),
            Self::Many(values) => values.into_iter().next(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SystemResource {
    #[serde(rename = "free-memory", default, deserialize_with = "lenient_u64")]
    pub free_memory: Option<u64>,
    #[serde(rename = "total-memory", default, deserialize_with = "lenient_u64")]
    pub total_memory: Option<u64>,
    /// MHz. Recorded but **not** used to reason about performance: RouterOS's
    /// frequency fields were measured not to predict throughput on this device
    /// (root CLAUDE.md §Environment notes).
    #[serde(rename = "cpu-frequency", default, deserialize_with = "lenient_u64")]
    pub cpu_frequency: Option<u64>,
    #[serde(rename = "cpu-load", default, deserialize_with = "lenient_u64")]
    pub cpu_load: Option<u64>,
    #[serde(default)]
    pub uptime: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Container {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(rename = "memory-current", default, deserialize_with = "lenient_u64")]
    pub memory_current: Option<u64>,
}

/// Accepts a JSON number, a numeric string, or null; anything else is `None`
/// rather than a failed parse of the whole response.
///
/// A visitor rather than an untagged enum: [`OneOrMany`] already buffers the
/// document, and an untagged enum nested inside that buffer matches its
/// catch-all arm instead of the value, turning every figure into `None`.
fn lenient_u64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    struct Lenient;

    impl<'de> serde::de::Visitor<'de> for Lenient {
        type Value = Option<u64>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a number, a numeric string, or null")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
            Ok(Some(value))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
            Ok(u64::try_from(value).ok())
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
            Ok((value.is_finite() && value >= 0.0).then_some(value as u64))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
            Ok(value.trim().parse().ok())
        }

        fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D: Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_any(Lenient)
        }
    }

    deserializer.deserialize_any(Lenient)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::fixtures;

    fn resource(body: &str) -> SystemResource {
        serde_json::from_str::<OneOrMany<SystemResource>>(body)
            .unwrap()
            .into_first()
            .unwrap()
    }

    #[test]
    fn a_bare_resource_object_parses_with_string_numbers() {
        let resource = resource(fixtures::ROUTEROS_RESOURCE);

        assert_eq!(resource.free_memory, Some(786_432_000));
        assert_eq!(resource.cpu_load, Some(0));
        assert_eq!(resource.uptime.as_deref(), Some("3d04:12:55"));
    }

    /// The same device answers with real numbers in an array on some paths and
    /// versions; both spellings must land in the same type.
    #[test]
    fn an_array_wrapped_resource_with_real_numbers_parses_too() {
        let resource = resource(r#"[{"free-memory":786432000,"cpu-load":3}]"#);

        assert_eq!(resource.free_memory, Some(786_432_000));
        assert_eq!(resource.cpu_frequency, None);
    }

    /// A field of an unexpected type yields `None` for that field only — one
    /// oddity on a device this program does not own must not blank the footer.
    #[test]
    fn a_container_list_parses_and_an_odd_field_does_not_fail_it() {
        let containers: Vec<Container> =
            serde_json::from_str(fixtures::ROUTEROS_CONTAINERS).unwrap();

        assert_eq!(containers[0].memory_current, Some(89_346_048));
        assert_eq!(containers[0].status.as_deref(), Some("running"));
        assert_eq!(containers[1].memory_current, None);
    }
}
