//! Studio four-pane chrome and exclusive-focus key router.
//!
//! ProteinView is the 3D engine; this module is the Studio session chrome:
//! Overlay (Help | RunComposer | RunStatus) → EditSpec Select/EditRegion →
//! focused-pane Idle → session chrome. Workflow stays an empty shell.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Named chrome panes. Tab cycles exclusive focus among these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneId {
    Workflow,
    Tree,
    View,
    EditSpec,
}

impl PaneId {
    pub const ALL: [PaneId; 4] = [
        PaneId::Workflow,
        PaneId::Tree,
        PaneId::View,
        PaneId::EditSpec,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Workflow => "Workflow",
            Self::Tree => "Tree",
            Self::View => "View",
            Self::EditSpec => "EditSpec",
        }
    }

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&p| p == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|&p| p == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Session overlay. Depth is 0 or 1; Help, Composer, and ContextMenu never stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
    RunComposer,
    #[allow(dead_code)]
    RunStatus,
    ContextMenu,
}

impl Overlay {
    pub fn is_open(self) -> bool {
        self != Self::None
    }
}

/// Pane-local mode. Idle means no Select/EditRegion on the focused pane.
/// Session idle is overlay None AND this Idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionMode {
    Idle,
    Select,
    EditRegion,
}

impl InteractionMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Select => "Select",
            Self::EditRegion => "EditRegion",
        }
    }
}

/// Testable action produced by the key router. `main` applies these to `App`.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    Quit,
    CyclePaneNext,
    CyclePanePrev,
    ToggleCollapse,
    EnterSelect,
    EnterRun,
    OpenEmptyForm,
    EditFocusedRegion,
    ExitSelectKeepThenCycleNext,
    ExitSelectKeepThenCyclePrev,
    ClearSelection,
    CloseOverlay,
    CloseHelp,
    ToggleHelp,
    RotateX(f64),
    RotateY(f64),
    RotateZ(f64),
    Pan(f64, f64),
    ZoomIn,
    ZoomOut,
    ResetCamera,
    CycleColor,
    CycleViz,
    ToggleHd,
    ToggleFullHd,
    PrevChain,
    NextChain,
    ToggleAutoRotate,
    ToggleInterface,
    ToggleInteractions,
    ToggleLigands,
    RegionNext,
    RegionPrev,
    RegionAdd,
    RegionDelete,
    RegionSplit,
    Undo,
    Redo,
    SeqCursor(i32),
    SeqExpandStart(i32),
    SeqExpandEnd(i32),
    SeqSelectSegment,
    SeqJumpSegment(i32),
    SeqYankRange,
    SeqYankLetters,
    SeqActionShortcut(&'static str),
    EditFormTab,
    EditFormBackTab,
    EditFormNextField,
    EditFormPrevField,
    EditFormAdjust(i32),
    EditFormSave,
    EditFormCancel,
    EditFormBackspace,
    EditFormChar(char),
    ContextMenuPrev,
    ContextMenuNext,
    ContextMenuApply,
    CloseContextMenu,
    RunIgnore,
    TreeNext,
    TreePrev,
    TreeCollapse,
    TreeExpand,
    TreeActivate,
    Ignore,
}

/// Exclusive-focus chrome: four panes, one pane mode, one overlay.
#[derive(Debug, Clone)]
pub struct Shell {
    pub focused: PaneId,
    pub expanded: [bool; 4],
    pub mode: InteractionMode,
    pub previous_mode: InteractionMode,
    pub overlay: Overlay,
    pub workflow_h: u16,
    pub tree_w: u16,
    pub editspec_w: u16,
    pub tree_h: u16,
    pub editspec_h: u16,
}

impl Default for Shell {
    fn default() -> Self {
        Self::pdb_session()
    }
}

impl Shell {
    /// PDB / `gemlib studio <pdb>` default: View + EditSpec open, Workflow and Tree collapsed.
    pub fn pdb_session() -> Self {
        Self {
            focused: PaneId::View,
            expanded: [false, false, true, true],
            mode: InteractionMode::Idle,
            previous_mode: InteractionMode::Idle,
            overlay: Overlay::None,
            workflow_h: 6,
            tree_w: 22,
            editspec_w: 48,
            tree_h: 8,
            editspec_h: 14,
        }
    }

    /// Campaign directory: land on the product tree. View stays open for row→3D.
    pub fn campaign_session() -> Self {
        Self {
            focused: PaneId::Tree,
            expanded: [false, true, true, true],
            mode: InteractionMode::Idle,
            previous_mode: InteractionMode::Idle,
            overlay: Overlay::None,
            workflow_h: 6,
            tree_w: 22,
            editspec_w: 48,
            tree_h: 8,
            editspec_h: 14,
        }
    }

    pub fn is_expanded(&self, pane: PaneId) -> bool {
        self.expanded[pane_index(pane)]
    }

    pub fn set_expanded(&mut self, pane: PaneId, expanded: bool) {
        self.expanded[pane_index(pane)] = expanded;
    }

    pub fn toggle_collapse(&mut self) {
        self.toggle_pane(self.focused);
    }

    pub fn toggle_pane(&mut self, pane: PaneId) {
        let idx = pane_index(pane);
        self.expanded[idx] = !self.expanded[idx];
    }

    pub fn cycle_focus_next(&mut self) {
        self.focused = self.focused.next();
    }

    pub fn cycle_focus_prev(&mut self) {
        self.focused = self.focused.prev();
    }

    pub fn focus(&mut self, pane: PaneId) {
        self.focused = pane;
    }

    pub fn enter_mode(&mut self, mode: InteractionMode) {
        if self.mode != mode {
            self.previous_mode = self.mode;
            self.mode = mode;
        }
    }

    pub fn restore_previous_mode(&mut self) {
        let previous = self.previous_mode;
        self.mode = if previous == InteractionMode::EditRegion {
            InteractionMode::Idle
        } else {
            previous
        };
        self.previous_mode = InteractionMode::Idle;
    }

    pub fn enter_idle(&mut self) {
        self.mode = InteractionMode::Idle;
        self.previous_mode = InteractionMode::Idle;
    }

    pub fn session_idle(&self) -> bool {
        self.overlay == Overlay::None && self.mode == InteractionMode::Idle
    }

    pub fn open_overlay(&mut self, overlay: Overlay) {
        if self.overlay == Overlay::None && overlay != Overlay::None {
            self.overlay = overlay;
        }
    }

    pub fn close_overlay(&mut self) {
        self.overlay = Overlay::None;
    }

    /// Leave Select without clearing the residue range, then cycle panes.
    pub fn exit_select_keep_then_cycle(&mut self, next: bool) {
        if self.mode == InteractionMode::Select {
            self.enter_idle();
        }
        if next {
            self.cycle_focus_next();
        } else {
            self.cycle_focus_prev();
        }
    }

    /// Click another pane: Select / EditRegion → Idle (keep residue range).
    /// Form widget teardown is the caller's job; this must not restore Select.
    pub fn pointer_leave_local_mode(&mut self) {
        match self.mode {
            InteractionMode::Select | InteractionMode::EditRegion => self.enter_idle(),
            InteractionMode::Idle => {}
        }
    }

    pub fn editspec_focused(&self) -> bool {
        self.focused == PaneId::EditSpec
    }

    pub fn view_focused(&self) -> bool {
        self.focused == PaneId::View
    }

    pub fn tree_focused(&self) -> bool {
        self.focused == PaneId::Tree
    }
}

fn pane_index(pane: PaneId) -> usize {
    match pane {
        PaneId::Workflow => 0,
        PaneId::Tree => 1,
        PaneId::View => 2,
        PaneId::EditSpec => 3,
    }
}

/// Documented key table (overlay → pane mode → idle → session chrome).
pub const KEY_TABLE: &[(&str, &str)] = &[
    ("Tab / Shift+Tab", "Idle: cycle panes. Select: Idle (keep selection) then cycle. EditRegion: form field. Overlay: freeze"),
    ("f", "Collapse / expand the focused pane (idle only); click ▾/▸ does the same for that pane"),
    ("Mouse: ▾ / border", "Click a pane's top-right badge to fold it. Drag a shared border to resize"),
    ("q / Ctrl+C", "Quit only when idle (no overlay / form)"),
    ("?", "Help overlay; exclusive with Composer"),
    ("Ctrl+R", "Open Run Composer from any pane including Select; Ignore if Composer/Status or EditRegion is open"),
    ("h/l j/k w/a/s/d u/i [ ] v", "3D camera / viz (View pane Idle only; j/k rotate only when View is focused)"),
    ("x", "Enter Select (EditSpec Idle only)"),
    ("Enter", "EditSpec Idle: empty form (or prefilled if a selection remains). Tree: load structure. Workflow / View: Ignore. Select: operate on the range"),
    ("e", "Edit the focused existing region (EditSpec only)"),
    ("Tree: j/k h/l Enter", "Move / collapse / expand / load structure_path into View"),
    ("Tree mouse", "▾/▸ toggles children only. Label selects; loads View only when structure_path exists. Wheel scrolls. One Line = one row"),
    ("Select: h/l H/L s [ ] 1-5", "Sequence cursor / boundary / segment / action shortcuts"),
    ("Right-click", "Selection or existing region: Action / Label menu"),
    ("Select: Esc", "Clear selection and return EditSpec Idle (one Esc)"),
    ("Select: Tab", "Idle (keep selection) then cycle pane"),
    ("EditRegion: form keys", "Type range (A51-80 / A:51-80 / 51-80); Tab is a field; Esc cancels"),
    ("Run Composer: Esc", "Close overlay; Ctrl+R does not stack"),
];

/// Single key router. Priority: overlay → EditRegion → Select → pane Idle → chrome.
pub fn route_key(shell: &Shell, key: KeyEvent, has_selection: bool, can_run: bool) -> KeyAction {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match shell.overlay {
        Overlay::Help => {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('?') => KeyAction::CloseHelp,
                _ => KeyAction::Ignore,
            };
        }
        Overlay::RunComposer | Overlay::RunStatus => return route_run(key, ctrl),
        Overlay::ContextMenu => return route_context_menu(key),
        Overlay::None => {}
    }

    match shell.mode {
        InteractionMode::EditRegion => route_edit_region(key, ctrl),
        InteractionMode::Select => route_select(key, ctrl, can_run),
        InteractionMode::Idle => route_idle(shell, key, ctrl, has_selection, can_run),
    }
}

fn route_run(key: KeyEvent, ctrl: bool) -> KeyAction {
    match key.code {
        KeyCode::Esc => KeyAction::CloseOverlay,
        KeyCode::Char('r') if ctrl => KeyAction::RunIgnore,
        _ => KeyAction::RunIgnore,
    }
}

fn route_context_menu(key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Esc => KeyAction::CloseContextMenu,
        KeyCode::Enter => KeyAction::ContextMenuApply,
        KeyCode::Up | KeyCode::Char('k') => KeyAction::ContextMenuPrev,
        KeyCode::Down | KeyCode::Char('j') => KeyAction::ContextMenuNext,
        _ => KeyAction::Ignore,
    }
}

fn route_edit_region(key: KeyEvent, ctrl: bool) -> KeyAction {
    if key.code == KeyCode::Char('r') && ctrl {
        return KeyAction::Ignore;
    }
    match key.code {
        KeyCode::Esc => KeyAction::EditFormCancel,
        KeyCode::Enter => KeyAction::EditFormSave,
        KeyCode::BackTab => KeyAction::EditFormBackTab,
        KeyCode::Tab => KeyAction::EditFormTab,
        KeyCode::Char('j') => KeyAction::EditFormNextField,
        KeyCode::Char('k') => KeyAction::EditFormPrevField,
        KeyCode::Char('+') | KeyCode::Char('=') => KeyAction::EditFormAdjust(1),
        KeyCode::Char('-') => KeyAction::EditFormAdjust(-1),
        KeyCode::Backspace => KeyAction::EditFormBackspace,
        KeyCode::Char(ch) => KeyAction::EditFormChar(ch),
        _ => KeyAction::Ignore,
    }
}

fn route_select(key: KeyEvent, ctrl: bool, can_run: bool) -> KeyAction {
    if matches!(key.code, KeyCode::Tab) {
        return KeyAction::ExitSelectKeepThenCycleNext;
    }
    if matches!(key.code, KeyCode::BackTab) {
        return KeyAction::ExitSelectKeepThenCyclePrev;
    }
    if key.code == KeyCode::Char('r') && ctrl {
        return if can_run {
            KeyAction::EnterRun
        } else {
            KeyAction::Ignore
        };
    }
    match key.code {
        KeyCode::Esc => KeyAction::ClearSelection,
        KeyCode::Left | KeyCode::Char('h') => KeyAction::SeqCursor(-1),
        KeyCode::Right | KeyCode::Char('l') => KeyAction::SeqCursor(1),
        KeyCode::Char('H') => KeyAction::SeqExpandStart(1),
        KeyCode::Char('L') => KeyAction::SeqExpandEnd(1),
        KeyCode::Char('s') => KeyAction::SeqSelectSegment,
        KeyCode::Char('[') => KeyAction::SeqJumpSegment(-1),
        KeyCode::Char(']') => KeyAction::SeqJumpSegment(1),
        KeyCode::Char('y') => KeyAction::SeqYankRange,
        KeyCode::Char('Y') => KeyAction::SeqYankLetters,
        KeyCode::Char('1') => KeyAction::SeqActionShortcut("keep"),
        KeyCode::Char('2') => KeyAction::SeqActionShortcut("edit"),
        KeyCode::Char('3') => KeyAction::SeqActionShortcut("replace"),
        KeyCode::Char('4') => KeyAction::SeqActionShortcut("insert"),
        KeyCode::Char('5') => KeyAction::SeqActionShortcut("delete"),
        KeyCode::Enter => KeyAction::OpenEmptyForm,
        KeyCode::Char('e') => KeyAction::EditFocusedRegion,
        _ => KeyAction::Ignore,
    }
}

fn route_idle(shell: &Shell, key: KeyEvent, ctrl: bool, has_selection: bool, can_run: bool) -> KeyAction {
    if key.code == KeyCode::Char('r') && ctrl {
        return if can_run {
            KeyAction::EnterRun
        } else {
            KeyAction::Ignore
        };
    }
    match key.code {
        KeyCode::Tab => KeyAction::CyclePaneNext,
        KeyCode::BackTab => KeyAction::CyclePanePrev,
        KeyCode::Char('f') => KeyAction::ToggleCollapse,
        KeyCode::Char('q') => KeyAction::Quit,
        KeyCode::Char('c') if ctrl => KeyAction::Quit,
        KeyCode::Char('x') if shell.editspec_focused() => KeyAction::EnterSelect,
        KeyCode::Enter if shell.tree_focused() => KeyAction::TreeActivate,
        KeyCode::Enter if shell.editspec_focused() => KeyAction::OpenEmptyForm,
        KeyCode::Char('e') if shell.editspec_focused() => KeyAction::EditFocusedRegion,
        KeyCode::Esc if has_selection && shell.editspec_focused() => KeyAction::ClearSelection,
        KeyCode::Char('?') => KeyAction::ToggleHelp,
        KeyCode::Char('j') | KeyCode::Down => {
            if shell.tree_focused() {
                KeyAction::TreeNext
            } else if shell.editspec_focused() {
                KeyAction::RegionNext
            } else if shell.view_focused() {
                KeyAction::RotateX(1.0)
            } else {
                KeyAction::Ignore
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if shell.tree_focused() {
                KeyAction::TreePrev
            } else if shell.editspec_focused() {
                KeyAction::RegionPrev
            } else if shell.view_focused() {
                KeyAction::RotateX(-1.0)
            } else {
                KeyAction::Ignore
            }
        }
        KeyCode::Char('h') | KeyCode::Left => {
            if shell.tree_focused() {
                KeyAction::TreeCollapse
            } else if shell.view_focused() {
                KeyAction::RotateY(-1.0)
            } else {
                KeyAction::Ignore
            }
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if shell.tree_focused() {
                KeyAction::TreeExpand
            } else if shell.view_focused() {
                KeyAction::RotateY(1.0)
            } else {
                KeyAction::Ignore
            }
        }
        KeyCode::Char('u') => {
            if shell.editspec_focused() {
                KeyAction::Undo
            } else if shell.view_focused() {
                KeyAction::RotateZ(-1.0)
            } else {
                KeyAction::Ignore
            }
        }
        KeyCode::Char('U') if shell.editspec_focused() => KeyAction::Redo,
        KeyCode::Char('i') if shell.view_focused() => KeyAction::RotateZ(1.0),
        KeyCode::Char('w') if shell.view_focused() => KeyAction::Pan(0.0, 1.0),
        KeyCode::Char('s') if shell.editspec_focused() => KeyAction::RegionSplit,
        KeyCode::Char('s') if shell.view_focused() => KeyAction::Pan(0.0, -1.0),
        KeyCode::Char('a') if shell.editspec_focused() => KeyAction::RegionAdd,
        KeyCode::Char('a') if shell.view_focused() => KeyAction::Pan(-1.0, 0.0),
        KeyCode::Char('d') if shell.editspec_focused() => KeyAction::RegionDelete,
        KeyCode::Char('d') if shell.view_focused() => KeyAction::Pan(1.0, 0.0),
        KeyCode::Char('+') | KeyCode::Char('=') if shell.view_focused() => KeyAction::ZoomIn,
        KeyCode::Char('-') if shell.view_focused() => KeyAction::ZoomOut,
        KeyCode::Char('r') if shell.view_focused() => KeyAction::ResetCamera,
        KeyCode::Char('c') => KeyAction::CycleColor,
        KeyCode::Char('v') => KeyAction::CycleViz,
        KeyCode::Char('m') => KeyAction::ToggleHd,
        KeyCode::Char('M') => KeyAction::ToggleFullHd,
        KeyCode::Char('[') => KeyAction::PrevChain,
        KeyCode::Char(']') => KeyAction::NextChain,
        KeyCode::Char(' ') => KeyAction::ToggleAutoRotate,
        KeyCode::Char('F') => KeyAction::ToggleInterface,
        KeyCode::Char('I') => KeyAction::ToggleInteractions,
        KeyCode::Char('g') => KeyAction::ToggleLigands,
        _ => KeyAction::Ignore,
    }
}

/// Parse a typed EditRegion range. Accepts `A51-80`, `A:51-80`, or `51-80`.
pub fn parse_direct_range(input: &str, default_chain: &str) -> Result<(String, usize, usize), String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || !trimmed.chars().any(|c| c.is_ascii_digit()) {
        return Err("Direct range has no numbers".to_string());
    }

    let (chain, rest) = if let Some(rest) = trimmed.strip_prefix(|c: char| c.is_ascii_alphabetic()) {
        let chain = trimmed.chars().next().unwrap().to_ascii_uppercase().to_string();
        let rest = rest.strip_prefix(':').unwrap_or(rest);
        (chain, rest)
    } else if let Some((head, tail)) = trimmed.split_once(':') {
        if head.len() == 1 && head.chars().all(|c| c.is_ascii_alphabetic()) {
            (head.to_ascii_uppercase(), tail)
        } else {
            (default_chain.to_string(), trimmed)
        }
    } else {
        (default_chain.to_string(), trimmed)
    };

    let Some((start_s, end_s)) = rest.split_once('-') else {
        return Err(format!("Direct range start/end parse fails: '{trimmed}'"));
    };
    let start: usize = start_s
        .trim()
        .parse()
        .map_err(|_| format!("Direct range start/end parse fails: '{trimmed}'"))?;
    let end: usize = end_s
        .trim()
        .parse()
        .map_err(|_| format!("Direct range start/end parse fails: '{trimmed}'"))?;
    if start > end {
        return Err(format!("Range start is greater than end: {start} > {end}"));
    }
    if chain.is_empty() {
        return Err("Direct range has no chain".to_string());
    }
    Ok((chain, start, end))
}

/// Parse a compact EditSpec token list (`A1-50=, A51-80~`) into (chain, start, end, action).
pub fn parse_compact_regions(spec: &str) -> Result<Vec<(String, usize, usize, String)>, String> {
    let mut out = Vec::new();
    for raw in spec.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let (range_part, action) = split_action_suffix(token);
        let (chain, start, end) = parse_direct_range(range_part, "A")?;
        out.push((chain, start, end, action));
    }
    Ok(out)
}

fn split_action_suffix(token: &str) -> (&str, String) {
    if let Some(rest) = token.strip_suffix('=') {
        return (rest, "keep".to_string());
    }
    if let Some(rest) = token.strip_suffix('~') {
        return (rest, "edit".to_string());
    }
    if let Some(rest) = token.strip_suffix('+') {
        return (rest, "insert".to_string());
    }
    if let Some(rest) = token.strip_suffix('-') {
        if rest.chars().any(|c| c == '-') {
            return (rest, "delete".to_string());
        }
    }
    if let Some((range, _len)) = token.split_once('>') {
        return (range, "replace".to_string());
    }
    (token, "edit".to_string())
}

pub fn compact_edit_spec(regions: &[(String, usize, usize, String)]) -> String {
    regions
        .iter()
        .map(|(chain, start, end, action)| {
            let sym = match action.as_str() {
                "keep" => "=",
                "edit" => "~",
                "replace" => ">",
                "insert" => "+",
                "delete" => "-",
                other => return format!("{chain}{start}-{end}{other}"),
            };
            format!("{chain}{start}-{end}{sym}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn route(shell: &Shell, ev: KeyEvent) -> KeyAction {
        route_key(shell, ev, false, true)
    }

    #[test]
    fn pdb_session_collapses_tree_and_workflow() {
        let shell = Shell::pdb_session();
        assert_eq!(shell.focused, PaneId::View);
        assert!(!shell.is_expanded(PaneId::Workflow));
        assert!(!shell.is_expanded(PaneId::Tree));
        assert!(shell.is_expanded(PaneId::View));
        assert!(shell.is_expanded(PaneId::EditSpec));
    }

    #[test]
    fn campaign_session_lands_on_tree() {
        let shell = Shell::campaign_session();
        assert_eq!(shell.focused, PaneId::Tree);
        assert!(shell.is_expanded(PaneId::Tree));
        assert!(shell.is_expanded(PaneId::View));
        assert!(!shell.is_expanded(PaneId::Workflow));
    }

    #[test]
    fn tree_focus_enter_activates_row_not_editspec_form() {
        let mut shell = Shell::campaign_session();
        assert_eq!(route(&shell, key(KeyCode::Enter)), KeyAction::TreeActivate);
        assert_eq!(route(&shell, key(KeyCode::Char('j'))), KeyAction::TreeNext);
        assert_eq!(route(&shell, key(KeyCode::Char('h'))), KeyAction::TreeCollapse);
        shell.focus(PaneId::View);
        assert_eq!(route(&shell, key(KeyCode::Enter)), KeyAction::Ignore);
        shell.focus(PaneId::Workflow);
        assert_eq!(route(&shell, key(KeyCode::Enter)), KeyAction::Ignore);
        assert_eq!(route(&shell, key(KeyCode::Char('x'))), KeyAction::Ignore);
        assert_eq!(route(&shell, key(KeyCode::Char('e'))), KeyAction::Ignore);
    }

    #[test]
    fn view_enter_is_ignore() {
        let shell = Shell::pdb_session();
        assert_eq!(shell.focused, PaneId::View);
        assert_eq!(route(&shell, key(KeyCode::Enter)), KeyAction::Ignore);
        assert_eq!(route(&shell, key(KeyCode::Char('x'))), KeyAction::Ignore);
        assert_eq!(route(&shell, key(KeyCode::Char('e'))), KeyAction::Ignore);
    }

    #[test]
    fn editspec_enter_opens_empty_form() {
        let mut shell = Shell::pdb_session();
        shell.focus(PaneId::EditSpec);
        assert_eq!(route(&shell, key(KeyCode::Enter)), KeyAction::OpenEmptyForm);
        assert_eq!(route(&shell, key(KeyCode::Char('x'))), KeyAction::EnterSelect);
        assert_eq!(route(&shell, key(KeyCode::Char('e'))), KeyAction::EditFocusedRegion);
    }

    #[test]
    fn tab_cycles_exclusive_focus() {
        let mut shell = Shell::pdb_session();
        assert_eq!(route(&shell, key(KeyCode::Tab)), KeyAction::CyclePaneNext);
        shell.cycle_focus_next();
        assert_eq!(shell.focused, PaneId::EditSpec);
        shell.cycle_focus_next();
        assert_eq!(shell.focused, PaneId::Workflow);
        shell.cycle_focus_prev();
        assert_eq!(shell.focused, PaneId::EditSpec);
    }

    #[test]
    fn only_one_pane_is_focused() {
        let mut shell = Shell::pdb_session();
        let start = shell.focused;
        shell.cycle_focus_next();
        assert_ne!(shell.focused, start);
        assert_eq!(PaneId::ALL.iter().filter(|&&p| p == shell.focused).count(), 1);
    }

    #[test]
    fn view_jk_rotates_editspec_jk_navigates() {
        let view = Shell::pdb_session();
        assert_eq!(route(&view, key(KeyCode::Char('j'))), KeyAction::RotateX(1.0));
        let mut editspec = Shell::pdb_session();
        editspec.focus(PaneId::EditSpec);
        assert_eq!(route(&editspec, key(KeyCode::Char('j'))), KeyAction::RegionNext);
        assert_eq!(route(&editspec, key(KeyCode::Char('k'))), KeyAction::RegionPrev);
    }

    #[test]
    fn view_mode_does_not_steal_3d_keys_for_sequence() {
        let shell = Shell::pdb_session();
        assert_eq!(route(&shell, key(KeyCode::Char('h'))), KeyAction::RotateY(-1.0));
        assert_eq!(route(&shell, key(KeyCode::Char('l'))), KeyAction::RotateY(1.0));
        assert_eq!(route(&shell, key(KeyCode::Char('w'))), KeyAction::Pan(0.0, 1.0));
    }

    #[test]
    fn x_enters_select_and_select_owns_hl() {
        let mut shell = Shell::pdb_session();
        shell.focus(PaneId::EditSpec);
        assert_eq!(route(&shell, key(KeyCode::Char('x'))), KeyAction::EnterSelect);
        shell.enter_mode(InteractionMode::Select);
        assert_eq!(route(&shell, key(KeyCode::Char('h'))), KeyAction::SeqCursor(-1));
        assert_eq!(route(&shell, key(KeyCode::Char('q'))), KeyAction::Ignore);
    }

    #[test]
    fn q_quits_only_when_idle() {
        let view = Shell::pdb_session();
        assert_eq!(route(&view, key(KeyCode::Char('q'))), KeyAction::Quit);
        let mut select = Shell::pdb_session();
        select.enter_mode(InteractionMode::Select);
        assert_ne!(route(&select, key(KeyCode::Char('q'))), KeyAction::Quit);
        let mut run = Shell::pdb_session();
        run.open_overlay(Overlay::RunComposer);
        assert_ne!(route(&run, key(KeyCode::Char('q'))), KeyAction::Quit);
        let mut form = Shell::pdb_session();
        form.enter_mode(InteractionMode::EditRegion);
        assert_ne!(route(&form, key(KeyCode::Char('q'))), KeyAction::Quit);
    }

    #[test]
    fn ctrl_r_opens_composer_from_any_pane_including_select() {
        let view = Shell::pdb_session();
        assert_eq!(route(&view, key_ctrl(KeyCode::Char('r'))), KeyAction::EnterRun);
        let mut select = Shell::pdb_session();
        select.focus(PaneId::EditSpec);
        select.enter_mode(InteractionMode::Select);
        assert_eq!(route(&select, key_ctrl(KeyCode::Char('r'))), KeyAction::EnterRun);
        let mut tree = Shell::pdb_session();
        tree.focus(PaneId::Tree);
        assert_eq!(route(&tree, key_ctrl(KeyCode::Char('r'))), KeyAction::EnterRun);
        let mut run = Shell::pdb_session();
        run.open_overlay(Overlay::RunComposer);
        assert_eq!(route(&run, key_ctrl(KeyCode::Char('r'))), KeyAction::RunIgnore);
        let mut form = Shell::pdb_session();
        form.enter_mode(InteractionMode::EditRegion);
        assert_eq!(route(&form, key_ctrl(KeyCode::Char('r'))), KeyAction::Ignore);
        assert_eq!(
            route_key(&view, key_ctrl(KeyCode::Char('r')), false, false),
            KeyAction::Ignore
        );
    }

    #[test]
    fn select_esc_clears_and_returns_editspec_idle() {
        let mut shell = Shell::pdb_session();
        shell.focus(PaneId::EditSpec);
        shell.enter_mode(InteractionMode::Select);
        assert_eq!(route(&shell, key(KeyCode::Esc)), KeyAction::ClearSelection);
        shell.enter_idle();
        assert_eq!(shell.focused, PaneId::EditSpec);
        assert_eq!(shell.mode, InteractionMode::Idle);
        assert!(shell.session_idle());
    }

    #[test]
    fn select_tab_exits_keep_then_cycles() {
        let mut shell = Shell::pdb_session();
        shell.focus(PaneId::EditSpec);
        shell.enter_mode(InteractionMode::Select);
        assert_eq!(
            route(&shell, key(KeyCode::Tab)),
            KeyAction::ExitSelectKeepThenCycleNext
        );
        shell.exit_select_keep_then_cycle(true);
        assert_eq!(shell.mode, InteractionMode::Idle);
        assert_ne!(shell.focused, PaneId::EditSpec);
    }

    #[test]
    fn pointer_from_select_to_view_idles_keep_focus_view() {
        let mut shell = Shell::pdb_session();
        shell.focus(PaneId::EditSpec);
        shell.enter_mode(InteractionMode::Select);
        shell.pointer_leave_local_mode();
        shell.focus(PaneId::View);
        assert_eq!(shell.focused, PaneId::View);
        assert_eq!(shell.mode, InteractionMode::Idle);
    }

    #[test]
    fn pointer_from_form_opened_in_select_idles_not_restore_select() {
        let mut shell = Shell::pdb_session();
        shell.focus(PaneId::EditSpec);
        shell.enter_mode(InteractionMode::Select);
        shell.enter_mode(InteractionMode::EditRegion);
        assert_eq!(shell.previous_mode, InteractionMode::Select);
        shell.pointer_leave_local_mode();
        shell.focus(PaneId::View);
        assert_eq!(shell.focused, PaneId::View);
        assert_eq!(shell.mode, InteractionMode::Idle);
        assert_ne!(shell.mode, InteractionMode::Select);
    }

    #[test]
    fn context_menu_owns_jk_enter_esc() {
        let mut shell = Shell::pdb_session();
        shell.open_overlay(Overlay::ContextMenu);
        assert_eq!(route(&shell, key(KeyCode::Char('j'))), KeyAction::ContextMenuNext);
        assert_eq!(route(&shell, key(KeyCode::Char('k'))), KeyAction::ContextMenuPrev);
        assert_eq!(route(&shell, key(KeyCode::Enter)), KeyAction::ContextMenuApply);
        assert_eq!(route(&shell, key(KeyCode::Esc)), KeyAction::CloseContextMenu);
        assert_eq!(route(&shell, key(KeyCode::Char('q'))), KeyAction::Ignore);
    }

    #[test]
    fn help_and_composer_do_not_stack() {
        let mut shell = Shell::pdb_session();
        shell.open_overlay(Overlay::RunComposer);
        assert_eq!(route(&shell, key(KeyCode::Char('?'))), KeyAction::RunIgnore);
        shell.close_overlay();
        assert_eq!(route(&shell, key(KeyCode::Char('?'))), KeyAction::ToggleHelp);
        shell.open_overlay(Overlay::Help);
        shell.open_overlay(Overlay::RunComposer);
        assert_eq!(shell.overlay, Overlay::Help);
    }

    #[test]
    fn edit_region_esc_restores_previous_mode() {
        let mut shell = Shell::pdb_session();
        shell.enter_mode(InteractionMode::Select);
        shell.enter_mode(InteractionMode::EditRegion);
        assert_eq!(route(&shell, key(KeyCode::Esc)), KeyAction::EditFormCancel);
        shell.restore_previous_mode();
        assert_eq!(shell.mode, InteractionMode::Select);
    }

    #[test]
    fn parse_direct_range_forms() {
        assert_eq!(
            parse_direct_range("A51-80", "B").unwrap(),
            ("A".to_string(), 51, 80)
        );
        assert_eq!(
            parse_direct_range("A:51-80", "B").unwrap(),
            ("A".to_string(), 51, 80)
        );
        assert_eq!(
            parse_direct_range("51-80", "C").unwrap(),
            ("C".to_string(), 51, 80)
        );
        assert!(parse_direct_range("abc", "A").unwrap_err().contains("no numbers"));
        assert!(parse_direct_range("A80-51", "A").unwrap_err().contains("greater than end"));
    }

    #[test]
    fn parse_compact_and_roundtrip() {
        let parsed = parse_compact_regions("A1-50=, A51-80~").unwrap();
        assert_eq!(parsed[0], ("A".to_string(), 1, 50, "keep".to_string()));
        assert_eq!(parsed[1], ("A".to_string(), 51, 80, "edit".to_string()));
        assert_eq!(compact_edit_spec(&parsed), "A1-50=,A51-80~");
    }

    #[test]
    fn key_table_documents_router() {
        let joined = KEY_TABLE
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Tab"));
        assert!(joined.contains("View"));
        assert!(joined.contains("Select"));
        assert!(joined.contains("EditRegion"));
        assert!(joined.contains("Run"));
        assert!(joined.contains("Ctrl+R"));
    }
}
