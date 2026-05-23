use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, EditField, PREDEFINED_LABELS, VALID_ACTIONS, label_color};
use crate::edit_history::IssueSeverity;

/// Width of the regions sidebar in columns.
pub const SIDEBAR_WIDTH: u16 = 34;

/// Return the display color for a given editspec action string.
fn action_color(action: &str) -> Color {
    match action {
        "keep" => Color::Rgb(0, 200, 80),
        "edit" => Color::Rgb(255, 200, 0),
        "replace" => Color::Rgb(255, 80, 80),
        "insert" => Color::Rgb(80, 150, 255),
        "delete" => Color::Rgb(140, 140, 140),
        _ => Color::White,
    }
}

/// Return a short symbol for the action.
fn action_symbol(action: &str) -> &'static str {
    match action {
        "keep" => "=",
        "edit" => "~",
        "replace" => ">",
        "insert" => "+",
        "delete" => "-",
        _ => "?",
    }
}

/// Number of header lines before the first region item.
/// Layout: title (1) + empty (1) + legend (1) + empty (1) = 4 lines.
pub const REGIONS_HEADER_LINES: u16 = 4;

/// Render a field label with cursor indicator.
fn field_label<'a>(label: &str, active: bool) -> Span<'a> {
    if active {
        Span::styled(
            format!(">{}", label),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(label.to_string(), Style::default().fg(Color::DarkGray))
    }
}

/// Render a field value, highlighted when the cursor is on it.
fn field_value<'a>(value: &str, active: bool) -> Span<'a> {
    if active {
        Span::styled(
            format!("[{}]", value),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(format!("[{}]", value), Style::default().fg(Color::White))
    }
}

/// Render the action field showing all options with current one highlighted.
fn render_action_field(action: &str, active: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    spans.push(field_label("Action:", active));
    spans.push(Span::raw(" "));

    for (i, a) in VALID_ACTIONS.iter().enumerate() {
        if *a == action {
            // Current selection — highlight.
            let sym = action_symbol(a);
            spans.push(Span::styled(
                format!("{}{}", sym, a),
                Style::default()
                    .fg(Color::Black)
                    .bg(action_color(a))
                    .add_modifier(Modifier::BOLD),
            ));
        } else if active {
            // Show other options dimly when field is active.
            spans.push(Span::styled(
                format!(" {}", a),
                Style::default().fg(Color::DarkGray),
            ));
        }
        if i < VALID_ACTIONS.len() - 1 && active {
            spans.push(Span::raw(" "));
        }
    }
    spans
}

/// Render the label field with dropdown preview of predefined labels.
fn render_label_field(label: &str, active: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    spans.push(field_label("Label:", active));
    spans.push(Span::raw(" "));

    if label.is_empty() {
        spans.push(field_value("_", active));
    } else {
        let lcolor = label_color(label);
        if active {
            spans.push(Span::styled(
                format!("[{}]", label),
                Style::default()
                    .fg(Color::Black)
                    .bg(lcolor)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                format!("[{}]", label),
                Style::default().fg(lcolor),
            ));
        }
    }
    spans
}

/// Render predefined labels preview when the label field is active.
fn render_label_preview() -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // First few labels on one line.
    let mut row_spans: Vec<Span> = vec![Span::raw("  ")];
    for (i, label) in PREDEFINED_LABELS.iter().enumerate() {
        if i > 0 && i % 4 == 0 {
            row_spans.push(Span::raw("  "));
            lines.push(Line::from(row_spans));
            row_spans = vec![Span::raw("  ")];
        }
        let color = label_color(label);
        row_spans.push(Span::styled(
            label.to_string(),
            Style::default().fg(color),
        ));
        row_spans.push(Span::raw(" "));
    }
    if row_spans.len() > 1 {
        lines.push(Line::from(row_spans));
    }

    lines
}

/// Render the EditSpec Regions panel as a left-edge sidebar.
pub fn render_regions_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let mut lines: Vec<Line> = Vec::new();

    let is_editing = app.edit_state.editing;

    // Title line with edit mode indicator.
    let title = if is_editing {
        Line::from(vec![
            Span::styled(
                " EditSpec Regions",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "         [EDITING]",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(Span::styled(
            " EditSpec Regions",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
    };
    lines.push(title);
    lines.push(Line::from(""));

    // Legend row (only in view mode).
    if !is_editing {
        lines.push(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled("=", Style::default().fg(action_color("keep"))),
            Span::styled("keep ", Style::default().fg(Color::DarkGray)),
            Span::styled("~", Style::default().fg(action_color("edit"))),
            Span::styled("edit ", Style::default().fg(Color::DarkGray)),
            Span::styled(">", Style::default().fg(action_color("replace"))),
            Span::styled("repl ", Style::default().fg(Color::DarkGray)),
            Span::styled("+", Style::default().fg(action_color("insert"))),
            Span::styled("ins ", Style::default().fg(Color::DarkGray)),
            Span::styled("-", Style::default().fg(action_color("delete"))),
            Span::styled("del", Style::default().fg(Color::DarkGray)),
        ]));
        lines.push(Line::from(""));
    }

    // Delete confirmation message.
    if app.edit_state.delete_confirm {
        lines.push(Line::from(Span::styled(
            " Press 'd' again to confirm delete",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            " or any other key to cancel",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
    }

    match &app.annotation {
        Some(ann) => match &ann.editspec_regions {
            Some(regions) if !regions.is_empty() => {
                for (i, region) in regions.iter().enumerate() {
                    // If editing this region, render the edit form instead.
                    if is_editing && app.edit_state.editing_region_idx == Some(i) {
                        render_edit_form(&mut lines, app);
                        continue;
                    }

                    let is_focused = i == app.focused_region && !is_editing;
                    let color = action_color(&region.action);
                    let sym = action_symbol(&region.action);

                    let range_str = format!("{}-{}", region.range[0], region.range[1]);

                    // Build label display with color.
                    let label_text = region
                        .label
                        .as_deref()
                        .unwrap_or(&region.action);

                    if is_focused {
                        let mut spans = vec![
                            Span::styled(
                                format!(" \u{25b8} {}{} ", sym, region.chain),
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!("{} ", range_str),
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(color),
                            ),
                        ];
                        // Label with color.
                        let lc = label_color(label_text);
                        spans.push(Span::styled(
                            label_text.to_string(),
                            Style::default()
                                .fg(Color::Black)
                                .bg(lc)
                                .add_modifier(Modifier::BOLD),
                        ));
                        lines.push(Line::from(spans));
                    } else {
                        let mut spans = vec![
                            Span::styled(
                                format!("   {}{} ", sym, region.chain),
                                Style::default().fg(color),
                            ),
                            Span::styled(
                                format!("{} ", range_str),
                                Style::default().fg(Color::White),
                            ),
                        ];
                        let lc = label_color(label_text);
                        if region.label.is_some() {
                            spans.push(Span::styled(
                                label_text.to_string(),
                                Style::default()
                                    .fg(lc)
                                    .add_modifier(Modifier::BOLD),
                            ));
                        } else {
                            spans.push(Span::styled(
                                label_text.to_string(),
                                Style::default().fg(Color::DarkGray),
                            ));
                        }
                        lines.push(Line::from(spans));
                    }
                }

                // If adding new region, render the edit form at the end.
                if is_editing && app.edit_state.editing_region_idx.is_none() {
                    render_edit_form(&mut lines, app);
                }

                // Scroll hint (only in view mode).
                if !is_editing && regions.len() as u16 > area.height.saturating_sub(6) {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        " Scroll: mouse wheel",
                        Style::default().fg(Color::DarkGray),
                    )));
                }

                // Edit mode keybinding hint.
                if is_editing {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        " Tab:next +/-:val Enter:save Esc:x",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            _ => {
                lines.push(Line::from(Span::styled(
                    " No region data in annotation",
                    Style::default().fg(Color::DarkGray),
                )));
                // Still show add form if editing.
                if is_editing && app.edit_state.editing_region_idx.is_none() {
                    render_edit_form(&mut lines, app);
                }
            }
        },
        None => {
            lines.push(Line::from(Span::styled(
                " No annotation loaded",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Use --annotation <file>",
                Style::default().fg(Color::DarkGray),
            )));
            // Still show add form if editing (creates annotation).
            if is_editing && app.edit_state.editing_region_idx.is_none() {
                render_edit_form(&mut lines, app);
            }
        }
    }

    // Validation issues (only in view mode, after region list).
    if !is_editing && !app.validation_issues.is_empty() {
        lines.push(Line::from(""));
        for issue in &app.validation_issues {
            let color = match issue.severity {
                IssueSeverity::Error => Color::Red,
                IssueSeverity::Warning => Color::Yellow,
            };
            let icon = match issue.severity {
                IssueSeverity::Error => "!",
                IssueSeverity::Warning => "*",
            };
            // Truncate message to fit sidebar width.
            let msg = if issue.message.len() > 30 {
                format!("{} ...", &issue.message[..27])
            } else {
                issue.message.clone()
            };
            lines.push(Line::from(Span::styled(
                format!(" {} {}", icon, msg),
                Style::default().fg(color),
            )));
        }
    }

    // No bridge warning in edit mode.
    if is_editing && !app.python_available {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " No Python bridge (local only)",
            Style::default().fg(Color::Yellow),
        )));
    }

    // Store item metadata for mouse click mapping.
    let item_count = match &app.annotation {
        Some(ann) => ann
            .editspec_regions
            .as_ref()
            .map(|r| r.len())
            .unwrap_or(0),
        None => 0,
    };
    app.panel_click_header = REGIONS_HEADER_LINES;
    app.panel_item_count = item_count;

    let issue_count = app.validation_issues.len();
    let border_title = if is_editing {
        " EDIT ".to_string()
    } else if issue_count > 0 {
        format!(" Regions ({} issues) ", issue_count)
    } else {
        " Regions ".to_string()
    };
    let border_color = if is_editing {
        Color::Yellow
    } else if issue_count > 0 {
        let has_error = app.validation_issues.iter().any(|i| i.severity == IssueSeverity::Error);
        if has_error { Color::Red } else { Color::Yellow }
    } else {
        Color::Cyan
    };

    let panel = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(border_title)
                .title_style(Style::default().fg(border_color)),
        )
        .scroll((app.panel_scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(panel, area);
}

/// Render the inline edit form for a region.
fn render_edit_form(lines: &mut Vec<Line<'static>>, app: &App) {
    let es = &app.edit_state;
    let cursor = es.cursor_field;

    // Separator line.
    lines.push(Line::from(Span::styled(
        " \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        Style::default().fg(Color::DarkGray),
    )));

    // Line 1: Chain + RangeStart + RangeEnd
    let chain_active = cursor == EditField::Chain;
    let rs_active = cursor == EditField::RangeStart;
    let re_active = cursor == EditField::RangeEnd;

    lines.push(Line::from(vec![
        field_label("Chain:", chain_active),
        Span::raw(" "),
        field_value(&es.draft_chain, chain_active),
        Span::raw(" "),
        field_label("Range:", rs_active || re_active),
        Span::raw(" "),
        field_value(&format!("{}", es.draft_range_start), rs_active),
        Span::styled("-", Style::default().fg(Color::White)),
        field_value(&format!("{}", es.draft_range_end), re_active),
    ]));

    // Line 2: Action
    let action_active = cursor == EditField::Action;
    lines.push(Line::from(render_action_field(&es.draft_action, action_active)));

    // Line 3: Label
    let label_active = cursor == EditField::Label;
    lines.push(Line::from(render_label_field(&es.draft_label, label_active)));

    // Label dropdown preview when label field is active.
    if label_active {
        for preview_line in render_label_preview() {
            lines.push(preview_line);
        }
    }

    // Validation error.
    if let Some(ref err) = es.validation_error {
        lines.push(Line::from(Span::styled(
            format!(" ! {}", err),
            Style::default().fg(Color::Red),
        )));
    }
}
