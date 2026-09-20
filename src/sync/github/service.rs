use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::{Value, json};

use crate::config::IntegrationConfig;
use crate::sync::github::api::{GitHubClient, Issue};
use crate::sync::github::body::{
    Descendant, collect_descendants, extract_task_id, parse_hierarchical_issue_body,
    parse_root_task_metadata, render_root_body,
};
use crate::sync::github::remote::{GitHubRepo, github_repo};
use crate::sync::is_commit_on_remote;
use crate::task::Task;

pub const DEFAULT_LABEL_PREFIX: &str = "dex";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Checking,
    Creating,
    Updating,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub current: usize,
    pub total: usize,
    pub task_id: String,
    pub task_name: String,
    pub phase: Phase,
}

/// Fields the remote copy was found to have advanced past the local task.
#[derive(Debug, Clone, Default)]
pub struct LocalUpdates {
    pub updated_at: Option<String>,
    pub completed: Option<bool>,
    pub completed_at: Option<String>,
    pub result: Option<String>,
    pub started_at: Option<String>,
    pub commit: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct SyncResult {
    pub task_id: String,
    pub metadata: Value,
    pub created: bool,
    pub skipped: bool,
    pub pulled_from_remote: bool,
    pub local_updates: Option<LocalUpdates>,
    pub issue_not_closing_reason: Option<String>,
    pub subtask_results: Vec<SyncResult>,
}

impl SyncResult {
    pub fn new_public(task_id: &str, metadata: Value, created: bool) -> Self {
        Self::new(task_id, metadata, created)
    }

    fn new(task_id: &str, metadata: Value, created: bool) -> Self {
        Self {
            task_id: task_id.to_string(),
            metadata,
            created,
            skipped: false,
            pulled_from_remote: false,
            local_updates: None,
            issue_not_closing_reason: None,
            subtask_results: Vec::new(),
        }
    }
}

struct CachedIssue {
    number: u64,
    title: String,
    body: String,
    state: String,
    labels: Vec<String>,
}

pub struct GitHubSyncService {
    client: GitHubClient,
    repo: GitHubRepo,
    label_prefix: String,
    cwd: PathBuf,
}

pub fn github_token(token_env: Option<&str>) -> Option<String> {
    let env_name = token_env.unwrap_or("GITHUB_TOKEN");
    if let Ok(token) = std::env::var(env_name)
        && !token.is_empty()
    {
        return Some(token);
    }
    let output = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Builds the service when sync is enabled and usable, warning like the
/// original when it is enabled but cannot run.
pub fn service_if_configured(config: &IntegrationConfig, cwd: &Path) -> Option<GitHubSyncService> {
    if !config.enabled {
        return None;
    }
    let Some(repo) = github_repo(cwd) else {
        eprintln!("GitHub sync enabled but no GitHub remote found. Sync disabled.");
        return None;
    };
    let env_name = config.token_env.as_deref().unwrap_or("GITHUB_TOKEN");
    let Some(token) = github_token(config.token_env.as_deref()) else {
        eprintln!(
            "GitHub sync enabled but no token found (checked {env_name} and gh CLI). Sync disabled."
        );
        return None;
    };
    Some(GitHubSyncService::new(
        crate::sync::github::api::api_url(),
        token,
        repo,
        config.label_prefix.clone(),
        cwd.to_path_buf(),
    ))
}

pub fn service_or_error(
    config: &IntegrationConfig,
    cwd: &Path,
) -> anyhow::Result<GitHubSyncService> {
    let repo = github_repo(cwd).ok_or_else(|| {
        anyhow::anyhow!(
            "Cannot determine GitHub repository.\nThis directory is not in a git repository with a GitHub remote."
        )
    })?;
    let env_name = config.token_env.as_deref().unwrap_or("GITHUB_TOKEN");
    let token = github_token(config.token_env.as_deref()).ok_or_else(|| {
        anyhow::anyhow!(
            "GitHub token not found.\nSet {env_name} environment variable or authenticate with: gh auth login"
        )
    })?;
    Ok(GitHubSyncService::new(
        crate::sync::github::api::api_url(),
        token,
        repo,
        config.label_prefix.clone(),
        cwd.to_path_buf(),
    ))
}

impl GitHubSyncService {
    pub fn new(
        base_url: String,
        token: String,
        repo: GitHubRepo,
        label_prefix: Option<String>,
        cwd: PathBuf,
    ) -> Self {
        Self {
            client: GitHubClient::new(base_url, token),
            repo,
            label_prefix: label_prefix.unwrap_or_else(|| DEFAULT_LABEL_PREFIX.to_string()),
            cwd,
        }
    }

    pub fn repo(&self) -> &GitHubRepo {
        &self.repo
    }

    pub fn repo_string(&self) -> String {
        format!("{}/{}", self.repo.owner, self.repo.repo)
    }

    pub fn remote_id(task: &Task) -> Option<u64> {
        let metadata = task.metadata.as_ref()?;
        metadata["github"]["issueNumber"]
            .as_u64()
            .or_else(|| metadata["github_issue_number"].as_u64())
    }

    pub fn remote_url(&self, task: &Task) -> Option<String> {
        if let Some(url) = task
            .metadata
            .as_ref()
            .and_then(|m| m["github"]["issueUrl"].as_str())
        {
            return Some(url.to_string());
        }
        Self::remote_id(task).map(|number| self.issue_url(number))
    }

    fn issue_url(&self, number: u64) -> String {
        format!("https://github.com/{}/issues/{number}", self.repo_string())
    }

    pub fn close_remote(&self, task: &Task) -> anyhow::Result<()> {
        let Some(number) = Self::remote_id(task) else {
            return Ok(());
        };
        self.client
            .update_issue(
                &self.repo.owner,
                &self.repo.repo,
                number,
                json!({"state": "closed"}),
            )
            .map(|_| ())
    }

    pub fn sync_task(&self, task: &Task, tasks: &[Task]) -> anyhow::Result<Option<SyncResult>> {
        if let Some(parent_id) = &task.parent_id {
            return match tasks.iter().find(|candidate| &candidate.id == parent_id) {
                Some(parent) => self.sync_task(parent, tasks),
                None => Ok(None),
            };
        }
        self.sync_parent(task, tasks, true, None, 1, 1, &mut |_| {})
            .map(Some)
    }

    pub fn sync_all(
        &self,
        tasks: &[Task],
        on_progress: &mut dyn FnMut(Progress),
    ) -> anyhow::Result<Vec<SyncResult>> {
        let roots: Vec<&Task> = tasks
            .iter()
            .filter(|task| task.parent_id.is_none())
            .collect();
        let total = roots.len();
        let cache = self.fetch_all_dex_issues()?;
        let mut results = Vec::new();
        for (index, root) in roots.iter().enumerate() {
            on_progress(Progress {
                current: index + 1,
                total,
                task_id: root.id.clone(),
                task_name: root.name.clone(),
                phase: Phase::Checking,
            });
            results.push(self.sync_parent(
                root,
                tasks,
                true,
                Some(&cache),
                index + 1,
                total,
                on_progress,
            )?);
        }
        Ok(results)
    }

    #[allow(clippy::too_many_arguments)]
    fn sync_parent(
        &self,
        parent: &Task,
        tasks: &[Task],
        skip_unchanged: bool,
        cache: Option<&HashMap<String, CachedIssue>>,
        current: usize,
        total: usize,
        on_progress: &mut dyn FnMut(Progress),
    ) -> anyhow::Result<SyncResult> {
        let progress = |phase: Phase| Progress {
            current,
            total,
            task_id: parent.id.clone(),
            task_name: parent.name.clone(),
            phase,
        };
        let descendants: Vec<Descendant> = collect_descendants(tasks, &parent.id)
            .into_iter()
            .map(|mut descendant| {
                descendant.task.completed = self.should_mark_completed(&descendant.task, tasks);
                descendant
            })
            .collect();
        let cached = cache.and_then(|cache| cache.get(&parent.id));
        let mut issue_number = Self::remote_id(parent).or(cached.map(|cached| cached.number));
        if issue_number.is_none() {
            issue_number = self.find_issue_by_task_id(&parent.id)?;
        }
        let should_close = self.should_mark_completed(parent, tasks);
        let expected_state = if should_close { "closed" } else { "open" };
        let not_closing = self.issue_not_closing_reason(parent, tasks);

        let Some(number) = issue_number else {
            on_progress(progress(Phase::Creating));
            let metadata = self.create_issue(parent, &descendants, should_close)?;
            let mut result = SyncResult::new(&parent.id, metadata, true);
            result.issue_not_closing_reason = not_closing;
            return Ok(result);
        };

        let stored_state = parent
            .metadata
            .as_ref()
            .and_then(|m| m["github"]["state"].as_str());
        if skip_unchanged && expected_state == "closed" && stored_state == Some("closed") {
            on_progress(progress(Phase::Skipped));
            let mut result =
                SyncResult::new(&parent.id, self.metadata(number, expected_state), false);
            result.skipped = true;
            return Ok(result);
        }

        let mut current_state = cached.map(|cached| cached.state.clone());
        if let Some(cached) = cached
            && let Some(pulled) = self.pull_if_remote_is_newer(
                parent,
                tasks,
                number,
                cached,
                current_state.as_deref(),
            )
        {
            on_progress(progress(Phase::Skipped));
            return Ok(pulled);
        }

        let expected_body = render_root_body(parent, &descendants);
        let expected_labels = self.labels(parent, should_close);
        if skip_unchanged {
            let has_changes = match cached {
                Some(cached) => self.issue_needs_update(
                    &cached.title,
                    &cached.body,
                    &cached.state,
                    &cached.labels,
                    &parent.name,
                    &expected_body,
                    &expected_labels,
                    should_close,
                ),
                None => match self
                    .client
                    .get_issue(&self.repo.owner, &self.repo.repo, number)
                {
                    Ok(issue) => {
                        current_state = Some(issue.state.clone());
                        let labels = self.own_labels(&issue.labels);
                        self.issue_needs_update(
                            &issue.title,
                            &issue.body,
                            &issue.state,
                            &labels,
                            &parent.name,
                            &expected_body,
                            &expected_labels,
                            should_close,
                        )
                    }
                    Err(_) => true,
                },
            };
            if !has_changes {
                on_progress(progress(Phase::Skipped));
                let mut result =
                    SyncResult::new(&parent.id, self.metadata(number, expected_state), false);
                result.skipped = true;
                result.issue_not_closing_reason = not_closing;
                return Ok(result);
            }
        } else if cached.is_none() {
            current_state = self
                .client
                .get_issue(&self.repo.owner, &self.repo.repo, number)
                .ok()
                .map(|issue| issue.state);
        }

        on_progress(progress(Phase::Updating));
        let mut fields = json!({
            "title": parent.name,
            "body": expected_body,
            "labels": expected_labels,
        });
        if should_close {
            fields["state"] = json!("closed");
        } else if current_state.as_deref() == Some("open") {
            fields["state"] = json!("open");
        }
        self.client
            .update_issue(&self.repo.owner, &self.repo.repo, number, fields)
            .with_context(|| format!("failed to update issue #{number}"))?;
        let mut result = SyncResult::new(&parent.id, self.metadata(number, expected_state), false);
        result.issue_not_closing_reason = not_closing;
        Ok(result)
    }

    fn pull_if_remote_is_newer(
        &self,
        parent: &Task,
        tasks: &[Task],
        number: u64,
        cached: &CachedIssue,
        current_state: Option<&str>,
    ) -> Option<SyncResult> {
        let remote = parse_root_task_metadata(&cached.body)?;
        let remote_updated = remote.updated_at.as_deref()?;
        let local_updated = parent.updated_at.as_deref()?;
        if !is_newer(remote_updated, local_updated) {
            return None;
        }
        let mut updates = LocalUpdates {
            updated_at: Some(remote_updated.to_string()),
            ..LocalUpdates::default()
        };
        if remote.completed == Some(true) && !parent.completed {
            updates.completed = Some(true);
            updates.completed_at = remote.completed_at.clone().flatten();
            updates.result = remote.result.clone();
            updates.started_at = remote.started_at.clone().flatten();
        }
        let local_has_commit = parent
            .metadata
            .as_ref()
            .is_some_and(|m| m["commit"]["sha"].is_string());
        if let Some(commit) = &remote.commit
            && !local_has_commit
        {
            updates.commit = Some(commit.clone());
        }
        let mut metadata = self.metadata(number, current_state.unwrap_or("open"));
        if current_state.is_none() {
            metadata["state"] = Value::Null;
        }
        let mut result = SyncResult::new(&parent.id, metadata, false);
        result.skipped = true;
        result.pulled_from_remote = true;
        result.local_updates = Some(updates);
        result.subtask_results = self.reconcile_subtasks(&cached.body, tasks);
        Some(result)
    }

    fn reconcile_subtasks(&self, body: &str, tasks: &[Task]) -> Vec<SyncResult> {
        parse_hierarchical_issue_body(body)
            .subtasks
            .into_iter()
            .filter_map(|remote| {
                let local = tasks.iter().find(|task| task.id == remote.task.id)?;
                let remote_updated = remote.task.updated_at.as_deref()?;
                let local_updated = local.updated_at.as_deref()?;
                if !is_newer(remote_updated, local_updated) {
                    return None;
                }
                let mut updates = LocalUpdates {
                    updated_at: Some(remote_updated.to_string()),
                    ..LocalUpdates::default()
                };
                if remote.task.completed && !local.completed {
                    updates.completed = Some(true);
                    updates.completed_at = remote.task.completed_at.clone();
                    updates.result = remote.task.result.clone();
                    updates.started_at = remote.task.started_at.clone();
                }
                let local_has_commit = local
                    .metadata
                    .as_ref()
                    .is_some_and(|m| m["commit"]["sha"].is_string());
                if let Some(commit) = remote.task.metadata.as_ref().and_then(|m| m.get("commit"))
                    && !local_has_commit
                {
                    updates.commit = Some(commit.clone());
                }
                let mut result = SyncResult::new(&remote.task.id, json!({}), false);
                result.skipped = true;
                result.pulled_from_remote = true;
                result.local_updates = Some(updates);
                Some(result)
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn issue_needs_update(
        &self,
        title: &str,
        body: &str,
        state: &str,
        labels: &[String],
        expected_title: &str,
        expected_body: &str,
        expected_labels: &[String],
        should_close: bool,
    ) -> bool {
        if title != expected_title || body.trim() != expected_body.trim() {
            return true;
        }
        let expected_state = if should_close { "closed" } else { "open" };
        if state != expected_state {
            return true;
        }
        let mut have: Vec<&String> = labels.iter().collect();
        let mut want: Vec<&String> = expected_labels.iter().collect();
        have.sort();
        want.sort();
        have != want
    }

    fn create_issue(
        &self,
        parent: &Task,
        descendants: &[Descendant],
        should_close: bool,
    ) -> anyhow::Result<Value> {
        let body = render_root_body(parent, descendants);
        let labels = self.labels(parent, should_close);
        let issue = self
            .client
            .create_issue(
                &self.repo.owner,
                &self.repo.repo,
                &parent.name,
                &body,
                &labels,
            )
            .context("failed to create GitHub issue")?;
        if should_close {
            self.client.update_issue(
                &self.repo.owner,
                &self.repo.repo,
                issue.number,
                json!({"state": "closed"}),
            )?;
            if let Some(result) = &parent.result {
                self.client.create_comment(
                    &self.repo.owner,
                    &self.repo.repo,
                    issue.number,
                    &format!("## Result\n\n{result}"),
                )?;
            }
        }
        Ok(json!({
            "issueNumber": issue.number,
            "issueUrl": issue.html_url,
            "repo": self.repo_string(),
            "state": if should_close { "closed" } else { "open" },
        }))
    }

    fn metadata(&self, number: u64, state: &str) -> Value {
        json!({
            "issueNumber": number,
            "issueUrl": self.issue_url(number),
            "repo": self.repo_string(),
            "state": state,
        })
    }

    fn labels(&self, task: &Task, should_close: bool) -> Vec<String> {
        vec![
            self.label_prefix.clone(),
            format!("{}:priority-{}", self.label_prefix, task.priority),
            format!(
                "{}:{}",
                self.label_prefix,
                if should_close { "completed" } else { "pending" }
            ),
        ]
    }

    fn own_labels(&self, labels: &[String]) -> Vec<String> {
        labels
            .iter()
            .filter(|label| label.starts_with(&self.label_prefix))
            .cloned()
            .collect()
    }

    pub fn should_mark_completed(&self, task: &Task, tasks: &[Task]) -> bool {
        if !task.completed {
            return false;
        }
        if let Some(sha) = task
            .metadata
            .as_ref()
            .and_then(|m| m["commit"]["sha"].as_str())
        {
            return is_commit_on_remote(&self.cwd, sha);
        }
        let descendants = collect_descendants(tasks, &task.id);
        if descendants.is_empty() {
            return false;
        }
        descendants
            .iter()
            .all(|descendant| self.should_mark_completed(&descendant.task, tasks))
    }

    pub fn issue_not_closing_reason(&self, task: &Task, tasks: &[Task]) -> Option<String> {
        if !task.completed || self.should_mark_completed(task, tasks) {
            return None;
        }
        let blocking: Vec<String> = collect_descendants(tasks, &task.id)
            .iter()
            .filter(|descendant| !self.should_mark_completed(&descendant.task, tasks))
            .map(|descendant| self.subtask_blocking_reason(&descendant.task))
            .collect();
        if !blocking.is_empty() {
            return Some(blocking.join("; "));
        }
        if let Some(sha) = task
            .metadata
            .as_ref()
            .and_then(|m| m["commit"]["sha"].as_str())
            && !is_commit_on_remote(&self.cwd, sha)
        {
            return Some(format!("commit {} not pushed to remote", short(sha)));
        }
        Some("completed without commit (use --no-commit to close manually)".to_string())
    }

    fn subtask_blocking_reason(&self, subtask: &Task) -> String {
        if let Some(sha) = subtask
            .metadata
            .as_ref()
            .and_then(|m| m["commit"]["sha"].as_str())
            && !is_commit_on_remote(&self.cwd, sha)
        {
            return format!("subtask {} commit {} not pushed", subtask.id, short(sha));
        }
        if !subtask.completed {
            return format!("subtask {} not completed", subtask.id);
        }
        format!("subtask {} completed without commit", subtask.id)
    }

    pub fn find_issue_by_task_id(&self, task_id: &str) -> anyhow::Result<Option<u64>> {
        let issues =
            match self
                .client
                .list_issues(&self.repo.owner, &self.repo.repo, &self.label_prefix)
            {
                Ok(issues) => issues,
                Err(_) => return Ok(None),
            };
        Ok(issues
            .iter()
            .filter(|issue| !issue.is_pull_request)
            .find(|issue| extract_task_id(&issue.body).as_deref() == Some(task_id))
            .map(|issue| issue.number))
    }

    fn fetch_all_dex_issues(&self) -> anyhow::Result<HashMap<String, CachedIssue>> {
        let issues =
            self.client
                .list_issues(&self.repo.owner, &self.repo.repo, &self.label_prefix)?;
        Ok(issues
            .into_iter()
            .filter(|issue| !issue.is_pull_request)
            .filter_map(|issue| {
                let task_id = extract_task_id(&issue.body)?;
                Some((task_id, self.cache_entry(issue)))
            })
            .collect())
    }

    fn cache_entry(&self, issue: Issue) -> CachedIssue {
        CachedIssue {
            number: issue.number,
            title: issue.title,
            body: issue.body,
            state: issue.state,
            labels: self.own_labels(&issue.labels),
        }
    }
}

fn is_newer(remote: &str, local: &str) -> bool {
    let parse = |stamp: &str| {
        time::OffsetDateTime::parse(stamp, &time::format_description::well_known::Rfc3339).ok()
    };
    match (parse(remote), parse(local)) {
        (Some(remote), Some(local)) => remote > local,
        _ => false,
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}
