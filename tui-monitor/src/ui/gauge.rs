//! The horizontal bar used by every percentage on screen.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

use super::theme;

/// Eighth-of-a-cell partials, so a bar moves smoothly instead of in whole
/// characters. Index is the number of eighths filled.
const PARTIALS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

/// A bar `width` cells wide, filled to `percent`.
pub fn bar<'a>(percent: f64, width: usize, fill: Color) -> Vec<Span<'a>> {
    let percent = percent.clamp(0.0, 100.0);
    let eighths = ((percent / 100.0) * (width * 8) as f64).round() as usize;
    let (full, remainder) = (eighths / 8, eighths % 8);

    let mut filled = "█".repeat(full);
    if remainder > 0 && full < width {
        filled.push_str(PARTIALS[remainder]);
    }
    let drawn = full + usize::from(remainder > 0 && full < width);

    vec![
        Span::styled(filled, Style::default().fg(fill)),
        Span::styled(
            "━".repeat(width.saturating_sub(drawn)),
            Style::default().fg(theme::TRACK),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn_width(spans: &[Span]) -> usize {
        spans.iter().map(|s| s.content.chars().count()).sum()
    }

    /// The bar shares a row with aligned columns, so a width that varies with
    /// the value would shift everything to its right.
    #[test]
    fn the_bar_occupies_its_full_width_at_every_value() {
        for percent in [0.0, 0.4, 12.5, 50.0, 99.9, 100.0] {
            assert_eq!(
                drawn_width(&bar(percent, 12, Color::Green)),
                12,
                "at {percent}%"
            );
        }
    }

    #[test]
    fn out_of_range_values_are_clamped_rather_than_overflowing() {
        assert_eq!(drawn_width(&bar(-10.0, 8, Color::Green)), 8);
        assert_eq!(drawn_width(&bar(1000.0, 8, Color::Green)), 8);
        assert_eq!(bar(100.0, 8, Color::Green)[0].content.as_ref(), "████████");
        assert!(bar(0.0, 8, Color::Green)[0].content.is_empty());
    }
}
