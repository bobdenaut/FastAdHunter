//! Every colour and text style, named by meaning rather than hue: a panel asks
//! for `BLOCKED`, not for red.

use ratatui::style::{Color, Modifier, Style};

use crate::models::events::Verdict;

pub const ACCENT: Color = Color::Cyan;
pub const OK: Color = Color::Green;
pub const WARN: Color = Color::Yellow;
pub const BLOCKED: Color = Color::Red;
pub const MUTED: Color = Color::DarkGray;
/// The unfilled part of a gauge — dim enough to read as a track rather than as
/// data.
pub const TRACK: Color = Color::Rgb(45, 50, 60);

/// Colour for one RSS reading in MiB. Applied per graph column, so the chart
/// shows *when* memory crossed a threshold rather than only where it is now —
/// the chart autoscales, so its rows carry no fixed magnitude.
pub fn rss(megabytes: f64, thresholds: crate::config::RssThresholds) -> Color {
    if megabytes >= thresholds.alert_mb {
        BLOCKED
    } else if megabytes >= thresholds.warn_mb {
        WARN
    } else {
        OK
    }
}

pub fn heading() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn label() -> Style {
    Style::default().fg(MUTED)
}

pub fn strong(color: Color) -> Style {
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub fn verdict(verdict: Verdict) -> Color {
    match verdict {
        Verdict::Block => BLOCKED,
        Verdict::Allow => WARN,
        Verdict::Pass => OK,
        Verdict::Unknown => MUTED,
    }
}

/// Green while a link is answering, red once it is not.
pub fn link(online: bool) -> Color {
    if online {
        OK
    } else {
        BLOCKED
    }
}

/// Green / amber / red by how full something is. Used for cache load and for
/// the router's own memory, so "nearly full" looks the same everywhere.
pub fn saturation(percent: f64) -> Color {
    match percent {
        p if p >= 90.0 => BLOCKED,
        p if p >= 70.0 => WARN,
        _ => OK,
    }
}
