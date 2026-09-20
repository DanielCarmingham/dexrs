//! Model Context Protocol server over stdio: newline-delimited JSON-RPC 2.0
//! exposing the same three tools as the original dex.

use std::io::{BufRead, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::config::Config;
use crate::listing::{self, ListFilter};
use crate::service::{self, CreateInput, UpdateInput};
use crate::store::{self, WriteOptions};
use crate::sync::registry;
use crate::task::timestamp;

const PROTOCOL_VERSION: &str = "2024-11-05";
const MAX_CONTENT_LENGTH: usize = 50 * 1024;
const SHA_PATTERN: &str = "^[0-9a-f]{7,40}$";

pub struct Server<'a> {
    pub store_dir: &'a Path,
    pub cwd: &'a Path,
    pub config: &'a Config,
    pub write_options: &'a WriteOptions,
}

pub fn help_text(invoked_as: &str) -> String {
    format!(
        "{invoked_as} mcp - Start MCP (Model Context Protocol) server\n\n\
         USAGE:\n  {invoked_as} mcp [options]\n\n\
         OPTIONS:\n  --config <path>            Use custom config file\n  \
         --storage-path <path>      Override storage file location\n  \
         -h, --help                 Show this help message\n\n\
         DESCRIPTION:\n  Starts the MCP server over stdio for integration with AI assistants.\n  \
         The server exposes task management tools that can be called by MCP clients.\n\n\
         EXAMPLE:\n  {invoked_as} mcp                    # Start MCP server with default storage\n  \
         {invoked_as} mcp --config ./test.toml\n  {invoked_as} mcp --storage-path ~/.dex/tasks\n"
    )
}

impl Server<'_> {
    pub fn serve<R: BufRead, W: Write>(&self, input: R, mut output: W) -> anyhow::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let message: Value = match serde_json::from_str(&line) {
                Ok(message) => message,
                Err(error) => {
                    let reply = json!({"jsonrpc": "2.0", "id": Value::Null, "error": {"code": -32700, "message": format!("Parse error: {error}")}});
                    writeln!(output, "{reply}")?;
                    output.flush()?;
                    continue;
                }
            };
            let id = message.get("id").cloned();
            let method = message["method"].as_str().unwrap_or_default().to_string();
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let Some(id) = id else {
                continue;
            };
            let reply = match self.dispatch(&method, params) {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                Err((code, text)) => {
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": text}})
                }
            };
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
        Ok(())
    }

    fn dispatch(&self, method: &str, params: Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "dex", "version": "1.0.0"},
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tool_definitions()})),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default();
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                Ok(self.call_tool(name, arguments))
            }
            _ => Err((-32601, format!("Method not found: {method}"))),
        }
    }

    fn call_tool(&self, name: &str, arguments: Value) -> Value {
        let outcome = match name {
            "create_task" => self.create_task(arguments),
            "update_task" => self.update_task(arguments),
            "list_tasks" => self.list_tasks(arguments),
            other => Err(anyhow::anyhow!("Unknown tool: {other}")),
        };
        match outcome {
            Ok(data) => json!({"content": [{"type": "text", "text": pretty(&data)}]}),
            Err(error) => {
                let text = format!("{error:#}");
                let (message, suggestion) = match text.split_once("\nHint: ") {
                    Some((message, hint)) => (message.to_string(), Some(hint.to_string())),
                    None => (text, None),
                };
                let mut body = json!({"error": message});
                if let Some(suggestion) = suggestion {
                    body["suggestion"] = json!(suggestion);
                }
                json!({"content": [{"type": "text", "text": pretty(&body)}], "isError": true})
            }
        }
    }

    fn create_task(&self, args: Value) -> anyhow::Result<Value> {
        let mut errors = Vec::new();
        let name = required_text(&args, "name", &mut errors);
        let description = required_text(&args, "description", &mut errors);
        let parent_id = optional_text(&args, "parent_id", &mut errors);
        let priority = optional_priority(&args, &mut errors);
        let blocked_by = optional_id_list(&args, "blocked_by", &mut errors);
        validation(errors)?;
        let task = store::transact_with(self.store_dir, self.write_options, |tasks| {
            service::create(
                tasks,
                CreateInput {
                    name: name.unwrap_or_default(),
                    description,
                    parent_id,
                    priority,
                    blocked_by,
                    ..CreateInput::default()
                },
            )
        })?;
        registry::auto_sync(
            self.store_dir,
            self.write_options,
            self.config,
            self.cwd,
            &task.id,
        );
        let tasks = store::read_tasks(self.store_dir)?;
        let task = tasks
            .into_iter()
            .find(|candidate| candidate.id == task.id)
            .unwrap_or(task);
        Ok(serde_json::to_value(task)?)
    }

    fn update_task(&self, args: Value) -> anyhow::Result<Value> {
        let mut errors = Vec::new();
        let id = required_text(&args, "id", &mut errors).unwrap_or_default();
        let name = optional_text(&args, "name", &mut errors);
        let description = optional_text(&args, "description", &mut errors);
        let parent_id = nullable_text(&args, "parent_id", &mut errors);
        let priority = optional_priority(&args, &mut errors);
        let completed = optional_bool(&args, "completed", &mut errors);
        let started_at = nullable_text(&args, "started_at", &mut errors);
        if let Some(Some(stamp)) = &started_at
            && time::OffsetDateTime::parse(stamp, &time::format_description::well_known::Rfc3339)
                .is_err()
        {
            errors.push("started_at: Invalid datetime".to_string());
        }
        let result = optional_text(&args, "result", &mut errors);
        let commit_sha = optional_text(&args, "commit_sha", &mut errors);
        if let Some(sha) = &commit_sha
            && !regex::Regex::new(SHA_PATTERN)
                .expect("static")
                .is_match(&sha.to_ascii_lowercase())
        {
            errors.push(
                "commit_sha: Invalid git SHA format (expected 7-40 hex characters)".to_string(),
            );
        }
        let commit_message = optional_text(&args, "commit_message", &mut errors);
        let commit_branch = optional_text(&args, "commit_branch", &mut errors);
        let commit_url = optional_text(&args, "commit_url", &mut errors);
        if let Some(url) = &commit_url
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            errors.push("commit_url: Invalid url".to_string());
        }
        let delete = optional_bool(&args, "delete", &mut errors);
        let add_blocked_by = optional_id_list(&args, "add_blocked_by", &mut errors);
        let remove_blocked_by = optional_id_list(&args, "remove_blocked_by", &mut errors);
        validation(errors)?;

        if delete == Some(true) {
            let removed = store::transact_with(self.store_dir, self.write_options, |tasks| {
                service::delete(tasks, &id)
            })?;
            registry::close_remotes(self.config, self.cwd, &removed);
            let task = removed.into_iter().find(|task| task.id == id);
            return Ok(json!({"deleted": true, "id": id, "task": task}));
        }

        let metadata = commit_sha.map(|sha| {
            let mut commit = json!({"sha": sha, "timestamp": timestamp()});
            if let Some(message) = commit_message {
                commit["message"] = json!(message);
            }
            if let Some(branch) = commit_branch {
                commit["branch"] = json!(branch);
            }
            if let Some(url) = commit_url {
                commit["url"] = json!(url);
            }
            Some(json!({"commit": commit}))
        });
        let task = store::transact_with(self.store_dir, self.write_options, |tasks| {
            service::update(
                tasks,
                UpdateInput {
                    id: id.clone(),
                    name,
                    description,
                    parent_id,
                    priority,
                    completed,
                    result: result.map(Some),
                    started_at,
                    metadata,
                    add_blocked_by,
                    remove_blocked_by,
                    ..UpdateInput::default()
                },
            )
        })?;
        registry::auto_sync(
            self.store_dir,
            self.write_options,
            self.config,
            self.cwd,
            &id,
        );
        let tasks = store::read_tasks(self.store_dir)?;
        let task = tasks
            .into_iter()
            .find(|candidate| candidate.id == id)
            .unwrap_or(task);
        Ok(serde_json::to_value(task)?)
    }

    fn list_tasks(&self, args: Value) -> anyhow::Result<Value> {
        let mut errors = Vec::new();
        let completed = optional_bool(&args, "completed", &mut errors);
        let query = optional_text(&args, "query", &mut errors);
        let all = optional_bool(&args, "all", &mut errors);
        let blocked = optional_bool(&args, "blocked", &mut errors);
        let ready = optional_bool(&args, "ready", &mut errors);
        validation(errors)?;
        let tasks = store::read_tasks(self.store_dir)?;
        let filter = ListFilter {
            all: all == Some(true),
            completed: completed == Some(true),
            blocked: blocked == Some(true),
            ready: ready == Some(true),
            query,
            ..ListFilter::default()
        };
        let selected: Vec<_> = listing::select(&tasks, &filter)
            .into_iter()
            .cloned()
            .collect();
        Ok(serde_json::to_value(selected)?)
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn validation(errors: Vec<String>) -> anyhow::Result<()> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Validation error: {}", errors.join(", ")))
    }
}

fn required_text(args: &Value, key: &str, errors: &mut Vec<String>) -> Option<String> {
    match args.get(key) {
        None | Some(Value::Null) => {
            errors.push(format!("{key}: Required"));
            None
        }
        Some(_) => optional_text(args, key, errors),
    }
}

fn optional_text(args: &Value, key: &str, errors: &mut Vec<String>) -> Option<String> {
    match args.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => {
            if text.is_empty() {
                errors.push(format!(
                    "{key}: String must contain at least 1 character(s)"
                ));
                None
            } else if text.chars().count() > MAX_CONTENT_LENGTH {
                errors.push(format!(
                    "{key}: String must contain at most {MAX_CONTENT_LENGTH} character(s)"
                ));
                None
            } else {
                Some(text.clone())
            }
        }
        Some(_) => {
            errors.push(format!("{key}: Expected string"));
            None
        }
    }
}

fn nullable_text(args: &Value, key: &str, errors: &mut Vec<String>) -> Option<Option<String>> {
    match args.get(key) {
        None => None,
        Some(Value::Null) => Some(None),
        Some(_) => optional_text(args, key, errors).map(Some),
    }
}

fn optional_bool(args: &Value, key: &str, errors: &mut Vec<String>) -> Option<bool> {
    match args.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::Bool(flag)) => Some(*flag),
        Some(_) => {
            errors.push(format!("{key}: Expected boolean"));
            None
        }
    }
}

fn optional_priority(args: &Value, errors: &mut Vec<String>) -> Option<i64> {
    match args.get("priority") {
        None | Some(Value::Null) => None,
        Some(Value::Number(number)) => match number.as_i64() {
            Some(value) if (0..=100).contains(&value) => Some(value),
            _ => {
                errors.push("priority: Expected integer between 0 and 100".to_string());
                None
            }
        },
        Some(_) => {
            errors.push("priority: Expected number".to_string());
            None
        }
    }
}

fn optional_id_list(args: &Value, key: &str, errors: &mut Vec<String>) -> Vec<String> {
    match args.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item.as_str() {
                Some(text) if !text.is_empty() => Some(text.to_string()),
                _ => {
                    errors.push(format!("{key}: Expected non-empty string ids"));
                    None
                }
            })
            .collect(),
        Some(_) => {
            errors.push(format!("{key}: Expected array"));
            Vec::new()
        }
    }
}

fn tool_definitions() -> Vec<Value> {
    let text = |description: &str| json!({"type": "string", "minLength": 1, "maxLength": MAX_CONTENT_LENGTH, "description": description});
    let id_list = |description: &str| json!({"type": "array", "items": {"type": "string", "minLength": 1}, "description": description});
    vec![
        json!({
            "name": "create_task",
            "description": "Create a task ticket with comprehensive context like a GitHub Issue. Explain what needs to be done and why, the requirements and constraints, your implementation approach, and how you'll know it's complete. Use this for complex work that needs coordination across sessions or when context should persist. Break large work into subtasks for better tracking.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": text("One-line summary (like GitHub Issue title). Action-oriented, specific. Example: 'Add JWT authentication to API endpoints'"),
                    "description": text("Comprehensive description like a GitHub Issue body. Explain what needs to be done and why, the specific requirements and constraints, the implementation approach with steps and technical choices, how you'll know it's done, and any relevant files or dependencies. Write naturally - agents and humans should understand the full picture without asking questions."),
                    "parent_id": {"type": "string", "minLength": 1, "description": "Parent task ID to create as child. Supports 3-level hierarchy: epic (L0) → task (L1) → subtask (L2). Cannot create children of subtasks (max depth enforced)."},
                    "priority": {"type": "integer", "minimum": 0, "maximum": 100, "description": "Priority level - lower number = higher priority (default: 1, max: 100)"},
                    "blocked_by": id_list("Array of task IDs that must be completed before this task can be started. Creates bidirectional blocking relationships."),
                },
                "required": ["name", "description"],
            },
        }),
        json!({
            "name": "update_task",
            "description": "Update task fields, mark complete with result, or delete. When completing, provide comprehensive result: what was implemented, key decisions made, trade-offs considered, any follow-ups needed. Think PR description: explain the resolution at a high level without reading code.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "minLength": 1, "description": "Task ID"},
                    "name": text("Updated name"),
                    "description": text("Updated description"),
                    "parent_id": {"type": ["string", "null"], "minLength": 1, "description": "Parent task ID (null to remove parent)"},
                    "priority": {"type": "integer", "minimum": 0, "maximum": 100, "description": "Updated priority"},
                    "completed": {"type": "boolean", "description": "Mark task as completed (true) or pending (false)"},
                    "started_at": {"type": ["string", "null"], "format": "date-time", "description": "Timestamp when task was started (ISO 8601). Set to mark as in-progress, null to clear."},
                    "result": {"type": "string", "maxLength": MAX_CONTENT_LENGTH, "description": "Implementation summary like a PR description. Explain what was implemented and how the solution works, key decisions made and their rationale, trade-offs or alternatives you considered, and any follow-up work or tech debt. Write naturally so anyone can understand the solution without reading code."},
                    "commit_sha": {"type": "string", "pattern": SHA_PATTERN, "description": "Git commit SHA that implements this task"},
                    "commit_message": {"type": "string", "description": "Commit message"},
                    "commit_branch": {"type": "string", "description": "Branch name where commit was made"},
                    "commit_url": {"type": "string", "format": "uri", "description": "URL to the commit (e.g., GitHub commit URL)"},
                    "delete": {"type": "boolean", "description": "Set to true to delete the task"},
                    "add_blocked_by": id_list("Array of task IDs to add as blockers. These tasks must be completed before this one can start."),
                    "remove_blocked_by": id_list("Array of task IDs to remove as blockers."),
                },
                "required": ["id"],
            },
        }),
        json!({
            "name": "list_tasks",
            "description": "List and search tasks. Use to review context, find related work, or understand current state. Filter by status, search content. By default shows only pending tasks.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "completed": {"type": "boolean", "description": "Filter by completion status (true = completed, false = pending)"},
                    "query": {"type": "string", "description": "Search in name and description"},
                    "all": {"type": "boolean", "description": "Show all tasks (pending and completed)"},
                    "blocked": {"type": "boolean", "description": "Filter to only blocked tasks (tasks with incomplete blockers)"},
                    "ready": {"type": "boolean", "description": "Filter to only ready tasks (pending tasks with all blockers completed)"},
                },
            },
        }),
    ]
}
