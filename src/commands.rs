use std::ffi::OsString;
use std::io::Write;

use clap::Parser;

use crate::cli::{Cli, Command};
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
        Command::Complete { id, result, force } => {
            let store = resolved_store()?;
            store::transact(&store, |tasks| {
                validate_completion(tasks, &id, force)?;
                let now = timestamp();
                let task = find_task_mut(tasks, &id)?;
                task.completed = true;
                task.result = result;
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
                    task.description = Some(description);
                }
                if let Some(priority) = priority {
                    task.priority = Some(priority);
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
    }
}

fn resolved_store() -> anyhow::Result<std::path::PathBuf> {
    let cwd = std::env::current_dir()?;
    store::resolve_store_dir(&cwd, std::env::var_os("DEX_STORAGE_PATH").as_deref())
}

fn find_task_mut<'a>(tasks: &'a mut [Task], id: &str) -> anyhow::Result<&'a mut Task> {
    tasks
        .iter_mut()
        .find(|task| task.id == id)
        .ok_or_else(|| anyhow::anyhow!("task {id} not found"))
}
