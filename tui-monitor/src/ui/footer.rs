//! The footer: the router's own figures, on one line.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::state::{AppState, LinkStatus, RouterStatus};
use crate::util::format::{bytes, percent, spaced_uptime, truncate};

use super::theme;

/// Longest a router failure may print before the line stops fitting.
const REASON_WIDTH: usize = 32;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    frame.render_widget(
        Paragraph::new(router_line(&state.router)).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn router_line<'a>(router: &RouterStatus) -> Line<'a> {
    let mut spans = vec![Span::styled(" RouterOS ", theme::heading())];

    // The figures below are last-known-good. Without this a dead REST endpoint
    // reads as a frozen-but-healthy router.
    if let LinkStatus::Down(reason) = &router.link {
        spans.push(Span::styled(
            format!("✕ {} │ ", truncate(reason, REASON_WIDTH)),
            theme::strong(theme::BLOCKED),
        ));
    }

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

    // The device's uptime. The header carries the FastAdHunter process's, which
    // is a different clock — a container restart moves one and not the other.
    if let Some(uptime) = router.uptime.as_deref() {
        spans.push(Span::raw(format!(" │ up {}", spaced_uptime(uptime))));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            uptime: Some("2d23h57m20s".to_string()),
            ..Default::default()
        };
        let rendered = text(&router_line(&router));

        assert!(rendered.contains("750.0 MiB of 1.0 GiB"), "{rendered}");
        assert!(rendered.contains("CPU 3%"), "{rendered}");
        assert!(rendered.contains("(running)"), "{rendered}");
        // The device's clock, not the process's — the header carries that one.
        assert!(rendered.contains("up 2d 23h 57m 20s"), "{rendered}");
    }

    /// Router figures are last-known-good, so a failing poll has to say so —
    /// otherwise a dead REST endpoint is indistinguishable from a quiet router.
    #[test]
    fn a_failing_router_poll_is_marked_beside_its_stale_figures() {
        let router = RouterStatus {
            link: LinkStatus::Down("error decoding response body".to_string()),
            free_memory: Some(786_432_000),
            total_memory: Some(1_073_741_824),
            ..Default::default()
        };
        let rendered = text(&router_line(&router));

        assert!(rendered.contains('✕'), "{rendered}");
        assert!(rendered.contains("error decoding"), "{rendered}");
        assert!(rendered.contains("750.0 MiB"), "the last reading stays");
    }
}
