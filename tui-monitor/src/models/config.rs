use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AppliedConfig {
    pub dns: DnsSection,
}

#[derive(Debug, Deserialize)]
pub struct DnsSection {
    pub upstreams: UpstreamsSection,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamsSection {
    pub strategy: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_do_not_break_the_read() {
        let json = r#"{
            "engine": {"mode": "dns+http"},
            "dns": {
                "listen": {"address": "::", "port": 53},
                "upstreams": {
                    "strategy": "adaptive",
                    "timeout_ms": 800,
                    "penalty_failures": 2,
                    "servers": [{"address": "1.1.1.1", "protocol": "udp"}]
                }
            },
            "api": {"port": 8443}
        }"#;

        let config: AppliedConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.dns.upstreams.strategy, "adaptive");
    }

    #[test]
    fn fallback_reads_as_written() {
        let json = r#"{"dns": {"upstreams": {"strategy": "fallback"}}}"#;
        let config: AppliedConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.dns.upstreams.strategy, "fallback");
    }
}
