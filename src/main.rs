mod app;
mod bridge;
mod edit_history;
mod event;
mod model;
mod parser;
mod render;
mod shell;
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

use app::{ActivePanel, App, AppConfig, ConnectionType, LayoutMode, RenderMode, VizMode};
use shell::{InteractionMode, KeyAction, PaneId, route_key};

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
        KeyAction::CyclePaneNext => app.shell.cycle_focus_next(),
        KeyAction::CyclePanePrev => app.shell.cycle_focus_prev(),
        KeyAction::ToggleCollapse => app.shell.toggle_collapse(),
        KeyAction::EnterSelect => {
            app.enter_select_mode();
            app.sync_selection_overlay();
        }
        KeyAction::EnterRun => app.enter_run_mode(),
        KeyAction::OpenEmptyForm => app.edit_region_open_empty(),
        KeyAction::EditFocusedRegion => app.edit_region_start(),
        KeyAction::RestorePreviousMode => {
            app.shell.restore_previous_mode();
        }
        KeyAction::ClearSelection => {
            app.seq_selection.clear();
            app.sync_selection_overlay();
        }
        KeyAction::CloseHelp => app.show_help = false,
        KeyAction::ToggleHelp => app.show_help = !app.show_help,
        KeyAction::RotateX(d) => app.camera.rotate_x(d),
        KeyAction::RotateY(d) => app.camera.rotate_y(d),
        KeyAction::RotateZ(d) => app.camera.rotate_z(d),
        KeyAction::Pan(x, y) => app.camera.pan(x, y),
        KeyAction::ZoomIn => app.camera.zoom_in(),
        KeyAction::ZoomOut => app.camera.zoom_out(),
        KeyAction::ResetCamera => {
            let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
            app.camera.reset();
            app.recalculate_zoom(cols, rows);
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
                app.focused_region = (app.focused_region + 1).min(region_count - 1);
            }
        }
        KeyAction::RegionPrev => {
            if app.focused_region > 0 {
                app.focused_region -= 1;
            }
        }
        KeyAction::RegionAdd => app.edit_region_add(),
        KeyAction::RegionDelete => {
            app.edit_region_delete();
        }
        KeyAction::RegionSplit => app.edit_region_split(),
        KeyAction::Undo => app.edit_undo(),
        KeyAction::Redo => app.edit_redo(),
        KeyAction::SeqCursor(delta) => {
            let residues = app.current_residues().to_vec();
            if !residues.is_empty() {
                app.seq_selection.move_cursor(&residues, delta);
                let cursor = app.seq_selection.cursor;
                if delta < 0 && cursor < app.seq_h_scroll as usize {
                    app.seq_h_scroll = cursor as u16;
                }
                if delta > 0 {
                    if let Some(sidebar) = app.last_sidebar_rect {
                        let visible = sidebar.width.saturating_sub(2) as usize;
                        if cursor >= app.seq_h_scroll as usize + visible {
                            app.seq_h_scroll =
                                cursor.saturating_sub(visible.saturating_sub(1)) as u16;
                        }
                    }
                }
                app.sync_selection_overlay();
            }
        }
        KeyAction::SeqExpandStart(delta) => {
            let residues = app.current_residues().to_vec();
            if !residues.is_empty() {
                app.seq_selection.expand_start(&residues, delta);
                app.sync_selection_overlay();
            }
        }
        KeyAction::SeqExpandEnd(delta) => {
            let residues = app.current_residues().to_vec();
            if !residues.is_empty() {
                app.seq_selection.expand_end(&residues, delta);
                app.sync_selection_overlay();
            }
        }
        KeyAction::SeqSelectSegment => {
            let residues = app.current_residues().to_vec();
            if !residues.is_empty() {
                let cursor = app.seq_selection.cursor;
                app.seq_selection.select_segment(&residues, cursor);
                app.sync_selection_overlay();
            }
        }
        KeyAction::SeqJumpSegment(dir) => {
            let residues = app.current_residues().to_vec();
            if !residues.is_empty() {
                app.seq_selection.jump_segment(&residues, dir);
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
        KeyAction::RunIgnore | KeyAction::Ignore => {}
    }
}

/// Handle mouse events for sidebar interaction.
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
        MouseEventKind::ScrollUp => {
            if app.active_panel != ActivePanel::None {
                app.panel_scroll = app.panel_scroll.saturating_sub(1);
            }
        }
        MouseEventKind::ScrollDown => {
            if app.active_panel != ActivePanel::None {
                let max_scroll = max_panel_scroll(app);
                app.panel_scroll = app.panel_scroll.saturating_add(1).min(max_scroll);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(pane) = ui::chrome::pane_at(
                &ui::chrome::ChromeRects {
                    workflow: app.last_workflow_rect.unwrap_or_default(),
                    tree: app.last_tree_rect.unwrap_or_default(),
                    view: app.last_view_rect.unwrap_or_default(),
                    editspec: app.last_sidebar_rect.unwrap_or_default(),
                },
                me.column,
                me.row,
            ) {
                app.shell.focus(pane);
            }
            if let Some(sidebar_rect) = app.last_sidebar_rect {
                if me.column >= sidebar_rect.x
                    && me.column < sidebar_rect.x + sidebar_rect.width
                    && me.row >= sidebar_rect.y
                    && me.row < sidebar_rect.y + sidebar_rect.height
                {
                    handle_sidebar_click(app, me.row, me.column, sidebar_rect, logfile);
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.seq_selection.dragging {
                if let Some(sidebar_rect) = app.last_sidebar_rect {
                    if me.column >= sidebar_rect.x
                        && me.column < sidebar_rect.x + sidebar_rect.width
                    {
                        let col_offset = me.column.saturating_sub(sidebar_rect.x) as usize;
                        let residue_idx = app.seq_h_scroll as usize + col_offset;
                        let chain = app.protein.chains.get(app.current_chain);
                        let max_res = chain.map(|c| c.residues.len()).unwrap_or(0);
                        if residue_idx < max_res {
                            app.seq_selection.end = Some(residue_idx);
                            app.seq_selection.active = true;
                            app.enter_select_mode();
                            app.sync_selection_overlay();
                        }
                    }
                }
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            // End drag
            if app.seq_selection.dragging {
                app.seq_selection.dragging = false;
            }
        }
        _ => {}
    }
}

/// Calculate the maximum scroll offset for the current panel.
/// Prevents scrolling past the last visible item.
fn max_panel_scroll(app: &App) -> u16 {
    if app.panel_item_count == 0 || app.last_sidebar_rect.is_none() {
        return 0;
    }
    let sidebar_height = app
        .last_sidebar_rect
        .map(|r| r.height)
        .unwrap_or(0);
    let total_content = app.panel_click_header + app.panel_item_count as u16;
    total_content.saturating_sub(sidebar_height)
}

/// Handle a click inside the sidebar area.
fn handle_sidebar_click(
    app: &mut App,
    row: u16,
    col: u16,
    sidebar_rect: Rect,
    logfile: &mut Option<std::fs::File>,
) {
    // Convert absolute row to panel-relative row, then add scroll offset
    // to account for the Paragraph's .scroll() displacement
    let item_row = row.saturating_sub(sidebar_rect.y).saturating_add(app.panel_scroll);

    match app.shell.focused {
        PaneId::EditSpec | PaneId::View => {
            // Check if click is in the sequence area (on or after the sequence line)
            let seq_row = app.seq_line_row;
            if seq_row > 0 && item_row == seq_row {
                // Click on the sequence line: select the residue at this column.
                let col_offset = col.saturating_sub(sidebar_rect.x) as usize;
                let residue_idx = app.seq_h_scroll as usize + col_offset;
                let chain = app.protein.chains.get(app.current_chain);
                if let Some(c) = chain {
                    if residue_idx < c.residues.len() {
                        app.seq_selection.click(&c.residues, residue_idx);
                        app.enter_select_mode();
                        app.sync_selection_overlay();
                        log!(
                            logfile,
                            "seq_click: residue_idx={} segment=({:?})",
                            residue_idx,
                            app.seq_selection.range()
                        );
                    }
                }
                return;
            }

            // Otherwise, check region list clicks
            let header = app.panel_click_header;
            if item_row >= header && app.panel_item_count > 0 {
                let region_idx = (item_row - header) as usize;
                if region_idx < app.panel_item_count {
                    app.focused_region = region_idx;
                    log!(
                        logfile,
                        "sidebar_click: panel=EditSpec region_idx={}",
                        region_idx
                    );
                }
            }
        }
        _ => {}
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
                        app.show_help,
                        app.seq_selection.active,
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
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(8),
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

            ui::statusbar::render_statusbar(frame, outer[2], &app);
            ui::helpbar::render_helpbar(frame, outer[3], &app);

            if app.shell.mode == InteractionMode::Run {
                ui::run_overlay::render_run_overlay(frame, frame.area());
            }
            if app.show_help {
                ui::help_overlay::render_help_overlay(frame, frame.area());
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
