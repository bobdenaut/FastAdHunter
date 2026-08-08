//! The right column: the engine's reference figures, one box each.
//!
//! These move slowly and are read by glancing. Below the two history windows in
//! the left panel they sat past the fold, so the figures worth checking when
//! something looks wrong were the ones needing a scroll to reach.
//!
//! One box per subject rather than one panel with rules: the border is the
//! separator, and a box keeps its frame even when the column is scrolled part
//! way past it.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Widget,
};
use ratatui::Frame;

use crate::models::telemetry::Telemetry;
use crate::state::AppState;
use crate::util::format::{bytes, mean_ms, mib, percent, thousands, truncate};

use super::theme;

/// Order is deliberate: upstream health first because it is the one that
/// explains an outage, then the three memory/cache readings that explain a
/// slow drift.
///
/// Returns the largest useful scroll offset, so the caller can clamp against a
/// column whose height just changed.
pub fn render(frame: &mut Frame, area: Rect, state: &AppState, scroll: u16) -> u16 {
    // Resizing can hand a panel any rectangle, and ratatui's Scrollbar panics
    // on an empty one.
    if area.width < 2 || area.height < 2 {
        return 0;
    }

    let sections: Vec<(&str, Vec<Line>)> = match state.telemetry.as_ref() {
        Some(telemetry) => vec![
            ("Upstreams", upstream_lines(telemetry)),
            ("Memory", memory_lines(telemetry)),
            ("Cache", cache_lines(telemetry)),
            ("Engine", engine_lines(telemetry)),
        ],
        // Before the first poll every box still draws, so the column does not
        // appear and disappear as the appliance answers.
        None => ["Upstreams", "Memory", "Cache", "Engine"]
            .into_iter()
            .map(|title| (title, vec![waiting()]))
            .collect(),
    };

    // Each box is exactly its content plus a border, and the stack is drawn at
    // its full height whatever the terminal can show.
    let constraints: Vec<Constraint> = sections
        .iter()
        .map(|(_, lines)| Constraint::Length(lines.len() as u16 + 2))
        .collect();
    let content: u16 = sections
        .iter()
        .map(|(_, lines)| lines.len() as u16 + 2)
        .sum();

    let max_scroll = content.saturating_sub(area.height);
    let scroll = scroll.min(max_scroll);

    // Drawn off-screen at full height, then blitted with the offset applied.
    // Scrolling a `Paragraph` would clip each box's border away with it; this
    // scrolls the stack as a picture, so a part-visible box keeps its frame.
    let canvas_area = Rect::new(0, 0, area.width, content.max(area.height));
    let mut canvas = Buffer::empty(canvas_area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(canvas_area);

    for ((title, lines), row) in sections.into_iter().zip(rows.iter()) {
        Paragraph::new(lines)
            .block(boxed(title))
            .render(*row, &mut canvas);
    }

    let target = frame.buffer_mut();
    for y in 0..area.height.min(content.saturating_sub(scroll)) {
        for x in 0..area.width {
            *target.get_mut(area.x + x, area.y + y) = canvas.get(x, y + scroll).clone();
        }
    }

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

/// `┌─ Memory ───┐`, not `┌Memory──────┐`. The border is the separator here —
/// the left panel's `── Heading ──` rules exist only because it has none — so
/// the title sits *inside* the rule with air on both sides.
///
/// Three spans, not one styled string: the word carries the left panel's
/// heading style and the rule around it stays border-coloured.
fn boxed<'a>(title: &str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .title(Line::from(vec![
            Span::raw("─ "),
            Span::styled(title.to_string(), theme::heading()),
            Span::raw(" "),
        ]))
}

fn waiting<'a>() -> Line<'a> {
    Line::from(Span::styled("  (no data yet)", theme::label()))
}

fn upstream_lines<'a>(telemetry: &Telemetry) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    for upstream in &telemetry.engine.upstreams {
        let failure_rate = percent(upstream.failures, upstream.attempts);
        let healthy = upstream.consecutive_failures == 0;
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", if healthy { "●" } else { "✕" }),
                ratatui::style::Style::default().fg(theme::link(healthy)),
            ),
            Span::raw(format!(
                "{:<26} {:>14}",
                truncate(&upstream.address, 26),
                thousands(upstream.attempts)
            )),
        ]));
        lines.push(Line::from(Span::styled(
            format!(
                "     {} fail {} ({failure_rate:.2}%), streak {}",
                protocol(upstream.protocol),
                upstream.failures,
                upstream.consecutive_failures
            ),
            theme::label(),
        )));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  none configured",
            theme::label(),
        )));
    }
    lines
}

fn memory_lines<'a>(telemetry: &Telemetry) -> Vec<Line<'a>> {
    let memory = &telemetry.memory;
    let components = &memory.components;
    let stats = components.stats_aggregates_bytes + components.stats_clients_bytes;

    let mut lines = Vec::new();
    for (label, value) in [
        ("Ruleset", Some(components.ruleset_bytes)),
        ("Cache", Some(components.cache_estimated_bytes)),
        ("Stats", Some(stats)),
        ("Accounted", Some(components.accounted_bytes)),
        ("Residual", components.residual_bytes),
    ] {
        lines.push(figure(
            label,
            value.map_or("—".to_string(), |v| format!("{:.1} MiB", mib(v))),
        ));
    }

    lines.push(figure("Cached entries", thousands(memory.cache_entries)));
    // A rising major count is the appliance swapping, which no other figure
    // on this screen would show.
    for (label, value) in [
        ("Page faults maj", memory.major_page_faults),
        ("Page faults min", memory.minor_page_faults),
    ] {
        lines.push(figure(label, value.map_or("—".to_string(), thousands)));
    }
    lines
}

fn cache_lines<'a>(telemetry: &Telemetry) -> Vec<Line<'a>> {
    let cache = &telemetry.cache;

    vec![
        figure("Fresh", thousands(cache.fresh)),
        figure("Stale", thousands(cache.stale)),
        figure("Expired", thousands(cache.expired)),
        figure("Evictions", thousands(cache.evictions)),
        figure(
            "Bytes",
            format!("{} / {}", bytes(cache.bytes), bytes(cache.max_bytes)),
        ),
        // Two ceilings bound this cache — entries and bytes. Whichever is
        // fuller is the one that will start evicting.
        figure(
            "Load",
            format!(
                "{:.1}% e / {:.1}% b",
                cache.load_percent, cache.byte_load_percent
            ),
        ),
    ]
}

fn engine_lines<'a>(telemetry: &Telemetry) -> Vec<Line<'a>> {
    let counters = &telemetry.engine.counters;
    let latency = &telemetry.engine.latency.dns;

    // The three stages partition every resolved query, so they are read
    // together: only `forward` crosses the network, and since 0.2.13 an SWR
    // stale serve is timed as the cache read it is, not as a forward.
    let mut lines = vec![
        figure("Lat block", mean_ms(latency.block)),
        figure("Lat cache hit", mean_ms(latency.cache_hit)),
        figure("Lat forward", mean_ms(latency.forward)),
        figure("SWR done", thousands(counters.swr.completed)),
        figure("SWR failed", thousands(counters.swr.failed)),
        figure("Sweeps", thousands(counters.cache_cleanup.runs)),
        figure("Swept", bytes(counters.cache_cleanup.bytes_freed)),
    ];

    // Non-zero means Statistics and Metrics have both under-counted, so every
    // figure on this screen is low by at least this much.
    if counters.events_dropped > 0 {
        lines.push(Line::from(Span::styled(
            format!(" ! events dropped: {}", counters.events_dropped),
            theme::strong(theme::BLOCKED),
        )));
    }
    if counters.http.pass + counters.http.block > 0 {
        lines.push(figure("HTTP blocked", thousands(counters.http.block)));
        lines.push(figure("HTTP relayed", bytes(counters.http.response_bytes)));
    }
    lines
}

/// One labelled figure. The two fields fill the box exactly — `" • "` plus 16
/// plus a space plus 24 is the 44 columns inside a
/// [`super::layout::SIDE_WIDTH`] border — so every value lands flush on the
/// right edge.
fn figure<'a>(label: &str, value: String) -> Line<'a> {
    Line::from(format!(" • {label:<16} {value:>24}"))
}

fn protocol(protocol: fah_model::Protocol) -> &'static str {
    match protocol {
        fah_model::Protocol::Udp => "udp",
        fah_model::Protocol::Dot => "dot",
        fah_model::Protocol::Doh => "doh",
        fah_model::Protocol::Unknown => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::fixtures;
    use crate::ui::layout::SIDE_WIDTH;

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

    /// Both ceilings are shown: whichever is fuller is the one that evicts.
    #[test]
    fn the_cache_box_reports_the_entry_and_byte_loads_separately() {
        let rendered = text(&cache_lines(&fixtures::telemetry()));

        assert!(rendered.contains("2.6% e / 2.4% b"), "{rendered}");
        assert!(rendered.contains("Evictions"), "{rendered}");
    }

    #[test]
    fn an_unhealthy_upstream_is_marked_and_a_healthy_one_is_not() {
        let rendered = text(&upstream_lines(&fixtures::telemetry()));

        assert!(rendered.contains("● 1.1.1.1:853"), "{rendered}");
        assert!(rendered.contains("✕ 9.9.9.9:853"), "{rendered}");
        assert!(rendered.contains("streak 3"), "{rendered}");
    }

    /// The fixture has none dropped, so the warning must be absent — it is a
    /// line that only appears when something is wrong.
    #[test]
    fn the_dropped_events_warning_appears_only_when_events_were_dropped() {
        let mut telemetry = fixtures::telemetry();
        assert!(!text(&engine_lines(&telemetry)).contains("events dropped"));

        telemetry.engine.counters.events_dropped = 7;
        assert!(text(&engine_lines(&telemetry)).contains("events dropped: 7"));
    }

    #[test]
    fn memory_prints_a_dash_when_the_residual_cannot_be_derived() {
        let mut telemetry = fixtures::telemetry();
        telemetry.memory.components.residual_bytes = None;
        let rendered = text(&memory_lines(&telemetry));

        assert!(rendered.contains("Residual"), "{rendered}");
        assert!(rendered.contains('—'), "{rendered}");
    }

    /// The four boxes must read as one set: same inset, same spacing, in the
    /// order upstream-health-first. A bare `.title()` butts the word against
    /// the corner and the column stops looking deliberate.
    #[test]
    fn every_box_carries_its_title_inset_in_the_rule() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let state = crate::state::SharedState::new(crate::config::UiConfig::default().limits());
        state.update(|app| app.telemetry = Some(fixtures::telemetry()));

        let mut terminal = Terminal::new(TestBackend::new(SIDE_WIDTH, 60)).unwrap();
        terminal
            .draw(|frame| {
                render(frame, frame.size(), &state.read(), 0);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let drawn: String = buffer.content.iter().map(|c| c.symbol()).collect();
        for title in ["─ Upstreams ", "─ Memory ", "─ Cache ", "─ Engine "] {
            assert!(drawn.contains(title), "missing {title:?}");
        }

        // Row 0 is the first box's top edge: `┌─ Upstreams ───…┐`. The word
        // takes the left panel's heading colour; the rule around it must not.
        assert_eq!(buffer.get(3, 0).symbol(), "U");
        assert_eq!(buffer.get(3, 0).style().fg, Some(theme::ACCENT));
        assert_ne!(
            buffer.get(1, 0).style().fg,
            Some(theme::ACCENT),
            "the rule before the title is not part of it"
        );
    }

    /// Each box is sized from its own line count, so a row wider than the box
    /// would wrap and push every border below it down by one.
    #[test]
    fn no_row_outgrows_the_column_it_is_drawn_in() {
        let telemetry = fixtures::telemetry();
        let inner = SIDE_WIDTH as usize - 2;

        for lines in [
            upstream_lines(&telemetry),
            memory_lines(&telemetry),
            cache_lines(&telemetry),
            engine_lines(&telemetry),
        ] {
            for line in &lines {
                let drawn: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
                assert!(drawn <= inner, "{drawn} > {inner}: {:?}", text(&lines));
            }
        }
    }

    /// The figures are read by scanning down the right-hand edge, so they have
    /// to share one. A ragged column is the thing the fixed fields buy.
    #[test]
    fn every_figure_ends_on_the_same_column() {
        let telemetry = fixtures::telemetry();
        let inner = SIDE_WIDTH as usize - 2;

        for lines in [
            memory_lines(&telemetry),
            cache_lines(&telemetry),
            engine_lines(&telemetry),
        ] {
            // Warning rows such as "! events dropped" are deliberately not
            // figures, and are free to be shorter.
            for line in lines
                .iter()
                .filter(|l| text(&[(*l).clone()]).starts_with(" • "))
            {
                let drawn: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
                assert_eq!(drawn, inner, "{:?}", text(&lines));
            }
        }
    }
}
