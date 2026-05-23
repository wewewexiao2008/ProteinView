//! Operation history (undo/redo) and real-time validation for EditSpec regions.
//!
//! This module provides:
//! - `EditHistory` — a stack-based undo/redo system that snapshots the full
//!   region list before each edit operation.
//! - `ValidationIssue` / `IssueSeverity` — structured validation results for
//!   overlap detection, gap detection, range validity, and empty-chain warnings.
//! - `validate_regions` — pure function that runs all local validation checks.

use crate::app::EditSpecRegion;

// ---------------------------------------------------------------------------
// Edit history
// ---------------------------------------------------------------------------

/// Maximum number of undo snapshots retained by default.
const DEFAULT_MAX_DEPTH: usize = 100;

/// A snapshot of the full EditSpec region list at a point in time.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Human-readable description, e.g. "add region", "delete region 3".
    pub description: String,
    /// Complete snapshot of all regions.
    pub snapshot: Vec<EditSpecRegion>,
    /// Which region was focused when this snapshot was taken.
    pub focused_region: usize,
    /// Panel scroll offset when this snapshot was taken.
    pub panel_scroll: u16,
}

/// Undo/redo stack for EditSpec editing operations.
#[derive(Debug, Clone)]
pub struct EditHistory {
    undo_stack: Vec<HistoryEntry>,
    redo_stack: Vec<HistoryEntry>,
    max_depth: usize,
}

impl Default for EditHistory {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_DEPTH)
    }
}

impl EditHistory {
    /// Create a new history with the given maximum depth.
    pub fn new(max_depth: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_depth: max_depth.max(1),
        }
    }

    /// Push a snapshot onto the undo stack.  Clears the redo stack.
    /// If the undo stack exceeds `max_depth`, the oldest entry is discarded.
    pub fn push(&mut self, entry: HistoryEntry) {
        self.redo_stack.clear();
        self.undo_stack.push(entry);
        if self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0);
        }
    }

    /// Pop the most recent entry from the undo stack and push it onto the redo stack.
    pub fn undo(&mut self) -> Option<HistoryEntry> {
        if let Some(entry) = self.undo_stack.pop() {
            self.redo_stack.push(entry.clone());
            Some(entry)
        } else {
            None
        }
    }

    /// Pop the most recent entry from the redo stack and push it onto the undo stack.
    pub fn redo(&mut self) -> Option<HistoryEntry> {
        if let Some(entry) = self.redo_stack.pop() {
            self.undo_stack.push(entry.clone());
            Some(entry)
        } else {
            None
        }
    }

    /// Whether an undo operation is available.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Whether a redo operation is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Reset both stacks.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Severity of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Error,
    Warning,
}

/// A single validation issue found when checking EditSpec regions.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub message: String,
    /// Indices of regions involved in this issue.
    pub region_indices: Vec<usize>,
}

/// Run all local validation checks on a list of EditSpec regions.
///
/// Returns a list of issues ordered: errors first, then warnings.
/// This function performs no I/O and is safe to call on every state change.
pub fn validate_regions(regions: &[EditSpecRegion]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    // 1. Range validity: start > end, start == 0
    for (i, r) in regions.iter().enumerate() {
        if r.range[0] == 0 && r.range[1] == 0 {
            // Range [0,0] can mean "whole chain" in some contexts — not an error.
            continue;
        }
        if r.range[0] > r.range[1] {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: format!(
                    "Invalid range [{}-{}] on chain {}: start > end",
                    r.range[0], r.range[1], r.chain
                ),
                region_indices: vec![i],
            });
        } else if r.range[0] == 0 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Warning,
                message: format!(
                    "Range starts at 0 on chain {} — residue numbering usually starts at 1",
                    r.chain
                ),
                region_indices: vec![i],
            });
        }
    }

    // 2. Overlap detection (same chain, overlapping ranges)
    issues.extend(detect_overlaps(regions));

    // 3. Gap detection between adjacent regions on the same chain
    issues.extend(detect_gaps(regions));

    // 4. Unknown action names
    let valid_actions = ["keep", "edit", "replace", "insert", "delete"];
    for (i, r) in regions.iter().enumerate() {
        if !valid_actions.contains(&r.action.as_str()) {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Warning,
                message: format!("Unknown action '{}' on chain {}", r.action, r.chain),
                region_indices: vec![i],
            });
        }
    }

    issues
}

/// Detect overlapping regions on the same chain.
fn detect_overlaps(regions: &[EditSpecRegion]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    // Group by chain.
    let mut by_chain: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, r) in regions.iter().enumerate() {
        by_chain.entry(r.chain.clone()).or_default().push(i);
    }

    for (_chain, indices) in &by_chain {
        if indices.len() < 2 {
            continue;
        }
        // Sort indices by range start.
        let mut sorted = indices.clone();
        sorted.sort_by_key(|&i| regions[i].range[0]);

        for a in 0..sorted.len() {
            for b in (a + 1)..sorted.len() {
                let idx_a = sorted[a];
                let idx_b = sorted[b];
                let ra = &regions[idx_a];
                let rb = &regions[idx_b];
                // Overlap: ra.end >= rb.start (both inclusive ranges).
                if ra.range[1] >= rb.range[0] {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        message: format!(
                            "Overlapping regions: [{}-{}] and [{}-{}] on chain {}",
                            ra.range[0], ra.range[1], rb.range[0], rb.range[1], ra.chain
                        ),
                        region_indices: vec![idx_a, idx_b],
                    });
                }
            }
        }
    }

    issues
}

/// Detect gaps between adjacent regions on the same chain.
fn detect_gaps(regions: &[EditSpecRegion]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    let mut by_chain: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, r) in regions.iter().enumerate() {
        by_chain.entry(r.chain.clone()).or_default().push(i);
    }

    for (_chain, indices) in &by_chain {
        if indices.len() < 2 {
            continue;
        }
        // Sort by range start.
        let mut sorted = indices.clone();
        sorted.sort_by_key(|&i| regions[i].range[0]);

        for w in sorted.windows(2) {
            let idx_a = w[0];
            let idx_b = w[1];
            let ra = &regions[idx_a];
            let rb = &regions[idx_b];
            // Gap: ra.end + 1 < rb.start
            if ra.range[1] + 1 < rb.range[0] {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Warning,
                    message: format!(
                        "Gap between [{}-{}] and [{}-{}] on chain {} (residues {}-{})",
                        ra.range[0],
                        ra.range[1],
                        rb.range[0],
                        rb.range[1],
                        ra.chain,
                        ra.range[1] + 1,
                        rb.range[0] - 1
                    ),
                    region_indices: vec![idx_a, idx_b],
                });
            }
        }
    }

    issues
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_region(chain: &str, start: usize, end: usize, action: &str) -> EditSpecRegion {
        EditSpecRegion {
            chain: chain.to_string(),
            range: [start, end],
            action: action.to_string(),
            label: None,
        }
    }

    #[test]
    fn test_validate_no_issues() {
        let regions = vec![
            make_region("A", 1, 10, "keep"),
            make_region("A", 11, 20, "edit"),
            make_region("B", 5, 15, "keep"),
        ];
        let issues = validate_regions(&regions);
        assert!(issues.is_empty(), "Expected no issues, got {:?}", issues);
    }

    #[test]
    fn test_validate_overlap() {
        let regions = vec![
            make_region("A", 1, 15, "keep"),
            make_region("A", 10, 20, "edit"),
        ];
        let issues = validate_regions(&regions);
        assert!(issues.iter().any(|i| i.message.contains("Overlapping")));
    }

    #[test]
    fn test_validate_gap() {
        let regions = vec![
            make_region("A", 1, 10, "keep"),
            make_region("A", 20, 30, "edit"),
        ];
        let issues = validate_regions(&regions);
        assert!(issues.iter().any(|i| i.message.contains("Gap")));
    }

    #[test]
    fn test_validate_invalid_range() {
        let regions = vec![make_region("A", 20, 10, "keep")];
        let issues = validate_regions(&regions);
        assert!(issues.iter().any(|i| i.message.contains("Invalid range")));
    }

    #[test]
    fn test_validate_unknown_action() {
        let regions = vec![make_region("A", 1, 10, "foobar")];
        let issues = validate_regions(&regions);
        assert!(issues.iter().any(|i| i.message.contains("Unknown action")));
    }

    #[test]
    fn test_history_push_undo_redo() {
        let mut history = EditHistory::new(10);
        assert!(!history.can_undo());
        assert!(!history.can_redo());

        history.push(HistoryEntry {
            description: "add".to_string(),
            snapshot: vec![make_region("A", 1, 5, "keep")],
            focused_region: 0,
            panel_scroll: 0,
        });
        assert!(history.can_undo());
        assert!(!history.can_redo());

        let entry = history.undo().unwrap();
        assert_eq!(entry.description, "add");
        assert!(!history.can_undo());
        assert!(history.can_redo());

        let entry = history.redo().unwrap();
        assert_eq!(entry.description, "add");
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn test_history_push_clears_redo() {
        let mut history = EditHistory::new(10);
        history.push(HistoryEntry {
            description: "first".to_string(),
            snapshot: vec![],
            focused_region: 0,
            panel_scroll: 0,
        });
        let _ = history.undo();
        assert!(history.can_redo());

        // Pushing a new entry should clear redo.
        history.push(HistoryEntry {
            description: "second".to_string(),
            snapshot: vec![],
            focused_region: 0,
            panel_scroll: 0,
        });
        assert!(!history.can_redo());
        assert!(history.can_undo());
    }

    #[test]
    fn test_history_max_depth() {
        let mut history = EditHistory::new(3);
        for i in 0..5 {
            history.push(HistoryEntry {
                description: format!("entry {}", i),
                snapshot: vec![],
                focused_region: 0,
                panel_scroll: 0,
            });
        }
        // Only the last 3 should remain.
        assert_eq!(history.undo_stack.len(), 3);
        let entry = history.undo().unwrap();
        assert_eq!(entry.description, "entry 4");
    }

    #[test]
    fn test_history_clear() {
        let mut history = EditHistory::new(10);
        history.push(HistoryEntry {
            description: "a".to_string(),
            snapshot: vec![],
            focused_region: 0,
            panel_scroll: 0,
        });
        history.push(HistoryEntry {
            description: "b".to_string(),
            snapshot: vec![],
            focused_region: 0,
            panel_scroll: 0,
        });
        let _ = history.undo();
        assert!(history.can_undo());
        assert!(history.can_redo());

        history.clear();
        assert!(!history.can_undo());
        assert!(!history.can_redo());
    }
}
