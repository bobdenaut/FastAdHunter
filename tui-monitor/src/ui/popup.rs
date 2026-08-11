//! The query-detail overlay.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::models::events::QueryItem;
use crate::models::events::EventType;
use crate::models::lan::LanNames;
use crate::util::format::{bytes, millis};

use super::{layout, theme};

const WIDTH_PERCENT: u16 = 60;
const HEIGHT_PERCENT: u16 = 45;

/// Where the overlay sits, so the input handler can tell a click inside it
/// from a click that should dismiss it.
pub fn area(screen: Rect) -> Rect {
    layout::centered(WIDTH_PERCENT, HEIGHT_PERCENT, screen)
}

pub fn render(frame: &mut Frame, screen: Rect, item: &QueryItem, names: &LanNames) {
    let area = area(screen);
    frame.render_widget(Clear, area);

    let colour = theme::verdict(item.verdict);
    frame.render_widget(
        Paragraph::new(detail_lines(item, names))
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

fn detail_lines<'a>(item: &QueryItem, names: &LanNames) -> Vec<Line<'a>> {
    let colour = theme::verdict(item.verdict);
    let mut lines = vec![
        field("Client", item.client.to_string()),
        field("Name", item.resolved_name(names).unwrap_or("—").to_string()),
        Line::from(vec![
            Span::styled(format!("{:<11}", "Host"), theme::label()),
            Span::styled(item.domain.clone(), theme::strong(theme::ACCENT)),
        ]),
        field("Kind", item.kind.to_string()),
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
    //
    // Keyed on `kind`, which API.md §Events names as the discriminator — not on
    // whether `qtype` happens to be set, which hides every HTTP field the
    // moment a request carries one.
    if item.kind == EventType::Http {
        for (label, value) in [
            ("Method", item.method.clone()),
            ("Path", item.path.clone()),
            ("Status", item.status.map(|s| s.to_string())),
            ("Bytes", item.bytes.map(bytes)),
        ] {
            if let Some(value) = value {
                lines.push(field(label, value));
            }
        }
    } else {
        lines.push(field(
            "Type",
            item.qtype.clone().unwrap_or_else(|| "—".to_string()),
        ));
        lines.push(field("Cache", item.cache_label()));
    }
    lines
}

fn field<'a>(label: &str, value: impl Into<Span<'a>>) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label:<11}"), theme::label()),
        value.into(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::events::Verdict;

    fn text(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn item(event_type: EventType) -> QueryItem {
        QueryItem {
            kind: event_type,
            ts: "2026-07-17T10:41:03.610Z".to_string(),
            client: std::net::IpAddr::from([192, 168, 10, 15]),
            client_name: None,
            domain: "ads.example.com".to_string(),
            qtype: None,
            verdict: Verdict::Block,
            rule: None,
            list: None,
            duration_ms: 0.1,
            cached: false,
            method: None,
            path: None,
            status: None,
            bytes: None,
        }
    }

    #[test]
    fn a_dns_cache_miss_is_rendered() {
        let rendered = text(&detail_lines(
            &QueryItem {
                qtype: Some("A".to_string()),
                cached: false,
                ..item(EventType::Dns)
            },
            &LanNames::default(),
        ));

        assert!(rendered.contains("Cache"), "{rendered}");
        assert!(rendered.contains("MISS"), "{rendered}");
    }
    
    #[test]
    fn a_dns_item_shows_the_record_type_and_the_cache_flag() {
        let rendered = text(&detail_lines(
            &QueryItem {
                qtype: Some("AAAA".to_string()),
                cached: true,
                ..item(EventType::Dns)
            },
            &LanNames::default(),
        ));

        assert!(rendered.contains("AAAA"), "{rendered}");
        assert!(rendered.contains("Cache"), "{rendered}");
        assert!(rendered.contains("HIT"), "{rendered}");
        assert!(!rendered.contains("Method"), "{rendered}");
    }

    #[test]
    fn an_http_item_shows_the_request_fields() {
        let rendered = text(&detail_lines(
            &QueryItem {
                method: Some("GET".to_string()),
                path: Some("/pixel.gif?id=7".to_string()),
                status: Some(200),
                bytes: Some(0),
                ..item(EventType::Http)
            },
            &LanNames::default(),
        ));

        assert!(rendered.contains("GET"), "{rendered}");
        assert!(rendered.contains("/pixel.gif?id=7"), "{rendered}");
        assert!(rendered.contains("200"), "{rendered}");
        assert!(!rendered.contains("Cache"), "{rendered}");
    }

    /// The reason `kind` is the discriminator and `qtype` is not: keying on the
    /// payload field hid every HTTP detail the moment one arrived set.
    #[test]
    fn an_http_item_carrying_a_qtype_still_shows_its_http_fields() {
        let rendered = text(&detail_lines(
            &QueryItem {
                qtype: Some("A".to_string()),
                method: Some("POST".to_string()),
                status: Some(403),
                ..item(EventType::Http)
            },
            &LanNames::default(),
        ));

        assert!(rendered.contains("POST"), "{rendered}");
        assert!(rendered.contains("403"), "{rendered}");
    }

    /// A DNS event whose `qtype` is missing must still print the row — absent
    /// is a dash, not a reason to drop the field.
    #[test]
    fn a_dns_item_without_a_qtype_prints_a_dash() {
        let rendered = text(&detail_lines(&item(EventType::Dns), &LanNames::default()));

        assert!(rendered.contains("Type"), "{rendered}");
        assert!(rendered.contains('—'), "{rendered}");
    }

    /// The panel that exists to answer "who was this" must use the same two
    /// name sources the feed does, not `client_name` alone. No family suffix
    /// here — the address is printed in full one line above.
    #[test]
    fn a_resolved_name_reaches_the_details_panel() {
        let mut unnamed = item(EventType::Dns);
        unnamed.client = "fd6c:7f32:8e91::1".parse().unwrap();
        unnamed.client_name = None;

        let bare = text(&detail_lines(&unnamed, &LanNames::default()));
        assert!(!bare.contains(" - ipv6"), "{bare}");

        let mut names = LanNames::default();
        names.replace(std::collections::HashMap::from([(
            unnamed.client,
            std::sync::Arc::from("Alina's Note 10"),
        )]));
        let resolved = text(&detail_lines(&unnamed, &names));
        assert!(resolved.contains("Alina's Note 10"), "{resolved}");
    }
}
