//! The status header: three gauges over four aligned columns, and the RSS
//! graph filling the width that remains.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::config::RssThresholds;
use crate::models::telemetry::Telemetry;
use crate::state::{AppState, LinkStatus};
use crate::util::format::{mib, millis, thousands, truncate};

use super::{chart, gauge, theme};

const BAR_WIDTH: usize = 12;
const COL2: usize = 20;
const COL3: usize = 16;
const COL4: usize = 18;
const LABEL_WIDTH: usize = 7;
const VALUE_WIDTH: usize = 8;

/// Longest a link's reason may print. `LinkStatus::Down` carries a whole
/// reqwest error, which unbounded pushes the version, uptime and clock off the
/// right edge — the figures most worth having when a link is down.
const STATUS_WIDTH: usize = 28;

/// Width of everything left of the graph. Fixed, so the divider's `┬` meets the
/// graph's first column on every row.
const LEFT_WIDTH: usize =
    LABEL_WIDTH + BAR_WIDTH + 1 + VALUE_WIDTH + 3 + COL2 + 3 + COL3 + 3 + COL4 + 1;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState, thresholds: RssThresholds) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let graph_width = inner_width.saturating_sub(LEFT_WIDTH);

    let series = state.rss_series();
    let graph = chart::braille(series, graph_width, 3);

    // Naming the failure beats "waiting" when the wait is permanent.
    let waiting = match &state.perf {
        LinkStatus::Down(reason) => {
            format!("/history/perf: {}", truncate(reason, STATUS_WIDTH))
        }
        _ => "waiting for /history/perf".to_string(),
    };

    let mut lines = vec![
        title_line(state, inner_width),
        divider_line(series, state.rss_stride, &waiting, graph_width, thresholds),
    ];
    lines.extend(gauge_lines(state, &graph, thresholds));

    // "counters since boot" rather than "since boot": RSS, entries and
    // fresh/stale are instantaneous, and the graph carries its own 24h label.
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Status (counters since boot)"),
        ),
        area,
    );
}

fn title_line<'a>(state: &AppState, inner_width: usize) -> Line<'a> {
    let rules = state
        .telemetry
        .as_ref()
        .map_or(0, |t| t.engine.ruleset.rules);

    let mut spans = vec![
        Span::styled(" FastAdHunter Monitor ", theme::heading()),
        Span::raw("  "),
        Span::styled(
            format!("● WS {}", truncate(state.events.label(), STATUS_WIDTH)),
            Style::default().fg(theme::link(state.events.is_online())),
        ),
        Span::raw(" │ "),
        // Two indicators: the socket can be up while the poller is failing.
        Span::styled(
            format!("API {}", truncate(state.api.label(), STATUS_WIDTH)),
            Style::default().fg(theme::link(state.api.is_online())),
        ),
        Span::raw(format!(" │ Rules: {}", thousands(rules))),
    ];

    // Only when non-zero: frames arriving that this build cannot parse means
    // the feed is under-reporting, which nothing else on screen would show.
    if state.undecodable_frames > 0 {
        spans.push(Span::styled(
            format!(" │ ! {} undecodable", state.undecodable_frames),
            theme::strong(theme::BLOCKED),
        ));
    }

    let right = right_status(state);
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    spans.push(Span::raw(
        " ".repeat(inner_width.saturating_sub(used + right.chars().count())),
    ));
    spans.push(Span::styled(right, Style::default().fg(theme::ACCENT)));
    Line::from(spans)
}

/// Version, uptime and the clock. Uptime is the counter-reset marker: every
/// figure here is process-lifetime, so a drop explains a counter that fell.
fn right_status(state: &AppState) -> String {
    let clock = chrono::Local::now().format("%H:%M:%S");
    match state.telemetry.as_ref() {
        Some(telemetry) => format!(
            "v{} │ up {} │ {clock} ",
            telemetry.process.version,
            uptime(telemetry.process.uptime_seconds)
        ),
        None => format!("{clock} "),
    }
}

fn uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3600;
    let minutes = (seconds % 3600) / 60;

    // Two units is enough at any magnitude, and the clock beside it already
    // ticks — seconds here would only make the row twitch.
    if days > 0 {
        format!("{days}d {hours:02}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

fn divider_line<'a>(
    series: &[f64],
    stride: u64,
    waiting: &str,
    graph_width: usize,
    thresholds: RssThresholds,
) -> Line<'a> {
    let min = series.iter().copied().fold(f64::INFINITY, f64::min);
    let max = series.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    // Each figure carries its own colour, on the same thresholds the graph
    // uses; the words between them stay neutral.
    let parts: Vec<(String, Color)> = match series.last() {
        Some(last) => vec![
            (" RSS history — persisted (Min ".to_string(), theme::MUTED),
            (
                format!("{}", min.round() as u64),
                theme::rss(min, thresholds),
            ),
            (" │ Max ".to_string(), theme::MUTED),
            (
                format!("{}", max.round() as u64),
                theme::rss(max, thresholds),
            ),
            // "Last", not "Now": this is the newest *persisted* sample, up to
            // one poll interval stale. The gauge above it carries live RSS, and
            // two figures a row apart must not both claim to be current.
            (" │ Last ".to_string(), theme::MUTED),
            (
                format!("{}", last.round() as u64),
                theme::rss(*last, thresholds),
            ),
            // A strided series covers `n`× the span its point count suggests.
            (
                match stride {
                    0 | 1 => " MB) ".to_string(),
                    n => format!(" MB, 1 in {n}) "),
                },
                theme::MUTED,
            ),
        ],
        // Naming the source beats a graph frame labelled with numbers that
        // came from nowhere.
        None => vec![(format!(" RSS history — {waiting} "), theme::MUTED)],
    };

    let length: usize = parts.iter().map(|(text, _)| text.chars().count()).sum();
    let (left, right) = rule(length, graph_width);

    // `┬` lands on the last column before the graph, which is where every
    // gauge row closes with `│`.
    let mut spans = vec![
        Span::raw(format!(" {}", "─".repeat(LEFT_WIDTH.saturating_sub(2)))),
        Span::raw("┬"),
        Span::raw(left),
    ];
    spans.extend(
        parts
            .into_iter()
            .map(|(text, colour)| Span::styled(text, Style::default().fg(colour))),
    );
    spans.push(Span::raw(right));
    Line::from(spans)
}

/// The rules either side of a centred label, together filling exactly `width`.
/// Both empty when the label alone already fills it.
fn rule(length: usize, width: usize) -> (String, String) {
    if width <= length {
        return (String::new(), String::new());
    }
    let left = (width - length) / 2;
    ("─".repeat(left), "─".repeat(width - length - left))
}

struct GaugeRow {
    label: &'static str,
    percent: f64,
    colour: Color,
    value: String,
    col2: String,
    col3: String,
    col4: String,
}

impl GaugeRow {
    /// Everything right of the bar, at a fixed width.
    fn suffix(&self) -> String {
        format!(
            " {} │ {} │ {} │ {}│",
            fit(&self.value, VALUE_WIDTH),
            fit(&self.col2, COL2),
            fit(&self.col3, COL3),
            fit(&self.col4, COL4),
        )
    }
}

/// Pads or truncates to exactly `width`. `{:<w$}` only pads, so a counter that
/// outgrew its column would push the graph right on that row alone.
fn fit(text: &str, width: usize) -> String {
    let mut out: String = text.chars().take(width).collect();
    out.push_str(&" ".repeat(width - out.chars().count()));
    out
}

fn gauge_lines<'a>(
    state: &AppState,
    graph: &chart::Braille,
    thresholds: RssThresholds,
) -> Vec<Line<'a>> {
    let rows = match state.telemetry.as_ref() {
        Some(telemetry) => populated_rows(telemetry),
        None => empty_rows(),
    };

    // One colour class per column, shared by all three rows: a cell is
    // coloured by what it charts, not by how high up it sits.
    let bands: Vec<Color> = graph
        .columns
        .iter()
        .map(|megabytes| theme::rss(*megabytes, thresholds))
        .collect();

    rows.into_iter()
        .zip(&graph.rows)
        .map(|(row, graph_row)| {
            let mut spans = vec![Span::raw(format!(
                " {:<width$}",
                row.label,
                width = LABEL_WIDTH - 1
            ))];
            spans.extend(gauge::bar(row.percent, BAR_WIDTH, row.colour));
            spans.push(Span::raw(row.suffix()));
            spans.extend(
                chart::runs(graph_row, &bands)
                    .into_iter()
                    .map(|(text, first)| {
                        let colour = bands.get(first).copied().unwrap_or(theme::MUTED);
                        Span::styled(text, Style::default().fg(colour))
                    }),
            );
            Line::from(spans)
        })
        .collect()
}

fn populated_rows(telemetry: &Telemetry) -> Vec<GaugeRow> {
    let memory = &telemetry.memory;
    let cache = &telemetry.cache;
    let latency = &telemetry.engine.latency.dns;

    let rss = memory.process_rss.map_or(0.0, mib);
    let peak = memory.process_peak_rss.map_or(0.0, mib);
    let ruleset = mib(memory.components.ruleset_bytes);
    let residual = memory.components.residual_bytes.map_or(0.0, mib);
    let hit_percent = cache.lookup_hit_percent();

    vec![
        GaugeRow {
            label: "RSS",
            // Against the RB5009's 1 GB, shared with RouterOS.
            percent: (rss / 1024.0) * 100.0,
            colour: theme::ACCENT,
            value: format!("{rss:5.1} MB"),
            col2: format!("Peak {peak:6.1} MB"),
            col3: format!("Rules {ruleset:5.1} MB"),
            // RSS minus the accounted components: allocator overhead, thread
            // stacks and fragmentation.
            col4: format!("Residual {residual:5.1} MB"),
        },
        GaugeRow {
            label: "Hit",
            percent: hit_percent,
            colour: theme::OK,
            value: format!("{hit_percent:5.1}%  "),
            // Over cache *lookups*: a blocked query never reaches the cache.
            // The stats panel's "Hit (all q)" is the other denominator.
            col2: format!("{}/{} lookups", cache.hits, cache.hits + cache.misses),
            col3: format!("DNS L: {}", mean_ms(latency.block)),
            col4: format!("Cache H: {}", mean_ms(latency.cache_hit)),
        },
        GaugeRow {
            label: "Cache",
            percent: cache.load_percent,
            colour: theme::saturation(cache.load_percent),
            value: format!("{:5.1}%  ", cache.load_percent),
            col2: format!(
                "{}/{} ({:.1}MB)",
                cache.entries,
                cache.capacity,
                mib(cache.bytes)
            ),
            col3: format!("Fresh {}", cache.fresh),
            col4: format!("Stale {}", cache.stale),
        },
    ]
}

/// Dashes until the first poll returns: a cache that has served nothing and a
/// cache that has not been read are different things.
fn empty_rows() -> Vec<GaugeRow> {
    ["RSS", "Hit", "Cache"]
        .into_iter()
        .map(|label| GaugeRow {
            label,
            percent: 0.0,
            colour: theme::MUTED,
            value: "     —  ".to_string(),
            col2: "—".to_string(),
            col3: "—".to_string(),
            col4: "—".to_string(),
        })
        .collect()
}

/// A stage's lifetime mean in milliseconds, or `—` when it has recorded
/// nothing — which is not the same as `0.000 ms`.
fn mean_ms(stage: fah_model::StageTotals) -> String {
    match stage.mean_seconds() {
        Some(seconds) => format!("{} ms", millis(seconds * 1000.0)),
        None => "—".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::fixtures;

    const WAITING: &str = "waiting for /history/perf";

    /// A reqwest error runs to ~100 characters. Unbounded it consumes the title
    /// row, and the version, uptime and clock are clipped off the right edge —
    /// exactly when a reader needs to know how long the process has been up.
    #[test]
    fn a_long_link_error_cannot_evict_the_version_and_clock() {
        let mut state = AppState::default();
        state.telemetry = Some(fixtures::telemetry());
        state.api = LinkStatus::Down(
            "error sending request for url (https://172.17.0.2:8443/api/v1/telemetry): \
             connection refused (os error 111)"
                .to_string(),
        );

        let line = title_line(&state, 200);
        let rendered: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(rendered.contains("v0.2.10"), "{rendered}");
        assert!(rendered.contains("up 2d 03h"), "{rendered}");
        assert_eq!(rendered.chars().count(), 200, "the row still fits exactly");
    }

    /// The gauge row carries live RSS from `/api/v1/telemetry`; this figure is
    /// the newest *persisted* `/history/perf` sample and lags it by up to a
    /// poll interval. Both labelled "Now" is how 59.9 and 62 ended up on the
    /// same header looking like a contradiction.
    #[test]
    fn the_divider_does_not_claim_its_last_sample_is_the_current_one() {
        let line = divider_line(&[42.0, 62.0], 1, WAITING, 60, RssThresholds::default());
        let rendered: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(rendered.contains("Last 62"), "{rendered}");
        assert!(!rendered.contains("Now"), "{rendered}");
    }

    /// A permanent failure must not read as an ongoing wait.
    #[test]
    fn an_empty_graph_names_the_failure_rather_than_saying_waiting() {
        let line = divider_line(
            &[],
            1,
            "/history/perf: HTTP 500",
            60,
            RssThresholds::default(),
        );
        let rendered: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(rendered.contains("/history/perf: HTTP 500"), "{rendered}");
    }

    #[test]
    fn uptime_shortens_as_the_magnitude_falls() {
        assert_eq!(uptime(184_920), "2d 03h");
        assert_eq!(uptime(7_260), "2h 01m");
        assert_eq!(uptime(90), "1m");
    }

    #[test]
    fn a_centred_label_always_fills_exactly_the_graph_width() {
        for width in [0, 10, 43, 44, 200] {
            let label = " RSS history ".chars().count();
            let (left, right) = rule(label, width);
            let drawn = left.chars().count() + label + right.chars().count();
            assert_eq!(drawn, width.max(label), "width {width}");
        }
    }

    /// The divider's figures must be coloured individually, on the graph's
    /// thresholds — a single colour for the whole title reports the peak only.
    #[test]
    fn the_divider_colours_each_figure_by_its_own_value() {
        let thresholds = RssThresholds::default();
        let line = divider_line(&[42.0, 150.0, 46.0], 1, WAITING, 80, thresholds);

        let coloured: Vec<(&str, Color)> = line
            .spans
            .iter()
            .filter_map(|s| s.style.fg.map(|c| (s.content.as_ref(), c)))
            .filter(|(text, _)| text.chars().all(|c| c.is_ascii_digit()))
            .collect();

        assert_eq!(
            coloured,
            vec![
                ("42", theme::OK),
                ("150", theme::BLOCKED),
                ("46", theme::OK)
            ]
        );
    }

    /// Ragged rows would leave the graph column jagged and the divider's `┬`
    /// pointing at nothing.
    #[test]
    fn every_gauge_row_is_the_same_width_populated_or_not() {
        let expected = LEFT_WIDTH - LABEL_WIDTH - BAR_WIDTH;

        for rows in [populated_rows(&fixtures::telemetry()), empty_rows()] {
            for row in rows {
                assert_eq!(row.suffix().chars().count(), expected, "{}", row.label);
            }
        }
    }

    /// The divider's `┬` must sit on the same column as every gauge row's
    /// closing `│`, and the graph must start one column right of both.
    #[test]
    fn the_divider_tee_lands_on_the_gauge_rows_closing_bar() {
        let divider: String = divider_line(&[48.0], 1, WAITING, 20, RssThresholds::default())
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        let tee = divider.chars().position(|c| c == '┬').unwrap();

        let row = &populated_rows(&fixtures::telemetry())[0];
        let prefix = LABEL_WIDTH + BAR_WIDTH;
        let bar = prefix + row.suffix().chars().count() - 1;

        assert_eq!(tee, bar);
        assert_eq!(tee, LEFT_WIDTH - 1);
    }

    /// A counter that outgrows its column must lose digits, never shift the
    /// graph — one ragged row breaks the whole block's alignment.
    #[test]
    fn an_oversized_column_is_truncated_rather_than_widening_the_row() {
        let row = GaugeRow {
            label: "Hit",
            percent: 0.0,
            colour: theme::MUTED,
            value: "999999999 MB".to_string(),
            col2: "1234567890/1234567890 lookups".to_string(),
            col3: "DNS L: 1234.567 ms".to_string(),
            col4: "Residual 99999.9 MB".to_string(),
        };

        assert_eq!(
            row.suffix().chars().count(),
            LEFT_WIDTH - LABEL_WIDTH - BAR_WIDTH
        );
    }

    /// The chart autoscales, so a column's colour must come from its value and
    /// not from the row it lands in: the same glyph row is green on an idle
    /// appliance and red after a bad compile.
    #[test]
    fn a_graph_column_is_coloured_by_its_value_not_its_row() {
        let thresholds = RssThresholds {
            warn_mb: 60.0,
            alert_mb: 100.0,
        };

        assert_eq!(theme::rss(49.0, thresholds), theme::OK);
        assert_eq!(theme::rss(59.9, thresholds), theme::OK);
        assert_eq!(theme::rss(60.0, thresholds), theme::WARN);
        assert_eq!(theme::rss(82.0, thresholds), theme::WARN);
        assert_eq!(theme::rss(100.0, thresholds), theme::BLOCKED);
        assert_eq!(theme::rss(230.0, thresholds), theme::BLOCKED);
    }

    /// A graph drawn from every `n`-th sample looks exactly like one drawn from
    /// all of them, so the divider has to name the stride it was served at.
    #[test]
    fn the_divider_says_when_the_series_arrived_decimated() {
        let thresholds = RssThresholds::default();
        let text = |stride| -> String {
            divider_line(&[48.0, 52.0], stride, WAITING, 60, thresholds)
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect()
        };

        // 0 is the pre-first-read state and 1 is an undecimated series; both
        // mean "every stored sample is here", so neither may claim a stride.
        for undecimated in [0, 1] {
            let line = text(undecimated);
            assert!(line.contains(" MB) "), "stride {undecimated}: {line}");
            assert!(!line.contains("1 in"), "stride {undecimated}: {line}");
        }
        assert!(text(4).contains(" MB, 1 in 4) "), "{}", text(4));
    }

    /// A series crossing both thresholds paints three runs, in order, and
    /// still covers the full graph width.
    #[test]
    fn a_climbing_series_paints_green_then_amber_then_red() {
        let graph = chart::braille(&[40.0, 70.0, 150.0], 12, 3);
        let thresholds = RssThresholds {
            warn_mb: 60.0,
            alert_mb: 100.0,
        };
        let bands: Vec<Color> = graph
            .columns
            .iter()
            .map(|mb| theme::rss(*mb, thresholds))
            .collect();

        let runs = chart::runs(&graph.rows[2], &bands);
        let colours: Vec<Color> = runs.iter().map(|(_, first)| bands[*first]).collect();
        let width: usize = runs.iter().map(|(text, _)| text.chars().count()).sum();

        assert_eq!(colours, vec![theme::OK, theme::WARN, theme::BLOCKED]);
        assert_eq!(width, 12);
    }

    #[test]
    fn a_stage_that_recorded_nothing_prints_a_dash_not_a_zero() {
        assert_eq!(mean_ms(fah_model::StageTotals::default()), "—");
        assert_eq!(
            mean_ms(fah_model::StageTotals {
                count: 2,
                sum_seconds: 0.0001,
            }),
            "0.050 ms"
        );
    }
}
