use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::store::{self, WriteOptions};
use crate::sync::github::service::github_token;
use crate::task::Task;

pub struct Context<'a> {
    pub store_dir: &'a Path,
    pub cwd: &'a Path,
    pub config: &'a Config,
    pub config_path: Option<&'a Path>,
    pub write_options: &'a WriteOptions,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Severity {
    Error,
    Warning,
}

type TaskFix = Box<dyn Fn(&mut Vec<Task>) -> anyhow::Result<()>>;

enum Fix {
    Task(TaskFix),
    Config(PathBuf),
    Migrate { from: PathBuf, to: PathBuf },
}

struct Issue {
    severity: Severity,
    message: String,
    fix: Option<Fix>,
}

impl Issue {
    fn error(message: String) -> Self {
        Self {
            severity: Severity::Error,
            message,
            fix: None,
        }
    }

    fn warning(message: String, fix: Option<Fix>) -> Self {
        Self {
            severity: Severity::Warning,
            message,
            fix,
        }
    }
}

pub fn run<W: Write>(ctx: &Context<'_>, fix: bool, out: &mut W) -> anyhow::Result<i32> {
    let mut issues = Vec::new();

    writeln!(out, "\nChecking storage location...")?;
    let location = check_storage_location(ctx);
    report(out, &location, "Storage location correct")?;
    issues.extend(location);

    writeln!(out, "\nChecking config...")?;
    let config_issues = check_config(ctx);
    report(out, &config_issues, "Config valid")?;
    issues.extend(config_issues);

    writeln!(out, "\nChecking storage...")?;
    let tasks = store::read_tasks(ctx.store_dir).unwrap_or_default();
    let storage = check_storage(&tasks);
    report(out, &storage, &format!("{} task(s) validated", tasks.len()))?;
    issues.extend(storage);

    writeln!(out)?;
    if issues.is_empty() {
        writeln!(out, "No issues found.")?;
        return Ok(0);
    }
    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    let warnings = issues.len() - errors;
    let mut parts = Vec::new();
    if errors > 0 {
        parts.push(format!("{errors} error(s)"));
    }
    if warnings > 0 {
        parts.push(format!("{warnings} warning(s)"));
    }
    writeln!(out, "Found {}.", parts.join(", "))?;
    let fixable = issues.iter().filter(|i| i.fix.is_some()).count();
    if fixable == 0 {
        return Ok(0);
    }
    if !fix {
        writeln!(out, "\nRun dex doctor --fix to fix {fixable} issue(s).")?;
        return Ok(0);
    }

    writeln!(out, "\nApplying fixes...")?;
    let mut fixed = 0;
    let mut task_fixes = Vec::new();
    for issue in issues {
        match issue.fix {
            None => {}
            Some(Fix::Task(apply)) => task_fixes.push((issue.message, apply)),
            Some(Fix::Config(path)) => match add_auto_sync_config(&path) {
                Ok(()) => {
                    writeln!(out, "  ✓ Fixed: {}", issue.message)?;
                    fixed += 1;
                }
                Err(_) => writeln!(out, "  ✗ Failed to fix: {}", issue.message)?,
            },
            Some(Fix::Migrate { from, to }) => match migrate_storage(&from, &to) {
                Ok(count) => {
                    writeln!(
                        out,
                        "  Migrated {count} task(s) from {} to {}",
                        from.display(),
                        to.display()
                    )?;
                    writeln!(out, "  ✓ Fixed: {}", issue.message)?;
                    fixed += 1;
                }
                Err(_) => writeln!(out, "  ✗ Failed to fix: {}", issue.message)?,
            },
        }
    }
    if !task_fixes.is_empty() {
        // Applied together so the store validates once every repair is in.
        let outcome = store::transact_with(ctx.store_dir, ctx.write_options, |tasks| {
            for (_, apply) in &task_fixes {
                apply(tasks)?;
            }
            Ok(())
        });
        for (message, _) in &task_fixes {
            match &outcome {
                Ok(()) => {
                    writeln!(out, "  ✓ Fixed: {message}")?;
                    fixed += 1;
                }
                Err(_) => writeln!(out, "  ✗ Failed to fix: {message}")?,
            }
        }
    }
    writeln!(out, "\nFixed {fixed} issue(s).")?;
    Ok(0)
}

fn report<W: Write>(out: &mut W, issues: &[Issue], ok: &str) -> anyhow::Result<()> {
    if issues.is_empty() {
        writeln!(out, "  ✓ {ok}")?;
    }
    for issue in issues {
        let icon = match issue.severity {
            Severity::Error => "✗",
            Severity::Warning => "⚠",
        };
        writeln!(out, "  {icon} {}", issue.message)?;
    }
    Ok(())
}

fn check_storage_location(ctx: &Context<'_>) -> Vec<Issue> {
    let mut issues = Vec::new();
    let current = ctx.store_dir.to_path_buf();
    let alternate = if ctx.config.centralized {
        store::git_root(ctx.cwd)
            .ok()
            .flatten()
            .map(|root| (root.join(".dex"), "in-repo (.dex/)".to_string()))
    } else {
        store::project_key(ctx.cwd).ok().and_then(|key| {
            config::dex_home().ok().map(|home| {
                (
                    home.join("projects").join(&key),
                    format!("centralized (~/.dex/projects/{key}/)"),
                )
            })
        })
    };
    if let Some((path, label)) = alternate
        && path != current
    {
        let count = count_tasks(&path);
        if count > 0 {
            issues.push(Issue::warning(
                format!(
                    "Found {count} task(s) in previous {label} location. These were not migrated when storage mode changed."
                ),
                Some(Fix::Migrate {
                    from: path,
                    to: current,
                }),
            ));
        }
    }
    issues
}

fn count_tasks(store_dir: &Path) -> usize {
    fs::read_to_string(store_dir.join("tasks.jsonl"))
        .map(|contents| {
            contents
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count()
        })
        .unwrap_or(0)
}

fn migrate_storage(from: &Path, to: &Path) -> anyhow::Result<usize> {
    fs::create_dir_all(to)?;
    let source = fs::read_to_string(from.join("tasks.jsonl"))?;
    let target_path = to.join("tasks.jsonl");
    let existing = fs::read_to_string(&target_path).unwrap_or_default();
    let known: std::collections::HashSet<String> = existing
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| value["id"].as_str().map(str::to_string))
        .collect();
    let mut merged = existing.trim_end().to_string();
    let mut migrated = 0;
    for line in source.lines().filter(|line| !line.trim().is_empty()) {
        let id = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|value| value["id"].as_str().map(str::to_string));
        if id.is_some_and(|id| !known.contains(&id)) {
            if !merged.is_empty() {
                merged.push('\n');
            }
            merged.push_str(line);
            migrated += 1;
        }
    }
    if !merged.is_empty() {
        merged.push('\n');
    }
    fs::write(&target_path, merged)?;
    fs::remove_file(from.join("tasks.jsonl"))?;
    let _ = fs::remove_dir(from);
    Ok(migrated)
}

fn check_config(ctx: &Context<'_>) -> Vec<Issue> {
    let mut issues = Vec::new();
    let global = match ctx.config_path {
        Some(path) => Some(path.to_path_buf()),
        None => config::global_config_path().ok(),
    };
    let project = config::project_config_path(ctx.cwd).ok().flatten();
    for (path, label) in [(global.clone(), "Global"), (project.clone(), "Project")] {
        let Some(path) = path else {
            continue;
        };
        if path.exists() && config::read_file(&path).is_err() {
            issues.push(Issue::error(format!(
                "{label} config invalid TOML: {}",
                path.display()
            )));
        }
    }
    if ctx.config.engine != "file" {
        issues.push(Issue::error(format!(
            "Unsupported storage engine '{}'. Only 'file' is supported. Use sync.github for GitHub integration.",
            ctx.config.engine
        )));
    }
    if ctx.config.github.enabled {
        let env_name = ctx
            .config
            .github
            .token_env
            .as_deref()
            .unwrap_or("GITHUB_TOKEN");
        if github_token(ctx.config.github.token_env.as_deref()).is_none() {
            issues.push(Issue::warning(
                format!(
                    "GitHub sync enabled but no token found (checked {env_name} env var and gh CLI)"
                ),
                None,
            ));
        }
        for (path, label) in [(global, "global"), (project, "project")] {
            let Some(path) = path else {
                continue;
            };
            let Ok(raw) = config::read_file(&path) else {
                continue;
            };
            let enabled =
                config::lookup(&raw, "sync.github.enabled").and_then(|v| v.as_bool()) == Some(true);
            let has_auto = config::lookup(&raw, "sync.github.auto").is_some();
            if enabled && !has_auto {
                issues.push(Issue::warning(
                    format!(
                        "Missing [sync.github.auto] in {label} config ({})",
                        path.display()
                    ),
                    Some(Fix::Config(path)),
                ));
            }
        }
    }
    issues
}

fn add_auto_sync_config(path: &Path) -> anyhow::Result<()> {
    let mut document = config::read_file(path)?;
    if config::lookup(&document, "sync.github.enabled").is_none() {
        config::set(
            &mut document,
            "sync.github.enabled",
            toml::Value::Boolean(true),
        );
    }
    config::set(
        &mut document,
        "sync.github.auto.on_change",
        toml::Value::Boolean(true),
    );
    config::write_file(path, &document)
}

fn check_storage(tasks: &[Task]) -> Vec<Issue> {
    let mut issues = Vec::new();
    let exists = |id: &str| tasks.iter().any(|task| task.id == id);
    for task in tasks {
        let id = task.id.clone();
        if let Some(parent_id) = &task.parent_id {
            if !exists(parent_id) {
                let fix_id = id.clone();
                issues.push(Issue::warning(
                    format!("Task {id}: parent_id '{parent_id}' does not exist (orphaned)"),
                    Some(Fix::Task(Box::new(move |tasks| {
                        if let Some(task) = tasks.iter_mut().find(|task| task.id == fix_id) {
                            task.parent_id = None;
                        }
                        Ok(())
                    }))),
                ));
            } else if !tasks
                .iter()
                .any(|candidate| &candidate.id == parent_id && candidate.children.contains(&id))
            {
                let (child_id, parent) = (id.clone(), parent_id.clone());
                issues.push(Issue::warning(
                    format!(
                        "Task {id}: parent_id '{parent_id}' but parent does not list it as a child"
                    ),
                    Some(Fix::Task(Box::new(move |tasks| {
                        if let Some(parent) = tasks.iter_mut().find(|task| task.id == parent) {
                            parent.children.push(child_id.clone());
                        }
                        Ok(())
                    }))),
                ));
            }
        }
        match depth(tasks, &id) {
            None => issues.push(Issue::error(format!(
                "Task {id}: circular parent reference detected"
            ))),
            Some(depth) if depth > 3 => issues.push(Issue::error(format!(
                "Task {id}: exceeds max depth (depth={depth}, max=3)"
            ))),
            Some(_) => {}
        }
        for blocker in &task.blocked_by {
            if !exists(blocker) {
                let (fix_id, blocker) = (id.clone(), blocker.clone());
                issues.push(Issue::warning(
                    format!("Task {id}: blockedBy '{blocker}' does not exist (dangling reference)"),
                    Some(Fix::Task(Box::new(move |tasks| {
                        if let Some(task) = tasks.iter_mut().find(|task| task.id == fix_id) {
                            task.blocked_by.retain(|existing| existing != &blocker);
                        }
                        Ok(())
                    }))),
                ));
            }
        }
        for blocked in &task.blocks {
            match tasks.iter().find(|candidate| &candidate.id == blocked) {
                None => issues.push(Issue::warning(
                    format!("Task {id}: blocks '{blocked}' does not exist (dangling reference)"),
                    None,
                )),
                Some(target) if !target.blocked_by.contains(&id) => {
                    let (blocker, blocked) = (id.clone(), blocked.clone());
                    issues.push(Issue::warning(
                        format!("Task {id}: blocks '{blocked}' but {blocked}.blockedBy is missing '{id}'"),
                        Some(Fix::Task(Box::new(move |tasks| {
                            if let Some(task) = tasks.iter_mut().find(|task| task.id == blocked) {
                                task.blocked_by.push(blocker.clone());
                            }
                            Ok(())
                        }))),
                    ));
                }
                Some(_) => {}
            }
        }
        for child in &task.children {
            match tasks.iter().find(|candidate| &candidate.id == child) {
                None => issues.push(Issue::error(format!(
                    "Task {id}: child '{child}' does not exist (data corruption)"
                ))),
                Some(found) if found.parent_id.as_deref() != Some(id.as_str()) => {
                    let (child_id, parent) = (child.clone(), id.clone());
                    issues.push(Issue::warning(
                        format!(
                            "Task {id}: lists child '{child}' but child's parent_id is '{}'",
                            found.parent_id.as_deref().unwrap_or("null")
                        ),
                        Some(Fix::Task(Box::new(move |tasks| {
                            if let Some(task) = tasks.iter_mut().find(|task| task.id == child_id) {
                                task.parent_id = Some(parent.clone());
                            }
                            Ok(())
                        }))),
                    ));
                }
                Some(_) => {}
            }
        }
    }
    issues
}

/// Depth counted like the original (1 for a root), None on a cycle.
fn depth(tasks: &[Task], id: &str) -> Option<usize> {
    let mut visited = std::collections::HashSet::new();
    let mut current = id.to_string();
    let mut depth = 1;
    loop {
        if !visited.insert(current.clone()) {
            return None;
        }
        let task = tasks.iter().find(|task| task.id == current)?;
        match &task.parent_id {
            Some(parent) if tasks.iter().any(|task| &task.id == parent) => {
                depth += 1;
                current = parent.clone();
            }
            _ => return Some(depth),
        }
    }
}
