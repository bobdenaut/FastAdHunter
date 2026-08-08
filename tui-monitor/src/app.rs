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
        match code {
            KeyCode::Char('q') => return Flow::Quit,
            KeyCode::Esc => self.ui.popup = None,
            KeyCode::Enter => self.toggle_popup(),
            KeyCode::Up => self.scroll_feed(-1),
            KeyCode::Down => self.scroll_feed(1),
            KeyCode::PageUp => self.scroll_feed(-(PAGE as isize)),
            KeyCode::PageDown => self.scroll_feed(PAGE as isize),
            KeyCode::Home => self.ui.feed_row = 0,
            KeyCode::Char('w') => self.ui.stats_scroll = self.ui.stats_scroll.saturating_sub(1),
            KeyCode::Char('s') => self.ui.stats_scroll = self.ui.stats_scroll.saturating_add(1),
            KeyCode::Char('e') => self.ui.details_scroll = self.ui.details_scroll.saturating_sub(1),
            KeyCode::Char('d') => self.ui.details_scroll = self.ui.details_scroll.saturating_add(1),
            _ => {}
        }
        Flow::Continue
    }

    fn mouse(&mut self, mouse: event::MouseEvent) {
        let Some(regions) = self.ui.regions.as_ref() else {
            return;
        };
        let (column, row) = (mouse.column, mouse.row);
        let over_stats = layout::contains(regions.stats, column, row);
        let over_details = regions
            .details
            .is_some_and(|area| layout::contains(area, column, row));

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.ui.popup.is_some() {
                    // A click inside the overlay is for the overlay; anywhere
                    // else dismisses it.
                    if !layout::contains(popup::area(regions.screen), column, row) {
                        self.ui.popup = None;
                    }
                } else if layout::contains(regions.queries, column, row) {
                    if let Some(index) = queries::row_at(regions.queries, &self.ui.table, row) {
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
            MouseEventKind::ScrollUp => self.scroll_feed(-1),
            MouseEventKind::ScrollDown => self.scroll_feed(1),
            _ => {}
        }
    }

    fn scroll_feed(&mut self, delta: isize) {
        let last = self.state.read().queries.len().saturating_sub(1);
        self.ui.feed_row = self.ui.feed_row.saturating_add_signed(delta).min(last);
    }

    fn toggle_popup(&mut self) {
        if self.ui.popup.is_some() {
            self.ui.popup = None;
        } else {
            self.select(self.ui.feed_row);
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
