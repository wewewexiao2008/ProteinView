mod app;
mod bridge;
mod debug_run;
mod edit_history;
mod events;
mod event;
mod model;
mod parser;
mod product_tree;
mod render;
mod shell;
mod workflow;
mod ui;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{MouseButton, MouseEvent, MouseEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use std::io;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use app::{
    ActivePanel, App, AppConfig, ConnectionType, ContextTarget, LayoutMode, RenderMode, VizMode,
};
use shell::{KeyAction, Overlay, PaneId, route_key};

macro_rules! log {
    ($file:expr, $($arg:tt)*) => {
        if let Some(f) = $file.as_mut() {
            use std::io::Write;
            let _ = writeln!(f, $($arg)*);
            let _ = f.flush();
        }
    };
}

/// Terminal protein structure viewer
#[derive(Parser)]
#[command(name = "proteinview", version, about = "TUI protein structure viewer")]
struct Cli {
    /// Path to PDB, mmCIF, or XYZ file
    file: Option<String>,

    /// Use HD rendering (HalfBlock over SSH, FullHD locally)
    #[arg(long)]
    hd: bool,

    /// Force full pixel graphics (Sixel/Kitty/iTerm2) regardless of SSH
    #[arg(long, alias = "pixel")]
    fullhd: bool,

    /// Render mode: braille, halfblock (or hd), fullhd (or pixel)
    #[arg(long = "render", value_name = "MODE")]
    render_mode: Option<String>,

    /// Color scheme: plddt, structure, element, chain, bfactor, rainbow
    #[arg(long, default_value = "plddt")]
    color: String,

    /// Visualization mode: cartoon, backbone, wireframe
    #[arg(long, default_value = "cartoon")]
    mode: String,

    /// Fetch structure from RCSB PDB by ID
    #[arg(long)]
    fetch: Option<String>,

    /// Write debug log to file (e.g. --log debug.log)
    #[arg(long)]
    log: Option<String>,

    /// Number of render threads (default: 4)
    #[arg(long, default_value = "4")]
    threads: usize,

    /// Export viewer state to a JSON file on exit (for external integration)
    #[arg(long)]
    state_file: Option<String>,

    /// Focus on a specific chain by name at startup
    #[arg(long)]
    focus_chain: Option<String>,

    /// Load annotation JSON for gemlib context panels
    #[arg(long)]
    annotation: Option<String>,

    /// Compact EditSpec to load at startup (`gemlib studio -e`)
    #[arg(long)]
    edit: Option<String>,

    /// Absolute gemlib CLI path from the Python launcher (never spawned here)
    #[arg(long)]
    gemlib_bin: Option<String>,

    /// Write compact EditSpec to this path on exit
    #[arg(long)]
    output: Option<String>,
}

fn apply_key_action(
    app: &mut App,
    action: KeyAction,
    logfile: &mut Option<std::fs::File>,
) {
    match action {
        KeyAction::Quit => app.should_quit = true,
        KeyAction::CyclePaneNext => {
            app.shell.cycle_focus_next();
            app.emit_session_focus();
        }
        KeyAction::CyclePanePrev => {
            app.shell.cycle_focus_prev();
            app.emit_session_focus();
        }
        KeyAction::ExitSelectKeepThenCycleNext => {
            app.shell.exit_select_keep_then_cycle(true);
        }
        KeyAction::ExitSelectKeepThenCyclePrev => {
            app.shell.exit_select_keep_then_cycle(false);
        }
        KeyAction::ToggleCollapse => app.toggle_focused_collapse(),
        KeyAction::EnterSelect => {
            app.enter_select_mode();
            app.sync_selection_overlay();
        }
        KeyAction::EnterRun => app.enter_run_mode(),
        KeyAction::ConfirmDebugRun => app.confirm_debug_run(),
        KeyAction::ToggleDebug => app.toggle_debug_mode(),
        KeyAction::RunCycleLane => app.cycle_run_priority(),
        KeyAction::RunConcurrency(delta) => app.bump_run_concurrency(delta),
        KeyAction::OpenEmptyForm => app.edit_region_open_empty(),
        KeyAction::EditFocusedRegion => app.edit_region_start(),
        KeyAction::ClearSelection => app.clear_editspec_selection(),
        KeyAction::CloseOverlay => app.close_run_overlay(),
        KeyAction::CloseContextMenu => {
            app.close_context_menu();
            app.status_banner = None;
        }
        KeyAction::ContextMenuNext => app.context_menu_move(1),
        KeyAction::ContextMenuPrev => app.context_menu_move(-1),
        KeyAction::ContextMenuApply => app.apply_context_menu(),
        KeyAction::CloseHelp => app.close_help_overlay(),
        KeyAction::ToggleHelp => app.toggle_help_overlay(),
        KeyAction::RotateX(d) => {
            app.camera.rotate_x(d);
            app.emit_view_camera();
        }
        KeyAction::RotateY(d) => {
            app.camera.rotate_y(d);
            app.emit_view_camera();
        }
        KeyAction::RotateZ(d) => {
            app.camera.rotate_z(d);
            app.emit_view_camera();
        }
        KeyAction::Pan(x, y) => {
            app.camera.pan(x, y);
            app.emit_view_camera();
        }
        KeyAction::ZoomIn => {
            app.camera.zoom_in();
            app.emit_view_camera();
        }
        KeyAction::ZoomOut => {
            app.camera.zoom_out();
            app.emit_view_camera();
        }
        KeyAction::ResetCamera => {
            let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
            app.reset_view_camera(cols, rows);
        }
        KeyAction::CycleColor => app.cycle_color(),
        KeyAction::CycleViz => app.cycle_viz_mode(),
        KeyAction::ToggleHd => {
            let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
            app.toggle_hd(cols, rows);
        }
        KeyAction::ToggleFullHd => {
            let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
            app.toggle_fullhd(cols, rows);
        }
        KeyAction::PrevChain => app.prev_chain(),
        KeyAction::NextChain => app.next_chain(),
        KeyAction::ToggleAutoRotate => app.camera.auto_rotate = !app.camera.auto_rotate,
        KeyAction::ToggleInterface => app.toggle_interface(),
        KeyAction::ToggleInteractions => app.toggle_interactions(),
        KeyAction::ToggleLigands => app.toggle_ligands(),
        KeyAction::RegionNext => {
            let region_count = app
                .annotation
                .as_ref()
                .and_then(|a| a.editspec_regions.as_ref())
                .map(|r| r.len())
                .unwrap_or(0);
            if region_count > 0 {
                let idx = (app.focused_region + 1).min(region_count - 1);
                app.focus_region(idx, false);
            }
        }
        KeyAction::RegionPrev => {
            if app.focused_region > 0 {
                app.focus_region(app.focused_region - 1, false);
            }
        }
        KeyAction::RegionAdd => app.edit_region_add(),
        KeyAction::RegionDelete => {
            app.edit_region_delete();
        }
        KeyAction::RegionSplit => app.edit_region_split(),
        KeyAction::Undo => app.edit_undo(),
        KeyAction::Redo => app.session_redo(),
        KeyAction::ToggleConsole => app.toggle_console(),
        KeyAction::CloseConsole => app.close_console_focus(),
        KeyAction::ConsoleCycleNext => {
            app.close_console_focus();
            app.shell.cycle_focus_next();
            app.emit_session_focus();
        }
        KeyAction::ConsoleCyclePrev => {
            app.close_console_focus();
            app.shell.cycle_focus_prev();
            app.emit_session_focus();
        }
        KeyAction::ConsoleScroll(delta) => app.scroll_console(delta),
        KeyAction::ToggleConsoleVerbose => app.toggle_console_verbose(),
        KeyAction::SessionUndo => app.session_undo(),
        KeyAction::SessionRedo => app.session_redo(),
        KeyAction::SeqCursor(delta) => {
            let residues = app.current_residues().to_vec();
            if !residues.is_empty() {
                app.seq_selection.move_cursor(&residues, delta);
                let cursor = app.seq_selection.cursor;
                app.ensure_seq_visible(cursor);
                app.sync_focused_region_from_selection();
                app.sync_selection_overlay();
                app.emit_editspec_cursor();
            }
        }
        KeyAction::SeqExpandStart(delta) => {
            let residues = app.current_residues().to_vec();
            if !residues.is_empty() {
                app.seq_selection.expand_start(&residues, delta);
                app.sync_focused_region_from_selection();
                app.sync_selection_overlay();
            }
        }
        KeyAction::SeqExpandEnd(delta) => {
            let residues = app.current_residues().to_vec();
            if !residues.is_empty() {
                app.seq_selection.expand_end(&residues, delta);
                app.sync_focused_region_from_selection();
                app.sync_selection_overlay();
            }
        }
        KeyAction::SeqSelectSegment => {
            let residues = app.current_residues().to_vec();
            if !residues.is_empty() {
                app.prepare_editspec_undo();
                let cursor = app.seq_selection.cursor;
                app.seq_selection.select_segment(&residues, cursor);
                app.sync_focused_region_from_selection();
                app.sync_selection_overlay();
                app.finish_editspec_select("select segment");
            }
        }
        KeyAction::SeqJumpSegment(dir) => {
            let residues = app.current_residues().to_vec();
            if !residues.is_empty() {
                app.seq_selection.jump_segment(&residues, dir);
                app.sync_focused_region_from_selection();
                app.sync_selection_overlay();
            }
        }
        KeyAction::SeqYankRange => {
            if let Some((s, e)) = app.seq_selection.range() {
                if let Some(c) = app.protein.chains.get(app.current_chain) {
                    if s < c.residues.len() && e < c.residues.len() {
                        let text = format!(
                            "{}:{}-{}",
                            c.id, c.residues[s].seq_num, c.residues[e].seq_num
                        );
                        ui::editspec_panel::yank_to_clipboard(&text);
                    }
                }
            }
        }
        KeyAction::SeqYankLetters => {
            if let Some((s, e)) = app.seq_selection.range() {
                if let Some(c) = app.protein.chains.get(app.current_chain) {
                    use ui::editspec_panel::aa_one_letter;
                    let seq: String = c.residues[s..=e]
                        .iter()
                        .map(|r| aa_one_letter(&r.name))
                        .collect();
                    if !seq.is_empty() {
                        ui::editspec_panel::yank_to_clipboard(&seq);
                    }
                }
            }
        }
        KeyAction::SeqActionShortcut(action) => {
            if let Some(msg) = app.apply_action_shortcut(action) {
                app.edit_state.validation_error = None;
                app.status_banner = Some(msg.clone());
                log!(logfile, "action shortcut: {}", msg);
            }
            app.revalidate();
        }
        KeyAction::EditFormTab => {
            if app.edit_state.cursor_field == app::EditField::Label {
                app.edit_cycle_label();
            } else {
                app.edit_next_field();
            }
        }
        KeyAction::EditFormBackTab => app.edit_prev_field(),
        KeyAction::EditFormNextField => app.edit_next_field(),
        KeyAction::EditFormPrevField => app.edit_prev_field(),
        KeyAction::EditFormAdjust(delta) => match app.edit_state.cursor_field {
            app::EditField::Action => app.edit_cycle_action(delta > 0),
            app::EditField::Chain => app.edit_cycle_chain(delta > 0),
            app::EditField::RangeStart => app.edit_adjust_range(app::EditField::RangeStart, delta),
            app::EditField::RangeEnd => app.edit_adjust_range(app::EditField::RangeEnd, delta),
            _ => {}
        },
        KeyAction::EditFormSave => {
            app.edit_save();
        }
        KeyAction::EditFormCancel => app.edit_cancel(),
        KeyAction::EditFormBackspace => app.edit_label_backspace(),
        KeyAction::EditFormChar(ch) => {
            match app.edit_state.cursor_field {
                app::EditField::Action if ch == 'h' => app.edit_cycle_action(false),
                app::EditField::Action if ch == 'l' => app.edit_cycle_action(true),
                _ => app.edit_label_input(ch),
            }
        }
        KeyAction::TreeNext => app.tree_move(1),
        KeyAction::TreePrev => app.tree_move(-1),
        KeyAction::TreeCollapse => app.tree_set_expanded(false),
        KeyAction::TreeExpand => app.tree_set_expanded(true),
        KeyAction::TreeActivate => {
            if let Err(err) = app.activate_tree_row() {
                log!(logfile, "tree activate failed: {err}");
            }
        }
        KeyAction::WorkflowNext => app.workflow_move(1),
        KeyAction::WorkflowPrev => app.workflow_move(-1),
        KeyAction::OpenBlockPalette => app.open_block_palette(),
        KeyAction::WorkflowDelete => app.request_workflow_delete(),
        KeyAction::BlockPalettePrev => app.block_palette_move(-1),
        KeyAction::BlockPaletteNext => app.block_palette_move(1),
        KeyAction::BlockPaletteApply => app.apply_block_palette(),
        KeyAction::CloseBlockPalette => app.close_block_palette(),
        KeyAction::WorkflowActivate => {
            if let Err(err) = app.activate_workflow_node() {
                log!(logfile, "workflow activate failed: {err}");
            }
        }
        KeyAction::RunIgnore | KeyAction::Ignore => {}
    }
}

/// Handle mouse events for sidebar interaction.
fn chrome_rects(app: &App) -> ui::chrome::ChromeRects {
    ui::chrome::ChromeRects {
        workflow: app.last_workflow_rect.unwrap_or_default(),
        tree: app.last_tree_rect.unwrap_or_default(),
        view: app.last_view_rect.unwrap_or_default(),
        editspec: app.last_sidebar_rect.unwrap_or_default(),
    }
}

fn handle_mouse_event(app: &mut App, me: MouseEvent, logfile: &mut Option<std::fs::File>) {
    app.mouse_event_count += 1;
    log!(
        logfile,
        "mouse: kind={:?} row={} col={} (total={})",
        me.kind,
        me.row,
        me.column,
        app.mouse_event_count
    );
    match me.kind {
        MouseEventKind::Down(MouseButton::Left)
            if app.shell.overlay == Overlay::BlockPalette =>
        {
            if let Some(palette) = app.block_palette.as_ref() {
                if let Some(idx) = ui::block_palette::hit_test_item(palette, me.column, me.row) {
                    if let Some(palette) = app.block_palette.as_mut() {
                        palette.cursor = idx;
                    }
                    app.apply_block_palette();
                    return;
                }
            }
            app.close_block_palette();
            return;
        }
        MouseEventKind::Down(MouseButton::Left)
            if app.shell.overlay == Overlay::ContextMenu =>
        {
            if let Some(menu) = app.context_menu.as_ref() {
                if let Some(idx) = ui::context_menu::hit_test_item(menu, me.column, me.row) {
                    if let Some(menu) = app.context_menu.as_mut() {
                        menu.cursor = idx;
                    }
                    app.apply_context_menu();
                    return;
                }
            }
            app.close_context_menu();
            return;
        }
        MouseEventKind::Down(MouseButton::Right) => {
            if matches!(
                app.shell.overlay,
                Overlay::Help | Overlay::RunComposer | Overlay::RunStatus | Overlay::BlockPalette
            ) {
                return;
            }
            if app.shell.overlay == Overlay::ContextMenu {
                app.close_context_menu();
            }
            match ui::chrome::pane_at(&chrome_rects(app), me.column, me.row) {
                Some(PaneId::EditSpec) => {
                    if let Some(sidebar_rect) = app.last_sidebar_rect {
                        handle_sidebar_right_click(app, me.row, me.column, sidebar_rect, logfile);
                    }
                }
                Some(PaneId::Workflow) => {
                    app.handle_workflow_right_click(me.column, me.row);
                }
                _ => {}
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            if app.shell.console_focused {
                if me.kind == MouseEventKind::ScrollUp {
                    app.scroll_console(-1);
                } else {
                    app.scroll_console(1);
                }
                return;
            }
            match ui::chrome::pane_at(&chrome_rects(app), me.column, me.row) {
                Some(PaneId::View) => {
                    if me.kind == MouseEventKind::ScrollUp {
                        app.camera.zoom_in();
                    } else {
                        app.camera.zoom_out();
                    }
                }
                Some(PaneId::EditSpec) => {
                    if me.kind == MouseEventKind::ScrollUp {
                        app.panel_scroll = app.panel_scroll.saturating_sub(1);
                    } else {
                        let max_scroll = max_panel_scroll(app);
                        app.panel_scroll = app.panel_scroll.saturating_add(1).min(max_scroll);
                    }
                }
                Some(PaneId::Tree) => {
                    if me.kind == MouseEventKind::ScrollUp {
                        app.tree_scroll = app.tree_scroll.saturating_sub(1);
                    } else {
                        app.tree_scroll = app
                            .tree_scroll
                            .saturating_add(1)
                            .min(app.max_tree_scroll());
                    }
                }
                _ => {}
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if app.shell.overlay.is_open() {
                return;
            }
            app.view_drag_last = None;
            app.chrome_drag = None;
            app.workflow_drag = None;
            let rects = chrome_rects(app);
            if let Some(pane) = ui::chrome::fold_pane_at(&rects, me.column, me.row) {
                app.pointer_focus_pane(pane);
                app.shell.toggle_pane(pane);
                return;
            }
            if let Some(drag) = ui::chrome::divider_at(&rects, app.layout_mode, me.column, me.row)
            {
                app.chrome_drag = Some(drag);
                return;
            }
            if let Some(pane) = ui::chrome::pane_at(&rects, me.column, me.row) {
                app.pointer_focus_pane(pane);
                match pane {
                    PaneId::Tree => {
                        if let Err(err) = app.handle_tree_click(me.column, me.row) {
                            log!(logfile, "tree click failed: {err}");
                        }
                    }
                    PaneId::EditSpec => {
                        if let Some(sidebar_rect) = app.last_sidebar_rect {
                            handle_sidebar_click(app, me.row, me.column, sidebar_rect, logfile);
                        }
                    }
                    PaneId::View => {
                        app.seq_selection.dragging = false;
                        app.view_drag_last = Some((me.column, me.row));
                    }
                    PaneId::Workflow => {
                        if let Err(err) = app.handle_workflow_click(me.column, me.row) {
                            log!(logfile, "workflow click failed: {err}");
                        }
                    }
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.shell.overlay.is_open() {
                return;
            }
            if let Some(drag) = app.chrome_drag {
                let rects = chrome_rects(app);
                ui::chrome::apply_drag(&mut app.shell, drag, &rects, me.column, me.row);
                return;
            }
            if app.seq_selection.dragging {
                if let Some(sidebar_rect) = app.last_sidebar_rect {
                    if let Some((chain_idx, hit)) = ui::editspec_panel::hit_test_sequences(
                        sidebar_rect,
                        me.column,
                        me.row,
                        &app.seq_blocks,
                        app.panel_scroll,
                    ) {
                        if chain_idx == app.current_chain {
                            let idx = match hit {
                                ui::editspec_panel::SeqHit::Letter(i)
                                | ui::editspec_panel::SeqHit::SecondaryStructure(i)
                                | ui::editspec_panel::SeqHit::ActionMarker(i) => i,
                            };
                            let max_res = app.current_residues().len();
                            app.seq_selection.drag_to(idx, max_res);
                            app.ensure_seq_visible(idx);
                            app.sync_focused_region_from_selection();
                            app.enter_select_mode();
                            app.sync_selection_overlay();
                        }
                    }
                }
                return;
            }
            if app.workflow_drag.is_some() {
                app.handle_workflow_drag(me.column, me.row);
                return;
            }
            if app.view_drag_last.is_some() {
                app.apply_view_drag(me.column, me.row);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if app.workflow_drag.is_some() {
                app.handle_workflow_drop(me.column, me.row);
            }
            app.seq_selection.dragging = false;
            app.view_drag_last = None;
            app.chrome_drag = None;
        }
        _ => {}
    }
}

/// Calculate the maximum scroll offset for the current panel.
/// Prevents scrolling past the last visible item.
fn max_panel_scroll(app: &App) -> u16 {
    let view_h = app
        .last_sidebar_rect
        .map(|r| r.height.saturating_sub(2))
        .unwrap_or(0);
    app.panel_content_lines.saturating_sub(view_h)
}

/// Handle a click inside the sidebar area.
fn handle_sidebar_click(
    app: &mut App,
    row: u16,
    col: u16,
    sidebar_rect: Rect,
    logfile: &mut Option<std::fs::File>,
) {
    if app.shell.focused != PaneId::EditSpec {
        return;
    }

    if !app.current_residues().is_empty() || !app.seq_blocks.is_empty() {
        match ui::editspec_panel::hit_test_sequences(
            sidebar_rect,
            col,
            row,
            &app.seq_blocks,
            app.panel_scroll,
        ) {
            Some((chain_idx, ui::editspec_panel::SeqHit::Letter(idx))) => {
                app.set_current_chain(chain_idx);
                let residues = app.current_residues().to_vec();
                app.seq_selection.click(&residues, idx);
                app.ensure_seq_visible(idx);
                app.sync_focused_region_from_selection();
                app.enter_select_mode();
                app.sync_selection_overlay();
                log!(
                    logfile,
                    "seq_click: residue_idx={} range={:?}",
                    idx,
                    app.seq_selection.range()
                );
                return;
            }
            Some((chain_idx, ui::editspec_panel::SeqHit::SecondaryStructure(idx))) => {
                app.set_current_chain(chain_idx);
                let residues = app.current_residues().to_vec();
                app.seq_selection.select_segment(&residues, idx);
                app.ensure_seq_visible(idx);
                app.sync_focused_region_from_selection();
                app.enter_select_mode();
                app.sync_selection_overlay();
                log!(
                    logfile,
                    "ss_click: residue_idx={} segment={:?}",
                    idx,
                    app.seq_selection.range()
                );
                return;
            }
            Some((_, ui::editspec_panel::SeqHit::ActionMarker(_))) => return,
            None => {}
        }
    }

    let inner = ui::editspec_panel::sidebar_inner(sidebar_rect);
    let item_row = row.saturating_sub(inner.y).saturating_add(app.panel_scroll);
    let header = app.panel_click_header;
    if item_row >= header && app.panel_item_count > 0 {
        let region_idx = (item_row - header) as usize;
        if region_idx < app.panel_item_count {
            app.focus_region(region_idx, true);
            log!(
                logfile,
                "sidebar_click: panel=EditSpec region_idx={}",
                region_idx
            );
        }
    }
}

fn handle_sidebar_right_click(
    app: &mut App,
    row: u16,
    col: u16,
    sidebar_rect: Rect,
    logfile: &mut Option<std::fs::File>,
) {
    if !app.seq_blocks.is_empty() {
        match ui::editspec_panel::hit_test_sequences(
            sidebar_rect,
            col,
            row,
            &app.seq_blocks,
            app.panel_scroll,
        ) {
            Some((chain_idx, ui::editspec_panel::SeqHit::Letter(idx)))
            | Some((chain_idx, ui::editspec_panel::SeqHit::SecondaryStructure(idx))) => {
                app.set_current_chain(chain_idx);
                let residues = app.current_residues().to_vec();
                if app.seq_selection.contains(idx) {
                    app.enter_select_mode();
                    app.open_context_menu(col, row, ContextTarget::Selection);
                    log!(logfile, "context_menu: selection at residue {idx}");
                    return;
                }
                if let Some(region_idx) = app.region_index_covering_residue(idx) {
                    app.focus_region(region_idx, false);
                    app.open_context_menu(col, row, ContextTarget::Region(region_idx));
                    log!(logfile, "context_menu: region {region_idx} at residue {idx}");
                    return;
                }
                app.seq_selection.click(&residues, idx);
                app.enter_select_mode();
                app.sync_selection_overlay();
                app.open_context_menu(col, row, ContextTarget::Selection);
                log!(logfile, "context_menu: new selection at residue {idx}");
                return;
            }
            Some((_, ui::editspec_panel::SeqHit::ActionMarker(_))) => return,
            None => {}
        }
    }

    let inner = ui::editspec_panel::sidebar_inner(sidebar_rect);
    let item_row = row.saturating_sub(inner.y).saturating_add(app.panel_scroll);
    let header = app.panel_click_header;
    if item_row >= header && app.panel_item_count > 0 {
        let region_idx = (item_row - header) as usize;
        if region_idx < app.panel_item_count {
            app.focus_region(region_idx, true);
            app.open_context_menu(col, row, ContextTarget::Region(region_idx));
            log!(logfile, "context_menu: region list {region_idx}");
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Cap rayon thread pool. 4 threads is the sweet spot: the framebuffer
    // only has ~60 tiles (64x64) so more threads hit diminishing returns,
    // and 4 leaves cores free for the terminal emulator and OS.
    let num_threads = cli.threads.max(1);
    match rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build_global()
    {
        Ok(()) => {}
        Err(e) => eprintln!("Warning: failed to initialize rayon thread pool: {e}"),
    }

    // Determine the file path
    let file_path = if let Some(pdb_id) = &cli.fetch {
        parser::fetch::fetch_pdb(pdb_id)?
    } else if let Some(path) = &cli.file {
        path.clone()
    } else {
        eprintln!("Error: provide a file path or use --fetch <PDB_ID>");
        std::process::exit(1);
    };

    // Load protein structure (dispatch by file extension)
    let lower = file_path.to_lowercase();
    let is_xyz = lower.ends_with(".xyz");
    let protein = if is_xyz {
        parser::xyz::load_xyz(&file_path)?
    } else {
        parser::pdb::load_structure(&file_path)?
    };
    eprintln!(
        "Loaded: {} ({} chains, {} residues, {} atoms{})",
        protein.name,
        protein.chains.len(),
        protein.residue_count(),
        protein.atom_count(),
        if protein.ligands.is_empty() {
            String::new()
        } else {
            format!(", {} ligands", protein.ligand_count())
        },
    );

    // Open log file if requested
    let mut logfile: Option<std::fs::File> = match &cli.log {
        Some(path) => {
            let f = std::fs::File::create(path)
                .map_err(|e| anyhow::anyhow!("cannot create log file '{}': {}", path, e))?;
            Some(f)
        }
        None => None,
    };

    // Detect connection type
    let connection_type = ConnectionType::detect();
    log!(logfile, "connection type: {:?}", connection_type);

    // Determine render mode from CLI flags
    let render_mode = if let Some(mode_str) = &cli.render_mode {
        match mode_str.to_ascii_lowercase().as_str() {
            "braille" => RenderMode::Braille,
            "halfblock" | "hd" | "half-block" => RenderMode::HalfBlock,
            "fullhd" | "pixel" | "full-hd" => RenderMode::FullHD,
            _ => {
                eprintln!("Warning: unknown render mode '{}', using default", mode_str);
                RenderMode::Braille
            }
        }
    } else if cli.fullhd {
        // --fullhd / --pixel always forces FullHD regardless of SSH
        RenderMode::FullHD
    } else if cli.hd {
        // --hd is SSH-aware: FullHD locally, HalfBlock over SSH
        match connection_type {
            ConnectionType::Local => RenderMode::FullHD,
            ConnectionType::Ssh => RenderMode::HalfBlock,
        }
    } else {
        RenderMode::Braille
    };

    // Get terminal dimensions before entering alternate screen
    let (term_cols, term_rows) = crossterm::terminal::size().unwrap_or((80, 24));
    log!(logfile, "terminal size: {}x{}", term_cols, term_rows);

    // Install panic hook that restores the terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);
        original_hook(info);
    }));

    // Setup terminal — must happen before Picker::from_query_stdio()
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Detect terminal graphics protocol (Sixel/Kitty/iTerm2) and font size.
    // Must be called after entering alternate screen but before spawning the
    // input thread (which reads from stdin).
    let picker = ratatui_image::picker::Picker::from_query_stdio()
        .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks());
    log!(
        logfile,
        "picker: protocol={:?} font_size={:?}",
        picker.protocol_type(),
        picker.font_size()
    );

    // Re-enable raw mode after Picker::from_query_stdio().
    // The picker internally calls enable_raw_mode()/disable_raw_mode() to query
    // the terminal, which can leave the terminal in a non-raw state on some
    // systems.  Re-establishing raw mode here ensures crossterm's event reader
    // receives proper input (including mouse escape sequences).
    disable_raw_mode()?;
    enable_raw_mode()?;
    log!(logfile, "raw mode re-enabled after picker query");

    // Parse CLI color scheme override
    let color_override = match cli.color.to_ascii_lowercase().as_str() {
        "structure" => None, // default, no override needed
        "element" => Some(render::color::ColorSchemeType::Element),
        "chain" => Some(render::color::ColorSchemeType::Chain),
        "bfactor" | "b-factor" => Some(render::color::ColorSchemeType::BFactor),
        "rainbow" => Some(render::color::ColorSchemeType::Rainbow),
        "plddt" => Some(render::color::ColorSchemeType::Plddt),
        _ => {
            eprintln!(
                "Warning: unknown color scheme '{}', using structure",
                cli.color
            );
            None
        }
    };

    // Parse CLI visualization mode override
    let user_explicit_mode = !cli.mode.eq_ignore_ascii_case("cartoon")
        || std::env::args().any(|a| a == "--mode" || a.starts_with("--mode="));
    let viz_mode = match cli.mode.to_ascii_lowercase().as_str() {
        "cartoon" => VizMode::Cartoon,
        "backbone" => VizMode::Backbone,
        "wireframe" => VizMode::Wireframe,
        _ => {
            eprintln!(
                "Warning: unknown visualization mode '{}', using cartoon",
                cli.mode
            );
            VizMode::Cartoon
        }
    };

    // XYZ files default to Element coloring + Wireframe mode unless overridden
    let (color_override, viz_mode) = if is_xyz {
        let color = if color_override.is_none() && cli.color == "structure" {
            Some(render::color::ColorSchemeType::Element)
        } else {
            color_override
        };
        let viz = if !user_explicit_mode {
            VizMode::Wireframe
        } else {
            viz_mode
        };
        (color, viz)
    } else {
        (color_override, viz_mode)
    };

    // Create app with actual terminal dimensions for dynamic zoom
    let mut app = App::new(
        protein,
        AppConfig {
            render_mode,
            viz_mode,
            user_explicit_mode,
            color_override,
        },
        term_cols,
        term_rows,
        picker,
    );
    // Apply --focus-chain: set initial chain by name
    if let Some(chain_name) = &cli.focus_chain {
        let idx = app
            .protein
            .chains
            .iter()
            .position(|c| &c.id == chain_name);
        if let Some(i) = idx {
            app.current_chain = i;
            log!(logfile, "focus_chain: set to '{}' (index {})", chain_name, i);
        } else {
            eprintln!(
                "Warning: chain '{}' not found (available: {})",
                chain_name,
                app.protein.chains.iter().map(|c| c.id.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
    }

    // Load annotation JSON if provided
    if let Some(ann_path) = &cli.annotation {
        app.load_annotation(ann_path);
        log!(logfile, "annotation: loaded from '{}'", ann_path);
    }
    if let Some(spec) = &cli.edit {
        app.load_edit_spec_text(spec);
        log!(logfile, "edit: loaded '{}'", spec);
    }
    app.gemlib_bin = cli.gemlib_bin.clone();
    app.output_path = cli.output.clone();
    app.active_panel = ActivePanel::EditSpec;
    if let Some(state_path) = &cli.state_file {
        app.state_file = Some(state_path.clone());
        if let Ok(text) = std::fs::read_to_string(state_path) {
            match product_tree::load_studio_seed(&text) {
                Ok(seed) => {
                    let has_tree = !seed.product_tree.is_empty();
                    app.apply_studio_seed(seed);
                    if has_tree {
                        log!(logfile, "product_tree: loaded from '{}'", state_path);
                    }
                }
                Err(err) => log!(logfile, "product_tree: ignored seed ({err})"),
            }
        }
        if let Ok(meta) = std::fs::metadata(state_path) {
            app.state_mtime = meta.modified().ok();
        }
    }

    // Enable mouse capture for sidebar interaction
    execute!(
        terminal.backend_mut(),
        crossterm::event::EnableMouseCapture
    )?;

    log!(
        logfile,
        "app created: render_mode={:?} chains={} zoom={:.2} active_panel={:?}",
        app.render_mode,
        app.protein.chains.len(),
        app.camera.zoom,
        app.active_panel
    );

    // Spawn dedicated input thread — decouples input from rendering so
    // quit always works even when HD rendering is slow
    let (input_rx, quit_flag) = event::spawn_input_thread();

    // Main loop
    let tick_rate = Duration::from_millis(33); // ~30 FPS
    let mut frame_count: u64 = 0;
    // Track how long the previous terminal.draw() took so we can skip frames
    // when rendering is too slow (prevents PTY buffer saturation & freezes).
    let mut last_draw_duration = Duration::ZERO;
    let mut frames_to_skip: u32 = 0;

    loop {
        // Drain all queued input from the dedicated input thread
        let mut had_input = false;
        while let Ok(app_event) = input_rx.try_recv() {
            had_input = true;
            match app_event {
                event::AppEvent::Resize(cols, rows) => {
                    log!(logfile, "resize: {}x{}", cols, rows);
                    let old_mode = app.layout_mode;
                    app.recalculate_zoom(cols, rows);
                    app.mesh_dirty_flag();
                    // Reset scroll when layout mode changes to avoid stale offsets
                    if app.layout_mode != old_mode {
                        app.panel_scroll = 0;
                    }
                }
                event::AppEvent::Key(key) => {
                    log!(logfile, "key: {:?}", key.code);
                    let action = route_key(
                        &app.shell,
                        key,
                        app.seq_selection.active,
                        app.can_run(),
                    );
                    apply_key_action(&mut app, action, &mut logfile);
                }
                event::AppEvent::Mouse(me) => {
                    handle_mouse_event(&mut app, me, &mut logfile);
                }
            }
        }

        if app.should_quit {
            break;
        }

        // Ensure ribbon mesh cache is fresh (rebuilds only when color scheme changes).
        // Must happen outside terminal.draw() since ribbon_mesh() needs &mut self.
        // Only rebuild when in Cartoon mode — Backbone/Wireframe don't use the
        // ribbon mesh, so skipping this preserves the lazy-mesh optimization for
        // large structures that start in a non-Cartoon mode.
        if app.viz_mode == VizMode::Cartoon {
            app.ribbon_mesh();
        }

        // Always poll the background interface thread, even during skipped
        // frames, so the result is absorbed as soon as it's available.
        app.poll_background_interface();
        app.poll_studio_state();

        // Adaptive frame skipping: if the previous draw took longer than the
        // tick rate, skip frames proportionally.  User input always forces a
        // redraw so the UI stays responsive.
        //
        // Do NOT call app.tick() during skipped frames — that would advance
        // auto-rotate without a corresponding render, causing the protein to
        // "jump" when rendering resumes.  Instead we just sleep and let the
        // camera's dt-clamping handle the gap.
        if frames_to_skip > 0 && !had_input {
            frames_to_skip -= 1;
            // Reset the camera's tick timer so the next real tick doesn't see
            // a huge accumulated dt from the skipped frames.
            app.camera.reset_tick_timer();
            std::thread::sleep(tick_rate);
            continue;
        }

        // Render
        frame_count += 1;
        if frame_count <= 3 || frame_count % 300 == 0 {
            log!(
                logfile,
                "frame {} render start (render_mode={:?} viz={:?} panel={:?} last_draw={:?})",
                frame_count,
                app.render_mode,
                app.viz_mode,
                app.active_panel,
                last_draw_duration
            );
        }

        // After a render-mode switch, force ratatui to redraw every cell.
        // Without this, its diff-based rendering may leave stale characters
        // from the previous mode (e.g. braille dots under a FullHD image).
        if app.needs_clear {
            // Delete any Kitty graphics images that may be lingering from
            // a previous FullHD session.  Harmless no-op if there are none.
            let cleanup = render::kitty_png::KittyPngImage::cleanup_escape();
            execute!(terminal.backend_mut(), crossterm::style::Print(&cleanup))?;
            terminal.clear()?;
            app.needs_clear = false;
        }

        let draw_start = Instant::now();
        terminal.draw(|frame| {
            let console_h = if app.console_open { 6 } else { 0 };
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(8),
                    Constraint::Length(console_h),
                    Constraint::Length(2),
                    Constraint::Length(1),
                ])
                .split(frame.area());

            ui::header::render_header(frame, outer[0], &app.protein.name, app.python_available);

            let chrome = ui::chrome::split_chrome(outer[1], &app.shell, app.layout_mode);
            app.last_workflow_rect = Some(chrome.workflow);
            app.last_tree_rect = Some(chrome.tree);
            app.last_view_rect = Some(chrome.view);
            app.last_sidebar_rect = Some(chrome.editspec);

            ui::workflow_pane::render_workflow_pane(frame, chrome.workflow, &app);
            ui::tree_pane::render_tree_pane(frame, chrome.tree, &app);

            let view_block = ui::chrome::pane_block(&app.shell, PaneId::View);
            if app.shell.is_expanded(PaneId::View) {
                let view_inner = view_block.inner(chrome.view);
                frame.render_widget(view_block, chrome.view);
                ui::viewport::render_viewport(frame, view_inner, &app);
            } else {
                frame.render_widget(view_block, chrome.view);
            }

            if app.shell.is_expanded(PaneId::EditSpec) {
                ui::editspec_panel::render_editspec_panel(frame, chrome.editspec, &mut app);
            } else {
                frame.render_widget(
                    ui::chrome::pane_block(&app.shell, PaneId::EditSpec),
                    chrome.editspec,
                );
            }

            if app.console_open {
                ui::console::render_console(frame, outer[2], &app);
            }
            ui::statusbar::render_statusbar(frame, outer[3], &app);
            ui::helpbar::render_helpbar(frame, outer[4], &app);

            match app.shell.overlay {
                Overlay::RunComposer | Overlay::RunStatus => {
                    ui::run_overlay::render_run_overlay(frame, frame.area(), &app);
                }
                Overlay::Help => {
                    ui::help_overlay::render_help_overlay(frame, frame.area());
                }
                Overlay::ContextMenu => {
                    if let Some(menu) = app.context_menu.as_mut() {
                        ui::context_menu::render_context_menu(frame, frame.area(), menu);
                    }
                }
                Overlay::BlockPalette => {
                    if let Some(palette) = app.block_palette.as_mut() {
                        ui::block_palette::render_block_palette(frame, frame.area(), palette);
                    }
                }
                Overlay::None => {}
            }
        })?;
        last_draw_duration = draw_start.elapsed();

        // If the draw took longer than two tick periods, skip some frames to
        // let the terminal catch up and avoid saturating the PTY write buffer.
        if last_draw_duration > tick_rate * 2 {
            // Skip 1-3 frames depending on how slow the draw was.
            frames_to_skip = ((last_draw_duration.as_millis() / tick_rate.as_millis()) as u32)
                .saturating_sub(1)
                .min(3);
        }

        app.tick();

        // Sleep for the remainder of the tick period to cap at ~30 FPS.
        // Account for the time already spent drawing so the frame rate stays
        // consistent regardless of render cost.
        let elapsed = draw_start.elapsed();
        if let Some(remaining) = tick_rate.checked_sub(elapsed) {
            std::thread::sleep(remaining);
        }
    }

    // Signal input thread to stop
    quit_flag.store(true, Ordering::Relaxed);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), crossterm::event::DisableMouseCapture)?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Some(output) = &app.output_path {
        if let Some(spec) = app.compact_edit_spec_string() {
            if let Err(e) = std::fs::write(output, spec) {
                eprintln!("Warning: failed to write --output '{output}': {e}");
            }
        }
    }

    // Export viewer state if --state-file was provided
    if let Some(state_path) = &cli.state_file {
        let focused_chain = app
            .protein
            .chains
            .get(app.current_chain)
            .map(|c| c.id.as_str())
            .unwrap_or("?");
        let viz_name = app.viz_mode.name();
        let color_name = if app.active_panel == ActivePanel::Interface {
            "Interface"
        } else {
            match &app.color_scheme.scheme_type {
                render::color::ColorSchemeType::Structure => "Structure",
                render::color::ColorSchemeType::Chain => "Chain",
                render::color::ColorSchemeType::Element => "Element",
                render::color::ColorSchemeType::BFactor => "BFactor",
                render::color::ColorSchemeType::Rainbow => "Rainbow",
                render::color::ColorSchemeType::Plddt => "Plddt",
                render::color::ColorSchemeType::Interface => "Interface",
            }
        };
        let render_name = app.render_mode.name();
        let (rot_x, rot_y, rot_z) = app.camera.euler_angles();
        let m = app.camera.rotation_matrix();
        let state_json = format!(
            "{{\n  \"focused_chain\": \"{}\",\n  \"viz_mode\": \"{}\",\n  \"color_scheme\": \"{}\",\n  \"render_mode\": \"{}\",\n  \"camera\": {{ \"rot_x\": {:.6}, \"rot_y\": {:.6}, \"rot_z\": {:.6}, \"zoom\": {:.6}, \"pan_x\": {:.6}, \"pan_y\": {:.6} }},\n  \"rotation_matrix\": [[{:.15e}, {:.15e}, {:.15e}], [{:.15e}, {:.15e}, {:.15e}], [{:.15e}, {:.15e}, {:.15e}]],\n  \"active_panel\": \"{}\",\n  \"interface_active\": {},\n  \"show_interactions\": {},\n  \"show_ligands\": {},\n  \"auto_rotate\": {},\n  \"edit_spec\": \"{}\",\n  \"focused_pane\": \"{}\",\n  \"interaction_mode\": \"{}\"\n}}\n",
            focused_chain,
            viz_name,
            color_name,
            render_name,
            rot_x,
            rot_y,
            rot_z,
            app.camera.zoom,
            app.camera.pan_x,
            app.camera.pan_y,
            m[0][0], m[0][1], m[0][2],
            m[1][0], m[1][1], m[1][2],
            m[2][0], m[2][1], m[2][2],
            app.shell.focused.name(),
            app.active_panel == ActivePanel::Interface,
            app.show_interactions,
            app.show_ligands,
            app.camera.auto_rotate,
            app.compact_edit_spec_string().unwrap_or_default(),
            app.shell.focused.name(),
            app.shell.mode.name(),
        );
        use std::io::Write;
        match std::fs::File::create(state_path) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(state_json.as_bytes()) {
                    eprintln!("Warning: failed to write state file: {}", e);
                }
            }
            Err(e) => eprintln!("Warning: failed to create state file '{}': {}", state_path, e),
        }
    }

    Ok(())
}
