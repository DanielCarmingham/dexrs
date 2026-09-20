use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::config::IntegrationConfig;
use crate::sync::github::body::collect_descendants;
use crate::sync::github::service::{Phase, Progress, SyncResult};
use crate::sync::is_commit_on_remote;
use crate::sync::parse_metadata_comments;
use crate::sync::shortcut::api::{ShortcutClient, Story, WorkflowState};
use crate::sync::shortcut::story::render_story_description;
use crate::task::Task;

pub const DEFAULT_LABEL: &str = "dex";

struct CachedStory {
    id: u64,
    name: String,
    description: String,
    completed: bool,
    labels: Vec<String>,
    workflow_state_id: Option<u64>,
}

pub struct ShortcutSyncService {
    client: ShortcutClient,
    workspace: String,
    team: String,
    workflow: Option<u64>,
    label: String,
    cwd: PathBuf,
    resolved_team: RefCell<Option<String>>,
    resolved_workflow: RefCell<Option<u64>>,
    workflows: RefCell<HashMap<u64, Vec<WorkflowState>>>,
}

pub fn shortcut_token(token_env: Option<&str>) -> Option<String> {
    std::env::var(token_env.unwrap_or("SHORTCUT_API_TOKEN"))
        .ok()
        .filter(|token| !token.is_empty())
}

fn build(
    config: &IntegrationConfig,
    cwd: &Path,
    token: String,
) -> anyhow::Result<ShortcutSyncService> {
    let team = config.team.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "Shortcut team not configured.\nAdd 'team' to [sync.shortcut] section in dex.toml."
        )
    })?;
    let base_url = crate::sync::shortcut::api::api_url();
    let workspace = match &config.workspace {
        Some(workspace) => workspace.clone(),
        None => ShortcutClient::new(base_url.clone(), token.clone()).workspace_slug()?,
    };
    Ok(ShortcutSyncService::new(
        base_url,
        token,
        workspace,
        team,
        config
            .workflow
            .as_deref()
            .and_then(|value| value.parse().ok()),
        config.label.clone(),
        cwd.to_path_buf(),
    ))
}

pub fn service_if_configured(
    config: &IntegrationConfig,
    cwd: &Path,
) -> Option<ShortcutSyncService> {
    if !config.enabled {
        return None;
    }
    let env_name = config.token_env.as_deref().unwrap_or("SHORTCUT_API_TOKEN");
    let Some(token) = shortcut_token(config.token_env.as_deref()) else {
        eprintln!("Shortcut sync enabled but no token found (checked {env_name}). Sync disabled.");
        return None;
    };
    if config.team.is_none() {
        eprintln!("Shortcut sync enabled but no team specified in config. Sync disabled.");
        return None;
    }
    match build(config, cwd, token) {
        Ok(service) => Some(service),
        Err(error) => {
            eprintln!("Failed to fetch Shortcut workspace: {error}. Sync disabled.");
            None
        }
    }
}

pub fn service_or_error(
    config: &IntegrationConfig,
    cwd: &Path,
) -> anyhow::Result<ShortcutSyncService> {
    let env_name = config.token_env.as_deref().unwrap_or("SHORTCUT_API_TOKEN");
    let token = shortcut_token(config.token_env.as_deref()).ok_or_else(|| {
        anyhow::anyhow!("Shortcut API token not found.\nSet the {env_name} environment variable.")
    })?;
    build(config, cwd, token)
}

impl ShortcutSyncService {
    pub fn new(
        base_url: String,
        token: String,
        workspace: String,
        team: String,
        workflow: Option<u64>,
        label: Option<String>,
        cwd: PathBuf,
    ) -> Self {
        Self {
            client: ShortcutClient::new(base_url, token),
            workspace,
            team,
            workflow,
            label: label.unwrap_or_else(|| DEFAULT_LABEL.to_string()),
            cwd,
            resolved_team: RefCell::new(None),
            resolved_workflow: RefCell::new(None),
            workflows: RefCell::new(HashMap::new()),
        }
    }

    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    pub fn remote_id(task: &Task) -> Option<u64> {
        task.metadata.as_ref()?["shortcut"]["storyId"].as_u64()
    }

    pub fn remote_url(&self, task: &Task) -> Option<String> {
        task.metadata.as_ref()?["shortcut"]["storyUrl"]
            .as_str()
            .map(str::to_string)
    }

    fn story_url(&self, id: u64) -> String {
        format!("https://app.shortcut.com/{}/story/{id}", self.workspace)
    }

    pub fn close_remote(&self, task: &Task) -> anyhow::Result<()> {
        let Some(id) = Self::remote_id(task) else {
            return Ok(());
        };
        let workflow = self.workflow_id()?;
        let done = self.state_of_kind(workflow, "done")?;
        self.client
            .update_story(id, json!({ "workflow_state_id": done }))
            .map(|_| ())
    }

    fn team_id(&self) -> anyhow::Result<String> {
        if let Some(id) = self.resolved_team.borrow().clone() {
            return Ok(id);
        }
        let looks_like_uuid =
            self.team.len() == 36 && self.team.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        let id = if looks_like_uuid {
            self.team.clone()
        } else {
            self.client
                .list_teams()?
                .into_iter()
                .find(|team| team.mention_name == self.team || team.name == self.team)
                .map(|team| team.id)
                .ok_or_else(|| anyhow::anyhow!("Team not found: {}", self.team))?
        };
        *self.resolved_team.borrow_mut() = Some(id.clone());
        Ok(id)
    }

    fn workflow_id(&self) -> anyhow::Result<u64> {
        if let Some(id) = self.workflow {
            return Ok(id);
        }
        if let Some(id) = *self.resolved_workflow.borrow() {
            return Ok(id);
        }
        let team = self.client.get_team(&self.team_id()?)?;
        let id = *team
            .workflow_ids
            .first()
            .ok_or_else(|| anyhow::anyhow!("Team {} has no workflows", self.team))?;
        *self.resolved_workflow.borrow_mut() = Some(id);
        Ok(id)
    }

    fn states(&self, workflow: u64) -> anyhow::Result<Vec<WorkflowState>> {
        if let Some(states) = self.workflows.borrow().get(&workflow) {
            return Ok(states.clone());
        }
        let states = self.client.workflow_states(workflow)?;
        self.workflows.borrow_mut().insert(workflow, states.clone());
        Ok(states)
    }

    fn state_of_kind(&self, workflow: u64, kind: &str) -> anyhow::Result<u64> {
        self.states(workflow)?
            .iter()
            .find(|state| state.kind == kind)
            .map(|state| state.id)
            .ok_or_else(|| anyhow::anyhow!("No {kind} state found in workflow {workflow}"))
    }

    fn state_kind(&self, workflow: u64, state_id: u64) -> anyhow::Result<String> {
        Ok(self
            .states(workflow)?
            .iter()
            .find(|state| state.id == state_id)
            .map(|state| state.kind.clone())
            .unwrap_or_else(|| "unstarted".to_string()))
    }

    fn target_state(&self, task: &Task, workflow: u64, completed: bool) -> anyhow::Result<u64> {
        if completed {
            return self.state_of_kind(workflow, "done");
        }
        if (task.started_at.is_some() || task.completed)
            && let Ok(started) = self.state_of_kind(workflow, "started")
        {
            return Ok(started);
        }
        self.state_of_kind(workflow, "unstarted")
    }

    fn expected_kind(task: &Task, completed: bool) -> &'static str {
        if completed {
            "done"
        } else if task.started_at.is_some() || task.completed {
            "started"
        } else {
            "unstarted"
        }
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
        self.client.ensure_label(&self.label)?;
        let cache = self.fetch_all_dex_stories();
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

    fn metadata(&self, id: u64, state: &str) -> Value {
        json!({
            "storyId": id,
            "storyUrl": self.story_url(id),
            "workspace": self.workspace,
            "state": state,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn sync_parent(
        &self,
        parent: &Task,
        tasks: &[Task],
        skip_unchanged: bool,
        cache: Option<&HashMap<String, CachedStory>>,
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
        let subtasks: Vec<&Task> = tasks
            .iter()
            .filter(|task| task.parent_id.as_deref() == Some(parent.id.as_str()))
            .collect();
        let cached = cache.and_then(|cache| cache.get(&parent.id));
        let mut story_id = Self::remote_id(parent).or(cached.map(|cached| cached.id));
        if story_id.is_none() {
            story_id = self.find_story_by_task_id(&parent.id);
        }
        let workflow = self.workflow_id()?;
        let should_complete = self.should_mark_completed(parent, tasks);
        let expected_kind = Self::expected_kind(parent, should_complete);

        let Some(id) = story_id else {
            on_progress(progress(Phase::Creating));
            let metadata = self.create_story(parent, workflow, tasks)?;
            let created_id = metadata["storyId"].as_u64().unwrap_or_default();
            let mut result = SyncResult::new_public(&parent.id, metadata, true);
            result.subtask_results =
                self.sync_subtasks(&subtasks, created_id, tasks, workflow, cache)?;
            self.sync_blockers(parent, created_id, tasks)?;
            return Ok(result);
        };

        let stored_state = parent
            .metadata
            .as_ref()
            .and_then(|m| m["shortcut"]["state"].as_str());
        if skip_unchanged && expected_kind == "done" && stored_state == Some("done") {
            on_progress(progress(Phase::Skipped));
            let mut result =
                SyncResult::new_public(&parent.id, self.metadata(id, expected_kind), false);
            result.skipped = true;
            return Ok(result);
        }

        let mut current_state = cached.and_then(|cached| cached.workflow_state_id);
        let mut parent_skipped = false;
        if skip_unchanged {
            let expected_description = render_story_description(parent);
            let has_changes = match cached {
                Some(cached) => self.story_needs_update(
                    &cached.name,
                    &cached.description,
                    cached.completed,
                    &cached.labels,
                    &parent.name,
                    &expected_description,
                    should_complete,
                ),
                None => match self.client.get_story(id) {
                    Ok(story) => {
                        current_state = story.workflow_state_id;
                        self.story_needs_update(
                            &story.name,
                            &story.description,
                            story.completed,
                            &story.labels,
                            &parent.name,
                            &expected_description,
                            should_complete,
                        )
                    }
                    Err(_) => true,
                },
            };
            parent_skipped = !has_changes;
        } else if cached.is_none() {
            current_state = self
                .client
                .get_story(id)
                .ok()
                .and_then(|story| story.workflow_state_id);
        }

        if parent_skipped {
            on_progress(progress(Phase::Skipped));
        } else {
            on_progress(progress(Phase::Updating));
            self.update_story(parent, id, workflow, tasks, current_state)?;
        }
        let subtask_results = self.sync_subtasks(&subtasks, id, tasks, workflow, cache)?;
        self.sync_blockers(parent, id, tasks)?;
        let mut result =
            SyncResult::new_public(&parent.id, self.metadata(id, expected_kind), false);
        result.skipped = parent_skipped;
        result.subtask_results = subtask_results;
        Ok(result)
    }

    fn sync_subtasks(
        &self,
        subtasks: &[&Task],
        parent_story: u64,
        tasks: &[Task],
        workflow: u64,
        cache: Option<&HashMap<String, CachedStory>>,
    ) -> anyhow::Result<Vec<SyncResult>> {
        let team = self.team_id()?;
        let mut results = Vec::new();
        for subtask in subtasks {
            let cached = cache.and_then(|cache| cache.get(&subtask.id));
            let mut story_id = Self::remote_id(subtask).or(cached.map(|cached| cached.id));
            if story_id.is_none() {
                story_id = self.find_story_by_task_id(&subtask.id);
            }
            let should_complete = self.should_mark_completed(subtask, tasks);
            let mut created = false;
            let id = match story_id {
                Some(id) => {
                    let current_state = match cached.and_then(|cached| cached.workflow_state_id) {
                        Some(state) => Some(state),
                        None => self
                            .client
                            .get_story(id)
                            .ok()
                            .and_then(|story| story.workflow_state_id),
                    };
                    self.update_story(subtask, id, workflow, tasks, current_state)?;
                    id
                }
                None => {
                    let state = self.target_state(subtask, workflow, should_complete)?;
                    let story = self.client.create_story(json!({
                        "name": subtask.name,
                        "description": render_story_description(subtask),
                        "story_type": "chore",
                        "workflow_state_id": state,
                        "labels": [{"name": self.label}],
                        "group_id": team,
                        "parent_story_id": parent_story,
                    }))?;
                    created = true;
                    story.id
                }
            };
            self.sync_blockers(subtask, id, tasks)?;
            let mut result = SyncResult::new_public(
                &subtask.id,
                self.metadata(id, Self::expected_kind(subtask, should_complete)),
                created,
            );
            result.created = created;
            results.push(result);
            let nested: Vec<&Task> = tasks
                .iter()
                .filter(|task| task.parent_id.as_deref() == Some(subtask.id.as_str()))
                .collect();
            if !nested.is_empty() {
                results.extend(self.sync_subtasks(&nested, id, tasks, workflow, cache)?);
            }
        }
        Ok(results)
    }

    fn sync_blockers(&self, task: &Task, story_id: u64, tasks: &[Task]) -> anyhow::Result<()> {
        if task.blocked_by.is_empty() {
            return Ok(());
        }
        let existing: Vec<u64> = self
            .client
            .get_story(story_id)?
            .story_links
            .iter()
            .filter(|link| link.verb == "blocks" && link.object_id == story_id)
            .map(|link| link.subject_id)
            .collect();
        for blocker_id in &task.blocked_by {
            let Some(blocker) = tasks.iter().find(|task| &task.id == blocker_id) else {
                continue;
            };
            let Some(blocker_story) = Self::remote_id(blocker) else {
                continue;
            };
            if existing.contains(&blocker_story) {
                continue;
            }
            if let Err(error) = self.client.create_blocks_link(blocker_story, story_id) {
                eprintln!(
                    "Warning: Failed to create blocker link from story {blocker_story} to {story_id}: {error}"
                );
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn story_needs_update(
        &self,
        name: &str,
        description: &str,
        completed: bool,
        labels: &[String],
        expected_name: &str,
        expected_description: &str,
        should_complete: bool,
    ) -> bool {
        name != expected_name
            || description.trim() != expected_description.trim()
            || completed != should_complete
            || !labels.iter().any(|label| label == &self.label)
    }

    fn create_story(&self, task: &Task, workflow: u64, tasks: &[Task]) -> anyhow::Result<Value> {
        let team = self.team_id()?;
        let should_complete = self.should_mark_completed(task, tasks);
        let state = self.target_state(task, workflow, should_complete)?;
        let story = self.client.create_story(json!({
            "name": task.name,
            "description": render_story_description(task),
            "story_type": "feature",
            "workflow_state_id": state,
            "labels": [{"name": self.label}],
            "group_id": team,
        }))?;
        let kind = self.state_kind(workflow, state)?;
        Ok(self.metadata(story.id, &kind))
    }

    fn update_story(
        &self,
        task: &Task,
        id: u64,
        workflow: u64,
        tasks: &[Task],
        current_state: Option<u64>,
    ) -> anyhow::Result<()> {
        let should_complete = self.should_mark_completed(task, tasks);
        let target = self.target_state(task, workflow, should_complete)?;
        let mut state = Some(target);
        if !should_complete {
            match current_state {
                Some(current) if self.state_kind(workflow, current)? == "done" => state = None,
                Some(_) => {}
                None => state = None,
            }
        }
        let mut fields = json!({
            "name": task.name,
            "description": render_story_description(task),
            "labels": [{"name": self.label}],
        });
        if let Some(state) = state {
            fields["workflow_state_id"] = json!(state);
        }
        self.client.update_story(id, fields).map(|_| ())
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
        !descendants.is_empty()
            && descendants
                .iter()
                .all(|descendant| self.should_mark_completed(&descendant.task, tasks))
    }

    fn find_story_by_task_id(&self, task_id: &str) -> Option<u64> {
        let query = format!(
            "label:\"{}\" description:\"dex:task:id:{task_id}\"",
            self.label
        );
        self.client
            .search_stories(&query)
            .ok()?
            .into_iter()
            .find(|story| {
                parse_metadata_comments(&story.description, "task")
                    .and_then(|metadata| metadata.id)
                    .as_deref()
                    == Some(task_id)
            })
            .map(|story| story.id)
    }

    fn fetch_all_dex_stories(&self) -> HashMap<String, CachedStory> {
        let Ok(stories) = self
            .client
            .search_stories(&format!("label:\"{}\"", self.label))
        else {
            return HashMap::new();
        };
        stories
            .into_iter()
            .filter_map(|story| {
                let task_id = parse_metadata_comments(&story.description, "task")?.id?;
                Some((task_id, Self::cache_entry(story)))
            })
            .collect()
    }

    fn cache_entry(story: Story) -> CachedStory {
        CachedStory {
            id: story.id,
            name: story.name,
            description: story.description,
            completed: story.completed,
            labels: story.labels,
            workflow_state_id: story.workflow_state_id,
        }
    }
}
