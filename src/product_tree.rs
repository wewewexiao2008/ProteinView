//! Campaign product tree seeded by gemlib into `--state-file`.
//!
//! Nodes follow `SampleRecord.parent_ids`, not `stages/iter/attempt` paths.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StudioSeed {
    pub campaign_root: Option<String>,
    #[serde(default)]
    pub product_tree: ProductTree,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProductTree {
    pub campaign_root: Option<String>,
    pub selected_sample_id: Option<String>,
    #[serde(default)]
    pub roots: Vec<ProductTreeNode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProductTreeNode {
    pub sample_id: String,
    pub kind: String,
    #[serde(default)]
    pub parent_ids: Vec<String>,
    #[serde(default)]
    pub metrics: serde_json::Value,
    #[serde(default)]
    pub label: String,
    pub structure_path: Option<String>,
    #[serde(default = "default_expanded")]
    pub expanded: bool,
    #[serde(default)]
    pub children: Vec<ProductTreeNode>,
}

fn default_expanded() -> bool {
    true
}

impl ProductTree {
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn visible_rows(&self) -> Vec<(usize, &ProductTreeNode)> {
        let mut rows = Vec::new();
        fn walk<'a>(
            node: &'a ProductTreeNode,
            depth: usize,
            rows: &mut Vec<(usize, &'a ProductTreeNode)>,
        ) {
            rows.push((depth, node));
            if node.expanded {
                for child in &node.children {
                    walk(child, depth + 1, rows);
                }
            }
        }
        for root in &self.roots {
            walk(root, 0, &mut rows);
        }
        rows
    }

    pub fn set_expanded(&mut self, sample_id: &str, expanded: bool) -> bool {
        fn walk(node: &mut ProductTreeNode, sample_id: &str, expanded: bool) -> bool {
            if node.sample_id == sample_id {
                node.expanded = expanded;
                return true;
            }
            node.children
                .iter_mut()
                .any(|child| walk(child, sample_id, expanded))
        }
        self.roots
            .iter_mut()
            .any(|root| walk(root, sample_id, expanded))
    }

    pub fn select(&mut self, sample_id: &str) {
        self.selected_sample_id = Some(sample_id.to_string());
    }
}

pub fn load_studio_seed(text: &str) -> Result<StudioSeed, serde_json::Error> {
    serde_json::from_str(text)
}

pub fn resolve_structure_path(campaign_root: Option<&str>, relative: &str) -> PathBuf {
    let rel = Path::new(relative);
    if rel.is_absolute() {
        return rel.to_path_buf();
    }
    match campaign_root {
        Some(root) => Path::new(root).join(rel),
        None => rel.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: &str = r#"{
      "campaign_root": "/tmp/camp",
      "product_tree": {
        "campaign_root": "/tmp/camp",
        "selected_sample_id": null,
        "roots": [
          {
            "sample_id": "backbone:aaaaaaaaaaaaaaaa",
            "kind": "backbone",
            "parent_ids": [],
            "metrics": {},
            "label": "backbone backbone:aaaaaaaaaaaaaaaa",
            "structure_path": "outputs/backbone.pdb",
            "expanded": true,
            "children": [
              {
                "sample_id": "sequence:bbbbbbbbbbbbbbbb",
                "kind": "sequence",
                "parent_ids": ["backbone:aaaaaaaaaaaaaaaa"],
                "metrics": {"mpnn_perplexity": 1.25},
                "label": "sequence sequence:bbbbbbbbbbbbbbbb  mpnn_perplexity=1.25",
                "structure_path": null,
                "expanded": true,
                "children": [
                  {
                    "sample_id": "prediction:cccccccccccccccc",
                    "kind": "prediction",
                    "parent_ids": ["sequence:bbbbbbbbbbbbbbbb"],
                    "metrics": {"plddt": 91.0},
                    "label": "prediction prediction:cccccccccccccccc  plddt=91",
                    "structure_path": "outputs/prediction.pdb",
                    "expanded": true,
                    "children": []
                  }
                ]
              }
            ]
          }
        ]
      }
    }"#;

    #[test]
    fn seed_follows_parent_ids_not_stage_paths() {
        let seed = load_studio_seed(SEED).unwrap();
        let rows = seed.product_tree.visible_rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].1.kind, "backbone");
        assert_eq!(rows[1].1.kind, "sequence");
        assert_eq!(rows[2].1.kind, "prediction");
        for (_depth, node) in rows {
            assert!(!node.label.contains("stages/"));
            assert!(!node.label.contains("iter_"));
            assert!(!node.label.contains("attempt_"));
        }
    }

    #[test]
    fn collapse_hides_descendants() {
        let mut seed = load_studio_seed(SEED).unwrap();
        assert!(seed.product_tree.set_expanded("backbone:aaaaaaaaaaaaaaaa", false));
        let rows = seed.product_tree.visible_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.sample_id, "backbone:aaaaaaaaaaaaaaaa");
    }

    #[test]
    fn structure_path_loads_the_same_parser_view_uses() {
        let dir = tempfile::tempdir().unwrap();
        let pdb = dir.path().join("prediction.pdb");
        std::fs::write(
            &pdb,
            "ATOM      1  N   ALA A   1       0.000   0.000   0.000  1.00 50.00           N\n\
             ATOM      2  CA  ALA A   1       1.458   0.000   0.000  1.00 50.00           C\n\
             END\n",
        )
        .unwrap();
        let path = resolve_structure_path(
            Some(dir.path().to_str().unwrap()),
            "prediction.pdb",
        );
        let protein = crate::parser::pdb::load_structure(path.to_str().unwrap()).unwrap();
        assert!(protein.residue_count() >= 1);
    }

    #[test]
    fn prediction_structure_path_is_under_campaign_root() {
        let seed = load_studio_seed(SEED).unwrap();
        let pred = seed.product_tree.visible_rows()[2].1;
        let path = resolve_structure_path(
            seed.campaign_root.as_deref(),
            pred.structure_path.as_deref().unwrap(),
        );
        assert_eq!(path, PathBuf::from("/tmp/camp/outputs/prediction.pdb"));
        assert!(seed.product_tree.visible_rows()[1].1.structure_path.is_none());
    }
}
