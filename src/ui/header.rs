use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Render the retro-styled header with protein name and Python bridge status.
pub fn render_header(frame: &mut Frame, area: Rect, protein_name: &str, python_available: bool) {
    let title_text = format!(" ProteinView ─── {} ", protein_name);

    // Build the right-side indicator showing bridge status.
    let mode_label = if python_available { "" } else { " [Read-only]" };
    let mode_len = mode_label.len();

    let fill_len = (area.width as usize).saturating_sub(title_text.len() + 4 + mode_len) / 2;
    let fill = " ─".repeat(fill_len);

    let mut spans = vec![
        Span::styled("╭─── ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "ProteinView",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ─── ", Style::default().fg(Color::DarkGray)),
        Span::styled(protein_name, Style::default().fg(Color::Yellow)),
        Span::styled(fill, Style::default().fg(Color::DarkGray)),
    ];

    if !python_available {
        spans.push(Span::styled(
            " [Read-only]",
            Style::default().fg(Color::Rgb(255, 165, 0)),
        ));
    }

    spans.push(Span::styled("╮", Style::default().fg(Color::DarkGray)));

    let header = Paragraph::new(Line::from(spans));
    frame.render_widget(header, area);
}
