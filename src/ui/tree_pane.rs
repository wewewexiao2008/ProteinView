//! Product tree pane. Rows come from gemlib `state.product_tree`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::product_tree::ProductTreeNode;
use crate::shell::PaneId;
use crate::ui::chrome::pane_block;

const FOLD_COLS: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeHit {
    Fold,
    Label,
}

#[derive(Debug, Clone, Copy)]
pub struct TreeRowHit {
    pub depth: usize,
    pub has_children: bool,
}

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
        ]);
        frame.render_widget(body, inner);
        return;
    }

    let rows = app.product_tree.visible_rows();
    let scroll = app.tree_scroll as usize;
    let view_h = inner.height as usize;
    let mut lines = Vec::new();
    for (idx, (depth, node)) in rows.iter().enumerate().skip(scroll).take(view_h) {
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
        let linked = app
            .focused_workflow_node()
            .map(|wf| {
                crate::workflow::tree_row_matches_node(
                    &node.kind,
                    node.condition_node(),
                    wf,
                )
            })
            .unwrap_or(true);
        let mut style = Style::default().fg(if linked {
            Color::Gray
        } else {
            Color::DarkGray
        });
        if selected {
            style = style.fg(Color::Cyan);
        }
        if cursor {
            style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
        }
        let text = truncate_line(
            &format!("{indent}{marker}{}", compact_label(node)),
            inner.width,
        );
        lines.push(Line::from(Span::styled(text, style)));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

pub fn hit_test(
    outer: Rect,
    col: u16,
    row: u16,
    scroll: u16,
    rows: &[TreeRowHit],
) -> Option<(usize, TreeHit)> {
    if outer.width < 3 || outer.height < 3 {
        return None;
    }
    let inner = Rect {
        x: outer.x.saturating_add(1),
        y: outer.y.saturating_add(1),
        width: outer.width.saturating_sub(2),
        height: outer.height.saturating_sub(2),
    };
    if col < inner.x || col >= inner.x.saturating_add(inner.width) {
        return None;
    }
    if row < inner.y || row >= inner.y.saturating_add(inner.height) {
        return None;
    }
    let idx = (row - inner.y) as usize + scroll as usize;
    let meta = rows.get(idx)?;
    let rel = col - inner.x;
    let fold_start = (meta.depth.saturating_mul(2)) as u16;
    let fold_end = fold_start.saturating_add(FOLD_COLS);
    if meta.has_children && rel >= fold_start && rel < fold_end {
        Some((idx, TreeHit::Fold))
    } else {
        Some((idx, TreeHit::Label))
    }
}

fn compact_label(node: &ProductTreeNode) -> String {
    let short = short_id(&node.sample_id);
    let score = primary_score(&node.metrics);
    if score.is_empty() {
        format!("{} {short}", kind_icon(&node.kind))
    } else {
        format!("{} {short}  {score}", kind_icon(&node.kind))
    }
}

fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "input" => "•",
        "backbone" => "◇",
        "sequence" => "≡",
        "prediction" => "◈",
        "ensemble" => "⊕",
        "evaluation" => "★",
        "selection" => "✓",
        _ => "·",
    }
}

fn short_id(sample_id: &str) -> &str {
    let tail = sample_id.split_once(':').map(|(_, rest)| rest).unwrap_or(sample_id);
    if tail.len() > 4 { &tail[..4] } else { tail }
}

fn primary_score(value: &serde_json::Value) -> String {
    let Some(map) = value.as_object() else {
        return String::new();
    };
    for key in ["plddt", "ptm", "mpnn_perplexity"] {
        if let Some(v) = map.get(key).filter(|v| !v.is_null()) {
            return format_metric(v);
        }
    }
    map.iter()
        .find(|(_, v)| !v.is_null())
        .map(|(_, v)| format_metric(v))
        .unwrap_or_default()
}

fn format_metric(value: &serde_json::Value) -> String {
    if let Some(n) = value.as_f64() {
        if n.fract() == 0.0 {
            format!("{n:.0}")
        } else {
            format!("{n}")
        }
    } else if let Some(s) = value.as_str() {
        s.to_string()
    } else {
        value.to_string()
    }
}

fn truncate_line(text: &str, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    let max = width as usize;
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    text.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::new(0, 0, 24, 10)
    }

    fn node(kind: &str, sample_id: &str, metrics: serde_json::Value) -> ProductTreeNode {
        ProductTreeNode {
            sample_id: sample_id.to_string(),
            kind: kind.to_string(),
            parent_ids: Vec::new(),
            metrics,
            condition: serde_json::Value::Null,
            label: String::new(),
            structure_path: None,
            expanded: true,
            children: Vec::new(),
        }
    }

    #[test]
    fn compact_label_uses_seq_and_fold_icons() {
        let seq = node(
            "sequence",
            "sequence:55c0cf5ea7eba53d",
            serde_json::json!({"mpnn_perplexity": 1.21}),
        );
        let fold = node(
            "prediction",
            "prediction:2dfb63219c77e391",
            serde_json::json!({"plddt": 91.0, "ptm": 0.82}),
        );
        assert_eq!(compact_label(&seq), "≡ 55c0  1.21");
        assert_eq!(compact_label(&fold), "◈ 2dfb  91");
        assert_eq!(kind_icon("sequence"), "≡");
        assert_eq!(kind_icon("prediction"), "◈");
    }

    #[test]
    fn fold_hit_is_marker_of_that_row() {
        let rows = [TreeRowHit {
            depth: 0,
            has_children: true,
        }];
        assert_eq!(hit_test(rect(), 1, 1, 0, &rows), Some((0, TreeHit::Fold)));
        assert_eq!(hit_test(rect(), 2, 1, 0, &rows), Some((0, TreeHit::Fold)));
        assert_eq!(hit_test(rect(), 4, 1, 0, &rows), Some((0, TreeHit::Label)));
    }

    #[test]
    fn leaf_marker_is_label() {
        let rows = [TreeRowHit {
            depth: 1,
            has_children: false,
        }];
        assert_eq!(hit_test(rect(), 3, 1, 0, &rows), Some((0, TreeHit::Label)));
        assert_eq!(hit_test(rect(), 6, 1, 0, &rows), Some((0, TreeHit::Label)));
    }

    #[test]
    fn child_fold_is_indented() {
        let rows = [
            TreeRowHit {
                depth: 0,
                has_children: true,
            },
            TreeRowHit {
                depth: 1,
                has_children: true,
            },
        ];
        assert_eq!(hit_test(rect(), 3, 2, 0, &rows), Some((1, TreeHit::Fold)));
        assert_eq!(hit_test(rect(), 1, 2, 0, &rows), Some((1, TreeHit::Label)));
    }

    #[test]
    fn scroll_maps_visible_row() {
        let rows = [
            TreeRowHit {
                depth: 0,
                has_children: true,
            },
            TreeRowHit {
                depth: 1,
                has_children: false,
            },
        ];
        assert_eq!(
            hit_test(rect(), 6, 1, 1, &rows),
            Some((1, TreeHit::Label))
        );
    }

    #[test]
    fn border_and_empty_are_misses() {
        let rows = [TreeRowHit {
            depth: 0,
            has_children: true,
        }];
        assert_eq!(hit_test(rect(), 4, 0, 0, &rows), None);
        assert_eq!(hit_test(rect(), 4, 3, 0, &rows), None);
    }
}
