use std::ffi::OsString;
use std::io::Write;

use anyhow::Context;
use clap::Parser;
use serde_json::json;

use crate::cli::{Cli, Command};
use crate::git;
use crate::listing::{self, ListFilter};
use crate::relations;
use crate::show;
use crate::status;
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
    let command = cli.command.unwrap_or(Command::Status { json: false });

    match command {
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
            name_flag,
            description,
            priority,
            parent,
            blocked_by,
        } => {
            let Some(name) = name.or(name_flag) else {
                anyhow::bail!("task name is required\nUsage: dexrs create \"name\" [options]");
            };
            let store = resolved_store()?;
            let task = store::transact(&store, |tasks| {
                let id = generate_id(|candidate| tasks.iter().any(|task| task.id == candidate));
                let task = Task::new(id.clone(), name, description, priority);
                tasks.push(task.clone());
                relations::set_parent(tasks, &id, parent.as_deref())?;
                for blocker in blocked_by
                    .iter()
                    .flat_map(|value| relations::split_ids(value))
                {
                    relations::add_blocker(tasks, &id, blocker)?;
                }
                Ok(task)
            })?;
            writeln!(stdout, "created {}", task.id)?;
            Ok(0)
        }
        Command::Plan {
            file,
            priority,
            parent,
        } => {
            let contents = std::fs::read_to_string(&file)
                .with_context(|| format!("failed to read plan file {}", file.display()))?;
            let name = plan_name(&file, &contents);
            let store = resolved_store()?;
            let (task, line) = store::transact(&store, |tasks| {
                let id = generate_id(|candidate| tasks.iter().any(|task| task.id == candidate));
                let task = Task::new(id.clone(), name, Some(contents), priority);
                tasks.push(task.clone());
                relations::set_parent(tasks, &id, parent.as_deref())?;
                Ok((task, listing::task_line(tasks, &tasks[tasks.len() - 1])))
            })?;
            writeln!(stdout, "Created task {} from plan\n{line}", task.id)?;
            Ok(0)
        }
        Command::Start { id, force } => {
            let store = resolved_store()?;
            store::transact(&store, |tasks| {
                let now = timestamp();
                let task = find_task_mut(tasks, &id)?;
                if listing::is_in_progress(task) && !force {
                    anyhow::bail!(
                        "task {id} is already in progress and may be being worked on by someone else\n\
                         Hint: Use --force to re-claim the task"
                    );
                }
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
            parent,
            add_blocker,
            remove_blocker,
            commit,
        } => {
            let commit = commit
                .map(|reference| git::commit_metadata(&std::env::current_dir()?, &reference))
                .transpose()?;
            let store = resolved_store()?;
            store::transact(&store, |tasks| {
                if let Some(parent) = &parent {
                    relations::set_parent(tasks, &id, Some(parent))?;
                }
                for blocker in add_blocker
                    .iter()
                    .flat_map(|value| relations::split_ids(value))
                {
                    relations::add_blocker(tasks, &id, blocker)?;
                }
                for blocker in remove_blocker
                    .iter()
                    .flat_map(|value| relations::split_ids(value))
                {
                    relations::remove_blocker(tasks, &id, blocker)?;
                }
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
                if let Some(commit) = commit {
                    set_metadata(task, "commit", commit);
                }
                task.updated_at = Some(timestamp());
                Ok(())
            })?;
            writeln!(stdout, "updated {id}")?;
            Ok(0)
        }
        Command::Delete { id, force } => {
            let store = resolved_store()?;
            let removed = store::transact(&store, |tasks| {
                find_task_mut(tasks, &id)?;
                let subtree: std::collections::HashSet<String> = relations::subtree_ids(tasks, &id)
                    .into_iter()
                    .map(str::to_string)
                    .collect();
                let subtasks = subtree.len() - 1;
                if subtasks > 0 && !force {
                    anyhow::bail!(
                        "task {id} has {subtasks} subtasks that would also be deleted\n\
                         Hint: Use --force to delete the task and its subtasks"
                    );
                }
                tasks.retain(|task| !subtree.contains(&task.id));
                for task in tasks {
                    task.children.retain(|child| !subtree.contains(child));
                    task.blocked_by.retain(|blocker| !subtree.contains(blocker));
                    task.blocks.retain(|blocked| !subtree.contains(blocked));
                }
                Ok(subtasks)
            })?;
            match removed {
                0 => writeln!(stdout, "Deleted task {id}")?,
                1 => writeln!(stdout, "Deleted task {id} and 1 subtask")?,
                count => writeln!(stdout, "Deleted task {id} and {count} subtasks")?,
            }
            Ok(0)
        }
        Command::Status { json } => {
            let store = resolved_store()?;
            let tasks = store::read_tasks(&store)?;
            let dashboard = status::dashboard(&tasks);
            if json {
                writeln!(
                    stdout,
                    "{}",
                    serde_json::to_string(&status::to_json(&dashboard))?
                )?;
            } else {
                write!(stdout, "{}", status::render(&tasks, &dashboard))?;
            }
            Ok(0)
        }
        Command::List {
            filter,
            all,
            completed,
            in_progress,
            blocked,
            ready,
            flat,
            query,
            json,
        } => {
            let store = resolved_store()?;
            let tasks = store::read_tasks(&store)?;
            let filter = ListFilter {
                all,
                completed,
                in_progress,
                blocked,
                ready,
                query: filter.or(query),
            };
            let selected = listing::select(&tasks, &filter);
            if json {
                writeln!(stdout, "{}", serde_json::to_string(&selected)?)?;
            } else if selected.is_empty() {
                writeln!(stdout, "No tasks found.")?;
            } else if flat {
                write!(stdout, "{}", listing::render_flat(&tasks, &selected))?;
            } else {
                write!(stdout, "{}", listing::render_tree(&tasks, &selected))?;
            }
            Ok(0)
        }
        Command::Show {
            ids,
            full,
            expand,
            json,
        } => {
            let store = resolved_store()?;
            let tasks = store::read_tasks(&store)?;
            let selected = ids
                .iter()
                .map(|id| {
                    listing::find(&tasks, id).ok_or_else(|| anyhow::anyhow!("task {id} not found"))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            if json {
                match selected.as_slice() {
                    [task] => writeln!(stdout, "{}", serde_json::to_string(task)?)?,
                    many => writeln!(stdout, "{}", serde_json::to_string(many)?)?,
                }
            } else {
                for (index, task) in selected.iter().enumerate() {
                    if index > 0 {
                        writeln!(stdout)?;
                    }
                    write!(stdout, "{}", show::render(&tasks, task, full || expand))?;
                }
            }
            Ok(0)
        }
    }
}

fn resolved_store() -> anyhow::Result<std::path::PathBuf> {
    let cwd = std::env::current_dir()?;
    store::resolve_store_dir(&cwd, std::env::var_os("DEX_STORAGE_PATH").as_deref())
}

fn plan_name(file: &std::path::Path, contents: &str) -> String {
    contents
        .lines()
        .map(str::trim)
        .find_map(|line| {
            let heading = line.trim_start_matches('#');
            (heading.len() < line.len() && heading.starts_with(' ')).then(|| heading.trim())
        })
        .filter(|heading| !heading.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            file.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "plan".to_string())
        })
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
