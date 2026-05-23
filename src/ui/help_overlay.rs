use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

/// Render a centered help overlay
pub fn render_help_overlay(frame: &mut Frame, area: Rect) {
    // Center the popup -- guard against tiny terminals
    if area.width < 10 || area.height < 10 {
        return;
    }
    let popup_width = 60u16.min(area.width.saturating_sub(4));
    let popup_height = 40u16.min(area.height.saturating_sub(4));
    let x = (area.width - popup_width) / 2;
    let y = (area.height - popup_height) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(Span::styled(
            "  Keybindings",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  h / l      ", Style::default().fg(Color::Yellow)),
            Span::raw("Rotate Y-axis"),
        ]),
        Line::from(vec![
            Span::styled("  j / k      ", Style::default().fg(Color::Yellow)),
            Span::raw("Rotate X-axis"),
        ]),
        Line::from(vec![
            Span::styled("  u / i      ", Style::default().fg(Color::Yellow)),
            Span::raw("Rotate Z-axis (roll)"),
        ]),
        Line::from(vec![
            Span::styled("  + / -      ", Style::default().fg(Color::Yellow)),
            Span::raw("Zoom in / out"),
        ]),
        Line::from(vec![
            Span::styled("  w/a/s/d    ", Style::default().fg(Color::Yellow)),
            Span::raw("Pan up/left/down/right"),
        ]),
        Line::from(vec![
            Span::styled("  r          ", Style::default().fg(Color::Yellow)),
            Span::raw("Reset view"),
        ]),
        Line::from(vec![
            Span::styled("  c          ", Style::default().fg(Color::Yellow)),
            Span::raw("Cycle color scheme"),
        ]),
        Line::from(vec![
            Span::styled("  v          ", Style::default().fg(Color::Yellow)),
            Span::raw("Cycle viz mode"),
        ]),
        Line::from(vec![
            Span::styled("  m          ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle Braille / HD"),
        ]),
        Line::from(vec![
            Span::styled("  M          ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle HD / FullHD (Sixel/Kitty)"),
        ]),
        Line::from(vec![
            Span::styled("  [ / ]      ", Style::default().fg(Color::Yellow)),
            Span::raw("Prev / next chain"),
        ]),
        Line::from(vec![
            Span::styled("  Tab        ", Style::default().fg(Color::Yellow)),
            Span::raw("Cycle sidebar panels"),
        ]),
        Line::from(vec![
            Span::styled("  f          ", Style::default().fg(Color::Yellow)),
            Span::raw("Close current panel"),
        ]),
        Line::from(vec![
            Span::styled("  F          ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle interface analysis"),
        ]),
        Line::from(vec![
            Span::styled("  I          ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle interface interactions"),
        ]),
        Line::from(vec![
            Span::styled("  g          ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle ligand visibility"),
        ]),
        Line::from(vec![
            Span::styled("  Space      ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle auto-rotation"),
        ]),
        // EditSpec panel section.
        Line::from(""),
        Line::from(Span::styled(
            "  EditSpec Panel",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  Enter      ", Style::default().fg(Color::Yellow)),
            Span::raw("Edit focused region"),
        ]),
        Line::from(vec![
            Span::styled("  a          ", Style::default().fg(Color::Yellow)),
            Span::raw("Add new region"),
        ]),
        Line::from(vec![
            Span::styled("  dd         ", Style::default().fg(Color::Yellow)),
            Span::raw("Delete focused region"),
        ]),
        Line::from(vec![
            Span::styled("  s          ", Style::default().fg(Color::Yellow)),
            Span::raw("Split region at midpoint"),
        ]),
        Line::from(vec![
            Span::styled("  j / k      ", Style::default().fg(Color::Yellow)),
            Span::raw("Navigate regions"),
        ]),
        Line::from(vec![
            Span::styled("  [ / ]      ", Style::default().fg(Color::Yellow)),
            Span::raw("Switch chain"),
        ]),
        Line::from(vec![
            Span::styled("  y          ", Style::default().fg(Color::Yellow)),
            Span::raw("Yank selected range (e.g. A:51-80)"),
        ]),
        Line::from(vec![
            Span::styled("  Y          ", Style::default().fg(Color::Yellow)),
            Span::raw("Yank selected sequence letters"),
        ]),
        Line::from(vec![
            Span::styled("  u          ", Style::default().fg(Color::Yellow)),
            Span::raw("Undo last edit"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+r     ", Style::default().fg(Color::Yellow)),
            Span::raw("Redo last undo"),
        ]),
        // Edit mode section.
        Line::from(""),
        Line::from(Span::styled(
            "  Edit Mode",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  Tab / j    ", Style::default().fg(Color::Yellow)),
            Span::raw("Next field"),
        ]),
        Line::from(vec![
            Span::styled("  Shift+Tab / k", Style::default().fg(Color::Yellow)),
            Span::raw("Previous field"),
        ]),
        Line::from(vec![
            Span::styled("  +/- / h/l  ", Style::default().fg(Color::Yellow)),
            Span::raw("Adjust value / cycle option"),
        ]),
        Line::from(vec![
            Span::styled("  Enter      ", Style::default().fg(Color::Yellow)),
            Span::raw("Save changes"),
        ]),
        Line::from(vec![
            Span::styled("  Esc        ", Style::default().fg(Color::Yellow)),
            Span::raw("Cancel edit"),
        ]),
        Line::from(vec![
            Span::styled("  Tab (label)", Style::default().fg(Color::Yellow)),
            Span::raw("Cycle predefined labels"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Press ? or Esc to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

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
