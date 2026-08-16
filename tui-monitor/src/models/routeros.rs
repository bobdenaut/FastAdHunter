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

/// One `/rest/ipv6/neighbor` row — the ND cache, which is the only place the
/// mapping from a v6 address to a device exists at all.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Neighbor {
    #[serde(default)]
    pub address: Option<String>,
    #[serde(rename = "mac-address", default)]
    pub mac_address: Option<String>,
}

/// One `/rest/ip/dhcp-server/lease` row — the bridge from a MAC to the IPv4
/// address the appliance already knows by name.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DhcpLease {
    #[serde(default)]
    pub address: Option<String>,
    #[serde(rename = "mac-address", default)]
    pub mac_address: Option<String>,
    /// Fallbacks when the appliance has no name for the lease's address, worst
    /// last: a router comment is chosen by a human, a host-name by the device.
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(rename = "host-name", default)]
    pub host_name: Option<String>,
}

/// One `/rest/container` row.
///
/// `name` is derived from the image file and carries its version, so the
/// configured name is matched against `comment` — that one is chosen by a human
/// and survives an image bump.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Container {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    /// The CLI's spelling, and older builds'. The RB5009 does not send it —
    /// kept because it costs one `Option` and its absence is not guaranteed
    /// across RouterOS versions.
    #[serde(default)]
    pub status: Option<String>,
    /// How the RB5009 states it: the `/container` property set carries
    /// `running` and no `status` at all.
    #[serde(default, deserialize_with = "lenient_bool")]
    pub running: Option<bool>,
    #[serde(rename = "memory-current", default, deserialize_with = "lenient_u64")]
    pub memory_current: Option<u64>,
}

impl Container {
    pub fn is_named(&self, wanted: &str) -> bool {
        self.comment.as_deref() == Some(wanted) || self.name.as_deref() == Some(wanted)
    }

    /// The word the footer prints. `running` is the field that carries it on
    /// the RB5009; `status` wins where a device sends one, because it
    /// distinguishes states the flag cannot (`paused`, `error`).
    ///
    /// Consumes the row: the caller drops it immediately afterwards, so
    /// borrowing here would clone a `String` every poll for nothing.
    pub fn status_label(self) -> Option<String> {
        self.status.or_else(|| {
            self.running
                .map(|running| if running { "running" } else { "stopped" }.to_string())
        })
    }
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

/// Accepts a JSON bool, `"true"`/`"false"`, or null; anything else is `None`.
fn lenient_bool<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<bool>, D::Error> {
    struct Lenient;

    impl<'de> serde::de::Visitor<'de> for Lenient {
        type Value = Option<bool>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a bool, \"true\"/\"false\", or null")
        }

        fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
            Ok(Some(value))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
            Ok(value.trim().parse().ok())
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
            Ok(Some(value != 0))
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
        let mut containers = serde_json::from_str::<Vec<Container>>(fixtures::ROUTEROS_CONTAINERS)
            .unwrap()
            .into_iter();
        let first = containers.next().unwrap();
        let second = containers.next().unwrap();

        assert_eq!(first.memory_current, Some(89_346_048));
        assert_eq!(first.status_label().as_deref(), Some("running"));
        assert_eq!(second.memory_current, None);
    }

    /// `name` carries the image file's version, so the configured name has to
    /// reach the row through the comment — otherwise every image bump blanks
    /// the footer's container figures until someone edits the config.
    #[test]
    fn the_configured_name_matches_the_comment_a_version_suffixed_name_misses() {
        let containers: Vec<Container> =
            serde_json::from_str(fixtures::ROUTEROS_CONTAINERS).unwrap();

        assert!(containers[0].is_named("fastadhunter"));
        assert!(!containers[1].is_named("fastadhunter"));
    }

    /// The device sends one spelling or the other; the footer prints the same
    /// word either way.
    #[test]
    fn a_running_flag_reads_as_a_status_when_no_status_is_sent() {
        let row = |body: &str| serde_json::from_str::<Container>(body).unwrap();

        assert_eq!(
            row(r#"{"running":"false"}"#).status_label().as_deref(),
            Some("stopped")
        );
        assert_eq!(
            row(r#"{"running":true}"#).status_label().as_deref(),
            Some("running")
        );
        // A status the device does send wins over the flag beside it.
        assert_eq!(
            row(r#"{"status":"paused","running":"true"}"#)
                .status_label()
                .as_deref(),
            Some("paused")
        );
        assert_eq!(row("{}").status_label(), None);
    }
}
