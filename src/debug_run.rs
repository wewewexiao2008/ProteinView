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
