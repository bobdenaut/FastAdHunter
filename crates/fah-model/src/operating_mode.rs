use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The engine's filtering scope, fixed at container start (CONTEXT.md:
/// Operating Mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatingMode {
    #[serde(rename = "dns")]
    Dns,
    #[serde(rename = "dns+http")]
    DnsHttp,
    #[serde(rename = "dns+http+https")]
    DnsHttpHttps,
}

impl fmt::Display for OperatingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            OperatingMode::Dns => "dns",
            OperatingMode::DnsHttp => "dns+http",
            OperatingMode::DnsHttpHttps => "dns+http+https",
        })
    }
}

/// A config string didn't match `dns`, `dns+http` or `dns+http+https`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseOperatingModeError(pub String);

impl fmt::Display for ParseOperatingModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid operating mode: {:?}", self.0)
    }
}

impl std::error::Error for ParseOperatingModeError {}

impl FromStr for OperatingMode {
    type Err = ParseOperatingModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dns" => Ok(OperatingMode::Dns),
            "dns+http" => Ok(OperatingMode::DnsHttp),
            "dns+http+https" => Ok(OperatingMode::DnsHttpHttps),
            other => Err(ParseOperatingModeError(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_from_config_strings() {
        assert_eq!("dns".parse::<OperatingMode>().unwrap(), OperatingMode::Dns);
        assert_eq!(
            "dns+http".parse::<OperatingMode>().unwrap(),
            OperatingMode::DnsHttp
        );
        assert_eq!(
            "dns+http+https".parse::<OperatingMode>().unwrap(),
            OperatingMode::DnsHttpHttps
        );
        assert!("dns+https".parse::<OperatingMode>().is_err());
    }

    #[test]
    fn display_matches_config_strings() {
        assert_eq!(OperatingMode::Dns.to_string(), "dns");
        assert_eq!(OperatingMode::DnsHttp.to_string(), "dns+http");
        assert_eq!(OperatingMode::DnsHttpHttps.to_string(), "dns+http+https");
    }

    #[test]
    fn serde_roundtrip_matches_config_strings() {
        for mode in [
            OperatingMode::Dns,
            OperatingMode::DnsHttp,
            OperatingMode::DnsHttpHttps,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, format!("{:?}", mode.to_string()));
            let back: OperatingMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }
}
