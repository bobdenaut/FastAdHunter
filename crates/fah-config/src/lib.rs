//! TOML config loading, merging, precedence, and hot-reload (ARCHITECTURE.md L1).

mod env;
mod error;
mod schema;
mod tz;

use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

pub use error::ConfigError;
pub use schema::{
    parse_days, parse_time_of_day, ApiConfig, AssignmentConfig, BlockingMode, Config,
    DnsBlockingConfig, DnsCacheConfig, DnsConfig, DnsListenConfig, DnsUpstreamsConfig,
    EngineConfig, EngineMode, HistoryConfig, HttpConfig, HttpListenConfig, HttpsConfig,
    HttpsListenConfig, InterceptionConfig, LogConfig, LogFormat, LogLevel, NoSni, PolicyConfig,
    RuleListConfig, RulesConfig, RuntimeConfig, ScheduleConfig, SniConfig, StatsConfig,
    UpstreamProtocol, UpstreamServerConfig, UpstreamStrategy,
};
pub use tz::{LocalTime, PosixTz, TzError};

impl Config {
    /// Loads config with the documented precedence: defaults < file < `FAH__` env vars
    /// (CONFIGURATION.md). First boot (`path` doesn't exist): writes the default TOML to
    /// `path`, then proceeds with in-memory defaults as the file layer.
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        Self::load_inner(path, true)
    }

    /// Like [`Config::load`] but never writes to disk: a missing file yields
    /// in-memory defaults as the file layer instead of generating one. Used by
    /// `--healthcheck`, which must observe the effective config without the
    /// side effect of creating the very file whose absence signals a problem.
    pub fn load_readonly(path: &Path) -> Result<Config, ConfigError> {
        Self::load_inner(path, false)
    }

    fn load_inner(path: &Path, write_missing: bool) -> Result<Config, ConfigError> {
        let config = if path.exists() {
            let text = fs::read_to_string(path).map_err(|source| ConfigError::Io {
                path: path.to_path_buf(),
                source,
            })?;
            Config::from_toml_str(&text)?
        } else {
            let defaults = Config::default();
            if write_missing {
                let text = defaults.to_toml_string()?;
                write_atomic(path, &text)?;
            }
            defaults
        };

        let pairs = env::collect_env_pairs();
        let config = env::apply_env_overrides(config, &pairs)?;
        validate(&config)?;
        Ok(config)
    }

    /// Parses a TOML string into a `Config`, filling in documented defaults for any
    /// absent section or field. Does not run [`validate`] — callers that need a fully
    /// validated, effective config should go through [`Config::load`].
    pub fn from_toml_str(toml_str: &str) -> Result<Config, ConfigError> {
        Ok(toml::from_str(toml_str)?)
    }

    /// Serializes to a pretty TOML string, used for first-boot file generation.
    pub fn to_toml_string(&self) -> Result<String, ConfigError> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Runs the same semantic checks [`Config::load`] applies, on a config
    /// assembled some other way — `POST /api/v1/config` validates a merged
    /// candidate before persisting it (CONFIGURATION.md: "API changes are
    /// validated and written back to the file").
    pub fn validate(&self) -> Result<(), ConfigError> {
        validate(self)
    }

    /// Atomically writes this config back to `path` — the write-back half of
    /// `POST /api/v1/config` ("no hidden state; the file always reflects the
    /// running intent"). Same temp-file-then-rename as first-boot generation,
    /// so a crash mid-write can never truncate a working config.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        write_atomic(path, &self.to_toml_string()?)
    }
}

/// Writes `text` to `path` atomically: ensures the parent directory exists,
/// writes to a sibling temp file, then renames it into place. A crash or a
/// second instance booting concurrently can never observe a half-written
/// config — readers see either the old file or the complete new one.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), ConfigError> {
    let at = |p: &Path| {
        let p = p.to_path_buf();
        move |source| ConfigError::Io { path: p, source }
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(at(parent))?;
        }
    }

    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".tmp.{}", std::process::id()));
    let tmp = path.with_file_name(tmp_name);

    let tmp_err = at(&tmp);
    let path_err = at(path);

    fs::write(&tmp, text).map_err(tmp_err)?;
    fs::rename(&tmp, path).map_err(path_err)
}

/// Floor for `[dns.cache] max_bytes` — see the check in [`validate`].
const MIN_CACHE_MAX_BYTES: u64 = 1024 * 1024;

pub const MAX_UPSTREAM_SERVERS: usize = 8;

pub const MAX_HTTP_RUNTIMES: usize = 64;

fn validate(config: &Config) -> Result<(), ConfigError> {
    let dns_address = validate_ip("dns.listen.address", &config.dns.listen.address)?;
    let api_address = validate_ip("api.address", &config.api.address)?;
    // Validated unconditionally, not only when `engine.mode` includes http:
    // the file is written back whole, so a malformed `[http.listen]` should be
    // rejected while the operator is editing it, not on the restart months
    // later that first turns the mode on.
    let http_address = validate_ip("http.listen.address", &config.http.listen.address)?;
    let https_address = validate_ip("https.listen.address", &config.https.listen.address)?;

    validate_nonzero_port("dns.listen.port", config.dns.listen.port)?;
    validate_nonzero_port("api.port", config.api.port)?;
    validate_nonzero_port("http.listen.port", config.http.listen.port)?;
    validate_nonzero_port("https.listen.port", config.https.listen.port)?;
    if config.dns.listen.dot_enabled {
        validate_nonzero_port("dns.listen.dot_port", config.dns.listen.dot_port)?;
    }

    validate_listen_sockets(
        config,
        ListenAddresses {
            dns: dns_address,
            api: api_address,
            http: http_address,
            https: https_address,
        },
    )?;

    if config.https.max_connections == 0 {
        return Err(ConfigError::Validation {
            key: "https.max_connections",
            message: "must be at least 1".to_string(),
        });
    }
    if config.https.hello_timeout_ms == 0 {
        return Err(ConfigError::Validation {
            key: "https.hello_timeout_ms",
            message: "must be at least 1; 0 would close every connection before its ClientHello"
                .to_string(),
        });
    }
    if config.https.idle_timeout_ms == 0 {
        return Err(ConfigError::Validation {
            key: "https.idle_timeout_ms",
            message: "must be at least 1; 0 would close every spliced session at once".to_string(),
        });
    }

    // A ceiling of zero would accept nothing while looking configured.
    if config.http.max_connections == 0 {
        return Err(ConfigError::Validation {
            key: "http.max_connections",
            message: "must be at least 1".to_string(),
        });
    }
    if config.dns.tcp_max_connections == 0 {
        return Err(ConfigError::Validation {
            key: "dns.tcp_max_connections",
            message: "must be at least 1".to_string(),
        });
    }

    if config.runtime.http_runtimes > MAX_HTTP_RUNTIMES {
        return Err(ConfigError::Validation {
            key: "runtime.http_runtimes",
            message: format!("must be at most {MAX_HTTP_RUNTIMES}"),
        });
    }

    if config.dns.cache.min_ttl_seconds > config.dns.cache.max_ttl_seconds {
        return Err(ConfigError::Validation {
            key: "dns.cache.min_ttl_seconds",
            message: format!(
                "must be <= max_ttl_seconds ({} > {})",
                config.dns.cache.min_ttl_seconds, config.dns.cache.max_ttl_seconds
            ),
        });
    }

    // 1 MiB spread over 16 cache shards still leaves ~64 KiB per shard — one
    // maximum-size DNS answer. Below that a shard could be unable to hold a
    // single entry, turning every insert into an immediate eviction.
    if config.dns.cache.max_bytes < MIN_CACHE_MAX_BYTES {
        return Err(ConfigError::Validation {
            key: "dns.cache.max_bytes",
            message: format!(
                "must be at least {MIN_CACHE_MAX_BYTES} (got {})",
                config.dns.cache.max_bytes
            ),
        });
    }

    validate_range(
        "history.retention_days",
        config.history.retention_days,
        1,
        3650,
    )?;
    validate_range(
        "history.sample_interval_seconds",
        config.history.sample_interval_seconds,
        1,
        86_400,
    )?;

    validate_range(
        "dns.upstreams.timeout_ms",
        config.dns.upstreams.timeout_ms,
        1,
        10_000,
    )?;
    validate_range(
        "dns.upstreams.penalty_failures",
        config.dns.upstreams.penalty_failures,
        1,
        255,
    )?;

    if config.dns.upstreams.servers.is_empty() {
        return Err(ConfigError::Validation {
            key: "dns.upstreams.servers",
            message: "at least one upstream server is required".to_string(),
        });
    }
    if config.dns.upstreams.servers.len() > MAX_UPSTREAM_SERVERS {
        return Err(ConfigError::Validation {
            key: "dns.upstreams.servers",
            message: format!(
                "at most {MAX_UPSTREAM_SERVERS} upstream servers are supported, got {}",
                config.dns.upstreams.servers.len()
            ),
        });
    }
    for server in &config.dns.upstreams.servers {
        match server.protocol {
            UpstreamProtocol::Dot => {
                if server.hostname.as_deref().unwrap_or("").is_empty() {
                    return Err(ConfigError::Validation {
                        key: "dns.upstreams.servers.hostname",
                        message: format!(
                            "dot upstream {} requires a hostname for certificate verification",
                            server.address
                        ),
                    });
                }
            }
            // DoH takes its certificate name from the URL host
            // (CONFIGURATION.md's example carries no hostname); the optional
            // hostname only overrides it for IP-literal URLs.
            UpstreamProtocol::Doh => {
                if !server.address.starts_with("https://") {
                    return Err(ConfigError::Validation {
                        key: "dns.upstreams.servers.address",
                        message: format!("doh upstream {} must be an https:// URL", server.address),
                    });
                }
            }
            UpstreamProtocol::Udp => {}
        }
    }

    // Structural only — `fah_common::egress::AllowedNet` is the authority and
    // re-parses these at startup. `fah-config` is L1 and cannot import
    // `fah-common` (both L1, and layering.rs demands a strictly lower layer),
    // so the shape is checked here to keep `--healthcheck` honest: a typo that
    // only surfaced on the next real boot is the p1.5-07 defect all over again.
    for entry in &config.egress.allow_destinations {
        validate_allowed_destination(entry)?;
    }

    validate_policies(config)?;

    Ok(())
}

/// The ceiling on distinct policies, the default one included. Mirrors
/// `fah_model::PolicyId::MAX`, which this crate cannot name (sibling L1) — the
/// compiled ruleset packs policy membership into a `u16` per rule, and
/// `fah-rules` asserts the two constants agree.
const MAX_POLICIES: usize = 16;

/// `[schedule]` and `[[policies]]`. Everything here is checked while the
/// operator is editing rather than on the evening a schedule first matters:
/// a policy naming a list that does not exist, or a window whose times do not
/// parse, silently does nothing at runtime.
fn validate_policies(config: &Config) -> Result<(), ConfigError> {
    if let Err(source) = tz::PosixTz::parse(&config.schedule.timezone) {
        return Err(ConfigError::Validation {
            key: "schedule.timezone",
            message: format!("{source} (got {:?})", config.schedule.timezone),
        });
    }

    // The default policy always exists and always occupies one slot.
    if config.policies.len() + 1 > MAX_POLICIES {
        return Err(ConfigError::Validation {
            key: "policies",
            message: format!(
                "at most {} policies may be defined ({MAX_POLICIES} including the implicit \
                 default); got {}",
                MAX_POLICIES - 1,
                config.policies.len()
            ),
        });
    }

    let mut seen: Vec<&str> = Vec::with_capacity(config.policies.len());
    for policy in &config.policies {
        if policy.id.trim().is_empty() {
            return Err(ConfigError::Validation {
                key: "policies.id",
                message: "a policy id must not be empty".to_string(),
            });
        }
        // "default" is the implicit policy every unassigned client gets;
        // letting one be defined would leave two things answering to the name.
        if policy.id == "default" {
            return Err(ConfigError::Validation {
                key: "policies.id",
                message: "\"default\" is the implicit policy and cannot be redefined".to_string(),
            });
        }
        if seen.contains(&policy.id.as_str()) {
            return Err(ConfigError::Validation {
                key: "policies.id",
                message: format!("duplicate policy id {:?}", policy.id),
            });
        }
        seen.push(&policy.id);

        for list in policy.lists.iter().flatten() {
            if !config.rules.lists.iter().any(|entry| &entry.id == list) {
                return Err(ConfigError::Validation {
                    key: "policies.lists",
                    message: format!(
                        "policy {:?} references rule list {list:?}, which is not in [[rules.lists]]",
                        policy.id
                    ),
                });
            }
        }

        if let Some(mode) = &policy.blocking_mode {
            if mode.parse::<BlockingMode>().is_err() {
                return Err(ConfigError::Validation {
                    key: "policies.blocking_mode",
                    message: format!(
                        "policy {:?} sets blocking_mode {mode:?}; supported: null_ip",
                        policy.id
                    ),
                });
            }
        }

        for assignment in &policy.assignments {
            validate_assignment(&policy.id, assignment)?;
        }
    }

    Ok(())
}

fn validate_assignment(policy: &str, assignment: &AssignmentConfig) -> Result<(), ConfigError> {
    let invalid = |key: &'static str, message: String| ConfigError::Validation { key, message };

    if assignment.client.trim().is_empty() {
        return Err(invalid(
            "policies.assignments.client",
            format!("policy {policy:?} has an assignment with an empty client"),
        ));
    }
    // An address or prefix is recognized by parsing, anything else is a client
    // name — but a *malformed* address must not silently become a name nobody
    // will ever be called, so a `/` commits it to being a prefix.
    if let Some((address, prefix)) = assignment.client.split_once('/') {
        let Ok(address) = address.parse::<IpAddr>() else {
            return Err(invalid(
                "policies.assignments.client",
                format!("{:?} is not a CIDR block", assignment.client),
            ));
        };
        let max = if address.is_ipv4() { 32 } else { 128 };
        if !matches!(prefix.parse::<u8>(), Ok(len) if len <= max) {
            return Err(invalid(
                "policies.assignments.client",
                format!(
                    "{:?} has an invalid prefix length (max /{max})",
                    assignment.client
                ),
            ));
        }
    }

    if let Some(days) = &assignment.days {
        parse_days(days).map_err(|message| {
            invalid(
                "policies.assignments.days",
                format!("policy {policy:?}: {message}"),
            )
        })?;
    }

    match (&assignment.start, &assignment.end) {
        (None, None) => {}
        (Some(start), Some(end)) => {
            for (key, value) in [
                ("policies.assignments.start", start),
                ("policies.assignments.end", end),
            ] {
                parse_time_of_day(value)
                    .map_err(|message| invalid(key, format!("policy {policy:?}: {message}")))?;
            }
        }
        _ => {
            return Err(invalid(
                "policies.assignments.start",
                format!(
                    "policy {policy:?}: a schedule needs both start and end (a half-open window \
                     would silently never close)"
                ),
            ));
        }
    }

    Ok(())
}

/// One `[egress] allow_destinations` entry: an IP address, optionally with a
/// CIDR prefix.
fn validate_allowed_destination(entry: &str) -> Result<(), ConfigError> {
    const KEY: &str = "egress.allow_destinations";
    let (address, prefix) = match entry.split_once('/') {
        Some((address, prefix)) => (address, Some(prefix)),
        None => (entry, None),
    };
    let Ok(address) = address.parse::<std::net::IpAddr>() else {
        return Err(ConfigError::Validation {
            key: KEY,
            message: format!("{entry:?} is not an IP address or CIDR block"),
        });
    };
    let Some(prefix) = prefix else {
        return Ok(());
    };
    let max = if address.is_ipv4() { 32 } else { 128 };
    match prefix.parse::<u8>() {
        Ok(len) if len <= max => Ok(()),
        _ => Err(ConfigError::Validation {
            key: KEY,
            message: format!("{entry:?} has an invalid prefix length (max /{max})"),
        }),
    }
}

fn validate_ip(key: &'static str, address: &str) -> Result<IpAddr, ConfigError> {
    address
        .parse::<IpAddr>()
        .map_err(|source| ConfigError::Validation {
            key,
            message: source.to_string(),
        })
}

struct ListenAddresses {
    dns: IpAddr,
    api: IpAddr,
    http: IpAddr,
    https: IpAddr,
}

struct ListenSocket {
    key: &'static str,
    label: &'static str,
    addr: SocketAddr,
    active: bool,
}

fn is_dual_stack(address: IpAddr) -> bool {
    matches!(address, IpAddr::V6(v6) if v6.is_unspecified())
}

fn addresses_overlap(a: IpAddr, b: IpAddr) -> bool {
    if a == b || is_dual_stack(a) || is_dual_stack(b) {
        return true;
    }
    a.is_ipv4() == b.is_ipv4() && (a.is_unspecified() || b.is_unspecified())
}

fn validate_listen_sockets(config: &Config, addresses: ListenAddresses) -> Result<(), ConfigError> {
    let listen = &config.dns.listen;
    let sockets = [
        ListenSocket {
            key: "dns.listen.port",
            label: "[dns.listen] port",
            addr: SocketAddr::new(addresses.dns, listen.port),
            active: true,
        },
        ListenSocket {
            key: "api.port",
            label: "[api] port",
            addr: SocketAddr::new(addresses.api, config.api.port),
            active: true,
        },
        ListenSocket {
            key: "http.listen.port",
            label: "[http.listen] port",
            addr: SocketAddr::new(addresses.http, config.http.listen.port),
            active: config.engine.mode.serves_http(),
        },
        ListenSocket {
            key: "https.listen.port",
            label: "[https.listen] port",
            addr: SocketAddr::new(addresses.https, config.https.listen.port),
            active: config.engine.mode.serves_https(),
        },
        ListenSocket {
            key: "dns.listen.dot_port",
            label: "[dns.listen] dot_port",
            addr: SocketAddr::new(addresses.dns, listen.dot_port),
            active: listen.dot_enabled,
        },
    ];

    for (index, later) in sockets.iter().enumerate() {
        if !later.active {
            continue;
        }
        for earlier in sockets[..index].iter().filter(|socket| socket.active) {
            if later.addr.port() == earlier.addr.port()
                && addresses_overlap(later.addr.ip(), earlier.addr.ip())
            {
                return Err(ConfigError::Validation {
                    key: later.key,
                    message: format!(
                        "{} overlaps {} ({}); two TCP listeners cannot bind one socket",
                        later.addr, earlier.label, earlier.addr
                    ),
                });
            }
        }
    }

    Ok(())
}

fn validate_nonzero_port(key: &'static str, port: u16) -> Result<(), ConfigError> {
    if port == 0 {
        return Err(ConfigError::Validation {
            key,
            message: "must be a non-zero port".to_string(),
        });
    }
    Ok(())
}

/// Rejects `0` and absurd values on an inclusive `[min, max]` bound — the
/// history intervals, where `0` would busy-loop or drop everything on the next
/// prune, and an absurd value is almost certainly a typo.
fn validate_range(key: &'static str, value: u32, min: u32, max: u32) -> Result<(), ConfigError> {
    if value < min || value > max {
        return Err(ConfigError::Validation {
            key,
            message: format!("must be between {min} and {max} (got {value})"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::apply_env_overrides;

    #[test]
    fn defaults_match_configuration_md_sample() {
        let config = Config::default();
        assert_eq!(config.dns.cache.max_entries, 10_000);
        assert_eq!(config.dns.cache.max_bytes, 64 * 1024 * 1024);
        assert_eq!(config.api.port, 8443);
        assert_eq!(config.log.level, LogLevel::Info);
        assert_eq!(config.dns.upstreams.servers.len(), 2);
        assert_eq!(config.dns.upstreams.servers[0].address, "1.1.1.1");
        assert_eq!(
            config.dns.upstreams.servers[0].protocol,
            UpstreamProtocol::Udp
        );
        assert_eq!(config.dns.upstreams.servers[1].address, "9.9.9.9");
        assert_eq!(config.rules.lists.len(), 1);
        assert_eq!(config.rules.lists[0].id, "oisd-basic");
        assert_eq!(config.rules.lists[0].url, "https://small.oisd.nl");
    }

    #[test]
    fn parses_full_reference_toml_verbatim() {
        let toml_str = r#"
[engine]
mode = "dns"

[dns.listen]
address = "::"
port = 53
dot_enabled = true
dot_port = 853
doh_enabled = true

[dns.blocking]
mode = "null_ip"
ttl_seconds = 10

[dns.cache]
max_entries = 10000
max_bytes = 67108864
min_ttl_seconds = 0
max_ttl_seconds = 86400
negative_ttl_max_seconds = 60
serve_stale = true
swr_workers = 3
cleanup_interval_seconds = 360

[dns.upstreams]
strategy = "adaptive"
timeout_ms = 800

[[dns.upstreams.servers]]
address = "1.1.1.1"
protocol = "udp"

[[dns.upstreams.servers]]
address = "9.9.9.9"
protocol = "udp"

[rules]
refresh_hours_default = 24

[[rules.lists]]
id = "oisd-basic"
url = "https://small.oisd.nl"
enabled = true

[stats]
snapshot_interval_seconds = 300

[history]
enabled = true
sample_interval_seconds = 60
retention_days = 30

[api]
address = "0.0.0.0"
port = 8443
tls = true

[log]
level = "info"
format = "text"
"#;
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config, Config::default());
    }

    /// Stale-while-refresh must be on out of the box: a feature that needs a
    /// hand-edited TOML to work ships off for almost everyone. `0` is the
    /// documented way to turn it off and has to keep parsing.
    #[test]
    fn stale_while_refresh_is_on_by_default_and_zero_disables_it() {
        assert_eq!(DnsCacheConfig::default().swr_workers, 3);

        let disabled = Config::from_toml_str("[dns.cache]\nswr_workers = 0\n").unwrap();
        assert_eq!(disabled.dns.cache.swr_workers, 0);
        assert!(
            disabled.dns.cache.serve_stale,
            "disabling the refresh pool must not disable serve-stale itself"
        );
    }

    /// Same contract for the scheduled sweep: on by default, `0` the
    /// documented off switch. Disabling it must not touch the caps — those
    /// are what keep the cache bounded, and the sweep only returns memory
    /// underneath them.
    #[test]
    fn cache_cleanup_is_on_by_default_and_zero_disables_it() {
        assert_eq!(DnsCacheConfig::default().cleanup_interval_seconds, 360);

        let disabled =
            Config::from_toml_str("[dns.cache]\ncleanup_interval_seconds = 0\n").unwrap();
        assert_eq!(disabled.dns.cache.cleanup_interval_seconds, 0);
        assert_eq!(disabled.dns.cache.max_entries, 10_000);
        assert_eq!(disabled.dns.cache.max_bytes, 64 * 1024 * 1024);
    }

    #[test]
    fn partial_section_override_keeps_other_fields_default() {
        let config = Config::from_toml_str("[dns.cache]\nmax_entries = 500\n").unwrap();
        assert_eq!(config.dns.cache.max_entries, 500);
        assert_eq!(config.dns.cache.max_ttl_seconds, 86_400);
        assert_eq!(config.dns.listen, DnsListenConfig::default());
    }

    #[test]
    fn missing_section_uses_full_section_defaults() {
        let config = Config::from_toml_str("[engine]\nmode = \"dns+http\"\n").unwrap();
        assert_eq!(config.engine.mode, EngineMode::DnsHttp);
        assert_eq!(config.dns, DnsConfig::default());
        assert_eq!(config.api, ApiConfig::default());
    }

    #[test]
    fn encrypted_dns_listeners_default_on_with_dot_on_853() {
        let config = Config::default();
        assert!(config.dns.listen.dot_enabled);
        assert_eq!(config.dns.listen.dot_port, 853);
        assert!(config.dns.listen.doh_enabled);

        let config =
            Config::from_toml_str("[dns.listen]\ndot_enabled = false\ndoh_enabled = false\n")
                .unwrap();
        assert!(!config.dns.listen.dot_enabled);
        assert!(!config.dns.listen.doh_enabled);
        assert_eq!(config.dns.listen.dot_port, 853);
    }

    #[test]
    fn dot_port_env_override_applies() {
        let pairs = vec![("FAH__DNS__LISTEN__DOT_PORT".to_string(), "8853".to_string())];
        let config = apply_env_overrides(Config::default(), &pairs).unwrap();
        assert_eq!(config.dns.listen.dot_port, 8853);
    }

    #[test]
    fn dot_port_must_not_collide_with_another_listener_unless_dot_is_off() {
        let validated = |toml: &str| Config::from_toml_str(toml).unwrap().validate();

        let text = validated("[dns.listen]\ndot_port = 8443\n")
            .unwrap_err()
            .to_string();
        assert!(text.contains("dns.listen.dot_port"), "got: {text}");
        assert!(text.contains("[api] port"), "got: {text}");

        let text = validated("[dns.listen]\ndot_port = 0\n")
            .unwrap_err()
            .to_string();
        assert!(text.contains("dns.listen.dot_port"), "got: {text}");

        validated("[dns.listen]\ndot_enabled = false\ndot_port = 8443\n")
            .expect("a disabled DoT listener binds nothing, so its port cannot collide");
    }

    #[test]
    fn a_config_without_an_https_section_still_parses() {
        let config = Config::from_toml_str("[dns.listen]\nport = 5353\n").unwrap();
        assert_eq!(config.https, HttpsConfig::default());
        assert_eq!(config.https.listen.port, 8444);
    }

    #[test]
    fn a_zero_https_timeout_is_rejected_rather_than_read_literally() {
        for (key, toml) in [
            ("https.hello_timeout_ms", "[https]\nhello_timeout_ms = 0\n"),
            ("https.idle_timeout_ms", "[https]\nidle_timeout_ms = 0\n"),
        ] {
            let err = Config::from_toml_str(toml).unwrap().validate().unwrap_err();
            assert!(err.to_string().contains(key), "{key}: {err}");
        }
    }

    #[test]
    fn the_https_listener_may_not_share_another_listener_port() {
        let full = "[engine]\nmode = \"dns+http+https\"\n";
        for (section, toml) in [
            ("[api] port", format!("{full}[https.listen]\nport = 8443\n")),
            (
                "[http.listen] port",
                format!("{full}[https.listen]\nport = 8080\n"),
            ),
            (
                "[dns.listen] port",
                format!("{full}[dns.listen]\nport = 5353\n[https.listen]\nport = 5353\n"),
            ),
        ] {
            let config = Config::from_toml_str(&toml).unwrap();
            let message = config.validate().unwrap_err().to_string();
            assert!(message.contains("https.listen.port"), "{message}");
            assert!(message.contains(section), "{message}");
        }
    }

    #[test]
    fn every_active_listener_pair_is_compared_for_a_port_collision() {
        let full = "[engine]\nmode = \"dns+http+https\"\n";
        for (key, section, toml) in [
            (
                "api.port",
                "[dns.listen] port",
                format!("{full}[dns.listen]\nport = 9000\n[api]\nport = 9000\n"),
            ),
            (
                "http.listen.port",
                "[dns.listen] port",
                format!("{full}[dns.listen]\nport = 9000\n[http.listen]\nport = 9000\n"),
            ),
            (
                "http.listen.port",
                "[api] port",
                format!("{full}[api]\nport = 9000\n[http.listen]\nport = 9000\n"),
            ),
            (
                "https.listen.port",
                "[http.listen] port",
                format!("{full}[http.listen]\nport = 9000\n[https.listen]\nport = 9000\n"),
            ),
            (
                "dns.listen.dot_port",
                "[https.listen] port",
                format!("{full}[dns.listen]\ndot_port = 9000\n[https.listen]\nport = 9000\n"),
            ),
        ] {
            let config = Config::from_toml_str(&toml).unwrap();
            let message = config.validate().unwrap_err().to_string();
            assert!(message.contains(key), "expected key {key}: {message}");
            assert!(
                message.contains(section),
                "expected the other endpoint {section}: {message}"
            );
        }
    }

    #[test]
    fn a_collision_message_names_both_endpoints_with_address_and_port() {
        let toml = "[dns.listen]\naddress = \"0.0.0.0\"\nport = 9000\n[api]\naddress = \
                    \"0.0.0.0\"\nport = 9000\n";
        let message = Config::from_toml_str(toml)
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string();
        assert!(message.contains("0.0.0.0:9000"), "{message}");
        assert!(message.contains("[dns.listen] port"), "{message}");
        assert!(message.contains("api.port"), "{message}");
    }

    #[test]
    fn a_listener_that_does_not_bind_cannot_collide() {
        for toml in [
            "[api]\nport = 8080\n",
            "[engine]\nmode = \"dns+http\"\n[http.listen]\nport = 8444\n",
            "[dns.listen]\ndot_enabled = false\ndot_port = 8443\n",
            "[dns.listen]\ndot_enabled = false\ndot_port = 53\n",
        ] {
            Config::from_toml_str(toml)
                .unwrap()
                .validate()
                .unwrap_or_else(|err| panic!("{toml:?} binds no overlapping pair: {err}"));
        }
    }

    #[test]
    fn enabling_a_listener_is_what_makes_a_parked_collision_fail() {
        let parked = "[api]\nport = 8080\n";
        Config::from_toml_str(parked)
            .unwrap()
            .validate()
            .expect("in dns mode the HTTP listener never binds");

        let enabled = format!("[engine]\nmode = \"dns+http\"\n{parked}");
        let message = Config::from_toml_str(&enabled)
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string();
        assert!(message.contains("http.listen.port"), "{message}");
        assert!(message.contains("[api] port"), "{message}");
    }

    #[test]
    fn two_listeners_on_disjoint_addresses_may_share_a_port() {
        for toml in [
            "[engine]\nmode = \"dns+http+https\"\n[https.listen]\naddress = \
             \"192.168.1.1\"\nport = 9000\n[api]\naddress = \"10.0.0.1\"\nport = 9000\n",
            "[api]\naddress = \"127.0.0.1\"\nport = 9000\n[dns.listen]\naddress = \
             \"::1\"\nport = 9000\n",
            "[api]\naddress = \"0.0.0.0\"\nport = 9000\n[dns.listen]\naddress = \
             \"fd00::1\"\nport = 9000\n",
        ] {
            Config::from_toml_str(toml)
                .unwrap()
                .validate()
                .unwrap_or_else(|err| panic!("{toml:?} cannot conflict: {err}"));
        }
    }

    #[test]
    fn a_wildcard_address_overlaps_the_concrete_ones_it_covers() {
        for (dns, api) in [
            ("0.0.0.0", "192.168.1.1"),
            ("192.168.1.1", "0.0.0.0"),
            ("::", "192.168.1.1"),
            ("::", "fd00::1"),
            ("fd00::1", "::"),
            ("::", "0.0.0.0"),
            ("0.0.0.0", "::"),
            ("::", "::"),
            ("127.0.0.1", "127.0.0.1"),
        ] {
            let toml = format!(
                "[dns.listen]\naddress = \"{dns}\"\nport = 9000\n[api]\naddress = \
                 \"{api}\"\nport = 9000\n"
            );
            let err = Config::from_toml_str(&toml)
                .unwrap()
                .validate()
                .unwrap_err();
            assert!(
                err.to_string().contains("api.port"),
                "{dns} and {api} overlap: {err}"
            );
        }
    }

    #[test]
    fn address_overlap_is_decided_by_family_and_wildcards() {
        let ip = |text: &str| text.parse::<IpAddr>().unwrap();

        assert!(addresses_overlap(ip("::"), ip("192.168.1.1")));
        assert!(addresses_overlap(ip("::"), ip("0.0.0.0")));
        assert!(addresses_overlap(ip("::"), ip("fd00::1")));
        assert!(addresses_overlap(ip("0.0.0.0"), ip("192.168.1.1")));
        assert!(addresses_overlap(ip("127.0.0.1"), ip("127.0.0.1")));

        assert!(!addresses_overlap(ip("0.0.0.0"), ip("fd00::1")));
        assert!(!addresses_overlap(ip("192.168.1.1"), ip("10.0.0.1")));
        assert!(!addresses_overlap(ip("::1"), ip("fd00::1")));
        assert!(!addresses_overlap(ip("127.0.0.1"), ip("::1")));
    }

    #[test]
    fn unknown_top_level_section_is_rejected() {
        let err = Config::from_toml_str("[bogus]\nx = 1\n").unwrap_err();
        assert!(err.to_string().contains("bogus"));
    }

    #[test]
    fn unknown_nested_key_is_rejected() {
        let err = Config::from_toml_str("[dns.cache]\nmax_entrees = 500\n").unwrap_err();
        assert!(err.to_string().contains("max_entrees"));
    }

    #[test]
    fn engine_mode_rejects_unknown_variant() {
        let err = Config::from_toml_str("[engine]\nmode = \"dns+bogus\"\n").unwrap_err();
        assert!(err.to_string().contains("dns+bogus"));
    }

    #[test]
    fn upstream_server_missing_address_is_rejected() {
        let err =
            Config::from_toml_str("[[dns.upstreams.servers]]\nprotocol = \"udp\"\n").unwrap_err();
        assert!(err.to_string().contains("address"));
    }

    #[test]
    fn history_defaults_match_configuration_md_sample() {
        let history = Config::default().history;
        assert!(history.enabled);
        assert_eq!(history.sample_interval_seconds, 60);
        assert_eq!(history.retention_days, 30);
    }

    #[test]
    fn history_retention_days_zero_is_rejected() {
        let mut config = Config::default();
        config.history.retention_days = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("history.retention_days"));
    }

    #[test]
    fn history_absurd_values_are_rejected() {
        let mut config = Config::default();
        config.history.retention_days = 1_000_000;
        assert!(config.validate().is_err());

        let mut config = Config::default();
        config.history.sample_interval_seconds = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn dns_tcp_max_connections_zero_is_rejected() {
        let mut config = Config::default();
        config.dns.tcp_max_connections = 0;
        assert!(matches!(
            validate(&config).unwrap_err(),
            ConfigError::Validation {
                key: "dns.tcp_max_connections",
                ..
            }
        ));
    }

    #[test]
    fn history_env_override_applies() {
        let pairs = vec![("FAH__HISTORY__RETENTION_DAYS".to_string(), "90".to_string())];
        let config = apply_env_overrides(Config::default(), &pairs).unwrap();
        assert_eq!(config.history.retention_days, 90);
    }

    #[test]
    fn runtime_env_override_applies_and_is_validated() {
        let pairs = vec![("FAH__RUNTIME__HTTP_RUNTIMES".to_string(), "2".to_string())];
        let config = apply_env_overrides(Config::default(), &pairs).unwrap();
        assert_eq!(config.runtime.http_runtimes, 2);

        let pairs = vec![("FAH__RUNTIME__HTTP_RUNTIMES".to_string(), "x".to_string())];
        assert!(apply_env_overrides(Config::default(), &pairs).is_err());

        let pairs = vec![("FAH__RUNTIME__HTTP_RUNTIMES".to_string(), "65".to_string())];
        let config = apply_env_overrides(Config::default(), &pairs).unwrap();
        assert!(matches!(
            validate(&config).unwrap_err(),
            ConfigError::Validation {
                key: "runtime.http_runtimes",
                ..
            }
        ));
    }

    #[test]
    fn dns_tcp_max_connections_env_override_applies_and_is_validated() {
        let pairs = vec![(
            "FAH__DNS__TCP_MAX_CONNECTIONS".to_string(),
            "512".to_string(),
        )];
        let config = apply_env_overrides(Config::default(), &pairs).unwrap();
        assert_eq!(config.dns.tcp_max_connections, 512);

        let pairs = vec![("FAH__DNS__TCP_MAX_CONNECTIONS".to_string(), "x".to_string())];
        assert!(matches!(
            apply_env_overrides(Config::default(), &pairs).unwrap_err(),
            ConfigError::InvalidEnvValue { .. }
        ));

        let pairs = vec![("FAH__DNS__TCP_MAX_CONNECTIONS".to_string(), "0".to_string())];
        let config = apply_env_overrides(Config::default(), &pairs).unwrap();
        assert!(matches!(
            validate(&config).unwrap_err(),
            ConfigError::Validation {
                key: "dns.tcp_max_connections",
                ..
            }
        ));
    }

    #[test]
    fn dns_udp_max_inflight_env_override_applies_and_zero_stays_no_cap() {
        let pairs = vec![("FAH__DNS__UDP_MAX_INFLIGHT".to_string(), "4096".to_string())];
        let config = apply_env_overrides(Config::default(), &pairs).unwrap();
        assert_eq!(config.dns.udp_max_inflight, 4096);
        validate(&config).unwrap();

        let pairs = vec![("FAH__DNS__UDP_MAX_INFLIGHT".to_string(), "0".to_string())];
        let config = apply_env_overrides(Config::default(), &pairs).unwrap();
        assert_eq!(config.dns.udp_max_inflight, 0);
        validate(&config).unwrap();

        let pairs = vec![("FAH__DNS__UDP_MAX_INFLIGHT".to_string(), "x".to_string())];
        assert!(matches!(
            apply_env_overrides(Config::default(), &pairs).unwrap_err(),
            ConfigError::InvalidEnvValue { .. }
        ));
    }

    #[test]
    fn every_env_variable_configuration_md_documents_has_an_override_arm() {
        let doc = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../CONFIGURATION.md");
        let text = std::fs::read_to_string(&doc)
            .unwrap_or_else(|err| panic!("read {}: {err}", doc.display()));

        let documented: Vec<String> = text
            .lines()
            .filter_map(|line| line.split("Env: ").nth(1))
            .map(|rest| {
                rest.split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string()
            })
            .filter(|var| var.starts_with("FAH__"))
            .collect();

        assert!(
            documented.len() >= 3,
            "the `Env:` scan found {} names; CONFIGURATION.md changed shape and this test \
             stopped checking anything",
            documented.len()
        );

        for var in documented {
            let pairs = vec![(var.clone(), "1".to_string())];
            let outcome = apply_env_overrides(Config::default(), &pairs);
            assert!(
                !matches!(outcome, Err(ConfigError::UnknownEnvKey { .. })),
                "CONFIGURATION.md documents `Env: {var}` but `env::apply_one` has no arm for it, \
                 so setting it fails the whole config load instead of overriding the key"
            );
        }
    }

    #[test]
    fn env_override_wins_over_file_and_defaults() {
        let config = Config::from_toml_str("[dns.cache]\nmax_entries = 500\n").unwrap();
        let pairs = vec![(
            "FAH__DNS__CACHE__MAX_ENTRIES".to_string(),
            "100000".to_string(),
        )];
        let config = apply_env_overrides(config, &pairs).unwrap();
        assert_eq!(config.dns.cache.max_entries, 100_000);
    }

    #[test]
    fn env_bool_and_numeric_coercion_succeeds() {
        let pairs = vec![
            ("FAH__API__TLS".to_string(), "false".to_string()),
            ("FAH__API__PORT".to_string(), "9443".to_string()),
        ];
        let config = apply_env_overrides(Config::default(), &pairs).unwrap();
        assert!(!config.api.tls);
        assert_eq!(config.api.port, 9443);
    }

    #[test]
    fn env_invalid_numeric_value_names_key_and_expected_form() {
        let pairs = vec![("FAH__API__PORT".to_string(), "notanumber".to_string())];
        let err = apply_env_overrides(Config::default(), &pairs).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("FAH__API__PORT"));
        assert!(message.contains("u16"));
    }

    #[test]
    fn env_unknown_key_is_rejected() {
        let pairs = vec![("FAH__DNS__TYPOX".to_string(), "1".to_string())];
        let err = apply_env_overrides(Config::default(), &pairs).unwrap_err();
        assert!(matches!(err, ConfigError::UnknownEnvKey { .. }));
    }

    #[test]
    fn env_array_of_tables_paths_rejected_with_explanation() {
        let pairs = vec![("FAH__DNS__UPSTREAMS__SERVERS".to_string(), "x".to_string())];
        let err = apply_env_overrides(Config::default(), &pairs).unwrap_err();
        assert!(err.to_string().contains("array-of-tables"));

        let pairs = vec![("FAH__RULES__LISTS".to_string(), "x".to_string())];
        let err = apply_env_overrides(Config::default(), &pairs).unwrap_err();
        assert!(err.to_string().contains("array-of-tables"));
    }

    #[test]
    fn env_interception_paths_are_rejected() {
        for var in [
            "FAH__HTTPS__INTERCEPTION__CLIENTS",
            "FAH__HTTPS__INTERCEPTION__EXCLUDE_DOMAINS",
        ] {
            let pairs = vec![(var.to_string(), "192.168.88.10".to_string())];
            let err = apply_env_overrides(Config::default(), &pairs).unwrap_err();
            assert!(
                matches!(err, ConfigError::UnknownEnvKey { .. }),
                "{var}: {err}"
            );
        }
    }

    #[test]
    fn interception_keys_are_absent_from_a_default_toml() {
        let text = Config::default().to_toml_string().unwrap();
        assert!(!text.contains("interception"), "{text}");
    }

    #[test]
    fn from_toml_str_is_the_file_layer_alone() {
        let text = "[https.interception]\nclients = [\"10.0.0.1\"]\n\n[api]\nport = 8443\n";
        let config = Config::from_toml_str(text).unwrap();
        assert_eq!(
            config.https.interception.clients,
            Some(vec!["10.0.0.1".to_string()])
        );
        assert_eq!(config.api.port, 8443);

        let pairs = vec![("FAH__API__PORT".to_string(), "9443".to_string())];
        let effective = apply_env_overrides(Config::from_toml_str(text).unwrap(), &pairs).unwrap();
        assert_eq!(effective.api.port, 9443);
        assert_eq!(Config::from_toml_str(text).unwrap().api.port, 8443);
    }

    #[test]
    fn write_atomic_error_paths_are_unchanged() {
        let dir = tempfile::tempdir().unwrap();

        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "not a directory").unwrap();
        let err = write_atomic(&blocker.join("nested.toml"), "x").unwrap_err();
        assert!(
            matches!(&err, ConfigError::Io { path, .. } if path == &blocker),
            "{err:?}"
        );

        let occupied = dir.path().join("occupied.toml");
        fs::create_dir(&occupied).unwrap();
        let err = write_atomic(&occupied, "x").unwrap_err();
        assert!(
            matches!(&err, ConfigError::Io { path, .. } if path == &occupied),
            "{err:?}"
        );

        let good = dir.path().join("nested").join("fastadhunter.toml");
        write_atomic(&good, "value = 1\n").unwrap();
        assert_eq!(fs::read_to_string(&good).unwrap(), "value = 1\n");
    }

    #[test]
    fn first_boot_generates_file_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fastadhunter.toml");
        let config = Config::load(&path).unwrap();
        assert_eq!(config, Config::default());

        let written = fs::read_to_string(&path).unwrap();
        let reparsed = Config::from_toml_str(&written).unwrap();
        assert_eq!(reparsed, Config::default());
    }

    #[test]
    fn existing_file_is_left_untouched_and_used() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fastadhunter.toml");
        let custom = "[dns.cache]\nmax_entries = 42\n";
        fs::write(&path, custom).unwrap();

        let config = Config::load(&path).unwrap();
        assert_eq!(config.dns.cache.max_entries, 42);
        assert_eq!(fs::read_to_string(&path).unwrap(), custom);
    }

    #[test]
    fn to_toml_string_round_trips_through_from_toml_str() {
        let original = Config::default();
        let text = original.to_toml_string().unwrap();
        let reparsed = Config::from_toml_str(&text).unwrap();
        assert_eq!(original, reparsed);
    }

    #[test]
    fn validation_rejects_min_ttl_greater_than_max_ttl() {
        let toml_str = "[dns.cache]\nmin_ttl_seconds = 100\nmax_ttl_seconds = 10\n";
        let config = Config::from_toml_str(toml_str).unwrap();
        let err = validate(&config).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation {
                key: "dns.cache.min_ttl_seconds",
                ..
            }
        ));
    }

    #[test]
    fn validation_rejects_a_cache_byte_cap_below_one_entry_per_shard() {
        // 64 KiB over 16 shards is 4 KiB each — under a single large answer,
        // which would make every insert evict what it just stored.
        let config = Config::from_toml_str("[dns.cache]\nmax_bytes = 65536\n").unwrap();
        let err = validate(&config).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation {
                key: "dns.cache.max_bytes",
                ..
            }
        ));
    }

    #[test]
    fn validation_rejects_more_upstreams_than_an_endpoint_index_can_name() {
        let mut config = Config::default();
        let template = config.dns.upstreams.servers[0].clone();
        config.dns.upstreams.servers = vec![template; MAX_UPSTREAM_SERVERS];
        validate(&config).expect("the cap itself must still be accepted");

        config
            .dns
            .upstreams
            .servers
            .push(config.dns.upstreams.servers[0].clone());
        let err = validate(&config).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation {
                key: "dns.upstreams.servers",
                ..
            }
        ));
    }

    #[test]
    fn upstream_penalty_failures_defaults_and_round_trips() {
        let config = Config::from_toml_str("[dns.upstreams]\ntimeout_ms = 800\n").unwrap();
        assert_eq!(config.dns.upstreams.penalty_failures, 2);
        assert_eq!(config.dns.upstreams.strategy, UpstreamStrategy::Adaptive);
        validate(&config).unwrap();

        let config = Config::from_toml_str(
            "[dns.upstreams]\nstrategy = \"adaptive\"\npenalty_failures = 3\n",
        )
        .unwrap();
        assert_eq!(config.dns.upstreams.strategy, UpstreamStrategy::Adaptive);
        assert_eq!(config.dns.upstreams.penalty_failures, 3);
        validate(&config).unwrap();

        let text = config.to_toml_string().unwrap();
        let reparsed = Config::from_toml_str(&text).unwrap();
        assert_eq!(reparsed.dns.upstreams, config.dns.upstreams);
    }

    #[test]
    fn validation_rejects_zero_penalty_failures() {
        let config = Config::from_toml_str("[dns.upstreams]\npenalty_failures = 0\n").unwrap();
        let err = validate(&config).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation {
                key: "dns.upstreams.penalty_failures",
                ..
            }
        ));
    }

    #[test]
    fn validation_bounds_upstream_timeout_ms() {
        for value in ["0", "10001"] {
            let config =
                Config::from_toml_str(&format!("[dns.upstreams]\ntimeout_ms = {value}\n")).unwrap();
            let err = validate(&config).unwrap_err();
            assert!(matches!(
                err,
                ConfigError::Validation {
                    key: "dns.upstreams.timeout_ms",
                    ..
                }
            ));
        }

        let config = Config::from_toml_str("[dns.upstreams]\ntimeout_ms = 10000\n").unwrap();
        validate(&config).unwrap();
    }

    #[test]
    fn upstream_env_overrides_apply() {
        let pairs = vec![
            (
                "FAH__DNS__UPSTREAMS__STRATEGY".to_string(),
                "adaptive".to_string(),
            ),
            (
                "FAH__DNS__UPSTREAMS__PENALTY_FAILURES".to_string(),
                "3".to_string(),
            ),
        ];
        let config = apply_env_overrides(Config::default(), &pairs).unwrap();
        assert_eq!(config.dns.upstreams.strategy, UpstreamStrategy::Adaptive);
        assert_eq!(config.dns.upstreams.penalty_failures, 3);
    }

    #[test]
    fn upstream_env_invalid_values_are_rejected() {
        let pairs = vec![(
            "FAH__DNS__UPSTREAMS__PENALTY_FAILURES".to_string(),
            "abc".to_string(),
        )];
        let err = apply_env_overrides(Config::default(), &pairs).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidEnvValue { .. }));

        let pairs = vec![(
            "FAH__DNS__UPSTREAMS__STRATEGY".to_string(),
            "nope".to_string(),
        )];
        let message = apply_env_overrides(Config::default(), &pairs)
            .unwrap_err()
            .to_string();
        assert!(message.contains("adaptive"));

        let pairs = vec![(
            "FAH__DNS__UPSTREAMS__STRATEGY".to_string(),
            "fallback".to_string(),
        )];
        let message = apply_env_overrides(Config::default(), &pairs)
            .unwrap_err()
            .to_string();
        assert!(message.contains("removed after 0.3.3"), "{message}");
    }

    #[test]
    fn the_removed_fallback_strategy_is_rejected_at_load() {
        let err = Config::from_toml_str(
            r#"
[dns.upstreams]
strategy = "fallback"
"#,
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("removed after 0.3.3"), "{message}");
        assert!(message.contains("adaptive"), "{message}");
    }

    #[test]
    fn validation_rejects_unparseable_bind_address() {
        let toml_str = "[dns.listen]\naddress = \"not-an-ip\"\n";
        let config = Config::from_toml_str(toml_str).unwrap();
        let err = validate(&config).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation {
                key: "dns.listen.address",
                ..
            }
        ));
    }

    #[test]
    fn validation_rejects_zero_port() {
        let config = Config::from_toml_str("[dns.listen]\nport = 0\n").unwrap();
        let err = validate(&config).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation {
                key: "dns.listen.port",
                ..
            }
        ));
    }

    /// Default-deny is the security property; assert the parsed default keeps
    /// it, not just the struct's `Default`.
    #[test]
    fn egress_allow_list_is_empty_unless_configured() {
        let config = Config::from_toml_str("").unwrap();
        assert!(config.egress.allow_destinations.is_empty());
    }

    #[test]
    fn egress_allow_list_accepts_addresses_and_cidr_blocks() {
        let toml_str = "[egress]\nallow_destinations = [\"192.168.10.50\", \
             \"192.168.10.0/24\", \"fd00::/8\", \"::1\"]\n";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.egress.allow_destinations.len(), 4);
        assert!(validate(&config).is_ok());
    }

    /// A typo here silently widens or voids an egress exception, so it must
    /// fail at `--healthcheck` rather than on the next real boot.
    #[test]
    fn egress_allow_list_rejects_malformed_entries() {
        for bad in [
            "not-an-ip",
            "192.168.10.0/33",
            "fd00::/129",
            "192.168.10.0/x",
            "",
        ] {
            let mut config = Config::default();
            config.egress.allow_destinations = vec![bad.to_string()];
            let err = validate(&config).unwrap_err();
            assert!(
                matches!(err, ConfigError::Validation { key, .. } if key == "egress.allow_destinations"),
                "{bad:?} must be rejected by key, got {err:?}"
            );
        }
    }

    #[test]
    fn validation_rejects_too_many_http_runtimes() {
        let mut config = Config::default();
        config.runtime.http_runtimes = MAX_HTTP_RUNTIMES + 1;
        let err = validate(&config).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation {
                key: "runtime.http_runtimes",
                ..
            }
        ));
        config.runtime.http_runtimes = MAX_HTTP_RUNTIMES;
        validate(&config).unwrap();
        config.runtime.http_runtimes = 0;
        validate(&config).unwrap();
    }

    #[test]
    fn validation_rejects_empty_upstreams() {
        let mut config = Config::default();
        config.dns.upstreams.servers.clear();
        let err = validate(&config).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation {
                key: "dns.upstreams.servers",
                ..
            }
        ));
    }

    #[test]
    fn validation_rejects_dot_upstream_without_hostname() {
        let toml_str = "[[dns.upstreams.servers]]\naddress = \"1.1.1.1\"\nprotocol = \"dot\"\n";
        let config = Config::from_toml_str(toml_str).unwrap();
        let err = validate(&config).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation {
                key: "dns.upstreams.servers.hostname",
                ..
            }
        ));
    }

    #[test]
    fn validation_accepts_dot_upstream_with_hostname() {
        let toml_str = "[[dns.upstreams.servers]]\naddress = \"1.1.1.1\"\nprotocol = \"dot\"\nhostname = \"cloudflare-dns.com\"\n";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert!(validate(&config).is_ok());
    }

    #[test]
    fn validation_accepts_doh_upstream_without_hostname() {
        // CONFIGURATION.md's documented DoH example carries no hostname —
        // the certificate name comes from the URL host.
        let toml_str = "[[dns.upstreams.servers]]\naddress = \"https://cloudflare-dns.com/dns-query\"\nprotocol = \"doh\"\n";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert!(validate(&config).is_ok());
    }

    #[test]
    fn validation_rejects_doh_upstream_without_https_url() {
        let toml_str = "[[dns.upstreams.servers]]\naddress = \"1.1.1.1\"\nprotocol = \"doh\"\n";
        let config = Config::from_toml_str(toml_str).unwrap();
        let err = validate(&config).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation {
                key: "dns.upstreams.servers.address",
                ..
            }
        ));
    }

    #[test]
    fn readonly_load_does_not_create_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fastadhunter.toml");
        let config = Config::load_readonly(&path).unwrap();
        assert_eq!(config, Config::default());
        assert!(!path.exists(), "healthcheck load must not write to disk");
    }

    #[test]
    fn first_boot_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deeper").join("fah.toml");
        let config = Config::load(&path).unwrap();
        assert_eq!(config, Config::default());
        assert!(path.exists());
    }
}
