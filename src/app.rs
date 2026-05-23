use std::sync::mpsc;

use ratatui::style::Color;
use ratatui_image::picker::Picker;

use crate::bridge::GemlibBridge;
use crate::edit_history::{
    EditHistory, HistoryEntry, ValidationIssue, validate_regions,
};
use crate::model::interface::{InterfaceAnalysis, analyze_binding_pockets, analyze_interface};
use crate::model::protein::Protein;
use crate::render::camera::Camera;
use crate::render::color::{ColorScheme, ColorSchemeType};
use crate::render::ribbon::{RibbonTriangle, generate_ribbon_mesh};

/// Structures with more residues than this threshold trigger performance
/// optimizations (background interface analysis, backbone default, reduced LOD).
pub const LARGE_STRUCTURE_THRESHOLD: usize = 5000;

/// Layout orientation based on terminal aspect ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Wide terminal (aspect > 1.5): sidebar left + main right.
    Horizontal,
    /// Narrow/tall terminal (aspect <= 1.5): main top + panel bottom.
    Vertical,
}

impl LayoutMode {
    /// Compute layout mode from terminal dimensions.
    pub fn from_size(cols: u16, rows: u16) -> Self {
        if cols as f32 / rows as f32 > 1.5 {
            LayoutMode::Horizontal
        } else {
            LayoutMode::Vertical
        }
    }
}

/// Which sidebar panel is active (if any).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePanel {
    None,
    Interface,
    EditSpec,
    Iteration,
}

impl ActivePanel {
    /// All panel variants in tab-cycle order (excluding None).
    const PANELS: [ActivePanel; 3] = [
        ActivePanel::Interface,
        ActivePanel::EditSpec,
        ActivePanel::Iteration,
    ];

    /// Advance to the next panel in the cycle.
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::PANELS[0],
            _ => {
                let idx = Self::PANELS.iter().position(|&p| p == self).unwrap_or(0);
                let next_idx = (idx + 1) % Self::PANELS.len();
                Self::PANELS[next_idx]
            }
        }
    }

    /// Go back to the previous panel in the cycle.
    pub fn prev(self) -> Self {
        match self {
            Self::None => Self::PANELS[Self::PANELS.len() - 1],
            _ => {
                let idx = Self::PANELS.iter().position(|&p| p == self).unwrap_or(0);
                let prev_idx = (idx + Self::PANELS.len() - 1) % Self::PANELS.len();
                Self::PANELS[prev_idx]
            }
        }
    }

    /// Sidebar width in columns for this panel.
    pub fn width(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Interface => crate::ui::interface_panel::SIDEBAR_WIDTH,
            Self::EditSpec => 60,
            Self::Iteration => 34,
        }
    }

    /// Panel height in rows for vertical (bottom) layout mode.
    pub fn height(self) -> u16 {
        match self {
            Self::None => 0,
            Self::EditSpec => 16,
            _ => 10,
        }
    }

    /// Human-readable panel name.
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Interface => "Interface",
            Self::EditSpec => "EditSpec",
            Self::Iteration => "Iteration",
        }
    }
}

/// Which field in the edit form the cursor is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditField {
    Chain,
    RangeStart,
    RangeEnd,
    Action,
    Label,
}

impl EditField {
    /// All fields in tab order.
    const FIELDS: [EditField; 5] = [
        EditField::Chain,
        EditField::RangeStart,
        EditField::RangeEnd,
        EditField::Action,
        EditField::Label,
    ];

    /// Advance to the next field in the cycle.
    pub fn next(self) -> Self {
        let idx = Self::FIELDS.iter().position(|&f| f == self).unwrap_or(0);
        Self::FIELDS[(idx + 1) % Self::FIELDS.len()]
    }

    /// Go back to the previous field in the cycle.
    pub fn prev(self) -> Self {
        let idx = Self::FIELDS.iter().position(|&f| f == self).unwrap_or(0);
        Self::FIELDS[(idx + Self::FIELDS.len() - 1) % Self::FIELDS.len()]
    }
}

/// State for sequence selection in the Sequence panel.
#[derive(Debug, Clone, Default)]
pub struct SeqSelection {
    /// Start residue index of the selection (inclusive).
    pub start: Option<usize>,
    /// End residue index of the selection (inclusive).
    pub end: Option<usize>,
    /// Whether a mouse drag is in progress.
    pub dragging: bool,
}

impl SeqSelection {
    /// Return the inclusive range of selected residues, sorted.
    pub fn range(&self) -> Option<(usize, usize)> {
        match (self.start, self.end) {
            (Some(s), Some(e)) => Some((s.min(e), s.max(e))),
            (Some(s), None) => Some((s, s)),
            _ => None,
        }
    }

    /// Check if a residue index is within the selection.
    pub fn contains(&self, idx: usize) -> bool {
        if let Some((s, e)) = self.range() {
            idx >= s && idx <= e
        } else {
            false
        }
    }

    /// Clear the selection.
    pub fn clear(&mut self) {
        self.start = None;
        self.end = None;
        self.dragging = false;
    }
}

/// State for the inline region editor.
#[derive(Debug, Clone)]
pub struct EditState {
    /// True when the editor is active.
    pub editing: bool,
    /// Which region is being edited (None = adding new region).
    pub editing_region_idx: Option<usize>,
    /// Which field the cursor is on.
    pub cursor_field: EditField,
    /// Draft chain letter.
    pub draft_chain: String,
    /// Draft range start.
    pub draft_range_start: usize,
    /// Draft range end.
    pub draft_range_end: usize,
    /// Draft action (keep/edit/replace/insert/delete).
    pub draft_action: String,
    /// Draft label text.
    pub draft_label: String,
    /// Validation error message to display below the edited region.
    pub validation_error: Option<String>,
    /// Confirmation state for delete: true after first 'd', awaiting second 'd'.
    pub delete_confirm: bool,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            editing: false,
            editing_region_idx: None,
            cursor_field: EditField::Chain,
            draft_chain: "A".to_string(),
            draft_range_start: 1,
            draft_range_end: 10,
            draft_action: "edit".to_string(),
            draft_label: String::new(),
            validation_error: None,
            delete_confirm: false,
        }
    }
}

/// Predefined label names for the label/tag system.
pub const PREDEFINED_LABELS: &[&str] = &[
    "receptor",
    "gem",
    "solmate",
    "linker",
    "binding",
    "loop",
    "core",
    "helix",
    "beta",
    "repaired",
    "variant",
    "active",
    "interface",
];

/// All valid EditSpec action names (canonical long form).
pub const VALID_ACTIONS: &[&str] = &["keep", "edit", "replace", "insert", "delete"];

/// Return the display color for a label string.
pub fn label_color(label: &str) -> Color {
    match label {
        "receptor" => Color::Rgb(100, 149, 237),
        "gem" => Color::Rgb(0, 206, 209),
        "solmate" => Color::Rgb(147, 112, 219),
        "linker" => Color::Rgb(255, 165, 0),
        "binding" => Color::Rgb(255, 215, 0),
        "loop" => Color::Rgb(144, 238, 144),
        "core" => Color::Rgb(210, 105, 30),
        "helix" => Color::Rgb(70, 130, 180),
        "beta" => Color::Rgb(186, 85, 211),
        "repaired" => Color::Rgb(50, 205, 50),
        "variant" => Color::Rgb(255, 99, 71),
        "active" => Color::Rgb(0, 191, 255),
        "interface" => Color::Rgb(255, 105, 180),
        _ => Color::White,
    }
}

/// Annotation data loaded from a JSON file passed via `--annotation`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Annotation {
    #[serde(default)]
    pub editspec_regions: Option<Vec<EditSpecRegion>>,
    #[serde(default)]
    pub iteration: Option<IterationInfo>,
    #[serde(default)]
    pub highlights: Option<HighlightInfo>,
}

/// A single EditSpec region in the annotation.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EditSpecRegion {
    pub chain: String,
    pub range: [usize; 2],
    pub action: String,
    #[serde(default)]
    pub label: Option<String>,
}

/// Iteration progress info in the annotation.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct IterationInfo {
    pub current: u32,
    pub total: u32,
    #[serde(default)]
    pub best_sc_tm: Option<f64>,
    #[serde(default)]
    pub best_plddt: Option<f64>,
    #[serde(default)]
    pub candidates: Option<u32>,
    #[serde(default)]
    pub high_quality: Option<u32>,
}

/// Highlight residues in the annotation.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct HighlightInfo {
    pub chain: String,
    #[serde(default)]
    pub residues: Vec<usize>,
    #[serde(default)]
    pub highlight_type: Option<String>,
}

/// Visualization mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VizMode {
    Backbone,
    Cartoon,
    Wireframe,
}

impl VizMode {
    pub fn next(&self) -> Self {
        match self {
            Self::Backbone => Self::Cartoon,
            Self::Cartoon => Self::Wireframe,
            Self::Wireframe => Self::Backbone,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Backbone => "Backbone",
            Self::Cartoon => "Cartoon",
            Self::Wireframe => "Wireframe",
        }
    }
}

/// Rendering mode for the 3D viewport
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderMode {
    /// Braille dots - highest text-mode spatial resolution, monochrome per cell
    Braille,
    /// HD-quality colored braille via software rasterizer (Lambert shading,
    /// z-buffer, depth fog).  Fast everywhere including SSH.
    HalfBlock,
    /// Full pixel graphics via Sixel/Kitty/iTerm2 - best quality, high bandwidth
    FullHD,
}

impl RenderMode {
    pub fn name(&self) -> &str {
        match self {
            Self::Braille => "Braille",
            Self::HalfBlock => "HD",
            Self::FullHD => "FullHD",
        }
    }
}

/// Whether the terminal session is local or over SSH.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionType {
    Local,
    Ssh,
}

impl ConnectionType {
    /// Detect whether the current session is running over SSH.
    ///
    /// This checks the `SSH_CLIENT`, `SSH_TTY`, and `SSH_CONNECTION`
    /// environment variables. Note that this can produce false positives
    /// in containers, CI environments, or VS Code Remote sessions where
    /// these variables may be inherited. Users can override the default
    /// render mode with `--fullhd` if detection is wrong.
    pub fn detect() -> Self {
        if std::env::var("SSH_CLIENT").is_ok()
            || std::env::var("SSH_TTY").is_ok()
            || std::env::var("SSH_CONNECTION").is_ok()
        {
            Self::Ssh
        } else {
            Self::Local
        }
    }
}

/// Configuration bundle for [`App::new`], replacing individual parameters
/// to avoid too_many_arguments.
pub struct AppConfig {
    pub render_mode: RenderMode,
    pub viz_mode: VizMode,
    pub user_explicit_mode: bool,
    pub color_override: Option<ColorSchemeType>,
}

/// Main application state
pub struct App {
    pub protein: Protein,
    pub camera: Camera,
    pub color_scheme: ColorScheme,
    pub viz_mode: VizMode,
    pub current_chain: usize,
    pub render_mode: RenderMode,
    pub show_help: bool,
    pub show_ligands: bool,
    /// Which sidebar panel is currently active (replaces the old `show_interface` bool).
    pub active_panel: ActivePanel,
    pub show_interactions: bool,
    pub interface_analysis: InterfaceAnalysis,
    pub should_quit: bool,
    /// Whether the B-factor column likely contains pLDDT confidence scores.
    pub has_plddt: bool,
    /// Cached ribbon mesh — regenerated only when color scheme changes.
    pub mesh_cache: Vec<RibbonTriangle>,
    mesh_dirty: bool,
    /// ratatui-image protocol picker for Sixel/Kitty/iTerm2 graphics.
    pub picker: Picker,
    /// Detected connection type (local vs SSH).
    pub connection_type: ConnectionType,
    /// Temporary warning when user enters FullHD over SSH.
    pub ssh_hd_warning: bool,
    /// Countdown frames to auto-dismiss the SSH HD warning (~90 frames = 3 seconds at 30fps).
    pub ssh_hd_warning_frames: u8,
    /// Set to `true` after a render-mode switch so the main loop can call
    /// `terminal.clear()` before the next draw, forcing ratatui to redraw
    /// every cell and preventing stale content from the previous mode.
    pub needs_clear: bool,
    /// Saved color scheme type to restore when leaving interface mode.
    /// When interface mode is active, we display Interface colors but
    /// preserve the user's chosen scheme so it can be restored on exit.
    saved_color_scheme_type: ColorSchemeType,
    /// Whether interface analysis has been computed. For large structures
    /// (> LARGE_STRUCTURE_THRESHOLD residues), computation starts on a
    /// background thread at startup and completes before the user needs it.
    /// If the user requests interface mode before computation completes,
    /// the toggle is a no-op until the next frame.
    interface_computed: bool,
    /// Receiver for background interface analysis (large structures only).
    interface_rx: Option<mpsc::Receiver<InterfaceAnalysis>>,
    /// Cached result of `total_residues > LARGE_STRUCTURE_THRESHOLD`, set once
    /// in `App::new` to avoid per-frame O(n) `residue_count()` calls.
    pub is_large: bool,
    /// Annotation data loaded from `--annotation` JSON file.
    pub annotation: Option<Annotation>,
    /// Index of the focused region in the Regions panel.
    pub focused_region: usize,
    /// Scroll offset for the active sidebar panel (in lines).
    pub panel_scroll: u16,
    /// Stored sidebar layout rect for mouse hit-testing.
    pub last_sidebar_rect: Option<ratatui::layout::Rect>,
    /// Number of header lines before the first clickable item in the active panel.
    /// Updated during each draw call so mouse click mapping stays accurate.
    pub panel_click_header: u16,
    /// Total number of clickable items in the active panel.
    /// Used to clamp scroll offset and validate click targets.
    pub panel_item_count: usize,
    /// PyO3 bridge to gemlib Python APIs.  `None` when Python is unavailable,
    /// in which case editing features are disabled and the app runs in read-only mode.
    pub bridge: Option<GemlibBridge>,
    /// Whether the Python/gemlib bridge was successfully initialized.
    /// Controls the "Read-only" indicator in the header and status bar.
    pub python_available: bool,
    /// Current layout orientation, computed from terminal aspect ratio.
    pub layout_mode: LayoutMode,
    /// State for the inline region editor in the Regions panel.
    pub edit_state: EditState,
    /// Undo/redo operation history for EditSpec edits.
    pub edit_history: EditHistory,
    /// Cached validation issues, recomputed on every state change.
    pub validation_issues: Vec<ValidationIssue>,
    /// Horizontal scroll offset for the Sequence panel (in characters).
    pub seq_h_scroll: u16,
    /// Selection state for the Sequence panel.
    pub seq_selection: SeqSelection,
}

impl App {
    pub fn new(
        mut protein: Protein,
        config: AppConfig,
        term_cols: u16,
        term_rows: u16,
        picker: Picker,
    ) -> Self {
        let AppConfig {
            render_mode,
            viz_mode,
            user_explicit_mode,
            color_override,
        } = config;
        protein.center();
        // If user explicitly requested pLDDT via CLI, trust that even if
        // the heuristic disagrees.
        let has_plddt = protein.has_plddt() || color_override == Some(ColorSchemeType::Plddt);
        let total_residues = protein.residue_count();
        let radius = protein.bounding_radius().max(1.0);

        let vp_rows = term_rows.saturating_sub(4) as f64;
        let vp_cols = term_cols as f64;
        let (font_w, font_h) = picker.font_size();

        let auto_zoom = match render_mode {
            RenderMode::FullHD => {
                let proto = picker.protocol_type();
                let (px_w, px_h) = if proto != ratatui_image::picker::ProtocolType::Halfblocks
                    && font_w > 0
                    && font_h > 0
                {
                    (vp_cols * font_w as f64, vp_rows * font_h as f64)
                } else {
                    // Fallback to braille-like resolution
                    (vp_cols * 2.0, vp_rows * 4.0)
                };
                0.9 * px_w.min(px_h) / (2.0 * radius)
            }
            RenderMode::HalfBlock => {
                let px_w = vp_cols * 2.0;
                let px_h = vp_rows * 4.0;
                0.9 * px_w.min(px_h) / (2.0 * radius)
            }
            RenderMode::Braille => {
                let px_w = vp_cols * 2.0;
                let px_h = vp_rows * 4.0;
                0.9 * px_w.min(px_h) / (2.0 * radius)
            }
        };
        let mut camera = Camera::default();
        camera.zoom = auto_zoom;

        let is_large = total_residues > LARGE_STRUCTURE_THRESHOLD;

        // For large structures, start interface analysis on a background thread
        // so it's ready by the time the user presses 'f'.
        let interface_rx = if is_large {
            let bg_protein = protein.clone();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut ia = analyze_interface(&bg_protein, 4.5);
                if !bg_protein.ligands.is_empty() {
                    ia.binding_pockets = Some(analyze_binding_pockets(&bg_protein, 4.5));
                }
                let _ = tx.send(ia);
            });
            // Interface analysis is running in the background — it'll be ready
            // by the time the user presses 'f'.
            Some(rx)
        } else {
            None
        };

        let (interface_analysis, interface_computed) = if is_large {
            let empty = InterfaceAnalysis {
                contacts: Vec::new(),
                interface_residues: std::collections::HashSet::new(),
                chain_interface_counts: vec![0; protein.chains.len()],
                total_interface_residues: 0,
                binding_pockets: None,
                interactions: Vec::new(),
            };
            (empty, false)
        } else {
            let mut ia = analyze_interface(&protein, 4.5);
            if !protein.ligands.is_empty() {
                ia.binding_pockets = Some(analyze_binding_pockets(&protein, 4.5));
            }
            (ia, true)
        };

        // For large structures, default to Backbone mode for instant
        // interactivity — but only if the user didn't explicitly choose a mode.
        let viz_mode = if is_large && !user_explicit_mode && viz_mode == VizMode::Cartoon {
            VizMode::Backbone
        } else {
            viz_mode
        };

        let initial_scheme = color_override.unwrap_or(ColorSchemeType::Structure);
        let color_scheme = ColorScheme::new(initial_scheme, total_residues);
        // Only build ribbon mesh eagerly if we're actually in Cartoon mode.
        // For Backbone/Wireframe, defer until the user switches to Cartoon.
        let (mesh_cache, mesh_dirty) = if viz_mode == VizMode::Cartoon {
            (generate_ribbon_mesh(&protein, &color_scheme), false)
        } else {
            (Vec::new(), true)
        };

        let connection_type = ConnectionType::detect();

        // Initialize the Python bridge.  Failure is non-fatal: PV degrades to
        // read-only mode and the header shows a "Read-only" indicator.
        let (bridge, python_available) = match GemlibBridge::new() {
            Ok(b) => {
                eprintln!("Python bridge: initialized (gemlib + contiger available)");
                (Some(b), true)
            }
            Err(e) => {
                eprintln!("Warning: Python bridge unavailable — running in read-only mode.");
                eprintln!("  Reason: {}", e);
                (None, false)
            }
        };

        Self {
            protein,
            camera,
            color_scheme,
            viz_mode,
            current_chain: 0,
            render_mode,
            show_help: false,
            show_ligands: true,
            active_panel: ActivePanel::None,
            show_interactions: false,
            interface_analysis,
            should_quit: false,
            has_plddt,
            mesh_cache,
            mesh_dirty,
            picker,
            connection_type,
            ssh_hd_warning: false,
            ssh_hd_warning_frames: 0,
            needs_clear: false,
            saved_color_scheme_type: initial_scheme,
            interface_computed,
            interface_rx,
            is_large,
            annotation: None,
            focused_region: 0,
            panel_scroll: 0,
            last_sidebar_rect: None,
            panel_click_header: 0,
            panel_item_count: 0,
            bridge,
            python_available,
            layout_mode: LayoutMode::from_size(term_cols, term_rows),
            edit_state: EditState::default(),
            edit_history: EditHistory::default(),
            validation_issues: Vec::new(),
            seq_h_scroll: 0,
            seq_selection: SeqSelection::default(),
        }
    }

    pub fn cycle_color(&mut self) {
        if self.active_panel == ActivePanel::Interface {
            // While interface mode is active, cycle the saved scheme so the
            // user's preference is tracked, but keep displaying Interface colors.
            self.saved_color_scheme_type = self.saved_color_scheme_type.next(self.has_plddt);
        } else {
            let next = self.color_scheme.scheme_type.next(self.has_plddt);
            self.color_scheme = ColorScheme::new(next, self.protein.residue_count());
            self.mesh_dirty = true;
        }
    }

    /// Poll the background interface analysis thread (non-blocking).
    /// Called each frame so results are absorbed as soon as they're ready.
    pub fn poll_background_interface(&mut self) {
        if self.interface_computed {
            return;
        }
        if let Some(rx) = &self.interface_rx {
            match rx.try_recv() {
                Ok(ia) => {
                    self.interface_analysis = ia;
                    self.interface_computed = true;
                    self.interface_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    // Still computing — nothing to do yet.
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Background thread panicked or dropped the sender.
                    // Drop the rx and fall back to synchronous computation.
                    self.interface_rx = None;
                    let mut ia = analyze_interface(&self.protein, 4.5);
                    if !self.protein.ligands.is_empty() {
                        ia.binding_pockets = Some(analyze_binding_pockets(&self.protein, 4.5));
                    }
                    self.interface_analysis = ia;
                    self.interface_computed = true;
                }
            }
        }
    }

    pub fn cycle_viz_mode(&mut self) {
        self.viz_mode = self.viz_mode.next();
    }

    fn rebuild_interface_colors(&mut self) {
        self.color_scheme = ColorScheme::new_interface(
            self.protein.residue_count(),
            self.current_chain,
            &self.interface_analysis,
            &self.protein,
        );
        self.mesh_dirty = true;
    }

    pub fn toggle_interface(&mut self) {
        if self.active_panel == ActivePanel::Interface {
            // Close the interface panel.
            self.active_panel = ActivePanel::None;
            self.show_interactions = false;
            // Restore the user's saved color scheme instead of hardcoding Structure
            self.color_scheme =
                ColorScheme::new(self.saved_color_scheme_type, self.protein.residue_count());
            self.mesh_dirty = true;
        } else {
            // Open the interface panel.
            self.active_panel = ActivePanel::Interface;
            self.panel_scroll = 0;
            // Check if background analysis is ready, otherwise compute synchronously.
            if !self.interface_computed {
                // Determine background thread status without holding a
                // long-lived borrow on self.interface_rx.
                let bg_status = self.interface_rx.as_ref().map(|rx| rx.try_recv());
                match bg_status {
                    Some(Ok(ia)) => {
                        self.interface_analysis = ia;
                        self.interface_computed = true;
                        self.interface_rx = None;
                    }
                    Some(Err(mpsc::TryRecvError::Empty)) => {
                        // Still computing — don't enter interface mode yet.
                        // poll_background_interface() will absorb the result
                        // when ready; the user can press `f` again.
                        self.active_panel = ActivePanel::None;
                        return;
                    }
                    Some(Err(mpsc::TryRecvError::Disconnected)) => {
                        // Thread panicked — drop the rx and fall through to
                        // synchronous computation below.
                        self.interface_rx = None;
                    }
                    None => {
                        // No background thread was spawned.
                    }
                }
                // If we still don't have it (no rx, or disconnected), compute synchronously.
                if !self.interface_computed {
                    let mut ia = analyze_interface(&self.protein, 4.5);
                    if !self.protein.ligands.is_empty() {
                        ia.binding_pockets = Some(analyze_binding_pockets(&self.protein, 4.5));
                    }
                    self.interface_analysis = ia;
                    self.interface_computed = true;
                }
            }
            // Save the user's current color scheme before switching to Interface
            self.saved_color_scheme_type = self.color_scheme.scheme_type;
            self.rebuild_interface_colors();
        }
    }

    pub fn toggle_interactions(&mut self) {
        if self.active_panel == ActivePanel::Interface {
            self.show_interactions = !self.show_interactions;
        }
    }

    pub fn toggle_ligands(&mut self) {
        self.show_ligands = !self.show_ligands;
    }

    /// Cycle to the next sidebar panel (Tab binding).
    pub fn cycle_panel_next(&mut self) {
        let prev = self.active_panel;
        self.active_panel = self.active_panel.next();
        if self.active_panel == ActivePanel::Interface && prev != ActivePanel::Interface {
            // Entering interface — ensure analysis is computed and apply interface colors.
            self.ensure_interface_analysis();
            self.saved_color_scheme_type = self.color_scheme.scheme_type;
            self.rebuild_interface_colors();
        } else if prev == ActivePanel::Interface && self.active_panel != ActivePanel::Interface {
            // Leaving interface — restore saved colors.
            self.show_interactions = false;
            self.color_scheme =
                ColorScheme::new(self.saved_color_scheme_type, self.protein.residue_count());
            self.mesh_dirty = true;
        }
        self.panel_scroll = 0;
    }

    /// Cycle to the previous sidebar panel (Shift+Tab binding).
    pub fn cycle_panel_prev(&mut self) {
        let prev = self.active_panel;
        self.active_panel = self.active_panel.prev();
        if self.active_panel == ActivePanel::Interface && prev != ActivePanel::Interface {
            self.ensure_interface_analysis();
            self.saved_color_scheme_type = self.color_scheme.scheme_type;
            self.rebuild_interface_colors();
        } else if prev == ActivePanel::Interface && self.active_panel != ActivePanel::Interface {
            self.show_interactions = false;
            self.color_scheme =
                ColorScheme::new(self.saved_color_scheme_type, self.protein.residue_count());
            self.mesh_dirty = true;
        }
        self.panel_scroll = 0;
    }

    /// Close the current sidebar panel (f binding).
    pub fn close_panel(&mut self) {
        if self.active_panel == ActivePanel::Interface {
            self.show_interactions = false;
            self.color_scheme =
                ColorScheme::new(self.saved_color_scheme_type, self.protein.residue_count());
            self.mesh_dirty = true;
        }
        self.active_panel = ActivePanel::None;
        self.panel_scroll = 0;
    }

    /// Ensure interface analysis is computed, starting background or sync as needed.
    fn ensure_interface_analysis(&mut self) {
        if self.interface_computed {
            return;
        }
        let bg_status = self.interface_rx.as_ref().map(|rx| rx.try_recv());
        match bg_status {
            Some(Ok(ia)) => {
                self.interface_analysis = ia;
                self.interface_computed = true;
                self.interface_rx = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) => {
                // Still computing — toggle_interface will handle this.
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.interface_rx = None;
                let mut ia = analyze_interface(&self.protein, 4.5);
                if !self.protein.ligands.is_empty() {
                    ia.binding_pockets = Some(analyze_binding_pockets(&self.protein, 4.5));
                }
                self.interface_analysis = ia;
                self.interface_computed = true;
            }
            None => {
                let mut ia = analyze_interface(&self.protein, 4.5);
                if !self.protein.ligands.is_empty() {
                    ia.binding_pockets = Some(analyze_binding_pockets(&self.protein, 4.5));
                }
                self.interface_analysis = ia;
                self.interface_computed = true;
            }
        }
    }

    /// Load an annotation JSON file from disk.
    pub fn load_annotation(&mut self, path: &str) {
        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<Annotation>(&content) {
                Ok(ann) => {
                    self.annotation = Some(ann);
                }
                Err(e) => {
                    eprintln!("Warning: failed to parse annotation '{}': {}", path, e);
                }
            },
            Err(e) => {
                eprintln!("Warning: failed to read annotation '{}': {}", path, e);
            }
        }
    }

    /// Get the cached ribbon mesh, regenerating if dirty.
    pub fn ribbon_mesh(&mut self) -> &[RibbonTriangle] {
        if self.mesh_dirty {
            self.mesh_cache = generate_ribbon_mesh(&self.protein, &self.color_scheme);
            self.mesh_dirty = false;
        }
        &self.mesh_cache
    }

    pub fn next_chain(&mut self) {
        if !self.protein.chains.is_empty() {
            self.current_chain = (self.current_chain + 1) % self.protein.chains.len();
            if self.active_panel == ActivePanel::Interface {
                self.rebuild_interface_colors();
            }
        }
    }

    pub fn prev_chain(&mut self) {
        if !self.protein.chains.is_empty() {
            self.current_chain = if self.current_chain == 0 {
                self.protein.chains.len() - 1
            } else {
                self.current_chain - 1
            };
            if self.active_panel == ActivePanel::Interface {
                self.rebuild_interface_colors();
            }
        }
    }

    pub fn chain_names(&self) -> Vec<String> {
        self.protein.chains.iter().map(|c| c.id.clone()).collect()
    }

    /// Returns `true` when the scene is being actively animated (e.g. auto-rotate).
    /// Used to trigger half-resolution rendering in FullHD mode for smoother
    /// frame rates on large structures.
    pub fn is_interacting(&self) -> bool {
        self.camera.auto_rotate
    }

    pub fn tick(&mut self) {
        self.camera.tick();

        // Tick down SSH HD warning
        if self.ssh_hd_warning && self.ssh_hd_warning_frames > 0 {
            self.ssh_hd_warning_frames -= 1;
            if self.ssh_hd_warning_frames == 0 {
                self.ssh_hd_warning = false;
            }
        }
    }

    /// Mark the ribbon mesh cache as dirty, forcing a rebuild on the next frame.
    /// Called when terminal resize occurs or other events invalidate the mesh.
    pub fn mesh_dirty_flag(&mut self) {
        self.mesh_dirty = true;
    }

    /// Recalculate the zoom factor based on current render mode and terminal size.
    /// Call this after changing `render_mode` so the protein fills the viewport
    /// correctly for the new framebuffer dimensions.
    pub fn recalculate_zoom(&mut self, term_cols: u16, term_rows: u16) {
        // Update layout mode on resize
        self.layout_mode = LayoutMode::from_size(term_cols, term_rows);
        let radius = self.protein.bounding_radius().max(1.0);
        let vp_rows = term_rows.saturating_sub(4) as f64;
        let vp_cols = term_cols as f64;
        let (font_w, font_h) = self.picker.font_size();

        let (px_w, px_h) = match self.render_mode {
            RenderMode::FullHD => {
                let proto = self.picker.protocol_type();
                if proto != ratatui_image::picker::ProtocolType::Halfblocks
                    && font_w > 0
                    && font_h > 0
                {
                    (vp_cols * font_w as f64, vp_rows * font_h as f64)
                } else {
                    (vp_cols * 2.0, vp_rows * 4.0)
                }
            }
            RenderMode::HalfBlock => (vp_cols * 2.0, vp_rows * 4.0),
            RenderMode::Braille => (vp_cols * 2.0, vp_rows * 4.0),
        };
        self.camera.zoom = 0.9 * px_w.min(px_h) / (2.0 * radius);
    }

    /// Cycle lower render tiers: Braille -> HalfBlock -> Braille.
    /// From FullHD, steps down to HalfBlock (next lower tier).
    /// Bound to `m`.
    pub fn toggle_hd(&mut self, term_cols: u16, term_rows: u16) {
        self.render_mode = match self.render_mode {
            RenderMode::Braille => RenderMode::HalfBlock,
            RenderMode::HalfBlock => RenderMode::Braille,
            RenderMode::FullHD => RenderMode::HalfBlock,
        };
        // Dismiss any stale SSH warning (no longer in FullHD)
        self.ssh_hd_warning = false;
        self.ssh_hd_warning_frames = 0;
        self.needs_clear = true;
        self.recalculate_zoom(term_cols, term_rows);
    }

    /// Upgrade to FullHD (Sixel/Kitty) or back to HalfBlock.
    /// Bound to `M` (Shift+M).  Warns when entering FullHD over SSH.
    pub fn toggle_fullhd(&mut self, term_cols: u16, term_rows: u16) {
        self.render_mode = match self.render_mode {
            RenderMode::FullHD => RenderMode::HalfBlock,
            _ => RenderMode::FullHD,
        };

        self.needs_clear = true;

        if self.render_mode == RenderMode::FullHD && self.connection_type == ConnectionType::Ssh {
            self.ssh_hd_warning = true;
            self.ssh_hd_warning_frames = 90;
        } else {
            // Leaving FullHD — dismiss any active SSH warning
            self.ssh_hd_warning = false;
            self.ssh_hd_warning_frames = 0;
        }

        self.recalculate_zoom(term_cols, term_rows);
    }

    // -- Region editing methods -----------------------------------------------

    /// Take a snapshot of the current region list for undo history.
    fn snapshot_regions(&self) -> Vec<EditSpecRegion> {
        self.annotation
            .as_ref()
            .and_then(|a| a.editspec_regions.as_ref())
            .cloned()
            .unwrap_or_default()
    }

    /// Push the current state onto the undo history before an edit operation.
    fn push_history(&mut self, description: &str) {
        self.edit_history.push(HistoryEntry {
            description: description.to_string(),
            snapshot: self.snapshot_regions(),
            focused_region: self.focused_region,
            panel_scroll: self.panel_scroll,
        });
    }

    /// Re-run validation on the current region list and cache the results.
    fn revalidate(&mut self) {
        self.validation_issues = self
            .annotation
            .as_ref()
            .and_then(|a| a.editspec_regions.as_ref())
            .map(|regions| validate_regions(regions))
            .unwrap_or_default();
    }

    /// Undo the last edit operation.  Restores the region list, focus, and scroll
    /// from the history snapshot.
    pub fn edit_undo(&mut self) {
        if let Some(entry) = self.edit_history.undo() {
            if self.annotation.is_none() {
                self.annotation = Some(Annotation {
                    editspec_regions: Some(Vec::new()),
                    iteration: None,
                    highlights: None,
                });
            }
            if let Some(ref mut ann) = self.annotation {
                ann.editspec_regions = Some(entry.snapshot);
            }
            self.focused_region = entry.focused_region;
            self.panel_scroll = entry.panel_scroll;
            self.revalidate();
        }
    }

    /// Redo the last undone edit operation.
    pub fn edit_redo(&mut self) {
        if let Some(entry) = self.edit_history.redo() {
            if self.annotation.is_none() {
                self.annotation = Some(Annotation {
                    editspec_regions: Some(Vec::new()),
                    iteration: None,
                    highlights: None,
                });
            }
            if let Some(ref mut ann) = self.annotation {
                ann.editspec_regions = Some(entry.snapshot);
            }
            self.focused_region = entry.focused_region;
            self.panel_scroll = entry.panel_scroll;
            self.revalidate();
        }
    }

    /// Enter edit mode for an existing region (Enter key on a region).
    pub fn edit_region_start(&mut self) {
        if self.active_panel != ActivePanel::EditSpec || self.edit_state.editing {
            return;
        }
        let regions = match self.annotation.as_ref().and_then(|a| a.editspec_regions.as_ref()) {
            Some(r) => r,
            None => return,
        };
        let idx = self.focused_region.min(regions.len().saturating_sub(1));
        let region = &regions[idx];

        self.edit_state = EditState {
            editing: true,
            editing_region_idx: Some(idx),
            cursor_field: EditField::Chain,
            draft_chain: region.chain.clone(),
            draft_range_start: region.range[0],
            draft_range_end: region.range[1],
            draft_action: region.action.clone(),
            draft_label: region.label.clone().unwrap_or_default(),
            validation_error: None,
            delete_confirm: false,
        };
    }

    /// Start adding a new region (a key in EditSpec panel).
    pub fn edit_region_add(&mut self) {
        if self.active_panel != ActivePanel::EditSpec || self.edit_state.editing {
            return;
        }
        // Default chain is the first protein chain, or "A" if no chains.
        let default_chain = self
            .protein
            .chains
            .first()
            .map(|c| c.id.clone())
            .unwrap_or_else(|| "A".to_string());

        self.edit_state = EditState {
            editing: true,
            editing_region_idx: None,
            cursor_field: EditField::Chain,
            draft_chain: default_chain,
            draft_range_start: 1,
            draft_range_end: 10,
            draft_action: "edit".to_string(),
            draft_label: String::new(),
            validation_error: None,
            delete_confirm: false,
        };
    }

    /// Delete the focused region (dd -- double d confirmation).
    /// Returns true if the delete was executed (second 'd').
    pub fn edit_region_delete(&mut self) -> bool {
        if self.active_panel != ActivePanel::EditSpec || self.edit_state.editing {
            return false;
        }

        if !self.edit_state.delete_confirm {
            // First 'd' — enter confirmation state.
            self.edit_state.delete_confirm = true;
            return false;
        }

        // Second 'd' — execute the delete.
        self.edit_state.delete_confirm = false;

        // Take snapshot and push history before mutation.
        let snapshot = self.snapshot_regions();
        let idx = self.focused_region.min(snapshot.len().saturating_sub(1));
        if snapshot.is_empty() {
            return false;
        }
        self.edit_history.push(HistoryEntry {
            description: format!("delete region {}", idx),
            snapshot,
            focused_region: self.focused_region,
            panel_scroll: self.panel_scroll,
        });

        if let Some(ref mut ann) = self.annotation {
            if let Some(ref mut regions) = ann.editspec_regions {
                if !regions.is_empty() {
                    regions.remove(idx);
                    // Clamp focused_region to valid range.
                    if self.focused_region >= regions.len() && !regions.is_empty() {
                        self.focused_region = regions.len() - 1;
                    }
                    self.revalidate();
                    return true;
                }
            }
        }
        false
    }

    /// Split the focused region at its midpoint (s key in EditSpec panel).
    pub fn edit_region_split(&mut self) {
        if self.active_panel != ActivePanel::EditSpec || self.edit_state.editing {
            return;
        }

        // Collect split parameters from immutable borrow first.
        let split_info = {
            match self.annotation.as_ref().and_then(|a| a.editspec_regions.as_ref()) {
                Some(regions) if !regions.is_empty() => {
                    let idx = self.focused_region.min(regions.len().saturating_sub(1));
                    let region = &regions[idx];
                    let start = region.range[0];
                    let end = region.range[1];
                    if end <= start + 1 {
                        None // Too small to split.
                    } else {
                        Some((idx, start, end, region.chain.clone(), region.action.clone(), region.label.clone()))
                    }
                }
                _ => None,
            }
        };

        let Some((idx, start, end, chain, action, label)) = split_info else {
            return;
        };

        let mid = start + (end - start) / 2;

        // Push history before mutation.
        self.push_history(&format!("split region {}", idx));

        if let Some(ref mut ann) = self.annotation {
            if let Some(ref mut regions) = ann.editspec_regions {
                // Modify the original region to be the first half.
                regions[idx].range[1] = mid;

                // Insert the second half as a new region right after.
                let new_region = EditSpecRegion {
                    chain,
                    range: [mid + 1, end],
                    action,
                    label,
                };
                regions.insert(idx + 1, new_region);
                self.revalidate();
            }
        }
    }

    /// Cancel the current edit operation (Escape key).
    pub fn edit_cancel(&mut self) {
        self.edit_state = EditState::default();
    }

    /// Move the cursor to the next/previous edit field.
    pub fn edit_next_field(&mut self) {
        if self.edit_state.editing {
            self.edit_state.cursor_field = self.edit_state.cursor_field.next();
        }
    }

    pub fn edit_prev_field(&mut self) {
        if self.edit_state.editing {
            self.edit_state.cursor_field = self.edit_state.cursor_field.prev();
        }
    }

    /// Cycle the action field forward/backward.
    pub fn edit_cycle_action(&mut self, forward: bool) {
        if !self.edit_state.editing {
            return;
        }
        let current_idx = VALID_ACTIONS
            .iter()
            .position(|&a| a == self.edit_state.draft_action)
            .unwrap_or(0);
        let new_idx = if forward {
            (current_idx + 1) % VALID_ACTIONS.len()
        } else {
            (current_idx + VALID_ACTIONS.len() - 1) % VALID_ACTIONS.len()
        };
        self.edit_state.draft_action = VALID_ACTIONS[new_idx].to_string();
    }

    /// Cycle the chain through available chains in the protein.
    pub fn edit_cycle_chain(&mut self, forward: bool) {
        if !self.edit_state.editing {
            return;
        }
        let chains: Vec<String> = self.protein.chains.iter().map(|c| c.id.clone()).collect();
        if chains.is_empty() {
            return;
        }
        let current_idx = chains
            .iter()
            .position(|c| c == &self.edit_state.draft_chain)
            .unwrap_or(0);
        let new_idx = if forward {
            (current_idx + 1) % chains.len()
        } else {
            (current_idx + chains.len() - 1) % chains.len()
        };
        self.edit_state.draft_chain = chains[new_idx].clone();
    }

    /// Increment or decrement a range field by the given delta.
    pub fn edit_adjust_range(&mut self, field: EditField, delta: i32) {
        if !self.edit_state.editing {
            return;
        }
        match field {
            EditField::RangeStart => {
                let v = self.edit_state.draft_range_start as i32 + delta;
                self.edit_state.draft_range_start = v.max(1) as usize;
            }
            EditField::RangeEnd => {
                let v = self.edit_state.draft_range_end as i32 + delta;
                self.edit_state.draft_range_end = v.max(1) as usize;
            }
            _ => {}
        }
    }

    /// Input a character into the label field.
    pub fn edit_label_input(&mut self, ch: char) {
        if !self.edit_state.editing || self.edit_state.cursor_field != EditField::Label {
            return;
        }
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            if self.edit_state.draft_label.len() < 20 {
                self.edit_state.draft_label.push(ch);
            }
        }
    }

    /// Delete the last character from the label field.
    pub fn edit_label_backspace(&mut self) {
        if !self.edit_state.editing || self.edit_state.cursor_field != EditField::Label {
            return;
        }
        self.edit_state.draft_label.pop();
    }

    /// Cycle through predefined labels for the label field (Tab in label field).
    pub fn edit_cycle_label(&mut self) {
        if !self.edit_state.editing || self.edit_state.cursor_field != EditField::Label {
            return;
        }
        let current = self.edit_state.draft_label.as_str();
        let idx = PREDEFINED_LABELS
            .iter()
            .position(|&l| l == current)
            .map(|i| (i + 1) % PREDEFINED_LABELS.len())
            .unwrap_or(0);
        self.edit_state.draft_label = PREDEFINED_LABELS[idx].to_string();
    }

    /// Validate the current draft and save it.
    /// Returns true if save was successful.
    pub fn edit_save(&mut self) -> bool {
        if !self.edit_state.editing {
            return false;
        }

        // Local validation.
        let start = self.edit_state.draft_range_start;
        let end = self.edit_state.draft_range_end;
        let chain = &self.edit_state.draft_chain;
        let action = &self.edit_state.draft_action;

        // Validate range.
        if start > end {
            self.edit_state.validation_error =
                Some(format!("Invalid range: {} > {}", start, end));
            return false;
        }

        // Validate action.
        if !VALID_ACTIONS.contains(&action.as_str()) {
            self.edit_state.validation_error =
                Some(format!("Unknown action: '{}'", action));
            return false;
        }

        // Check overlap with existing regions (excluding the one being edited).
        let editing_idx = self.edit_state.editing_region_idx;
        if let Some(ref ann) = self.annotation {
            if let Some(ref regions) = ann.editspec_regions {
                for (i, r) in regions.iter().enumerate() {
                    if Some(i) == editing_idx {
                        continue; // Skip the region being edited.
                    }
                    if r.chain == *chain && r.range[1] >= start && r.range[0] <= end {
                        self.edit_state.validation_error = Some(format!(
                            "Overlaps with region {} [{}-{}] on chain {}",
                            i, r.range[0], r.range[1], r.chain
                        ));
                        return false;
                    }
                }
            }
        }

        // Optionally validate via bridge if available.
        if let Some(ref bridge) = self.bridge {
            let bridge_regions = {
                let mut all: Vec<crate::bridge::EditSpecRegionData> = Vec::new();
                // Collect existing regions.
                if let Some(ref ann) = self.annotation {
                    if let Some(ref regions) = ann.editspec_regions {
                        for (i, r) in regions.iter().enumerate() {
                            if Some(i) == editing_idx {
                                // Replace with draft.
                                all.push(crate::bridge::EditSpecRegionData {
                                    chain: self.edit_state.draft_chain.clone(),
                                    range: [self.edit_state.draft_range_start, self.edit_state.draft_range_end],
                                    action: self.edit_state.draft_action.clone(),
                                    label: if self.edit_state.draft_label.is_empty() {
                                        None
                                    } else {
                                        Some(self.edit_state.draft_label.clone())
                                    },
                                });
                            } else {
                                all.push(crate::bridge::EditSpecRegionData {
                                    chain: r.chain.clone(),
                                    range: r.range,
                                    action: r.action.clone(),
                                    label: r.label.clone(),
                                });
                            }
                        }
                    }
                }
                // If adding new, append draft.
                if editing_idx.is_none() {
                    all.push(crate::bridge::EditSpecRegionData {
                        chain: self.edit_state.draft_chain.clone(),
                        range: [self.edit_state.draft_range_start, self.edit_state.draft_range_end],
                        action: self.edit_state.draft_action.clone(),
                        label: if self.edit_state.draft_label.is_empty() {
                            None
                        } else {
                            Some(self.edit_state.draft_label.clone())
                        },
                    });
                }
                all
            };
            if let Ok(issues) = bridge.validate_edit_spec(&bridge_regions) {
                let errors: Vec<_> = issues
                    .iter()
                    .filter(|i| i.severity == "error")
                    .collect();
                if !errors.is_empty() {
                    self.edit_state.validation_error =
                        Some(errors[0].message.clone());
                    return false;
                }
            }
        }

        // All validation passed — push history before mutation.
        let description = match editing_idx {
            Some(idx) => format!("edit region {}", idx),
            None => "add region".to_string(),
        };
        self.push_history(&description);

        // Apply the change.
        let new_region = EditSpecRegion {
            chain: self.edit_state.draft_chain.clone(),
            range: [self.edit_state.draft_range_start, self.edit_state.draft_range_end],
            action: self.edit_state.draft_action.clone(),
            label: if self.edit_state.draft_label.is_empty() {
                None
            } else {
                Some(self.edit_state.draft_label.clone())
            },
        };

        // Ensure annotation structure exists.
        if self.annotation.is_none() {
            self.annotation = Some(Annotation {
                editspec_regions: Some(Vec::new()),
                iteration: None,
                highlights: None,
            });
        }

        if let Some(ref mut ann) = self.annotation {
            if ann.editspec_regions.is_none() {
                ann.editspec_regions = Some(Vec::new());
            }
            if let Some(ref mut regions) = ann.editspec_regions {
                match editing_idx {
                    Some(idx) if idx < regions.len() => {
                        regions[idx] = new_region;
                    }
                    _ => {
                        // Adding new region.
                        regions.push(new_region);
                        self.focused_region = regions.len() - 1;
                    }
                }
            }
        }

        // Clear edit state.
        self.edit_state = EditState::default();
        self.revalidate();
        true
    }
}
