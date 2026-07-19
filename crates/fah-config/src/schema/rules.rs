use serde::{Deserialize, Serialize};

use crate::schema::default_true;

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
        refresh_hours: None,
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
    /// Per-list override of `[rules] refresh_hours_default` (CONFIGURATION.md:
    /// "per-list override via API" — `PATCH /api/v1/lists/{id}`). Absent means
    /// "follow the default", so a later change to the default still applies.
    #[serde(default)]
    pub refresh_hours: Option<u32>,
}
