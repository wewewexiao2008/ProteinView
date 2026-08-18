use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::events::EventLevel;

pub fn render_console(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.shell.console_focused;
    let border = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let title = if app.console_verbose {
        " Console · verbose "
    } else {
        " Console "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let visible = app.event_log.visible(app.console_verbose);
    let height = inner.height.max(1) as usize;
    let max_scroll = visible.len().saturating_sub(height);
    let scroll = (app.console_scroll as usize).min(max_scroll);
    let rows: Vec<Line> = visible
        .iter()
        .skip(scroll)
        .take(height)
        .map(|event| {
            let color = match event.level {
                EventLevel::Run => Color::Rgb(255, 200, 0),
                EventLevel::Intent => Color::Cyan,
                EventLevel::Session => Color::Gray,
                EventLevel::Nav => Color::DarkGray,
            };
            Line::from(vec![
                Span::styled(
                    format!("{:<18} ", event.tag()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(event.summary.clone(), Style::default().fg(color)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(rows), inner);
}

#[cfg(test)]
mod tests {
    use crate::events::{EventLevel, EventLog, EventPane, empty_payload};

    #[test]
    fn console_scroll_clamps_to_visible() {
        let mut log = EventLog::default();
        for index in 0..8 {
            log.emit(
                EventLevel::Intent,
                EventPane::Tree,
                "tree.load",
                true,
                format!("load {index}"),
                empty_payload(),
            );
        }
        let visible = log.visible(false);
        let height = 3usize;
        let max_scroll = visible.len().saturating_sub(height);
        let scroll = 10.min(max_scroll);
        assert_eq!(scroll, 5);
        assert_eq!(visible[scroll].summary, "load 5");
    }
}
