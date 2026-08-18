//! Exclusive add-block overlay for the Workflow pane.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::workflow::WorkflowNode;

#[derive(Debug, Clone)]
pub struct BlockChoice {
    pub block: String,
    pub caption: String,
}

#[derive(Debug, Clone)]
pub struct BlockPalette {
    pub cursor: usize,
    pub items: Vec<BlockChoice>,
    pub last_rect: Rect,
}

impl BlockPalette {
    pub fn from_nodes(nodes: &[WorkflowNode]) -> Self {
        let mut items = vec![BlockChoice {
            block: "import".to_string(),
            caption: "import".to_string(),
        }];
        if !nodes.iter().any(|node| node.kind == "loop") {
            items.push(BlockChoice {
                block: "loop".to_string(),
                caption: "loop".to_string(),
            });
        }
        let has_rfd = nodes.iter().any(|node| node.block == "rfd" && node.kind == "step");
        if !has_rfd && nodes.iter().any(|node| node.kind == "loop") {
            items.push(BlockChoice {
                block: "rfd".to_string(),
                caption: "rfd (loop)".to_string(),
            });
        }
        Self {
            cursor: 0,
            items,
            last_rect: Rect::default(),
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.items.is_empty() {
            self.cursor = 0;
            return;
        }
        let next = self.cursor as isize + delta;
        self.cursor = next.clamp(0, self.items.len() as isize - 1) as usize;
    }

    pub fn selected_block(&self) -> Option<&str> {
        self.items.get(self.cursor).map(|item| item.block.as_str())
    }
}

pub fn render_block_palette(frame: &mut Frame, area: Rect, palette: &mut BlockPalette) {
    if area.width < 8 || area.height < 4 || palette.items.is_empty() {
        return;
    }
    let inner_w = palette
        .items
        .iter()
        .map(|item| item.caption.chars().count())
        .max()
        .unwrap_or(12)
        .saturating_add(2)
        .min(28) as u16;
    let width = (inner_w + 2).min(area.width);
    let height = ((palette.items.len() as u16) + 2).min(area.height);
    let x = area.width.saturating_sub(width) / 4;
    let y = 2;
    let popup = Rect::new(x, y, width, height);
    palette.last_rect = popup;
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    for (idx, item) in palette.items.iter().enumerate() {
        let style = if idx == palette.cursor {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Yellow)
        };
        lines.push(Line::from(Span::styled(item.caption.clone(), style)));
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Add block "),
        ),
        popup,
    );
}

pub fn hit_test_item(palette: &BlockPalette, col: u16, row: u16) -> Option<usize> {
    let rect = palette.last_rect;
    if rect.width < 3 || rect.height < 3 {
        return None;
    }
    if col < rect.x.saturating_add(1)
        || col >= rect.x.saturating_add(rect.width).saturating_sub(1)
        || row < rect.y.saturating_add(1)
        || row >= rect.y.saturating_add(rect.height).saturating_sub(1)
    {
        return None;
    }
    let idx = (row - rect.y.saturating_add(1)) as usize;
    (idx < palette.items.len()).then_some(idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{compose_draft_node, loop_draft_node};

    #[test]
    fn palette_offers_loop_only_when_missing() {
        let compose_only = vec![compose_draft_node("seed", "import")];
        let items: Vec<_> = BlockPalette::from_nodes(&compose_only)
            .items
            .into_iter()
            .map(|item| item.block)
            .collect();
        assert!(items.contains(&"import".to_string()));
        assert!(items.contains(&"loop".to_string()));
        assert!(!items.contains(&"rfd".to_string()));
        assert!(!items.contains(&"probe".to_string()));
        assert!(!items.contains(&"de_novo".to_string()));
        assert!(!items.contains(&"edit_rfd".to_string()));

        let with_loop = vec![
            compose_draft_node("seed", "import"),
            loop_draft_node("optimize", 2),
        ];
        let items: Vec<_> = BlockPalette::from_nodes(&with_loop)
            .items
            .into_iter()
            .map(|item| item.block)
            .collect();
        assert!(!items.contains(&"loop".to_string()));
        assert!(items.contains(&"rfd".to_string()));
    }
}
