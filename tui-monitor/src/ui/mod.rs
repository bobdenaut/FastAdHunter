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
    pub feed_top: usize,
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

    let feed = queries::render(
        frame,
        regions.queries,
        state,
        ui.feed_row,
        ui.feed_top,
        &mut ui.table,
        config.slow_query_ms,
    );
    footer::render(frame, regions.footer, state);

    let details_max = regions
        .details
        .map(|area| details::render(frame, area, state, ui.details_scroll))
        .unwrap_or(0);

    if let Some(item) = ui.popup.as_ref() {
        popup::render(frame, screen, item, &state.lan_names);
    }

    // Clamped against what was actually laid out, so a scrollbar cannot run
    // past the last line of a panel whose height just changed.
    ui.stats_scroll = ui.stats_scroll.min(max_scroll);
    ui.details_scroll = ui.details_scroll.min(details_max);
    ui.feed_top = feed.top;
    ui.feed_row = feed.selected;
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

        let scrolled = [
            (None, 0, 0),
            (open.clone(), 0, 0),
            (None, 0, 3),
            (None, 3, 7),
            (None, 7, 3),
            (open.clone(), usize::MAX, usize::MAX),
        ];

        for width in 1..=60u16 {
            for height in 1..=30u16 {
                for (popup, feed_top, feed_row) in scrolled.clone() {
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    let mut ui = UiState {
                        popup,
                        feed_top,
                        feed_row,
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

    fn filled(count: usize) -> crate::state::SharedState {
        let state = crate::state::SharedState::new(crate::config::UiConfig::default().limits());
        for index in 0..count {
            state.update(|app| app.push_query(crate::models::fixtures::query(index)));
        }
        state
    }

    fn text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn a_populated_feed_draws_at_every_panel_height() {
        let state = filled(40);
        let config = crate::config::UiConfig::default();

        for height in 1..=30u16 {
            for width in [1u16, 20, 80, 140] {
                for (feed_top, feed_row) in [
                    (0, 0),
                    (0, usize::MAX),
                    (3, 5),
                    (30, 0),
                    (39, 39),
                    (usize::MAX, usize::MAX),
                ] {
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    let mut ui = UiState {
                        feed_top,
                        feed_row,
                        ..UiState::default()
                    };
                    terminal
                        .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
                        .unwrap_or_else(|err| panic!("{width}x{height} @{feed_top}: {err}"));
                }
            }
        }
    }

    #[test]
    fn arrivals_do_not_pull_a_scrolled_viewport_back_to_the_newest_row() {
        let state = filled(40);
        let config = crate::config::UiConfig::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let mut ui = UiState {
            feed_top: 10,
            feed_row: 12,
            ..UiState::default()
        };

        terminal
            .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
            .unwrap();
        assert_eq!(ui.feed_top, 10);
        assert!(text(&terminal).contains("d29.example"), "row 10");

        for index in 40..47 {
            state.update(|app| app.push_query(crate::models::fixtures::query(index)));
        }
        terminal
            .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
            .unwrap();

        assert_eq!(ui.feed_top, 10, "the viewport did not jump");
        assert_eq!(ui.feed_row, 12, "nor did the selection");
        let drawn = text(&terminal);
        assert!(
            !drawn.contains("d46.example"),
            "the newest arrival stayed off screen: {drawn}"
        );
    }

    #[test]
    fn a_live_viewport_shows_every_arrival() {
        let state = filled(40);
        let config = crate::config::UiConfig::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let mut ui = UiState::default();

        terminal
            .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
            .unwrap();
        assert!(text(&terminal).contains("d39.example"));

        state.update(|app| app.push_query(crate::models::fixtures::query(40)));
        terminal
            .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
            .unwrap();

        assert_eq!(ui.feed_top, 0, "still at the newest rows");
        assert!(text(&terminal).contains("d40.example"), "the arrival shows");
    }

    #[test]
    fn evicting_the_oldest_rows_clamps_the_viewport() {
        let limits = crate::config::UiConfig::default().limits();
        let state = crate::state::SharedState::new(limits);
        for index in 0..limits.feed_rows {
            state.update(|app| app.push_query(crate::models::fixtures::query(index)));
        }
        let config = crate::config::UiConfig::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let mut ui = UiState {
            feed_top: limits.feed_rows - 1,
            feed_row: limits.feed_rows - 1,
            ..UiState::default()
        };

        terminal
            .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
            .unwrap();
        let visible = queries::visible_rows(ui.regions.as_ref().unwrap().queries);
        assert_eq!(
            ui.feed_top,
            limits.feed_rows - visible,
            "clamped to the end"
        );

        state.update(|app| app.push_query(crate::models::fixtures::query(9_999)));
        assert_eq!(
            state.read().queries.len(),
            limits.feed_rows,
            "the ring evicted one"
        );
        terminal
            .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
            .unwrap();

        assert_eq!(ui.feed_top, limits.feed_rows - visible, "still in range");
        assert!(ui.feed_row < limits.feed_rows, "and so is the selection");
    }

    #[test]
    fn a_feed_that_no_longer_fills_the_panel_pins_the_viewport_to_the_top() {
        let state = filled(3);
        let config = crate::config::UiConfig::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
        let mut ui = UiState {
            feed_top: 5,
            ..UiState::default()
        };

        terminal
            .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
            .unwrap();

        assert_eq!(ui.feed_top, 0, "nothing left to scroll to");
    }

    #[test]
    fn a_window_past_the_end_is_clamped() {
        let state = filled(40);
        let config = crate::config::UiConfig::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let mut ui = UiState {
            feed_top: 999,
            feed_row: 999,
            ..UiState::default()
        };

        terminal
            .draw(|frame| draw(frame, &state.read(), &mut ui, &config))
            .unwrap();

        assert_eq!(ui.feed_top, 35, "40 rows, five visible");
        assert_eq!(ui.feed_row, 39);
    }
}
