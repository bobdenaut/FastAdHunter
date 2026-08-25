//! The live query feed.

use std::borrow::Cow;

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{
    Block, Borders, Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, TableState,
};
use ratatui::Frame;

use crate::models::events::QueryItem;
use crate::models::lan::LanNames;
use crate::state::AppState;
use crate::util::format::millis;

use super::theme;

/// Rows of the header and border above the first data row, for translating a
/// mouse click into a row index.
const ROWS_ABOVE_DATA: u16 = 2;

const COLUMNS: [(&str, Constraint); 7] = [
    // Untitled: the two values name themselves, and a heading here would sit
    // wider than the column it labels.
    ("", Constraint::Length(2)),
    ("Client", Constraint::Length(38)),
    ("Domain", Constraint::Min(20)),
    ("Type", Constraint::Length(6)),
    ("Verdict", Constraint::Length(8)),
    ("Cache", Constraint::Length(6)),
    ("Time (ms)", Constraint::Length(9)),
];

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    selected: usize,
    top: usize,
    table_state: &mut TableState,
    slow_ms: f64,
) -> Feed {
    let selected = selected.min(state.queries.len().saturating_sub(1));
    let visible = visible_rows(area);
    let top = top.min(max_top(state.queries.len(), visible));

    // Resizing can hand a panel any rectangle, and ratatui's Scrollbar panics
    // on an empty one. A frame too small to hold the border has nothing to say.
    if area.width < 2 || area.height < 2 {
        return Feed { top, selected };
    }

    let rows = state
        .queries
        .iter()
        .skip(top)
        .take(visible)
        .map(|item| row(item, &state.lan_names, slow_ms));

    let header = Row::new(COLUMNS.map(|(title, _)| Cell::from(title))).style(
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::UNDERLINED),
    );

    let table = Table::new(rows, COLUMNS.map(|(_, width)| width))
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Live Queries Feed ({})", state.queries.len())),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let on_screen = !state.queries.is_empty() && selected >= top && selected < top + visible;
    table_state.select(on_screen.then(|| selected - top));
    *table_state.offset_mut() = 0;
    frame.render_stateful_widget(table, area, table_state);

    let mut scrollbar_state = ScrollbarState::new(state.queries.len()).position(top);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼")),
        area,
        &mut scrollbar_state,
    );

    Feed { top, selected }
}

pub struct Feed {
    pub top: usize,
    pub selected: usize,
}

pub fn visible_rows(area: Rect) -> usize {
    data_rows(area).max(1)
}

fn data_rows(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(ROWS_ABOVE_DATA + 1))
}

pub fn max_top(len: usize, visible: usize) -> usize {
    len.saturating_sub(visible)
}

pub fn window_top(top: usize, selected: usize, visible: usize) -> usize {
    if selected < top {
        selected
    } else if selected >= top.saturating_add(visible) {
        selected.saturating_sub(visible - 1)
    } else {
        top
    }
}

/// Borrows every field it can: only the client label and the duration are
/// built per row.
fn row<'a>(item: &'a QueryItem, names: &'a LanNames, slow_ms: f64) -> Row<'a> {
    Row::new(vec![
        // Explicitly styled, so the row's verdict colour does not repaint a
        // fact that has nothing to do with the verdict.
        Cell::from(item.family()).style(Style::default().fg(theme::NEUTRAL)),
        Cell::from(client_cell(item, names)),
        Cell::from(item.domain.as_str()),
        Cell::from(item.type_label()),
        Cell::from(item.verdict.as_str()),
        Cell::from(item.cache_label()),
        time_cell(item.duration_ms, slow_ms),
    ])
    .style(Style::default().fg(theme::verdict(item.verdict)))
}

/// A resolved name, else the bare address. Borrowed for a named client, which
/// is every row once the router answers — only the address case allocates.
fn client_cell<'a>(item: &'a QueryItem, names: &'a LanNames) -> Cow<'a, str> {
    match item.resolved_name(names) {
        Some(name) => Cow::Borrowed(name),
        None => Cow::Owned(item.client.to_string()),
    }
}

/// Amber past `slow_ms` (`[ui] slow_query_ms`). The style overrides the row's
/// verdict colour, so a slow pass reads as slow rather than as green.
fn time_cell<'a>(duration_ms: f64, slow_ms: f64) -> Cell<'a> {
    let cell = Cell::from(millis(duration_ms));
    if duration_ms > slow_ms {
        cell.style(
            Style::default()
                .fg(theme::WARN)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        cell
    }
}

/// The feed row a click landed on, given the table's current scroll offset.
/// `None` when the click was on the header or the border.
pub fn row_at(area: Rect, top: usize, row: u16) -> Option<usize> {
    let first_data_row = area.y + ROWS_ABOVE_DATA;
    let offset = usize::from(row.checked_sub(first_data_row)?);
    (offset < data_rows(area)).then(|| top.saturating_add(offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect {
            x: 46,
            y: 7,
            width: 120,
            height: 30,
        }
    }

    #[test]
    fn a_click_on_the_border_or_header_selects_nothing() {
        assert_eq!(row_at(area(), 0, 7), None, "border");
        assert_eq!(row_at(area(), 0, 8), None, "header");
        assert_eq!(row_at(area(), 0, 35), Some(26), "the last row");
        assert_eq!(row_at(area(), 0, 36), None, "the bottom border");
        assert_eq!(row_at(area(), 25, 36), None, "the bottom border, scrolled");
    }

    #[test]
    fn a_click_maps_to_the_row_under_it_offset_by_the_viewport() {
        assert_eq!(row_at(area(), 0, 9), Some(0));
        assert_eq!(row_at(area(), 0, 12), Some(3));
        assert_eq!(row_at(area(), 25, 9), Some(25));
        assert_eq!(row_at(area(), 25, 12), Some(28));
    }

    fn feed(count: usize) -> AppState {
        let mut state = AppState::new(crate::config::UiConfig::default().limits());
        for index in 0..count {
            state.push_query(crate::models::fixtures::query(index));
        }
        state
    }

    fn drawn(state: &AppState, selected: usize, top: usize, table_state: &mut TableState) -> Drawn {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut terminal = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let mut feed = Feed {
            top: 0,
            selected: 0,
        };
        terminal
            .draw(|frame| {
                feed = render(frame, frame.size(), state, selected, top, table_state, 50.0);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let highlighted = (ROWS_ABOVE_DATA..buffer.area.height.saturating_sub(1))
            .find(|y| buffer.get(1, *y).modifier.contains(Modifier::REVERSED))
            .map(|y| usize::from(y - ROWS_ABOVE_DATA));

        Drawn {
            text: buffer.content.iter().map(|cell| cell.symbol()).collect(),
            highlighted,
            feed,
        }
    }

    struct Drawn {
        text: String,
        highlighted: Option<usize>,
        feed: Feed,
    }

    #[test]
    fn the_newest_rows_are_drawn_first_and_the_selection_is_marked() {
        let mut table_state = TableState::default();
        let drawn = drawn(&feed(40), 0, 0, &mut table_state);

        assert!(
            drawn.text.contains("Live Queries Feed (40)"),
            "{}",
            drawn.text
        );
        assert!(drawn.text.contains("d39.example"), "newest first");
        assert_eq!(table_state.selected(), Some(0));
        assert_eq!(drawn.highlighted, Some(0));
    }

    #[test]
    fn a_scrolled_window_draws_from_its_top_and_keeps_the_selection_marked() {
        let mut table_state = TableState::default();
        let drawn = drawn(&feed(40), 12, 10, &mut table_state);

        assert_eq!(drawn.feed.top, 10, "the window the wheel left it on");
        assert_eq!(table_state.offset(), 0, "the table holds only the window");
        assert_eq!(table_state.selected(), Some(2), "row 12 of a window at 10");
        assert_eq!(drawn.highlighted, Some(2), "row 12 sits two rows down");
        assert!(drawn.text.contains("d29.example"), "row 10 from the newest");
        assert!(!drawn.text.contains("d39.example"), "newest out of view");
    }

    #[test]
    fn a_selection_outside_the_window_is_not_drawn_and_does_not_drag_it() {
        let mut table_state = TableState::default();

        let below = drawn(&feed(40), 30, 10, &mut table_state);
        assert_eq!(below.feed.top, 10, "the window stayed where it was put");
        assert_eq!(table_state.selected(), None, "nothing to highlight");
        assert_eq!(below.highlighted, None);
        assert!(below.text.contains("d29.example"), "row 10 still on top");

        let above = drawn(&feed(40), 2, 10, &mut table_state);
        assert_eq!(above.feed.top, 10);
        assert_eq!(above.highlighted, None);
        assert!(above.text.contains("d29.example"), "and it did not move");
    }

    #[test]
    fn the_window_cannot_run_past_the_oldest_row_a_panel_can_show() {
        let mut table_state = TableState::default();
        let drawn = drawn(&feed(40), 0, 999, &mut table_state);

        assert_eq!(drawn.feed.top, 35, "40 rows, five of them visible");
        assert!(drawn.text.contains("d4.example"), "the oldest row");
    }

    #[test]
    fn a_panel_too_small_to_draw_hands_its_state_back_clamped() {
        let state = feed(40);
        let mut table_state = TableState::default();
        let tiny = Rect::new(0, 0, 1, 1);

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 8)).unwrap();
        let mut feed = Feed {
            top: 0,
            selected: 0,
        };
        terminal
            .draw(|frame| {
                feed = render(frame, tiny, &state, 999, 999, &mut table_state, 50.0);
            })
            .unwrap();

        assert_eq!(feed.selected, 39, "clamped to the feed it holds");
        assert_eq!(feed.top, 39, "and so is the window");
    }

    #[test]
    fn an_empty_feed_selects_nothing() {
        let mut table_state = TableState::default();
        let drawn = drawn(&feed(0), 0, 0, &mut table_state);

        assert!(drawn.text.contains("Live Queries Feed (0)"));
        assert_eq!(table_state.selected(), None);
        assert_eq!(drawn.highlighted, None);
    }

    #[test]
    fn the_window_moves_only_as_far_as_it_must_to_show_the_selection() {
        assert_eq!(window_top(10, 12, 5), 10, "already inside");
        assert_eq!(window_top(10, 10, 5), 10, "on the first row");
        assert_eq!(window_top(10, 14, 5), 10, "on the last row");
        assert_eq!(window_top(10, 15, 5), 11, "one past the last");
        assert_eq!(window_top(10, 2, 5), 2, "above it, so it leads");
        assert_eq!(window_top(0, 0, 1), 0, "a one-row panel");
    }

    #[test]
    fn the_oldest_window_leaves_no_blank_rows() {
        assert_eq!(max_top(40, 5), 35);
        assert_eq!(max_top(3, 5), 0, "a feed that does not fill the panel");
        assert_eq!(max_top(0, 5), 0, "an empty one");
    }
}
