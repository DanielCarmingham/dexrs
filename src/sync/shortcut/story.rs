use serde_json::Value;

use crate::sync::{CommentMetadata, encode_metadata_value, parse_metadata_comments};
use crate::task::Task;

pub struct ParsedStory {
    pub context: String,
    pub metadata: Option<CommentMetadata>,
}

pub fn render_story_description(task: &Task) -> String {
    let comment = |key: &str, value: &str| format!("<!-- dex:task:{key}:{value} -->");
    let mut lines = vec![comment("id", &task.id)];
    if let Some(parent_id) = &task.parent_id {
        lines.push(comment("parent_id", parent_id));
    }
    lines.push(comment("priority", &task.priority.to_string()));
    lines.push(comment("completed", &task.completed.to_string()));
    lines.push(comment(
        "created_at",
        task.created_at.as_deref().unwrap_or(""),
    ));
    lines.push(comment(
        "updated_at",
        task.updated_at.as_deref().unwrap_or(""),
    ));
    lines.push(comment(
        "completed_at",
        task.completed_at.as_deref().unwrap_or("null"),
    ));
    if let Some(result) = task.result.as_deref().filter(|result| !result.is_empty()) {
        lines.push(comment("result", &encode_metadata_value(result)));
    }
    if let Some(commit) = task.metadata.as_ref().and_then(|m| m.get("commit")) {
        let field = |key: &str| commit.get(key).and_then(Value::as_str);
        if let Some(sha) = field("sha") {
            lines.push(comment("commit_sha", sha));
        }
        if let Some(message) = field("message") {
            lines.push(comment("commit_message", &encode_metadata_value(message)));
        }
        if let Some(branch) = field("branch") {
            lines.push(comment("commit_branch", branch));
        }
        if let Some(url) = field("url") {
            lines.push(comment("commit_url", url));
        }
        if let Some(stamp) = field("timestamp") {
            lines.push(comment("commit_timestamp", stamp));
        }
    }
    if !task.description.is_empty() {
        lines.push(String::new());
        lines.push(task.description.clone());
    }
    lines.join("\n")
}

pub fn parse_story_description(description: &str) -> ParsedStory {
    ParsedStory {
        context: crate::sync::github::body::strip_task_comments(description),
        metadata: parse_metadata_comments(description, "task"),
    }
}
