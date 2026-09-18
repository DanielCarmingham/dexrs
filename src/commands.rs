use std::ffi::OsString;
use std::io::Write;

use clap::Parser;
use serde_json::json;

use crate::cli::{Cli, Command};
use crate::git;
use crate::store;
use crate::task::{Task, generate_id, timestamp};
use crate::validate::validate_completion;

pub fn run<I, W, E>(args: I, mut stdout: W, _stderr: E) -> anyhow::Result<i32>
where
    I: IntoIterator<Item = OsString>,
    W: Write,
    E: Write,
{
    let cli = Cli::parse_from(args);

    match cli.command {
        Command::Dir => {
            let cwd = std::env::current_dir()?;
            let store =
                store::resolve_store_dir(&cwd, std::env::var_os("DEX_STORAGE_PATH").as_deref())?;
            writeln!(stdout, "{}", store.display())?;
            Ok(0)
        }
        Command::Init => {
            let cwd = std::env::current_dir()?;
            let store =
                store::resolve_store_dir(&cwd, std::env::var_os("DEX_STORAGE_PATH").as_deref())?;
            store::init_store(&store)?;
            writeln!(stdout, "initialized {}", store.display())?;
            Ok(0)
        }
        Command::Create {
            name,
            description,
            priority,
        } => {
            let store = resolved_store()?;
            let task = store::transact(&store, |tasks| {
                let id = generate_id(|candidate| tasks.iter().any(|task| task.id == candidate));
                let task = Task::new(id, name, description, priority);
                tasks.push(task.clone());
                Ok(task)
            })?;
            writeln!(stdout, "created {}", task.id)?;
            Ok(0)
        }
        Command::Start { id } => {
            let store = resolved_store()?;
            store::transact(&store, |tasks| {
                let now = timestamp();
                let task = find_task_mut(tasks, &id)?;
                task.started_at = Some(now.clone());
                task.updated_at = Some(now);
                task.completed = false;
                Ok(())
            })?;
            writeln!(stdout, "started {id}")?;
            Ok(0)
        }
        Command::Complete {
            id,
            result,
            commit,
            no_commit: _,
            force,
        } => {
            let Some(result) = result else {
                anyhow::bail!(
                    "--result (-r) is required\nUsage: dexrs complete <task-id> --result \"completion notes\""
                );
            };
            let commit = commit
                .map(|reference| git::commit_metadata(&std::env::current_dir()?, &reference))
                .transpose()?;
            let store = resolved_store()?;
            store::transact(&store, |tasks| {
                validate_completion(tasks, &id, force)?;
                let now = timestamp();
                let task = find_task_mut(tasks, &id)?;
                task.completed = true;
                task.result = Some(result);
                if let Some(commit) = commit {
                    set_metadata(task, "commit", commit);
                }
                task.started_at.get_or_insert_with(|| now.clone());
                task.completed_at = Some(now.clone());
                task.updated_at = Some(now);
                Ok(())
            })?;
            writeln!(stdout, "completed {id}")?;
            Ok(0)
        }
        Command::Edit {
            id,
            name,
            description,
            priority,
        } => {
            let store = resolved_store()?;
            store::transact(&store, |tasks| {
                let task = find_task_mut(tasks, &id)?;
                if let Some(name) = name {
                    task.name = name;
                }
                if let Some(description) = description {
                    task.description = description;
                }
                if let Some(priority) = priority {
                    task.priority = priority;
                }
                task.updated_at = Some(timestamp());
                Ok(())
            })?;
            writeln!(stdout, "updated {id}")?;
            Ok(0)
        }
        Command::Delete { id } => {
            let store = resolved_store()?;
            store::transact(&store, |tasks| {
                let index = tasks
                    .iter()
                    .position(|task| task.id == id)
                    .ok_or_else(|| anyhow::anyhow!("task {id} not found"))?;
                tasks.remove(index);
                for task in tasks {
                    if task.parent_id.as_deref() == Some(id.as_str()) {
                        task.parent_id = None;
                    }
                    task.children.retain(|child| child != &id);
                    task.blocked_by.retain(|blocker| blocker != &id);
                    task.blocks.retain(|blocked| blocked != &id);
                }
                Ok(())
            })?;
            writeln!(stdout, "deleted {id}")?;
            Ok(0)
        }
        Command::Status { json } => {
            let store = resolved_store()?;
            let tasks = store::read_tasks(&store)?;
            let counts = status_counts(&tasks);
            if json {
                writeln!(
                    stdout,
                    "{}",
                    serde_json::to_string(&json!({
                        "todo": counts.todo,
                        "in_progress": counts.in_progress,
                        "done": counts.done,
                    }))?
                )?;
            } else {
                writeln!(
                    stdout,
                    "{} todo, {} in progress, {} done",
                    counts.todo, counts.in_progress, counts.done
                )?;
            }
            Ok(0)
        }
        Command::List { json } => {
            let store = resolved_store()?;
            let mut tasks = store::read_tasks(&store)?;
            tasks.sort_by(|left, right| left.id.cmp(&right.id));
            if json {
                writeln!(stdout, "{}", serde_json::to_string(&tasks)?)?;
            } else {
                for task in tasks {
                    writeln!(stdout, "{} {} {}", status_icon(&task), task.id, task.name)?;
                }
            }
            Ok(0)
        }
        Command::Show { id, json } => {
            let store = resolved_store()?;
            let tasks = store::read_tasks(&store)?;
            let task = tasks
                .iter()
                .find(|task| task.id == id)
                .ok_or_else(|| anyhow::anyhow!("task {id} not found"))?;
            if json {
                writeln!(stdout, "{}", serde_json::to_string(task)?)?;
            } else {
                writeln!(stdout, "{} {} {}", status_icon(task), task.id, task.name)?;
                if !task.description.is_empty() {
                    writeln!(stdout, "{}", task.description)?;
                }
                writeln!(stdout, "priority: {}", task.priority)?;
            }
            Ok(0)
        }
    }
}

struct StatusCounts {
    todo: usize,
    in_progress: usize,
    done: usize,
}

fn status_counts(tasks: &[Task]) -> StatusCounts {
    let mut counts = StatusCounts {
        todo: 0,
        in_progress: 0,
        done: 0,
    };

    for task in tasks {
        if task.completed {
            counts.done += 1;
        } else if task.started_at.is_some() {
            counts.in_progress += 1;
        } else {
            counts.todo += 1;
        }
    }

    counts
}

fn status_icon(task: &Task) -> &'static str {
    if task.completed {
        "[x]"
    } else if task.started_at.is_some() {
        "[>]"
    } else {
        "[ ]"
    }
}

fn resolved_store() -> anyhow::Result<std::path::PathBuf> {
    let cwd = std::env::current_dir()?;
    store::resolve_store_dir(&cwd, std::env::var_os("DEX_STORAGE_PATH").as_deref())
}

fn set_metadata(task: &mut Task, key: &str, value: serde_json::Value) {
    match task.metadata.as_mut() {
        Some(serde_json::Value::Object(metadata)) => {
            metadata.insert(key.to_string(), value);
        }
        _ => task.metadata = Some(json!({ key: value })),
    }
}

fn find_task_mut<'a>(tasks: &'a mut [Task], id: &str) -> anyhow::Result<&'a mut Task> {
    tasks
        .iter_mut()
        .find(|task| task.id == id)
        .ok_or_else(|| anyhow::anyhow!("task {id} not found"))
}
