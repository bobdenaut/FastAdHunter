//! The terminal, the event loop and input handling.

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crate::BoxError;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::config::UiConfig;
use crate::state::SharedState;
use crate::ui::{self, layout, popup, queries, UiState};

/// How long to wait for input before giving the redraw budget a chance.
const INPUT_POLL: Duration = Duration::from_millis(50);

/// Rows a page key moves.
const PAGE: usize = 10;

pub struct App {
    state: SharedState,
    ui: UiState,
    config: UiConfig,
}

impl App {
    pub fn new(state: SharedState, config: UiConfig) -> Self {
        Self {
            state,
            ui: UiState::default(),
            config,
        }
    }

    /// Blocking, and deliberately not `async`: every call in it — `draw`,
    /// `event::poll`, `event::read` — is a synchronous terminal syscall, so an
    /// `async fn` here would be one that never yields.
    pub fn run(mut self) -> Result<(), BoxError> {
        let mut terminal = TerminalGuard::enter()?;
        let mut last_draw = Instant::now() - self.config.redraw();

        loop {
            // Redrawn when input arrived or the budget elapsed, rather than on
            // every pass — the widget tree is rebuilt from scratch each frame.
            if last_draw.elapsed() >= self.config.redraw() {
                let state = self.state.read();
                let config = self.config;
                terminal
                    .0
                    .draw(|frame| ui::draw(frame, &state, &mut self.ui, &config))?;
                last_draw = Instant::now();
            }

            if event::poll(INPUT_POLL)? {
                if self.handle(event::read()?) == Flow::Quit {
                    return Ok(());
                }
                last_draw = Instant::now() - self.config.redraw();
            }
        }
    }

    fn handle(&mut self, event: Event) -> Flow {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.key(key.code),
            Event::Mouse(mouse) => {
                self.mouse(mouse);
                Flow::Continue
            }
            _ => Flow::Continue,
        }
    }

    fn key(&mut self, code: KeyCode) -> Flow {
        let behind = self.ui.popup.is_some();
        match code {
            KeyCode::Char('q') => return Flow::Quit,
            KeyCode::Esc => self.ui.popup = None,
            KeyCode::Enter => self.toggle_popup(),
            KeyCode::Up if !behind => self.move_feed(-1),
            KeyCode::Down if !behind => self.move_feed(1),
            KeyCode::PageUp if !behind => self.move_feed(-(PAGE as isize)),
            KeyCode::PageDown if !behind => self.move_feed(PAGE as isize),
            KeyCode::Home if !behind => self.jump_feed(0),
            KeyCode::End if !behind => self.jump_feed(usize::MAX),
            KeyCode::Char('w') if !behind => {
                self.ui.stats_scroll = self.ui.stats_scroll.saturating_sub(1);
            }
            KeyCode::Char('s') if !behind => {
                self.ui.stats_scroll = self.ui.stats_scroll.saturating_add(1);
            }
            KeyCode::Char('e') if !behind => {
                self.ui.details_scroll = self.ui.details_scroll.saturating_sub(1);
            }
            KeyCode::Char('d') if !behind => {
                self.ui.details_scroll = self.ui.details_scroll.saturating_add(1);
            }
            _ => {}
        }
        Flow::Continue
    }

    fn mouse(&mut self, mouse: event::MouseEvent) {
        let Some(regions) = self.ui.regions.as_ref() else {
            return;
        };
        let (column, row) = (mouse.column, mouse.row);
        let behind = self.ui.popup.is_some();
        let over_stats = !behind && layout::contains(regions.stats, column, row);
        let over_details = !behind
            && regions
                .details
                .is_some_and(|area| layout::contains(area, column, row));
        let over_queries = !behind && layout::contains(regions.queries, column, row);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.ui.popup.is_some() {
                    // A click inside the overlay is for the overlay; anywhere
                    // else dismisses it.
                    if !layout::contains(popup::area(regions.screen), column, row) {
                        self.ui.popup = None;
                    }
                } else if over_queries {
                    if let Some(index) = queries::row_at(regions.queries, self.ui.feed_top, row) {
                        self.select(index);
                    }
                }
            }
            MouseEventKind::ScrollUp if over_stats => {
                self.ui.stats_scroll = self.ui.stats_scroll.saturating_sub(1);
            }
            MouseEventKind::ScrollDown if over_stats => {
                self.ui.stats_scroll = self.ui.stats_scroll.saturating_add(1);
            }
            MouseEventKind::ScrollUp if over_details => {
                self.ui.details_scroll = self.ui.details_scroll.saturating_sub(1);
            }
            MouseEventKind::ScrollDown if over_details => {
                self.ui.details_scroll = self.ui.details_scroll.saturating_add(1);
            }
            MouseEventKind::ScrollUp if over_queries => self.scroll_view(-1),
            MouseEventKind::ScrollDown if over_queries => self.scroll_view(1),
            _ => {}
        }
    }

    fn move_feed(&mut self, delta: isize) {
        self.jump_feed(self.row_in_view().saturating_add_signed(delta));
    }

    fn row_in_view(&self) -> usize {
        let top = self.ui.feed_top;
        self.ui.feed_row.clamp(top, top + self.visible_rows() - 1)
    }

    fn jump_feed(&mut self, row: usize) {
        let last = self.state.read().queries.len().saturating_sub(1);
        self.ui.feed_row = row.min(last);
        self.ui.feed_top =
            queries::window_top(self.ui.feed_top, self.ui.feed_row, self.visible_rows());
    }

    fn scroll_view(&mut self, delta: isize) {
        let len = self.state.read().queries.len();
        let max = queries::max_top(len, self.visible_rows());
        self.ui.feed_top = self.ui.feed_top.saturating_add_signed(delta).min(max);
    }

    fn visible_rows(&self) -> usize {
        self.ui
            .regions
            .as_ref()
            .map_or(1, |regions| queries::visible_rows(regions.queries))
    }

    fn toggle_popup(&mut self) {
        if self.ui.popup.is_some() {
            self.ui.popup = None;
        } else {
            self.select(self.row_in_view());
        }
    }

    /// Opens the overlay on one row, cloning it: the feed shifts as events
    /// arrive, so an index held across frames would drift onto another query.
    fn select(&mut self, index: usize) {
        if let Some(item) = self.state.read().queries.get(index).cloned() {
            self.ui.feed_row = index;
            self.ui.popup = Some(item);
        }
    }
}

#[derive(PartialEq, Eq)]
enum Flow {
    Continue,
    Quit,
}

/// Owns raw mode and the alternate screen, restoring both on drop so a panic or
/// an early return cannot leave the terminal unusable.
struct TerminalGuard(Terminal<CrosstermBackend<Stdout>>);

impl TerminalGuard {
    fn enter() -> Result<Self, BoxError> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        install_panic_hook();
        Ok(Self(Terminal::new(CrosstermBackend::new(stdout))?))
    }
}

/// Leaves the alternate screen *before* the default hook prints. Otherwise the
/// message lands on a buffer the restore then discards, and a panic is
/// indistinguishable from a clean exit.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        previous(info);
    }));
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.0.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.0.show_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::{App, Flow, PAGE};
    use crate::config::UiConfig;
    use crate::state::SharedState;
    use crate::ui::{layout, queries};
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;

    fn app(count: usize) -> App {
        let state = SharedState::new(UiConfig::default().limits());
        for index in 0..count {
            state.update(|inner| inner.push_query(crate::models::fixtures::query(index)));
        }
        App::new(state, UiConfig::default())
    }

    fn at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn region(app: &App, pick: fn(&layout::Regions) -> Rect) -> Rect {
        pick(app.ui.regions.as_ref().expect("laid out"))
    }

    fn laid_out(count: usize, width: u16, height: u16) -> App {
        let mut app = app(count);
        app.ui.regions = Some(layout::split(Rect::new(0, 0, width, height)));
        app
    }

    fn visible(app: &App) -> usize {
        queries::visible_rows(region(app, |regions| regions.queries))
    }

    fn on_screen(app: &App) -> bool {
        app.ui.feed_row >= app.ui.feed_top && app.ui.feed_row < app.ui.feed_top + visible(app)
    }

    #[test]
    fn the_wheel_moves_the_viewport_and_leaves_the_selection_alone() {
        let mut app = laid_out(40, 120, 30);
        app.ui.feed_row = 2;

        app.scroll_view(5);
        assert_eq!(app.ui.feed_top, 5, "the viewport moved");
        assert_eq!(app.ui.feed_row, 2, "the selection did not");

        app.scroll_view(-3);
        assert_eq!(app.ui.feed_top, 2);
        assert_eq!(app.ui.feed_row, 2, "still untouched on the way back");
    }

    #[test]
    fn the_wheel_stops_at_the_oldest_row_the_panel_can_show() {
        let mut app = laid_out(40, 120, 30);
        let max = 40 - visible(&app);

        app.scroll_view(PAGE as isize);
        app.scroll_view(PAGE as isize);
        app.scroll_view(PAGE as isize);
        app.scroll_view(PAGE as isize);
        assert_eq!(app.ui.feed_top, max, "clamped, never past the end");

        app.scroll_view(-(PAGE as isize) * 9);
        assert_eq!(app.ui.feed_top, 0, "and saturates at the newest row");
    }

    #[test]
    fn the_arrow_keys_move_the_selection_and_the_viewport_follows_it() {
        let mut app = laid_out(40, 120, 30);
        let rows = visible(&app);

        app.key(KeyCode::Down);
        assert_eq!(app.ui.feed_row, 1, "the selection stepped");
        assert_eq!(app.ui.feed_top, 0, "still on screen, so the window held");

        for _ in 1..rows {
            app.key(KeyCode::Down);
        }
        assert_eq!(app.ui.feed_row, rows);
        assert_eq!(app.ui.feed_top, 1, "the window moved one row, no more");

        app.key(KeyCode::Up);
        assert_eq!(app.ui.feed_row, rows - 1);
        assert_eq!(app.ui.feed_top, 1, "still inside, so it did not move back");

        app.key(KeyCode::PageUp);
        assert_eq!(app.ui.feed_row, rows - 1 - PAGE, "a page up");
        assert_eq!(app.ui.feed_top, 1, "still inside the window it was in");

        app.key(KeyCode::PageUp);
        assert_eq!(app.ui.feed_row, 0, "saturates at the newest row");
        assert_eq!(app.ui.feed_top, 0, "and pulls the window with it");
    }

    #[test]
    fn a_page_down_stops_at_the_oldest_row_the_feed_holds() {
        let mut app = laid_out(3, 120, 30);
        app.key(KeyCode::PageDown);

        assert_eq!(app.ui.feed_row, 2);
        assert_eq!(app.ui.feed_top, 0, "three rows all fit");
    }

    #[test]
    fn home_and_end_carry_the_selection_with_the_viewport() {
        let mut app = laid_out(40, 120, 30);
        let rows = visible(&app);

        app.key(KeyCode::End);
        assert_eq!(app.ui.feed_row, 39, "the oldest row the feed holds");
        assert_eq!(app.ui.feed_top, 40 - rows, "and the window that shows it");
        assert!(on_screen(&app), "so the selection is visible");

        app.scroll_view(-4);
        app.key(KeyCode::Home);
        assert_eq!(app.ui.feed_row, 0, "the newest row");
        assert_eq!(app.ui.feed_top, 0, "and the window at the top");
        assert!(on_screen(&app));
    }

    #[test]
    fn an_arrow_key_steps_inside_the_window_it_was_scrolled_to() {
        let mut app = laid_out(40, 120, 12);
        let rows = visible(&app);
        app.scroll_view(20);
        assert!(!on_screen(&app), "the wheel left the selection behind");

        app.key(KeyCode::Down);
        assert_eq!(app.ui.feed_top, 20, "the window the user is looking at");
        assert_eq!(app.ui.feed_row, 21, "one row into it, not row 1");
        assert!(on_screen(&app));

        app.scroll_view(-20);
        app.key(KeyCode::Up);
        assert_eq!(app.ui.feed_top, 0, "and it holds on the way back");
        assert_eq!(app.ui.feed_row, rows - 2, "stepping up from the last row");
        assert!(on_screen(&app));
    }

    #[test]
    fn the_overlay_opens_a_row_the_window_is_showing() {
        let mut app = laid_out(40, 120, 12);
        app.scroll_view(20);

        app.key(KeyCode::Enter);

        assert_eq!(app.ui.feed_row, 20, "the first row of the window");
        assert!(on_screen(&app), "Enter opened a row on screen");
        let opened = app.ui.popup.as_ref().expect("the overlay opened");
        assert_eq!(opened.domain, "d19.example", "row 20 of a 40-row feed");
    }

    #[test]
    fn home_and_end_hold_still_on_an_empty_feed() {
        let mut app = laid_out(0, 120, 30);

        app.key(KeyCode::End);
        assert_eq!((app.ui.feed_row, app.ui.feed_top), (0, 0));

        app.key(KeyCode::Home);
        assert_eq!((app.ui.feed_row, app.ui.feed_top), (0, 0));
    }

    #[test]
    fn a_feed_shorter_than_the_panel_keeps_end_at_the_top() {
        let mut app = laid_out(3, 120, 30);

        app.key(KeyCode::End);
        assert_eq!(app.ui.feed_row, 2, "the oldest of three");
        assert_eq!(app.ui.feed_top, 0, "all three fit, so nothing scrolled");
    }

    #[test]
    fn the_wheel_moves_the_feed_only_while_it_is_over_it() {
        let mut app = laid_out(40, 120, 30);
        let header = region(&app, |regions| regions.header);
        let footer = region(&app, |regions| regions.footer);
        let queries = region(&app, |regions| regions.queries);

        app.mouse(at(MouseEventKind::ScrollDown, header.x + 1, header.y + 1));
        app.mouse(at(MouseEventKind::ScrollDown, footer.x + 1, footer.y + 1));
        assert_eq!(app.ui.feed_top, 0, "neither bar touched the feed");

        app.mouse(at(MouseEventKind::ScrollDown, queries.x + 1, queries.y + 1));
        assert_eq!(app.ui.feed_top, 1, "over the feed, it scrolls");
    }

    #[test]
    fn an_open_overlay_swallows_the_wheel() {
        let mut app = laid_out(40, 120, 30);
        let queries = region(&app, |regions| regions.queries);
        let stats = region(&app, |regions| regions.stats);
        app.select(0);
        assert!(app.ui.popup.is_some());

        app.mouse(at(MouseEventKind::ScrollDown, queries.x + 1, queries.y + 1));
        app.mouse(at(MouseEventKind::ScrollDown, stats.x + 1, stats.y + 1));

        assert_eq!(app.ui.feed_top, 0, "the feed held still");
        assert_eq!(app.ui.stats_scroll, 0, "and so did the panel behind it");
    }

    #[test]
    fn an_open_overlay_swallows_the_navigation_keys() {
        let mut app = laid_out(40, 120, 30);
        app.key(KeyCode::Down);
        app.key(KeyCode::Enter);
        assert!(app.ui.popup.is_some(), "the overlay opened");

        app.key(KeyCode::Down);
        app.key(KeyCode::PageDown);
        app.key(KeyCode::Home);
        app.key(KeyCode::Char('s'));
        app.key(KeyCode::Char('d'));

        assert_eq!(app.ui.feed_row, 1, "the selection held still behind it");
        assert_eq!(app.ui.stats_scroll, 0, "as did the panels behind it");
        assert_eq!(app.ui.details_scroll, 0);
        assert!(app.key(KeyCode::Char('q')) == Flow::Quit, "q still quits");

        app.key(KeyCode::Esc);
        app.key(KeyCode::Down);
        assert_eq!(app.ui.feed_row, 2, "closing it hands the keys back");
    }

    #[test]
    fn a_left_click_selects_the_row_under_the_cursor() {
        let mut app = laid_out(40, 120, 30);
        let queries = region(&app, |regions| regions.queries);

        app.mouse(at(
            MouseEventKind::Down(MouseButton::Left),
            queries.x + 1,
            queries.y + 5,
        ));

        assert_eq!(app.ui.feed_row, 3, "two border rows and a header above it");
        assert_eq!(app.ui.feed_top, 0, "the viewport did not move");
        let opened = app.ui.popup.as_ref().expect("the overlay opened");
        assert_eq!(opened.domain, "d36.example");
    }

    #[test]
    fn a_click_on_the_panels_bottom_border_selects_nothing() {
        let mut app = laid_out(40, 120, 30);
        let queries = region(&app, |regions| regions.queries);
        let bottom = queries.y + queries.height - 1;

        app.mouse(at(
            MouseEventKind::Down(MouseButton::Left),
            queries.x + 1,
            bottom,
        ));

        assert!(app.ui.popup.is_none(), "no row lies under the border");
        assert_eq!(app.ui.feed_row, 0, "so nothing was selected");
    }

    #[test]
    fn the_overlay_opens_the_selected_row_not_the_top_of_the_window() {
        let mut app = laid_out(40, 120, 30);
        app.ui.feed_top = 6;
        app.ui.feed_row = 9;
        app.key(KeyCode::Enter);

        let opened = app.ui.popup.as_ref().expect("the overlay opened");
        assert_eq!(opened.domain, "d30.example", "row 9 of a 40-row feed");
    }

    #[test]
    fn moving_the_viewport_never_moves_the_selection() {
        let mut app = laid_out(40, 120, 30);
        let queries = region(&app, |regions| regions.queries);

        app.scroll_view(12);
        assert_eq!(app.ui.feed_row, 0, "the wheel left the selection alone");

        app.mouse(at(
            MouseEventKind::Down(MouseButton::Left),
            queries.x + 1,
            queries.y + 4,
        ));
        assert_eq!(app.ui.feed_row, 14, "the row under the cursor, not row 2");
        assert_eq!(app.ui.feed_top, 12, "and the viewport did not move");
        let opened = app.ui.popup.as_ref().expect("the overlay opened");
        assert_eq!(opened.domain, "d25.example");

        app.key(KeyCode::Esc);
        app.scroll_view(-6);
        assert_eq!(app.ui.feed_top, 6, "the wheel moved the window again");
        assert_eq!(app.ui.feed_row, 14, "with the selection still on its row");
    }
}
