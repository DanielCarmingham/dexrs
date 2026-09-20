use serde_json::{Value, json};

use crate::archive::ArchivedTask;
use crate::listing::{find, status_icon};
use crate::task::{DEFAULT_PRIORITY, Task};

const TEXT_MAX: usize = 300;
const TREE_NAME_MAX: usize = 50;

pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(3)).collect();
    format!("{cut}...")
}

struct Context<'a> {
    ancestors: Vec<&'a Task>,
    children: Vec<&'a Task>,
    grandchildren: Vec<&'a Task>,
    open_blockers: Vec<&'a Task>,
    blocks: Vec<&'a Task>,
}

fn context<'a>(tasks: &'a [Task], task: &'a Task) -> Context<'a> {
    let mut ancestors = Vec::new();
    let mut current = task.parent_id.as_deref().and_then(|id| find(tasks, id));
    while let Some(ancestor) = current {
        ancestors.push(ancestor);
        current = ancestor.parent_id.as_deref().and_then(|id| find(tasks, id));
    }
    ancestors.reverse();

    let mut children: Vec<&Task> = task
        .children
        .iter()
        .filter_map(|id| find(tasks, id))
        .collect();
    children.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then(left.completed.cmp(&right.completed))
    });
    let grandchildren = children
        .iter()
        .flat_map(|child| child.children.iter().filter_map(|id| find(tasks, id)))
        .collect();
    Context {
        ancestors,
        children,
        grandchildren,
        open_blockers: task
            .blocked_by
            .iter()
            .filter_map(|id| find(tasks, id))
            .filter(|blocker| !blocker.completed)
            .collect(),
        blocks: task
            .blocks
            .iter()
            .filter_map(|id| find(tasks, id))
            .collect(),
    }
}

pub fn render(tasks: &[Task], task: &Task, full: bool, expand: bool) -> String {
    let ctx = context(tasks, task);
    let mut out = String::new();
    let mut truncated = false;

    if ctx.ancestors.is_empty() && ctx.children.is_empty() {
        let priority = if task.priority != DEFAULT_PRIORITY {
            format!(" [p{}]", task.priority)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "{} {}{priority}: {}\n\n",
            status_icon(task),
            task.id,
            task.name
        ));
    } else {
        render_tree(&ctx, task, expand, &mut out);
        out.push('\n');
    }

    push_related(&mut out, "Blocked by", &ctx.open_blockers);
    let open_blocked: Vec<&Task> = ctx
        .blocks
        .iter()
        .copied()
        .filter(|blocked| !blocked.completed)
        .collect();
    push_related(&mut out, "Blocks", &open_blocked);

    out.push_str("Description:\n");
    truncated |= push_text(&mut out, &task.description, full);
    if let Some(result) = &task.result {
        out.push_str("\nResult:\n");
        truncated |= push_text(&mut out, result, full);
    }

    if let Some(commit) = task.metadata.as_ref().and_then(|m| m.get("commit")) {
        out.push_str("\nCommit:\n");
        out.push_str(&format!("  SHA:    {}\n", text(commit, "sha")));
        if let Some(message) = commit.get("message").and_then(Value::as_str) {
            out.push_str(&format!("  Message: {message}\n"));
        }
        if let Some(branch) = commit.get("branch").and_then(Value::as_str) {
            out.push_str(&format!("  Branch:  {branch}\n"));
        }
        if let Some(url) = commit.get("url").and_then(Value::as_str) {
            out.push_str(&format!("  URL:     {url}\n"));
        }
    }

    let own_github = task.metadata.as_ref().and_then(|m| m.get("github"));
    let inherited = ctx
        .ancestors
        .iter()
        .rev()
        .find_map(|ancestor| ancestor.metadata.as_ref().and_then(|m| m.get("github")));
    if let Some(github) = own_github.or(inherited) {
        let via_parent = if own_github.is_none() {
            " (via parent)"
        } else {
            ""
        };
        out.push_str("\nGitHub Issue:\n");
        out.push_str(&format!(
            "  #{} ({}){via_parent}\n",
            text(github, "issueNumber"),
            text(github, "repo")
        ));
        out.push_str(&format!("  {}\n", text(github, "issueUrl")));
    }

    out.push('\n');
    push_timestamp(&mut out, "Created:  ", task.created_at.as_deref());
    push_timestamp(&mut out, "Updated:  ", task.updated_at.as_deref());
    push_timestamp(&mut out, "Started:  ", task.started_at.as_deref());
    push_timestamp(&mut out, "Completed:", task.completed_at.as_deref());

    let parent = ctx.ancestors.last();
    if parent.is_some() || !ctx.children.is_empty() || truncated {
        out.push_str("\nMore Information:\n");
        if let Some(parent) = parent {
            out.push_str(&format!("  • View parent task: dex show {}\n", parent.id));
        }
        if !ctx.children.is_empty() {
            out.push_str(&format!("  • View subtree: dex list {}\n", task.id));
        }
        if truncated {
            out.push_str(&format!(
                "  • View full content: dex show {} --full\n",
                task.id
            ));
        }
    }
    out
}

pub fn enriched_json(tasks: &[Task], task: &Task, expand: bool) -> Value {
    let ctx = context(tasks, task);
    let mut value = serde_json::to_value(task).expect("task serializes");
    let object = value.as_object_mut().expect("task is an object");
    let brief =
        |task: &Task| json!({"id": task.id, "name": task.name, "completed": task.completed});

    object.insert(
        "ancestors".into(),
        ctx.ancestors
            .iter()
            .map(|ancestor| {
                if expand {
                    json!({"id": ancestor.id, "name": ancestor.name, "description": ancestor.description})
                } else {
                    json!({"id": ancestor.id, "name": ancestor.name})
                }
            })
            .collect(),
    );
    object.insert("depth".into(), json!(ctx.ancestors.len()));
    let pending = ctx.children.iter().filter(|child| !child.completed).count();
    object.insert(
        "subtasks".into(),
        json!({
            "pending": pending,
            "completed": ctx.children.len() - pending,
            "children": ctx.children,
        }),
    );
    let pending_grandchildren = ctx
        .grandchildren
        .iter()
        .filter(|child| !child.completed)
        .count();
    object.insert(
        "grandchildren".into(),
        if ctx.grandchildren.is_empty() {
            Value::Null
        } else {
            json!({
                "pending": pending_grandchildren,
                "completed": ctx.grandchildren.len() - pending_grandchildren,
                "tasks": ctx.grandchildren,
            })
        },
    );
    object.insert(
        "blockedBy".into(),
        ctx.open_blockers.iter().map(|t| brief(t)).collect(),
    );
    object.insert(
        "blocks".into(),
        ctx.blocks.iter().map(|t| brief(t)).collect(),
    );
    object.insert("isBlocked".into(), json!(!ctx.open_blockers.is_empty()));
    value
}

pub fn archived_json(record: &ArchivedTask) -> Value {
    let mut value = serde_json::to_value(record).expect("archived task serializes");
    value
        .as_object_mut()
        .expect("record is an object")
        .insert("archived".into(), json!(true));
    value
}

pub fn render_archived(record: &ArchivedTask) -> String {
    let mut out = format!("[x] {}: {}", record.id, record.name);
    match record.archived_children.len() {
        0 => {}
        1 => out.push_str(" (1 subtask)"),
        count => out.push_str(&format!(" ({count} subtasks)")),
    }
    out.push_str(" (ARCHIVED)\n\nDescription:\n");
    let description = if record.description.is_empty() {
        "(no description)"
    } else {
        record.description.as_str()
    };
    push_text(&mut out, description, true);
    if let Some(result) = &record.result {
        out.push_str("\nResult:\n");
        push_text(&mut out, result, true);
    }
    out.push('\n');
    push_timestamp(&mut out, "Completed:", record.completed_at.as_deref());
    push_timestamp(&mut out, "Archived: ", Some(&record.archived_at));
    out
}

fn render_tree(ctx: &Context<'_>, task: &Task, expand: bool, out: &mut String) {
    for (depth, ancestor) in ctx.ancestors.iter().enumerate() {
        let indent = if depth == 0 {
            String::new()
        } else {
            "    ".repeat(depth - 1)
        };
        let connector = if depth == 0 { "" } else { "└── " };
        out.push_str(&format!("{indent}{connector}{}\n", tree_line(ancestor, 0)));
        if expand && !ancestor.description.is_empty() {
            let extra = if depth == 0 { "" } else { "    " };
            out.push_str(&format!(
                "{indent}{extra}      {}\n",
                truncate(&ancestor.description, TEXT_MAX)
            ));
        }
    }

    let depth = ctx.ancestors.len();
    let current_indent = if depth > 1 {
        "    ".repeat(depth - 1)
    } else {
        String::new()
    };
    let connector = if depth > 0 { "└── " } else { "" };
    out.push_str(&format!(
        "{current_indent}{connector}{}  ← viewing\n",
        tree_line(task, ctx.children.len())
    ));

    let child_indent = if depth > 0 {
        format!("{current_indent}    ")
    } else {
        String::new()
    };
    let last = ctx.children.len().saturating_sub(1);
    for (index, child) in ctx.children.iter().enumerate() {
        let connector = if index == last {
            "└── "
        } else {
            "├── "
        };
        let grandchildren = ctx
            .grandchildren
            .iter()
            .filter(|g| g.parent_id.as_deref() == Some(child.id.as_str()))
            .count();
        out.push_str(&format!(
            "{child_indent}{connector}{}\n",
            tree_line(child, grandchildren)
        ));
    }
}

fn tree_line(task: &Task, child_count: usize) -> String {
    let mut line = format!(
        "{} {}: {}",
        status_icon(task),
        task.id,
        truncate(&task.name, TREE_NAME_MAX)
    );
    match child_count {
        0 => {}
        1 => line.push_str(" (1 subtask)"),
        count => line.push_str(&format!(" ({count} subtasks)")),
    }
    line
}

fn push_related(out: &mut String, title: &str, related: &[&Task]) {
    if related.is_empty() {
        return;
    }
    out.push_str(&format!("{title}:\n"));
    for task in related {
        out.push_str(&format!("  • {}: {}\n", task.id, truncate(&task.name, 50)));
    }
    out.push('\n');
}

fn push_text(out: &mut String, text: &str, full: bool) -> bool {
    let truncated = !full && text.chars().count() > TEXT_MAX;
    let shown = if truncated {
        truncate(text, TEXT_MAX)
    } else {
        text.to_string()
    };
    if shown.is_empty() {
        out.push('\n');
    }
    for line in shown.lines() {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    truncated
}

fn text<'a>(value: &'a Value, key: &str) -> std::borrow::Cow<'a, str> {
    match value.get(key) {
        Some(Value::String(text)) => text.as_str().into(),
        Some(Value::Null) | None => "".into(),
        Some(other) => other.to_string().into(),
    }
}

fn push_timestamp(out: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        out.push_str(&format!("{label} {value}\n"));
    }
}
