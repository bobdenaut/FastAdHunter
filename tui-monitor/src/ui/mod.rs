//! Drawing. Every function here takes `&AppState` and produces widgets — none
//! of them can reach a socket, a URL or a token.
//!
//! One module per panel, so a new panel is a new module plus one call in
//! [`draw`].

pub mod chart;
pub mod details;
pub mod footer;
pub mod gauge;
pub mod header;
pub mod layout;
pub mod popup;
pub mod queries;
pub mod stats;
pub mod theme;

use ratatui::widgets::TableState;
use ratatui::Frame;

use crate::state::AppState;

/// Scroll positions, the open overlay and the table cursor — owned by
/// [`crate::app::App`], never shared with a worker.
#[derive(Default)]
pub struct UiState {
    pub feed_row: usize,
    pub stats_scroll: u16,
    pub details_scroll: u16,
    pub popup: Option<crate::models::events::QueryItem>,
    pub table: TableState,
    /// The layout of the last frame, so a mouse event can be resolved against
    /// what is actually on screen.
    pub regions: Option<layout::Regions>,
}

pub fn draw(
    frame: &mut Frame,
    state: &AppState,
    ui: &mut UiState,
    config: &crate::config::UiConfig,
) {
    let screen = frame.size();
    let regions = layout::split(screen);

    header::render(frame, regions.header, state, config.rss_thresholds());
    let max_scroll = stats::render(frame, regions.stats, state, ui.stats_scroll);
    queries::render(
        frame,
        regions.queries,
        state,
        ui.feed_row,
        &mut ui.table,
        config.slow_query_ms,
    );
    footer::render(frame, regions.footer, state);

    let details_max = regions
        .details
        .map(|area| details::render(frame, area, state, ui.details_scroll))
        .unwrap_or(0);

    if let Some(item) = ui.popup.as_ref() {
        popup::render(frame, screen, item);
    }

    // Clamped against what was actually laid out, so a scrollbar cannot run
    // past the last line of a panel whose height just changed.
    ui.stats_scroll = ui.stats_scroll.min(max_scroll);
    ui.details_scroll = ui.details_scroll.min(details_max);
    ui.regions = Some(regions);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Dragging a window corner can hand the next frame any size at all, down
    /// to one cell. A panic there is invisible: the hook prints inside the
    /// alternate screen, which the terminal restore then wipes.
    #[test]
    fn every_terminal_size_draws_without_panicking() {
        let state = crate::state::SharedState::new(crate::config::UiConfig::default().limits());
        let config = crate::config::UiConfig::default();

        // The overlay too: it is drawn over whatever rectangle is left, and a
        // popup open while the window shrinks is the likelier way to hit this.
        let open = match crate::models::events::decode(crate::models::fixtures::EVENTS_QUERY) {
            crate::models::events::Decoded::Event(crate::models::events::ServerEvent::Query(
                item,
            )) => Some(*item),
            _ => None,
        };
        assert!(open.is_some(), "fixtures/events-query.json");

        for width in 1..=60u16 {
            for height in 1..=30u16 {
                for popup in [None, open.clone()] {
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    let mut ui = UiState {
                        popup,
                        ..UiState::default()
                    };
                    terminal
                        .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
                        .unwrap_or_else(|err| panic!("{width}x{height}: {err}"));
                }
            }
        }

        // The detail column only appears past a width threshold, so the sweep
        // above never reaches it. Scrolled past its own end too, which is what
        // a wheel over a column that just shrank produces.
        for width in [126u16, 127, 160, 220] {
            for height in [3u16, 11, 24, 60] {
                for details_scroll in [0u16, 5, u16::MAX] {
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    let mut ui = UiState {
                        details_scroll,
                        ..UiState::default()
                    };
                    terminal
                        .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
                        .unwrap_or_else(|err| panic!("{width}x{height}: {err}"));
                }
            }
        }
    }

    /// With telemetry present every box carries real figures, which is a
    /// different height — and a different blit — from the empty case above.
    #[test]
    fn a_populated_detail_column_draws_at_every_offset() {
        let state = crate::state::SharedState::new(crate::config::UiConfig::default().limits());
        state.update(|app| app.telemetry = Some(crate::models::fixtures::telemetry()));
        let config = crate::config::UiConfig::default();

        for height in [3u16, 12, 30, 60] {
            for details_scroll in [0u16, 1, 20, u16::MAX] {
                let mut terminal = Terminal::new(TestBackend::new(140, height)).unwrap();
                let mut ui = UiState {
                    details_scroll,
                    ..UiState::default()
                };
                terminal
                    .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
                    .unwrap_or_else(|err| panic!("140x{height} @{details_scroll}: {err}"));
            }
        }
    }
}
