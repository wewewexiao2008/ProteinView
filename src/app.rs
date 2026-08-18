use std::sync::mpsc;

use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui_image::picker::Picker;

use crate::bridge::GemlibBridge;
use crate::product_tree::{ProductTree, StudioSeed, resolve_structure_path};
use crate::ui::block_palette::BlockPalette;
use crate::workflow::{WorkflowError, WorkflowStatus};
use crate::edit_history::{
    EditHistory, HistoryEntry, ValidationIssue, validate_regions,
};
use crate::events::{
    EventLevel, EventLog, EventPane, empty_payload, run_op_from_console_event,
};
use crate::shell::{self, InteractionMode, Overlay, PaneId, Shell, parse_compact_regions, parse_direct_range};
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
    RangeText,
    Chain,
    RangeStart,
    RangeEnd,
    Action,
    Label,
}

impl EditField {
    /// Direct EditRegion form: typed range, action, label.
    const FIELDS: [EditField; 3] = [
        EditField::RangeText,
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
    /// Cursor position in the sequence (0-based residue index).
    pub cursor: usize,
    /// Whether the selection is active (user has explicitly selected residues).
    pub active: bool,
}

impl SeqSelection {
    /// Return the inclusive range of selected residues, sorted.
    pub fn range(&self) -> Option<(usize, usize)> {
        if !self.active {
            return None;
        }
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

    /// Clear the selection (but keep cursor position).
    pub fn clear(&mut self) {
        self.start = None;
        self.end = None;
        self.dragging = false;
        self.active = false;
    }

    /// Select a segment (contiguous residues with the same secondary structure)
    /// starting from the given cursor position.
    pub fn select_segment(&mut self, residues: &[crate::model::protein::Residue], cursor: usize) {
        if residues.is_empty() || cursor >= residues.len() {
            return;
        }
        self.cursor = cursor;
        let (start, end) = find_segment(residues, cursor);
        self.start = Some(start);
        self.end = Some(end);
        self.active = true;
    }

    /// Move the cursor by one residue and update the selection if active.
    pub fn move_cursor(&mut self, residues: &[crate::model::protein::Residue], delta: i32) {
        let max = if residues.is_empty() { 0 } else { residues.len() - 1 };
        let new_pos = if delta > 0 {
            self.cursor.saturating_add(delta as usize).min(max)
        } else {
            self.cursor.saturating_sub((-delta) as usize)
        };
        self.cursor = new_pos;
        // If selection is active, move it to the new segment.
        if self.active && !residues.is_empty() {
            let (start, end) = find_segment(residues, self.cursor);
            self.start = Some(start);
            self.end = Some(end);
        }
    }

    /// Expand or shrink the selection by one residue at the end boundary.
    pub fn expand_end(&mut self, residues: &[crate::model::protein::Residue], delta: i32) {
        let max = if residues.is_empty() { 0 } else { residues.len() - 1 };
        if let Some(e) = self.end {
            let new_end = if delta > 0 {
                e.saturating_add(delta as usize).min(max)
            } else {
                e.saturating_sub((-delta) as usize)
            };
            self.end = Some(new_end);
            self.active = true;
        } else if !residues.is_empty() {
            // No selection yet: select current cursor's segment first.
            self.select_segment(residues, self.cursor);
        }
    }

    /// Shrink or expand the selection by one residue at the start boundary.
    pub fn expand_start(&mut self, residues: &[crate::model::protein::Residue], delta: i32) {
        if let Some(s) = self.start {
            let new_start = if delta > 0 {
                s.saturating_add(delta as usize).min(self.end.unwrap_or(s))
            } else {
                s.saturating_sub((-delta) as usize)
            };
            self.start = Some(new_start);
            self.active = true;
        } else if !residues.is_empty() {
            self.select_segment(residues, self.cursor);
        }
    }

    /// Select an inclusive residue-index range. Missing sequence maps to empty.
    pub fn select_range(&mut self, start: usize, end: usize, max: usize) {
        if max == 0 {
            self.clear();
            return;
        }
        let last = max - 1;
        let start = start.min(last);
        let end = end.min(last);
        self.start = Some(start.min(end));
        self.end = Some(start.max(end));
        self.cursor = start.min(end);
        self.active = true;
        self.dragging = false;
    }

    /// Set cursor on the clicked residue and begin an arbitrary-range drag.
    pub fn click(&mut self, residues: &[crate::model::protein::Residue], idx: usize) {
        if idx < residues.len() {
            self.cursor = idx;
            self.start = Some(idx);
            self.end = Some(idx);
            self.active = true;
            self.dragging = true;
        }
    }

    /// Extend selection during a mouse drag.
    pub fn drag_to(&mut self, idx: usize, max: usize) {
        let idx = idx.min(max.saturating_sub(1));
        if self.start.is_none() {
            self.start = Some(self.cursor);
        }
        self.end = Some(idx);
        self.active = true;
        self.dragging = true;
    }

    /// Jump to the previous/next secondary-structure segment.
    pub fn jump_segment(&mut self, residues: &[crate::model::protein::Residue], dir: i32) {
        if residues.is_empty() || self.cursor >= residues.len() {
            return;
        }
        let current_ss = residues[self.cursor].secondary_structure;
        if dir >= 0 {
            let mut i = self.cursor;
            while i < residues.len() && residues[i].secondary_structure == current_ss {
                i += 1;
            }
            if i < residues.len() {
                self.select_segment(residues, i);
            }
        } else {
            let mut i = self.cursor;
            while i > 0 && residues[i].secondary_structure == current_ss {
                i -= 1;
            }
            if residues[i].secondary_structure == current_ss && i > 0 {
                i -= 1;
            }
            self.select_segment(residues, i);
        }
    }
}

/// Find contiguous residues with the same secondary structure as the one at `cursor_idx`.
fn find_segment(residues: &[crate::model::protein::Residue], cursor_idx: usize) -> (usize, usize) {
    if residues.is_empty() || cursor_idx >= residues.len() {
        return (0, 0);
    }
    let target_ss = residues[cursor_idx].secondary_structure;
    let mut start = cursor_idx;
    let mut end = cursor_idx;
    while start > 0 && residues[start - 1].secondary_structure == target_ss {
        start -= 1;
    }
    while end < residues.len() - 1 && residues[end + 1].secondary_structure == target_ss {
        end += 1;
    }
    (start, end)
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
    /// Typed range field (`A51-80`, `A:51-80`, or `51-80`).
    pub draft_range_text: String,
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
    /// First overlap save is a warning; second Enter replaces overlapping regions.
    pub overlap_replace_armed: bool,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            editing: false,
            editing_region_idx: None,
            cursor_field: EditField::RangeText,
            draft_range_text: String::new(),
            draft_chain: "A".to_string(),
            draft_range_start: 1,
            draft_range_end: 10,
            draft_action: "edit".to_string(),
            draft_label: String::new(),
            validation_error: None,
            delete_confirm: false,
            overlap_replace_armed: false,
        }
    }
}

/// One chain's wrapped sequence block in the EditSpec column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeqChainBlock {
    pub chain_idx: usize,
    pub start_row: u16,
    pub per_line: usize,
    pub wrap_lines: usize,
    pub residue_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextTarget {
    Selection,
    Region(usize),
    Workflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextItem {
    Header(&'static str),
    Action(&'static str),
    Label(&'static str),
    EditRange,
    Delete,
    ReplaceOverlap,
    WorkflowAdd,
    WorkflowDelete,
}

impl ContextItem {
    pub fn selectable(&self) -> bool {
        !matches!(self, Self::Header(_))
    }

    pub fn caption(&self) -> String {
        match self {
            Self::Header(title) => format!("── {title} ──"),
            Self::Action(name) => format!(" {name}"),
            Self::Label(name) => format!(" {name}"),
            Self::EditRange => " Edit range".to_string(),
            Self::Delete => " Delete region".to_string(),
            Self::ReplaceOverlap => " Replace overlapping".to_string(),
            Self::WorkflowAdd => " 加一块".to_string(),
            Self::WorkflowDelete => " 删除".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub col: u16,
    pub row: u16,
    pub cursor: usize,
    pub items: Vec<ContextItem>,
    pub target: ContextTarget,
    pub last_rect: Rect,
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
    /// Horizontal scroll offset for the Sequence panel (unused after wrap).
    pub seq_h_scroll: u16,
    /// Residues per wrapped sequence line (25 or 50).
    pub seq_per_line: usize,
    /// Number of wrapped sequence blocks this frame.
    pub seq_wrap_lines: usize,
    /// One wrap block per chain shown in the EditSpec sequence column.
    pub seq_blocks: Vec<SeqChainBlock>,
    /// Total EditSpec content lines, for vertical scroll clamp.
    pub panel_content_lines: u16,
    /// Selection state for the Sequence panel.
    pub seq_selection: SeqSelection,
    pub context_menu: Option<ContextMenu>,
    pub status_banner: Option<String>,
    pub pending_overlap_action: Option<String>,
    /// Debug counter: total mouse events received (shown in statusbar when > 0).
    pub mouse_event_count: u64,
    /// Row (within the sidebar) where the sequence line is rendered.
    /// Updated each frame by the editspec panel renderer for mouse hit-testing.
    pub seq_line_row: u16,
    /// Content line index of the secondary-structure row (not scroll-adjusted).
    pub ss_line_row: u16,
    /// Four-pane Studio chrome and exclusive-focus router.
    pub shell: Shell,
    /// Optional `--output` path for compact EditSpec on exit.
    pub output_path: Option<String>,
    /// Optional `--gemlib-bin` recorded for Fleet submit (never spawned here).
    pub gemlib_bin: Option<String>,
    pub last_workflow_rect: Option<ratatui::layout::Rect>,
    pub last_tree_rect: Option<ratatui::layout::Rect>,
    pub last_view_rect: Option<ratatui::layout::Rect>,
    /// Last View-pane mouse cell while rotating the camera.
    pub view_drag_last: Option<(u16, u16)>,
    /// Shared-border resize in progress.
    pub chrome_drag: Option<crate::ui::chrome::ChromeDrag>,
    pub product_tree: ProductTree,
    pub tree_cursor: usize,
    pub tree_scroll: u16,
    pub workflow_status: Option<WorkflowStatus>,
    pub workflow_error: Option<WorkflowError>,
    pub workflow_graph: Option<serde_json::Value>,
    pub workflow_cursor: usize,
    pub workflow_delete_confirm: bool,
    pub workflow_drag: Option<usize>,
    pub block_palette: Option<BlockPalette>,
    pub campaign_root: Option<String>,
    pub loaded_structure_path: Option<String>,
    pub state_file: Option<String>,
    pub state_mtime: Option<std::time::SystemTime>,
    pub console_open: bool,
    pub console_verbose: bool,
    pub console_scroll: u16,
    pub event_log: EventLog,
    console_run_seen: usize,
    undo_stack: Vec<RevertSnapshot>,
    redo_stack: Vec<RevertSnapshot>,
    editspec_undo_hold: Option<RevertSnapshot>,
}

#[derive(Clone)]
enum RevertSnapshot {
    Workflow {
        graph: Option<serde_json::Value>,
        status: Option<WorkflowStatus>,
        cursor: usize,
    },
    Load {
        path: Option<String>,
    },
    EditSpec {
        regions: Vec<EditSpecRegion>,
        focused: usize,
        scroll: u16,
        selection: SeqSelection,
    },
    View {
        scheme_type: ColorSchemeType,
        saved_scheme_type: ColorSchemeType,
        viz_mode: VizMode,
        current_chain: usize,
        camera: Camera,
        render_mode: RenderMode,
        selection: SeqSelection,
    },
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
        // Unit tests never embed Python: GEMLIB PYTHONPATH plus pyo3 init
        // can abort the process before assertions run.
        let (bridge, python_available) = if cfg!(test) {
            (None, false)
        } else {
            match GemlibBridge::new() {
                Ok(b) => {
                    eprintln!("Python bridge: initialized (gemlib + contiger available)");
                    (Some(b), true)
                }
                Err(e) => {
                    eprintln!("Warning: Python bridge unavailable — running in read-only mode.");
                    eprintln!("  Reason: {}", e);
                    (None, false)
                }
            }
        };

        Self {
            protein,
            camera,
            color_scheme,
            viz_mode,
            current_chain: 0,
            render_mode,
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
            seq_per_line: 25,
            seq_wrap_lines: 0,
            seq_blocks: Vec::new(),
            panel_content_lines: 0,
            seq_selection: SeqSelection::default(),
            context_menu: None,
            status_banner: None,
            pending_overlap_action: None,
            mouse_event_count: 0,
            seq_line_row: u16::MAX,
            ss_line_row: u16::MAX,
            shell: Shell::pdb_session(),
            output_path: None,
            gemlib_bin: None,
            last_workflow_rect: None,
            last_tree_rect: None,
            last_view_rect: None,
            view_drag_last: None,
            chrome_drag: None,
            product_tree: ProductTree::default(),
            tree_cursor: 0,
            tree_scroll: 0,
            workflow_status: None,
            workflow_error: None,
            workflow_graph: None,
            workflow_cursor: 0,
            workflow_delete_confirm: false,
            workflow_drag: None,
            block_palette: None,
            campaign_root: None,
            loaded_structure_path: None,
            state_file: None,
            state_mtime: None,
            console_open: false,
            console_verbose: false,
            console_scroll: 0,
            event_log: EventLog::default(),
            console_run_seen: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            editspec_undo_hold: None,
        }
    }

    pub fn apply_studio_seed(&mut self, seed: StudioSeed) {
        let first_seed = self.product_tree.is_empty();
        self.campaign_root = seed
            .campaign_root
            .or_else(|| seed.product_tree.campaign_root.clone());
        self.product_tree = seed.product_tree;
        self.workflow_status = seed.workflow_status;
        self.workflow_error = seed.workflow_error;
        self.workflow_graph = seed.workflow_graph;
        if first_seed {
            self.tree_cursor = 0;
            self.tree_scroll = 0;
            self.workflow_cursor = 0;
            self.workflow_delete_confirm = false;
            if !self.product_tree.is_empty() {
                self.shell = Shell::campaign_session();
            }
        }
        if let Some(spec) = seed.edit_spec.as_deref() {
            if !spec.is_empty() {
                self.load_edit_spec_text(spec);
            }
        }
        if let Some(status) = &self.workflow_status {
            if let Some(index) = status
                .nodes
                .iter()
                .position(|node| node.waiting && node.kind == "step")
            {
                self.workflow_cursor = index;
            }
        }
        let _ = self.reload_view_from_tree();
    }

    pub fn reload_view_from_tree(&mut self) -> Result<bool, String> {
        let selected = self.product_tree.selected_sample_id.clone();
        let rows = self.product_tree.visible_rows();
        let target = if let Some(id) = selected {
            rows.iter().position(|(_, node)| node.sample_id == id)
        } else {
            rows.iter()
                .rposition(|(_, node)| node.first_structure_path().is_some())
        };
        let Some(index) = target else {
            return Ok(false);
        };
        self.tree_cursor = index;
        self.activate_tree_row()
    }

    pub fn poll_studio_state(&mut self) -> bool {
        let Some(path) = self.state_file.clone() else {
            return false;
        };
        let meta = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => return false,
        };
        let modified = match meta.modified() {
            Ok(time) => time,
            Err(_) => return false,
        };
        if self.state_mtime == Some(modified) {
            self.poll_console_hint();
            return false;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            return false;
        };
        let Ok(seed) = crate::product_tree::load_studio_seed(&text) else {
            return false;
        };
        self.state_mtime = Some(modified);
        self.apply_studio_seed(seed);
        self.poll_console_hint();
        true
    }

    pub fn poll_console_hint(&mut self) {
        let Some(root) = self.campaign_root.as_deref() else {
            return;
        };
        let path = std::path::Path::new(root).join("console.jsonl");
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        if let Some(hint) = crate::debug_run::last_console_hint(&text) {
            self.status_banner = Some(hint);
            self.console_open = true;
        }
        self.project_run_events(&text);
    }

    fn project_run_events(&mut self, text: &str) {
        let rows = crate::debug_run::parse_console_records(text);
        if rows.len() <= self.console_run_seen {
            return;
        }
        for row in rows.iter().skip(self.console_run_seen) {
            let op = run_op_from_console_event(&row.event);
            self.event_log.emit(
                EventLevel::Run,
                EventPane::None,
                op,
                false,
                row.message.clone(),
                serde_json::json!({
                    "event": row.event,
                    "stage": row.stage,
                }),
            );
        }
        self.console_run_seen = rows.len();
        self.scroll_console_to_end();
    }

    fn emit(
        &mut self,
        level: EventLevel,
        pane: EventPane,
        op: &str,
        undoable: bool,
        summary: impl Into<String>,
        revert: Option<RevertSnapshot>,
    ) {
        self.event_log.emit(level, pane, op, undoable, summary, empty_payload());
        if undoable {
            if let Some(snapshot) = revert {
                self.undo_stack.push(snapshot);
                self.redo_stack.clear();
            }
        }
        if let Some(summary) = self.event_log.newest_visible_summary(self.console_verbose) {
            self.status_banner = Some(summary);
        }
        self.scroll_console_to_end();
    }

    fn snapshot_workflow(&self) -> RevertSnapshot {
        RevertSnapshot::Workflow {
            graph: self.workflow_graph.clone(),
            status: self.workflow_status.clone(),
            cursor: self.workflow_cursor,
        }
    }

    fn snapshot_load(&self) -> RevertSnapshot {
        RevertSnapshot::Load {
            path: self.loaded_structure_path.clone(),
        }
    }

    fn snapshot_editspec(&self) -> RevertSnapshot {
        RevertSnapshot::EditSpec {
            regions: self.snapshot_regions(),
            focused: self.focused_region,
            scroll: self.panel_scroll,
            selection: self.seq_selection.clone(),
        }
    }

    fn snapshot_view(&self) -> RevertSnapshot {
        RevertSnapshot::View {
            scheme_type: self.color_scheme.scheme_type,
            saved_scheme_type: self.saved_color_scheme_type,
            viz_mode: self.viz_mode,
            current_chain: self.current_chain,
            camera: self.camera.clone(),
            render_mode: self.render_mode,
            selection: self.seq_selection.clone(),
        }
    }

    fn apply_revert(&mut self, snapshot: RevertSnapshot) {
        match snapshot {
            RevertSnapshot::Workflow {
                graph,
                status,
                cursor,
            } => {
                self.workflow_graph = graph;
                self.workflow_status = status;
                self.workflow_cursor = cursor;
                let _ = self.persist_workflow_draft();
            }
            RevertSnapshot::Load { path } => {
                self.loaded_structure_path = path.clone();
                if let Some(path) = path {
                    if let Ok(protein) = crate::parser::pdb::load_structure(&path) {
                        self.replace_protein(protein);
                    }
                }
            }
            RevertSnapshot::EditSpec {
                regions,
                focused,
                scroll,
                selection,
            } => {
                if self.annotation.is_none() {
                    self.annotation = Some(Annotation {
                        editspec_regions: Some(Vec::new()),
                        iteration: None,
                        highlights: None,
                    });
                }
                if let Some(ref mut ann) = self.annotation {
                    ann.editspec_regions = Some(regions);
                }
                self.focused_region = focused;
                self.panel_scroll = scroll;
                self.seq_selection = selection;
                self.revalidate();
                self.sync_selection_overlay();
            }
            RevertSnapshot::View {
                scheme_type,
                saved_scheme_type,
                viz_mode,
                current_chain,
                camera,
                render_mode,
                selection,
            } => {
                self.saved_color_scheme_type = saved_scheme_type;
                self.viz_mode = viz_mode;
                self.current_chain = current_chain;
                self.camera = camera;
                self.render_mode = render_mode;
                self.seq_selection = selection;
                if scheme_type == ColorSchemeType::Interface
                    || self.active_panel == ActivePanel::Interface
                {
                    self.rebuild_interface_colors();
                } else {
                    self.color_scheme =
                        ColorScheme::new(scheme_type, self.protein.residue_count());
                    self.mesh_dirty = true;
                }
                self.sync_selection_overlay();
            }
        }
    }

    pub fn toggle_console(&mut self) {
        if self.shell.console_focused {
            self.close_console_focus();
            return;
        }
        self.console_open = true;
        self.shell.console_focused = true;
        self.emit(
            EventLevel::Session,
            EventPane::None,
            "session.overlay_open",
            false,
            "console focused",
            None,
        );
    }

    pub fn close_console_focus(&mut self) {
        self.shell.console_focused = false;
        self.emit(
            EventLevel::Session,
            EventPane::None,
            "session.overlay_close",
            false,
            "console unfocused",
            None,
        );
    }

    pub fn scroll_console(&mut self, delta: i32) {
        let visible = self.event_log.visible(self.console_verbose).len();
        let height = 4usize;
        let max = visible.saturating_sub(height) as u16;
        if delta < 0 {
            self.console_scroll = self.console_scroll.saturating_sub(delta.unsigned_abs() as u16);
        } else {
            self.console_scroll = self
                .console_scroll
                .saturating_add(delta as u16)
                .min(max);
        }
    }

    fn scroll_console_to_end(&mut self) {
        let visible = self.event_log.visible(self.console_verbose).len();
        self.console_scroll = visible.saturating_sub(4) as u16;
    }

    pub fn toggle_console_verbose(&mut self) {
        self.console_verbose = !self.console_verbose;
        self.scroll_console_to_end();
    }

    pub fn session_undo(&mut self) {
        let Some(snapshot) = self.undo_stack.pop() else {
            return;
        };
        let current = match &snapshot {
            RevertSnapshot::Workflow { .. } => self.snapshot_workflow(),
            RevertSnapshot::Load { .. } => self.snapshot_load(),
            RevertSnapshot::EditSpec { .. } => self.snapshot_editspec(),
            RevertSnapshot::View { .. } => self.snapshot_view(),
        };
        self.apply_revert(snapshot);
        self.redo_stack.push(current);
        self.emit(
            EventLevel::Session,
            EventPane::None,
            "session.undo",
            false,
            "undo TUI",
            None,
        );
    }

    pub fn prepare_editspec_undo(&mut self) {
        self.editspec_undo_hold = Some(self.snapshot_editspec());
    }

    pub fn finish_editspec_select(&mut self, summary: &str) {
        let revert = self.editspec_undo_hold.take();
        self.emit(
            EventLevel::Intent,
            EventPane::EditSpec,
            "editspec.select",
            true,
            summary.to_string(),
            revert,
        );
    }

    pub fn emit_session_focus(&mut self) {
        self.emit(
            EventLevel::Session,
            EventPane::None,
            "session.focus",
            false,
            format!("focus {}", self.shell.focused.name()),
            None,
        );
    }

    pub fn toggle_focused_collapse(&mut self) {
        self.shell.toggle_collapse();
        let pane = self.shell.focused.name();
        let state = if self.shell.is_expanded(self.shell.focused) {
            "expand"
        } else {
            "collapse"
        };
        self.emit(
            EventLevel::Session,
            EventPane::None,
            "session.collapse",
            false,
            format!("{state} {pane}"),
            None,
        );
    }

    pub fn emit_view_camera(&mut self) {
        self.emit(
            EventLevel::Nav,
            EventPane::View,
            "view.camera",
            false,
            "camera",
            None,
        );
    }

    pub fn reset_view_camera(&mut self, cols: u16, rows: u16) {
        let revert = self.snapshot_view();
        self.camera.reset();
        self.recalculate_zoom(cols, rows);
        self.emit(
            EventLevel::Intent,
            EventPane::View,
            "view.reset_camera",
            true,
            "reset camera",
            Some(revert),
        );
    }

    pub fn clear_editspec_selection(&mut self) {
        let revert = if self.seq_selection.active {
            Some(self.snapshot_editspec())
        } else {
            None
        };
        self.abandon_edit_form();
        self.status_banner = None;
        self.pending_overlap_action = None;
        self.seq_selection.clear();
        if self.shell.mode == InteractionMode::Select {
            self.shell.enter_idle();
        }
        self.sync_selection_overlay();
        if revert.is_some() {
            self.emit(
                EventLevel::Intent,
                EventPane::EditSpec,
                "editspec.clear_select",
                true,
                "clear select",
                revert,
            );
        }
    }

    pub fn emit_editspec_cursor(&mut self) {
        self.emit(
            EventLevel::Nav,
            EventPane::EditSpec,
            "editspec.cursor",
            false,
            format!("seq {}", self.seq_selection.cursor),
            None,
        );
    }

    pub fn session_redo(&mut self) {
        let Some(snapshot) = self.redo_stack.pop() else {
            return;
        };
        let current = match &snapshot {
            RevertSnapshot::Workflow { .. } => self.snapshot_workflow(),
            RevertSnapshot::Load { .. } => self.snapshot_load(),
            RevertSnapshot::EditSpec { .. } => self.snapshot_editspec(),
            RevertSnapshot::View { .. } => self.snapshot_view(),
        };
        self.apply_revert(snapshot);
        self.undo_stack.push(current);
        self.emit(
            EventLevel::Session,
            EventPane::None,
            "session.redo",
            false,
            "redo TUI",
            None,
        );
    }

    pub fn confirm_debug_run(&mut self) {
        self.close_run_overlay();
        match self.spawn_debug_run() {
            Ok(()) => {
                self.console_open = true;
                self.emit(
                    EventLevel::Run,
                    EventPane::None,
                    "run.debug_start",
                    false,
                    "waiting debug … (3s)",
                    None,
                );
            }
            Err(err) => {
                self.status_banner = Some(err);
            }
        }
    }

    fn spawn_debug_run(&self) -> Result<(), String> {
        let bin = self
            .gemlib_bin
            .as_deref()
            .ok_or_else(|| "缺少 gemlib-bin".to_string())?;
        let root = self
            .campaign_root
            .as_deref()
            .ok_or_else(|| "缺少 campaign".to_string())?;
        let recipe = crate::debug_run::campaign_recipe_path(root)
            .ok_or_else(|| "campaign 里没有 run.yaml".to_string())?;
        let argv = crate::debug_run::debug_run_argv(bin, &recipe, root);
        let mut cmd = std::process::Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        if let Some(state) = &self.state_file {
            cmd.env("GEMLIB_STUDIO_STATE", state);
        }
        cmd.spawn().map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn tree_visible_count(&self) -> usize {
        self.product_tree.visible_rows().len()
    }

    pub fn tree_move(&mut self, delta: isize) {
        let count = self.tree_visible_count();
        if count == 0 {
            self.tree_cursor = 0;
            return;
        }
        let next = self.tree_cursor as isize + delta;
        self.tree_cursor = next.clamp(0, count as isize - 1) as usize;
        self.focus_workflow_from_tree();
        self.ensure_tree_visible();
        self.emit(
            EventLevel::Nav,
            EventPane::Tree,
            "tree.cursor",
            false,
            format!("tree cursor {}", self.tree_cursor),
            None,
        );
    }

    pub fn tree_set_expanded(&mut self, expanded: bool) {
        let sample_id = self
            .product_tree
            .visible_rows()
            .get(self.tree_cursor)
            .map(|(_depth, node)| node.sample_id.clone());
        if let Some(sample_id) = sample_id {
            self.product_tree.set_expanded(&sample_id, expanded);
            self.reindex_tree_cursor(&sample_id);
            self.ensure_tree_visible();
            self.emit(
                EventLevel::Nav,
                EventPane::Tree,
                "tree.fold",
                false,
                format!("fold {sample_id}"),
                None,
            );
        }
    }

    pub fn handle_tree_click(&mut self, col: u16, row: u16) -> Result<(), String> {
        let Some(rect) = self.last_tree_rect else {
            return Ok(());
        };
        let meta: Vec<crate::ui::tree_pane::TreeRowHit> = self
            .product_tree
            .visible_rows()
            .iter()
            .map(|(depth, node)| crate::ui::tree_pane::TreeRowHit {
                depth: *depth,
                has_children: !node.children.is_empty(),
            })
            .collect();
        let Some((idx, hit)) =
            crate::ui::tree_pane::hit_test(rect, col, row, self.tree_scroll, &meta)
        else {
            return Ok(());
        };
        match hit {
            crate::ui::tree_pane::TreeHit::Fold => {
                let Some((sample_id, expanded)) = self
                    .product_tree
                    .visible_rows()
                    .get(idx)
                    .map(|(_depth, node)| (node.sample_id.clone(), node.expanded))
                else {
                    return Ok(());
                };
                self.product_tree.set_expanded(&sample_id, !expanded);
                self.reindex_tree_cursor(&sample_id);
                self.ensure_tree_visible();
                self.emit(
                    EventLevel::Nav,
                    EventPane::Tree,
                    "tree.fold",
                    false,
                    format!("fold {sample_id}"),
                    None,
                );
            }
            crate::ui::tree_pane::TreeHit::Label => {
                self.tree_cursor = idx;
                self.activate_tree_row()?;
                self.ensure_tree_visible();
            }
        }
        Ok(())
    }

    pub fn max_tree_scroll(&self) -> u16 {
        let view_h = self
            .last_tree_rect
            .map(|r| r.height.saturating_sub(2))
            .unwrap_or(0);
        (self.tree_visible_count() as u16).saturating_sub(view_h)
    }

    fn reindex_tree_cursor(&mut self, sample_id: &str) {
        if let Some(idx) = self
            .product_tree
            .visible_rows()
            .iter()
            .position(|(_depth, node)| node.sample_id == sample_id)
        {
            self.tree_cursor = idx;
            return;
        }
        let count = self.tree_visible_count();
        self.tree_cursor = count.saturating_sub(1);
    }

    fn ensure_tree_visible(&mut self) {
        let view_h = self
            .last_tree_rect
            .map(|r| r.height.saturating_sub(2))
            .unwrap_or(0);
        if view_h == 0 {
            return;
        }
        let cursor = self.tree_cursor as u16;
        if cursor < self.tree_scroll {
            self.tree_scroll = cursor;
        } else if cursor >= self.tree_scroll.saturating_add(view_h) {
            self.tree_scroll = cursor.saturating_sub(view_h.saturating_sub(1));
        }
        self.tree_scroll = self.tree_scroll.min(self.max_tree_scroll());
    }

    pub fn replace_protein(&mut self, mut protein: Protein) {
        protein.center();
        self.has_plddt = protein.has_plddt();
        self.protein = protein;
        self.current_chain = 0;
        self.mesh_dirty = true;
        self.needs_clear = true;
        self.seq_selection.clear();
        self.color_scheme = ColorScheme::new(self.color_scheme.scheme_type, self.protein.residue_count());
        self.recalculate_zoom(80, 24);
    }

    pub fn activate_tree_row(&mut self) -> Result<bool, String> {
        let row = self
            .product_tree
            .visible_rows()
            .get(self.tree_cursor)
            .map(|(_depth, node)| {
                (
                    node.sample_id.clone(),
                    node.first_structure_path().map(str::to_string),
                )
            });
        let Some((sample_id, structure_path)) = row else {
            return Ok(false);
        };
        self.product_tree.select(&sample_id);
        self.focus_workflow_from_tree();
        let Some(relative) = structure_path else {
            return Ok(false);
        };
        let path = resolve_structure_path(self.campaign_root.as_deref(), &relative);
        let revert = self.snapshot_load();
        let protein = crate::parser::pdb::load_structure(path.to_str().unwrap_or_default())
            .map_err(|err| err.to_string())?;
        self.replace_protein(protein);
        self.loaded_structure_path = Some(path.display().to_string());
        self.emit(
            EventLevel::Intent,
            EventPane::Tree,
            "tree.load",
            true,
            format!("load {}", relative),
            Some(revert),
        );
        Ok(true)
    }

    pub fn cycle_color(&mut self) {
        let revert = self.snapshot_view();
        if self.active_panel == ActivePanel::Interface {
            // While interface mode is active, cycle the saved scheme so the
            // user's preference is tracked, but keep displaying Interface colors.
            self.saved_color_scheme_type = self.saved_color_scheme_type.next(self.has_plddt);
        } else {
            let next = self.color_scheme.scheme_type.next(self.has_plddt);
            self.color_scheme = ColorScheme::new(next, self.protein.residue_count());
            self.mesh_dirty = true;
        }
        self.emit(
            EventLevel::Intent,
            EventPane::View,
            "view.style",
            true,
            "cycle color",
            Some(revert),
        );
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
        let revert = self.snapshot_view();
        self.viz_mode = self.viz_mode.next();
        self.emit(
            EventLevel::Intent,
            EventPane::View,
            "view.style",
            true,
            format!("viz {}", self.viz_mode.name()),
            Some(revert),
        );
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

    pub fn set_current_chain(&mut self, idx: usize) {
        if idx >= self.protein.chains.len() || idx == self.current_chain {
            return;
        }
        self.current_chain = idx;
        if self.active_panel == ActivePanel::Interface {
            self.rebuild_interface_colors();
        }
    }

    pub fn next_chain(&mut self) {
        if !self.protein.chains.is_empty() {
            let revert = self.snapshot_view();
            self.current_chain = (self.current_chain + 1) % self.protein.chains.len();
            if self.seq_selection.active {
                self.seq_selection.clear();
                self.sync_selection_overlay();
            }
            if self.active_panel == ActivePanel::Interface {
                self.rebuild_interface_colors();
            }
            self.emit_view_chain(Some(revert));
        }
    }

    pub fn prev_chain(&mut self) {
        if !self.protein.chains.is_empty() {
            let revert = self.snapshot_view();
            self.current_chain = if self.current_chain == 0 {
                self.protein.chains.len() - 1
            } else {
                self.current_chain - 1
            };
            if self.seq_selection.active {
                self.seq_selection.clear();
                self.sync_selection_overlay();
            }
            if self.active_panel == ActivePanel::Interface {
                self.rebuild_interface_colors();
            }
            self.emit_view_chain(Some(revert));
        }
    }

    fn emit_view_chain(&mut self, revert: Option<RevertSnapshot>) {
        let name = self
            .protein
            .chains
            .get(self.current_chain)
            .map(|chain| chain.id.as_str())
            .unwrap_or("—");
        self.emit(
            EventLevel::Intent,
            EventPane::View,
            "view.chain",
            revert.is_some(),
            format!("chain {name}"),
            revert,
        );
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
        let revert = self.snapshot_view();
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
        self.emit(
            EventLevel::Intent,
            EventPane::View,
            "view.style",
            true,
            format!("hd {}", self.render_mode.name()),
            Some(revert),
        );
    }

    /// Upgrade to FullHD (Sixel/Kitty) or back to HalfBlock.
    /// Bound to `M` (Shift+M).  Warns when entering FullHD over SSH.
    pub fn toggle_fullhd(&mut self, term_cols: u16, term_rows: u16) {
        let revert = self.snapshot_view();
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
        self.emit(
            EventLevel::Intent,
            EventPane::View,
            "view.style",
            true,
            format!("hd {}", self.render_mode.name()),
            Some(revert),
        );
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
        let revert = self.snapshot_editspec();
        self.edit_history.push(HistoryEntry {
            description: description.to_string(),
            snapshot: self.snapshot_regions(),
            focused_region: self.focused_region,
            panel_scroll: self.panel_scroll,
        });
        self.emit(
            EventLevel::Intent,
            EventPane::EditSpec,
            "editspec.action",
            true,
            description.to_string(),
            Some(revert),
        );
    }

    /// Re-run validation on the current region list and cache the results.
    pub fn revalidate(&mut self) {
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
        if !self.undo_stack.is_empty() {
            self.session_undo();
            return;
        }
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
    /// If there are no regions, delegates to `edit_region_add()` instead.
    pub fn edit_region_start(&mut self) {
        if self.edit_state.editing && self.shell.mode != InteractionMode::EditRegion {
            self.abandon_edit_form();
        }
        if self.edit_state.editing {
            return;
        }
        let regions = match self.annotation.as_ref().and_then(|a| a.editspec_regions.as_ref()) {
            Some(r) if !r.is_empty() => r,
            _ => {
                self.edit_region_open_empty();
                return;
            }
        };
        let idx = self.focused_region.min(regions.len().saturating_sub(1));
        let region = &regions[idx];
        let range_text = format!("{}{}-{}", region.chain, region.range[0], region.range[1]);

        self.edit_state = EditState {
            editing: true,
            editing_region_idx: Some(idx),
            cursor_field: EditField::RangeText,
            draft_range_text: range_text,
            draft_chain: region.chain.clone(),
            draft_range_start: region.range[0],
            draft_range_end: region.range[1],
            draft_action: region.action.clone(),
            draft_label: region.label.clone().unwrap_or_default(),
            validation_error: None,
            delete_confirm: false,
            overlap_replace_armed: false,
        };
        self.shell.focus(PaneId::EditSpec);
        self.shell.enter_mode(InteractionMode::EditRegion);
    }

    /// Start adding a new region (a key in EditSpec panel).
    pub fn edit_region_add(&mut self) {
        if self.edit_state.editing && self.shell.mode != InteractionMode::EditRegion {
            self.abandon_edit_form();
        }
        if self.edit_state.editing {
            return;
        }
        // Default chain is the current protein chain, or first chain.
        let default_chain = self
            .protein
            .chains
            .get(self.current_chain)
            .map(|c| c.id.clone())
            .or_else(|| self.protein.chains.first().map(|c| c.id.clone()))
            .unwrap_or_else(|| "A".to_string());

        // Default range end is the current chain's residue count, or 10 as fallback.
        let default_range_end = self
            .protein
            .chains
            .get(self.current_chain)
            .map(|c| c.residues.len())
            .unwrap_or(10)
            .max(1);

        self.edit_state = EditState {
            editing: true,
            editing_region_idx: None,
            cursor_field: EditField::RangeText,
            draft_range_text: String::new(),
            draft_chain: default_chain,
            draft_range_start: 1,
            draft_range_end: default_range_end,
            draft_action: "edit".to_string(),
            draft_label: String::new(),
            validation_error: None,
            delete_confirm: false,
            overlap_replace_armed: false,
        };
        self.shell.focus(PaneId::EditSpec);
        self.shell.enter_mode(InteractionMode::EditRegion);
    }

    /// Enter opens an empty form, or a selection-prefilled form when a range is active.
    pub fn edit_region_open_empty(&mut self) {
        if self.edit_state.editing && self.shell.mode != InteractionMode::EditRegion {
            self.abandon_edit_form();
        }
        if self.edit_state.editing {
            return;
        }
        if self.seq_selection.active {
            self.edit_region_open_from_selection();
            return;
        }
        self.edit_region_add();
    }

    pub fn edit_region_open_from_selection(&mut self) {
        if self.edit_state.editing && self.shell.mode != InteractionMode::EditRegion {
            self.abandon_edit_form();
        }
        if self.edit_state.editing {
            return;
        }
        let Some((start, end)) = self.seq_selection.range() else {
            self.edit_region_add();
            return;
        };
        let Some(chain) = self.protein.chains.get(self.current_chain) else {
            self.edit_region_add();
            return;
        };
        if start >= chain.residues.len() || end >= chain.residues.len() {
            self.edit_region_add();
            return;
        }
        let seq_start = chain.residues[start].seq_num as usize;
        let seq_end = chain.residues[end].seq_num as usize;
        self.edit_state = EditState {
            editing: true,
            editing_region_idx: None,
            cursor_field: EditField::Action,
            draft_range_text: format!("{}{}-{}", chain.id, seq_start, seq_end),
            draft_chain: chain.id.clone(),
            draft_range_start: seq_start,
            draft_range_end: seq_end,
            draft_action: "edit".to_string(),
            draft_label: String::new(),
            validation_error: None,
            delete_confirm: false,
            overlap_replace_armed: false,
        };
        self.shell.focus(PaneId::EditSpec);
        self.shell.enter_mode(InteractionMode::EditRegion);
    }

    /// Delete the focused region (dd -- double d confirmation).
    /// Returns true if the delete was executed (second 'd').
    pub fn edit_region_delete(&mut self) -> bool {
        if self.edit_state.editing {
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
        if self.edit_state.editing {
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
        self.shell.restore_previous_mode();
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

    /// Input a character into the label or typed-range field.
    pub fn edit_label_input(&mut self, ch: char) {
        if !self.edit_state.editing {
            return;
        }
        self.edit_state.overlap_replace_armed = false;
        self.edit_state.validation_error = None;
        match self.edit_state.cursor_field {
            EditField::Label if ch.is_alphanumeric() || ch == '-' || ch == '_' => {
                if self.edit_state.draft_label.len() < 20 {
                    self.edit_state.draft_label.push(ch);
                }
            }
            EditField::RangeText if ch.is_ascii_alphanumeric() || ch == '-' || ch == ':' => {
                if self.edit_state.draft_range_text.len() < 24 {
                    self.edit_state.draft_range_text.push(ch);
                }
            }
            _ => {}
        }
    }

    /// Delete the last character from the active text field.
    pub fn edit_label_backspace(&mut self) {
        if !self.edit_state.editing {
            return;
        }
        self.edit_state.overlap_replace_armed = false;
        self.edit_state.validation_error = None;
        match self.edit_state.cursor_field {
            EditField::Label => {
                self.edit_state.draft_label.pop();
            }
            EditField::RangeText => {
                self.edit_state.draft_range_text.pop();
            }
            _ => {}
        }
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

        let default_chain = self
            .protein
            .chains
            .get(self.current_chain)
            .map(|c| c.id.clone())
            .unwrap_or_else(|| self.edit_state.draft_chain.clone());
        match parse_direct_range(&self.edit_state.draft_range_text, &default_chain) {
            Ok((chain, start, end)) => {
                self.edit_state.draft_chain = chain;
                self.edit_state.draft_range_start = start;
                self.edit_state.draft_range_end = end;
            }
            Err(msg) => {
                self.edit_state.validation_error = Some(msg);
                return false;
            }
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

        // Overlap is a warning, not a lock. First Enter arms replace; second replaces.
        let editing_idx = self.edit_state.editing_region_idx;
        let overlap_idxs = self.overlapping_region_indices(chain, start, end, editing_idx);
        if !overlap_idxs.is_empty() && !self.edit_state.overlap_replace_armed {
            self.edit_state.overlap_replace_armed = true;
            let i = overlap_idxs[0];
            let r = self
                .annotation
                .as_ref()
                .and_then(|a| a.editspec_regions.as_ref())
                .and_then(|regions| regions.get(i));
            self.edit_state.validation_error = Some(match r {
                Some(r) => format!(
                    "Overlaps region {} [{}-{}]. Change range, or Enter again to replace.",
                    i, r.range[0], r.range[1]
                ),
                None => "Overlaps an existing region. Enter again to replace.".to_string(),
            });
            return false;
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
                    .filter(|i| {
                        !(self.edit_state.overlap_replace_armed
                            && i.message.to_ascii_lowercase().contains("overlap"))
                    })
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
                let mut apply_idx = editing_idx;
                if !overlap_idxs.is_empty() {
                    let mut drop = overlap_idxs;
                    drop.sort_unstable();
                    drop.dedup();
                    for o in drop.into_iter().rev() {
                        if o < regions.len() {
                            regions.remove(o);
                        }
                        if let Some(idx) = apply_idx {
                            if o < idx {
                                apply_idx = Some(idx - 1);
                            }
                        }
                    }
                }
                match apply_idx {
                    Some(idx) if idx < regions.len() => {
                        regions[idx] = new_region;
                    }
                    _ => {
                        regions.push(new_region);
                        self.focused_region = regions.len() - 1;
                    }
                }
            }
        }

        // Clear edit state.
        self.edit_state = EditState::default();
        self.shell.restore_previous_mode();
        self.revalidate();
        true
    }

    /// Apply an action shortcut (keys 1-5) to the current sequence selection.
    /// Creates a new region or modifies an existing one.
    /// Returns a status message for display.
    pub fn apply_action_shortcut(&mut self, action: &str) -> Option<String> {
        let (start, end) = self.seq_selection.range()?;
        let chain = self.protein.chains.get(self.current_chain)?;
        let chain_id = chain.id.clone();
        let seq_start = chain.residues.get(start)?.seq_num as usize;
        let seq_end = chain.residues.get(end)?.seq_num as usize;

        // Check if a region exactly covers this range on this chain.
        let exact_match = self.annotation.as_ref()
            .and_then(|a| a.editspec_regions.as_ref())
            .map(|regions| {
                regions.iter().position(|r| {
                    r.chain == chain_id && r.range[0] == seq_start && r.range[1] == seq_end
                })
            })
            .flatten();

        // Check for overlaps with existing regions.
        let has_overlap = self.annotation.as_ref()
            .and_then(|a| a.editspec_regions.as_ref())
            .map(|regions| {
                regions.iter().any(|r| {
                    r.chain == chain_id
                        && r.range[1] >= seq_start
                        && r.range[0] <= seq_end
                        && !(r.range[0] == seq_start && r.range[1] == seq_end)
                })
            })
            .unwrap_or(false);

        if has_overlap {
            if self.pending_overlap_action.as_deref() != Some(action) {
                self.pending_overlap_action = Some(action.to_string());
                let msg = format!(
                    "Overlaps existing region. Press {action} again to replace, or right-click → Replace overlapping."
                );
                self.status_banner = Some(msg.clone());
                return Some(msg);
            }
            self.remove_overlapping_regions(&chain_id, seq_start, seq_end, exact_match);
            self.pending_overlap_action = None;
            self.status_banner = None;
        } else {
            self.pending_overlap_action = None;
        }

        self.push_history(&format!("action shortcut {}:{}-{} -> {}", chain_id, seq_start, seq_end, action));

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
                if let Some(idx) = exact_match {
                    // Modify existing region's action.
                    let old_action = regions[idx].action.clone();
                    regions[idx].action = action.to_string();
                    Some(format!("Changed region {} [{}-{}] action: {} -> {}",
                        chain_id, seq_start, seq_end, old_action, action))
                } else {
                    // Create new region.
                    let sym = match action {
                        "keep" => "=",
                        "edit" => "~",
                        "replace" => ">",
                        "insert" => "+",
                        "delete" => "-",
                        _ => "?",
                    };
                    regions.push(EditSpecRegion {
                        chain: chain_id.clone(),
                        range: [seq_start, seq_end],
                        action: action.to_string(),
                        label: None,
                    });
                    self.focused_region = regions.len() - 1;
                    Some(format!("Created {} region {}:{}-{}",
                        sym, chain_id, seq_start, seq_end))
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn default_chain_id(&self) -> String {
        self.protein
            .chains
            .get(self.current_chain)
            .map(|c| c.id.clone())
            .or_else(|| self.protein.chains.first().map(|c| c.id.clone()))
            .unwrap_or_else(|| "A".to_string())
    }

    pub fn enter_select_mode(&mut self) {
        self.abandon_edit_form();
        self.shell.focus(PaneId::EditSpec);
        self.shell.enter_mode(InteractionMode::Select);
        self.emit(
            EventLevel::Intent,
            EventPane::EditSpec,
            "editspec.enter_select",
            false,
            "enter select",
            None,
        );
    }

    pub fn abandon_edit_form(&mut self) {
        self.edit_state = EditState::default();
        if self.shell.mode == InteractionMode::EditRegion {
            self.shell.enter_idle();
        }
    }

    fn overlapping_region_indices(
        &self,
        chain: &str,
        start: usize,
        end: usize,
        except: Option<usize>,
    ) -> Vec<usize> {
        self.annotation
            .as_ref()
            .and_then(|a| a.editspec_regions.as_ref())
            .map(|regions| {
                regions
                    .iter()
                    .enumerate()
                    .filter(|(i, r)| {
                        Some(*i) != except
                            && r.chain == chain
                            && r.range[1] >= start
                            && r.range[0] <= end
                    })
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn remove_overlapping_regions(
        &mut self,
        chain: &str,
        start: usize,
        end: usize,
        except: Option<usize>,
    ) {
        let mut drop = self.overlapping_region_indices(chain, start, end, except);
        drop.sort_unstable();
        drop.dedup();
        if let Some(regions) = self
            .annotation
            .as_mut()
            .and_then(|a| a.editspec_regions.as_mut())
        {
            for i in drop.into_iter().rev() {
                if i < regions.len() {
                    regions.remove(i);
                }
            }
        }
        if let Some(len) = self
            .annotation
            .as_ref()
            .and_then(|a| a.editspec_regions.as_ref())
            .map(|r| r.len())
        {
            if len == 0 {
                self.focused_region = 0;
            } else if self.focused_region >= len {
                self.focused_region = len - 1;
            }
        }
    }

    fn selection_seq_range(&self) -> Option<(String, usize, usize)> {
        let (start, end) = self.seq_selection.range()?;
        let chain = self.protein.chains.get(self.current_chain)?;
        let seq_start = chain.residues.get(start)?.seq_num as usize;
        let seq_end = chain.residues.get(end)?.seq_num as usize;
        Some((chain.id.clone(), seq_start, seq_end))
    }

    fn selection_exact_region(&self) -> Option<usize> {
        let (chain, start, end) = self.selection_seq_range()?;
        self.annotation
            .as_ref()
            .and_then(|a| a.editspec_regions.as_ref())?
            .iter()
            .position(|r| r.chain == chain && r.range[0] == start && r.range[1] == end)
    }

    fn selection_has_overlap(&self) -> bool {
        let Some((chain, start, end)) = self.selection_seq_range() else {
            return false;
        };
        !self
            .overlapping_region_indices(&chain, start, end, self.selection_exact_region())
            .is_empty()
    }

    pub fn residue_indices_for_seq_range(
        residues: &[crate::model::protein::Residue],
        seq_start: usize,
        seq_end: usize,
    ) -> Option<(usize, usize)> {
        let start = residues.iter().position(|r| (r.seq_num as usize) >= seq_start)?;
        let end = residues.iter().rposition(|r| (r.seq_num as usize) <= seq_end)?;
        if start <= end {
            Some((start, end))
        } else {
            None
        }
    }

    /// Region list and sequence share one selection. No sequence on this
    /// structure is `undef`: list focus stays, highlight clears.
    pub fn focus_region(&mut self, idx: usize, enter_select: bool) {
        let Some(region) = self
            .annotation
            .as_ref()
            .and_then(|a| a.editspec_regions.as_ref())
            .and_then(|regions| regions.get(idx))
            .cloned()
        else {
            return;
        };
        self.focused_region = idx;
        self.abandon_edit_form();

        let Some(chain_idx) = self
            .protein
            .chains
            .iter()
            .position(|c| c.id == region.chain)
        else {
            self.seq_selection.clear();
            self.sync_selection_overlay();
            self.status_banner = Some(format!(
                "Region {idx} chain {} is undef on this structure",
                region.chain
            ));
            return;
        };
        if chain_idx != self.current_chain {
            self.current_chain = chain_idx;
            if self.active_panel == ActivePanel::Interface {
                self.rebuild_interface_colors();
            }
        }

        let residues = self.current_residues();
        match Self::residue_indices_for_seq_range(residues, region.range[0], region.range[1]) {
            Some((start, end)) => {
                let max = residues.len();
                self.seq_selection.select_range(start, end, max);
                self.ensure_seq_visible(start);
                self.status_banner = None;
                if enter_select {
                    self.enter_select_mode();
                }
                self.sync_selection_overlay();
            }
            None => {
                self.seq_selection.clear();
                self.sync_selection_overlay();
                self.status_banner = Some(format!(
                    "Region {idx} {}:{}-{} is undef on this sequence",
                    region.chain, region.range[0], region.range[1]
                ));
            }
        }
    }

    pub fn ensure_seq_visible(&mut self, residue_idx: usize) {
        let (start_row, per) = self
            .seq_blocks
            .iter()
            .find(|b| b.chain_idx == self.current_chain)
            .map(|b| (b.start_row, b.per_line.max(1)))
            .unwrap_or((self.seq_line_row, self.seq_per_line.max(1)));
        if start_row == u16::MAX {
            return;
        }
        let row = start_row.saturating_add(((residue_idx / per) as u16).saturating_mul(3));
        let view_h = self
            .last_sidebar_rect
            .map(|r| r.height.saturating_sub(2))
            .unwrap_or(10);
        if row < self.panel_scroll {
            self.panel_scroll = row;
        } else if row.saturating_add(3) > self.panel_scroll.saturating_add(view_h) {
            self.panel_scroll = row.saturating_add(3).saturating_sub(view_h);
        }
    }

    pub fn sync_focused_region_from_selection(&mut self) {
        let Some((start, _)) = self.seq_selection.range() else {
            return;
        };
        if let Some(idx) = self.region_index_covering_residue(start) {
            self.focused_region = idx;
        }
    }

    pub fn region_index_covering_residue(&self, residue_idx: usize) -> Option<usize> {
        let chain = self.protein.chains.get(self.current_chain)?;
        let seq_num = chain.residues.get(residue_idx)?.seq_num as usize;
        self.annotation
            .as_ref()
            .and_then(|a| a.editspec_regions.as_ref())?
            .iter()
            .position(|r| r.chain == chain.id && r.range[0] <= seq_num && r.range[1] >= seq_num)
    }

    pub fn open_context_menu(&mut self, col: u16, row: u16, target: ContextTarget) {
        self.abandon_edit_form();
        self.shell.focus(PaneId::EditSpec);
        if matches!(
            self.shell.overlay,
            Overlay::Help | Overlay::RunComposer | Overlay::RunStatus | Overlay::BlockPalette
        ) {
            return;
        }
        if self.shell.overlay == Overlay::ContextMenu {
            self.shell.close_overlay();
        }
        let items = self.build_context_items(target);
        let cursor = items.iter().position(|i| i.selectable()).unwrap_or(0);
        self.context_menu = Some(ContextMenu {
            col,
            row,
            cursor,
            items,
            target,
            last_rect: Rect::default(),
        });
        self.shell.open_overlay(Overlay::ContextMenu);
    }

    fn build_context_items(&self, target: ContextTarget) -> Vec<ContextItem> {
        if matches!(target, ContextTarget::Workflow) {
            let mut items = vec![
                ContextItem::Header("图"),
                ContextItem::WorkflowAdd,
            ];
            if matches!(
                self.workflow_delete_allowed(),
                Some(
                    crate::workflow::WorkflowDeleteKind::Compose
                        | crate::workflow::WorkflowDeleteKind::Rfd
                        | crate::workflow::WorkflowDeleteKind::WholeLoop
                )
            ) {
                items.push(ContextItem::WorkflowDelete);
            }
            return items;
        }
        let mut items = vec![ContextItem::Header("Action")];
        for action in VALID_ACTIONS {
            items.push(ContextItem::Action(action));
        }
        items.push(ContextItem::Header("Label"));
        for label in PREDEFINED_LABELS {
            items.push(ContextItem::Label(label));
        }
        match target {
            ContextTarget::Region(_) => {
                items.push(ContextItem::Header("Region"));
                items.push(ContextItem::EditRange);
                items.push(ContextItem::Delete);
            }
            ContextTarget::Selection => {
                items.push(ContextItem::Header("Selection"));
                items.push(ContextItem::EditRange);
                if self.selection_has_overlap() {
                    items.push(ContextItem::ReplaceOverlap);
                }
            }
            ContextTarget::Workflow => {}
        }
        items
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
        if self.shell.overlay == Overlay::ContextMenu {
            self.shell.close_overlay();
        }
    }

    pub fn context_menu_move(&mut self, dir: i32) {
        let Some(menu) = self.context_menu.as_mut() else {
            return;
        };
        let n = menu.items.len() as i32;
        if n == 0 {
            return;
        }
        let mut i = menu.cursor as i32;
        for _ in 0..n {
            i = (i + dir).rem_euclid(n);
            if menu.items[i as usize].selectable() {
                menu.cursor = i as usize;
                return;
            }
        }
    }

    pub fn apply_context_menu(&mut self) {
        let Some(menu) = self.context_menu.clone() else {
            return;
        };
        let item = menu.items.get(menu.cursor).cloned();
        self.close_context_menu();
        if let Some(item) = item {
            self.apply_context_item(item, menu.target);
        }
    }

    fn apply_context_item(&mut self, item: ContextItem, target: ContextTarget) {
        match item {
            ContextItem::Header(_) => {}
            ContextItem::Action(name) => match target {
                ContextTarget::Workflow => {}
                ContextTarget::Selection => {
                    self.pending_overlap_action = Some(name.to_string());
                    if let Some(msg) = self.apply_action_shortcut(name) {
                        self.status_banner = Some(msg);
                    }
                    self.revalidate();
                }
                ContextTarget::Region(idx) => self.set_region_action(idx, name),
            },
            ContextItem::Label(name) => match target {
                ContextTarget::Workflow => {}
                ContextTarget::Selection => self.apply_label_to_selection(name),
                ContextTarget::Region(idx) => self.set_region_label(idx, name),
            },
            ContextItem::EditRange => match target {
                ContextTarget::Workflow => {}
                ContextTarget::Region(idx) => {
                    self.focused_region = idx;
                    self.edit_region_start();
                }
                ContextTarget::Selection => {
                    self.edit_region_open_from_selection();
                }
            },
            ContextItem::Delete => {
                if let ContextTarget::Region(idx) = target {
                    self.focused_region = idx;
                    self.delete_focused_region_now();
                }
            }
            ContextItem::WorkflowAdd => {
                self.open_block_palette();
            }
            ContextItem::WorkflowDelete => {
                self.request_workflow_delete();
            }
            ContextItem::ReplaceOverlap => {
                if let Some((chain, start, end)) = self.selection_seq_range() {
                    self.push_history("replace overlapping regions");
                    self.remove_overlapping_regions(&chain, start, end, None);
                    self.pending_overlap_action = None;
                    self.status_banner =
                        Some("Overlapping regions removed. Pick an Action or Label.".to_string());
                    self.revalidate();
                }
            }
        }
    }

    fn set_region_action(&mut self, idx: usize, action: &str) {
        let Some(regions) = self
            .annotation
            .as_ref()
            .and_then(|a| a.editspec_regions.as_ref())
        else {
            return;
        };
        if idx >= regions.len() {
            return;
        }
        self.push_history(&format!("set region {idx} action {action}"));
        if let Some(region) = self
            .annotation
            .as_mut()
            .and_then(|a| a.editspec_regions.as_mut())
            .and_then(|r| r.get_mut(idx))
        {
            region.action = action.to_string();
        }
        self.status_banner = Some(format!("Region {idx} → {action}"));
        self.revalidate();
    }

    fn set_region_label(&mut self, idx: usize, label: &str) {
        let Some(regions) = self
            .annotation
            .as_ref()
            .and_then(|a| a.editspec_regions.as_ref())
        else {
            return;
        };
        if idx >= regions.len() {
            return;
        }
        self.push_history(&format!("set region {idx} label {label}"));
        if let Some(region) = self
            .annotation
            .as_mut()
            .and_then(|a| a.editspec_regions.as_mut())
            .and_then(|r| r.get_mut(idx))
        {
            region.label = Some(label.to_string());
        }
        self.status_banner = Some(format!("Region {idx} label → {label}"));
        self.revalidate();
    }

    fn apply_label_to_selection(&mut self, label: &str) {
        let Some((chain, start, end)) = self.selection_seq_range() else {
            return;
        };
        if self.selection_has_overlap() {
            self.push_history(&format!("label {label} (replace overlap)"));
            self.remove_overlapping_regions(&chain, start, end, self.selection_exact_region());
        }
        self.pending_overlap_action = None;
        if self.apply_action_shortcut("edit").is_none() {
            return;
        }
        if let Some(idx) = self
            .annotation
            .as_ref()
            .and_then(|a| a.editspec_regions.as_ref())
            .and_then(|regions| {
                regions.iter().position(|r| {
                    r.chain == chain && r.range[0] == start && r.range[1] == end
                })
            })
        {
            if let Some(region) = self
                .annotation
                .as_mut()
                .and_then(|a| a.editspec_regions.as_mut())
                .and_then(|r| r.get_mut(idx))
            {
                region.label = Some(label.to_string());
            }
            self.status_banner = Some(format!("Selection labeled {label}"));
            self.revalidate();
        }
    }

    fn delete_focused_region_now(&mut self) {
        self.edit_state.delete_confirm = true;
        let _ = self.edit_region_delete();
        self.edit_state.delete_confirm = false;
        self.status_banner = Some("Region deleted".to_string());
    }

    pub fn can_run(&self) -> bool {
        let table_ok = self
            .workflow_status
            .as_ref()
            .map(|status| status.can_run)
            .unwrap_or(true);
        self.python_available && table_ok
    }

    pub fn workflow_move(&mut self, delta: isize) {
        let count = self
            .workflow_status
            .as_ref()
            .map(|status| status.nodes.len())
            .unwrap_or(0);
        if count == 0 {
            self.workflow_cursor = 0;
            return;
        }
        let next = self.workflow_cursor as isize + delta;
        self.workflow_cursor = next.clamp(0, count as isize - 1) as usize;
        self.workflow_delete_confirm = false;
        self.emit(
            EventLevel::Nav,
            EventPane::Workflow,
            "workflow.cursor",
            false,
            format!("wf cursor {}", self.workflow_cursor),
            None,
        );
    }

    pub fn focused_workflow_node(&self) -> Option<&crate::workflow::WorkflowNode> {
        self.workflow_status
            .as_ref()
            .and_then(|status| status.node(self.workflow_cursor))
    }

    pub fn handle_workflow_click(&mut self, col: u16, row: u16) -> Result<(), String> {
        let Some(rect) = self.last_workflow_rect else {
            self.workflow_drag = None;
            return Ok(());
        };
        let nodes = self
            .workflow_status
            .as_ref()
            .map(|status| status.nodes.as_slice())
            .unwrap_or(&[]);
        let Some(idx) = crate::ui::workflow_pane::hit_test(rect, col, row, nodes) else {
            self.workflow_drag = None;
            return Ok(());
        };
        self.workflow_cursor = idx;
        self.workflow_drag = Some(idx);
        self.workflow_delete_confirm = false;
        self.activate_workflow_node()
    }

    pub fn handle_workflow_drag(&mut self, col: u16, row: u16) {
        if self.workflow_drag.is_none() {
            return;
        }
        let Some(rect) = self.last_workflow_rect else {
            return;
        };
        let nodes = self
            .workflow_status
            .as_ref()
            .map(|status| status.nodes.as_slice())
            .unwrap_or(&[]);
        if let Some(idx) = crate::ui::workflow_pane::hit_test(rect, col, row, nodes) {
            self.workflow_cursor = idx;
        }
    }

    pub fn handle_workflow_drop(&mut self, col: u16, row: u16) {
        let Some(dragged) = self.workflow_drag.take() else {
            return;
        };
        let Some(rect) = self.last_workflow_rect else {
            return;
        };
        let nodes = self
            .workflow_status
            .as_ref()
            .map(|status| status.nodes.as_slice())
            .unwrap_or(&[])
            .to_vec();
        let Some(drop) = crate::ui::workflow_pane::hit_test(rect, col, row, &nodes) else {
            return;
        };
        if dragged == drop {
            return;
        }
        self.apply_workflow_rewire_drop(dragged, drop);
    }

    pub(crate) fn apply_workflow_rewire_drop(&mut self, dragged: usize, drop: usize) {
        let nodes = self
            .workflow_status
            .as_ref()
            .map(|status| status.nodes.as_slice())
            .unwrap_or(&[])
            .to_vec();
        let Some(rewire) = crate::workflow::classify_workflow_rewire(&nodes, dragged, drop) else {
            self.emit(
                EventLevel::Intent,
                EventPane::Workflow,
                "workflow.draft_error",
                false,
                "端口对不上，没改边",
                None,
            );
            return;
        };
        let revert = self.snapshot_workflow();
        self.ensure_workflow_graph();
        if let (Some(graph), Some(status)) = (
            self.workflow_graph.as_mut(),
            self.workflow_status.as_mut(),
        ) {
            crate::workflow::apply_workflow_rewire(graph, &mut status.nodes, &rewire);
            status.draft = true;
        } else {
            return;
        }
        self.workflow_cursor = dragged;
        match self.persist_workflow_draft() {
            Ok(()) => self.emit(
                EventLevel::Intent,
                EventPane::Workflow,
                "workflow.rewire",
                true,
                "已改 from · 草稿，未钉进 run.yaml",
                Some(revert),
            ),
            Err(err) => self.emit(
                EventLevel::Intent,
                EventPane::Workflow,
                "workflow.rewire",
                true,
                format!("已改 from · 未落盘: {err}"),
                Some(revert),
            ),
        }
    }

    pub fn handle_workflow_right_click(&mut self, col: u16, row: u16) {
        self.shell.focus(PaneId::Workflow);
        let Some(rect) = self.last_workflow_rect else {
            self.open_block_palette();
            return;
        };
        let nodes = self
            .workflow_status
            .as_ref()
            .map(|status| status.nodes.as_slice())
            .unwrap_or(&[]);
        if let Some(idx) = crate::ui::workflow_pane::hit_test(rect, col, row, nodes) {
            self.workflow_cursor = idx;
            self.workflow_delete_confirm = false;
            self.open_workflow_context_menu(col, row);
            return;
        }
        self.open_block_palette();
    }

    fn open_workflow_context_menu(&mut self, col: u16, row: u16) {
        if matches!(
            self.shell.overlay,
            Overlay::Help | Overlay::RunComposer | Overlay::RunStatus | Overlay::BlockPalette
        ) {
            return;
        }
        if self.shell.overlay == Overlay::ContextMenu {
            self.shell.close_overlay();
        }
        let items = self.build_context_items(ContextTarget::Workflow);
        let cursor = items.iter().position(|item| item.selectable()).unwrap_or(0);
        self.context_menu = Some(ContextMenu {
            col,
            row,
            cursor,
            items,
            target: ContextTarget::Workflow,
            last_rect: Rect::default(),
        });
        self.shell.open_overlay(Overlay::ContextMenu);
    }

    pub fn activate_workflow_node(&mut self) -> Result<(), String> {
        let Some(node) = self.focused_workflow_node() else {
            return Ok(());
        };
        let Some(relative) = node.structure_path.clone() else {
            return Ok(());
        };
        let path = resolve_structure_path(self.campaign_root.as_deref(), &relative);
        let revert = self.snapshot_load();
        let protein = crate::parser::pdb::load_structure(path.to_str().unwrap_or_default())
            .map_err(|err| err.to_string())?;
        self.replace_protein(protein);
        self.loaded_structure_path = Some(path.display().to_string());
        self.emit(
            EventLevel::Intent,
            EventPane::Workflow,
            "workflow.load",
            true,
            format!("load {}", relative),
            Some(revert),
        );
        Ok(())
    }

    fn focus_workflow_from_tree(&mut self) {
        let Some((kind, condition)) = self
            .product_tree
            .visible_rows()
            .get(self.tree_cursor)
            .map(|(_depth, node)| (node.kind.clone(), node.condition_node().map(str::to_string)))
        else {
            return;
        };
        let Some(status) = self.workflow_status.as_ref() else {
            return;
        };
        if let Some(idx) = crate::workflow::workflow_index_for_tree_row(
            &status.nodes,
            &kind,
            condition.as_deref(),
        ) {
            self.workflow_cursor = idx;
        }
    }

    pub fn open_block_palette(&mut self) {
        if matches!(
            self.shell.overlay,
            Overlay::Help | Overlay::RunComposer | Overlay::RunStatus | Overlay::ContextMenu
        ) {
            return;
        }
        let nodes = self
            .workflow_status
            .as_ref()
            .map(|status| status.nodes.as_slice())
            .unwrap_or(&[]);
        self.workflow_delete_confirm = false;
        self.block_palette = Some(BlockPalette::from_nodes(nodes));
        self.shell.open_overlay(Overlay::BlockPalette);
    }

    pub fn close_block_palette(&mut self) {
        self.block_palette = None;
        if self.shell.overlay == Overlay::BlockPalette {
            self.shell.close_overlay();
        }
    }

    pub fn block_palette_move(&mut self, delta: isize) {
        if let Some(palette) = self.block_palette.as_mut() {
            palette.move_cursor(delta);
        }
    }

    pub fn apply_block_palette(&mut self) {
        let Some(block) = self
            .block_palette
            .as_ref()
            .and_then(|palette| palette.selected_block())
            .map(str::to_string)
        else {
            self.close_block_palette();
            return;
        };
        self.close_block_palette();
        let revert = self.snapshot_workflow();
        self.add_workflow_block(&block);
        self.emit(
            EventLevel::Intent,
            EventPane::Workflow,
            "workflow.add",
            true,
            format!("add {block}"),
            Some(revert),
        );
    }

    pub fn request_workflow_delete(&mut self) {
        match self.workflow_delete_allowed() {
            None => {
                self.workflow_delete_confirm = false;
                self.status_banner = Some("这个盒不能删".to_string());
            }
            Some(crate::workflow::WorkflowDeleteKind::ForbiddenRequiredStep) => {
                self.workflow_delete_confirm = false;
                self.status_banner = Some("mpnn/fold/evaluate 不能单拆，要拆就拆整圈".to_string());
            }
            Some(crate::workflow::WorkflowDeleteKind::ForbiddenLoopSeed) => {
                self.workflow_delete_confirm = false;
                self.status_banner = Some("loop 还在，不能删 from 种".to_string());
            }
            Some(_) if !self.workflow_delete_confirm => {
                self.workflow_delete_confirm = true;
                let name = self
                    .focused_workflow_node()
                    .map(|node| node.id.clone())
                    .unwrap_or_else(|| "节点".to_string());
                self.status_banner = Some(format!("再按 d 确认删除 {name}"));
            }
            Some(_) => {
                self.workflow_delete_confirm = false;
                self.apply_workflow_delete();
            }
        }
    }

    fn workflow_delete_allowed(&self) -> Option<crate::workflow::WorkflowDeleteKind> {
        let status = self.workflow_status.as_ref()?;
        let from = crate::workflow::loop_from_id(self.workflow_graph.as_ref());
        crate::workflow::classify_workflow_delete(
            &status.nodes,
            self.workflow_cursor,
            from.as_deref(),
        )
    }

    fn apply_workflow_delete(&mut self) {
        let revert = self.snapshot_workflow();
        self.ensure_workflow_graph();
        let Some(kind) = self.workflow_delete_allowed() else {
            return;
        };
        match kind {
            crate::workflow::WorkflowDeleteKind::WholeLoop => self.delete_whole_loop(),
            crate::workflow::WorkflowDeleteKind::Rfd => self.delete_rfd_step(),
            crate::workflow::WorkflowDeleteKind::Compose => self.delete_compose_node(),
            crate::workflow::WorkflowDeleteKind::ForbiddenRequiredStep
            | crate::workflow::WorkflowDeleteKind::ForbiddenLoopSeed => {}
        }
        if let Some(status) = self.workflow_status.as_mut() {
            status.draft = true;
            if self.workflow_cursor >= status.nodes.len() {
                self.workflow_cursor = status.nodes.len().saturating_sub(1);
            }
        }
        match self.persist_workflow_draft() {
            Ok(()) => self.emit(
                EventLevel::Intent,
                EventPane::Workflow,
                "workflow.delete",
                true,
                "已删 · 草稿，未钉进 run.yaml",
                Some(revert),
            ),
            Err(err) => self.emit(
                EventLevel::Intent,
                EventPane::Workflow,
                "workflow.delete",
                true,
                format!("已删 · 未落盘: {err}"),
                Some(revert),
            ),
        }
    }

    fn add_workflow_block(&mut self, block: &str) {
        self.ensure_workflow_graph();
        if block == "loop" {
            self.add_loop();
        } else if block == "rfd" {
            self.add_rfd_step();
        } else {
            self.add_compose_block(block);
        }
        if let Some(status) = self.workflow_status.as_mut() {
            status.draft = true;
            status.can_run = false;
        }
        match self.persist_workflow_draft() {
            Ok(()) => {
                self.status_banner = Some(format!("已加 {block} · 草稿，未钉进 run.yaml"));
            }
            Err(err) => {
                self.status_banner = Some(format!("已加 {block} · 未落盘: {err}"));
            }
        }
    }

    fn ensure_workflow_graph(&mut self) {
        if self.workflow_graph.is_some() {
            return;
        }
        let Some(status) = self.workflow_status.as_ref() else {
            return;
        };
        let compose: Vec<serde_json::Value> = status
            .nodes
            .iter()
            .filter(|node| node.kind == "compose")
            .map(|node| {
                serde_json::json!({
                    "id": node.id,
                    "block": node.block,
                    "inputs": node.inputs,
                    "params": {},
                })
            })
            .collect();
        let loop_node = status.nodes.iter().find(|node| node.kind == "loop");
        let steps: Vec<String> = status
            .nodes
            .iter()
            .filter(|node| node.kind == "step")
            .map(|node| node.block.clone())
            .collect();
        let mut graph = serde_json::json!({ "compose": compose, "loop": null });
        if let Some(loop_node) = loop_node {
            let from = if loop_node.inputs.is_empty() {
                compose
                    .first()
                    .and_then(|node| node.get("id").cloned())
                    .map(|id| serde_json::json!([id]))
                    .unwrap_or_else(|| serde_json::json!([]))
            } else {
                serde_json::json!(loop_node.inputs)
            };
            graph["loop"] = serde_json::json!({
                "id": loop_node.id,
                "inputs": from,
                "rounds": loop_node.rounds.max(1),
                "steps": steps,
                "passthrough_rfd": !steps.iter().any(|step| step == "rfd"),
                "edit_spec": status.edit_spec,
                "gate": {"mode": "human"},
            });
        }
        self.workflow_graph = Some(graph);
    }

    fn next_compose_id(&self, block: &str) -> String {
        let used: Vec<String> = self
            .workflow_status
            .as_ref()
            .map(|status| {
                status
                    .nodes
                    .iter()
                    .filter(|node| node.kind == "compose")
                    .map(|node| node.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        if !used.iter().any(|id| id == block) {
            return block.to_string();
        }
        for index in 2..99 {
            let candidate = format!("{block}-{index}");
            if !used.iter().any(|id| id == &candidate) {
                return candidate;
            }
        }
        format!("{block}-x")
    }

    fn add_compose_block(&mut self, block: &str) {
        let id = self.next_compose_id(block);
        if let Some(graph) = self.workflow_graph.as_mut() {
            if let Some(compose) = graph.get_mut("compose").and_then(|value| value.as_array_mut()) {
                compose.push(serde_json::json!({
                    "id": id,
                    "block": block,
                    "inputs": [],
                    "params": {},
                }));
            }
        }
        let insert_at = self
            .workflow_status
            .as_ref()
            .and_then(|status| status.nodes.iter().position(|node| node.kind != "compose"))
            .unwrap_or_else(|| {
                self.workflow_status
                    .as_ref()
                    .map(|status| status.nodes.len())
                    .unwrap_or(0)
            });
        if let Some(status) = self.workflow_status.as_mut() {
            status
                .nodes
                .insert(insert_at, crate::workflow::compose_draft_node(&id, block));
            self.workflow_cursor = insert_at;
        }
    }

    fn add_loop(&mut self) {
        if self
            .workflow_status
            .as_ref()
            .is_some_and(|status| status.nodes.iter().any(|node| node.kind == "loop"))
        {
            self.status_banner = Some("已经有一颗 loop".to_string());
            return;
        }
        let from = self.workflow_status.as_ref().and_then(|status| {
            status
                .nodes
                .iter()
                .find(|node| node.kind == "compose")
                .map(|node| node.id.clone())
        });
        if let Some(graph) = self.workflow_graph.as_mut() {
            graph["loop"] = serde_json::json!({
                "id": "optimize",
                "inputs": from.as_ref().map(|id| vec![id.clone()]).unwrap_or_default(),
                "rounds": 1,
                "steps": ["mpnn", "fold", "evaluate"],
                "passthrough_rfd": true,
                "edit_spec": "",
                "gate": {"mode": "human"},
            });
        }
        if let Some(status) = self.workflow_status.as_mut() {
            let insert_at = status.nodes.len();
            let mut loop_node = crate::workflow::loop_draft_node("optimize", 1);
            if let Some(from) = from.as_ref() {
                loop_node.inputs = vec![from.clone()];
            }
            status.nodes.push(loop_node);
            status.nodes.push(crate::workflow::step_draft_node("optimize", "mpnn"));
            status.nodes.push(crate::workflow::step_draft_node("optimize", "fold"));
            status.nodes.push(crate::workflow::step_draft_node("optimize", "evaluate"));
            status.nodes.push(crate::workflow::gate_draft_node("optimize"));
            self.workflow_cursor = insert_at;
        }
    }

    fn delete_whole_loop(&mut self) {
        if let Some(graph) = self.workflow_graph.as_mut() {
            graph["loop"] = serde_json::Value::Null;
            if graph.get("optimize").is_some() {
                graph["optimize"] = serde_json::Value::Null;
            }
        }
        if let Some(status) = self.workflow_status.as_mut() {
            status.nodes.retain(|node| {
                node.kind != "loop" && node.kind != "step" && node.kind != "gate"
            });
        }
    }

    fn delete_rfd_step(&mut self) {
        if let Some(graph) = self.workflow_graph.as_mut() {
            for key in ["loop", "optimize"] {
                if let Some(steps) = graph
                    .get_mut(key)
                    .and_then(|value| value.get_mut("steps"))
                    .and_then(|value| value.as_array_mut())
                {
                    steps.retain(|step| step.as_str() != Some("rfd"));
                    graph[key]["passthrough_rfd"] = serde_json::json!(true);
                }
            }
        }
        if let Some(status) = self.workflow_status.as_mut() {
            status
                .nodes
                .retain(|node| !(node.kind == "step" && node.block == "rfd"));
        }
    }

    fn delete_compose_node(&mut self) {
        let Some(id) = self.focused_workflow_node().map(|node| node.id.clone()) else {
            return;
        };
        if let Some(graph) = self.workflow_graph.as_mut() {
            if let Some(compose) = graph.get_mut("compose").and_then(|value| value.as_array_mut()) {
                compose.retain(|node| node.get("id").and_then(|value| value.as_str()) != Some(id.as_str()));
            }
        }
        if let (Some(graph), Some(status)) = (
            self.workflow_graph.as_mut(),
            self.workflow_status.as_mut(),
        ) {
            crate::workflow::detach_compose_source(graph, &mut status.nodes, &id);
        } else if let Some(status) = self.workflow_status.as_mut() {
            for node in status.nodes.iter_mut() {
                if node.kind == "compose" {
                    node.inputs.retain(|item| item != &id);
                }
            }
        }
        if let Some(status) = self.workflow_status.as_mut() {
            status.nodes.retain(|node| node.id != id);
        }
    }

    fn add_rfd_step(&mut self) {
        if let Some(graph) = self.workflow_graph.as_mut() {
            for key in ["loop", "optimize"] {
                if let Some(steps) = graph
                    .get_mut(key)
                    .and_then(|value| value.get_mut("steps"))
                    .and_then(|value| value.as_array_mut())
                {
                    if !steps.iter().any(|step| step.as_str() == Some("rfd")) {
                        steps.insert(0, serde_json::json!("rfd"));
                    }
                    graph[key]["passthrough_rfd"] = serde_json::json!(false);
                }
            }
        }
        let loop_id = self
            .workflow_status
            .as_ref()
            .and_then(|status| {
                status
                    .nodes
                    .iter()
                    .find(|node| node.kind == "loop")
                    .map(|node| node.id.clone())
            })
            .unwrap_or_else(|| "optimize".to_string());
        let insert_at = self
            .workflow_status
            .as_ref()
            .and_then(|status| status.nodes.iter().position(|node| node.kind == "step"))
            .unwrap_or_else(|| {
                self.workflow_status
                    .as_ref()
                    .map(|status| status.nodes.len())
                    .unwrap_or(0)
            });
        if let Some(status) = self.workflow_status.as_mut() {
            if status
                .nodes
                .iter()
                .any(|node| node.kind == "step" && node.block == "rfd")
            {
                return;
            }
            status
                .nodes
                .insert(insert_at, crate::workflow::rfd_draft_node(&loop_id));
            self.workflow_cursor = insert_at;
        }
    }

    fn persist_workflow_draft(&self) -> Result<(), String> {
        let root = self.campaign_root.as_deref().ok_or("no campaign")?;
        let graph = self.workflow_graph.as_ref().ok_or("no graph")?;
        let recipe = crate::workflow::recipe_from_graph_dump(graph);
        let path = std::path::Path::new(root).join("workflow_draft.json");
        let text = serde_json::to_string_pretty(&recipe).map_err(|err| err.to_string())?;
        std::fs::write(path, text).map_err(|err| err.to_string())
    }

    pub fn enter_run_mode(&mut self) {
        if self.shell.mode == InteractionMode::EditRegion {
            return;
        }
        self.shell.open_overlay(Overlay::RunComposer);
        self.emit(
            EventLevel::Session,
            EventPane::None,
            "session.overlay_open",
            false,
            "composer",
            None,
        );
    }

    pub fn toggle_help_overlay(&mut self) {
        match self.shell.overlay {
            Overlay::None => {
                self.shell.open_overlay(Overlay::Help);
                self.emit(
                    EventLevel::Session,
                    EventPane::None,
                    "session.overlay_open",
                    false,
                    "help",
                    None,
                );
            }
            Overlay::Help => {
                self.shell.close_overlay();
                self.emit(
                    EventLevel::Session,
                    EventPane::None,
                    "session.overlay_close",
                    false,
                    "help",
                    None,
                );
            }
            Overlay::ContextMenu => {
                self.close_context_menu();
                self.shell.open_overlay(Overlay::Help);
                self.emit(
                    EventLevel::Session,
                    EventPane::None,
                    "session.overlay_open",
                    false,
                    "help",
                    None,
                );
            }
            Overlay::BlockPalette => {
                self.close_block_palette();
                self.shell.open_overlay(Overlay::Help);
                self.emit(
                    EventLevel::Session,
                    EventPane::None,
                    "session.overlay_open",
                    false,
                    "help",
                    None,
                );
            }
            Overlay::RunComposer | Overlay::RunStatus => {}
        }
    }

    pub fn close_help_overlay(&mut self) {
        if self.shell.overlay == Overlay::Help {
            self.shell.close_overlay();
            self.emit(
                EventLevel::Session,
                EventPane::None,
                "session.overlay_close",
                false,
                "help",
                None,
            );
        }
    }

    pub fn close_run_overlay(&mut self) {
        if matches!(self.shell.overlay, Overlay::RunComposer | Overlay::RunStatus) {
            self.shell.close_overlay();
            self.emit(
                EventLevel::Session,
                EventPane::None,
                "session.overlay_close",
                false,
                "composer",
                None,
            );
        }
    }

    /// Leave Select (keep range) or cancel EditRegion, then focus the clicked pane.
    /// Click-away from a form must Idle the pane machine; do not restore Select.
    pub fn apply_view_drag(&mut self, col: u16, row: u16) {
        if let Some((last_col, last_row)) = self.view_drag_last {
            let dx = col as i32 - last_col as i32;
            let dy = row as i32 - last_row as i32;
            if dx != 0 {
                self.camera.rotate_y(dx as f64);
            }
            if dy != 0 {
                self.camera.rotate_x(dy as f64);
            }
            if dx != 0 || dy != 0 {
                self.emit_view_camera();
            }
        }
        self.view_drag_last = Some((col, row));
    }

    pub fn pointer_focus_pane(&mut self, pane: PaneId) {
        if self.shell.overlay.is_open() {
            return;
        }
        let leaving_form = self.shell.mode == InteractionMode::EditRegion && pane != PaneId::EditSpec;
        if leaving_form {
            self.edit_state = EditState::default();
        }
        if leaving_form || self.shell.mode != InteractionMode::EditRegion {
            self.shell.pointer_leave_local_mode();
        }
        self.shell.focus(pane);
    }

    pub fn sync_selection_overlay(&mut self) {
        let overlay = self.selection_overlay_range();
        self.color_scheme.set_selection_overlay(overlay);
        self.mesh_dirty = true;
    }

    pub fn selection_overlay_range(&self) -> Option<(String, i32, i32)> {
        let (start, end) = self.seq_selection.range()?;
        let chain = self.protein.chains.get(self.current_chain)?;
        let start_num = chain.residues.get(start)?.seq_num;
        let end_num = chain.residues.get(end)?.seq_num;
        Some((chain.id.clone(), start_num.min(end_num), start_num.max(end_num)))
    }

    pub fn compact_edit_spec_string(&self) -> Option<String> {
        let regions = self.annotation.as_ref()?.editspec_regions.as_ref()?;
        if regions.is_empty() {
            return None;
        }
        let tuples: Vec<(String, usize, usize, String)> = regions
            .iter()
            .map(|r| (r.chain.clone(), r.range[0], r.range[1], r.action.clone()))
            .collect();
        Some(shell::compact_edit_spec(&tuples))
    }

    pub fn load_edit_spec_text(&mut self, spec: &str) {
        match parse_compact_regions(spec) {
            Ok(parsed) => {
                let regions = parsed
                    .into_iter()
                    .map(|(chain, start, end, action)| EditSpecRegion {
                        chain,
                        range: [start, end],
                        action,
                        label: None,
                    })
                    .collect();
                self.annotation = Some(Annotation {
                    editspec_regions: Some(regions),
                    iteration: None,
                    highlights: None,
                });
                self.revalidate();
            }
            Err(e) => eprintln!("Warning: failed to parse --edit '{spec}': {e}"),
        }
    }

    pub fn current_residues(&self) -> &[crate::model::protein::Residue] {
        self.protein
            .chains
            .get(self.current_chain)
            .map(|c| c.residues.as_slice())
            .unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::protein::{Atom, Residue, SecondaryStructure};

    fn residue(seq: i32, ss: SecondaryStructure) -> Residue {
        Residue {
            name: "ALA".to_string(),
            seq_num: seq,
            atoms: vec![Atom {
                name: "CA".to_string(),
                element: "C".to_string(),
                x: 0.0,
                y: 0.0,
                z: 0.0,
                b_factor: 0.0,
                is_backbone: true,
                is_hetero: false,
            }],
            secondary_structure: ss,
        }
    }

    fn helix_sheet_residues() -> Vec<Residue> {
        let mut residues = Vec::new();
        for i in 1..=5 {
            residues.push(residue(i, SecondaryStructure::Helix));
        }
        for i in 6..=8 {
            residues.push(residue(i, SecondaryStructure::Sheet));
        }
        residues
    }

    #[test]
    fn click_selects_that_residue_not_whole_ss_segment() {
        let residues = helix_sheet_residues();
        let mut sel = SeqSelection::default();
        sel.click(&residues, 2);
        assert_eq!(sel.range(), Some((2, 2)));
        assert!(sel.active);
        assert_ne!(sel.range(), Some((0, 4)));
    }

    #[test]
    fn select_segment_covers_contiguous_ss() {
        let residues = helix_sheet_residues();
        let mut sel = SeqSelection::default();
        sel.select_segment(&residues, 2);
        assert_eq!(sel.range(), Some((0, 4)));
    }

    #[test]
    fn drag_selects_arbitrary_range() {
        let residues = helix_sheet_residues();
        let mut sel = SeqSelection::default();
        sel.click(&residues, 1);
        sel.drag_to(6, residues.len());
        assert_eq!(sel.range(), Some((1, 6)));
    }

    #[test]
    fn select_range_covers_indices() {
        let mut sel = SeqSelection::default();
        sel.select_range(1, 6, 8);
        assert_eq!(sel.range(), Some((1, 6)));
        assert_eq!(sel.cursor, 1);
    }

    #[test]
    fn seq_range_maps_to_residue_indices_or_undef() {
        let residues = helix_sheet_residues();
        assert_eq!(
            App::residue_indices_for_seq_range(&residues, 1, 10),
            Some((0, 7))
        );
        assert_eq!(
            App::residue_indices_for_seq_range(&residues, 6, 8),
            Some((5, 7))
        );
        assert_eq!(App::residue_indices_for_seq_range(&residues, 100, 110), None);
    }

    #[test]
    fn jump_segment_moves_to_next_ss() {
        let residues = helix_sheet_residues();
        let mut sel = SeqSelection::default();
        sel.cursor = 0;
        sel.jump_segment(&residues, 1);
        assert_eq!(sel.range(), Some((5, 7)));
        sel.jump_segment(&residues, -1);
        assert_eq!(sel.range(), Some((0, 4)));
    }

    fn test_app() -> App {
        use crate::model::protein::{Chain, MoleculeType};
        let protein = Protein {
            name: "t".to_string(),
            chains: vec![Chain {
                id: "A".to_string(),
                residues: helix_sheet_residues(),
                molecule_type: MoleculeType::Protein,
            }],
            ligands: Vec::new(),
        };
        App::new(
            protein,
            AppConfig {
                render_mode: RenderMode::HalfBlock,
                viz_mode: VizMode::Backbone,
                user_explicit_mode: true,
                color_override: None,
            },
            80,
            24,
            ratatui_image::picker::Picker::halfblocks(),
        )
    }

    fn seed_rewire_app(root: &std::path::Path) -> App {
        let mut app = test_app();
        app.campaign_root = Some(root.display().to_string());
        let mut loop_node = crate::workflow::loop_draft_node("optimize", 2);
        loop_node.inputs = vec!["seed".to_string()];
        app.workflow_status = Some(WorkflowStatus {
            nodes: vec![
                crate::workflow::compose_draft_node("seed", "import"),
                crate::workflow::compose_draft_node("extra", "import"),
                loop_node,
                crate::workflow::step_draft_node("optimize", "mpnn"),
            ],
            can_run: true,
            edit_spec: String::new(),
            draft: false,
        });
        app.workflow_graph = Some(serde_json::json!({
            "compose": [
                {"id": "seed", "block": "import", "inputs": []},
                {"id": "extra", "block": "import", "inputs": []}
            ],
            "loop": {
                "id": "optimize",
                "inputs": ["seed"],
                "rounds": 2,
                "steps": ["mpnn"]
            }
        }));
        app
    }

    #[test]
    fn undo_tui_rewire_restores_graph_not_attempts() {
        let root = std::env::temp_dir().join(format!(
            "pv-undo-tui-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("outputs/attempt")).unwrap();
        let attempt = root.join("outputs/attempt/keep.txt");
        std::fs::write(&attempt, "untouched").unwrap();
        std::fs::write(
            root.join("console.jsonl"),
            "{\"event\":\"waiting\",\"stage\":\"fold\",\"message\":\"waiting fold … (3s)\"}\n",
        )
        .unwrap();

        let mut app = seed_rewire_app(&root);
        app.poll_console_hint();
        let waiting = app
            .event_log
            .events()
            .iter()
            .find(|event| event.op == "run.waiting")
            .expect("waiting projected");
        assert!(!waiting.undoable);
        assert_eq!(waiting.level, EventLevel::Run);

        app.apply_workflow_rewire_drop(2, 1);
        assert_eq!(
            app.workflow_graph.as_ref().unwrap()["loop"]["inputs"][0],
            "extra"
        );
        assert!(
            app.event_log
                .events()
                .iter()
                .any(|event| event.op == "workflow.rewire" && event.undoable)
        );

        app.session_undo();
        assert_eq!(
            app.workflow_graph.as_ref().unwrap()["loop"]["inputs"][0],
            "seed"
        );
        assert_eq!(std::fs::read_to_string(&attempt).unwrap(), "untouched");
        assert!(
            app.event_log
                .events()
                .iter()
                .any(|event| event.op == "run.waiting" && !event.undoable)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn undo_tui_style_restores_color_before_rewire() {
        let root = std::env::temp_dir().join(format!(
            "pv-undo-style-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut app = seed_rewire_app(&root);
        let before = app.color_scheme.scheme_type;
        app.apply_workflow_rewire_drop(2, 1);
        app.cycle_color();
        assert_ne!(app.color_scheme.scheme_type, before);
        assert_eq!(
            app.workflow_graph.as_ref().unwrap()["loop"]["inputs"][0],
            "extra"
        );
        app.session_undo();
        assert_eq!(app.color_scheme.scheme_type, before);
        assert_eq!(
            app.workflow_graph.as_ref().unwrap()["loop"]["inputs"][0],
            "extra"
        );
        app.session_undo();
        assert_eq!(
            app.workflow_graph.as_ref().unwrap()["loop"]["inputs"][0],
            "seed"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn default_visible_levels_hide_view_camera() {
        let mut app = test_app();
        app.emit_session_focus();
        app.emit_view_camera();
        app.cycle_color();
        let hidden = app.event_log.visible(false);
        assert!(hidden.iter().any(|event| event.op == "session.focus"));
        assert!(hidden.iter().any(|event| event.op == "view.style"));
        assert!(hidden.iter().all(|event| event.op != "view.camera"));
        let shown = app.event_log.visible(true);
        assert!(shown.iter().any(|event| event.op == "view.camera"));
    }
}
