use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RuntimeConfig {
    #[serde(default = "default_http_runtimes")]
    pub http_runtimes: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            http_runtimes: default_http_runtimes(),
        }
    }
}

fn default_http_runtimes() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| (cores.get() / 2).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_half_the_cores_and_never_zero() {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        assert_eq!(RuntimeConfig::default().http_runtimes, (cores / 2).max(1));
        assert!(RuntimeConfig::default().http_runtimes >= 1);
    }

    #[test]
    fn zero_is_accepted_as_the_shared_runtime_switch() {
        let parsed: RuntimeConfig = toml::from_str("http_runtimes = 0").unwrap();
        assert_eq!(parsed.http_runtimes, 0);
    }
}
