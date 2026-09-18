use crate::listing::{find, task_line};
use crate::task::Task;

const TRUNCATE_AT: usize = 500;

pub fn render(tasks: &[Task], task: &Task, full: bool) -> String {
    let mut output = String::new();
    render_context_tree(tasks, task, &mut output);

    push_text_section(&mut output, "Description", &task.description, full);
    if let Some(result) = &task.result {
        push_text_section(&mut output, "Result", result, full);
    }
    push_related(&mut output, tasks, "Blocked by", &task.blocked_by);
    push_related(&mut output, tasks, "Blocks", &task.blocks);

    output.push('\n');
    push_timestamp(&mut output, "Created:  ", task.created_at.as_deref());
    push_timestamp(&mut output, "Updated:  ", task.updated_at.as_deref());
    push_timestamp(&mut output, "Started:  ", task.started_at.as_deref());
    push_timestamp(&mut output, "Completed:", task.completed_at.as_deref());
    output
}

fn render_context_tree(tasks: &[Task], task: &Task, output: &mut String) {
    let mut ancestors = Vec::new();
    let mut current = task.parent_id.as_deref().and_then(|id| find(tasks, id));
    while let Some(ancestor) = current {
        ancestors.push(ancestor);
        current = ancestor.parent_id.as_deref().and_then(|id| find(tasks, id));
    }
    ancestors.reverse();

    let mut indent = String::new();
    for (depth, ancestor) in ancestors.iter().enumerate() {
        if depth > 0 {
            output.push_str(&indent);
            output.push_str("└── ");
            indent.push_str("    ");
        }
        output.push_str(&task_line(tasks, ancestor));
        output.push('\n');
    }
    if !ancestors.is_empty() {
        output.push_str(&indent);
        output.push_str("└── ");
        indent.push_str("    ");
    }
    output.push_str(&task_line(tasks, task));
    match task.children.len() {
        0 => {}
        1 => output.push_str(" (1 subtask)"),
        count => output.push_str(&format!(" ({count} subtasks)")),
    }
    output.push_str("  ← viewing\n");

    let children: Vec<&Task> = task
        .children
        .iter()
        .filter_map(|id| find(tasks, id))
        .collect();
    let last = children.len().saturating_sub(1);
    for (index, child) in children.iter().enumerate() {
        output.push_str(&indent);
        output.push_str(if index == last {
            "└── "
        } else {
            "├── "
        });
        output.push_str(&task_line(tasks, child));
        output.push('\n');
    }
}

fn push_text_section(output: &mut String, title: &str, text: &str, full: bool) {
    output.push_str(&format!("\n{title}:\n"));
    let shown = if !full && text.chars().count() > TRUNCATE_AT {
        let cut: String = text.chars().take(TRUNCATE_AT).collect();
        format!("{cut}...\n  (truncated; use --full to see the complete text)")
    } else {
        text.to_string()
    };
    for line in shown.lines() {
        output.push_str("  ");
        output.push_str(line);
        output.push('\n');
    }
    if shown.is_empty() {
        output.push('\n');
    }
}

fn push_related(output: &mut String, tasks: &[Task], title: &str, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    output.push_str(&format!("\n{title}:\n"));
    for id in ids {
        let name = find(tasks, id)
            .map(|task| task.name.as_str())
            .unwrap_or("?");
        output.push_str(&format!("  • {id}: {name}\n"));
    }
}

fn push_timestamp(output: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        output.push_str(&format!("{label} {value}\n"));
    }
}
