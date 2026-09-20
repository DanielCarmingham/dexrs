//! Task mutations with the original TaskService's validation rules and
//! messages, shared by import, the MCP server, and sync metadata updates.

use anyhow::{anyhow, bail};
use serde_json::Value;

use crate::relations::{self, Placement};
use crate::task::{Task, generate_id, timestamp};

#[derive(Debug, Default, Clone)]
pub struct CreateInput {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub parent_id: Option<String>,
    pub priority: Option<i64>,
    pub completed: Option<bool>,
    pub result: Option<String>,
    pub metadata: Option<Value>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub blocked_by: Vec<String>,
}

/// `Option<Option<_>>` fields distinguish "leave alone" from "set to null".
#[derive(Debug, Default, Clone)]
pub struct UpdateInput {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub parent_id: Option<Option<String>>,
    pub priority: Option<i64>,
    pub completed: Option<bool>,
    pub completed_at: Option<Option<String>>,
    pub result: Option<Option<String>>,
    pub started_at: Option<Option<String>>,
    pub updated_at: Option<String>,
    pub metadata: Option<Option<Value>>,
    pub add_blocked_by: Vec<String>,
    pub remove_blocked_by: Vec<String>,
}

pub fn create(tasks: &mut Vec<Task>, input: CreateInput) -> anyhow::Result<Task> {
    let id = match input.id {
        Some(id) => {
            if tasks.iter().any(|task| task.id == id) {
                bail!(
                    "Task with ID '{id}' already exists\nHint: Use a different ID or omit to auto-generate"
                );
            }
            id
        }
        None => generate_id(|candidate| tasks.iter().any(|task| task.id == candidate)),
    };
    for blocker in &input.blocked_by {
        if !tasks.iter().any(|task| &task.id == blocker) {
            bail!("Task \"{blocker}\" not found\nHint: The specified blocker task does not exist");
        }
    }
    let now = timestamp();
    let mut task = Task::new(id.clone(), input.name, input.description, input.priority);
    task.completed = input.completed.unwrap_or(false);
    task.result = input.result;
    task.metadata = input.metadata;
    task.created_at = Some(input.created_at.unwrap_or_else(|| now.clone()));
    task.updated_at = Some(input.updated_at.unwrap_or(now));
    task.started_at = input.started_at;
    task.completed_at = input.completed_at;
    tasks.push(task);
    if let Some(parent_id) = input.parent_id.as_deref() {
        relations::set_parent(tasks, &id, Some(parent_id), Placement::Create).inspect_err(
            |_| {
                tasks.retain(|task| task.id != id);
            },
        )?;
    }
    for blocker in &input.blocked_by {
        relations::add_blocker(tasks, &id, blocker)?;
    }
    tasks
        .iter()
        .find(|task| task.id == id)
        .cloned()
        .ok_or_else(|| anyhow!("task {id} vanished after creation"))
}

pub fn update(tasks: &mut [Task], input: UpdateInput) -> anyhow::Result<Task> {
    let id = input.id.clone();
    if !tasks.iter().any(|task| task.id == id) {
        bail!("Task \"{id}\" not found\nHint: Run \"dex list --all\" to see all available tasks");
    }
    if let Some(parent) = &input.parent_id {
        if parent.as_deref() == Some(id.as_str()) {
            bail!("Task cannot be its own parent\nHint: Choose a different task as the parent");
        }
        relations::set_parent(tasks, &id, parent.as_deref(), Placement::Move)?;
    }
    for blocker in &input.add_blocked_by {
        if blocker == &id {
            bail!(
                "Task cannot block itself\nHint: Remove the task's own ID from the add_blocked_by list"
            );
        }
        if !tasks.iter().any(|task| &task.id == blocker) {
            bail!("Task \"{blocker}\" not found\nHint: The specified blocker task does not exist");
        }
        if would_create_blocking_cycle(tasks, blocker, &id) {
            bail!(
                "Cannot add blocker {blocker}: would create a cycle\nHint: The specified task is already blocked by this task (directly or indirectly)"
            );
        }
        relations::add_blocker(tasks, &id, blocker)?;
    }
    for blocker in &input.remove_blocked_by {
        relations::remove_blocker(tasks, &id, blocker)?;
    }

    let now = timestamp();
    let task = tasks
        .iter_mut()
        .find(|task| task.id == id)
        .ok_or_else(|| anyhow!("task {id} not found"))?;
    if let Some(name) = input.name {
        task.name = name;
    }
    if let Some(description) = input.description {
        task.description = description;
    }
    if let Some(priority) = input.priority {
        task.priority = priority;
    }
    if let Some(completed) = input.completed {
        if completed && !task.completed {
            task.completed_at = Some(now.clone());
        } else if !completed && task.completed {
            task.completed_at = None;
        }
        task.completed = completed;
    }
    if let Some(completed_at) = input.completed_at {
        task.completed_at = completed_at;
    }
    if let Some(result) = input.result {
        task.result = result;
    }
    if let Some(started_at) = input.started_at {
        task.started_at = started_at;
    }
    if let Some(metadata) = input.metadata {
        task.metadata = metadata;
    }
    task.updated_at = Some(input.updated_at.unwrap_or(now));
    Ok(task.clone())
}

/// Removes a task and its descendants, returning the removed tasks.
pub fn delete(tasks: &mut Vec<Task>, id: &str) -> anyhow::Result<Vec<Task>> {
    if !tasks.iter().any(|task| task.id == id) {
        bail!("Task \"{id}\" not found\nHint: Run \"dex list --all\" to see all available tasks");
    }
    let ids: std::collections::HashSet<String> = relations::subtree_ids(tasks, id)
        .into_iter()
        .map(str::to_string)
        .collect();
    let removed: Vec<Task> = tasks
        .iter()
        .filter(|task| ids.contains(&task.id))
        .cloned()
        .collect();
    tasks.retain(|task| !ids.contains(&task.id));
    for task in tasks.iter_mut() {
        task.children.retain(|child| !ids.contains(child));
        task.blocked_by.retain(|blocker| !ids.contains(blocker));
        task.blocks.retain(|blocked| !ids.contains(blocked));
    }
    Ok(removed)
}

fn would_create_blocking_cycle(tasks: &[Task], blocker_id: &str, blocked_id: &str) -> bool {
    let mut visited = std::collections::HashSet::new();
    let mut stack = vec![blocker_id.to_string()];
    while let Some(current) = stack.pop() {
        if current == blocked_id {
            return true;
        }
        if !visited.insert(current.clone()) {
            continue;
        }
        if let Some(task) = tasks.iter().find(|task| task.id == current) {
            stack.extend(task.blocked_by.iter().cloned());
        }
    }
    false
}
