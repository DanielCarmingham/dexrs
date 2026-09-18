use std::collections::HashSet;

use crate::task::{DEFAULT_PRIORITY, Task};

#[derive(Debug, Default)]
pub struct ListFilter {
    pub all: bool,
    pub completed: bool,
    pub in_progress: bool,
    pub blocked: bool,
    pub ready: bool,
    pub query: Option<String>,
}

pub fn select<'a>(tasks: &'a [Task], filter: &ListFilter) -> Vec<&'a Task> {
    let subtree: Option<HashSet<&str>> = filter
        .query
        .as_deref()
        .filter(|query| tasks.iter().any(|task| task.id == *query))
        .map(|root| crate::relations::subtree_ids(tasks, root));
    let search = filter
        .query
        .as_deref()
        .filter(|_| subtree.is_none())
        .map(str::to_lowercase);

    let mut selected: Vec<&Task> = tasks
        .iter()
        .filter(|task| {
            if filter.completed {
                task.completed
            } else {
                filter.all || !task.completed
            }
        })
        .filter(|task| !filter.in_progress || is_in_progress(task))
        .filter(|task| !filter.blocked || is_blocked(tasks, task))
        .filter(|task| !filter.ready || is_ready(tasks, task))
        .filter(|task| {
            subtree
                .as_ref()
                .is_none_or(|ids| ids.contains(task.id.as_str()))
        })
        .filter(|task| {
            search.as_deref().is_none_or(|needle| {
                task.name.to_lowercase().contains(needle)
                    || task.description.to_lowercase().contains(needle)
            })
        })
        .collect();
    selected.sort_by(sort_key);
    selected
}

pub fn render_flat(tasks: &[Task], selected: &[&Task]) -> String {
    selected
        .iter()
        .map(|task| format!("{}\n", task_line(tasks, task)))
        .collect()
}

pub fn render_tree(tasks: &[Task], selected: &[&Task]) -> String {
    let mut included: HashSet<&str> = selected.iter().map(|task| task.id.as_str()).collect();
    for task in selected {
        let mut current = task.parent_id.as_deref();
        while let Some(parent_id) = current {
            if !included.insert(parent_id) {
                break;
            }
            current = find(tasks, parent_id).and_then(|parent| parent.parent_id.as_deref());
        }
    }

    let mut roots: Vec<&Task> = tasks
        .iter()
        .filter(|task| included.contains(task.id.as_str()))
        .filter(|task| {
            task.parent_id
                .as_deref()
                .is_none_or(|parent| !included.contains(parent))
        })
        .collect();
    roots.sort_by(sort_key);

    let mut output = String::new();
    for root in roots {
        output.push_str(&task_line(tasks, root));
        output.push('\n');
        render_children(tasks, root, &included, "", &mut output);
    }
    output
}

pub fn task_line(tasks: &[Task], task: &Task) -> String {
    let mut line = format!("{} {}", status_icon(task), task.id);
    if task.priority != DEFAULT_PRIORITY {
        line.push_str(&format!(" [p{}]", task.priority));
    }
    let open_blockers: Vec<&str> = task
        .blocked_by
        .iter()
        .filter(|id| find(tasks, id).is_some_and(|blocker| !blocker.completed))
        .map(String::as_str)
        .collect();
    match open_blockers.as_slice() {
        [] => {}
        [only] => line.push_str(&format!(" [B: {only}]")),
        many => line.push_str(&format!(" [B: {}]", many.len())),
    }
    line.push_str(&format!(": {}", task.name));
    line
}

pub fn status_icon(task: &Task) -> &'static str {
    if task.completed {
        "[x]"
    } else if task.started_at.is_some() {
        "[>]"
    } else {
        "[ ]"
    }
}

pub fn is_in_progress(task: &Task) -> bool {
    task.started_at.is_some() && !task.completed
}

pub fn is_blocked(tasks: &[Task], task: &Task) -> bool {
    task.blocked_by
        .iter()
        .any(|id| find(tasks, id).is_some_and(|blocker| !blocker.completed))
}

pub fn is_ready(tasks: &[Task], task: &Task) -> bool {
    !task.completed && task.started_at.is_none() && !is_blocked(tasks, task)
}

pub fn find<'a>(tasks: &'a [Task], id: &str) -> Option<&'a Task> {
    tasks.iter().find(|task| task.id == id)
}

fn render_children(
    tasks: &[Task],
    parent: &Task,
    included: &HashSet<&str>,
    prefix: &str,
    output: &mut String,
) {
    let mut children: Vec<&Task> = parent
        .children
        .iter()
        .filter(|id| included.contains(id.as_str()))
        .filter_map(|id| find(tasks, id))
        .collect();
    children.sort_by(sort_key);

    let last = children.len().saturating_sub(1);
    for (index, child) in children.iter().enumerate() {
        let (branch, extension) = if index == last {
            ("└── ", "    ")
        } else {
            ("├── ", "│   ")
        };
        output.push_str(prefix);
        output.push_str(branch);
        output.push_str(&task_line(tasks, child));
        output.push('\n');
        render_children(
            tasks,
            child,
            included,
            &format!("{prefix}{extension}"),
            output,
        );
    }
}

fn sort_key(left: &&Task, right: &&Task) -> std::cmp::Ordering {
    left.priority
        .cmp(&right.priority)
        .then_with(|| left.id.cmp(&right.id))
}
