use std::collections::HashSet;

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::listing::find;
use crate::relations::subtree_ids;
use crate::task::{Task, timestamp};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedTask {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub description: String,
    pub result: Option<String>,
    pub completed_at: Option<String>,
    pub archived_at: String,
    pub metadata: Option<serde_json::Value>,
    pub archived_children: Vec<ArchivedChild>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedChild {
    pub id: String,
    pub name: String,
    pub description: String,
    pub result: Option<String>,
}

pub fn parse_archive_jsonl(input: &str) -> anyhow::Result<Vec<ArchivedTask>> {
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

pub fn serialize_archive_jsonl(archived: &[ArchivedTask]) -> anyhow::Result<String> {
    let mut output = String::new();
    for record in archived {
        output.push_str(&serde_json::to_string(record)?);
        output.push('\n');
    }
    Ok(output)
}

pub fn check_archivable(tasks: &[Task], id: &str) -> anyhow::Result<()> {
    let task = find(tasks, id)
        .ok_or_else(|| anyhow!("task {id} not found\nHint: Run dex list --all to see all tasks"))?;
    if !task.completed {
        bail!(
            "task {id} is not completed\nHint: Complete the task with 'dex complete {id} --result \"...\"' first"
        );
    }
    let mut current = task.parent_id.as_deref();
    while let Some(parent_id) = current {
        let parent = find(tasks, parent_id)
            .ok_or_else(|| anyhow!("task {id} has missing parent {parent_id}"))?;
        if !parent.completed {
            bail!("task {id} has incomplete ancestor {parent_id}");
        }
        current = parent.parent_id.as_deref();
    }
    let incomplete: Vec<&str> = subtree_ids(tasks, id)
        .into_iter()
        .filter(|member| find(tasks, member).is_some_and(|task| !task.completed))
        .collect();
    if !incomplete.is_empty() {
        bail!(
            "task {id} has incomplete subtasks: {}",
            incomplete.join(", ")
        );
    }
    Ok(())
}

pub fn bulk_candidates<'a>(
    tasks: &'a [Task],
    completed_before: Option<&str>,
    except: &HashSet<String>,
) -> Vec<&'a Task> {
    let mut roots: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.completed)
        .filter(|task| !except.contains(&task.id))
        .filter(|task| {
            task.parent_id
                .as_deref()
                .and_then(|parent| find(tasks, parent))
                .is_none_or(|parent| !parent.completed)
        })
        .filter(|task| check_archivable(tasks, &task.id).is_ok())
        .filter(|task| {
            completed_before.is_none_or(|cutoff| {
                task.completed_at
                    .as_deref()
                    .is_some_and(|completed| completed < cutoff)
            })
        })
        .filter(|task| {
            subtree_ids(tasks, &task.id)
                .into_iter()
                .all(|member| !except.contains(member))
        })
        .collect();
    roots.sort_by(|left, right| left.id.cmp(&right.id));
    roots
}

pub fn archive_subtrees(tasks: &mut Vec<Task>, roots: &[String]) -> Vec<ArchivedTask> {
    let archived_at = timestamp();
    let ids: HashSet<String> = roots
        .iter()
        .flat_map(|root| {
            subtree_ids(tasks, root)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();

    let mut records: Vec<ArchivedTask> = tasks
        .iter()
        .filter(|task| ids.contains(&task.id))
        .map(|task| ArchivedTask {
            id: task.id.clone(),
            parent_id: task.parent_id.clone(),
            name: task.name.clone(),
            description: task.description.clone(),
            result: task.result.clone(),
            completed_at: task.completed_at.clone(),
            archived_at: archived_at.clone(),
            metadata: compact_metadata(task.metadata.as_ref()),
            archived_children: task
                .children
                .iter()
                .filter_map(|child| find(tasks, child))
                .map(|child| ArchivedChild {
                    id: child.id.clone(),
                    name: child.name.clone(),
                    description: child.description.clone(),
                    result: child.result.clone(),
                })
                .collect(),
        })
        .collect();
    records.sort_by(|left, right| left.id.cmp(&right.id));

    tasks.retain(|task| !ids.contains(&task.id));
    for task in tasks {
        task.children.retain(|child| !ids.contains(child));
        task.blocked_by.retain(|blocker| !ids.contains(blocker));
        task.blocks.retain(|blocked| !ids.contains(blocked));
    }
    records
}

fn compact_metadata(metadata: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    let metadata = metadata?;
    let mut kept = serde_json::Map::new();
    for key in ["github", "commit"] {
        if let Some(value) = metadata.get(key) {
            kept.insert(key.to_string(), value.clone());
        }
    }
    (!kept.is_empty()).then_some(serde_json::Value::Object(kept))
}

/// Archives completed root tasks older than `age_days` whose commit, if
/// any, is on the remote, keeping the `keep_recent` most recently completed
/// tasks. Returns the archived roots for logging.
pub fn auto_archive(
    tasks: &mut Vec<Task>,
    archived: &mut Vec<ArchivedTask>,
    config: &crate::config::ArchiveConfig,
    cwd: &std::path::Path,
) -> Vec<(String, String)> {
    if !config.auto {
        return Vec::new();
    }
    let now = time::OffsetDateTime::now_utc();
    let parse = |stamp: &str| {
        time::OffsetDateTime::parse(stamp, &time::format_description::well_known::Rfc3339).ok()
    };
    let mut completed: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.completed && task.completed_at.is_some())
        .collect();
    completed.sort_by(|left, right| right.completed_at.cmp(&left.completed_at));
    let recent: HashSet<String> = completed
        .iter()
        .take(config.keep_recent)
        .map(|task| task.id.clone())
        .collect();

    let roots: Vec<(String, String)> = tasks
        .iter()
        .filter(|task| task.parent_id.is_none() && task.completed)
        .filter(|task| !recent.contains(&task.id))
        .filter(|task| {
            task.completed_at
                .as_deref()
                .and_then(parse)
                .is_some_and(|completed_at| (now - completed_at).whole_days() >= config.age_days)
        })
        .filter(|task| check_archivable(tasks, &task.id).is_ok())
        .filter(|task| {
            task.metadata
                .as_ref()
                .and_then(|m| m["commit"]["sha"].as_str())
                .is_none_or(|sha| crate::sync::is_commit_on_remote(cwd, sha))
        })
        .map(|task| (task.id.clone(), task.name.clone()))
        .collect();
    if roots.is_empty() {
        return roots;
    }
    let ids: Vec<String> = roots.iter().map(|(id, _)| id.clone()).collect();
    archived.extend(archive_subtrees(tasks, &ids));
    roots
}

pub fn cutoff_for_duration(value: &str) -> anyhow::Result<String> {
    let invalid = || {
        anyhow!(
            "Invalid duration format: {value}\nHint: Expected format: 30d (days), 12w (weeks), 6m (months)"
        )
    };
    let (amount, unit) = value.split_at(value.len().saturating_sub(1));
    let amount: i64 = amount.parse().map_err(|_| invalid())?;
    let days = match unit {
        "d" => amount,
        "w" => amount * 7,
        "m" => amount * 30,
        _ => return Err(invalid()),
    };
    let cutoff = time::OffsetDateTime::now_utc() - time::Duration::days(days);
    Ok(cutoff.format(&time::format_description::well_known::Rfc3339)?)
}
