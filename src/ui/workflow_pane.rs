//! Empty Workflow shell. Recipe blocks belong to campaign-block-workflow.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::App;
use crate::shell::PaneId;
use crate::ui::chrome::pane_block;

pub fn render_workflow_pane(frame: &mut Frame, area: Rect, app: &App) {
    let block = pane_block(&app.shell, PaneId::Workflow);
    if !app.shell.is_expanded(PaneId::Workflow) {
        frame.render_widget(block, area);
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let body = Paragraph::new(vec![
        Line::from(Span::styled(
            " Empty shell",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Recipe blocks / iterate / gate live in",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            " 08-18-campaign-block-workflow.",
            Style::default().fg(Color::Cyan),
        )),
        Line::from(Span::styled(
            " This pane does not run a node canvas.",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .wrap(Wrap { trim: false });
    frame.render_widget(body, inner);
}
