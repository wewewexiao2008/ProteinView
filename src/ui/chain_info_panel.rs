use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;

/// Width of the chain info sidebar in columns.
pub const SIDEBAR_WIDTH: u16 = 34;

/// Render the Chain Info panel as a left-edge sidebar.
pub fn render_chain_info_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        " Chain Information",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Current chain info
    if let Some(chain) = app.protein.chains.get(app.current_chain) {
        let res_count = chain.residues.len();
        let atom_count = chain.residues.iter().map(|r| r.atoms.len()).sum::<usize>();

        lines.push(Line::from(vec![
            Span::styled(" Chain ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("[{}]", chain.id),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Residues: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", res_count),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Atoms: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", atom_count),
                Style::default().fg(Color::White),
            ),
        ]));

        // Sequence preview (first 25 residues)
        let seq: String = chain
            .residues
            .iter()
            .take(25)
            .map(|r| r.name.as_str())
            .collect();
        if !seq.is_empty() {
            let truncated = if chain.residues.len() > 25 {
                "..."
            } else {
                ""
            };
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Sequence:",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                format!(" {}", seq),
                Style::default().fg(Color::White),
            )));
            if !truncated.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!(" {}", truncated),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            " No chains loaded",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::from(""));

    // All chains overview
    lines.push(Line::from(Span::styled(
        " All Chains",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Record the row where chain items start (header + empty = 2 lines before items)
    let chain_list_start = lines.len() as u16;

    for (i, chain) in app.protein.chains.iter().enumerate() {
        let is_focused = i == app.current_chain;
        let res_count = chain.residues.len();
        if is_focused {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" \u{25b6} {} ", chain.id),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{} res", res_count),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("   {} ", chain.id),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{} res", res_count),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    }

    // Ligands section
    if !app.protein.ligands.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(" Ligands ({})", app.protein.ligands.len()),
            Style::default()
                .fg(Color::Rgb(255, 0, 255))
                .add_modifier(Modifier::BOLD),
        )));
        for ligand in app.protein.ligands.iter().take(10) {
            lines.push(Line::from(Span::styled(
                format!("   {} ({} atoms)", ligand.name, ligand.atoms.len()),
                Style::default().fg(Color::DarkGray),
            )));
        }
        if app.protein.ligands.len() > 10 {
            lines.push(Line::from(Span::styled(
                format!("   ... +{} more", app.protein.ligands.len() - 10),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    // Highlight info from annotation
    if let Some(ann) = &app.annotation {
        if let Some(hl) = &ann.highlights {
            if !hl.residues.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!(
                        " Highlighted: {} residues",
                        hl.residues.len()
                    ),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(vec![
                    Span::styled("   Chain: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        hl.chain.clone(),
                        Style::default().fg(Color::White),
                    ),
                ]));
                if let Some(ht) = &hl.highlight_type {
                    lines.push(Line::from(vec![
                        Span::styled("   Type: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            ht.clone(),
                            Style::default().fg(Color::White),
                        ),
                    ]));
                }
            }
        }
    }

    // Store item metadata for mouse click mapping
    app.panel_click_header = chain_list_start;
    app.panel_item_count = app.protein.chains.len();

    let panel = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Chain Info ")
                .title_style(Style::default().fg(Color::Cyan)),
        )
        .scroll((app.panel_scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(panel, area);
}
