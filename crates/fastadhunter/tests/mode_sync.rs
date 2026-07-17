//! Drift guard for the two parallel mode enums: `fah_config::EngineMode` and
//! `fah_model::OperatingMode`. They are duplicated on purpose — L1 sibling
//! crates can't import each other (CLAUDE.md hard rule) — so nothing but this
//! test keeps them in lockstep. The exhaustive matches below stop compiling if
//! a variant is added to one enum but not the other, and the assertions catch
//! any mismatch in the canonical wire strings.

use fah_config::EngineMode;
use fah_model::OperatingMode;

fn engine_mode_str(mode: EngineMode) -> &'static str {
    match mode {
        EngineMode::Dns => "dns",
        EngineMode::DnsHttp => "dns+http",
        EngineMode::DnsHttpHttps => "dns+http+https",
    }
}

fn operating_mode_str(mode: OperatingMode) -> &'static str {
    match mode {
        OperatingMode::Dns => "dns",
        OperatingMode::DnsHttp => "dns+http",
        OperatingMode::DnsHttpHttps => "dns+http+https",
    }
}

#[test]
fn every_engine_mode_maps_to_the_same_operating_mode_string() {
    for mode in [
        EngineMode::Dns,
        EngineMode::DnsHttp,
        EngineMode::DnsHttpHttps,
    ] {
        let s = engine_mode_str(mode);
        let parsed: OperatingMode = s
            .parse()
            .unwrap_or_else(|_| panic!("OperatingMode must accept EngineMode string `{s}`"));
        assert_eq!(operating_mode_str(parsed), s);
    }
}

#[test]
fn both_enums_reject_the_same_unknown_string() {
    assert!("dns+bogus".parse::<EngineMode>().is_err());
    assert!("dns+bogus".parse::<OperatingMode>().is_err());
}
