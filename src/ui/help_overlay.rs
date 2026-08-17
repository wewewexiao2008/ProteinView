use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::shell::KEY_TABLE;

/// Render a centered help overlay with the Studio key table.
pub fn render_help_overlay(frame: &mut Frame, area: Rect) {
    if area.width < 10 || area.height < 10 {
        return;
    }
    let popup_width = 78u16.min(area.width.saturating_sub(4));
    let popup_height = 28u16.min(area.height.saturating_sub(4));
    let x = (area.width - popup_width) / 2;
    let y = (area.height - popup_height) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);

    let mut help_text = vec![
        Line::from(Span::styled(
            "  Studio key router",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  One exclusive pane focus. Modes: View | Select | EditRegion | Run",
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
    ];
    for (key, meaning) in KEY_TABLE {
        help_text.push(Line::from(vec![
            Span::styled(format!("  {key:<22} "), Style::default().fg(Color::Yellow)),
            Span::raw(*meaning),
        ]));
    }
    help_text.push(Line::from(""));
    help_text.push(Line::from(Span::styled(
        "  Press ? or Esc to close",
        Style::default().fg(Color::DarkGray),
    )));

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Help "),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(help, popup_area);
}
