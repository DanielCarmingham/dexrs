use std::path::Path;

use serde_json::Value;

use crate::config::{Config, IntegrationConfig};
use crate::service::{self, UpdateInput};
use crate::store::{self, WriteOptions};
use crate::sync::github::service::{GitHubSyncService, Progress, SyncResult};
use crate::sync::shortcut::service::ShortcutSyncService;
use crate::sync::state::{is_sync_stale, write_sync_state};
use crate::task::{Task, timestamp};

pub enum Service {
    GitHub(GitHubSyncService),
    Shortcut(ShortcutSyncService),
}

impl Service {
    pub fn id(&self) -> &'static str {
        match self {
            Service::GitHub(_) => "github",
            Service::Shortcut(_) => "shortcut",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Service::GitHub(_) => "GitHub",
            Service::Shortcut(_) => "Shortcut",
        }
    }

    /// The repository or workspace the service pushes to, for messages.
    pub fn target(&self) -> String {
        match self {
            Service::GitHub(github) => github.repo_string(),
            Service::Shortcut(shortcut) => shortcut.workspace().to_string(),
        }
    }

    pub fn has_remote(&self, task: &Task) -> bool {
        match self {
            Service::GitHub(_) => GitHubSyncService::remote_id(task).is_some(),
            Service::Shortcut(_) => ShortcutSyncService::remote_id(task).is_some(),
        }
    }

    pub fn sync_task(&self, task: &Task, tasks: &[Task]) -> anyhow::Result<Option<SyncResult>> {
        match self {
            Service::GitHub(github) => github.sync_task(task, tasks),
            Service::Shortcut(shortcut) => shortcut.sync_task(task, tasks),
        }
    }

    pub fn sync_all(
        &self,
        tasks: &[Task],
        on_progress: &mut dyn FnMut(Progress),
    ) -> anyhow::Result<Vec<SyncResult>> {
        match self {
            Service::GitHub(github) => github.sync_all(tasks, on_progress),
            Service::Shortcut(shortcut) => shortcut.sync_all(tasks, on_progress),
        }
    }

    pub fn close_remote(&self, task: &Task) -> anyhow::Result<()> {
        match self {
            Service::GitHub(github) => github.close_remote(task),
            Service::Shortcut(shortcut) => shortcut.close_remote(task),
        }
    }

    fn config<'a>(&self, config: &'a Config) -> &'a IntegrationConfig {
        match self {
            Service::GitHub(_) => &config.github,
            Service::Shortcut(_) => &config.shortcut,
        }
    }
}

pub fn result_url(result: &SyncResult) -> Option<&str> {
    result.metadata["issueUrl"]
        .as_str()
        .or_else(|| result.metadata["storyUrl"].as_str())
}

/// Builds every enabled and usable service, printing the original's
/// warnings for enabled services that cannot run.
pub fn services(config: &Config, cwd: &Path) -> Vec<Service> {
    let mut services = Vec::new();
    if let Some(github) = crate::sync::github::service::service_if_configured(&config.github, cwd) {
        services.push(Service::GitHub(github));
    }
    if let Some(shortcut) =
        crate::sync::shortcut::service::service_if_configured(&config.shortcut, cwd)
    {
        services.push(Service::Shortcut(shortcut));
    }
    services
}

/// Writes a sync result's metadata (and any state pulled from the remote)
/// into the task store, recursing into subtask results.
pub fn apply_result(
    tasks: &mut [Task],
    service_id: &str,
    result: &SyncResult,
) -> anyhow::Result<()> {
    if let Some(task) = tasks.iter().find(|task| task.id == result.task_id) {
        let mut metadata = match &task.metadata {
            Some(Value::Object(existing)) => existing.clone(),
            _ => serde_json::Map::new(),
        };
        if !result.metadata.as_object().is_some_and(|m| m.is_empty()) {
            metadata.insert(service_id.to_string(), result.metadata.clone());
        }
        let mut input = UpdateInput {
            id: result.task_id.clone(),
            ..UpdateInput::default()
        };
        if let Some(updates) = result
            .local_updates
            .as_ref()
            .filter(|_| result.pulled_from_remote)
        {
            if let Some(commit) = &updates.commit {
                metadata.insert("commit".to_string(), commit.clone());
            }
            input.completed = updates.completed;
            if updates.completed_at.is_some() {
                input.completed_at = Some(updates.completed_at.clone());
            }
            if updates.result.is_some() {
                input.result = Some(updates.result.clone());
            }
            if updates.started_at.is_some() {
                input.started_at = Some(updates.started_at.clone());
            }
            input.updated_at = updates.updated_at.clone();
        }
        input.metadata = Some(Some(Value::Object(metadata)));
        service::update(tasks, input)?;
    }
    for subtask in &result.subtask_results {
        apply_result(tasks, service_id, subtask)?;
    }
    Ok(())
}

/// Runs the original's post-mutation sync: every configured service whose
/// auto settings ask for it syncs the task, metadata is saved, failures are
/// warned about, and the sync state is stamped.
pub fn auto_sync(
    store_dir: &Path,
    options: &WriteOptions,
    config: &Config,
    cwd: &Path,
    task_id: &str,
) {
    let services = services(config, cwd);
    if services.is_empty() {
        return;
    }
    let wants_sync = services.iter().any(|service| {
        let integration = service.config(config);
        integration.syncs_on_change()
            || integration
                .auto_max_age
                .as_deref()
                .is_some_and(|max_age| is_sync_stale(store_dir, max_age))
    });
    if !wants_sync {
        return;
    }
    let tasks = match store::read_tasks(store_dir) {
        Ok(tasks) => tasks,
        Err(_) => return,
    };
    let Some(task) = tasks.iter().find(|task| task.id == task_id) else {
        return;
    };
    for service in &services {
        match service.sync_task(task, &tasks) {
            Ok(Some(result)) if !result.skipped => {
                let outcome = store::transact_with(store_dir, options, |tasks| {
                    apply_result(tasks, service.id(), &result)
                });
                if let Err(error) = outcome {
                    eprintln!("{} sync failed: {error:#}", service.display_name());
                }
            }
            Ok(_) => {}
            Err(error) => eprintln!("{} sync failed: {error:#}", service.display_name()),
        }
    }
    let _ = write_sync_state(store_dir, &timestamp());
}

pub fn close_remotes(config: &Config, cwd: &Path, removed: &[Task]) {
    let services = services(config, cwd);
    for task in removed {
        for service in &services {
            if let Err(error) = service.close_remote(task) {
                eprintln!(
                    "Failed to close {} issue for task {}: {error:#}",
                    service.display_name(),
                    task.id
                );
            }
        }
    }
}
