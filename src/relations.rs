use std::collections::HashSet;

use anyhow::{anyhow, bail};

use crate::task::Task;

pub fn set_parent(tasks: &mut [Task], id: &str, parent_id: Option<&str>) -> anyhow::Result<()> {
    if let Some(parent_id) = parent_id {
        if parent_id == id {
            bail!("task {id} cannot be its own parent");
        }
        require(tasks, parent_id).map_err(|_| {
            anyhow!("task {parent_id} not found\nHint: The specified parent task does not exist")
        })?;
        if is_descendant(tasks, parent_id, id) {
            bail!("task {parent_id} is a subtask of {id} and cannot become its parent");
        }
    }

    let previous = require(tasks, id)?.parent_id.clone();
    if let Some(previous) = previous {
        require_mut(tasks, &previous)?
            .children
            .retain(|child| child != id);
    }
    if let Some(parent_id) = parent_id {
        let parent = require_mut(tasks, parent_id)?;
        if !parent.children.iter().any(|child| child == id) {
            parent.children.push(id.to_string());
        }
    }
    require_mut(tasks, id)?.parent_id = parent_id.map(str::to_string);
    Ok(())
}

pub fn add_blocker(tasks: &mut [Task], id: &str, blocker_id: &str) -> anyhow::Result<()> {
    if blocker_id == id {
        bail!("task {id} cannot block itself");
    }
    require(tasks, blocker_id).map_err(|_| {
        anyhow!("task {blocker_id} not found\nHint: The specified blocker task does not exist")
    })?;
    let task = require_mut(tasks, id)?;
    if !task
        .blocked_by
        .iter()
        .any(|existing| existing == blocker_id)
    {
        task.blocked_by.push(blocker_id.to_string());
    }
    let blocker = require_mut(tasks, blocker_id)?;
    if !blocker.blocks.iter().any(|existing| existing == id) {
        blocker.blocks.push(id.to_string());
    }
    Ok(())
}

pub fn remove_blocker(tasks: &mut [Task], id: &str, blocker_id: &str) -> anyhow::Result<()> {
    require_mut(tasks, id)?
        .blocked_by
        .retain(|existing| existing != blocker_id);
    if let Some(blocker) = tasks.iter_mut().find(|task| task.id == blocker_id) {
        blocker.blocks.retain(|existing| existing != id);
    }
    Ok(())
}

pub fn subtree_ids<'a>(tasks: &'a [Task], root: &str) -> HashSet<&'a str> {
    let mut ids = HashSet::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if let Some(task) = tasks.iter().find(|task| task.id == id) {
            ids.insert(task.id.as_str());
            pending.extend(task.children.iter().map(String::as_str));
        }
    }
    ids
}

pub fn split_ids(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|id| !id.is_empty())
}

fn is_descendant(tasks: &[Task], candidate: &str, ancestor: &str) -> bool {
    let mut current = candidate;
    while let Some(task) = tasks.iter().find(|task| task.id == current) {
        match task.parent_id.as_deref() {
            Some(parent) if parent == ancestor => return true,
            Some(parent) => current = parent,
            None => return false,
        }
    }
    false
}

fn require<'a>(tasks: &'a [Task], id: &str) -> anyhow::Result<&'a Task> {
    tasks
        .iter()
        .find(|task| task.id == id)
        .ok_or_else(|| anyhow!("task {id} not found"))
}

fn require_mut<'a>(tasks: &'a mut [Task], id: &str) -> anyhow::Result<&'a mut Task> {
    tasks
        .iter_mut()
        .find(|task| task.id == id)
        .ok_or_else(|| anyhow!("task {id} not found"))
}
