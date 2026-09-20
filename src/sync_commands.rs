use std::io::Write;
use std::path::Path;

use anyhow::bail;
use serde_json::{Value, json};

use crate::config::Config;
use crate::service::{self, CreateInput, UpdateInput};
use crate::store::{self, WriteOptions};
use crate::sync::github::api::{GitHubClient, Issue};
use crate::sync::github::body::{
    parse_hierarchical_issue_body, parse_root_task_metadata, strip_task_comments,
};
use crate::sync::github::remote::{GitHubRepo, github_repo, parse_issue_ref};
use crate::sync::github::service::{GitHubSyncService, Phase, Progress, SyncResult, github_token};
use crate::sync::registry::{self, Service, apply_result, result_url};
use crate::sync::shortcut::api::{ShortcutClient, Story};
use crate::sync::shortcut::service::shortcut_token;
use crate::sync::shortcut::story::parse_story_description;
use crate::sync::state::write_sync_state;
use crate::task::{Task, timestamp};

pub struct Context<'a> {
    pub store_dir: &'a Path,
    pub cwd: &'a Path,
    pub config: &'a Config,
    pub write_options: &'a WriteOptions,
}

pub struct SyncArgs {
    pub task_id: Option<String>,
    pub github: bool,
    pub shortcut: bool,
    pub dry_run: bool,
}

pub fn sync<W: Write>(ctx: &Context<'_>, args: SyncArgs, out: &mut W) -> anyhow::Result<i32> {
    let all = registry::services(ctx.config, ctx.cwd);
    let mut services = Vec::new();
    if args.github {
        match all
            .into_iter()
            .find(|service| matches!(service, Service::GitHub(_)))
        {
            Some(service) => services.push(service),
            None => bail!(
                "GitHub sync not available.\nEnsure GITHUB_TOKEN is set and sync.github is configured in dex.toml."
            ),
        }
    } else if args.shortcut {
        match all
            .into_iter()
            .find(|service| matches!(service, Service::Shortcut(_)))
        {
            Some(service) => services.push(service),
            None => bail!(
                "Shortcut sync not available.\nEnsure SHORTCUT_API_TOKEN is set and sync.shortcut is configured in dex.toml."
            ),
        }
    } else {
        services = all;
    }
    if services.is_empty() {
        bail!("No sync services available.\nConfigure GitHub and/or Shortcut sync in dex.toml.");
    }

    let tasks = store::read_tasks(ctx.store_dir)?;
    if let Some(task_id) = &args.task_id {
        let Some(task) = tasks.iter().find(|task| &task.id == task_id) else {
            let mut message = format!("Task {task_id} not found");
            if task_id.starts_with("http://") || task_id.starts_with("https://") {
                message.push_str(&format!("\nDid you mean: dex import {task_id}"));
            }
            bail!("{message}");
        };
        let root = root_of(&tasks, task);
        if args.dry_run {
            for service in &services {
                let action = if service.has_remote(root) {
                    "update"
                } else {
                    "create"
                };
                writeln!(
                    out,
                    "Would sync to {} {}:\n  [{action}] {}: {}",
                    service.display_name(),
                    service.target(),
                    root.id,
                    root.name
                )?;
            }
            return Ok(0);
        }
        for service in &services {
            if let Some(result) = service.sync_task(root, &tasks)? {
                save(ctx, service.id(), &result)?;
                writeln!(
                    out,
                    "Synced task {} to {} {}",
                    root.id,
                    service.display_name(),
                    service.target()
                )?;
                if let Some(url) = result_url(&result) {
                    writeln!(out, "  {url}")?;
                }
                if let Some(reason) = &result.issue_not_closing_reason {
                    writeln!(out, "  ! issue staying open ({reason})")?;
                }
            }
        }
        write_sync_state(ctx.store_dir, &timestamp())?;
        return Ok(0);
    }

    let roots: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.parent_id.is_none())
        .collect();
    if roots.is_empty() {
        writeln!(out, "No tasks to sync.")?;
        return Ok(0);
    }
    if args.dry_run {
        for service in &services {
            writeln!(
                out,
                "Would sync {} task(s) to {} {}:",
                roots.len(),
                service.display_name(),
                service.target()
            )?;
            for root in &roots {
                let action = if service.has_remote(root) {
                    "update"
                } else {
                    "create"
                };
                writeln!(out, "  [{action}] {}: {}", root.id, root.name)?;
            }
        }
        return Ok(0);
    }
    for service in &services {
        writeln!(
            out,
            "Syncing {} task(s) to {} {}...",
            roots.len(),
            service.display_name(),
            service.target()
        )?;
        let mut lines = Vec::new();
        let results = service.sync_all(&tasks, &mut |progress: Progress| {
            let name = crate::show::truncate(&progress.task_name, 50);
            let counter = format!("[{}/{}]", progress.current, progress.total);
            match progress.phase {
                Phase::Creating => lines.push(format!("{counter} + {}: {name}", progress.task_id)),
                Phase::Updating => lines.push(format!("{counter} ~ {}: {name}", progress.task_id)),
                Phase::Checking | Phase::Skipped => {}
            }
        })?;
        for line in lines {
            writeln!(out, "{line}")?;
        }
        for result in &results {
            if !result.skipped {
                save(ctx, service.id(), result)?;
            }
        }
        print_summary(out, service, &results)?;
    }
    write_sync_state(ctx.store_dir, &timestamp())?;
    Ok(0)
}

fn print_summary<W: Write>(
    out: &mut W,
    service: &Service,
    results: &[SyncResult],
) -> anyhow::Result<()> {
    let created = results.iter().filter(|r| r.created).count();
    let updated = results
        .iter()
        .filter(|r| !r.created && !r.skipped && !r.pulled_from_remote)
        .count();
    let pulled = results.iter().filter(|r| r.pulled_from_remote).count();
    let skipped = results
        .iter()
        .filter(|r| r.skipped && !r.pulled_from_remote)
        .count();
    writeln!(
        out,
        "Synced to {} {}",
        service.display_name(),
        service.target()
    )?;
    let mut parts = Vec::new();
    if created > 0 {
        parts.push(format!("{created} created"));
    }
    if updated > 0 {
        parts.push(format!("{updated} updated"));
    }
    if pulled > 0 {
        parts.push(format!("{pulled} pulled from remote"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} unchanged"));
    }
    if !parts.is_empty() {
        writeln!(out, "  ({})", parts.join(", "))?;
    }
    for result in results
        .iter()
        .filter(|r| r.issue_not_closing_reason.is_some())
    {
        writeln!(
            out,
            "  ! {}: issue staying open ({})",
            result.task_id,
            result
                .issue_not_closing_reason
                .as_deref()
                .unwrap_or_default()
        )?;
    }
    Ok(())
}

fn save(ctx: &Context<'_>, service_id: &str, result: &SyncResult) -> anyhow::Result<()> {
    store::transact_with(ctx.store_dir, ctx.write_options, |tasks| {
        apply_result(tasks, service_id, result)
    })
}

fn root_of<'a>(tasks: &'a [Task], task: &'a Task) -> &'a Task {
    let mut current = task;
    while let Some(parent) = current
        .parent_id
        .as_deref()
        .and_then(|id| tasks.iter().find(|task| task.id == id))
    {
        current = parent;
    }
    current
}

pub struct ImportArgs {
    pub reference: Option<String>,
    pub all: bool,
    pub github: bool,
    pub shortcut: bool,
    pub update: bool,
    pub dry_run: bool,
}

pub fn import<W: Write>(ctx: &Context<'_>, args: ImportArgs, out: &mut W) -> anyhow::Result<i32> {
    let Some(reference) = args.reference.as_deref() else {
        if !args.all {
            bail!(
                "Reference or --all required\nUsage: dex import #123, dex import sc#123, or dex import --all"
            );
        }
        if !args.shortcut {
            import_all_github(ctx, args.dry_run, args.update, out)?;
        }
        if !args.github {
            import_all_shortcut(ctx, args.dry_run, args.update, out)?;
        }
        return Ok(0);
    };
    if let Some((story_id, workspace)) = parse_shortcut_ref(reference) {
        import_shortcut_story(ctx, story_id, workspace, args.dry_run, args.update, out)?;
    } else {
        import_github_issue(ctx, reference, args.dry_run, args.update, out)?;
    }
    Ok(0)
}

fn parse_shortcut_ref(reference: &str) -> Option<(u64, Option<String>)> {
    let short = regex::Regex::new(r"(?i)^sc#(\d+)$").expect("static");
    if let Some(capture) = short.captures(reference) {
        return Some((capture[1].parse().ok()?, None));
    }
    let url =
        regex::Regex::new(r"(?i)^https?://app\.shortcut\.com/([^/]+)/story/(\d+)").expect("static");
    let capture = url.captures(reference)?;
    Some((capture[2].parse().ok()?, Some(capture[1].to_string())))
}

fn github_client(ctx: &Context<'_>) -> anyhow::Result<GitHubClient> {
    let env_name = ctx
        .config
        .github
        .token_env
        .as_deref()
        .unwrap_or("GITHUB_TOKEN");
    let token = github_token(ctx.config.github.token_env.as_deref()).ok_or_else(|| {
        anyhow::anyhow!(
            "GitHub token not found.\nSet the {env_name} environment variable: export {env_name}=ghp_...\nOr authenticate with: gh auth login"
        )
    })?;
    Ok(GitHubClient::new(
        crate::sync::github::api::api_url(),
        token,
    ))
}

fn github_issue_number(task: &Task) -> Option<u64> {
    GitHubSyncService::remote_id(task)
}

fn import_github_issue<W: Write>(
    ctx: &Context<'_>,
    reference: &str,
    dry_run: bool,
    update: bool,
    out: &mut W,
) -> anyhow::Result<()> {
    let client = github_client(ctx)?;
    let default_repo = github_repo(ctx.cwd);
    let Some(parsed) = parse_issue_ref(reference, default_repo.as_ref()) else {
        bail!(
            "Invalid issue reference: {reference}\nExpected: #123, owner/repo#123, or full GitHub URL"
        );
    };
    let repo = GitHubRepo {
        owner: parsed.owner.clone(),
        repo: parsed.repo.clone(),
    };
    let tasks = store::read_tasks(ctx.store_dir)?;
    let existing = tasks
        .iter()
        .find(|task| github_issue_number(task) == Some(parsed.number))
        .cloned();
    let issue = client.get_issue(&parsed.owner, &parsed.repo, parsed.number)?;
    if let Some(existing) = existing {
        if update {
            if dry_run {
                writeln!(
                    out,
                    "Would update task {} from GitHub issue #{}",
                    existing.id, parsed.number
                )?;
                return Ok(());
            }
            store::transact_with(ctx.store_dir, ctx.write_options, |tasks| {
                update_task_from_issue(tasks, &existing, &issue, &repo)
            })?;
            writeln!(
                out,
                "Updated task {} from GitHub issue #{}",
                existing.id, parsed.number
            )?;
            return Ok(());
        }
        writeln!(
            out,
            "Skipped GitHub issue #{}: already imported as task {}\n  Use --update to refresh from GitHub",
            parsed.number, existing.id
        )?;
        return Ok(());
    }
    let subtasks = parse_hierarchical_issue_body(&issue.body).subtasks;
    if dry_run {
        writeln!(
            out,
            "Would import from GitHub {}/{}:",
            parsed.owner, parsed.repo
        )?;
        writeln!(out, "  #{}: {}", issue.number, issue.title)?;
        if !subtasks.is_empty() {
            writeln!(out, "  ({} subtasks)", subtasks.len())?;
        }
        return Ok(());
    }
    let (task, created_subtasks) =
        store::transact_with(ctx.store_dir, ctx.write_options, |tasks| {
            let task = import_issue_as_task(tasks, &issue, &repo)?;
            let mut mapping: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for subtask in &subtasks {
                let local_parent = subtask
                    .parent_id
                    .as_ref()
                    .and_then(|remote_parent| mapping.get(remote_parent).cloned())
                    .unwrap_or_else(|| task.id.clone());
                let created = service::create(
                    tasks,
                    CreateInput {
                        id: Some(subtask.task.id.clone()),
                        name: subtask.task.name.clone(),
                        description: Some(if subtask.task.description.is_empty() {
                            "Imported from GitHub issue".to_string()
                        } else {
                            subtask.task.description.clone()
                        }),
                        parent_id: Some(local_parent),
                        priority: Some(subtask.task.priority),
                        completed: Some(subtask.task.completed),
                        result: subtask.task.result.clone(),
                        metadata: subtask.task.metadata.clone(),
                        created_at: subtask.task.created_at.clone(),
                        updated_at: subtask.task.updated_at.clone(),
                        started_at: None,
                        completed_at: subtask.task.completed_at.clone(),
                        blocked_by: Vec::new(),
                    },
                )?;
                mapping.insert(subtask.task.id.clone(), created.id);
            }
            Ok((task, mapping.len()))
        })?;
    writeln!(
        out,
        "Imported issue #{} as task {}: \"{}\"",
        parsed.number, task.id, task.name
    )?;
    if created_subtasks > 0 {
        writeln!(out, "  Created {created_subtasks} subtask(s)")?;
    }
    Ok(())
}

struct IssueData {
    description: String,
    metadata: Option<crate::sync::CommentMetadata>,
    github: Value,
}

fn issue_data(issue: &Issue, repo: &GitHubRepo) -> IssueData {
    let description = strip_task_comments(&parse_hierarchical_issue_body(&issue.body).description);
    let repo_string = format!("{}/{}", repo.owner, repo.repo);
    IssueData {
        description,
        metadata: parse_root_task_metadata(&issue.body),
        github: json!({
            "issueNumber": issue.number,
            "issueUrl": format!("https://github.com/{repo_string}/issues/{}", issue.number),
            "repo": repo_string,
        }),
    }
}

fn import_issue_as_task(
    tasks: &mut Vec<Task>,
    issue: &Issue,
    repo: &GitHubRepo,
) -> anyhow::Result<Task> {
    let data = issue_data(issue, repo);
    let meta = data.metadata.as_ref();
    let completed = meta
        .and_then(|m| m.completed)
        .unwrap_or(issue.state == "closed");
    let mut metadata = json!({ "github": data.github });
    if let Some(commit) = meta.and_then(|m| m.commit.clone()) {
        metadata["commit"] = commit;
    }
    service::create(
        tasks,
        CreateInput {
            id: meta.and_then(|m| m.id.clone()),
            name: issue.title.clone(),
            description: Some(if data.description.is_empty() {
                format!("Imported from GitHub issue #{}", issue.number)
            } else {
                data.description.clone()
            }),
            parent_id: None,
            priority: meta.and_then(|m| m.priority),
            completed: Some(completed),
            result: meta
                .and_then(|m| m.result.clone())
                .or_else(|| completed.then(|| "Imported as completed from GitHub".to_string())),
            metadata: Some(metadata),
            created_at: meta.and_then(|m| m.created_at.clone()),
            updated_at: meta.and_then(|m| m.updated_at.clone()),
            started_at: None,
            completed_at: meta.and_then(|m| m.completed_at.clone().flatten()),
            blocked_by: Vec::new(),
        },
    )
}

fn update_task_from_issue(
    tasks: &mut [Task],
    existing: &Task,
    issue: &Issue,
    repo: &GitHubRepo,
) -> anyhow::Result<()> {
    let data = issue_data(issue, repo);
    let meta = data.metadata.as_ref();
    let closed = issue.state == "closed";
    let mut metadata = match &existing.metadata {
        Some(Value::Object(map)) => map.clone(),
        _ => serde_json::Map::new(),
    };
    metadata.insert("github".into(), data.github);
    match meta.and_then(|m| m.commit.clone()) {
        Some(commit) => {
            metadata.insert("commit".into(), commit);
        }
        None => {
            metadata.remove("commit");
        }
    }
    service::update(
        tasks,
        UpdateInput {
            id: existing.id.clone(),
            name: Some(issue.title.clone()),
            description: Some(if data.description.is_empty() {
                existing.description.clone()
            } else {
                data.description.clone()
            }),
            priority: Some(meta.and_then(|m| m.priority).unwrap_or(existing.priority)),
            metadata: Some(Some(Value::Object(metadata))),
            completed: Some(closed),
            result: closed.then(|| {
                Some(
                    meta.and_then(|m| m.result.clone())
                        .or_else(|| existing.result.clone())
                        .unwrap_or_else(|| "Updated from closed GitHub issue".to_string()),
                )
            }),
            ..UpdateInput::default()
        },
    )?;
    Ok(())
}

fn import_all_github<W: Write>(
    ctx: &Context<'_>,
    dry_run: bool,
    update: bool,
    out: &mut W,
) -> anyhow::Result<()> {
    let Some(token) = github_token(ctx.config.github.token_env.as_deref()) else {
        eprintln!("Warning: GitHub token not found, skipping GitHub import.");
        return Ok(());
    };
    let Some(repo) = github_repo(ctx.cwd) else {
        eprintln!("Warning: No GitHub remote found, skipping GitHub import.");
        return Ok(());
    };
    let client = GitHubClient::new(crate::sync::github::api::api_url(), token);
    let label = ctx
        .config
        .github
        .label_prefix
        .clone()
        .unwrap_or_else(|| "dex".to_string());
    let issues: Vec<Issue> = client
        .list_issues(&repo.owner, &repo.repo, &label)?
        .into_iter()
        .filter(|issue| !issue.is_pull_request)
        .collect();
    let target = format!("{}/{}", repo.owner, repo.repo);
    if issues.is_empty() {
        writeln!(
            out,
            "No GitHub issues with \"{label}\" label found in {target}."
        )?;
        return Ok(());
    }
    let tasks = store::read_tasks(ctx.store_dir)?;
    let known = |number: u64| {
        tasks
            .iter()
            .find(|task| github_issue_number(task) == Some(number))
    };
    let to_import: Vec<&Issue> = issues
        .iter()
        .filter(|issue| known(issue.number).is_none())
        .collect();
    let to_update: Vec<&Issue> = if update {
        issues
            .iter()
            .filter(|issue| known(issue.number).is_some())
            .collect()
    } else {
        Vec::new()
    };
    let skipped = issues.len() - to_import.len() - to_update.len();
    if dry_run {
        if !to_import.is_empty() {
            writeln!(
                out,
                "Would import {} GitHub issue(s) from {target}:",
                to_import.len()
            )?;
            for issue in &to_import {
                writeln!(out, "  #{}: {}", issue.number, issue.title)?;
            }
        }
        if !to_update.is_empty() {
            writeln!(
                out,
                "Would update {} task(s) from GitHub {target}:",
                to_update.len()
            )?;
            for issue in &to_update {
                writeln!(
                    out,
                    "  #{} → {}",
                    issue.number,
                    known(issue.number).map(|t| t.id.as_str()).unwrap_or("?")
                )?;
            }
        }
        if skipped > 0 {
            writeln!(
                out,
                "  ({skipped} already imported, use --update to refresh)"
            )?;
        }
        return Ok(());
    }
    let mut imported = 0;
    let mut updated = 0;
    for issue in &to_import {
        let task = store::transact_with(ctx.store_dir, ctx.write_options, |tasks| {
            import_issue_as_task(tasks, issue, &repo)
        })?;
        writeln!(out, "Imported GitHub #{} as {}", issue.number, task.id)?;
        imported += 1;
    }
    for issue in &to_update {
        let existing = known(issue.number).cloned().expect("filtered on known");
        store::transact_with(ctx.store_dir, ctx.write_options, |tasks| {
            update_task_from_issue(tasks, &existing, issue, &repo)
        })?;
        writeln!(out, "Updated GitHub #{} → {}", issue.number, existing.id)?;
        updated += 1;
    }
    writeln!(
        out,
        "\nGitHub: Imported {imported}, updated {updated} issue(s) from {target}"
    )?;
    if skipped > 0 {
        writeln!(
            out,
            "Skipped {skipped} already imported (use --update to refresh)"
        )?;
    }
    Ok(())
}

fn shortcut_client(ctx: &Context<'_>) -> anyhow::Result<ShortcutClient> {
    let env_name = ctx
        .config
        .shortcut
        .token_env
        .as_deref()
        .unwrap_or("SHORTCUT_API_TOKEN");
    let token = shortcut_token(ctx.config.shortcut.token_env.as_deref()).ok_or_else(|| {
        anyhow::anyhow!("Shortcut API token not found.\nSet the {env_name} environment variable.")
    })?;
    Ok(ShortcutClient::new(
        crate::sync::shortcut::api::api_url(),
        token,
    ))
}

fn shortcut_story_id(task: &Task) -> Option<u64> {
    crate::sync::shortcut::service::ShortcutSyncService::remote_id(task)
}

fn story_metadata(
    story: &Story,
    workspace: &str,
) -> (String, Option<crate::sync::CommentMetadata>, Value) {
    let parsed = parse_story_description(&story.description);
    let shortcut = json!({
        "storyId": story.id,
        "storyUrl": story.app_url,
        "workspace": workspace,
        "state": if story.completed { "done" } else { "unstarted" },
    });
    (parsed.context, parsed.metadata, shortcut)
}

fn import_story_as_task(
    tasks: &mut Vec<Task>,
    story: &Story,
    workspace: &str,
) -> anyhow::Result<Task> {
    let (context, meta, shortcut) = story_metadata(story, workspace);
    let completed = meta
        .as_ref()
        .and_then(|m| m.completed)
        .unwrap_or(story.completed);
    let mut metadata = json!({ "shortcut": shortcut });
    if let Some(commit) = meta.as_ref().and_then(|m| m.commit.clone()) {
        metadata["commit"] = commit;
    }
    service::create(
        tasks,
        CreateInput {
            id: meta.as_ref().and_then(|m| m.id.clone()),
            name: story.name.clone(),
            description: Some(if context.is_empty() {
                format!("Imported from Shortcut story #{}", story.id)
            } else {
                context
            }),
            parent_id: None,
            priority: meta.as_ref().and_then(|m| m.priority),
            completed: Some(completed),
            result: meta
                .as_ref()
                .and_then(|m| m.result.clone())
                .or_else(|| completed.then(|| "Imported as completed from Shortcut".to_string())),
            metadata: Some(metadata),
            created_at: meta.as_ref().and_then(|m| m.created_at.clone()),
            updated_at: meta.as_ref().and_then(|m| m.updated_at.clone()),
            started_at: None,
            completed_at: meta.as_ref().and_then(|m| m.completed_at.clone().flatten()),
            blocked_by: Vec::new(),
        },
    )
}

fn update_task_from_story(
    tasks: &mut [Task],
    existing: &Task,
    story: &Story,
    workspace: &str,
) -> anyhow::Result<()> {
    let (context, meta, shortcut) = story_metadata(story, workspace);
    let mut metadata = match &existing.metadata {
        Some(Value::Object(map)) => map.clone(),
        _ => serde_json::Map::new(),
    };
    metadata.insert("shortcut".into(), shortcut);
    match meta.as_ref().and_then(|m| m.commit.clone()) {
        Some(commit) => {
            metadata.insert("commit".into(), commit);
        }
        None => {
            metadata.remove("commit");
        }
    }
    service::update(
        tasks,
        UpdateInput {
            id: existing.id.clone(),
            name: Some(story.name.clone()),
            description: Some(if context.is_empty() {
                existing.description.clone()
            } else {
                context
            }),
            priority: Some(
                meta.as_ref()
                    .and_then(|m| m.priority)
                    .unwrap_or(existing.priority),
            ),
            metadata: Some(Some(Value::Object(metadata))),
            completed: Some(story.completed),
            result: story.completed.then(|| {
                Some(
                    meta.as_ref()
                        .and_then(|m| m.result.clone())
                        .or_else(|| existing.result.clone())
                        .unwrap_or_else(|| "Updated from completed Shortcut story".to_string()),
                )
            }),
            ..UpdateInput::default()
        },
    )?;
    Ok(())
}

fn import_shortcut_story<W: Write>(
    ctx: &Context<'_>,
    story_id: u64,
    workspace: Option<String>,
    dry_run: bool,
    update: bool,
    out: &mut W,
) -> anyhow::Result<()> {
    let client = shortcut_client(ctx)?;
    let story = client.get_story(story_id)?;
    let workspace = match workspace {
        Some(workspace) => workspace,
        None => client.workspace_slug()?,
    };
    let tasks = store::read_tasks(ctx.store_dir)?;
    if let Some(existing) = tasks
        .iter()
        .find(|task| shortcut_story_id(task) == Some(story.id))
        .cloned()
    {
        if update {
            if dry_run {
                writeln!(
                    out,
                    "Would update task {} from Shortcut story #{}",
                    existing.id, story.id
                )?;
                return Ok(());
            }
            store::transact_with(ctx.store_dir, ctx.write_options, |tasks| {
                update_task_from_story(tasks, &existing, &story, &workspace)
            })?;
            writeln!(
                out,
                "Updated task {} from Shortcut story #{}",
                existing.id, story.id
            )?;
            return Ok(());
        }
        writeln!(
            out,
            "Skipped Shortcut story #{}: already imported as task {}\n  Use --update to refresh from Shortcut",
            story.id, existing.id
        )?;
        return Ok(());
    }
    if dry_run {
        writeln!(
            out,
            "Would import from Shortcut {workspace}:\n  #{}: {}",
            story.id, story.name
        )?;
        return Ok(());
    }
    let task = store::transact_with(ctx.store_dir, ctx.write_options, |tasks| {
        import_story_as_task(tasks, &story, &workspace)
    })?;
    writeln!(
        out,
        "Imported Shortcut story #{} as task {}: \"{}\"",
        story.id, task.id, task.name
    )?;
    Ok(())
}

fn import_all_shortcut<W: Write>(
    ctx: &Context<'_>,
    dry_run: bool,
    update: bool,
    out: &mut W,
) -> anyhow::Result<()> {
    let Some(token) = shortcut_token(ctx.config.shortcut.token_env.as_deref()) else {
        eprintln!("Warning: Shortcut API token not found, skipping Shortcut import.");
        return Ok(());
    };
    let client = ShortcutClient::new(crate::sync::shortcut::api::api_url(), token);
    let label = ctx
        .config
        .shortcut
        .label
        .clone()
        .unwrap_or_else(|| "dex".to_string());
    let workspace = match &ctx.config.shortcut.workspace {
        Some(workspace) => workspace.clone(),
        None => client.workspace_slug()?,
    };
    let stories = client.search_stories(&format!("label:\"{label}\""))?;
    if stories.is_empty() {
        writeln!(
            out,
            "No Shortcut stories with \"{label}\" label found in {workspace}."
        )?;
        return Ok(());
    }
    let tasks = store::read_tasks(ctx.store_dir)?;
    let known = |id: u64| {
        tasks
            .iter()
            .find(|task| shortcut_story_id(task) == Some(id))
    };
    let to_import: Vec<&Story> = stories
        .iter()
        .filter(|story| known(story.id).is_none())
        .collect();
    let to_update: Vec<&Story> = if update {
        stories
            .iter()
            .filter(|story| known(story.id).is_some())
            .collect()
    } else {
        Vec::new()
    };
    let skipped = stories.len() - to_import.len() - to_update.len();
    if dry_run {
        if !to_import.is_empty() {
            writeln!(
                out,
                "Would import {} Shortcut story(ies) from {workspace}:",
                to_import.len()
            )?;
            for story in &to_import {
                writeln!(out, "  #{}: {}", story.id, story.name)?;
            }
        }
        if !to_update.is_empty() {
            writeln!(
                out,
                "Would update {} task(s) from Shortcut {workspace}:",
                to_update.len()
            )?;
            for story in &to_update {
                writeln!(
                    out,
                    "  #{} → {}",
                    story.id,
                    known(story.id).map(|t| t.id.as_str()).unwrap_or("?")
                )?;
            }
        }
        if skipped > 0 {
            writeln!(
                out,
                "  ({skipped} already imported, use --update to refresh)"
            )?;
        }
        return Ok(());
    }
    let mut imported = 0;
    let mut updated = 0;
    for story in &to_import {
        let task = store::transact_with(ctx.store_dir, ctx.write_options, |tasks| {
            import_story_as_task(tasks, story, &workspace)
        })?;
        writeln!(out, "Imported Shortcut #{} as {}", story.id, task.id)?;
        imported += 1;
    }
    for story in &to_update {
        let existing = known(story.id).cloned().expect("filtered on known");
        store::transact_with(ctx.store_dir, ctx.write_options, |tasks| {
            update_task_from_story(tasks, &existing, story, &workspace)
        })?;
        writeln!(out, "Updated Shortcut #{} → {}", story.id, existing.id)?;
        updated += 1;
    }
    writeln!(
        out,
        "\nShortcut: Imported {imported}, updated {updated} story(ies) from {workspace}"
    )?;
    if skipped > 0 {
        writeln!(
            out,
            "Skipped {skipped} already imported (use --update to refresh)"
        )?;
    }
    Ok(())
}

pub fn export<W: Write>(
    ctx: &Context<'_>,
    ids: &[String],
    dry_run: bool,
    out: &mut W,
) -> anyhow::Result<i32> {
    if ids.is_empty() {
        bail!("At least one task ID is required\nUsage: dex export <task-id>...");
    }
    let service = crate::sync::github::service::service_or_error(&ctx.config.github, ctx.cwd)?;
    let target = service.repo_string();
    let tasks = store::read_tasks(ctx.store_dir)?;
    let mut exported = 0;
    let mut skipped = 0;
    for id in ids {
        let Some(task) = tasks.iter().find(|task| &task.id == id) else {
            eprintln!("Error: Task {id} not found");
            continue;
        };
        let root = root_of(&tasks, task);
        if github_issue_number(root).is_some() {
            writeln!(out, "Skipped {}: already synced to GitHub", root.id)?;
            skipped += 1;
            continue;
        }
        if dry_run {
            writeln!(
                out,
                "Would export to {target}:\n  [create] {}: {}",
                root.id, root.name
            )?;
            exported += 1;
            continue;
        }
        match service.sync_task(root, &tasks) {
            Ok(Some(result)) => {
                writeln!(out, "Exported task {} to {target}", root.id)?;
                writeln!(
                    out,
                    "  {}",
                    result.metadata["issueUrl"].as_str().unwrap_or_default()
                )?;
                exported += 1;
            }
            Ok(None) => {}
            Err(error) => eprintln!("Error exporting {id}: {error}"),
        }
    }
    if ids.len() > 1 {
        let mut parts = Vec::new();
        if exported > 0 {
            parts.push(format!(
                "{exported} {}",
                if dry_run {
                    "would be exported"
                } else {
                    "exported"
                }
            ));
        }
        if skipped > 0 {
            parts.push(format!("{skipped} skipped"));
        }
        if !parts.is_empty() {
            writeln!(out, "\n{}", parts.join(", "))?;
        }
    }
    Ok(0)
}
