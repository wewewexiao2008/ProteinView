//! Product tree pane. Rows come from gemlib `state.product_tree`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
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

    if app.product_tree.is_empty() {
        let body = Paragraph::new(vec![
            Line::from(Span::styled(
                " No campaign tree",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                " Open a v1 campaign directory.",
                Style::default().fg(Color::Gray),
            )),
        ])
        .wrap(Wrap { trim: false });
        frame.render_widget(body, inner);
        return;
    }

    let rows = app.product_tree.visible_rows();
    let mut lines = Vec::new();
    for (idx, (depth, node)) in rows.iter().enumerate() {
        let marker = if node.children.is_empty() {
            "  "
        } else if node.expanded {
            "▾ "
        } else {
            "▸ "
        };
        let indent = "  ".repeat(*depth);
        let selected = app
            .product_tree
            .selected_sample_id
            .as_deref()
            == Some(node.sample_id.as_str());
        let cursor = idx == app.tree_cursor;
        let mut style = Style::default().fg(Color::Gray);
        if selected {
            style = style.fg(Color::Cyan);
        }
        if cursor {
            style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
        }
        let text = format!("{indent}{marker}{}", node.label);
        lines.push(Line::from(Span::styled(text, style)));
    }
    let body = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(body, inner);
}
