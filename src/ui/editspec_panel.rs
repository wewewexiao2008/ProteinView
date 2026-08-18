use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::app::{
    App, EditField, PREDEFINED_LABELS, SeqChainBlock, VALID_ACTIONS, label_color,
};
use crate::shell::PaneId;
use crate::ui::chrome::pane_block;
use crate::edit_history::IssueSeverity;
use crate::model::protein::SecondaryStructure;

/// Width of the EditSpec sidebar in columns (horizontal layout).
/// Width of the EditSpec sidebar in columns (horizontal layout).
#[allow(dead_code)]
pub const SIDEBAR_WIDTH: u16 = 60;

/// No letter/SS row this frame. 0 is a valid content index, so it is not absent.
pub const ABSENT_SEQ_ROW: u16 = u16::MAX;

/// Inner content of a bordered pane: last_sidebar_rect is outer chrome.
pub fn sidebar_inner(outer: Rect) -> Rect {
    Rect {
        x: outer.x.saturating_add(1),
        y: outer.y.saturating_add(1),
        width: outer.width.saturating_sub(2),
        height: outer.height.saturating_sub(2),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqHit {
    Letter(usize),
    SecondaryStructure(usize),
    ActionMarker(usize),
}

/// Residues per group before a spacer.
pub const SEQ_GROUP: usize = 5;
pub const SEQ_LINE_NARROW: usize = 25;
pub const SEQ_LINE_WIDE: usize = 50;
pub const SEQ_PREFIX_COLS: usize = 5;
pub const SEQ_BLOCK_ROWS: u16 = 3;

pub fn residues_per_line(inner_width: u16) -> usize {
    let w = inner_width as usize;
    let need = |n: usize| SEQ_PREFIX_COLS + n + n.saturating_sub(1) / SEQ_GROUP;
    if w >= need(SEQ_LINE_WIDE) {
        SEQ_LINE_WIDE
    } else {
        SEQ_LINE_NARROW
    }
}

/// Column of residue `offset` on a wrap line, including prefix and every-5 gaps.
pub fn display_col_for_offset(offset: usize) -> usize {
    SEQ_PREFIX_COLS + offset + offset / SEQ_GROUP
}

/// Map a content column to a residue offset on that wrap line.
pub fn offset_at_display_col(col: usize, per_line: usize) -> Option<usize> {
    if per_line == 0 {
        return None;
    }
    if col < SEQ_PREFIX_COLS {
        return Some(0);
    }
    let mut c = SEQ_PREFIX_COLS;
    for o in 0..per_line {
        if o > 0 && o % SEQ_GROUP == 0 {
            if col == c {
                return Some(o);
            }
            c += 1;
        }
        if col == c {
            return Some(o);
        }
        c += 1;
    }
    None
}

/// Visual rows a paragraph line occupies at `width` (ratatui wrap).
pub fn line_visual_rows(line: &Line<'_>, width: u16) -> u16 {
    if width == 0 {
        return 1;
    }
    let chars = line.width() as u16;
    if chars == 0 {
        return 1;
    }
    chars.div_ceil(width)
}

pub fn visual_rows_before(lines: &[Line<'_>], width: u16) -> u16 {
    lines.iter().map(|line| line_visual_rows(line, width)).sum()
}

/// Hit-test wrapped sequence blocks. `seq_start_row` is the first letter row.
pub fn hit_test_seq(
    outer: Rect,
    col: u16,
    row: u16,
    seq_start_row: u16,
    per_line: usize,
    wrap_lines: usize,
    panel_scroll: u16,
    residue_count: usize,
) -> Option<SeqHit> {
    let inner = sidebar_inner(outer);
    if col < inner.x
        || col >= inner.x.saturating_add(inner.width)
        || row < inner.y
        || row >= inner.y.saturating_add(inner.height)
    {
        return None;
    }
    if seq_start_row == ABSENT_SEQ_ROW || per_line == 0 || wrap_lines == 0 {
        return None;
    }
    let content_row = row.saturating_sub(inner.y).saturating_add(panel_scroll);
    if content_row < seq_start_row {
        return None;
    }
    let rel = content_row.saturating_sub(seq_start_row);
    let line = (rel / SEQ_BLOCK_ROWS) as usize;
    let row_in = rel % SEQ_BLOCK_ROWS;
    if line >= wrap_lines {
        return None;
    }
    let col_in = col.saturating_sub(inner.x) as usize;
    let offset = offset_at_display_col(col_in, per_line)?;
    let idx = line.saturating_mul(per_line).saturating_add(offset);
    if idx >= residue_count {
        return None;
    }
    match row_in {
        0 => Some(SeqHit::Letter(idx)),
        1 => Some(SeqHit::SecondaryStructure(idx)),
        _ => Some(SeqHit::ActionMarker(idx)),
    }
}

/// Hit-test every chain block. Last inner column is the scrollbar and is ignored.
pub fn hit_test_sequences(
    outer: Rect,
    col: u16,
    row: u16,
    blocks: &[SeqChainBlock],
    panel_scroll: u16,
) -> Option<(usize, SeqHit)> {
    let inner = sidebar_inner(outer);
    if inner.width > 0 && col + 1 >= inner.x.saturating_add(inner.width) {
        return None;
    }
    for block in blocks {
        if let Some(hit) = hit_test_seq(
            outer,
            col,
            row,
            block.start_row,
            block.per_line,
            block.wrap_lines,
            panel_scroll,
            block.residue_count,
        ) {
            return Some((block.chain_idx, hit));
        }
    }
    None
}

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

fn letter_style(base_fg: Color, selected: bool, cursor: bool) -> Style {
    if selected {
        Style::default()
            .fg(Color::Black)
            .bg(base_fg)
            .add_modifier(Modifier::BOLD)
    } else if cursor {
        Style::default()
            .fg(base_fg)
            .add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default().fg(base_fg)
    }
}

fn push_wrapped_sequence(
    lines: &mut Vec<Line<'_>>,
    app: &App,
    residues: &[crate::model::protein::Residue],
    chain_id: &str,
    inner_width: u16,
    highlight: bool,
) -> SeqChainBlock {
    let per = residues_per_line(inner_width);
    let wrap_lines = residues.len().div_ceil(per);
    let start_row = lines.len() as u16;

    let action_map = build_action_set(&app.annotation, chain_id);
    let sel_range = if highlight {
        app.seq_selection.range()
    } else {
        None
    };
    let cursor_idx = if highlight {
        Some(app.seq_selection.cursor)
    } else {
        None
    };

    for line_i in 0..wrap_lines {
        let start = line_i * per;
        let end = (start + per).min(residues.len());
        let prefix = format!("{:>4} ", residues[start].seq_num);

        let mut seq_spans = vec![Span::styled(
            prefix.clone(),
            Style::default().fg(Color::DarkGray),
        )];
        let mut ss_spans = vec![Span::raw(" ".repeat(SEQ_PREFIX_COLS))];
        let mut act_spans = vec![Span::raw(" ".repeat(SEQ_PREFIX_COLS))];

        for (j, residue) in residues[start..end].iter().enumerate() {
            if j > 0 && j % SEQ_GROUP == 0 {
                seq_spans.push(Span::raw(" "));
                ss_spans.push(Span::raw(" "));
                act_spans.push(Span::raw(" "));
            }
            let i = start + j;
            let action = action_map.get(&residue.seq_num);
            let base_fg = match action.map(|s| s.as_str()).unwrap_or("") {
                "keep" => Color::Rgb(0, 200, 80),
                "edit" => Color::Rgb(255, 200, 0),
                "replace" => Color::Rgb(255, 80, 80),
                "insert" => Color::Rgb(80, 150, 255),
                "delete" => Color::Rgb(140, 140, 140),
                _ => Color::White,
            };
            let selected = sel_range.map(|(s, e)| i >= s && i <= e).unwrap_or(false);
            seq_spans.push(Span::styled(
                aa_one_letter(&residue.name).to_string(),
                letter_style(base_fg, selected, cursor_idx == Some(i)),
            ));

            let ss_color = match residue.secondary_structure {
                SecondaryStructure::Helix => Color::Rgb(255, 100, 100),
                SecondaryStructure::Sheet => Color::Rgb(100, 180, 255),
                SecondaryStructure::Turn => Color::Rgb(200, 150, 255),
                SecondaryStructure::Coil => Color::Rgb(80, 80, 80),
            };
            ss_spans.push(Span::styled(
                ss_char(residue.secondary_structure).to_string(),
                Style::default().fg(ss_color),
            ));

            match action {
                Some(a) => act_spans.push(Span::styled(
                    action_symbol(a).to_string(),
                    Style::default().fg(action_color(a)),
                )),
                None => act_spans.push(Span::raw(" ")),
            }
        }
        lines.push(Line::from(seq_spans));
        lines.push(Line::from(ss_spans));
        lines.push(Line::from(act_spans));
    }

    SeqChainBlock {
        chain_idx: 0,
        start_row,
        per_line: per,
        wrap_lines,
        residue_count: residues.len(),
    }
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

    // Line 1: typed range (A51-80 / A:51-80 / 51-80)
    let range_active = cursor == EditField::RangeText;
    lines.push(Line::from(vec![
        field_label("Range:", range_active),
        Span::raw(" "),
        field_value(
            if es.draft_range_text.is_empty() {
                "_"
            } else {
                &es.draft_range_text
            },
            range_active,
        ),
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
    if app.protein.chains.is_empty() {
        lines.push(Line::from(Span::styled(
            " No chains loaded",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let mut chain_spans = vec![Span::styled(
            " Chains ",
            Style::default().fg(Color::DarkGray),
        )];
        for (i, chain) in app.protein.chains.iter().enumerate() {
            let label = format!("[{} {}]", chain.id, chain.residues.len());
            if i == app.current_chain {
                chain_spans.push(Span::styled(
                    label,
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                chain_spans.push(Span::styled(
                    label,
                    Style::default().fg(Color::Green),
                ));
            }
            chain_spans.push(Span::raw(" "));
        }

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
    // Section 3: Sequence (all chains, wrapped 25/50)
    // ========================================================================
    let inner_width = area.width.saturating_sub(3).max(1);
    lines.push(Line::from(""));
    {
        let per = residues_per_line(inner_width);
        let title: String = format!(" \u{2500}\u{2500} Sequence {per}/line")
            .chars()
            .chain(std::iter::repeat('\u{2500}'))
            .take(inner_width as usize)
            .collect();
        lines.push(Line::from(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
    }

    app.seq_blocks.clear();
    app.seq_wrap_lines = 0;
    let chain_snapshot: Vec<(usize, String, Vec<crate::model::protein::Residue>)> = app
        .protein
        .chains
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.residues.is_empty())
        .map(|(i, c)| (i, c.id.clone(), c.residues.clone()))
        .collect();
    if chain_snapshot.is_empty() {
        app.seq_line_row = ABSENT_SEQ_ROW;
        app.ss_line_row = ABSENT_SEQ_ROW;
        lines.push(Line::from(Span::styled(
            " No residues",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (chain_idx, chain_id, residues) in &chain_snapshot {
            let active = *chain_idx == app.current_chain;
            let header_style = if active {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            lines.push(Line::from(Span::styled(
                format!(" Chain {chain_id}  {} res", residues.len()),
                header_style,
            )));
            let mut block = push_wrapped_sequence(
                &mut lines,
                app,
                residues,
                chain_id,
                inner_width,
                active,
            );
            block.chain_idx = *chain_idx;
            if app.seq_blocks.is_empty() {
                app.seq_per_line = block.per_line;
                app.seq_line_row = block.start_row;
                app.ss_line_row = block.start_row.saturating_add(1);
            }
            app.seq_wrap_lines = app.seq_wrap_lines.saturating_add(block.wrap_lines);
            app.seq_blocks.push(block);
        }
        let keys = if app.shell.mode == crate::shell::InteractionMode::Select {
            " wheel:scroll  drag:select  all chains"
        } else {
            " wheel:scroll  all chains"
        };
        lines.push(Line::from(Span::styled(
            keys,
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
    app.panel_content_lines = lines.len() as u16;

    // No wrap: one Line is one row. Hit-test and panel_scroll share this axis.
    let panel = Paragraph::new(lines)
        .block(pane_block(&app.shell, PaneId::EditSpec))
        .scroll((app.panel_scroll, 0));

    frame.render_widget(panel, area);

    let view_h = area.height.saturating_sub(2);
    if app.panel_content_lines > view_h && area.width > 2 && area.height > 2 {
        let bar_area = Rect {
            x: area.x.saturating_add(area.width.saturating_sub(1)),
            y: area.y.saturating_add(1),
            width: 1,
            height: view_h,
        };
        let mut state = ScrollbarState::new(app.panel_content_lines as usize)
            .position(app.panel_scroll as usize)
            .viewport_content_length(view_h as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼")),
            bar_area,
            &mut state,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn residues_per_line_picks_25_or_50() {
        assert_eq!(residues_per_line(40), SEQ_LINE_NARROW);
        assert_eq!(residues_per_line(64), SEQ_LINE_WIDE);
    }

    #[test]
    fn display_col_inserts_gap_every_five() {
        assert_eq!(display_col_for_offset(0), SEQ_PREFIX_COLS);
        assert_eq!(display_col_for_offset(4), SEQ_PREFIX_COLS + 4);
        assert_eq!(display_col_for_offset(5), SEQ_PREFIX_COLS + 6);
        assert_eq!(offset_at_display_col(SEQ_PREFIX_COLS + 5, 25), Some(5));
        assert_eq!(offset_at_display_col(SEQ_PREFIX_COLS + 6, 25), Some(5));
    }

    #[test]
    fn letter_line_hit_skips_prefix_and_border() {
        let outer = Rect::new(10, 5, 40, 16);
        let seq_row = 2u16;
        let inner = sidebar_inner(outer);
        assert_eq!(
            hit_test_seq(outer, outer.x, inner.y + seq_row, seq_row, 25, 1, 0, 40),
            None
        );
        assert_eq!(
            hit_test_seq(outer, inner.x, inner.y + seq_row, seq_row, 25, 1, 0, 40),
            Some(SeqHit::Letter(0))
        );
        assert_eq!(
            hit_test_seq(
                outer,
                inner.x + SEQ_PREFIX_COLS as u16 + 2,
                inner.y + seq_row,
                seq_row,
                25,
                1,
                0,
                40
            ),
            Some(SeqHit::Letter(2))
        );
        assert_eq!(
            hit_test_seq(outer, inner.x, inner.y + 1, seq_row, 25, 1, 0, 40),
            None
        );
    }

    #[test]
    fn scroll_maps_visible_row_to_later_wrap_line() {
        let outer = Rect::new(10, 5, 40, 16);
        let inner = sidebar_inner(outer);
        let seq_row = 2u16;
        let scroll = 5u16;
        let hit = hit_test_seq(
            outer,
            inner.x + SEQ_PREFIX_COLS as u16,
            inner.y,
            seq_row,
            25,
            4,
            scroll,
            80,
        );
        assert_eq!(hit, Some(SeqHit::Letter(25)));
    }

    #[test]
    fn wrap_line_two_is_residue_25() {
        let outer = Rect::new(10, 5, 40, 20);
        let inner = sidebar_inner(outer);
        let seq_row = 2u16;
        let hit = hit_test_seq(
            outer,
            inner.x + SEQ_PREFIX_COLS as u16,
            inner.y + seq_row + SEQ_BLOCK_ROWS,
            seq_row,
            25,
            3,
            0,
            80,
        );
        assert_eq!(hit, Some(SeqHit::Letter(25)));
    }

    #[test]
    fn visual_rows_count_wrapped_header() {
        let wide = Line::from("─".repeat(40));
        assert_eq!(line_visual_rows(&wide, 20), 2);
        assert_eq!(visual_rows_before(&[wide, Line::from("")], 20), 3);
    }

    #[test]
    fn ss_line_hit_selects_segment_index() {
        let outer = Rect::new(10, 5, 40, 16);
        let inner = sidebar_inner(outer);
        let hit = hit_test_seq(
            outer,
            inner.x + SEQ_PREFIX_COLS as u16 + 1,
            inner.y + 3,
            2,
            25,
            1,
            0,
            40,
        );
        assert_eq!(hit, Some(SeqHit::SecondaryStructure(1)));
    }

    #[test]
    fn action_marker_line_keeps_residue_for_drag() {
        let outer = Rect::new(10, 5, 40, 16);
        let inner = sidebar_inner(outer);
        let hit = hit_test_seq(
            outer,
            inner.x + SEQ_PREFIX_COLS as u16,
            inner.y + 4,
            2,
            25,
            1,
            0,
            40,
        );
        assert_eq!(hit, Some(SeqHit::ActionMarker(0)));
    }

    #[test]
    fn hit_test_sequences_picks_second_chain() {
        let outer = Rect::new(10, 5, 40, 24);
        let inner = sidebar_inner(outer);
        let blocks = [
            SeqChainBlock {
                chain_idx: 0,
                start_row: 2,
                per_line: 25,
                wrap_lines: 1,
                residue_count: 10,
            },
            SeqChainBlock {
                chain_idx: 1,
                start_row: 6,
                per_line: 25,
                wrap_lines: 1,
                residue_count: 10,
            },
        ];
        let hit = hit_test_sequences(
            outer,
            inner.x + SEQ_PREFIX_COLS as u16,
            inner.y + 6,
            &blocks,
            0,
        );
        assert_eq!(hit, Some((1, SeqHit::Letter(0))));
    }

    #[test]
    fn absent_seq_rows_do_not_hit_header_as_letter() {
        let outer = Rect::new(10, 5, 40, 16);
        let inner = sidebar_inner(outer);
        let hit = hit_test_seq(outer, inner.x, inner.y, ABSENT_SEQ_ROW, 25, 1, 0, 8);
        assert_eq!(hit, None);
    }
}
