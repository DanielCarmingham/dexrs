use std::ffi::OsString;
use std::io::Write;

use anyhow::Context;
use clap::Parser;
use serde_json::json;

use std::collections::HashSet;

use crate::archive;
use crate::cli::{Cli, Command};
use crate::config;
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
    let args: Vec<OsString> = args.into_iter().collect();
    let invoked_as = args
        .first()
        .map(std::path::Path::new)
        .and_then(std::path::Path::file_stem)
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "dexrs".to_string());
    let cli = Cli::parse_from(&args);
    let command = cli.command.unwrap_or(Command::Status { json: false });
    let env_storage_path = std::env::var_os("DEX_STORAGE_PATH");
    let resolution = store::Resolution {
        cli_storage_path: cli.storage_path.as_deref(),
        cli_config_path: cli.config.as_deref(),
        env_storage_path: env_storage_path.as_deref(),
    };
    let resolved_store = || store::resolve_store_dir(&std::env::current_dir()?, &resolution);

    match command {
        Command::Completion { shell } => {
            let mut command = <Cli as clap::CommandFactory>::command().name(invoked_as.clone());
            clap_complete::generate(shell, &mut command, invoked_as, &mut stdout);
            Ok(0)
        }
        Command::Dir { global } => {
            let path = if global {
                config::dex_home()?
            } else {
                resolved_store()?
            };
            writeln!(stdout, "{}", path.display())?;
            Ok(0)
        }
        Command::Init { yes: _, config_dir } => {
            let config_path = match config_dir {
                Some(dir) => dir.join("dex.toml"),
                None => config::global_config_path()?,
            };
            if config_path.exists() {
                anyhow::bail!(
                    "Config file already exists at {}\nEdit the file directly or delete it to reinitialize.",
                    config_path.display()
                );
            }
            if let Some(parent) = config_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&config_path, config::DEFAULT_CONFIG)?;
            writeln!(stdout, "✓ Created config file at {}", config_path.display())?;
            writeln!(stdout)?;
            writeln!(
                stdout,
                "Shell completions: add `eval \"$({invoked_as} completion zsh)\"` (or bash/fish) to your shell config."
            )?;
            Ok(0)
        }
        Command::Config {
            input,
            global,
            local,
            unset,
            list,
        } => {
            let cwd = std::env::current_dir()?;
            let global_path = match &cli.config {
                Some(path) => path.clone(),
                None => config::global_config_path()?,
            };
            let project_path = config::project_config_path(&cwd)?;
            let (target_path, label) = if local {
                let Some(project_path) = project_path.clone() else {
                    anyhow::bail!(
                        "--local requires being in a git repository\nRun dex init to initialize a git repository or use --global"
                    );
                };
                (project_path, "local")
            } else {
                let _ = global;
                (global_path.clone(), "global")
            };
            let effective = |key: &str| -> anyhow::Result<Option<toml::Value>> {
                let global_value = config::read_file(&global_path)?;
                let local_value = match &project_path {
                    Some(path) => config::read_file(path)?,
                    None => toml::Value::Table(Default::default()),
                };
                Ok(config::lookup(&local_value, key)
                    .map(|value| (value.clone(), "local"))
                    .or_else(|| {
                        config::lookup(&global_value, key).map(|value| (value.clone(), "global"))
                    })
                    .map(|(value, source)| {
                        toml::Value::Table(toml::map::Map::from_iter([
                            ("value".to_string(), value),
                            (
                                "source".to_string(),
                                toml::Value::String(source.to_string()),
                            ),
                        ]))
                    }))
            };

            if list {
                writeln!(stdout, "Configuration:\n")?;
                for spec in config::SCHEMA {
                    if let Some(found) = effective(spec.key)? {
                        writeln!(
                            stdout,
                            "{} = {} [{}]",
                            spec.key,
                            config::format_value(found.get("value")),
                            found
                                .get("source")
                                .and_then(toml::Value::as_str)
                                .unwrap_or_default()
                        )?;
                    }
                }
                return Ok(0);
            }
            let Some(input) = input else {
                anyhow::bail!(
                    "Missing config key\nUsage: dex config <key>[=<value>]\nRun dex config --help for available keys."
                );
            };
            if unset {
                config::spec(&input)?;
                let mut document = config::read_file(&target_path)?;
                if config::unset(&mut document, &input) {
                    config::write_file(&target_path, &document)?;
                    writeln!(stdout, "Unset {input} in {label} config")?;
                } else {
                    writeln!(stdout, "Key {input} was not set in {label} config")?;
                }
                return Ok(0);
            }
            match input.split_once('=') {
                None => {
                    config::spec(&input)?;
                    let found = effective(&input)?;
                    writeln!(
                        stdout,
                        "{}",
                        config::format_value(found.as_ref().and_then(|found| found.get("value")))
                    )?;
                }
                Some((key, raw)) => {
                    let spec = config::spec(key)?;
                    let value = config::parse_value(spec, raw)?;
                    let mut document = config::read_file(&target_path)?;
                    config::set(&mut document, key, value.clone());
                    config::write_file(&target_path, &document)?;
                    writeln!(
                        stdout,
                        "Set {key} = {} in {label} config",
                        config::format_value(Some(&value))
                    )?;
                }
            }
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
            let (id, tasks) = store::transact(&store, |tasks| {
                let id = generate_id(|candidate| tasks.iter().any(|task| task.id == candidate));
                tasks.push(Task::new(id.clone(), name, description, priority));
                relations::set_parent(tasks, &id, parent.as_deref())?;
                for blocker in blocked_by
                    .iter()
                    .flat_map(|value| relations::split_ids(value))
                {
                    relations::add_blocker(tasks, &id, blocker)?;
                }
                Ok((id, tasks.clone()))
            })?;
            writeln!(stdout, "Created task {id}")?;
            write!(stdout, "{}", card(&tasks, &id))?;
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
            let tasks = store::transact(&store, |tasks| {
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
                Ok(tasks.clone())
            })?;
            writeln!(stdout, "Started task {id}")?;
            write!(stdout, "{}", card(&tasks, &id))?;
            Ok(0)
        }
        Command::Complete {
            id,
            result,
            commit,
            no_commit,
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
            let has_commit_decision = commit.is_some() || no_commit;
            let (tasks, open_blockers) = store::transact(&store, |tasks| {
                validate_completion(tasks, &id, force)?;
                let existing = listing::find(tasks, &id)
                    .ok_or_else(|| anyhow::anyhow!("task {id} not found"))?;
                if existing.children.is_empty()
                    && !has_commit_decision
                    && let Some(link) = remote_link(existing)
                {
                    anyhow::bail!(
                        "Task is linked to {link}.\n  \
                         Use --commit <sha> to link a commit (closes issue when merged)\n  \
                         Use --no-commit to complete without a commit (issue stays open)"
                    );
                }
                let open_blockers: Vec<(String, String)> = existing
                    .blocked_by
                    .iter()
                    .filter_map(|blocker| listing::find(tasks, blocker))
                    .filter(|blocker| !blocker.completed)
                    .map(|blocker| (blocker.id.clone(), blocker.name.clone()))
                    .collect();
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
                Ok((tasks.clone(), open_blockers))
            })?;
            if !open_blockers.is_empty() {
                writeln!(
                    stdout,
                    "Warning: This task is blocked by {} incomplete task(s):",
                    open_blockers.len()
                )?;
                for (blocker_id, name) in &open_blockers {
                    writeln!(stdout, "  • {blocker_id}: {name}")?;
                }
                writeln!(stdout)?;
            }
            writeln!(stdout, "Completed task {id}")?;
            write!(stdout, "{}", card(&tasks, &id))?;
            let parent = listing::find(&tasks, &id)
                .and_then(|task| task.parent_id.as_deref())
                .and_then(|parent_id| listing::find(&tasks, parent_id));
            if let Some(parent) = parent {
                let siblings_done = parent
                    .children
                    .iter()
                    .all(|child| listing::find(&tasks, child).is_none_or(|child| child.completed));
                if siblings_done && !parent.completed {
                    writeln!(stdout)?;
                    writeln!(
                        stdout,
                        "Hint: All subtasks of {} are now complete.",
                        parent.name
                    )?;
                    writeln!(
                        stdout,
                        "  • Complete parent: dex complete {} --result \"...\"",
                        parent.id
                    )?;
                    if remote_link(parent).is_some() {
                        writeln!(
                            stdout,
                            "    (Parent task with subtasks doesn't require --commit/--no-commit)"
                        )?;
                    }
                }
            }
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
            let tasks = store::transact(&store, |tasks| {
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
                Ok(tasks.clone())
            })?;
            writeln!(stdout, "Updated task {id}")?;
            let task =
                listing::find(&tasks, &id).ok_or_else(|| anyhow::anyhow!("task {id} not found"))?;
            writeln!(stdout, "{}", listing::task_line(&tasks, task))?;
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
                        Shown::Live(task) => show::enriched_json(&tasks, task, expand),
                        Shown::Archived(record) => show::archived_json(record),
                    })
                    .collect();
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
                        Shown::Live(task) => show::render(&tasks, task, full, expand),
                        Shown::Archived(record) => show::render_archived(record),
                    };
                    write!(stdout, "{rendered}")?;
                }
            }
            Ok(0)
        }
    }
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

fn card(tasks: &[Task], id: &str) -> String {
    listing::find(tasks, id)
        .map(|task| show::render(tasks, task, false, false))
        .unwrap_or_default()
}

fn remote_link(task: &Task) -> Option<String> {
    let metadata = task.metadata.as_ref()?;
    if let Some(github) = metadata.get("github") {
        let number = github
            .get("issueNumber")
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".to_string());
        return Some(format!("GitHub issue #{number}"));
    }
    metadata
        .get("shortcut")
        .map(|_| "Shortcut story".to_string())
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
