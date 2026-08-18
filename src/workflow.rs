//! Seeded parse graph + check table from gemlib `--state-file`.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkflowCheck {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub missing: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkflowChecks {
    #[serde(default)]
    pub edge: WorkflowCheck,
    #[serde(default)]
    pub input: WorkflowCheck,
    #[serde(default)]
    pub editspec: WorkflowCheck,
    #[serde(default)]
    pub tool: WorkflowCheck,
    #[serde(default)]
    pub ledger: WorkflowCheck,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub block: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub needs_editspec: bool,
    #[serde(default)]
    pub editspec_note: String,
    pub structure_path: Option<String>,
    #[serde(default)]
    pub light: String,
    #[serde(default)]
    pub waiting: bool,
    #[serde(default)]
    pub checks: WorkflowChecks,
    #[serde(default)]
    pub tree_kinds: Vec<String>,
    #[serde(default)]
    pub condition_node: Option<String>,
    #[serde(default)]
    pub rounds: u32,
    #[serde(default)]
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkflowStatus {
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    #[serde(default)]
    pub can_run: bool,
    #[serde(default)]
    pub edit_spec: String,
    #[serde(default)]
    pub draft: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowError {
    #[serde(default)]
    pub rule: String,
    pub node_id: Option<String>,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkflowLoop {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub block: String,
    #[serde(default)]
    pub rounds: u32,
    #[serde(default)]
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkflowGraph {
    #[serde(default)]
    pub compose: Vec<serde_json::Value>,
    pub loop_block: Option<WorkflowLoop>,
}

impl WorkflowStatus {
    pub fn node(&self, index: usize) -> Option<&WorkflowNode> {
        self.nodes.get(index)
    }
}

pub fn tree_row_matches_node(kind: &str, condition_node: Option<&str>, node: &WorkflowNode) -> bool {
    if let Some(wanted) = node.condition_node.as_deref().filter(|id| !id.is_empty()) {
        if let Some(got) = condition_node.filter(|id| !id.is_empty()) {
            return got == wanted;
        }
    }
    node.tree_kinds.iter().any(|item| item == kind)
}

pub fn workflow_index_for_tree_row(
    nodes: &[WorkflowNode],
    kind: &str,
    condition_node: Option<&str>,
) -> Option<usize> {
    if let Some(node_id) = condition_node.filter(|id| !id.is_empty()) {
        if let Some(idx) = nodes.iter().position(|node| node.id == node_id) {
            return Some(idx);
        }
    }
    let want = match kind {
        "input" | "backbone" => "compose",
        "sequence" => "mpnn",
        "prediction" | "ensemble" => "fold",
        "evaluation" => "evaluate",
        "selection" => "gate",
        _ => return None,
    };
    if want == "compose" {
        return nodes.iter().position(|node| node.kind == "compose");
    }
    nodes.iter().position(|node| node.block == want)
}

pub fn recipe_from_graph_dump(graph: &serde_json::Value) -> serde_json::Value {
    let mut compose = Vec::new();
    if let Some(nodes) = graph.get("compose").and_then(|value| value.as_array()) {
        for node in nodes {
            let mut item = serde_json::json!({
                "id": node.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "block": node.get("block").cloned().unwrap_or(serde_json::Value::Null),
            });
            if let Some(params) = node.get("params").and_then(|value| value.as_object()) {
                for (key, value) in params {
                    item[key] = value.clone();
                }
            }
            if let Some(inputs) = node.get("inputs").and_then(|value| value.as_array()) {
                if !inputs.is_empty() {
                    item["from"] = serde_json::Value::Array(inputs.clone());
                }
            }
            compose.push(item);
        }
    }
    let mut recipe = serde_json::json!({
        "kind": "workflow",
        "compose": compose,
    });
    let loop_block = graph
        .get("loop")
        .filter(|value| !value.is_null())
        .or_else(|| graph.get("optimize").filter(|value| !value.is_null()));
    if let Some(loop_block) = loop_block {
        let from = loop_block
            .get("inputs")
            .and_then(|value| value.as_array())
            .and_then(|items| items.first())
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        recipe["optimize"] = serde_json::json!({
            "from": from,
            "rounds": loop_block.get("rounds").cloned().unwrap_or(serde_json::json!(1)),
            "steps": loop_block.get("steps").cloned().unwrap_or(serde_json::json!([])),
            "edit_spec": loop_block.get("edit_spec").cloned().unwrap_or(serde_json::json!("")),
            "gate": loop_block.get("gate").cloned().unwrap_or(serde_json::json!({"mode": "human"})),
        });
    }
    recipe
}

pub fn compose_draft_node(id: &str, block: &str) -> WorkflowNode {
    WorkflowNode {
        id: id.to_string(),
        kind: "compose".to_string(),
        block: block.to_string(),
        label: id.to_string(),
        needs_editspec: false,
        editspec_note: "本节点不用 EditSpec".to_string(),
        structure_path: None,
        light: "warn".to_string(),
        waiting: false,
        checks: WorkflowChecks {
            input: WorkflowCheck {
                ok: false,
                missing: format!("缺 {id}.pdb"),
            },
            ..WorkflowChecks::default()
        },
        tree_kinds: vec!["input".to_string(), "backbone".to_string()],
        condition_node: Some(id.to_string()),
        rounds: 0,
        inputs: Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowDeleteKind {
    Compose,
    Rfd,
    WholeLoop,
    ForbiddenRequiredStep,
    ForbiddenLoopSeed,
}

pub fn loop_from_id(graph: Option<&serde_json::Value>) -> Option<String> {
    let loop_block = graph?
        .get("loop")
        .filter(|value| !value.is_null())
        .or_else(|| graph?.get("optimize").filter(|value| !value.is_null()))?;
    loop_block
        .get("inputs")
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| {
            loop_block
                .get("from")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
}

pub fn classify_workflow_delete(
    nodes: &[WorkflowNode],
    index: usize,
    from_id: Option<&str>,
) -> Option<WorkflowDeleteKind> {
    let node = nodes.get(index)?;
    if node.kind == "loop" || node.kind == "gate" {
        return Some(WorkflowDeleteKind::WholeLoop);
    }
    if node.kind == "step" && node.block == "rfd" {
        return Some(WorkflowDeleteKind::Rfd);
    }
    if node.kind == "step" && matches!(node.block.as_str(), "mpnn" | "fold" | "evaluate") {
        return Some(WorkflowDeleteKind::ForbiddenRequiredStep);
    }
    if node.kind == "compose" {
        if from_id.is_some_and(|id| id == node.id) {
            return Some(WorkflowDeleteKind::ForbiddenLoopSeed);
        }
        return Some(WorkflowDeleteKind::Compose);
    }
    None
}

pub fn loop_draft_node(id: &str, rounds: u32) -> WorkflowNode {
    WorkflowNode {
        id: id.to_string(),
        kind: "loop".to_string(),
        block: "loop".to_string(),
        label: id.to_string(),
        needs_editspec: false,
        editspec_note: "本节点不用 EditSpec".to_string(),
        structure_path: None,
        light: "ok".to_string(),
        waiting: false,
        checks: WorkflowChecks::default(),
        tree_kinds: vec![
            "sequence".to_string(),
            "prediction".to_string(),
            "evaluation".to_string(),
            "selection".to_string(),
        ],
        condition_node: None,
        rounds,
        inputs: Vec::new(),
    }
}

pub fn gate_draft_node(loop_id: &str) -> WorkflowNode {
    WorkflowNode {
        id: format!("{loop_id}.gate"),
        kind: "gate".to_string(),
        block: "gate".to_string(),
        label: format!("{loop_id}.gate"),
        needs_editspec: false,
        editspec_note: "本节点不用 EditSpec".to_string(),
        structure_path: None,
        light: "warn".to_string(),
        waiting: true,
        checks: WorkflowChecks::default(),
        tree_kinds: vec!["selection".to_string()],
        condition_node: None,
        rounds: 0,
        inputs: Vec::new(),
    }
}

pub fn step_draft_node(loop_id: &str, step: &str) -> WorkflowNode {
    let needs = matches!(step, "mpnn" | "rfd");
    let kinds = match step {
        "mpnn" => vec!["sequence".to_string()],
        "rfd" => vec!["backbone".to_string()],
        "fold" => vec!["prediction".to_string(), "ensemble".to_string()],
        "evaluate" => vec!["evaluation".to_string()],
        _ => Vec::new(),
    };
    WorkflowNode {
        id: format!("{loop_id}.{step}"),
        kind: "step".to_string(),
        block: step.to_string(),
        label: format!("{loop_id}.{step}"),
        needs_editspec: needs,
        editspec_note: if needs {
            "该步要改序列但区间空".to_string()
        } else {
            "本节点不用 EditSpec".to_string()
        },
        structure_path: None,
        light: if needs { "warn" } else { "ok" }.to_string(),
        waiting: false,
        checks: if needs {
            WorkflowChecks {
                editspec: WorkflowCheck {
                    ok: false,
                    missing: "该步要改序列但区间空".to_string(),
                },
                ..WorkflowChecks::default()
            }
        } else {
            WorkflowChecks::default()
        },
        tree_kinds: kinds,
        condition_node: None,
        rounds: 0,
        inputs: Vec::new(),
    }
}

pub fn rfd_draft_node(loop_id: &str) -> WorkflowNode {
    step_draft_node(loop_id, "rfd")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowRewire {
    Compose { node_id: String, from_id: String },
    Loop { from_id: String },
}

pub fn node_eats(node: &WorkflowNode) -> &'static [&'static str] {
    match (node.kind.as_str(), node.block.as_str()) {
        ("compose", "import") | ("compose", "probe") | ("compose", "de_novo")
        | ("compose", "edit_rfd") => &[],
        ("compose", "graft") | ("compose", "tool_chain") => &["input", "backbone"],
        ("compose", _) => &["input", "backbone"],
        ("step", "rfd") => &["backbone"],
        ("step", "mpnn") => &["backbone"],
        ("step", "fold") => &["sequence"],
        ("step", "evaluate") => &["prediction"],
        ("gate", _) => &["prediction"],
        ("loop", _) => &["input", "backbone"],
        _ => &[],
    }
}

pub fn node_emits(node: &WorkflowNode) -> &'static [&'static str] {
    match (node.kind.as_str(), node.block.as_str()) {
        ("compose", _) => &["input", "backbone"],
        ("step", "rfd") => &["backbone"],
        ("step", "mpnn") => &["sequence"],
        ("step", "fold") => &["prediction"],
        ("step", "evaluate") => &["evaluation"],
        ("gate", _) => &["selection"],
        ("loop", _) => &["input", "backbone"],
        _ => &[],
    }
}

fn emits_seed(node: &WorkflowNode) -> bool {
    node_emits(node)
        .iter()
        .any(|kind| *kind == "input" || *kind == "backbone")
}

fn ports_match(producer: &WorkflowNode, consumer: &WorkflowNode) -> bool {
    node_eats(consumer)
        .iter()
        .any(|kind| node_emits(producer).contains(kind))
}

fn compose_would_cycle(nodes: &[WorkflowNode], child_id: &str, parent_id: &str) -> bool {
    if child_id == parent_id {
        return true;
    }
    let mut stack = vec![parent_id.to_string()];
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = stack.pop() {
        if id == child_id {
            return true;
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(node) = nodes.iter().find(|node| node.id == id) {
            stack.extend(node.inputs.iter().cloned());
        }
    }
    false
}

pub fn classify_workflow_rewire(
    nodes: &[WorkflowNode],
    dragged: usize,
    drop: usize,
) -> Option<WorkflowRewire> {
    if dragged == drop {
        return None;
    }
    let consumer = nodes.get(dragged)?;
    let producer = nodes.get(drop)?;
    if matches!(consumer.kind.as_str(), "loop" | "step" | "gate") {
        if producer.kind == "compose" && emits_seed(producer) {
            return Some(WorkflowRewire::Loop {
                from_id: producer.id.clone(),
            });
        }
        return None;
    }
    if consumer.kind != "compose" || producer.kind != "compose" {
        return None;
    }
    if node_eats(consumer).is_empty() || !ports_match(producer, consumer) {
        return None;
    }
    if compose_would_cycle(nodes, &consumer.id, &producer.id) {
        return None;
    }
    Some(WorkflowRewire::Compose {
        node_id: consumer.id.clone(),
        from_id: producer.id.clone(),
    })
}

pub fn detach_compose_source(
    graph: &mut serde_json::Value,
    nodes: &mut [WorkflowNode],
    removed_id: &str,
) {
    if let Some(compose) = graph.get_mut("compose").and_then(|value| value.as_array_mut()) {
        for item in compose {
            if let Some(inputs) = item.get_mut("inputs").and_then(|value| value.as_array_mut()) {
                inputs.retain(|value| value.as_str() != Some(removed_id));
            }
        }
    }
    for node in nodes.iter_mut() {
        if node.kind == "compose" {
            node.inputs.retain(|id| id != removed_id);
        }
    }
}

pub fn apply_workflow_rewire(
    graph: &mut serde_json::Value,
    nodes: &mut [WorkflowNode],
    rewire: &WorkflowRewire,
) {
    match rewire {
        WorkflowRewire::Loop { from_id } => {
            for key in ["loop", "optimize"] {
                if graph.get(key).is_some_and(|value| !value.is_null()) {
                    graph[key]["inputs"] = serde_json::json!([from_id]);
                }
            }
            if let Some(node) = nodes.iter_mut().find(|node| node.kind == "loop") {
                node.inputs = vec![from_id.clone()];
            }
        }
        WorkflowRewire::Compose { node_id, from_id } => {
            if let Some(compose) = graph.get_mut("compose").and_then(|value| value.as_array_mut()) {
                for item in compose {
                    if item.get("id").and_then(|value| value.as_str()) == Some(node_id.as_str()) {
                        item["inputs"] = serde_json::json!([from_id]);
                    }
                }
            }
            if let Some(node) = nodes.iter_mut().find(|node| node.id == *node_id) {
                node.inputs = vec![from_id.clone()];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_row_prefers_condition_node_then_kind() {
        let seed = compose_draft_node("seed", "import");
        assert!(tree_row_matches_node("input", Some("seed"), &seed));
        assert!(!tree_row_matches_node("input", Some("other"), &seed));
        let mpnn = WorkflowNode {
            id: "opt.mpnn".to_string(),
            kind: "step".to_string(),
            block: "mpnn".to_string(),
            label: "mpnn".to_string(),
            needs_editspec: true,
            editspec_note: String::new(),
            structure_path: None,
            light: "ok".to_string(),
            waiting: false,
            checks: WorkflowChecks::default(),
            tree_kinds: vec!["sequence".to_string()],
            condition_node: None,
            rounds: 0,
            inputs: Vec::new(),
        };
        assert!(tree_row_matches_node("sequence", None, &mpnn));
        assert!(!tree_row_matches_node("prediction", None, &mpnn));
        assert_eq!(
            workflow_index_for_tree_row(&[seed, mpnn], "sequence", None),
            Some(1)
        );
    }

    #[test]
    fn recipe_from_dump_keeps_compose_params_and_loop_steps() {
        let graph = serde_json::json!({
            "compose": [{"id": "seed", "block": "import", "inputs": [], "params": {"input": "seed.pdb"}}],
            "loop": {
                "id": "optimize",
                "inputs": ["seed"],
                "rounds": 2,
                "steps": ["mpnn", "fold", "evaluate"],
                "passthrough_rfd": true,
                "edit_spec": "A1-10~",
                "gate": {"mode": "human"}
            }
        });
        let recipe = recipe_from_graph_dump(&graph);
        assert_eq!(recipe["kind"], "workflow");
        assert_eq!(recipe["compose"][0]["input"], "seed.pdb");
        assert_eq!(recipe["optimize"]["from"], "seed");
        assert_eq!(recipe["optimize"]["steps"][1], "fold");
    }

    #[test]
    fn classify_delete_protects_required_steps_and_loop_seed() {
        let nodes = vec![
            compose_draft_node("seed", "import"),
            loop_draft_node("optimize", 2),
            step_draft_node("optimize", "mpnn"),
            gate_draft_node("optimize"),
        ];
        assert_eq!(
            classify_workflow_delete(&nodes, 0, Some("seed")),
            Some(WorkflowDeleteKind::ForbiddenLoopSeed)
        );
        assert_eq!(
            classify_workflow_delete(&nodes, 2, Some("seed")),
            Some(WorkflowDeleteKind::ForbiddenRequiredStep)
        );
        assert_eq!(
            classify_workflow_delete(&nodes, 1, Some("seed")),
            Some(WorkflowDeleteKind::WholeLoop)
        );
        let extra = compose_draft_node("probe-1", "probe");
        let mut more = nodes;
        more.push(extra);
        let last = more.len() - 1;
        assert_eq!(
            classify_workflow_delete(&more, last, Some("seed")),
            Some(WorkflowDeleteKind::Compose)
        );
    }

    #[test]
    fn rewire_loop_onto_seed_and_reject_import_onto_import() {
        let nodes = vec![
            compose_draft_node("seed", "import"),
            compose_draft_node("extra", "import"),
            loop_draft_node("optimize", 2),
            step_draft_node("optimize", "mpnn"),
        ];
        assert_eq!(
            classify_workflow_rewire(&nodes, 2, 0),
            Some(WorkflowRewire::Loop {
                from_id: "seed".to_string()
            })
        );
        assert_eq!(
            classify_workflow_rewire(&nodes, 3, 0),
            Some(WorkflowRewire::Loop {
                from_id: "seed".to_string()
            })
        );
        assert_eq!(classify_workflow_rewire(&nodes, 1, 0), None);
        assert_eq!(classify_workflow_rewire(&nodes, 0, 1), None);
    }

    #[test]
    fn rewire_graft_onto_import_and_reject_cycle() {
        let mut graft = compose_draft_node("graft", "graft");
        graft.inputs = vec!["seed".to_string()];
        let nodes = vec![compose_draft_node("seed", "import"), graft];
        assert_eq!(
            classify_workflow_rewire(&nodes, 1, 0),
            Some(WorkflowRewire::Compose {
                node_id: "graft".to_string(),
                from_id: "seed".to_string()
            })
        );
        assert_eq!(classify_workflow_rewire(&nodes, 0, 1), None);
        let mut graph = serde_json::json!({
            "compose": [
                {"id": "seed", "block": "import", "inputs": []},
                {"id": "graft", "block": "graft", "inputs": []}
            ],
            "loop": {"id": "optimize", "inputs": ["seed"]}
        });
        let mut live = vec![
            compose_draft_node("seed", "import"),
            compose_draft_node("graft", "graft"),
            loop_draft_node("optimize", 1),
        ];
        apply_workflow_rewire(
            &mut graph,
            &mut live,
            &WorkflowRewire::Compose {
                node_id: "graft".to_string(),
                from_id: "seed".to_string(),
            },
        );
        assert_eq!(graph["compose"][1]["inputs"][0], "seed");
        assert_eq!(live[1].inputs, vec!["seed".to_string()]);
        apply_workflow_rewire(
            &mut graph,
            &mut live,
            &WorkflowRewire::Loop {
                from_id: "graft".to_string(),
            },
        );
        assert_eq!(graph["loop"]["inputs"][0], "graft");
        assert_eq!(live[2].inputs, vec!["graft".to_string()]);
    }

    #[test]
    fn detach_compose_source_clears_from_without_touching_loop() {
        let mut graph = serde_json::json!({
            "compose": [
                {"id": "seed", "block": "import", "inputs": []},
                {"id": "graft", "block": "graft", "inputs": ["seed"]},
                {"id": "extra", "block": "import", "inputs": []}
            ],
            "loop": {"id": "optimize", "inputs": ["graft"]}
        });
        let mut live = vec![
            compose_draft_node("seed", "import"),
            compose_draft_node("graft", "graft"),
            compose_draft_node("extra", "import"),
            loop_draft_node("optimize", 1),
        ];
        live[1].inputs = vec!["seed".to_string()];
        live[3].inputs = vec!["graft".to_string()];
        detach_compose_source(&mut graph, &mut live, "seed");
        assert!(graph["compose"][1]["inputs"].as_array().unwrap().is_empty());
        assert!(live[1].inputs.is_empty());
        assert_eq!(graph["loop"]["inputs"][0], "graft");
        assert_eq!(live[3].inputs, vec!["graft".to_string()]);
    }
}
