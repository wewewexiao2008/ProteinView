use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;

/// Width of the iteration sidebar in columns.
pub const SIDEBAR_WIDTH: u16 = 34;

/// Format a score with a trend arrow if a value is available.
fn format_score(label: &str, value: Option<f64>) -> Vec<Span<'static>> {
    match value {
        Some(v) => {
            let arrow = if v > 0.8 {
                "\u{2191}"
            } else if v > 0.5 {
                "\u{2192}"
            } else {
                "\u{2193}"
            };
            vec![
                Span::styled(format!(" {} ", label), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.3}", v),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!(" {}", arrow),
                    Style::default().fg(if v > 0.8 {
                        Color::Green
                    } else if v > 0.5 {
                        Color::Yellow
                    } else {
                        Color::Red
                    }),
                ),
            ]
        }
        None => vec![
            Span::styled(format!(" {} ", label), Style::default().fg(Color::DarkGray)),
            Span::styled(
                "---".to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ],
    }
}

/// Render the Iteration Overview panel as a left-edge sidebar.
pub fn render_iteration_panel(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        " Iteration Overview",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    match &app.annotation {
        Some(ann) => match &ann.iteration {
            Some(iter) => {
                // Iteration progress
                let progress_bar_width = 20u16;
                let filled = if iter.total > 0 {
                    (iter.current as u16 * progress_bar_width / iter.total as u16)
                        .min(progress_bar_width)
                } else {
                    0
                };

                let mut progress_spans = vec![
                    Span::styled(" ", Style::default()),
                    Span::styled(
                        format!("Iter {}/{}", iter.current, iter.total),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" [", Style::default().fg(Color::DarkGray)),
                ];
                for i in 0..progress_bar_width {
                    let c = if i < filled { '\u{2588}' } else { '\u{2591}' };
                    let color = if i < filled {
                        Color::Cyan
                    } else {
                        Color::DarkGray
                    };
                    progress_spans.push(Span::styled(
                        c.to_string(),
                        Style::default().fg(color),
                    ));
                }
                progress_spans.push(Span::styled(
                    "]",
                    Style::default().fg(Color::DarkGray),
                ));
                lines.push(Line::from(progress_spans));

                lines.push(Line::from(""));

                // Best scTM
                lines.push(Line::from(format_score("scTM ", iter.best_sc_tm)));

                // Best pLDDT
                if let Some(plddt) = iter.best_plddt {
                    lines.push(Line::from(vec![
                        Span::styled(" pLDDT ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format!("{:.1}", plddt),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!(
                                " {}",
                                if plddt > 90.0 {
                                    "\u{2605}"
                                } else if plddt > 70.0 {
                                    "\u{2606}"
                                } else {
                                    "\u{2022}"
                                }
                            ),
                            Style::default().fg(if plddt > 90.0 {
                                Color::Green
                            } else if plddt > 70.0 {
                                Color::Yellow
                            } else {
                                Color::Red
                            }),
                        ),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(" pLDDT ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            "---",
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                }

                lines.push(Line::from(""));

                // Candidate counts
                if let Some(candidates) = iter.candidates {
                    lines.push(Line::from(vec![
                        Span::styled(" Candidates: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format!("{}", candidates),
                            Style::default().fg(Color::White),
                        ),
                    ]));
                }
                if let Some(hq) = iter.high_quality {
                    lines.push(Line::from(vec![
                        Span::styled(" High quality: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format!("{}", hq),
                            Style::default().fg(Color::Green),
                        ),
                    ]));
                }
            }
            None => {
                lines.push(Line::from(Span::styled(
                    " No iteration data in annotation",
                    Style::default().fg(Color::DarkGray),
                )));
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
        }
    }

    let panel = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Iteration ")
                .title_style(Style::default().fg(Color::Cyan)),
        )
        .scroll((app.panel_scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(panel, area);
}
