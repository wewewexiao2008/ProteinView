use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::model::protein::{MoleculeType, SecondaryStructure};

/// Width of the sequence sidebar in columns.
pub const SIDEBAR_WIDTH: u16 = 60;

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
/// This works over SSH and in most modern terminals (xterm, kitty, iterm2, screen, tmux).
pub fn yank_to_clipboard(text: &str) {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    // OSC 52: \x1b]52;c;<base64>\x07
    let seq = format!("\x1b]52;c;{}\x07", encoded);
    // Write directly to stderr (which is the terminal in raw mode)
    let _ = std::io::Write::write_all(&mut std::io::stderr(), seq.as_bytes());
}

/// Build a set of seq_num -> action for the given chain.
/// Returns a HashMap-like approach using owned strings.
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

/// Render the Sequence panel as a sidebar.
pub fn render_sequence_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let mut lines: Vec<Line> = Vec::new();

    // Get current chain
    let chain = match app.protein.chains.get(app.current_chain) {
        Some(c) if c.molecule_type == MoleculeType::Protein => c,
        _ => {
            lines.push(Line::from(Span::styled(
                " No protein chain loaded",
                Style::default().fg(Color::DarkGray),
            )));
            let panel = Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::RIGHT)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .title(" Sequence ")
                        .title_style(Style::default().fg(Color::Cyan)),
                )
                .wrap(Wrap { trim: false });
            frame.render_widget(panel, area);
            return;
        }
    };

    let chain_id = &chain.id;
    let residues = &chain.residues;
    let res_count = residues.len();

    // Header line: "Sequence -- Chain [A] (150 res)"
    let chain_indicator = if app.protein.chains.len() > 1 {
        let idx = app.current_chain + 1;
        format!(" [{}/{}]", idx, app.protein.chains.len())
    } else {
        String::new()
    };
    lines.push(Line::from(vec![
        Span::styled(
            " Sequence",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" -- Chain [{}]{} ({} res)", chain_id, chain_indicator, res_count),
            Style::default().fg(Color::White),
        ),
    ]));
    lines.push(Line::from(""));

    // Build action map for coloring (owned data to avoid lifetime issues)
    let action_map = build_action_set(&app.annotation, chain_id);

    // Compute available content width (subtract 2 for border/padding)
    let content_width = area.width.saturating_sub(2) as usize;
    if content_width == 0 || residues.is_empty() {
        lines.push(Line::from(Span::styled(
            " No residues",
            Style::default().fg(Color::DarkGray),
        )));
        render_panel(frame, area, lines);
        return;
    }

    // Determine how many residues to show
    let residues_per_line = content_width;

    // Apply horizontal scroll offset
    let h_scroll = app.seq_h_scroll as usize;
    let start_res = h_scroll;
    let end_res = (start_res + residues_per_line).min(res_count);

    if start_res >= res_count {
        lines.push(Line::from(Span::styled(
            " Scroll back with h",
            Style::default().fg(Color::DarkGray),
        )));
        render_panel(frame, area, lines);
        return;
    }

    // Line 1: Index markers (every 10 residues)
    let mut marker_spans: Vec<Span> = Vec::new();
    for (i, residue) in residues.iter().enumerate() {
        if i < start_res || i >= end_res {
            continue;
        }
        let seq_num = residue.seq_num;
        if seq_num > 0 && (seq_num as usize) % 10 == 1 {
            // Show the seq_num starting at this position
            let num_str = format!("{}", seq_num);
            marker_spans.push(Span::styled(
                num_str,
                Style::default().fg(Color::DarkGray),
            ));
        } else {
            marker_spans.push(Span::raw(" "));
        }
    }
    if !marker_spans.is_empty() {
        lines.push(Line::from(marker_spans));
    }

    // Line 2: Sequence line with action coloring + selection highlight
    let selection = &app.seq_selection;
    let mut seq_spans: Vec<Span> = Vec::new();
    for (i, residue) in residues.iter().enumerate() {
        if i < start_res || i >= end_res {
            continue;
        }
        let letter = aa_one_letter(&residue.name);
        let is_selected = selection.contains(i);

        // Determine action color
        let action = action_map.get(&residue.seq_num);
        let (fg, bg) = if is_selected {
            match action.map(|s| s.as_str()).unwrap_or("") {
                "keep" => (Color::Black, Color::Rgb(0, 200, 80)),
                "edit" => (Color::Black, Color::Rgb(255, 200, 0)),
                "replace" => (Color::Black, Color::Rgb(255, 80, 80)),
                "insert" => (Color::Black, Color::Rgb(80, 150, 255)),
                "delete" => (Color::Black, Color::Rgb(140, 140, 140)),
                _ => (Color::Black, Color::White),
            }
        } else {
            match action.map(|s| s.as_str()).unwrap_or("") {
                "keep" => (Color::Rgb(0, 200, 80), Color::default()),
                "edit" => (Color::Rgb(255, 200, 0), Color::default()),
                "replace" => (Color::Rgb(255, 80, 80), Color::default()),
                "insert" => (Color::Rgb(80, 150, 255), Color::default()),
                "delete" => (Color::Rgb(140, 140, 140), Color::default()),
                _ => (Color::White, Color::default()),
            }
        };

        let mut style = Style::default().fg(fg);
        if is_selected {
            style = style.bg(bg).add_modifier(Modifier::BOLD);
        }
        seq_spans.push(Span::styled(letter.to_string(), style));
    }
    if !seq_spans.is_empty() {
        lines.push(Line::from(seq_spans));
    }

    // Line 3: Secondary structure line
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

    // Line 4: Action markers line (from editspec regions)
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

    // Line 5: Legend
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("=", Style::default().fg(action_color("keep"))),
        Span::styled("keep ", Style::default().fg(Color::DarkGray)),
        Span::styled("~", Style::default().fg(action_color("edit"))),
        Span::styled("edit ", Style::default().fg(Color::DarkGray)),
        Span::styled(">", Style::default().fg(action_color("replace"))),
        Span::styled("repl ", Style::default().fg(Color::DarkGray)),
        Span::styled("+", Style::default().fg(action_color("insert"))),
        Span::styled("ins ", Style::default().fg(Color::DarkGray)),
        Span::styled("-", Style::default().fg(action_color("delete"))),
        Span::styled("del ", Style::default().fg(Color::DarkGray)),
        Span::styled("H", Style::default().fg(Color::Rgb(255, 100, 100))),
        Span::styled("helix ", Style::default().fg(Color::DarkGray)),
        Span::styled("E", Style::default().fg(Color::Rgb(100, 180, 255))),
        Span::styled("strand ", Style::default().fg(Color::DarkGray)),
        Span::styled("-", Style::default().fg(Color::Rgb(80, 80, 80))),
        Span::styled("coil", Style::default().fg(Color::DarkGray)),
    ]));

    // Selection info line
    if let Some((s, e)) = app.seq_selection.range() {
        let seq_start = residues[s].seq_num;
        let seq_end = residues[e].seq_num;
        let sel_len = e - s + 1;
        // Extract selected sequence letters
        let sel_seq: String = residues[s..=e]
            .iter()
            .map(|r| aa_one_letter(&r.name))
            .collect();

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!(" Selected: {}-{} ({} res)", seq_start, seq_end, sel_len),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        // Show selected sequence (truncate if too long)
        let display_seq = if sel_seq.len() > 40 {
            format!("{}...", &sel_seq[..40])
        } else {
            sel_seq
        };
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(display_seq, Style::default().fg(Color::Green)),
        ]));
    }

    // Scroll hint
    if res_count > residues_per_line {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                " h/l:scroll [{}/{}] [ ]:chains y:copy",
                start_res + 1,
                res_count
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Store panel metadata for mouse interaction
    // Header is 2 lines (title + empty), index marker is 1 line = 3
    // Sequence content starts at line 3 (0-indexed)
    app.panel_click_header = 3;
    app.panel_item_count = res_count;

    render_panel(frame, area, lines);
}

fn render_panel(frame: &mut Frame, area: Rect, lines: Vec<Line<'_>>) {
    let panel = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Sequence ")
                .title_style(Style::default().fg(Color::Cyan)),
        )
        .scroll((0, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(panel, area);
}
