//! Charts drawn with Unicode, so they cost no widget state and no allocation
//! per cell beyond the strings handed to the renderer.

/// Braille dot bitmasks: `[row][column]` within one 2×4 cell.
const DOTS: [[u32; 2]; 4] = [[0x1, 0x8], [0x2, 0x10], [0x4, 0x20], [0x40, 0x80]];
const BRAILLE_BASE: u32 = 0x2800;

/// A rendered braille chart: the glyph rows, and what each cell column is
/// worth so the caller can colour by value rather than by position.
pub struct Braille {
    /// Top row first.
    pub rows: Vec<String>,
    /// Peak of every sample behind each cell column, left to right. Empty when
    /// there was nothing to draw.
    pub columns: Vec<f64>,
}

/// Renders `data` as `height` rows of braille, each `width` cells wide.
pub fn braille(data: &[f64], width: usize, height: usize) -> Braille {
    if data.is_empty() || width == 0 || height == 0 {
        return Braille {
            rows: vec![" ".repeat(width); height.max(1)],
            columns: Vec::new(),
        };
    }

    let min = data.iter().copied().fold(f64::INFINITY, f64::min);
    let max = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let flat = (max - min).abs() < f64::EPSILON;
    let dots_high = height * 4;

    // Two sub-columns per cell, each the PEAK of the range it covers rather
    // than one point sampled from it: a spike narrower than a sub-column is
    // exactly the reading an RSS graph exists to show.
    let columns = width * 2;
    let sampled: Vec<f64> = (0..columns)
        .map(|index| {
            let start = index * data.len() / columns;
            // `start + 1` is the floor when there are fewer samples than
            // sub-columns; without it the slice is empty and folds to −∞.
            let end = ((index + 1) * data.len() / columns).clamp(start + 1, data.len());
            data[start..end]
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max)
        })
        .collect();

    let height_of = |value: f64| -> usize {
        if flat {
            1
        } else {
            (((value - min) / (max - min)) * (dots_high as f64 - 1.0) + 1.0) as usize
        }
        .clamp(1, dots_high)
    };

    let rows = (0..height)
        .map(|row| {
            let top = row * 4;
            (0..width)
                .map(|column| {
                    let left = height_of(sampled[column * 2]);
                    let right = height_of(sampled[column * 2 + 1]);

                    let mut code = BRAILLE_BASE;
                    for (offset, mask) in DOTS.iter().enumerate() {
                        // Rows are drawn top-down; the series grows upward.
                        let from_bottom = dots_high - 1 - (top + offset);
                        if from_bottom < left {
                            code |= mask[0];
                        }
                        if from_bottom < right {
                            code |= mask[1];
                        }
                    }
                    char::from_u32(code).unwrap_or(' ')
                })
                .collect()
        })
        .collect();

    // A true range peak: the two sub-columns tile the cell's source range
    // exactly, so the max of the pair is the max of everything behind it.
    let columns = (0..width)
        .map(|column| sampled[column * 2].max(sampled[column * 2 + 1]))
        .collect();

    Braille { rows, columns }
}

/// Splits a row into the longest runs sharing one value class, so colouring by
/// value costs a handful of spans instead of one per cell.
pub fn runs<T: PartialEq>(row: &str, classes: &[T]) -> Vec<(String, usize)> {
    let mut runs: Vec<(String, usize)> = Vec::new();

    for (index, glyph) in row.chars().enumerate() {
        // A cell past the classified range extends the last run rather than
        // starting an unclassified one.
        match classes.get(index) {
            Some(class) => match runs.last_mut() {
                Some((text, first)) if classes.get(*first) == Some(class) => text.push(glyph),
                _ => runs.push((glyph.to_string(), index)),
            },
            None => match runs.last_mut() {
                Some((text, _)) => text.push(glyph),
                None => runs.push((glyph.to_string(), index)),
            },
        }
    }
    runs
}

/// A one-row sparkline of block characters — for series that need a shape, not
/// a readable magnitude.
pub fn sparkline(data: &[u64], width: usize) -> String {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    if data.is_empty() || width == 0 {
        return String::new();
    }
    let max = data.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return LEVELS[0].to_string().repeat(data.len().min(width));
    }

    // Newest `width` points: a fixed-width panel showing a growing series
    // should scroll, not compress.
    let start = data.len().saturating_sub(width);
    data[start..]
        .iter()
        .map(|value| {
            let level = (*value as f64 / max as f64 * (LEVELS.len() - 1) as f64).round() as usize;
            LEVELS[level.min(LEVELS.len() - 1)]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_is_exactly_the_requested_width() {
        let chart = braille(&[1.0, 5.0, 3.0, 9.0, 2.0], 30, 3);
        assert_eq!(chart.rows.len(), 3);
        assert_eq!(chart.columns.len(), 30);
        for row in &chart.rows {
            assert_eq!(row.chars().count(), 30);
        }
    }

    /// The header calls this before the first poll returns.
    #[test]
    fn an_empty_series_still_yields_a_full_size_blank_chart() {
        let chart = braille(&[], 20, 3);
        assert_eq!(chart.rows.len(), 3);
        assert!(chart.rows.iter().all(|row| row.chars().count() == 20));
        assert!(chart.columns.is_empty());
    }

    /// A single sample, or a perfectly flat series, must not divide by a zero
    /// range — RSS is flat on an idle appliance for minutes at a time.
    #[test]
    fn a_flat_series_renders_without_dividing_by_a_zero_range() {
        for series in [vec![58.0], vec![58.0; 40]] {
            let chart = braille(&series, 16, 3);
            assert!(chart.rows.iter().all(|row| row.chars().count() == 16));
            assert!(
                chart.rows[2].chars().all(|c| c != ' '),
                "the baseline is drawn"
            );
            assert!(chart.columns.iter().all(|v| *v == 58.0));
        }
    }

    /// Each column carries the peak of the samples behind it, so a spike is
    /// never averaged away before it can be coloured.
    #[test]
    fn a_column_reports_the_peak_of_the_samples_behind_it() {
        let chart = braille(&[10.0, 200.0, 10.0, 10.0], 2, 3);
        assert_eq!(chart.columns.len(), 2);
        assert_eq!(chart.columns.iter().copied().fold(0.0, f64::max), 200.0);
    }

    /// The graph holds far more samples than it has sub-columns, so the
    /// reduction has to be a bucket peak. Picking one point per position drops
    /// this spike from both the drawn height and the colour band — the two
    /// things the RSS graph is read for.
    #[test]
    fn a_spike_between_two_positions_survives_the_reduction() {
        const BLANK: char = '\u{2800}';
        let chart = braille(&[10.0, 10.0, 10.0, 95.0, 10.0, 10.0, 10.0], 2, 3);

        assert_eq!(chart.columns, vec![10.0, 95.0]);
        let top: Vec<char> = chart.rows[0].chars().collect();
        assert_eq!(top[0], BLANK, "the flat column stays empty at the top");
        assert_ne!(top[1], BLANK, "the spike reaches the top row");
    }

    /// The buckets must tile the series with no gap, at every width — a spike
    /// that falls between two of them is invisible however tall it is. Sweeping
    /// the spike's position is what catches an off-by-one at either edge.
    #[test]
    fn no_sample_falls_between_two_buckets() {
        const LENGTH: usize = 37;

        for width in 1..=12usize {
            for spike in 0..LENGTH {
                let mut data = vec![10.0; LENGTH];
                data[spike] = 95.0;
                let chart = braille(&data, width, 3);

                assert!(
                    chart.columns.contains(&95.0),
                    "width {width}, spike at {spike}"
                );
            }
        }
    }

    #[test]
    fn adjacent_cells_of_one_class_collapse_into_a_single_run() {
        let runs = runs("abcdef", &[1, 1, 1, 2, 2, 3]);
        let text: Vec<&str> = runs.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(text, vec!["abc", "de", "f"]);
    }

    /// Cells past the classified range join the last run rather than being
    /// dropped, so the row keeps its full width.
    #[test]
    fn unclassified_trailing_cells_keep_the_row_intact() {
        let runs = runs("abcdef", &[1, 1]);
        let width: usize = runs.iter().map(|(t, _)| t.chars().count()).sum();
        assert_eq!(width, 6);
    }

    #[test]
    fn the_sparkline_keeps_the_newest_points_and_fits_the_width() {
        let series: Vec<u64> = (0..100).collect();
        let line = sparkline(&series, 24);
        assert_eq!(line.chars().count(), 24);
        assert_eq!(line.chars().last(), Some('█'), "the maximum is the newest");
    }

    #[test]
    fn an_all_zero_series_is_a_flat_baseline_not_a_panic() {
        assert_eq!(sparkline(&[0, 0, 0], 10), "▁▁▁");
        assert!(sparkline(&[], 10).is_empty());
    }
}
