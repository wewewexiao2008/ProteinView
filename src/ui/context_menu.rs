use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::{ContextItem, ContextMenu};

/// Render the Action / Label popup at the last right-click cell.
pub fn render_context_menu(frame: &mut Frame, area: Rect, menu: &mut ContextMenu) {
    if area.width < 8 || area.height < 4 {
        return;
    }
    let inner_w = menu
        .items
        .iter()
        .map(|item| item.caption().chars().count())
        .max()
        .unwrap_or(16)
        .saturating_add(2)
        .min(36) as u16;
    let width = (inner_w + 2).min(area.width);
    let max_inner = area.height.saturating_sub(2).max(1) as usize;
    let visible = menu.items.len().min(max_inner).max(1);
    let height = (visible as u16).saturating_add(2).min(area.height);

    let mut x = menu.col;
    let mut y = menu.row;
    if x + width > area.width {
        x = area.width.saturating_sub(width);
    }
    if y + height > area.height {
        y = area.height.saturating_sub(height);
    }
    let popup = Rect::new(x, y, width, height);
    menu.last_rect = popup;

    let mut start = 0usize;
    if menu.cursor >= start + visible {
        start = menu.cursor + 1 - visible;
    }

    frame.render_widget(Clear, popup);

    let mut lines = Vec::new();
    for (offset, item) in menu.items.iter().skip(start).take(visible).enumerate() {
        let idx = start + offset;
        let selected = idx == menu.cursor && item.selectable();
        let style = match item {
            ContextItem::Header(_) => Style::default().fg(Color::DarkGray),
            _ if selected => Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            ContextItem::Action(_) => Style::default().fg(Color::Yellow),
            ContextItem::Label(_) => Style::default().fg(Color::Magenta),
            ContextItem::EditRange | ContextItem::ReplaceOverlap => {
                Style::default().fg(Color::Green)
            }
            ContextItem::Delete => Style::default().fg(Color::Red),
        };
        lines.push(Line::from(Span::styled(item.caption(), style)));
    }

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Action / Label "),
        ),
        popup,
    );
}

pub fn hit_test_item(menu: &ContextMenu, col: u16, row: u16) -> Option<usize> {
    let rect = menu.last_rect;
    if rect.width == 0 || rect.height == 0 {
        return None;
    }
    if col < rect.x || col >= rect.x + rect.width || row < rect.y || row >= rect.y + rect.height {
        return None;
    }
    if row <= rect.y || row + 1 >= rect.y + rect.height {
        return None;
    }
    let max_inner = rect.height.saturating_sub(2).max(1) as usize;
    let visible = menu.items.len().min(max_inner).max(1);
    let mut start = 0usize;
    if menu.cursor >= start + visible {
        start = menu.cursor + 1 - visible;
    }
    let idx = start + (row.saturating_sub(rect.y + 1) as usize);
    if idx < menu.items.len() && menu.items[idx].selectable() {
        Some(idx)
    } else {
        None
    }
}
