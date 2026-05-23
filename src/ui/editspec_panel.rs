use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, EditField, PREDEFINED_LABELS, VALID_ACTIONS, label_color};
use crate::edit_history::IssueSeverity;
use crate::model::protein::{MoleculeType, SecondaryStructure};

/// Width of the EditSpec sidebar in columns (horizontal layout).
/// Width of the EditSpec sidebar in columns (horizontal layout).
#[allow(dead_code)]
pub const SIDEBAR_WIDTH: u16 = 60;

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

/// Convert a 3-letter amino acid code to a 1-letter code.
pub fn aa_one_letter(three: &str) -> char {
    match three {
        "ALA" => 'A',
        "ARG" => 'R',
        "ASN" => 'N',
        "ASP" => 'D',
        "CYS" => 'C',
        "GLN" => 'Q',
        "GLU" => 'E',
        "GLY" => 'G',
        "HIS" => 'H',
        "ILE" => 'I',
        "LEU" => 'L',
        "LYS" => 'K',
        "MET" => 'M',
        "PHE" => 'F',
        "PRO" => 'P',
        "SER" => 'S',
        "THR" => 'T',
        "TRP" => 'W',
        "TYR" => 'Y',
        "VAL" => 'V',
        "SEC" => 'U',
        "PYL" => 'O',
        "ASX" => 'B',
        "GLX" => 'Z',
        "XLE" => 'J',
        "XAA" => 'X',
        _ => '?',
    }
}

/// Return the secondary structure display character.
fn ss_char(ss: SecondaryStructure) -> char {
    match ss {
        SecondaryStructure::Helix => 'H',
        SecondaryStructure::Sheet => 'E',
        SecondaryStructure::Turn => 'T',
        SecondaryStructure::Coil => '-',
    }
}

/// Yank text to clipboard using OSC 52 escape sequence.
pub fn yank_to_clipboard(text: &str) {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let seq = format!("\x1b]52;c;{}\x07", encoded);
    let _ = std::io::Write::write_all(&mut std::io::stderr(), seq.as_bytes());
}

/// Build a map of seq_num -> action for the given chain.
fn build_action_set(
    annotation: &Option<crate::app::Annotation>,
    chain_id: &str,
) -> std::collections::HashMap<i32, String> {
    let mut map = std::collections::HashMap::new();
    if let Some(ann) = annotation {
        if let Some(ref regions) = ann.editspec_regions {
            for region in regions {
                if region.chain == chain_id {
                    let start = region.range[0] as i32;
                    let end = region.range[1] as i32;
                    for seq_num in start..=end {
                        map.insert(seq_num, region.action.clone());
                    }
                }
            }
        }
    }
    map
}

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
            let sym = action_symbol(a);
            spans.push(Span::styled(
                format!("{}{}", sym, a),
                Style::default()
                    .fg(Color::Black)
                    .bg(action_color(a))
                    .add_modifier(Modifier::BOLD),
            ));
        } else if active {
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

/// Render the inline edit form for a region.
fn render_edit_form(lines: &mut Vec<Line<'static>>, app: &App) {
    let es = &app.edit_state;
    let cursor = es.cursor_field;

    // Separator line.
    lines.push(Line::from(Span::styled(
        " \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
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

    // Keybinding hint.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Tab:next +/-:val Enter:save Esc:x",
        Style::default().fg(Color::DarkGray),
    )));
}

/// Render the unified EditSpec panel (Chain Info + Regions + Sequence).
pub fn render_editspec_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let mut lines: Vec<Line> = Vec::new();
    let is_editing = app.edit_state.editing;

    // ========================================================================
    // Section 1: Chain Info (compact, ~2-3 lines)
    // ========================================================================
    if let Some(chain) = app.protein.chains.get(app.current_chain) {
        let res_count = chain.residues.len();
        let atom_count: usize = chain.residues.iter().map(|r| r.atoms.len()).sum();

        // Chain info on a single line
        let mut chain_spans = vec![
            Span::styled(" Chain ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("[{}]", chain.id),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {} res  {} atoms", res_count, atom_count),
                Style::default().fg(Color::White),
            ),
        ];

        // Ligand info
        if !app.protein.ligands.is_empty() {
            let lig_count = app.protein.ligands.len();
            let lig_names: Vec<&str> = app.protein.ligands.iter().take(3).map(|l| l.name.as_str()).collect();
            let names_str = lig_names.join(", ");
            let suffix = if lig_count > 3 {
                format!(", ...")
            } else {
                String::new()
            };
            chain_spans.push(Span::styled(
                format!("  Ligands: {} ({}{})", lig_count, names_str, suffix),
                Style::default().fg(Color::Rgb(255, 0, 255)),
            ));
        }

        lines.push(Line::from(chain_spans));
    } else {
        lines.push(Line::from(Span::styled(
            " No chains loaded",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::from(""));

    // ========================================================================
    // Section 2: EditSpec Regions (scrollable)
    // ========================================================================
    // Header with separator
    let edit_indicator = if is_editing {
        "  [EDITING]"
    } else {
        ""
    };
    lines.push(Line::from(vec![
        Span::styled(
            " \u{2500}\u{2500} Regions",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{}\u{2500}", edit_indicator),
            Style::default()
                .fg(if is_editing { Color::Yellow } else { Color::DarkGray })
                .add_modifier(if is_editing { Modifier::BOLD } else { Modifier::empty() }),
        ),
    ]));

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
    }

    // Delete confirmation message.
    if app.edit_state.delete_confirm {
        lines.push(Line::from(Span::styled(
            " Press 'd' again to confirm delete",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )));
    }

    // Region list or edit form
    let region_header_lines = lines.len() as u16;

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
            }
            _ => {
                // No regions defined — show helpful hint.
                lines.push(Line::from(Span::styled(
                    " No regions defined",
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(Span::styled(
                    " Press 'a' to add a region",
                    Style::default().fg(Color::DarkGray),
                )));
                if is_editing && app.edit_state.editing_region_idx.is_none() {
                    render_edit_form(&mut lines, app);
                }
            }
        },
        None => {
            // No annotation loaded — user can still add regions from scratch.
            lines.push(Line::from(Span::styled(
                " No regions defined",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                " Press 'a' to add a region",
                Style::default().fg(Color::DarkGray),
            )));
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
            // Truncate message to fit panel width.
            let msg = if issue.message.len() > 55 {
                format!("{} ...", &issue.message[..52])
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
        lines.push(Line::from(Span::styled(
            " No Python bridge (local only)",
            Style::default().fg(Color::Yellow),
        )));
    }

    // ========================================================================
    // Section 3: Sequence (compact, ~3-4 lines)
    // ========================================================================
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " \u{2500}\u{2500} Sequence\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    // Sequence section
    let chain = app.protein.chains.get(app.current_chain);
    let _seq_header_lines = lines.len() as u16;

    if let Some(c) = chain {
        if c.molecule_type == MoleculeType::Protein {
            let chain_id = &c.id;
            let residues = &c.residues;
            let res_count = residues.len();

            if res_count > 0 {
                let action_map = build_action_set(&app.annotation, chain_id);

                // Compute available content width
                let content_width = area.width.saturating_sub(2) as usize;
                if content_width > 0 {
                    let residues_per_line = content_width;

                    // Apply horizontal scroll offset
                    let h_scroll = app.seq_h_scroll as usize;
                    let start_res = h_scroll;
                    let end_res = (start_res + residues_per_line).min(res_count);

                    // Track the sequence line row for mouse hit-testing
                    app.seq_line_row = (lines.len() as u16).saturating_sub(app.panel_scroll);

                    if start_res < res_count {
                        // Get selection range for highlighting
                        let sel_range = app.seq_selection.range();
                        let cursor_idx = app.seq_selection.cursor;

                        // Sequence line with action coloring + selection/cursor highlighting
                        let mut seq_spans: Vec<Span> = Vec::new();
                        for (i, residue) in residues.iter().enumerate() {
                            if i < start_res || i >= end_res {
                                continue;
                            }
                            let letter = aa_one_letter(&residue.name);

                            // Determine action color
                            let action = action_map.get(&residue.seq_num);
                            let base_fg = match action.map(|s| s.as_str()).unwrap_or("") {
                                "keep" => Color::Rgb(0, 200, 80),
                                "edit" => Color::Rgb(255, 200, 0),
                                "replace" => Color::Rgb(255, 80, 80),
                                "insert" => Color::Rgb(80, 150, 255),
                                "delete" => Color::Rgb(140, 140, 140),
                                _ => Color::White,
                            };

                            // Check if this residue is selected
                            let is_selected = sel_range
                                .map(|(s, e)| i >= s && i <= e)
                                .unwrap_or(false);
                            let is_cursor = i == cursor_idx;

                            if is_selected {
                                // Selected: reverse video (colored background)
                                seq_spans.push(Span::styled(
                                    letter.to_string(),
                                    Style::default()
                                        .fg(Color::Black)
                                        .bg(base_fg)
                                        .add_modifier(Modifier::BOLD),
                                ));
                            } else if is_cursor {
                                // Cursor only (not selected): underline
                                seq_spans.push(Span::styled(
                                    letter.to_string(),
                                    Style::default()
                                        .fg(base_fg)
                                        .add_modifier(Modifier::UNDERLINED),
                                ));
                            } else {
                                seq_spans.push(Span::styled(
                                    letter.to_string(),
                                    Style::default().fg(base_fg),
                                ));
                            }
                        }
                        if !seq_spans.is_empty() {
                            lines.push(Line::from(seq_spans));
                        }

                        // Secondary structure line
                        let mut ss_spans: Vec<Span> = Vec::new();
                        for (i, residue) in residues.iter().enumerate() {
                            if i < start_res || i >= end_res {
                                continue;
                            }
                            let ch = ss_char(residue.secondary_structure);
                            let color = match residue.secondary_structure {
                                SecondaryStructure::Helix => Color::Rgb(255, 100, 100),
                                SecondaryStructure::Sheet => Color::Rgb(100, 180, 255),
                                SecondaryStructure::Turn => Color::Rgb(200, 150, 255),
                                SecondaryStructure::Coil => Color::Rgb(80, 80, 80),
                            };
                            ss_spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
                        }
                        if !ss_spans.is_empty() {
                            lines.push(Line::from(ss_spans));
                        }

                        // Action markers line
                        let mut act_spans: Vec<Span> = Vec::new();
                        for (i, residue) in residues.iter().enumerate() {
                            if i < start_res || i >= end_res {
                                continue;
                            }
                            let action = action_map.get(&residue.seq_num);
                            match action {
                                Some(a) => {
                                    let sym = action_symbol(a);
                                    let color = action_color(a);
                                    act_spans.push(Span::styled(
                                        sym.to_string(),
                                        Style::default().fg(color),
                                    ));
                                }
                                None => {
                                    act_spans.push(Span::raw(" "));
                                }
                            }
                        }
                        if !act_spans.is_empty() {
                            lines.push(Line::from(act_spans));
                        }

                        // Selection info and scroll hint
                        let sel_info = if let Some((s, e)) = sel_range {
                            let seq_start = residues[s].seq_num;
                            let seq_end = residues[e].seq_num;
                            format!(" sel:{}:{}-{} ", chain_id, seq_start, seq_end)
                        } else {
                            String::new()
                        };

                        if res_count > residues_per_line {
                            lines.push(Line::from(vec![
                                Span::styled(
                                    sel_info,
                                    Style::default()
                                        .fg(Color::Cyan)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(
                                    format!(
                                        " [{}/{}] h/l:nav H/L:sel 1-5:act y/Y:copy",
                                        start_res + 1,
                                        res_count
                                    ),
                                    Style::default().fg(Color::DarkGray),
                                ),
                            ]));
                        } else if !sel_info.is_empty() {
                            lines.push(Line::from(vec![
                                Span::styled(
                                    sel_info,
                                    Style::default()
                                        .fg(Color::Cyan)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(
                                    " h/l:nav H/L:sel 1-5:act y/Y:copy",
                                    Style::default().fg(Color::DarkGray),
                                ),
                            ]));
                        }
                    } else {
                        app.seq_line_row = 0;
                        lines.push(Line::from(Span::styled(
                            " Scroll back with h",
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                }
            } else {
                app.seq_line_row = 0;
                lines.push(Line::from(Span::styled(
                    " No residues",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        } else {
            app.seq_line_row = 0;
            lines.push(Line::from(Span::styled(
                " Not a protein chain",
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        app.seq_line_row = 0;
        lines.push(Line::from(Span::styled(
            " No chain selected",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // ========================================================================
    // Section 4: Status bar (1 line)
    // ========================================================================
    lines.push(Line::from(""));
    {
        let issue_count = app.validation_issues.len();
        let python_status = if app.python_available {
            ("Python ok", Color::Green)
        } else {
            ("No-Python", Color::Rgb(255, 165, 0))
        };
        let mut status_spans = vec![];
        if issue_count > 0 {
            status_spans.push(Span::styled(
                format!(" {} issues", issue_count),
                Style::default()
                    .fg(if app.validation_issues.iter().any(|i| i.severity == IssueSeverity::Error) {
                        Color::Red
                    } else {
                        Color::Yellow
                    }),
            ));
            status_spans.push(Span::styled(
                " \u{2502} ",
                Style::default().fg(Color::DarkGray),
            ));
        }
        status_spans.push(Span::styled(
            " u:undo Ctrl+r:redo",
            Style::default().fg(Color::DarkGray),
        ));
        status_spans.push(Span::styled(
            " \u{2502} ",
            Style::default().fg(Color::DarkGray),
        ));
        status_spans.push(Span::styled(
            python_status.0.to_string(),
            Style::default().fg(python_status.1),
        ));
        lines.push(Line::from(status_spans));
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
    app.panel_click_header = region_header_lines;
    app.panel_item_count = item_count;

    let issue_count = app.validation_issues.len();
    let border_title = if is_editing {
        " EDIT ".to_string()
    } else if issue_count > 0 {
        format!(" EditSpec ({} issues) ", issue_count)
    } else {
        " EditSpec ".to_string()
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
