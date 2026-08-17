use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::shell::InteractionMode;

/// Render the keybinding hints bar at the bottom.
pub fn render_helpbar(frame: &mut Frame, area: Rect, app: &App) {
    let spans = match app.shell.mode {
        InteractionMode::EditRegion => hint_line(&[
            ("Tab", "field"),
            ("type", "A51-80"),
            ("Enter", "save"),
            ("Esc", "back"),
        ]),
        InteractionMode::Select => hint_line(&[
            ("h/l", "cursor"),
            ("H/L", "edge"),
            ("s", "segment"),
            ("[/]", "jump"),
            ("1-5", "action"),
            ("Esc", "View"),
        ]),
        InteractionMode::Run => hint_line(&[("Esc", "close Run"), ("Ctrl+R", "ignored")]),
        InteractionMode::View if app.shell.editspec_focused() => hint_line(&[
            ("j/k", "regions"),
            ("x", "Select"),
            ("Enter", "form"),
            ("e", "edit"),
            ("Tab", "pane"),
            ("f", "fold"),
        ]),
        InteractionMode::View => hint_line(&[
            ("h/l j/k", "rotate"),
            ("x", "Select"),
            ("Ctrl+R", "Run"),
            ("Tab", "pane"),
            ("f", "fold"),
            ("q", "quit"),
        ]),
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn hint_line(pairs: &[(&str, &str)]) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        "\u{2570}\u{2500}\u{2500} ",
        Style::default().fg(Color::DarkGray),
    )];
    for (key, label) in pairs {
        spans.push(Span::styled(
            (*key).to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(":{label}  "),
            Style::default().fg(Color::Gray),
        ));
    }
    spans.push(Span::styled(
        "\u{2500}\u{2500}\u{256f}",
        Style::default().fg(Color::DarkGray),
    ));
    spans
}
