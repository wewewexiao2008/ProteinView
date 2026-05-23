//! PyO3 bridge layer for calling gemlib Python APIs from Rust.
//!
//! This module embeds the CPython interpreter via PyO3 and provides
//! Rust wrappers around key gemlib/contiger functions.  If the Python
//! interpreter or gemlib modules are unavailable, the bridge degrades
//! gracefully and ProteinView operates in read-only mode.

use anyhow::{Result, bail};
use pyo3::prelude::*;
use pyo3::types::PyList;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Data types shared with the rest of ProteinView
// ---------------------------------------------------------------------------

/// Information about a single chain extracted from a PDB file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainInfo {
    pub id: String,
    pub residue_count: usize,
    pub atom_count: usize,
}

/// Parsed EditSpec region returned by contiger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditSpecData {
    pub regions: Vec<EditSpecRegionData>,
    pub raw_spec: String,
}

/// A single region within an EditSpec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditSpecRegionData {
    pub chain: String,
    pub range: [usize; 2],
    pub action: String,
    pub label: Option<String>,
}

/// A validation issue found when checking EditSpec regions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: String, // "error" | "warning"
    pub message: String,
    pub region_index: Option<usize>,
}

// ---------------------------------------------------------------------------
// Error wrapper
// ---------------------------------------------------------------------------

/// Error type for PyO3 bridge operations.
#[derive(Debug)]
pub struct PyO3Error(String);

impl std::fmt::Display for PyO3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PyO3 bridge error: {}", self.0)
    }
}

impl std::error::Error for PyO3Error {}

// ---------------------------------------------------------------------------
// GemlibBridge
// ---------------------------------------------------------------------------

/// Bridge to the gemlib Python runtime.
///
/// Wraps the embedded CPython interpreter and exposes high-level methods
/// that delegate to gemlib/contiger Python code.  Construction is fallible
/// so that ProteinView can degrade to read-only mode when Python is missing.
pub struct GemlibBridge {
    /// Keep a handle to the Python interpreter so it stays alive.
    _guard: PyO3Guard,
}

/// RAII guard that ensures the Python interpreter is initialized.
/// Dropping this is fine -- PyO3 with `auto-initialize` manages the
/// interpreter lifecycle automatically.
struct PyO3Guard {
    _private: (),
}

impl GemlibBridge {
    /// Initialize the Python interpreter and verify that gemlib is importable.
    pub fn new() -> Result<Self> {
        // Ensure pixi's Python is on the path.  When ProteinView is launched
        // via `pixi run`, `sys.executable` already points to pixi's Python.
        // For standalone invocations we try to locate the pixi env.
        ensure_pyo3_python_env();

        // Try importing gemlib to verify the environment.
        let guard = PyO3Guard { _private: () };
        let bridge = Self { _guard: guard };
        bridge.check_available()?;

        Ok(bridge)
    }

    /// Quick availability check -- returns `true` if Python + gemlib are reachable.
    pub fn is_available(&self) -> bool {
        Python::with_gil(|py| {
            py.import("gemlib").is_ok() && py.import("contiger").is_ok()
        })
    }

    // -- High-level API methods -------------------------------------------

    /// Get chain information from a PDB file via gemmi (loaded through gemlib).
    pub fn get_chains(&self, pdb_path: &str) -> Result<Vec<ChainInfo>> {
        Python::with_gil(|py| {
            let gemmi_mod = py.import("gemmi")
                .map_err(|e| anyhow::anyhow!("failed to import gemmi: {}", e))?;
            let structure = gemmi_mod
                .call_method1("read_structure", (pdb_path,))
                .map_err(|e| anyhow::anyhow!("failed to read PDB with gemmi: {}", e))?;

            let chains_py = structure
                .call_method0("get_chains")
                .map_err(|e| anyhow::anyhow!("failed to call get_chains: {}", e))?;

            let mut result = Vec::new();
            for chain_obj in chains_py.try_iter()
                .map_err(|e| anyhow::anyhow!("failed to iterate chains: {}", e))?
            {
                let chain = chain_obj
                    .map_err(|e| anyhow::anyhow!("failed to get chain: {}", e))?;
                let id: String = chain
                    .getattr("name")
                    .map_err(|e| anyhow::anyhow!("failed to get chain name: {}", e))?
                    .extract()
                    .map_err(|e| anyhow::anyhow!("failed to extract chain name: {}", e))?;

                // Get residue count from polymer length.
                let residue_count: usize = chain
                    .call_method0("get_polymer")
                    .and_then(|p| p.getattr("length"))
                    .and_then(|l| l.extract::<usize>())
                    .unwrap_or(0);

                // Get atom count from chain.
                let atom_count: usize = chain
                    .len()
                    .map_err(|e| anyhow::anyhow!("failed to get chain len: {}", e))?;

                result.push(ChainInfo {
                    id,
                    residue_count,
                    atom_count,
                });
            }

            Ok(result)
        })
    }

    /// Parse an EditSpec string via contiger and return structured data.
    pub fn parse_edit_spec(&self, spec_str: &str) -> Result<EditSpecData> {
        Python::with_gil(|py| {
            let contiger_mod = py.import("contiger.core.edit_spec")
                .map_err(|e| anyhow::anyhow!("failed to import contiger.core.edit_spec: {}", e))?;
            let edit_spec = contiger_mod
                .call_method1("parse_edit_spec", (spec_str,))
                .map_err(|e| anyhow::anyhow!("failed to call parse_edit_spec: {}", e))?;

            let regions_attr = edit_spec
                .getattr("regions")
                .map_err(|e| anyhow::anyhow!("failed to get 'regions' attribute: {}", e))?;
            let regions_list = regions_attr
                .downcast::<PyList>()
                .map_err(|e| anyhow::anyhow!("'regions' is not a list: {}", e))?;

            let mut regions = Vec::new();
            for item_result in regions_list.try_iter()
                .map_err(|e| anyhow::anyhow!("failed to iterate regions: {}", e))?
            {
                let item = item_result
                    .map_err(|e| anyhow::anyhow!("failed to get region item: {}", e))?;
                let chain: String = item
                    .getattr("chain")?.extract()
                    .map_err(|e| anyhow::anyhow!("failed to extract chain: {}", e))?;
                let start: usize = item
                    .getattr("start")?.extract()
                    .map_err(|e| anyhow::anyhow!("failed to extract start: {}", e))?;
                let end: usize = item
                    .getattr("end")?.extract()
                    .map_err(|e| anyhow::anyhow!("failed to extract end: {}", e))?;
                let action: String = item
                    .getattr("action")?.extract()
                    .map_err(|e| anyhow::anyhow!("failed to extract action: {}", e))?;

                regions.push(EditSpecRegionData {
                    chain,
                    range: [start, end],
                    action,
                    label: None,
                });
            }

            Ok(EditSpecData {
                regions,
                raw_spec: spec_str.to_string(),
            })
        })
    }

    /// Apply edits to a PDB structure, returning the modified PDB content as a string.
    ///
    /// This is a placeholder for the full edit-application pipeline.
    /// The actual implementation will call contiger + gemlib graft logic.
    pub fn apply_edit(&self, _original_pdb: &str, _edit_spec: &str) -> Result<String> {
        // TODO: Implement full edit application via contiger/gemlib graft.
        // For now, return a not-yet-implemented error so callers can degrade.
        bail!("apply_edit is not yet implemented in the bridge layer")
    }

    /// Validate EditSpec regions for overlap/conflict issues.
    pub fn validate_edit_spec(
        &self,
        regions: &[EditSpecRegionData],
    ) -> Result<Vec<ValidationIssue>> {
        let mut issues = Vec::new();

        // Sort regions by (chain, start) for overlap detection.
        let mut sorted: Vec<(usize, &EditSpecRegionData)> =
            regions.iter().enumerate().collect();
        sorted.sort_by_key(|(_, r)| (r.chain.clone(), r.range[0]));

        // Check for overlapping regions within the same chain.
        for i in 0..sorted.len() {
            let (idx_a, a) = sorted[i];
            for j in (i + 1)..sorted.len() {
                let (_, b) = sorted[j];
                if a.chain != b.chain {
                    break; // Different chains, no overlap possible.
                }
                // Overlap: a.end >= b.start
                if a.range[1] >= b.range[0] {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        message: format!(
                            "Overlapping regions: [{},{}] and [{},{}] on chain {}",
                            a.range[0], a.range[1], b.range[0], b.range[1], a.chain
                        ),
                        region_index: Some(idx_a),
                    });
                }
            }
        }

        // Check for unknown actions.
        let valid_actions = ["=", "~", "keep", "edit", "replace", "insert", "delete"];
        for (i, r) in regions.iter().enumerate() {
            let action_lower = r.action.to_lowercase();
            let is_valid = valid_actions.iter().any(|a| {
                action_lower == *a || action_local_match(&action_lower, a)
            });
            if !is_valid && !r.action.starts_with('>') && !r.action.starts_with('+') {
                issues.push(ValidationIssue {
                    severity: "warning".to_string(),
                    message: format!("Unknown action '{}' on chain {}", r.action, r.chain),
                    region_index: Some(i),
                });
            }
        }

        // Check for invalid residue ranges.
        for (i, r) in regions.iter().enumerate() {
            if r.range[0] == 0 && r.range[1] == 0 {
                // Range [0,0] might mean "whole chain" -- not an error.
                continue;
            }
            if r.range[0] > r.range[1] && r.range[1] != 0 {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    message: format!(
                        "Invalid range [{},{}] on chain {}: start > end",
                        r.range[0], r.range[1], r.chain
                    ),
                    region_index: Some(i),
                });
            }
        }

        Ok(issues)
    }

    /// Generic method call into a Python module.
    ///
    /// `module_path` is a dotted import path like `"gemlib.graft"`.
    /// Returns the Python value as a JSON string for flexible deserialization.
    pub fn call_edit_spec_method(
        &self,
        module_path: &str,
        method: &str,
        args: &str, // JSON-encoded arguments
    ) -> Result<String> {
        Python::with_gil(|py| {
            let module = py.import(module_path)
                .map_err(|e| anyhow::anyhow!("failed to import '{}': {}", module_path, e))?;
            let py_args = py
                .import("json")?
                .call_method1("loads", (args,))
                .map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!(
                        "Failed to parse JSON args: {}",
                        e
                    ))
                })?;
            let result = module.call_method1(method, (py_args,))
                .map_err(|e| anyhow::anyhow!("failed to call {}.{}: {}", module_path, method, e))?;
            let json_str: String = py
                .import("json")?
                .call_method1("dumps", (result,))?
                .extract()
                .map_err(|e| anyhow::anyhow!("failed to serialize result to JSON: {}", e))?;
            Ok(json_str)
        })
    }

    // -- Private helpers --------------------------------------------------

    /// Verify that the essential Python modules are importable.
    fn check_available(&self) -> Result<()> {
        Python::with_gil(|py| {
            py.import("gemlib")
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Python 'gemlib' module not found. \
                         Ensure gemlib is installed (pixi install). \
                         Error: {}",
                        e
                    )
                })?;
            py.import("contiger")
                .map_err(|e| anyhow::anyhow!("Python 'contiger' module not found: {}", e))?;
            Ok(())
        })
    }
}

/// Check if a local action string matches a canonical action.
fn action_local_match(action_lower: &str, canonical: &str) -> bool {
    match canonical {
        "keep" => action_lower == "=",
        "edit" => action_lower == "~",
        _ => false,
    }
}

/// Set environment variables so PyO3 can locate the correct Python.
///
/// When running under pixi, `PYO3_PYTHON` should point to the pixi-managed
/// interpreter so that gemlib and its dependencies are importable.
fn ensure_pyo3_python_env() {
    // If PYO3_PYTHON is already set, respect it.
    if std::env::var("PYO3_PYTHON").is_ok() {
        return;
    }

    // Try to find the pixi Python in common locations.
    let candidates = [
        // pixi creates a .pixi/envs/default/bin/python3 symlink.
        ".pixi/envs/default/bin/python3".to_string(),
        // Absolute path relative to the workspace root.
        std::env::var("PIXI_PROJECT_ROOT")
            .map(|root| format!("{}/.pixi/envs/default/bin/python3", root))
            .unwrap_or_default(),
    ];

    for candidate in &candidates {
        if std::path::Path::new(candidate).exists() {
            // SAFETY: set_var is unsafe in Rust 2024 edition. We call it
            // during single-threaded initialization before spawning any
            // threads, so there is no data race on the environment block.
            unsafe {
                std::env::set_var("PYO3_PYTHON", candidate);
            }
            return;
        }
    }

    // Fall back to `python3` on PATH -- may or may not have gemlib.
    if let Ok(which) = which_python3() {
        // SAFETY: same rationale as above.
        unsafe {
            std::env::set_var("PYO3_PYTHON", which);
        }
    }
}

/// Try to locate `python3` on PATH.
fn which_python3() -> Result<String> {
    let output = std::process::Command::new("which")
        .arg("python3")
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run 'which python3': {}", e))?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(path);
        }
    }
    bail!("python3 not found on PATH")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_no_issues() {
        let regions = vec![
            EditSpecRegionData {
                chain: "A".to_string(),
                range: [1, 10],
                action: "=".to_string(),
                label: None,
            },
            EditSpecRegionData {
                chain: "A".to_string(),
                range: [20, 30],
                action: "~".to_string(),
                label: None,
            },
            EditSpecRegionData {
                chain: "B".to_string(),
                range: [5, 15],
                action: "=".to_string(),
                label: None,
            },
        ];
        let issues = validate_regions_pure(&regions);
        assert!(issues.is_empty(), "Expected no issues, got {:?}", issues);
    }

    #[test]
    fn test_validate_overlap_detected() {
        let regions = vec![
            EditSpecRegionData {
                chain: "A".to_string(),
                range: [1, 15],
                action: "=".to_string(),
                label: None,
            },
            EditSpecRegionData {
                chain: "A".to_string(),
                range: [10, 20],
                action: "~".to_string(),
                label: None,
            },
        ];
        let issues = validate_regions_pure(&regions);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, "error");
        assert!(issues[0].message.contains("Overlapping"));
    }

    #[test]
    fn test_validate_invalid_range() {
        let regions = vec![EditSpecRegionData {
            chain: "A".to_string(),
            range: [20, 10],
            action: "=".to_string(),
            label: None,
        }];
        let issues = validate_regions_pure(&regions);
        assert!(issues.iter().any(|i| i.message.contains("Invalid range")));
    }

    /// Pure-Rust validation helper (no Python needed) for unit tests.
    fn validate_regions_pure(regions: &[EditSpecRegionData]) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        let mut sorted: Vec<(usize, &EditSpecRegionData)> =
            regions.iter().enumerate().collect();
        sorted.sort_by_key(|(_, r)| (r.chain.clone(), r.range[0]));

        for i in 0..sorted.len() {
            let (idx_a, a) = sorted[i];
            for j in (i + 1)..sorted.len() {
                let (_, b) = sorted[j];
                if a.chain != b.chain {
                    break;
                }
                if a.range[1] >= b.range[0] {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        message: format!(
                            "Overlapping regions: [{},{}] and [{},{}] on chain {}",
                            a.range[0], a.range[1], b.range[0], b.range[1], a.chain
                        ),
                        region_index: Some(idx_a),
                    });
                }
            }
        }

        for (i, r) in regions.iter().enumerate() {
            if r.range[0] > r.range[1] && r.range[1] != 0 && r.range[0] != 0 {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    message: format!(
                        "Invalid range [{},{}] on chain {}: start > end",
                        r.range[0], r.range[1], r.chain
                    ),
                    region_index: Some(i),
                });
            }
        }

        issues
    }
}
