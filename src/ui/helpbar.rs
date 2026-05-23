use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Render the keybinding hints bar at the bottom
pub fn render_helpbar(frame: &mut Frame, area: Rect) {
    let help = Paragraph::new(Line::from(vec![
        Span::styled("\u{2570}\u{2500}\u{2500} ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "h/l",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": rotY  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "j/k",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": rotX  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "+/-",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": zoom  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "Tab",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": panels  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "f",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": close  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "c",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": color  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "v",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": mode  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "?",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": help  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "q",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": quit ", Style::default().fg(Color::Gray)),
        Span::styled("\u{2500}\u{2500}\u{256f}", Style::default().fg(Color::DarkGray)),
    ]));
    frame.render_widget(help, area);
}
