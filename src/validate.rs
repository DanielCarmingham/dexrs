use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, bail};

use crate::task::Task;

pub fn validate_tasks(tasks: &[Task]) -> anyhow::Result<()> {
    let by_id = task_map(tasks)?;
    validate_parent_children(tasks, &by_id)?;
    validate_blockers(tasks, &by_id)?;
    validate_no_blocking_cycles(tasks, &by_id)?;
    Ok(())
}

pub fn validate_completion(tasks: &[Task], id: &str, force: bool) -> anyhow::Result<()> {
    if force {
        return Ok(());
    }

    let by_id = task_map(tasks)?;
    let task = by_id
        .get(id)
        .copied()
        .ok_or_else(|| anyhow!("task {id} not found"))?;

    for child_id in &task.children {
        let child = by_id
            .get(child_id.as_str())
            .copied()
            .ok_or_else(|| anyhow!("task {id} references missing child {child_id}"))?;
        if !child.completed {
            bail!("cannot complete {id}: incomplete child {child_id}");
        }
    }

    Ok(())
}

fn task_map<'a>(tasks: &'a [Task]) -> anyhow::Result<HashMap<&'a str, &'a Task>> {
    let mut by_id = HashMap::new();
    for task in tasks {
        if by_id.insert(task.id.as_str(), task).is_some() {
            bail!("duplicate task id {}", task.id);
        }
    }
    Ok(by_id)
}

fn validate_parent_children(tasks: &[Task], by_id: &HashMap<&str, &Task>) -> anyhow::Result<()> {
    for task in tasks {
        if let Some(parent_id) = &task.parent_id {
            let parent = by_id
                .get(parent_id.as_str())
                .copied()
                .ok_or_else(|| anyhow!("task {} has missing parent {parent_id}", task.id))?;
            if !parent.children.contains(&task.id) {
                bail!("parent {parent_id} children missing child {}", task.id);
            }
        }

        for child_id in &task.children {
            let child = by_id
                .get(child_id.as_str())
                .copied()
                .ok_or_else(|| anyhow!("task {} references missing child {child_id}", task.id))?;
            if child.parent_id.as_deref() != Some(task.id.as_str()) {
                bail!("child {child_id} parent_id does not reference {}", task.id);
            }
        }
    }

    Ok(())
}

fn validate_blockers(tasks: &[Task], by_id: &HashMap<&str, &Task>) -> anyhow::Result<()> {
    for task in tasks {
        for blocker_id in &task.blocked_by {
            let blocker = by_id
                .get(blocker_id.as_str())
                .copied()
                .ok_or_else(|| anyhow!("task {} has missing blocker {blocker_id}", task.id))?;
            if !blocker.blocks.contains(&task.id) {
                bail!("task {blocker_id} blocks missing {}", task.id);
            }
        }

        for blocked_id in &task.blocks {
            let blocked = by_id
                .get(blocked_id.as_str())
                .copied()
                .ok_or_else(|| anyhow!("task {} blocks missing task {blocked_id}", task.id))?;
            if !blocked.blocked_by.contains(&task.id) {
                bail!("task {blocked_id} blockedBy missing {}", task.id);
            }
        }
    }

    Ok(())
}

fn validate_no_blocking_cycles(tasks: &[Task], by_id: &HashMap<&str, &Task>) -> anyhow::Result<()> {
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();

    for task in tasks {
        visit_blockers(task.id.as_str(), by_id, &mut visiting, &mut visited)?;
    }

    Ok(())
}

fn visit_blockers<'a>(
    id: &'a str,
    by_id: &HashMap<&'a str, &'a Task>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> anyhow::Result<()> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id) {
        bail!("blocking cycle involving {id}");
    }

    let task = by_id
        .get(id)
        .copied()
        .ok_or_else(|| anyhow!("task {id} not found"))?;
    for blocker_id in &task.blocked_by {
        visit_blockers(blocker_id, by_id, visiting, visited)?;
    }

    visiting.remove(id);
    visited.insert(id);
    Ok(())
}
