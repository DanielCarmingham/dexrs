use serde_json::Value;

use crate::sync::{CommentMetadata, encode_metadata_value, parse_metadata_comments};
use crate::task::{Task, timestamp};

pub const TASKS_HEADER: &str = "## Tasks";
pub const SUBTASKS_HEADER: &str = "## Subtasks";
const LEGACY_TREE_HEADER: &str = "## Task Tree";
const LEGACY_DETAILS_HEADER: &str = "## Task Details";

#[derive(Debug, Clone)]
pub struct Descendant {
    pub task: Task,
    pub depth: usize,
    pub parent_id: String,
}

#[derive(Debug, Clone)]
pub struct ParsedSubtask {
    pub task: Task,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedBody {
    pub description: String,
    pub subtasks: Vec<ParsedSubtask>,
}

pub fn collect_descendants(tasks: &[Task], root_id: &str) -> Vec<Descendant> {
    let mut result = Vec::new();
    collect(tasks, root_id, 0, &mut result);
    result
}

fn collect(tasks: &[Task], parent_id: &str, depth: usize, out: &mut Vec<Descendant>) {
    let mut children: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.parent_id.as_deref() == Some(parent_id))
        .collect();
    children.sort_by_key(|task| task.priority);
    for child in children {
        out.push(Descendant {
            task: child.clone(),
            depth,
            parent_id: parent_id.to_string(),
        });
        collect(tasks, &child.id, depth + 1, out);
    }
}

pub fn render_task_metadata_comments(
    task: &Task,
    prefix: &str,
    parent_id: Option<&str>,
) -> Vec<String> {
    let comment = |key: &str, value: &str| format!("<!-- dex:{prefix}:{key}:{value} -->");
    let mut lines = vec![comment("id", &task.id)];
    if let Some(parent_id) = parent_id {
        lines.push(comment("parent", parent_id));
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
        "started_at",
        task.started_at.as_deref().unwrap_or("null"),
    ));
    lines.push(comment(
        "completed_at",
        task.completed_at.as_deref().unwrap_or("null"),
    ));
    lines.push(comment(
        "blockedBy",
        &serde_json::to_string(&task.blocked_by).expect("strings"),
    ));
    lines.push(comment(
        "blocks",
        &serde_json::to_string(&task.blocks).expect("strings"),
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
    lines
}

fn render_details_block(descendant: &Descendant) -> String {
    let task = &descendant.task;
    let status = if task.completed { "✅ " } else { "" };
    let tree = if descendant.depth > 0 { "└─ " } else { "" };
    let mut lines = vec![
        "<details>".to_string(),
        format!("<summary>{status}{tree}<b>{}</b></summary>", task.name),
        String::new(),
    ];
    lines.extend(render_task_metadata_comments(
        task,
        "subtask",
        Some(&descendant.parent_id),
    ));
    lines.push(String::new());
    if !task.description.is_empty() {
        lines.push("### Description".into());
        lines.push(task.description.clone());
        lines.push(String::new());
    }
    if let Some(result) = task.result.as_deref().filter(|result| !result.is_empty()) {
        lines.push("### Result".into());
        lines.push(result.to_string());
        lines.push(String::new());
    }
    lines.push("</details>".into());
    lines.join("\n")
}

pub fn render_issue_body(context: &str, descendants: &[Descendant]) -> String {
    if descendants.is_empty() {
        return context.to_string();
    }
    let blocks: Vec<String> = descendants.iter().map(render_details_block).collect();
    format!("{context}\n\n{TASKS_HEADER}\n\n{}\n", blocks.join("\n\n"))
}

pub fn render_root_body(task: &Task, descendants: &[Descendant]) -> String {
    let meta = render_task_metadata_comments(task, "task", None).join("\n");
    format!(
        "{meta}\n{}",
        render_issue_body(&task.description, descendants)
    )
}

pub fn parse_root_task_metadata(body: &str) -> Option<CommentMetadata> {
    if let Some(metadata) = parse_metadata_comments(body, "task") {
        return Some(metadata);
    }
    let legacy = regex::Regex::new(r"<!-- dex:task:([a-zA-Z0-9]+) -->").expect("static");
    legacy.captures(body).map(|capture| CommentMetadata {
        id: Some(capture[1].to_string()),
        ..CommentMetadata::default()
    })
}

pub fn extract_task_id(body: &str) -> Option<String> {
    let current = regex::Regex::new(r"<!-- dex:task:id:([a-z0-9]+) -->").expect("static");
    if let Some(capture) = current.captures(body) {
        return Some(capture[1].to_string());
    }
    let legacy = regex::Regex::new(r"<!-- dex:task:([a-z0-9]+) -->").expect("static");
    legacy.captures(body).map(|capture| capture[1].to_string())
}

pub fn strip_task_comments(text: &str) -> String {
    regex::Regex::new(r"<!-- dex:task:[^\s]+ -->\n?")
        .expect("static")
        .replace_all(text, "")
        .trim()
        .to_string()
}

pub fn parse_hierarchical_issue_body(body: &str) -> ParsedBody {
    let headers = [
        TASKS_HEADER,
        SUBTASKS_HEADER,
        LEGACY_TREE_HEADER,
        LEGACY_DETAILS_HEADER,
    ];
    let first = headers
        .iter()
        .filter_map(|header| body.find(header).map(|index| (index, *header)))
        .min_by_key(|(index, _)| *index);
    let Some((index, header)) = first else {
        return ParsedBody {
            description: body.trim().to_string(),
            subtasks: Vec::new(),
        };
    };
    let description = body[..index].trim().to_string();
    let section_start = if header == TASKS_HEADER || header == SUBTASKS_HEADER {
        Some(index + header.len())
    } else {
        body.find(LEGACY_DETAILS_HEADER)
            .map(|details| details + LEGACY_DETAILS_HEADER.len())
    };
    let Some(start) = section_start else {
        return ParsedBody {
            description,
            subtasks: Vec::new(),
        };
    };
    let details = regex::Regex::new(r"(?s)<details>(.*?)</details>").expect("static");
    let subtasks = details
        .captures_iter(&body[start..])
        .filter_map(|capture| parse_details_block(&capture[1]))
        .collect();
    ParsedBody {
        description,
        subtasks,
    }
}

fn parse_details_block(content: &str) -> Option<ParsedSubtask> {
    let current = regex::Regex::new(r"(?i)<summary>\s*(✅\s*)?(└─\s*)?<b>(.+?)</b>\s*</summary>")
        .expect("static");
    let legacy =
        regex::Regex::new(r"(?i)<summary>\s*\[([ x])\]\s*(.*?)\s*</summary>").expect("static");
    let (name, completed_hint) = if let Some(capture) = current.captures(content) {
        (capture[3].trim().to_string(), capture.get(1).is_some())
    } else {
        let capture = legacy.captures(content)?;
        let cleaned = regex::Regex::new(r"^↳+\s*")
            .expect("static")
            .replace(capture[2].trim(), "");
        let cleaned = regex::Regex::new(r"</?b>")
            .expect("static")
            .replace_all(&cleaned, "");
        let cleaned = regex::Regex::new(r"<code>.*?</code>")
            .expect("static")
            .replace_all(&cleaned, "");
        (
            cleaned.trim().to_string(),
            capture[1].eq_ignore_ascii_case("x"),
        )
    };

    let metadata = parse_metadata_comments(content, "subtask")?;
    let id = metadata.id.clone()?;
    let section = |title: &str| -> Option<String> {
        regex::Regex::new(&format!(r"(?s)### {title}\s*\n(.*?)(?:###|\z)"))
            .expect("static")
            .captures(content)
            .map(|capture| capture[1].trim().to_string())
    };
    let now = timestamp();
    let task = Task {
        id,
        parent_id: None,
        name,
        description: section("(?:Description|Context)").unwrap_or_default(),
        priority: metadata.priority.unwrap_or(1),
        completed: metadata.completed.unwrap_or(completed_hint),
        result: section("Result"),
        metadata: metadata
            .commit
            .clone()
            .map(|commit| serde_json::json!({ "commit": commit })),
        created_at: Some(metadata.created_at.clone().unwrap_or_else(|| now.clone())),
        updated_at: Some(metadata.updated_at.clone().unwrap_or_else(|| now.clone())),
        started_at: metadata.started_at.clone().flatten(),
        completed_at: metadata.completed_at.clone().flatten(),
        blocked_by: metadata.blocked_by.clone().unwrap_or_default(),
        blocks: metadata.blocks.clone().unwrap_or_default(),
        children: Vec::new(),
    };
    Some(ParsedSubtask {
        task,
        parent_id: metadata.parent_id,
    })
}
