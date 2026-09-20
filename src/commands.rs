use std::ffi::OsString;
use std::io::Write;

use anyhow::Context;
use clap::Parser;
use serde_json::json;

use std::collections::HashSet;

use crate::archive;
use crate::cli::{Cli, Command};
use crate::git;
use crate::listing::{self, ListFilter};
use crate::relations;
use crate::show;
use crate::status;
use crate::store;
use crate::task::{Task, generate_id, serialize_tasks_jsonl, timestamp};
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
        Command::Archive {
            id,
            completed,
            older_than,
            except,
            dry_run,
        } => {
            if id.is_none() && !completed && older_than.is_none() {
                anyhow::bail!(
                    "specify a task id, --completed, or --older-than <duration>\nUsage: dexrs archive <task-id> | --completed | --older-than 30d"
                );
            }
            let cutoff = older_than
                .as_deref()
                .map(archive::cutoff_for_duration)
                .transpose()?;
            let except: HashSet<String> = except
                .iter()
                .flat_map(|value| relations::split_ids(value))
                .map(str::to_string)
                .collect();
            let store = resolved_store()?;
            let outcome = store::transact_with_archive(&store, |tasks, archived| {
                let roots: Vec<String> = match &id {
                    Some(id) => {
                        archive::check_archivable(tasks, id)?;
                        vec![id.clone()]
                    }
                    None => archive::bulk_candidates(tasks, cutoff.as_deref(), &except)
                        .into_iter()
                        .map(|task| task.id.clone())
                        .collect(),
                };
                if roots.is_empty() {
                    return Ok(None);
                }
                let before = serialize_tasks_jsonl(tasks)?.len();
                let mut preview = tasks.clone();
                let records = archive::archive_subtrees(&mut preview, &roots);
                let after = serialize_tasks_jsonl(&preview)?.len();
                if !dry_run {
                    *tasks = preview;
                    archived.extend(records.iter().cloned());
                }
                Ok(Some(ArchiveOutcome {
                    archived: records.len(),
                    roots: roots.len(),
                    reduction_percent: (before - after) * 100 / before.max(1),
                }))
            })?;
            let Some(outcome) = outcome else {
                writeln!(stdout, "No tasks found to archive.")?;
                return Ok(0);
            };
            let verb = if dry_run { "Would archive" } else { "Archived" };
            let mut line = format!("{verb} {}", plural(outcome.archived, "task"));
            if id.is_none() {
                line.push_str(&format!(" ({})", plural(outcome.roots, "root task")));
            }
            writeln!(stdout, "{line}")?;
            let subtasks = outcome.archived - outcome.roots;
            if subtasks > 0 {
                writeln!(stdout, "  Subtasks: {subtasks}")?;
            }
            writeln!(stdout, "  Size reduction: {}%", outcome.reduction_percent)?;
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
            archived,
            issue,
            commit,
            flat,
            query,
            json,
        } => {
            let store = resolved_store()?;
            if archived {
                let mut records = store::read_archive(&store)?;
                records.reverse();
                if json {
                    writeln!(stdout, "{}", serde_json::to_string(&records)?)?;
                } else if records.is_empty() {
                    writeln!(stdout, "No archived tasks found.")?;
                } else {
                    writeln!(
                        stdout,
                        "Showing {}\n",
                        plural(records.len(), "archived task")
                    )?;
                    for record in &records {
                        let line = show::render_archived(record);
                        writeln!(stdout, "{}", line.lines().next().unwrap_or_default())?;
                    }
                }
                return Ok(0);
            }
            let tasks = store::read_tasks(&store)?;
            let filter = ListFilter {
                all,
                completed,
                in_progress,
                blocked,
                ready,
                query: filter.or(query),
                issue,
                commit,
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
            let archive = store::read_archive(&store)?;
            let selected = ids
                .iter()
                .map(|id| {
                    listing::find(&tasks, id)
                        .map(Shown::Live)
                        .or_else(|| {
                            archive
                                .iter()
                                .find(|record| record.id == *id)
                                .map(Shown::Archived)
                        })
                        .ok_or_else(|| anyhow::anyhow!("task {id} not found"))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            if json {
                let values: Vec<serde_json::Value> = selected
                    .iter()
                    .map(|shown| match shown {
                        Shown::Live(task) => serde_json::to_value(task),
                        Shown::Archived(record) => serde_json::to_value(record),
                    })
                    .collect::<Result<_, _>>()?;
                match values.as_slice() {
                    [single] => writeln!(stdout, "{single}")?,
                    many => writeln!(stdout, "{}", serde_json::Value::Array(many.to_vec()))?,
                }
            } else {
                for (index, shown) in selected.iter().enumerate() {
                    if index > 0 {
                        writeln!(stdout)?;
                    }
                    let rendered = match shown {
                        Shown::Live(task) => show::render(&tasks, task, full || expand),
                        Shown::Archived(record) => show::render_archived(record),
                    };
                    write!(stdout, "{rendered}")?;
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

enum Shown<'a> {
    Live(&'a Task),
    Archived(&'a crate::archive::ArchivedTask),
}

struct ArchiveOutcome {
    archived: usize,
    roots: usize,
    reduction_percent: usize,
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
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
