//! Parse graph as boxes, arrows, and a gate loop-back in the Workflow strip.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::shell::PaneId;
use crate::ui::chrome::pane_block;
use crate::workflow::WorkflowNode;

const BOX_H: u16 = 3;
const COMPOSE_W: u16 = 12;
const STEP_W: u16 = 9;
const GATE_W: u16 = 10;

pub fn render_workflow_pane(frame: &mut Frame, area: Rect, app: &App) {
    let block = pane_block(&app.shell, PaneId::Workflow);
    if !app.shell.is_expanded(PaneId::Workflow) {
        frame.render_widget(block, area);
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(error) = &app.workflow_error {
        let where_node = error
            .node_id
            .as_deref()
            .map(|id| format!("  {id}"))
            .unwrap_or_default();
        let body = Paragraph::new(vec![
            Line::from(Span::styled(
                format!(" parse {}{where_node}", error.rule),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!(" {}", error.message),
                Style::default().fg(Color::Gray),
            )),
        ]);
        frame.render_widget(body, inner);
        return;
    }

    let Some(status) = &app.workflow_status else {
        let body = Paragraph::new(Line::from(Span::styled(
            " No recipe graph",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(body, inner);
        return;
    };

    let boxes = layout_workflow(area, &status.nodes);
    let mut header = String::new();
    if let Some(loop_node) = status.nodes.iter().find(|node| node.kind == "loop") {
        header.push_str(&display_block(loop_node));
        header.push_str("  ");
    }
    if status.draft {
        header.push_str("草稿 ");
    }
    if status.can_run {
        header.push_str("can_run");
    } else {
        header.push_str("cannot run");
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_line(&format!(" {header}"), inner.width),
            if status.draft {
                Style::default().fg(Color::Yellow)
            } else if status.can_run {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Gray)
            },
        ))),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    for (idx, rect) in &boxes {
        let Some(node) = status.nodes.get(*idx) else {
            continue;
        };
        if rect.width < 3 || rect.height == 0 {
            continue;
        }
        let cursor = *idx == app.workflow_cursor;
        let border = if cursor {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if node.waiting {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Gray)
        };
        if rect.height == 1 {
            continue;
        }
        let title = truncate_line(&display_block(node), rect.width.saturating_sub(2));
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border)
                .title(title),
            *rect,
        );
        if rect.height >= 3 && rect.width >= 4 {
            let lamp = format!(" {} {}", light_glyph(&node.light, node.waiting), extra_note(node));
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    truncate_line(&lamp, rect.width.saturating_sub(2)),
                    if cursor {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Gray)
                    },
                ))),
                Rect::new(
                    rect.x.saturating_add(1),
                    rect.y.saturating_add(1),
                    rect.width.saturating_sub(2),
                    1,
                ),
            );
        }
    }

    let rounds = status
        .nodes
        .iter()
        .find(|node| node.kind == "loop")
        .map(|node| node.rounds)
        .filter(|rounds| *rounds > 0);
    draw_arrows(frame, &boxes, &status.nodes, inner, rounds);
}

pub fn layout_workflow(outer: Rect, nodes: &[WorkflowNode]) -> Vec<(usize, Rect)> {
    if outer.width < 5 || outer.height < 3 || nodes.is_empty() {
        return Vec::new();
    }
    let inner = Rect {
        x: outer.x.saturating_add(1),
        y: outer.y.saturating_add(2),
        width: outer.width.saturating_sub(2),
        height: outer.height.saturating_sub(3),
    };
    if inner.width < 8 || inner.height == 0 {
        return Vec::new();
    }
    let compose: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.kind == "compose")
        .map(|(idx, _)| idx)
        .collect();
    let steps: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.kind == "step")
        .map(|(idx, _)| idx)
        .collect();
    let loop_idx = nodes.iter().enumerate().find(|(_, node)| node.kind == "loop").map(|(idx, _)| idx);
    let gate_idx = nodes.iter().enumerate().find(|(_, node)| node.kind == "gate").map(|(idx, _)| idx);

    let compose_w = COMPOSE_W.min(inner.width / 4).max(8);
    let step_w = STEP_W.min(inner.width / 6).max(7);
    let gate_w = GATE_W.min(inner.width / 5).max(8);
    let box_h = BOX_H.min(inner.height).max(1);
    let depths = compose_depths(nodes, &compose);
    let max_depth = depths.iter().copied().max().unwrap_or(0);
    let col_stride = compose_w.saturating_add(3);

    let mut boxes = Vec::new();
    let mut stack_at: Vec<u16> = vec![0; max_depth as usize + 1];
    for idx in &compose {
        let depth = depths
            .get(compose.iter().position(|item| item == idx).unwrap_or(0))
            .copied()
            .unwrap_or(0);
        let stack = stack_at.get(depth as usize).copied().unwrap_or(0);
        if let Some(slot) = stack_at.get_mut(depth as usize) {
            *slot = slot.saturating_add(1);
        }
        let x = inner.x.saturating_add(depth.saturating_mul(col_stride));
        let y = inner.y.saturating_add(stack.saturating_mul(box_h));
        if x.saturating_add(compose_w) > inner.x.saturating_add(inner.width)
            || y.saturating_add(box_h) > inner.y.saturating_add(inner.height)
        {
            continue;
        }
        boxes.push((*idx, Rect::new(x, y, compose_w.min(inner.width), box_h)));
    }

    let compose_right = inner
        .x
        .saturating_add((max_depth.saturating_add(1)).saturating_mul(col_stride));
    let mid_x = if compose.is_empty() {
        inner.x
    } else {
        compose_right.min(inner.x.saturating_add(inner.width.saturating_sub(step_w)))
    };
    if let Some(idx) = loop_idx {
        boxes.push((
            idx,
            Rect::new(inner.x, outer.y.saturating_add(1), inner.width.max(1), 1),
        ));
    }
    let mut last_step_right = mid_x;
    for (col, idx) in steps.iter().enumerate() {
        let x = mid_x.saturating_add((col as u16).saturating_mul(step_w.saturating_add(4)));
        if x.saturating_add(step_w) > inner.x.saturating_add(inner.width) {
            break;
        }
        boxes.push((*idx, Rect::new(x, inner.y, step_w, box_h.min(inner.height))));
        last_step_right = x.saturating_add(step_w);
    }

    if let Some(idx) = gate_idx {
        let x = last_step_right
            .saturating_add(3)
            .min(
                inner
                    .x
                    .saturating_add(inner.width)
                    .saturating_sub(gate_w),
            )
            .max(mid_x);
        boxes.push((idx, Rect::new(x, inner.y, gate_w.min(inner.width), box_h.min(inner.height))));
    }
    boxes
}

pub fn hit_test(outer: Rect, col: u16, row: u16, nodes: &[WorkflowNode]) -> Option<usize> {
    for (idx, rect) in layout_workflow(outer, nodes) {
        if col >= rect.x
            && col < rect.x.saturating_add(rect.width)
            && row >= rect.y
            && row < rect.y.saturating_add(rect.height)
        {
            return Some(idx);
        }
    }
    None
}

pub fn loop_return_index(nodes: &[WorkflowNode]) -> Option<usize> {
    nodes
        .iter()
        .position(|node| node.kind == "step" && node.block == "mpnn")
        .or_else(|| nodes.iter().position(|node| node.kind == "step"))
}

fn draw_arrows(
    frame: &mut Frame,
    boxes: &[(usize, Rect)],
    nodes: &[WorkflowNode],
    inner: Rect,
    rounds: Option<u32>,
) {
    let style = Style::default().fg(Color::Gray);
    let rect_of = |want: usize| {
        boxes
            .iter()
            .find(|(idx, _)| *idx == want)
            .map(|(_, rect)| *rect)
    };
    let steps: Vec<Rect> = boxes
        .iter()
        .filter(|(idx, _)| nodes.get(*idx).is_some_and(|node| node.kind == "step"))
        .map(|(_, rect)| *rect)
        .collect();
    let gate = boxes
        .iter()
        .find(|(idx, _)| nodes.get(*idx).is_some_and(|node| node.kind == "gate"))
        .map(|(_, rect)| *rect);

    for (idx, node) in nodes.iter().enumerate() {
        if node.kind != "compose" {
            continue;
        }
        let Some(to) = rect_of(idx) else {
            continue;
        };
        for source in &node.inputs {
            let Some(from_idx) = nodes.iter().position(|item| item.id == *source) else {
                continue;
            };
            if let Some(from) = rect_of(from_idx) {
                draw_directed_link(frame, from, to, style, inner);
            }
        }
    }
    let seed_rect = nodes
        .iter()
        .find(|node| node.kind == "loop")
        .and_then(|node| node.inputs.first())
        .and_then(|id| nodes.iter().position(|node| node.id == *id))
        .and_then(rect_of);
    if let (Some(from), Some(to)) = (seed_rect, steps.first()) {
        draw_directed_link(frame, from, *to, style, inner);
    }
    for pair in steps.windows(2) {
        draw_forward_link(frame, pair[0], pair[1], style, inner);
    }
    if let (Some(from), Some(to)) = (steps.last(), gate) {
        draw_forward_link(frame, *from, to, style, inner);
    }
    if let (Some(mpnn), Some(gate_rect)) = (
        loop_return_index(nodes).and_then(rect_of),
        gate,
    ) {
        draw_return_to_mpnn(frame, mpnn, gate_rect, rounds, style, inner);
    }
}

fn compose_depths(nodes: &[WorkflowNode], compose: &[usize]) -> Vec<u16> {
    let index_by_id: std::collections::HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.kind == "compose")
        .map(|(idx, node)| (node.id.as_str(), idx))
        .collect();
    let mut depths = vec![0u16; compose.len()];
    for _ in 0..compose.len().saturating_add(1) {
        let mut changed = false;
        for (slot, idx) in compose.iter().enumerate() {
            let Some(node) = nodes.get(*idx) else {
                continue;
            };
            let parent = node
                .inputs
                .iter()
                .filter_map(|id| index_by_id.get(id.as_str()).copied())
                .filter_map(|parent_idx| compose.iter().position(|item| *item == parent_idx))
                .map(|parent_slot| depths[parent_slot].saturating_add(1))
                .max()
                .unwrap_or(0);
            if parent != depths[slot] {
                depths[slot] = parent;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    depths
}

fn draw_directed_link(frame: &mut Frame, from: Rect, to: Rect, style: Style, clip: Rect) {
    let y0 = from.y.saturating_add(1);
    let y1 = to.y.saturating_add(1);
    let start = from.x.saturating_add(from.width);
    if to.x <= start {
        return;
    }
    if y0 == y1 {
        draw_forward_link(frame, from, to, style, clip);
        return;
    }
    if y1 > y0 {
        put_span(frame, start, y0, "┐", style, clip);
        for y in (y0.saturating_add(1))..y1 {
            put_span(frame, start, y, "│", style, clip);
        }
        put_span(frame, start, y1, "└", style, clip);
    } else {
        put_span(frame, start, y0, "┘", style, clip);
        for y in (y1.saturating_add(1))..y0 {
            put_span(frame, start, y, "│", style, clip);
        }
        put_span(frame, start, y1, "┌", style, clip);
    }
    let rest = start.saturating_add(1);
    if to.x > rest {
        let gap = to.x.saturating_sub(rest);
        let dash = "─".repeat(gap.saturating_sub(1) as usize);
        put_span(frame, rest, y1, &format!("{dash}▶"), style, clip);
    }
}

fn draw_forward_link(frame: &mut Frame, from: Rect, to: Rect, style: Style, clip: Rect) {
    let y = from.y.saturating_add(1);
    let start = from.x.saturating_add(from.width);
    if to.x <= start {
        return;
    }
    let gap = to.x.saturating_sub(start);
    if gap == 0 {
        return;
    }
    let dash = "─".repeat(gap.saturating_sub(1) as usize);
    put_span(frame, start, y, &format!("{dash}▶"), style, clip);
}

fn draw_return_to_mpnn(
    frame: &mut Frame,
    mpnn: Rect,
    gate: Rect,
    rounds: Option<u32>,
    style: Style,
    clip: Rect,
) {
    let mpnn_x = mpnn.x.saturating_add(mpnn.width / 2);
    let gate_x = gate.x.saturating_add(gate.width / 2);
    if gate_x <= mpnn_x {
        return;
    }
    let under = mpnn.y.saturating_add(mpnn.height).max(gate.y.saturating_add(gate.height));
    let bar_y = under.saturating_add(1);
    if under >= clip.y.saturating_add(clip.height) {
        return;
    }
    put_span(frame, mpnn_x, under, "│", style, clip);
    put_span(frame, gate_x, under, "│", style, clip);
    if bar_y < clip.y.saturating_add(clip.height) {
        put_span(frame, mpnn_x, bar_y, "└", style, clip);
        put_span(frame, gate_x, bar_y, "┘", style, clip);
        let mid_w = gate_x.saturating_sub(mpnn_x).saturating_sub(1);
        if mid_w > 0 {
            put_span(
                frame,
                mpnn_x.saturating_add(1),
                bar_y,
                &"─".repeat(mid_w as usize),
                style,
                clip,
            );
        }
        if let Some(rounds) = rounds {
            let caption = rounds_caption(rounds);
            let label_y = bar_y.saturating_add(1);
            if label_y < clip.y.saturating_add(clip.height) {
                let label_w = caption.chars().count() as u16;
                let span = gate_x.saturating_sub(mpnn_x).saturating_add(1);
                let label_x = mpnn_x
                    .saturating_add(span.saturating_sub(label_w) / 2)
                    .max(clip.x);
                put_span(frame, label_x, label_y, &caption, style, clip);
            }
        }
    }
}

pub fn rounds_caption(rounds: u32) -> String {
    format!("×{rounds}")
}

fn put_span(frame: &mut Frame, x: u16, y: u16, text: &str, style: Style, clip: Rect) {
    if x < clip.x || y < clip.y || x >= clip.x.saturating_add(clip.width) || y >= clip.y.saturating_add(clip.height)
    {
        return;
    }
    let width = (text.chars().count() as u16)
        .min(clip.x.saturating_add(clip.width).saturating_sub(x))
        .max(1);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text.to_string(), style))),
        Rect::new(x, y, width, 1),
    );
}

fn light_glyph(light: &str, waiting: bool) -> &'static str {
    if waiting {
        return "◐";
    }
    match light {
        "ok" => "●",
        "fail" => "✕",
        _ => "◐",
    }
}

fn display_block(node: &WorkflowNode) -> String {
    if node.kind == "loop" {
        return node.label.clone();
    }
    if node.label != node.id && !node.label.is_empty() {
        return node.label.clone();
    }
    node.block.clone()
}

fn extra_note(node: &WorkflowNode) -> String {
    if node.waiting {
        return if node.kind == "gate" {
            "human".to_string()
        } else {
            "running".to_string()
        };
    }
    if node.needs_editspec && node.checks.editspec.ok {
        return node.editspec_note.clone();
    }
    if !node.checks.input.ok {
        return node.checks.input.missing.clone();
    }
    if !node.checks.editspec.ok {
        return node.checks.editspec.missing.clone();
    }
    String::new()
}

fn truncate_line(text: &str, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    let max = width as usize;
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::WorkflowNode;

    fn node(id: &str, kind: &str, block: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            kind: kind.to_string(),
            block: block.to_string(),
            label: id.to_string(),
            needs_editspec: false,
            editspec_note: String::new(),
            structure_path: None,
            light: "ok".to_string(),
            waiting: false,
            checks: crate::workflow::WorkflowChecks::default(),
            tree_kinds: Vec::new(),
            condition_node: None,
            rounds: 0,
            inputs: Vec::new(),
        }
    }

    fn demo_nodes() -> Vec<WorkflowNode> {
        vec![
            node("seed", "compose", "import"),
            node("optimize", "loop", "loop"),
            node("optimize.mpnn", "step", "mpnn"),
            node("optimize.fold", "step", "fold"),
            node("optimize.evaluate", "step", "evaluate"),
            node("optimize.gate", "gate", "gate"),
        ]
    }

    #[test]
    fn layout_places_compose_loop_gate_in_columns() {
        let nodes = demo_nodes();
        let boxes = layout_workflow(Rect::new(0, 0, 80, 12), &nodes);
        let seed = boxes.iter().find(|(idx, _)| *idx == 0).unwrap().1;
        let mpnn = boxes.iter().find(|(idx, _)| *idx == 2).unwrap().1;
        let gate = boxes.iter().find(|(idx, _)| *idx == 5).unwrap().1;
        assert!(seed.x < mpnn.x);
        assert!(mpnn.x < gate.x);
    }

    #[test]
    fn rounds_caption_is_times_n() {
        assert_eq!(rounds_caption(2), "×2");
    }

    #[test]
    fn loop_return_prefers_mpnn_not_import() {
        assert_eq!(loop_return_index(&demo_nodes()), Some(2));
        assert_eq!(demo_nodes()[2].block, "mpnn");
        let mut with_rfd = demo_nodes();
        with_rfd.insert(2, node("optimize.rfd", "step", "rfd"));
        assert_eq!(loop_return_index(&with_rfd), Some(3));
        assert_eq!(with_rfd[3].block, "mpnn");
    }

    #[test]
    fn hit_test_maps_box_not_row_index() {
        let nodes = demo_nodes();
        let rect = Rect::new(0, 0, 80, 12);
        let boxes = layout_workflow(rect, &nodes);
        let seed = boxes.iter().find(|(idx, _)| *idx == 0).unwrap().1;
        assert_eq!(hit_test(rect, seed.x + 1, seed.y + 1, &nodes), Some(0));
        assert_eq!(hit_test(rect, 2, 0, &nodes), None);
    }

    #[test]
    fn layout_places_downstream_compose_to_the_right() {
        let mut child = node("graft", "compose", "graft");
        child.inputs = vec!["seed".to_string()];
        let mut nodes = demo_nodes();
        nodes.insert(1, child);
        let boxes = layout_workflow(Rect::new(0, 0, 100, 12), &nodes);
        let seed = boxes.iter().find(|(idx, _)| *idx == 0).unwrap().1;
        let graft = boxes.iter().find(|(idx, _)| *idx == 1).unwrap().1;
        assert!(seed.x < graft.x);
        assert!(graft.x < boxes.iter().find(|(idx, _)| nodes[*idx].block == "mpnn").unwrap().1.x);
    }
}
