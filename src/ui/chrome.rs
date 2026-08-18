//! Four-pane Studio chrome layout (Workflow | Tree | View | EditSpec).

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders};

use crate::app::LayoutMode;
use crate::shell::{PaneId, Shell};

const STRIP: u16 = 1;
const TREE_STRIP: u16 = 10;
const EDITSPEC_STRIP: u16 = 10;
const VIEW_STRIP: u16 = 10;
const FOLD_COLS: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeDrag {
    WorkflowHeight,
    TreeWidth,
    EditSpecWidth,
    TreeHeight,
    EditSpecHeight,
}

#[derive(Debug, Clone, Copy)]
pub struct ChromeRects {
    pub workflow: Rect,
    pub tree: Rect,
    pub view: Rect,
    pub editspec: Rect,
}

pub fn split_chrome(area: Rect, shell: &Shell, layout: LayoutMode) -> ChromeRects {
    match layout {
        LayoutMode::Horizontal => split_horizontal(area, shell),
        LayoutMode::Vertical => split_vertical(area, shell),
    }
}

fn split_horizontal(area: Rect, shell: &Shell) -> ChromeRects {
    let workflow_h = if shell.is_expanded(PaneId::Workflow) {
        shell
            .workflow_h
            .min(area.height.saturating_sub(8))
            .max(3)
    } else {
        STRIP
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(workflow_h), Constraint::Min(8)])
        .split(area);
    let tree_w = if shell.is_expanded(PaneId::Tree) {
        shell.tree_w.min(area.width.saturating_sub(32)).max(8)
    } else {
        TREE_STRIP
    };
    let editspec_w = if shell.is_expanded(PaneId::EditSpec) {
        shell
            .editspec_w
            .min(area.width.saturating_sub(tree_w + 16))
            .max(12)
    } else {
        EDITSPEC_STRIP
    };
    let view_w = if shell.is_expanded(PaneId::View) {
        0
    } else {
        VIEW_STRIP
    };
    let cols = if view_w == 0 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(tree_w),
                Constraint::Min(16),
                Constraint::Length(editspec_w),
            ])
            .split(rows[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(tree_w),
                Constraint::Length(view_w),
                Constraint::Min(8),
            ])
            .split(rows[1])
    };
    ChromeRects {
        workflow: rows[0],
        tree: cols[0],
        view: cols[1],
        editspec: cols[2],
    }
}

fn split_vertical(area: Rect, shell: &Shell) -> ChromeRects {
    let remain = area.height.saturating_sub(10);
    let workflow_h = if shell.is_expanded(PaneId::Workflow) {
        shell.workflow_h.min(remain.max(3) / 2).max(3)
    } else {
        STRIP
    };
    let tree_h = if shell.is_expanded(PaneId::Tree) {
        shell.tree_h.min(remain.max(3) / 2).max(3)
    } else {
        STRIP
    };
    let editspec_h = if shell.is_expanded(PaneId::EditSpec) {
        shell.editspec_h.min(remain.max(4)).max(4)
    } else {
        STRIP
    };
    let view_h = if shell.is_expanded(PaneId::View) {
        0
    } else {
        STRIP
    };
    let rows = if view_h == 0 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(workflow_h),
                Constraint::Length(tree_h),
                Constraint::Min(8),
                Constraint::Length(editspec_h),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(workflow_h),
                Constraint::Length(tree_h),
                Constraint::Length(view_h),
                Constraint::Min(6),
            ])
            .split(area)
    };
    ChromeRects {
        workflow: rows[0],
        tree: rows[1],
        view: rows[2],
        editspec: rows[3],
    }
}

pub fn pane_block(shell: &Shell, pane: PaneId) -> Block<'static> {
    let focused = shell.focused == pane;
    let collapsed = !shell.is_expanded(pane);
    let title = format!(" {} ", pane.name());
    let badge = if collapsed { " ▸ " } else { " ▾ " };
    let border = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let title_style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .title(title)
        .title(Line::from(badge).right_aligned())
        .title_style(title_style)
}

pub fn pane_at(rects: &ChromeRects, col: u16, row: u16) -> Option<PaneId> {
    let point = Rect::new(col, row, 1, 1);
    if intersects(rects.workflow, point) {
        Some(PaneId::Workflow)
    } else if intersects(rects.tree, point) {
        Some(PaneId::Tree)
    } else if intersects(rects.view, point) {
        Some(PaneId::View)
    } else if intersects(rects.editspec, point) {
        Some(PaneId::EditSpec)
    } else {
        None
    }
}

pub fn fold_hit(rect: Rect, col: u16, row: u16) -> bool {
    if rect.width < FOLD_COLS || rect.height < 1 {
        return false;
    }
    let start = rect.x.saturating_add(rect.width.saturating_sub(FOLD_COLS));
    row == rect.y && col >= start && col < rect.x.saturating_add(rect.width)
}

pub fn fold_pane_at(rects: &ChromeRects, col: u16, row: u16) -> Option<PaneId> {
    if fold_hit(rects.workflow, col, row) {
        Some(PaneId::Workflow)
    } else if fold_hit(rects.tree, col, row) {
        Some(PaneId::Tree)
    } else if fold_hit(rects.view, col, row) {
        Some(PaneId::View)
    } else if fold_hit(rects.editspec, col, row) {
        Some(PaneId::EditSpec)
    } else {
        None
    }
}

pub fn divider_at(
    rects: &ChromeRects,
    layout: LayoutMode,
    col: u16,
    row: u16,
) -> Option<ChromeDrag> {
    match layout {
        LayoutMode::Horizontal => {
            if on_v_edge(col, rects.tree, rects.view) && in_y_span(row, rects.tree, rects.view) {
                return Some(ChromeDrag::TreeWidth);
            }
            if on_v_edge(col, rects.view, rects.editspec)
                && in_y_span(row, rects.view, rects.editspec)
            {
                return Some(ChromeDrag::EditSpecWidth);
            }
            if on_h_edge(row, rects.workflow, rects.tree)
                && in_x_span(col, rects.workflow, rects.workflow)
            {
                return Some(ChromeDrag::WorkflowHeight);
            }
            None
        }
        LayoutMode::Vertical => {
            if on_h_edge(row, rects.tree, rects.view) && in_x_span(col, rects.tree, rects.view) {
                return Some(ChromeDrag::TreeHeight);
            }
            if on_h_edge(row, rects.view, rects.editspec)
                && in_x_span(col, rects.view, rects.editspec)
            {
                return Some(ChromeDrag::EditSpecHeight);
            }
            if on_h_edge(row, rects.workflow, rects.tree)
                && in_x_span(col, rects.workflow, rects.workflow)
            {
                return Some(ChromeDrag::WorkflowHeight);
            }
            None
        }
    }
}

pub fn apply_drag(shell: &mut Shell, drag: ChromeDrag, rects: &ChromeRects, col: u16, row: u16) {
    match drag {
        ChromeDrag::WorkflowHeight => {
            let max_h = rects
                .workflow
                .height
                .saturating_add(rects.view.height)
                .saturating_sub(8)
                .max(3);
            let new_h = row.saturating_sub(rects.workflow.y).clamp(3, max_h);
            shell.workflow_h = new_h;
            if new_h > STRIP + 1 {
                shell.set_expanded(PaneId::Workflow, true);
            }
        }
        ChromeDrag::TreeWidth => {
            let max_w = rects
                .tree
                .width
                .saturating_add(rects.view.width)
                .saturating_sub(16)
                .max(8);
            let new_w = col.saturating_sub(rects.tree.x).clamp(8, max_w);
            shell.tree_w = new_w;
            if new_w > TREE_STRIP + 2 {
                shell.set_expanded(PaneId::Tree, true);
            }
        }
        ChromeDrag::EditSpecWidth => {
            let right = rects.editspec.x.saturating_add(rects.editspec.width);
            let max_w = rects
                .editspec
                .width
                .saturating_add(rects.view.width)
                .saturating_sub(16)
                .max(12);
            let new_w = right.saturating_sub(col).clamp(12, max_w);
            shell.editspec_w = new_w;
            if new_w > EDITSPEC_STRIP + 2 {
                shell.set_expanded(PaneId::EditSpec, true);
            }
        }
        ChromeDrag::TreeHeight => {
            let max_h = rects
                .tree
                .height
                .saturating_add(rects.view.height)
                .saturating_sub(8)
                .max(3);
            let new_h = row.saturating_sub(rects.tree.y).clamp(3, max_h);
            shell.tree_h = new_h;
            if new_h > STRIP + 1 {
                shell.set_expanded(PaneId::Tree, true);
            }
        }
        ChromeDrag::EditSpecHeight => {
            let bottom = rects.editspec.y.saturating_add(rects.editspec.height);
            let max_h = rects
                .editspec
                .height
                .saturating_add(rects.view.height)
                .saturating_sub(8)
                .max(4);
            let new_h = bottom.saturating_sub(row).clamp(4, max_h);
            shell.editspec_h = new_h;
            if new_h > STRIP + 1 {
                shell.set_expanded(PaneId::EditSpec, true);
            }
        }
    }
}

fn on_v_edge(col: u16, left: Rect, right: Rect) -> bool {
    col == right.x || col == left.x.saturating_add(left.width.saturating_sub(1))
}

fn on_h_edge(row: u16, top: Rect, bottom: Rect) -> bool {
    row == bottom.y || row == top.y.saturating_add(top.height.saturating_sub(1))
}

fn in_y_span(row: u16, a: Rect, b: Rect) -> bool {
    let y0 = a.y.min(b.y);
    let y1 = a
        .y
        .saturating_add(a.height)
        .max(b.y.saturating_add(b.height));
    row >= y0 && row < y1
}

fn in_x_span(col: u16, a: Rect, b: Rect) -> bool {
    let x0 = a.x.min(b.x);
    let x1 = a
        .x
        .saturating_add(a.width)
        .max(b.x.saturating_add(b.width));
    col >= x0 && col < x1
}

fn intersects(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width
        && b.x < a.x + a.width
        && a.y < b.y + b.height
        && b.y < a.y + a.height
}

#[cfg(test)]
mod tests {
    use super::*;

    fn horiz_campaign() -> (Shell, ChromeRects) {
        let shell = Shell::campaign_session();
        let rects = split_chrome(Rect::new(0, 0, 120, 40), &shell, LayoutMode::Horizontal);
        (shell, rects)
    }

    #[test]
    fn fold_hit_is_top_right_three_cells() {
        let rect = Rect::new(10, 5, 20, 8);
        assert!(fold_hit(rect, 27, 5));
        assert!(fold_hit(rect, 29, 5));
        assert!(!fold_hit(rect, 26, 5));
        assert!(!fold_hit(rect, 29, 6));
    }

    #[test]
    fn fold_pane_at_picks_tree_badge() {
        let (_shell, rects) = horiz_campaign();
        let col = rects.tree.x + rects.tree.width - 1;
        assert_eq!(fold_pane_at(&rects, col, rects.tree.y), Some(PaneId::Tree));
        assert_eq!(fold_pane_at(&rects, rects.tree.x + 1, rects.tree.y), None);
    }

    #[test]
    fn divider_at_tree_right_is_tree_width() {
        let (_shell, rects) = horiz_campaign();
        let row = rects.tree.y + 2;
        assert_eq!(
            divider_at(&rects, LayoutMode::Horizontal, rects.view.x, row),
            Some(ChromeDrag::TreeWidth)
        );
        assert_eq!(
            divider_at(
                &rects,
                LayoutMode::Horizontal,
                rects.tree.x + 2,
                row
            ),
            None
        );
    }

    #[test]
    fn divider_at_editspec_left_is_editspec_width() {
        let (_shell, rects) = horiz_campaign();
        let row = rects.editspec.y + 2;
        assert_eq!(
            divider_at(&rects, LayoutMode::Horizontal, rects.editspec.x, row),
            Some(ChromeDrag::EditSpecWidth)
        );
    }

    #[test]
    fn divider_at_workflow_bottom_is_workflow_height() {
        let (_shell, rects) = horiz_campaign();
        assert_eq!(
            divider_at(
                &rects,
                LayoutMode::Horizontal,
                rects.workflow.x + 4,
                rects.tree.y
            ),
            Some(ChromeDrag::WorkflowHeight)
        );
    }

    #[test]
    fn apply_drag_widens_tree() {
        let (mut shell, rects) = horiz_campaign();
        let start = shell.tree_w;
        apply_drag(
            &mut shell,
            ChromeDrag::TreeWidth,
            &rects,
            rects.tree.x + 30,
            rects.tree.y + 2,
        );
        assert!(shell.tree_w > start);
        assert!(shell.is_expanded(PaneId::Tree));
    }
}
