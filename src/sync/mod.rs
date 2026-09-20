pub mod github;
pub mod registry;
pub mod shortcut;
pub mod state;

use std::path::Path;
use std::process::Command;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;

/// One task's metadata as carried in HTML comments inside an issue body or
/// story description. Every field is optional because remote bodies may be
/// hand-edited or written by older versions.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CommentMetadata {
    pub id: Option<String>,
    pub parent_id: Option<String>,
    pub priority: Option<i64>,
    pub completed: Option<bool>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub started_at: Option<Option<String>>,
    pub completed_at: Option<Option<String>>,
    pub blocked_by: Option<Vec<String>>,
    pub blocks: Option<Vec<String>>,
    pub result: Option<String>,
    pub commit: Option<Value>,
}

pub fn encode_metadata_value(value: &str) -> String {
    if value.contains('\n') || value.contains("-->") || value.starts_with("base64:") {
        format!("base64:{}", BASE64.encode(value.as_bytes()))
    } else {
        value.to_string()
    }
}

pub fn decode_metadata_value(value: &str) -> String {
    match value.strip_prefix("base64:") {
        Some(encoded) => BASE64
            .decode(encoded)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_else(|| value.to_string()),
        None => value.to_string(),
    }
}

/// Parses `<!-- dex:<prefix>:<key>:<value> -->` comments. `prefix` is
/// `task` for root issues and story descriptions, `subtask` for embedded
/// blocks. Returns None when no comment with the prefix is present.
pub fn parse_metadata_comments(text: &str, prefix: &str) -> Option<CommentMetadata> {
    let pattern =
        regex::Regex::new(&format!(r"<!-- dex:{prefix}:(\w+):(.*?) -->")).expect("static");
    let mut metadata = CommentMetadata::default();
    let mut commit = serde_json::Map::new();
    let mut found = false;
    for capture in pattern.captures_iter(text) {
        found = true;
        let key = &capture[1];
        let raw = &capture[2];
        let value = decode_metadata_value(raw);
        match key {
            "commit_sha" => {
                commit.insert("sha".into(), Value::String(value));
            }
            "commit_message" => {
                commit.insert("message".into(), Value::String(value));
            }
            "commit_branch" => {
                commit.insert("branch".into(), Value::String(value));
            }
            "commit_url" => {
                commit.insert("url".into(), Value::String(value));
            }
            "commit_timestamp" => {
                commit.insert("timestamp".into(), Value::String(value));
            }
            "id" => metadata.id = Some(value),
            "parent_id" | "parent" => metadata.parent_id = Some(value),
            "priority" => metadata.priority = value.parse().ok(),
            "completed" => metadata.completed = Some(raw == "true"),
            "status" => {
                if metadata.completed.is_none() {
                    metadata.completed = Some(raw == "completed");
                }
            }
            "created_at" => metadata.created_at = Some(value),
            "updated_at" => metadata.updated_at = Some(value),
            "started_at" => metadata.started_at = Some((raw != "null").then_some(value)),
            "completed_at" => metadata.completed_at = Some((raw != "null").then_some(value)),
            "blockedBy" => metadata.blocked_by = Some(parse_json_array(&value)),
            "blocks" => metadata.blocks = Some(parse_json_array(&value)),
            "result" => metadata.result = Some(value),
            _ => {}
        }
    }
    if !found {
        return None;
    }
    if commit.contains_key("sha") {
        metadata.commit = Some(Value::Object(commit));
    }
    Some(metadata)
}

fn parse_json_array(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
}

pub fn is_commit_on_remote(cwd: &Path, sha: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", sha, "origin/HEAD"])
        .current_dir(cwd)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
