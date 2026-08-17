//! Run overlay. Owns keys while open; does not spawn local wrappers.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

pub fn render_run_overlay(frame: &mut Frame, area: Rect) {
    if area.width < 10 || area.height < 8 {
        return;
    }
    let popup_width = 64u16.min(area.width.saturating_sub(4));
    let popup_height = 12u16.min(area.height.saturating_sub(4));
    let x = (area.width - popup_width) / 2;
    let y = (area.height - popup_height) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);
    let text = vec![
        Line::from(Span::styled(
            " Run",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::raw(
            " Fleet submit stays an overlay. GPU work is not launched",
        )),
        Line::from(Span::raw(
            " from this laptop. Studio does not spawn RFD/MPNN/Fold wrappers.",
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Esc closes. Ctrl+R is ignored while this overlay is open.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Run "),
            )
            .wrap(Wrap { trim: false }),
        popup_area,
    );
}
