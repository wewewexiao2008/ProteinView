mod app;
mod bridge;
mod edit_history;
mod event;
mod model;
mod parser;
mod render;
mod ui;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{KeyCode, MouseButton, MouseEvent, MouseEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use std::io;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use app::{ActivePanel, App, AppConfig, ConnectionType, LayoutMode, RenderMode, VizMode};

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
}

/// Handle key events when in EditSpec panel view mode (not editing).
/// Returns true if the key was consumed.
fn handle_editspec_view_key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
) -> bool {
    match key.code {
        KeyCode::Enter => {
            app.edit_state.delete_confirm = false;
            app.edit_region_start();
            true
        }
        KeyCode::Char('a') => {
            app.edit_state.delete_confirm = false;
            app.edit_region_add();
            true
        }
        KeyCode::Char('d') => {
            app.edit_region_delete();
            true
        }
        KeyCode::Char('s') => {
            app.edit_state.delete_confirm = false;
            app.edit_region_split();
            true
        }
        // Undo: restore previous state from history.
        KeyCode::Char('u') => {
            app.edit_state.delete_confirm = false;
            app.edit_undo();
            true
        }
        // Redo: restore next state from redo stack.
        KeyCode::Char('r')
            if key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL) =>
        {
            app.edit_state.delete_confirm = false;
            app.edit_redo();
            true
        }
        // j/k navigate regions (only when Regions panel is active and not editing).
        KeyCode::Char('j') | KeyCode::Down => {
            app.edit_state.delete_confirm = false;
            let region_count = app
                .annotation
                .as_ref()
                .and_then(|a| a.editspec_regions.as_ref())
                .map(|r| r.len())
                .unwrap_or(0);
            if region_count > 0 {
                app.focused_region =
                    (app.focused_region + 1).min(region_count - 1);
            }
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.edit_state.delete_confirm = false;
            if app.focused_region > 0 {
                app.focused_region -= 1;
            }
            true
        }
        _ => {
            // Any unhandled key cancels the delete confirmation.
            if app.edit_state.delete_confirm {
                app.edit_state.delete_confirm = false;
            }
            false
        }
    }
}

/// Handle key events when in Regions edit mode.
fn handle_edit_mode_key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
) {
    use app::EditField;

    match key.code {
        // Tab / Shift+Tab: move between fields.
        KeyCode::Tab => {
            if key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT) {
                app.edit_prev_field();
            } else if app.edit_state.cursor_field == EditField::Label {
                // In label field, Tab cycles predefined labels.
                app.edit_cycle_label();
            } else {
                app.edit_next_field();
            }
        }
        // j/k: move between fields (same as Tab/Shift+Tab in edit mode).
        KeyCode::Char('j') => app.edit_next_field(),
        KeyCode::Char('k') => app.edit_prev_field(),

        // + / = / - : adjust values or cycle options.
        KeyCode::Char('+') | KeyCode::Char('=') => {
            match app.edit_state.cursor_field {
                EditField::Action => app.edit_cycle_action(true),
                EditField::Chain => app.edit_cycle_chain(true),
                EditField::RangeStart => app.edit_adjust_range(EditField::RangeStart, 1),
                EditField::RangeEnd => app.edit_adjust_range(EditField::RangeEnd, 1),
                _ => {}
            }
        }
        KeyCode::Char('-') => {
            match app.edit_state.cursor_field {
                EditField::Action => app.edit_cycle_action(false),
                EditField::Chain => app.edit_cycle_chain(false),
                EditField::RangeStart => app.edit_adjust_range(EditField::RangeStart, -1),
                EditField::RangeEnd => app.edit_adjust_range(EditField::RangeEnd, -1),
                _ => {}
            }
        }

        // h / l: cycle actions (same as +/- for action/chain fields).
        KeyCode::Char('l') => {
            match app.edit_state.cursor_field {
                EditField::Action => app.edit_cycle_action(true),
                EditField::Chain => app.edit_cycle_chain(true),
                EditField::RangeStart => app.edit_adjust_range(EditField::RangeStart, 1),
                EditField::RangeEnd => app.edit_adjust_range(EditField::RangeEnd, 1),
                _ => {}
            }
        }
        KeyCode::Char('h') => {
            match app.edit_state.cursor_field {
                EditField::Action => app.edit_cycle_action(false),
                EditField::Chain => app.edit_cycle_chain(false),
                EditField::RangeStart => app.edit_adjust_range(EditField::RangeStart, -1),
                EditField::RangeEnd => app.edit_adjust_range(EditField::RangeEnd, -1),
                _ => {}
            }
        }

        // Enter: save.
        KeyCode::Enter => {
            app.edit_save();
        }

        // Escape: cancel.
        KeyCode::Esc => {
            app.edit_cancel();
        }

        // Backspace: delete last char in label field.
        KeyCode::Backspace => {
            app.edit_label_backspace();
        }

        // Direct character input for label field.
        KeyCode::Char(ch) => {
            app.edit_label_input(ch);
        }

        _ => {}
    }
}

/// Handle sequence-related keys in EditSpec panel (yank, scroll).
/// Returns true if the key was consumed.
fn handle_editspec_sequence_key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
) -> bool {
    match key.code {
        // y: yank selected range (e.g. "A:51-80")
        KeyCode::Char('y') => {
            if let Some((s, e)) = app.seq_selection.range() {
                let chain = app.protein.chains.get(app.current_chain);
                if let Some(c) = chain {
                    if s < c.residues.len() && e < c.residues.len() {
                        let seq_start = c.residues[s].seq_num;
                        let seq_end = c.residues[e].seq_num;
                        let text = format!("{}:{}-{}", c.id, seq_start, seq_end);
                        ui::editspec_panel::yank_to_clipboard(&text);
                    }
                }
            }
            true
        }
        // Y (shift): yank selected sequence letters
        KeyCode::Char('Y') => {
            if let Some((s, e)) = app.seq_selection.range() {
                let chain = app.protein.chains.get(app.current_chain);
                if let Some(c) = chain {
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
            true
        }
        // Escape: clear selection
        KeyCode::Esc => {
            app.seq_selection.clear();
            true
        }
        _ => false,
    }
}

/// Handle mouse events for sidebar interaction.
fn handle_mouse_event(app: &mut App, me: MouseEvent, logfile: &mut Option<std::fs::File>) {
    log!(
        logfile,
        "mouse: kind={:?} row={} col={}",
        me.kind,
        me.row,
        me.column
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
            // Check if click is inside the sidebar
            if let Some(sidebar_rect) = app.last_sidebar_rect {
                if me.column >= sidebar_rect.x
                    && me.column < sidebar_rect.x + sidebar_rect.width
                    && me.row >= sidebar_rect.y
                    && me.row < sidebar_rect.y + sidebar_rect.height
                {
                    // Clicked inside sidebar — determine which item was clicked
                    handle_sidebar_click(app, me.row, me.column, sidebar_rect, logfile);
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            // Handle mouse drag for sequence panel selection (within EditSpec)
            if app.active_panel == ActivePanel::EditSpec && app.seq_selection.dragging {
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
    _col: u16,
    sidebar_rect: Rect,
    logfile: &mut Option<std::fs::File>,
) {
    // Convert absolute row to panel-relative row, then add scroll offset
    // to account for the Paragraph's .scroll() displacement
    let item_row = row.saturating_sub(sidebar_rect.y).saturating_add(app.panel_scroll);

    match app.active_panel {
        ActivePanel::EditSpec => {
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

                    // In EditSpec edit mode, intercept keys for field editing.
                    if app.edit_state.editing && app.active_panel == ActivePanel::EditSpec {
                        handle_edit_mode_key(&mut app, key);
                        continue;
                    }

                    // In EditSpec panel view mode, handle edit operations.
                    if app.active_panel == ActivePanel::EditSpec {
                        if handle_editspec_view_key(&mut app, key) {
                            continue;
                        }
                        // Sequence-specific keys within EditSpec panel (yank, scroll).
                        if handle_editspec_sequence_key(&mut app, key) {
                            continue;
                        }
                    }

                    match key.code {
                        KeyCode::Char('q') => app.should_quit = true,
                        KeyCode::Char('c')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            app.should_quit = true
                        }
                        KeyCode::Char('h') | KeyCode::Left => app.camera.rotate_y(-1.0),
                        KeyCode::Char('l') | KeyCode::Right => app.camera.rotate_y(1.0),
                        KeyCode::Char('j') | KeyCode::Down => app.camera.rotate_x(1.0),
                        KeyCode::Char('k') | KeyCode::Up => app.camera.rotate_x(-1.0),
                        KeyCode::Char('u') => app.camera.rotate_z(-1.0),
                        KeyCode::Char('i') => app.camera.rotate_z(1.0),
                        KeyCode::Char('+') | KeyCode::Char('=') => app.camera.zoom_in(),
                        KeyCode::Char('-') => app.camera.zoom_out(),
                        KeyCode::Char('w') => app.camera.pan(0.0, 1.0),
                        KeyCode::Char('s') => app.camera.pan(0.0, -1.0),
                        KeyCode::Char('a') => app.camera.pan(-1.0, 0.0),
                        KeyCode::Char('d') => app.camera.pan(1.0, 0.0),
                        KeyCode::Char('r') => {
                            let (cols, rows) =
                                crossterm::terminal::size().unwrap_or((term_cols, term_rows));
                            app.camera.reset();
                            app.recalculate_zoom(cols, rows);
                        }
                        KeyCode::Char('c') => app.cycle_color(),
                        KeyCode::Char('v') => app.cycle_viz_mode(),
                        KeyCode::Char('m') => {
                            let (cols, rows) =
                                crossterm::terminal::size().unwrap_or((term_cols, term_rows));
                            app.toggle_hd(cols, rows);
                        }
                        KeyCode::Char('M') => {
                            let (cols, rows) =
                                crossterm::terminal::size().unwrap_or((term_cols, term_rows));
                            app.toggle_fullhd(cols, rows);
                        }
                        KeyCode::Char('[') => app.prev_chain(),
                        KeyCode::Char(']') => app.next_chain(),
                        KeyCode::Char(' ') => app.camera.auto_rotate = !app.camera.auto_rotate,
                        KeyCode::Tab => {
                            if key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT) {
                                app.cycle_panel_prev();
                            } else {
                                app.cycle_panel_next();
                            }
                        }
                        KeyCode::Char('f') => app.close_panel(),
                        KeyCode::Char('F') => app.toggle_interface(),
                        KeyCode::Char('I') => app.toggle_interactions(),
                        KeyCode::Char('g') => app.toggle_ligands(),
                        KeyCode::Char('?') => app.show_help = !app.show_help,
                        KeyCode::Esc => {
                            if app.show_help {
                                app.show_help = false;
                            }
                        }
                        _ => {}
                    }
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
            // Determine panel area and main area based on active panel and layout mode
            let main_area = if app.active_panel != ActivePanel::None {
                match app.layout_mode {
                    LayoutMode::Horizontal => {
                        let sidebar_w = app.active_panel.width();
                        let horiz = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([
                                Constraint::Length(sidebar_w),
                                Constraint::Min(20),
                            ])
                            .split(frame.area());

                        // Store sidebar rect for mouse hit-testing
                        app.last_sidebar_rect = Some(horiz[0]);

                        let sidebar_area = horiz[0];
                        match app.active_panel {
                            ActivePanel::Interface => {
                                let summary = app.interface_analysis.summary(&app.protein);
                                let chain_names = app.chain_names();
                                let interaction_counts = app.interface_analysis.interaction_counts();
                                ui::interface_panel::render_interface_panel(
                                    frame,
                                    sidebar_area,
                                    &summary,
                                    app.current_chain,
                                    &chain_names,
                                    app.show_interactions,
                                    interaction_counts,
                                );
                            }
                            ActivePanel::EditSpec => {
                                ui::editspec_panel::render_editspec_panel(
                                    frame,
                                    sidebar_area,
                                    &mut app,
                                );
                            }
                            ActivePanel::Iteration => {
                                ui::iteration_panel::render_iteration_panel(
                                    frame,
                                    sidebar_area,
                                    &app,
                                );
                            }
                            ActivePanel::None => unreachable!(),
                        }
                        horiz[1]
                    }
                    LayoutMode::Vertical => {
                        let panel_h = app.active_panel.height();
                        let vert = Layout::default()
                            .direction(Direction::Vertical)
                            .constraints([
                                Constraint::Min(10),
                                Constraint::Length(panel_h),
                            ])
                            .split(frame.area());

                        // Store panel rect for mouse hit-testing
                        app.last_sidebar_rect = Some(vert[1]);

                        let panel_area = vert[1];
                        match app.active_panel {
                            ActivePanel::Interface => {
                                let summary = app.interface_analysis.summary(&app.protein);
                                let chain_names = app.chain_names();
                                let interaction_counts = app.interface_analysis.interaction_counts();
                                ui::interface_panel::render_interface_panel(
                                    frame,
                                    panel_area,
                                    &summary,
                                    app.current_chain,
                                    &chain_names,
                                    app.show_interactions,
                                    interaction_counts,
                                );
                            }
                            ActivePanel::EditSpec => {
                                ui::editspec_panel::render_editspec_panel(
                                    frame,
                                    panel_area,
                                    &mut app,
                                );
                            }
                            ActivePanel::Iteration => {
                                ui::iteration_panel::render_iteration_panel(
                                    frame,
                                    panel_area,
                                    &app,
                                );
                            }
                            ActivePanel::None => unreachable!(),
                        }
                        vert[0]
                    }
                }
            } else {
                app.last_sidebar_rect = None;
                frame.area()
            };

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Header
                    Constraint::Min(3),    // Viewport
                    Constraint::Length(2), // Status bar
                    Constraint::Length(1), // Help bar
                ])
                .split(main_area);

            ui::header::render_header(frame, chunks[0], &app.protein.name, app.python_available);
            ui::viewport::render_viewport(frame, chunks[1], &app);
            ui::statusbar::render_statusbar(frame, chunks[2], &app);
            ui::helpbar::render_helpbar(frame, chunks[3], &app);

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
            "{{\n  \"focused_chain\": \"{}\",\n  \"viz_mode\": \"{}\",\n  \"color_scheme\": \"{}\",\n  \"render_mode\": \"{}\",\n  \"camera\": {{ \"rot_x\": {:.6}, \"rot_y\": {:.6}, \"rot_z\": {:.6}, \"zoom\": {:.6}, \"pan_x\": {:.6}, \"pan_y\": {:.6} }},\n  \"rotation_matrix\": [[{:.15e}, {:.15e}, {:.15e}], [{:.15e}, {:.15e}, {:.15e}], [{:.15e}, {:.15e}, {:.15e}]],\n  \"active_panel\": \"{}\",\n  \"interface_active\": {},\n  \"show_interactions\": {},\n  \"show_ligands\": {},\n  \"auto_rotate\": {}\n}}\n",
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
            app.active_panel.name(),
            app.active_panel == ActivePanel::Interface,
            app.show_interactions,
            app.show_ligands,
            app.camera.auto_rotate,
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
