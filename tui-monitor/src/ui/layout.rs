//! Where each panel goes. [`Regions`] is the one description of the screen:
//! drawing functions receive their rectangle and hit-testing asks the same
//! value, so no offset is hard-coded twice.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Width of **both** flanking columns — one constant, because the screen reads
/// as balanced only while they match. Wide enough for the statistics panel's
/// gauges and for `Bytes  484.6 KiB / 64.0 MiB`, the widest detail row.
pub const SIDE_WIDTH: u16 = 46;

/// The feed's floor. Under it the detail column is dropped entirely rather than
/// squeezed out of the one panel whose rows are the point of the program.
const QUERIES_MIN: u16 = 40;

/// Header: three gauge rows plus the graph, inside a border.
const HEADER_HEIGHT: u16 = 7;

/// Footer: one content line inside a border.
const FOOTER_HEIGHT: u16 = 3;

pub struct Regions {
    /// The whole terminal, kept so an overlay can be hit-tested against the
    /// same rectangle it was centred in.
    pub screen: Rect,
    pub header: Rect,
    pub stats: Rect,
    pub queries: Rect,
    /// `None` when the terminal is too narrow to hold it without eating the
    /// feed. The figures it carries are all reachable from the API anyway.
    pub details: Option<Rect>,
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

    let roomy = area.width >= 2 * SIDE_WIDTH + QUERIES_MIN;
    let columns: &[Constraint] = if roomy {
        &[
            Constraint::Length(SIDE_WIDTH),
            Constraint::Min(QUERIES_MIN),
            Constraint::Length(SIDE_WIDTH),
        ]
    } else {
        &[Constraint::Length(SIDE_WIDTH), Constraint::Min(QUERIES_MIN)]
    };
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(columns)
        .split(rows[1]);

    Regions {
        screen: area,
        header: rows[0],
        stats: body[0],
        queries: body[1],
        details: roomy.then(|| body[2]),
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
        assert_eq!(regions.stats.width, SIDE_WIDTH);
        assert_eq!(regions.queries.x, SIDE_WIDTH);
        assert_eq!(
            regions.footer.y + regions.footer.height,
            screen().height,
            "the footer must reach the bottom edge"
        );
        assert_eq!(regions.stats.height, regions.queries.height);
    }

    /// The three columns must meet exactly — a gap or an overlap between the
    /// feed and the detail column shows as a torn border.
    #[test]
    fn the_detail_column_abuts_the_feed_and_the_right_edge() {
        let regions = split(screen());
        let details = regions.details.expect("200 columns is roomy");

        assert_eq!(regions.queries.x + regions.queries.width, details.x);
        assert_eq!(details.x + details.width, screen().width);
        assert_eq!(details.height, regions.queries.height);
        // The screen reads as balanced only while the two flanks match.
        assert_eq!(details.width, regions.stats.width);
    }

    /// The feed is the one panel whose rows are the point of the program, so it
    /// keeps its floor and the detail column is what gives way.
    #[test]
    fn a_narrow_terminal_drops_the_detail_column_rather_than_the_feed() {
        let threshold = 2 * SIDE_WIDTH + QUERIES_MIN;

        for width in [40u16, 80, threshold - 1] {
            let regions = split(Rect { width, ..screen() });
            assert!(regions.details.is_none(), "width {width}");
            assert!(
                regions.queries.width >= QUERIES_MIN.min(width),
                "width {width}: feed squeezed to {}",
                regions.queries.width
            );
        }

        assert!(split(Rect {
            width: threshold,
            ..screen()
        })
        .details
        .is_some());
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
