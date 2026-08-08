//! The left column: live rates, the two history windows and the top-N tables.
//! Scrollable, because it is longer than any terminal.
//!
//! Engine, cache, memory and upstream figures live in [`super::details`] — they
//! are read by glancing, and here they sat past the fold.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

use crate::state::{AppState, LinkStatus, Window};
use crate::util::format::{thousands, truncate};

use super::{chart, gauge, theme};

/// Widest a name may print before it is elided.
const NAME_WIDTH: usize = 27;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState, scroll: u16) -> u16 {
    // Resizing can hand a panel any rectangle, and ratatui's Scrollbar panics
    // on an empty one. A frame too small to hold the border has nothing to say.
    if area.width < 2 || area.height < 2 {
        return 0;
    }

    let inner_width = area.width.saturating_sub(2) as usize;
    let bar_width = inner_width.saturating_sub(22).max(10);

    // "Last 24 h", not "Today": the endpoint's default window is rolling, so
    // this covers the same clock hour yesterday, not the time since midnight.
    let mut lines = live_rates(state, bar_width);
    lines.extend(window_section(
        "Last 24 h",
        state.last_24h.as_ref(),
        &state.history,
        inner_width,
    ));
    lines.extend(window_section(
        "Last 7 days",
        state.week.as_ref(),
        &state.history,
        inner_width,
    ));
    lines.extend(top_sections(state));

    // The scrollbar's range is the overflow, so the last line is reachable and
    // no further.
    let viewport = area.height.saturating_sub(2);
    let max_scroll = (lines.len() as u16).saturating_sub(viewport);
    let scroll = scroll.min(max_scroll);

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Metrics & Top Stats"),
            )
            .scroll((scroll, 0)),
        area,
    );

    let mut scrollbar_state = ScrollbarState::new(max_scroll as usize).position(scroll as usize);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼")),
        area,
        &mut scrollbar_state,
    );

    max_scroll
}

fn live_rates<'a>(state: &AppState, bar_width: usize) -> Vec<Line<'a>> {
    let live = &state.live;

    vec![
        rate_line(
            "Blocked Rate:",
            live.blocked_percent,
            bar_width,
            theme::BLOCKED,
        ),
        // "all q" distinguishes this from the header's Hit bar, which divides
        // by cache lookups instead.
        rate_line(
            "Hit (all q): ",
            live.cache_hit_percent,
            bar_width,
            theme::OK,
        ),
        Line::from(format!(
            "Total: {} │ Blocked: {}",
            thousands(live.queries_total),
            thousands(live.blocked_total)
        )),
    ]
}

fn rate_line<'a>(
    label: &'static str,
    value: f64,
    bar_width: usize,
    colour: ratatui::style::Color,
) -> Line<'a> {
    let mut spans = vec![Span::styled(label, theme::strong(colour)), Span::raw(" ")];
    spans.extend(gauge::bar(value, bar_width, colour));
    spans.push(Span::styled(
        format!(" {value:5.1}%"),
        theme::strong(colour),
    ));
    Line::from(spans)
}

fn window_section<'a>(
    title: &str,
    window: Option<&Window>,
    link: &LinkStatus,
    width: usize,
) -> Vec<Line<'a>> {
    let mut lines = vec![Line::from(""), heading(title)];

    let Some(window) = window else {
        // A failing poller reads as "no data yet" forever otherwise, which is
        // indistinguishable from an appliance that has simply not filled a
        // bucket.
        let (text, style) = match link {
            LinkStatus::Down(reason) => (
                format!("  {}", truncate(reason, NAME_WIDTH)),
                theme::strong(theme::BLOCKED),
            ),
            _ => ("  (no data yet)".to_string(), theme::label()),
        };
        lines.push(Line::from(Span::styled(text, style)));
        return lines;
    };

    lines.push(counter_line("Total Queries", window.queries, None));
    lines.push(counter_line(
        "Blocked",
        window.blocked,
        Some(window.percent_of_queries(window.blocked)),
    ));
    lines.push(counter_line(
        "Cache Hits",
        window.cache_hits,
        Some(window.percent_of_queries(window.cache_hits)),
    ));
    // Whatever record types the window holds, most-used first — the label set
    // is the API's, not a hard-coded A/AAAA/HTTPS.
    for (kind, count) in window.types_by_count().into_iter().take(4) {
        lines.push(counter_line(
            kind,
            count,
            Some(window.percent_of_queries(count)),
        ));
    }

    let spark = chart::sparkline(&window.series, width.saturating_sub(4));
    if !spark.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  {spark}"),
            ratatui::style::Style::default().fg(theme::ACCENT),
        )));
    }
    lines.push(Line::from(Span::styled(
        format!("  {}", window.coverage()),
        theme::label(),
    )));
    lines
}

fn counter_line<'a>(label: &str, value: u64, share: Option<f64>) -> Line<'a> {
    match share {
        Some(share) => Line::from(format!(
            " • {label:<14} {:>10} ({share:>5.1}%)",
            thousands(value)
        )),
        None => Line::from(format!(" • {label:<14} {:>10}", thousands(value))),
    }
}

fn top_sections<'a>(state: &AppState) -> Vec<Line<'a>> {
    let live = &state.live;
    let mut lines = Vec::new();

    lines.extend(top_list(
        "Top Blocked Domains",
        live.top_blocked_domains
            .iter()
            .map(|d| (truncate(&d.domain, NAME_WIDTH), d.count)),
    ));
    lines.extend(top_list(
        "Top Clients",
        live.top_clients
            .iter()
            // `label()` builds a String, so the truncation of it has to be
            // owned too — it cannot borrow a temporary that dies here.
            .map(|c| (truncate(&c.label(), NAME_WIDTH).into_owned(), c.count)),
    ));
    lines.extend(top_list(
        "Top Queried Domains",
        live.top_queried_domains
            .iter()
            .map(|d| (truncate(&d.domain, NAME_WIDTH), d.count)),
    ));
    lines
}

/// Generic over the name so a borrowed [`truncate`] result can be passed
/// straight through — `format!` consumes it by `Display` either way.
fn top_list<'a>(
    title: &str,
    rows: impl Iterator<Item = (impl std::fmt::Display, u64)>,
) -> Vec<Line<'a>> {
    let mut lines = vec![Line::from(""), heading(title)];
    for (name, count) in rows {
        lines.push(Line::from(format!(
            " • {name:<NAME_WIDTH$} {:>8}",
            thousands(count)
        )));
    }
    lines
}

fn heading<'a>(title: &str) -> Line<'a> {
    Line::from(Span::styled(format!("── {title} ──"), theme::heading()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::fixtures;

    fn text(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_window_with_no_data_says_so_instead_of_printing_zeros() {
        let rendered = text(&window_section(
            "Last 24 h",
            None,
            &LinkStatus::Connecting,
            40,
        ));
        assert!(rendered.contains("no data yet"), "{rendered}");
        assert!(!rendered.contains('0'), "{rendered}");
    }

    /// "No data yet" is true for a poller that has not answered *yet*. Once it
    /// is failing, saying so is the difference between "the appliance has not
    /// filled a bucket" and "this panel has been broken for an hour".
    #[test]
    fn a_failing_history_poll_names_the_error_instead_of_waiting_forever() {
        let rendered = text(&window_section(
            "Last 24 h",
            None,
            &LinkStatus::Down("unauthorized (check the token)".to_string()),
            40,
        ));

        assert!(rendered.contains("unauthorized"), "{rendered}");
        assert!(!rendered.contains("no data yet"), "{rendered}");
    }

    #[test]
    fn a_window_lists_the_record_types_the_api_actually_returned() {
        let summary: crate::models::history::HistorySummary =
            serde_json::from_str(fixtures::HISTORY_SUMMARY).unwrap();
        let window = Window::from_summary(&summary);
        let rendered = text(&window_section(
            "Last 24 h",
            Some(&window),
            &LinkStatus::Online,
            40,
        ));

        assert!(rendered.contains("Total Queries"), "{rendered}");
        // 5312 + 5077 + 4500
        assert!(rendered.contains("14,889"), "{rendered}");
        assert!(rendered.contains(" • A "), "{rendered}");
        assert!(rendered.contains("HTTPS"), "{rendered}");
        assert!(rendered.contains("3 buckets, hourly"), "{rendered}");
    }
}
