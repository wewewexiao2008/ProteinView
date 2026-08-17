//! Four-pane Studio chrome layout (Workflow | Tree | View | EditSpec).

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders};

use crate::app::LayoutMode;
use crate::shell::{PaneId, Shell};

const STRIP: u16 = 1;
const WORKFLOW_BODY: u16 = 6;
const TREE_BODY: u16 = 22;
const EDITSPEC_BODY: u16 = 48;
const TREE_STRIP: u16 = 10;
const EDITSPEC_STRIP: u16 = 10;
const VIEW_STRIP: u16 = 10;

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
        WORKFLOW_BODY.min(area.height.saturating_sub(8)).max(STRIP)
    } else {
        STRIP
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(workflow_h), Constraint::Min(8)])
        .split(area);
    let tree_w = if shell.is_expanded(PaneId::Tree) {
        TREE_BODY
    } else {
        TREE_STRIP
    };
    let editspec_w = if shell.is_expanded(PaneId::EditSpec) {
        EDITSPEC_BODY.min(area.width.saturating_sub(tree_w + 16))
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
    let workflow_h = if shell.is_expanded(PaneId::Workflow) {
        5
    } else {
        STRIP
    };
    let tree_h = if shell.is_expanded(PaneId::Tree) {
        5
    } else {
        STRIP
    };
    let editspec_h = if shell.is_expanded(PaneId::EditSpec) {
        12
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
    let title = if collapsed {
        format!(" {} (collapsed) ", pane.name())
    } else {
        format!(" {} ", pane.name())
    };
    let border = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .title(title)
        .title_style(if focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        })
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

fn intersects(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width
        && b.x < a.x + a.width
        && a.y < b.y + b.height
        && b.y < a.y + a.height
}
