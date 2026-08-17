//! Empty Tree shell. Campaign sample lineage belongs to studio-product-tree.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::App;
use crate::shell::PaneId;
use crate::ui::chrome::pane_block;

pub fn render_tree_pane(frame: &mut Frame, area: Rect, app: &App) {
    let block = pane_block(&app.shell, PaneId::Tree);
    if !app.shell.is_expanded(PaneId::Tree) {
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
            " Campaign sample lineage is",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            " 08-18-studio-product-tree.",
            Style::default().fg(Color::Cyan),
        )),
        Line::from(Span::styled(
            " This session does not render samples.",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .wrap(Wrap { trim: false });
    frame.render_widget(body, inner);
}
