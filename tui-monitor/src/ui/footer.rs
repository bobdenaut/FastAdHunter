//! The footer: the router's own figures, and the key bindings.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::state::{AppState, RouterStatus};
use crate::util::format::{bytes, percent, thousands};

use super::theme;

const KEYS: &str = "↑/↓ PgUp/PgDn feed │ w/s stats │ Enter details │ q quit";

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let lines = vec![router_line(&state.router), engine_line(state)];

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn router_line<'a>(router: &RouterStatus) -> Line<'a> {
    let mut spans = vec![Span::styled(" RouterOS ", theme::heading())];

    match (router.free_memory, router.total_memory) {
        (Some(free), Some(total)) if total > 0 => {
            let used = percent(total.saturating_sub(free), total);
            spans.push(Span::raw("free "));
            spans.push(Span::styled(
                bytes(free),
                ratatui::style::Style::default().fg(theme::saturation(used)),
            ));
            spans.push(Span::raw(format!(" of {} │ ", bytes(total))));
        }
        (Some(free), _) => spans.push(Span::raw(format!("free {} │ ", bytes(free)))),
        _ => spans.push(Span::raw("free — │ ")),
    }

    spans.push(Span::raw(format!(
        "CPU {} │ container {}",
        router.cpu_load.map_or("—".to_string(), |l| format!("{l}%")),
        router.container_memory.map_or("—".to_string(), bytes),
    )));

    if let Some(status) = router.container_status.as_deref() {
        let running = status == "running";
        spans.push(Span::raw(" ("));
        spans.push(Span::styled(
            status.to_string(),
            ratatui::style::Style::default().fg(theme::link(running)),
        ));
        spans.push(Span::raw(")"));
    }

    if let Some(uptime) = router.uptime.as_deref() {
        spans.push(Span::raw(format!(" │ up {uptime}")));
    }
    Line::from(spans)
}

/// Upstream attempt totals beside the key bindings — the one engine figure
/// worth a permanent line rather than a scroll away.
fn engine_line<'a>(state: &AppState) -> Line<'a> {
    let upstreams = state
        .telemetry
        .as_ref()
        .map(|telemetry| {
            telemetry
                .engine
                .upstreams
                .iter()
                .map(|u| format!("{}: {}", u.address, thousands(u.attempts)))
                .collect::<Vec<_>>()
                .join(" │ ")
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "no upstream data".to_string());

    Line::from(vec![
        Span::raw(format!(" {upstreams} │ ")),
        Span::styled(KEYS, theme::label()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::fixtures;

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Before the RouterOS worker's first tick — and forever, when no
    /// `[routeros]` section is configured.
    #[test]
    fn an_unconfigured_router_prints_dashes_rather_than_zeros() {
        let rendered = text(&router_line(&RouterStatus::default()));

        assert!(rendered.contains("free —"), "{rendered}");
        assert!(rendered.contains("CPU —"), "{rendered}");
        assert!(rendered.contains("container —"), "{rendered}");
    }

    #[test]
    fn a_populated_router_reports_free_memory_against_the_total() {
        let router = RouterStatus {
            free_memory: Some(786_432_000),
            total_memory: Some(1_073_741_824),
            cpu_load: Some(3),
            container_memory: Some(89_346_048),
            container_status: Some("running".to_string()),
            uptime: Some("3d04:12:55".to_string()),
            ..Default::default()
        };
        let rendered = text(&router_line(&router));

        assert!(rendered.contains("750.0 MB of 1.0 GB"), "{rendered}");
        assert!(rendered.contains("CPU 3%"), "{rendered}");
        assert!(rendered.contains("(running)"), "{rendered}");
        assert!(rendered.contains("up 3d04:12:55"), "{rendered}");
    }

    #[test]
    fn the_engine_line_lists_every_upstream_and_the_keys() {
        let mut state = AppState::default();
        state.telemetry = Some(fixtures::telemetry());
        let rendered = text(&engine_line(&state));

        assert!(rendered.contains("1.1.1.1:853: 201,883"), "{rendered}");
        assert!(rendered.contains("9.9.9.9:853: 4,332"), "{rendered}");
        assert!(rendered.contains("q quit"), "{rendered}");
        assert!(rendered.contains("q quit"), "{rendered}");
    }

    #[test]
    fn the_engine_line_says_so_before_the_first_poll() {
        let rendered = text(&engine_line(&AppState::default()));
        assert!(rendered.contains("no upstream data"), "{rendered}");
    }
}
