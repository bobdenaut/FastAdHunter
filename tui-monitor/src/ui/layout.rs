//! Where each panel goes. [`Regions`] is the one description of the screen:
//! drawing functions receive their rectangle and hit-testing asks the same
//! value, so no offset is hard-coded twice.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Width of the statistics column. The panel's content is laid out against it.
pub const STATS_WIDTH: u16 = 46;

/// Header: three gauge rows plus the graph, inside a border.
const HEADER_HEIGHT: u16 = 7;

/// Footer: two content lines inside a border.
const FOOTER_HEIGHT: u16 = 4;

pub struct Regions {
    /// The whole terminal, kept so an overlay can be hit-tested against the
    /// same rectangle it was centred in.
    pub screen: Rect,
    pub header: Rect,
    pub stats: Rect,
    pub queries: Rect,
    pub footer: Rect,
}

pub fn split(area: Rect) -> Regions {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(HEADER_HEIGHT),
            Constraint::Min(10),
            Constraint::Length(FOOTER_HEIGHT),
        ])
        .split(area);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(STATS_WIDTH), Constraint::Min(40)])
        .split(rows[1]);

    Regions {
        screen: area,
        header: rows[0],
        stats: body[0],
        queries: body[1],
        footer: rows[2],
    }
}

/// A rectangle centred in `area`, sized as a percentage of it.
pub fn centered(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

/// Whether a terminal cell lies inside a rectangle — the hit test behind mouse
/// clicks and scroll routing.
pub fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 200,
            height: 50,
        }
    }

    #[test]
    fn the_regions_tile_the_screen_without_overlapping() {
        let regions = split(screen());

        assert_eq!(regions.header.y, 0);
        assert_eq!(regions.header.height, HEADER_HEIGHT);
        assert_eq!(regions.stats.y, HEADER_HEIGHT);
        assert_eq!(regions.stats.width, STATS_WIDTH);
        assert_eq!(regions.queries.x, STATS_WIDTH);
        assert_eq!(
            regions.footer.y + regions.footer.height,
            screen().height,
            "the footer must reach the bottom edge"
        );
        assert_eq!(regions.stats.height, regions.queries.height);
    }

    /// The hit test behind mouse clicks and scroll routing.
    #[test]
    fn hit_testing_distinguishes_the_two_body_panels() {
        let regions = split(screen());

        assert!(contains(regions.stats, 10, 20));
        assert!(!contains(regions.queries, 10, 20));
        assert!(contains(regions.queries, 100, 20));
        assert!(!contains(regions.stats, 100, 20));
        // The footer is neither.
        assert!(!contains(regions.stats, 10, 48));
        assert!(!contains(regions.queries, 100, 48));
    }

    #[test]
    fn a_centred_rectangle_stays_inside_its_parent() {
        let popup = centered(60, 40, screen());
        assert!(popup.x > 0 && popup.y > 0);
        assert!(popup.x + popup.width <= screen().width);
        assert!(popup.y + popup.height <= screen().height);
    }
}
