use std::str::FromStr;

use crate::error::ConfigError;
use crate::schema::Config;

const PREFIX: &str = "FAH__";

/// Collects `FAH__`-prefixed pairs from the real process environment. The
/// only function in this module that touches `std::env` — everything else
/// operates on plain data, so tests never need `std::env::set_var`.
pub(crate) fn collect_env_pairs() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(key, _)| key.starts_with(PREFIX))
        .collect()
}

/// Applies `FAH__SECTION__FIELD=value` pairs on top of an already-loaded
/// config (defaults < file < env, per CONFIGURATION.md precedence).
pub(crate) fn apply_env_overrides(
    mut config: Config,
    pairs: &[(String, String)],
) -> Result<Config, ConfigError> {
    for (var, value) in pairs {
        let remainder = var
            .strip_prefix(PREFIX)
            .expect("pairs are pre-filtered by FAH__ prefix");
        let segments: Vec<String> = remainder.split("__").map(str::to_lowercase).collect();
        let path = segments.join(".");
        apply_one(&mut config, var, &path, &segments, value)?;
    }
    Ok(config)
}

fn apply_one(
    config: &mut Config,
    var: &str,
    path: &str,
    segments: &[String],
    value: &str,
) -> Result<(), ConfigError> {
    let seg: Vec<&str> = segments.iter().map(String::as_str).collect();
    match seg.as_slice() {
        ["engine", "mode"] => config.engine.mode = coerce_enum(var, path, value)?,

        ["dns", "listen", "address"] => config.dns.listen.address = value.to_string(),
        ["dns", "listen", "port"] => config.dns.listen.port = coerce_u16(var, path, value)?,

        ["dns", "blocking", "mode"] => config.dns.blocking.mode = coerce_enum(var, path, value)?,
        ["dns", "blocking", "ttl_seconds"] => {
            config.dns.blocking.ttl_seconds = coerce_u32(var, path, value)?
        }

        ["dns", "cache", "max_entries"] => {
            config.dns.cache.max_entries = coerce_u32(var, path, value)?
        }
        ["dns", "cache", "max_bytes"] => config.dns.cache.max_bytes = coerce_u64(var, path, value)?,
        ["dns", "cache", "min_ttl_seconds"] => {
            config.dns.cache.min_ttl_seconds = coerce_u32(var, path, value)?
        }
        ["dns", "cache", "max_ttl_seconds"] => {
            config.dns.cache.max_ttl_seconds = coerce_u32(var, path, value)?
        }
        ["dns", "cache", "negative_ttl_max_seconds"] => {
            config.dns.cache.negative_ttl_max_seconds = coerce_u32(var, path, value)?
        }
        ["dns", "cache", "serve_stale"] => {
            config.dns.cache.serve_stale = coerce_bool(var, path, value)?
        }
        ["dns", "cache", "swr_workers"] => {
            config.dns.cache.swr_workers = coerce_u32(var, path, value)?
        }
        ["dns", "cache", "cleanup_interval_seconds"] => {
            config.dns.cache.cleanup_interval_seconds = coerce_u32(var, path, value)?
        }

        ["dns", "upstreams", "strategy"] => {
            config.dns.upstreams.strategy = coerce_enum(var, path, value)?
        }
        ["dns", "upstreams", "timeout_ms"] => {
            config.dns.upstreams.timeout_ms = coerce_u32(var, path, value)?
        }

        ["rules", "refresh_hours_default"] => {
            config.rules.refresh_hours_default = coerce_u32(var, path, value)?
        }

        ["query_log", "enabled"] => config.query_log.enabled = coerce_bool(var, path, value)?,
        ["query_log", "ring_entries"] => {
            config.query_log.ring_entries = coerce_u32(var, path, value)?
        }
        ["query_log", "retention_days"] => {
            config.query_log.retention_days = coerce_u32(var, path, value)?
        }
        ["query_log", "retention_max_mb"] => {
            config.query_log.retention_max_mb = coerce_u32(var, path, value)?
        }
        ["query_log", "flush_interval_seconds"] => {
            config.query_log.flush_interval_seconds = coerce_u32(var, path, value)?
        }

        ["stats", "snapshot_interval_seconds"] => {
            config.stats.snapshot_interval_seconds = coerce_u32(var, path, value)?
        }

        ["history", "enabled"] => config.history.enabled = coerce_bool(var, path, value)?,
        ["history", "sample_interval_seconds"] => {
            config.history.sample_interval_seconds = coerce_u32(var, path, value)?
        }
        ["history", "retention_days"] => {
            config.history.retention_days = coerce_u32(var, path, value)?
        }

        ["api", "address"] => config.api.address = value.to_string(),
        ["api", "port"] => config.api.port = coerce_u16(var, path, value)?,
        ["api", "tls"] => config.api.tls = coerce_bool(var, path, value)?,
        ["api", "metrics_public"] => config.api.metrics_public = coerce_bool(var, path, value)?,

        ["log", "level"] => config.log.level = coerce_enum(var, path, value)?,
        ["log", "format"] => config.log.format = coerce_enum(var, path, value)?,

        _ => {
            return Err(ConfigError::UnknownEnvKey {
                var: var.to_string(),
                path: path.to_string(),
            })
        }
    }
    Ok(())
}

fn coerce_bool(var: &str, path: &str, value: &str) -> Result<bool, ConfigError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid(var, path, value, "a bool (`true` or `false`)")),
    }
}

fn coerce_u16(var: &str, path: &str, value: &str) -> Result<u16, ConfigError> {
    value
        .parse::<u16>()
        .map_err(|_| invalid(var, path, value, "a u16 integer"))
}

fn coerce_u32(var: &str, path: &str, value: &str) -> Result<u32, ConfigError> {
    value
        .parse::<u32>()
        .map_err(|_| invalid(var, path, value, "a u32 integer"))
}

fn coerce_u64(var: &str, path: &str, value: &str) -> Result<u64, ConfigError> {
    value
        .parse::<u64>()
        .map_err(|_| invalid(var, path, value, "a u64 integer"))
}

fn coerce_enum<T>(var: &str, path: &str, value: &str) -> Result<T, ConfigError>
where
    T: FromStr<Err = &'static str>,
{
    value
        .parse::<T>()
        .map_err(|expected| invalid(var, path, value, expected))
}

fn invalid(var: &str, path: &str, value: &str, expected: &'static str) -> ConfigError {
    ConfigError::InvalidEnvValue {
        var: var.to_string(),
        path: path.to_string(),
        value: value.to_string(),
        expected,
    }
}
