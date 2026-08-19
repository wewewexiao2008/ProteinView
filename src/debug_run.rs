//! Debug Run argv and console waiting hints. No GPU / Fleet spawn here.

use serde_json::Value;

pub fn campaign_recipe_path(campaign: &str) -> Option<String> {
    let root = std::path::Path::new(campaign);
    for name in ["config/run.yaml", "config/run.yml", "recipe.yaml"] {
        let path = root.join(name);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
}

pub fn fleet_schedule_argv(
    gemlib_bin: &str,
    campaign: &str,
    priority: &str,
    concurrency: u32,
) -> Vec<String> {
    vec![
        gemlib_bin.to_string(),
        "fleet".to_string(),
        "schedule".to_string(),
        "-o".to_string(),
        campaign.to_string(),
        "--priority".to_string(),
        priority.to_string(),
        "--concurrency".to_string(),
        concurrency.to_string(),
    ]
}

pub fn debug_run_argv(gemlib_bin: &str, recipe: &str, campaign: &str) -> Vec<String> {
    vec![
        gemlib_bin.to_string(),
        "pipeline-run".to_string(),
        recipe.to_string(),
        "-o".to_string(),
        campaign.to_string(),
        "--debug".to_string(),
    ]
}

pub struct ConsoleRecord {
    pub event: String,
    pub stage: String,
    pub message: String,
}

pub fn parse_console_records(text: &str) -> Vec<ConsoleRecord> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let event = value
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let stage = value
            .get("stage")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{event} {stage}").trim().to_string());
        rows.push(ConsoleRecord {
            event,
            stage,
            message,
        });
    }
    rows
}

pub fn last_console_hint(text: &str) -> Option<String> {
    let mut hint = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            hint = Some(message.to_string());
        } else if let Some(event) = value.get("event").and_then(Value::as_str) {
            let stage = value.get("stage").and_then(Value::as_str).unwrap_or("");
            hint = Some(format!("{event} {stage}").trim().to_string());
        }
    }
    hint
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fleet_schedule_argv_uses_priority_and_concurrency() {
        let argv = fleet_schedule_argv("/tmp/gemlib", "/tmp/camp", "fast", 4);
        assert_eq!(
            argv,
            vec![
                "/tmp/gemlib",
                "fleet",
                "schedule",
                "-o",
                "/tmp/camp",
                "--priority",
                "fast",
                "--concurrency",
                "4"
            ]
        );
    }

    #[test]
    fn debug_run_argv_uses_pipeline_run_debug() {
        let argv = debug_run_argv("/tmp/gemlib", "/tmp/camp/config/run.yaml", "/tmp/camp");
        assert_eq!(
            argv,
            vec![
                "/tmp/gemlib",
                "pipeline-run",
                "/tmp/camp/config/run.yaml",
                "-o",
                "/tmp/camp",
                "--debug"
            ]
        );
    }

    #[test]
    fn parse_console_records_keeps_waiting_and_placed() {
        let text = concat!(
            "{\"event\":\"waiting\",\"stage\":\"fold\",\"message\":\"waiting fold … (3s)\"}\n",
            "{\"event\":\"placed\",\"stage\":\"fold\",\"message\":\"placed fold\"}\n",
        );
        let rows = parse_console_records(text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].event, "waiting");
        assert_eq!(rows[1].event, "placed");
    }

    #[test]
    fn console_waiting_hint_reads_last_message() {
        let text = concat!(
            "{\"event\":\"waiting\",\"stage\":\"mpnn\",\"message\":\"waiting mpnn … (3s)\"}\n",
            "{\"event\":\"placed\",\"stage\":\"mpnn\",\"message\":\"placed mpnn\"}\n",
        );
        assert_eq!(last_console_hint(text).as_deref(), Some("placed mpnn"));
        assert!(
            last_console_hint("{\"event\":\"waiting\",\"stage\":\"fold\",\"message\":\"waiting fold … (3s)\"}\n")
                .unwrap()
                .contains("waiting fold")
        );
    }
}
