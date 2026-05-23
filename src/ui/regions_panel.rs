use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;

/// Width of the regions sidebar in columns.
pub const SIDEBAR_WIDTH: u16 = 34;

/// Return the display color for a given editspec action string.
fn action_color(action: &str) -> Color {
    match action {
        "keep" => Color::Rgb(0, 200, 80),
        "edit" => Color::Rgb(255, 200, 0),
        "replace" => Color::Rgb(255, 80, 80),
        "insert" => Color::Rgb(80, 150, 255),
        "delete" => Color::Rgb(140, 140, 140),
        _ => Color::White,
    }
}

/// Return a short symbol for the action.
fn action_symbol(action: &str) -> &'static str {
    match action {
        "keep" => "=",
        "edit" => "~",
        "replace" => ">",
        "insert" => "+",
        "delete" => "-",
        _ => "?",
    }
}

/// Number of header lines before the first region item.
/// Layout: title (1) + empty (1) + legend (1) + empty (1) = 4 lines.
pub const REGIONS_HEADER_LINES: u16 = 4;

/// Render the EditSpec Regions panel as a left-edge sidebar.
pub fn render_regions_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let mut lines: Vec<Line> = Vec::new();

    // Color legend
    lines.push(Line::from(Span::styled(
        " EditSpec Regions",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Legend row
    lines.push(Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled("=", Style::default().fg(action_color("keep"))),
        Span::styled("keep ", Style::default().fg(Color::DarkGray)),
        Span::styled("~", Style::default().fg(action_color("edit"))),
        Span::styled("edit ", Style::default().fg(Color::DarkGray)),
        Span::styled(">", Style::default().fg(action_color("replace"))),
        Span::styled("repl ", Style::default().fg(Color::DarkGray)),
        Span::styled("+", Style::default().fg(action_color("insert"))),
        Span::styled("ins ", Style::default().fg(Color::DarkGray)),
        Span::styled("-", Style::default().fg(action_color("delete"))),
        Span::styled("del", Style::default().fg(Color::DarkGray)),
    ]));

    lines.push(Line::from(""));

    match &app.annotation {
        Some(ann) => match &ann.editspec_regions {
            Some(regions) if !regions.is_empty() => {
                for (i, region) in regions.iter().enumerate() {
                    let is_focused = i == app.focused_region;
                    let color = action_color(&region.action);
                    let sym = action_symbol(&region.action);

                    let range_str = format!("{}-{}", region.range[0], region.range[1]);
                    let label = region
                        .label
                        .as_deref()
                        .unwrap_or(&region.action);

                    if is_focused {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!(" \u{25b6} {}{} ", sym, region.chain),
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!("{} ", range_str),
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(color),
                            ),
                            Span::styled(
                                label.to_string(),
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(color),
                            ),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("   {}{} ", sym, region.chain),
                                Style::default().fg(color),
                            ),
                            Span::styled(
                                format!("{} ", range_str),
                                Style::default().fg(Color::White),
                            ),
                            Span::styled(
                                label.to_string(),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]));
                    }
                }

                // Scroll hint
                if regions.len() as u16 > area.height.saturating_sub(6) {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        " Scroll: mouse wheel",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            _ => {
                lines.push(Line::from(Span::styled(
                    " No region data in annotation",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        },
        None => {
            lines.push(Line::from(Span::styled(
                " No annotation loaded",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Use --annotation <file>",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    // Store item metadata for mouse click mapping
    let item_count = match &app.annotation {
        Some(ann) => ann
            .editspec_regions
            .as_ref()
            .map(|r| r.len())
            .unwrap_or(0),
        None => 0,
    };
    app.panel_click_header = REGIONS_HEADER_LINES;
    app.panel_item_count = item_count;

    let panel = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Regions ")
                .title_style(Style::default().fg(Color::Cyan)),
        )
        .scroll((app.panel_scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(panel, area);
}
