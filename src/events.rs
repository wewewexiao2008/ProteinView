//! Session event log for the Studio console. Not a campaign wire object.

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventLevel {
    Session,
    Intent,
    Nav,
    Run,
}

impl EventLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Intent => "intent",
            Self::Nav => "nav",
            Self::Run => "run",
        }
    }

    pub fn visible_by_default(self) -> bool {
        !matches!(self, Self::Nav)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPane {
    Workflow,
    Tree,
    View,
    EditSpec,
    None,
}

impl EventPane {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Tree => "tree",
            Self::View => "view",
            Self::EditSpec => "editspec",
            Self::None => "—",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StudioEvent {
    pub id: u64,
    pub ts: String,
    pub level: EventLevel,
    pub pane: EventPane,
    pub op: String,
    pub undoable: bool,
    pub summary: String,
    pub payload: Value,
}

impl StudioEvent {
    pub fn tag(&self) -> String {
        match self.pane {
            EventPane::None => self.level.as_str().to_string(),
            pane => format!("{}:{}", pane.as_str(), self.op),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EventLog {
    events: Vec<StudioEvent>,
    next_id: u64,
}

impl EventLog {
    pub fn emit(
        &mut self,
        level: EventLevel,
        pane: EventPane,
        op: &str,
        undoable: bool,
        summary: impl Into<String>,
        payload: Value,
    ) -> &StudioEvent {
        self.next_id += 1;
        self.events.push(StudioEvent {
            id: self.next_id,
            ts: now_ts(),
            level,
            pane,
            op: op.to_string(),
            undoable,
            summary: summary.into(),
            payload,
        });
        self.events.last().expect("just pushed")
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn events(&self) -> &[StudioEvent] {
        &self.events
    }

    pub fn visible<'a>(&'a self, verbose: bool) -> Vec<&'a StudioEvent> {
        self.events
            .iter()
            .filter(|event| verbose || event.level.visible_by_default())
            .collect()
    }

    pub fn newest_visible_summary(&self, verbose: bool) -> Option<String> {
        self.visible(verbose).last().map(|event| event.summary.clone())
    }
}

pub fn run_op_from_console_event(event: &str) -> &'static str {
    match event {
        "waiting" => "run.waiting",
        "placed" => "run.placed",
        "missing" => "run.missing",
        "gate" => "run.gate",
        _ => "run.status",
    }
}

fn now_ts() -> String {
    chrono_like_now()
}

fn chrono_like_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn empty_payload() -> Value {
    json!({})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_has_required_fields() {
        let mut log = EventLog::default();
        let event = log
            .emit(
                EventLevel::Intent,
                EventPane::Workflow,
                "workflow.rewire",
                true,
                "rewire loop ← seed",
                json!({"from": "seed"}),
            )
            .clone();
        assert_eq!(event.id, 1);
        assert_eq!(event.level, EventLevel::Intent);
        assert_eq!(event.pane, EventPane::Workflow);
        assert_eq!(event.op, "workflow.rewire");
        assert!(event.undoable);
        assert!(!event.ts.is_empty());
        assert_eq!(event.payload["from"], "seed");
        assert_eq!(event.tag(), "workflow:workflow.rewire");
    }

    #[test]
    fn default_filter_hides_nav() {
        let mut log = EventLog::default();
        log.emit(
            EventLevel::Nav,
            EventPane::Workflow,
            "workflow.cursor",
            false,
            "cursor mpnn",
            empty_payload(),
        );
        log.emit(
            EventLevel::Session,
            EventPane::None,
            "session.focus",
            false,
            "focus Tree",
            empty_payload(),
        );
        log.emit(
            EventLevel::Run,
            EventPane::None,
            "run.waiting",
            false,
            "waiting fold … (3s)",
            empty_payload(),
        );
        let hidden = log.visible(false);
        assert_eq!(hidden.len(), 2);
        assert!(hidden.iter().all(|event| event.op != "workflow.cursor"));
        let shown = log.visible(true);
        assert_eq!(shown.len(), 3);
        assert_eq!(shown[0].op, "workflow.cursor");
    }

    #[test]
    fn run_ops_map_from_jsonl_event() {
        assert_eq!(run_op_from_console_event("waiting"), "run.waiting");
        assert_eq!(run_op_from_console_event("placed"), "run.placed");
        assert_eq!(run_op_from_console_event("missing"), "run.missing");
    }
}
