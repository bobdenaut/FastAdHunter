use serde::{Deserialize, Serialize};

/// `[rules]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RulesConfig {
    #[serde(default = "default_refresh_hours_default")]
    pub refresh_hours_default: u32,
    #[serde(default = "default_lists")]
    pub lists: Vec<RuleListConfig>,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            refresh_hours_default: default_refresh_hours_default(),
            lists: default_lists(),
        }
    }
}

fn default_refresh_hours_default() -> u32 {
    24
}

fn default_lists() -> Vec<RuleListConfig> {
    vec![RuleListConfig {
        id: "oisd-basic".to_string(),
        url: "https://small.oisd.nl".to_string(),
        enabled: true,
    }]
}

/// One `[[rules.lists]]` entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleListConfig {
    pub id: String,
    pub url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}
