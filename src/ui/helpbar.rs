use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::shell::{InteractionMode, Overlay};

/// Render the keybinding hints bar at the bottom.
pub fn render_helpbar(frame: &mut Frame, area: Rect, app: &App) {
    if app.shell.console_focused {
        frame.render_widget(
            Paragraph::new(Line::from(hint_line(&[
                ("j/k", "scroll"),
                ("v", "verbose"),
                ("u", "undo"),
                ("Esc/c", "back"),
                ("Tab", "pane"),
            ]))),
            area,
        );
        return;
    }
    if let Some(banner) = &app.status_banner {
        let spans = vec![
            Span::styled(
                "\u{2570}\u{2500}\u{2500} ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                banner.clone(),
                Style::default()
                    .fg(Color::Rgb(255, 200, 0))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ", Style::default().fg(Color::Gray)),
        ];
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }
    let spans = match app.shell.overlay {
        Overlay::Help => hint_line(&[("Esc/?", "close Help")]),
        Overlay::RunComposer | Overlay::RunStatus => {
            if app.debug {
                hint_line(&[
                    ("Enter", "Debug Run"),
                    ("Esc", "close"),
                    ("Ctrl+R", "ignored"),
                ])
            } else {
                hint_line(&[
                    ("h/l", "lane"),
                    ("+/-", "concurrency"),
                    ("Enter", "submit"),
                    ("Esc", "close"),
                ])
            }
        }
        Overlay::ContextMenu | Overlay::BlockPalette => hint_line(&[
            ("j/k", "item"),
            ("Enter", "apply"),
            ("Esc", "close"),
        ]),
        Overlay::None => match app.shell.mode {
            InteractionMode::EditRegion => hint_line(&[
                ("Tab", "field"),
                ("type", "A51-80"),
                ("Enter", "save"),
                ("Esc", "cancel"),
            ]),
            InteractionMode::Select => hint_line(&[
                ("h/l", "cursor"),
                ("H/L", "edge"),
                ("s", "segment"),
                ("[/]", "jump"),
                ("1-5", "action"),
                ("right", "menu"),
                ("Esc", "clear"),
                ("Tab", "pane"),
            ]),
            InteractionMode::Idle if app.shell.workflow_focused() => hint_line(&[
                ("j/k", "node"),
                ("Enter", "load"),
                ("a", "add"),
                ("d", "del"),
                ("drag", "from"),
                ("right", "menu"),
                ("Tab", "pane"),
            ]),
            InteractionMode::Idle if app.shell.tree_focused() => hint_line(&[
                ("j/k", "row"),
                ("h/l", "fold"),
                ("≡/◈", "seq/fold"),
                ("Enter", "load"),
                ("wheel", "scroll"),
            ]),
            InteractionMode::Idle if app.shell.editspec_focused() => hint_line(&[
                ("j/k", "regions"),
                ("x", "Select"),
                ("Enter", "form"),
                ("e", "edit"),
                ("right", "menu"),
                ("Tab", "pane"),
                ("f", "fold"),
            ]),
            InteractionMode::Idle => hint_line(&[
                ("Tab", "pane"),
                ("f", "fold"),
                ("drag", "size"),
                ("Ctrl+R", "Run"),
                ("D", if app.debug { "Dbg on" } else { "Dbg off" }),
                ("q", "quit"),
            ]),
        },
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn hint_line(pairs: &[(&str, &str)]) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        "\u{2570}\u{2500}\u{2500} ",
        Style::default().fg(Color::DarkGray),
    )];
    for (key, label) in pairs {
        spans.push(Span::styled(
            (*key).to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(":{label}  "),
            Style::default().fg(Color::Gray),
        ));
    }
    spans.push(Span::styled(
        "\u{2500}\u{2500}\u{256f}",
        Style::default().fg(Color::DarkGray),
    ));
    spans
}
