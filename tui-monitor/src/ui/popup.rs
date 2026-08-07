//! The query-detail overlay.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::models::events::QueryItem;
use crate::util::format::{bytes, millis};

use super::{layout, theme};

const WIDTH_PERCENT: u16 = 60;
const HEIGHT_PERCENT: u16 = 45;

/// Where the overlay sits, so the input handler can tell a click inside it
/// from a click that should dismiss it.
pub fn area(screen: Rect) -> Rect {
    layout::centered(WIDTH_PERCENT, HEIGHT_PERCENT, screen)
}

pub fn render(frame: &mut Frame, screen: Rect, item: &QueryItem) {
    let area = area(screen);
    frame.render_widget(Clear, area);

    let colour = theme::verdict(item.verdict);
    let mut lines = vec![
        field("Client", item.client.to_string()),
        field(
            "Name",
            item.client_name.clone().unwrap_or_else(|| "—".to_string()),
        ),
        Line::from(vec![
            Span::styled(format!("{:<11}", "Host"), theme::label()),
            Span::styled(item.domain.clone(), theme::strong(theme::ACCENT)),
        ]),
        field("Kind", item.kind.clone()),
        Line::from(vec![
            Span::styled(format!("{:<11}", "Verdict"), theme::label()),
            Span::styled(item.verdict.as_str(), theme::strong(colour)),
        ]),
        field("Rule", item.rule.clone().unwrap_or_else(|| "—".to_string())),
        field("List", item.list.clone().unwrap_or_else(|| "—".to_string())),
        field("Duration", format!("{} ms", millis(item.duration_ms))),
        field("Timestamp", item.ts.clone()),
    ];

    // DNS and HTTP items carry disjoint fields; showing the absent half as
    // null would suggest a value that was measured and came back empty.
    match item.qtype.as_deref() {
        Some(qtype) => {
            lines.push(field("Type", qtype.to_string()));
            lines.push(field(
                "Cached",
                if item.cached { "yes" } else { "no" }.to_string(),
            ));
        }
        None => {
            if let Some(method) = item.method.as_deref() {
                lines.push(field("Method", method.to_string()));
            }
            if let Some(path) = item.path.as_deref() {
                lines.push(field("Path", path.to_string()));
            }
            if let Some(status) = item.status {
                lines.push(field("Status", status.to_string()));
            }
            if let Some(relayed) = item.bytes {
                lines.push(field("Bytes", bytes(relayed)));
            }
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Query Details — Esc to close ")
                    .style(ratatui::style::Style::default().fg(colour)),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn field<'a>(label: &str, value: String) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label:<11}"), theme::label()),
        Span::raw(value),
    ])
}
