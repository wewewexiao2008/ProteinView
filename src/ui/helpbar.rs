use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{ActivePanel, App};

/// Render the keybinding hints bar at the bottom.
pub fn render_helpbar(frame: &mut Frame, area: Rect, app: &App) {
    let spans = if app.edit_state.editing && app.active_panel == ActivePanel::EditSpec {
        // Edit mode keybindings.
        vec![
            Span::styled("\u{2570}\u{2500}\u{2500} ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "Tab",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":field  ", Style::default().fg(Color::Gray)),
            Span::styled(
                "+/-",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":value  ", Style::default().fg(Color::Gray)),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":save  ", Style::default().fg(Color::Gray)),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":cancel ", Style::default().fg(Color::Gray)),
            Span::styled("\u{2500}\u{2500}\u{256f}", Style::default().fg(Color::DarkGray)),
        ]
    } else if app.active_panel == ActivePanel::EditSpec {
        // EditSpec panel view mode keybindings (combined regions + sequence).
        vec![
            Span::styled("\u{2570}\u{2500}\u{2500} ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":edit  ", Style::default().fg(Color::Gray)),
            Span::styled(
                "a",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":add  ", Style::default().fg(Color::Gray)),
            Span::styled(
                "dd",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":del  ", Style::default().fg(Color::Gray)),
            Span::styled(
                "s",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":split  ", Style::default().fg(Color::Gray)),
            Span::styled(
                "j/k",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":nav  ", Style::default().fg(Color::Gray)),
            Span::styled(
                "y",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":yank  ", Style::default().fg(Color::Gray)),
            Span::styled(
                "[/]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":chain  ", Style::default().fg(Color::Gray)),
            Span::styled(
                "u",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":undo  ", Style::default().fg(Color::Gray)),
            Span::styled(
                "?",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(":help ", Style::default().fg(Color::Gray)),
            Span::styled("\u{2500}\u{2500}\u{256f}", Style::default().fg(Color::DarkGray)),
        ]
    } else {
        // Default keybindings.
        vec![
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
        ]
    };

    let help = Paragraph::new(Line::from(spans));
    frame.render_widget(help, area);
}
