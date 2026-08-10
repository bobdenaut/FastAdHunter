//! The live query feed.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
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

const COLUMNS: [(&str, Constraint); 6] = [
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
    table_state: &mut TableState,
    slow_ms: f64,
) {
    // Resizing can hand a panel any rectangle, and ratatui's Scrollbar panics
    // on an empty one. A frame too small to hold the border has nothing to say.
    if area.width < 2 || area.height < 2 {
        return;
    }

    let rows = state
        .queries
        .iter()
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

    if !state.queries.is_empty() {
        table_state.select(Some(selected.min(state.queries.len() - 1)));
    }
    frame.render_stateful_widget(table, area, table_state);

    let mut scrollbar_state = ScrollbarState::new(state.queries.len()).position(selected);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼")),
        area,
        &mut scrollbar_state,
    );
}

/// Borrows every field it can: only the client label and the duration are
/// built per row.
fn row<'a>(item: &'a QueryItem, names: &'a LanNames, slow_ms: f64) -> Row<'a> {
    Row::new(vec![
        Cell::from(client_cell(item, names)),
        Cell::from(item.domain.as_str()),
        Cell::from(item.type_label()),
        Cell::from(item.verdict.as_str()),
        Cell::from(if item.cached { "Yes" } else { "No" }),
        time_cell(item.duration_ms, slow_ms),
    ])
    .style(Style::default().fg(theme::verdict(item.verdict)))
}

/// A name plus a dimmed family, or the bare address.
///
/// Costs one two-element `Vec` per named row, against the six-element one
/// [`Row::new`] already builds for the same row — the two texts carry different
/// styles, so a single span cannot express them and `format!` would allocate a
/// `String` instead without keeping the dimming. Both texts are borrowed.
///
/// The suffix is omitted on the address, where it would say what the text
/// already says and push a 39-character v6 address past the column.
fn client_cell<'a>(item: &'a QueryItem, names: &'a LanNames) -> Line<'a> {
    match item.resolved_name(names) {
        Some(name) => Line::from(vec![
            Span::raw(name),
            Span::styled(item.family(), Style::default().fg(theme::MUTED)),
        ]),
        None => Line::from(item.client.to_string()),
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
pub fn row_at(area: Rect, table_state: &TableState, row: u16) -> Option<usize> {
    let first_data_row = area.y + ROWS_ABOVE_DATA;
    let offset = row.checked_sub(first_data_row)?;
    Some(table_state.offset().saturating_add(offset as usize))
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
        let table_state = TableState::default();
        assert_eq!(row_at(area(), &table_state, 7), None, "border");
        assert_eq!(row_at(area(), &table_state, 8), None, "header");
    }

    #[test]
    fn a_click_maps_to_the_row_under_it_offset_by_the_scroll() {
        let mut table_state = TableState::default();
        assert_eq!(row_at(area(), &table_state, 9), Some(0));
        assert_eq!(row_at(area(), &table_state, 12), Some(3));

        *table_state.offset_mut() = 25;
        assert_eq!(row_at(area(), &table_state, 9), Some(25));
        assert_eq!(row_at(area(), &table_state, 12), Some(28));
    }
}
