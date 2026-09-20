use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use toml::Value;

use crate::store::git_root;

pub const DEFAULT_CONFIG: &str = r#"# dex configuration file

[storage]
engine = "file"

# File storage settings
[storage.file]
# path = "/custom/path"  # Uncomment to set custom storage path

# GitHub sync (optional - sync tasks with GitHub Issues)
# Note: owner/repo are automatically inferred from your git remote
# [sync.github]
# enabled = true
# token_env = "GITHUB_TOKEN"    # Environment variable containing GitHub token (or use gh CLI)
# label_prefix = "dex"           # Prefix for dex-related labels

# Shortcut sync (optional - sync tasks with Shortcut Stories)
# [sync.shortcut]
# enabled = true
# token_env = "SHORTCUT_API_TOKEN"  # Environment variable containing Shortcut API token
# team = "engineering"               # Team mention name or UUID (required)
# workspace = "mycompany"            # Workspace slug (auto-detected if not set)
# label = "dex"                      # Label for dex stories
"#;

pub struct KeySpec {
    pub key: &'static str,
    pub kind: Kind,
}

pub enum Kind {
    String,
    Bool,
    Number,
    Enum(&'static [&'static str]),
}

pub const SCHEMA: &[KeySpec] = &[
    KeySpec {
        key: "storage.engine",
        kind: Kind::Enum(&["file"]),
    },
    KeySpec {
        key: "storage.file.path",
        kind: Kind::String,
    },
    KeySpec {
        key: "storage.file.mode",
        kind: Kind::Enum(&["in-repo", "centralized"]),
    },
    KeySpec {
        key: "sync.github.enabled",
        kind: Kind::Bool,
    },
    KeySpec {
        key: "sync.github.token_env",
        kind: Kind::String,
    },
    KeySpec {
        key: "sync.github.label_prefix",
        kind: Kind::String,
    },
    KeySpec {
        key: "sync.github.auto.on_change",
        kind: Kind::Bool,
    },
    KeySpec {
        key: "sync.github.auto.max_age",
        kind: Kind::String,
    },
    KeySpec {
        key: "sync.shortcut.enabled",
        kind: Kind::Bool,
    },
    KeySpec {
        key: "sync.shortcut.token_env",
        kind: Kind::String,
    },
    KeySpec {
        key: "sync.shortcut.team",
        kind: Kind::String,
    },
    KeySpec {
        key: "sync.shortcut.workspace",
        kind: Kind::String,
    },
    KeySpec {
        key: "sync.shortcut.workflow",
        kind: Kind::String,
    },
    KeySpec {
        key: "sync.shortcut.label",
        kind: Kind::String,
    },
    KeySpec {
        key: "sync.shortcut.auto.on_change",
        kind: Kind::Bool,
    },
    KeySpec {
        key: "sync.shortcut.auto.max_age",
        kind: Kind::String,
    },
    KeySpec {
        key: "archive.auto",
        kind: Kind::Bool,
    },
    KeySpec {
        key: "archive.age_days",
        kind: Kind::Number,
    },
    KeySpec {
        key: "archive.keep_recent",
        kind: Kind::Number,
    },
];

pub fn dex_home() -> anyhow::Result<PathBuf> {
    if let Some(home) = std::env::var_os("DEX_HOME") {
        return Ok(PathBuf::from(home));
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("dex"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/dex"))
}

pub fn global_config_path() -> anyhow::Result<PathBuf> {
    Ok(dex_home()?.join("dex.toml"))
}

pub fn project_config_path(cwd: &Path) -> anyhow::Result<Option<PathBuf>> {
    Ok(git_root(cwd)?.map(|root| root.join(".dex/config.toml")))
}

pub struct Config {
    pub engine: String,
    pub storage_path: Option<PathBuf>,
    pub centralized: bool,
    pub github: IntegrationConfig,
    pub shortcut: IntegrationConfig,
    pub archive: ArchiveConfig,
}

#[derive(Debug, Clone, Default)]
pub struct IntegrationConfig {
    pub enabled: bool,
    pub token_env: Option<String>,
    pub label_prefix: Option<String>,
    pub label: Option<String>,
    pub team: Option<String>,
    pub workspace: Option<String>,
    pub workflow: Option<String>,
    /// None means the key is absent, which original dex treats as true.
    pub auto_on_change: Option<bool>,
    pub auto_max_age: Option<String>,
}

impl IntegrationConfig {
    pub fn syncs_on_change(&self) -> bool {
        self.auto_on_change != Some(false)
    }
}

#[derive(Debug, Clone)]
pub struct ArchiveConfig {
    pub auto: bool,
    pub age_days: i64,
    pub keep_recent: usize,
}

impl Default for ArchiveConfig {
    fn default() -> Self {
        Self {
            auto: false,
            age_days: 90,
            keep_recent: 50,
        }
    }
}

pub fn load(cwd: &Path, config_path: Option<&Path>) -> anyhow::Result<Config> {
    let mut merged = Value::Table(Default::default());
    let global = match config_path {
        Some(path) => path.to_path_buf(),
        None => global_config_path()?,
    };
    merge(&mut merged, read_file(&global)?);
    if let Some(project) = project_config_path(cwd)? {
        merge(&mut merged, read_file(&project)?);
    }

    let engine = lookup(&merged, "storage.engine")
        .and_then(Value::as_str)
        .unwrap_or("file")
        .to_string();
    if engine != "file" {
        bail!(
            "Unsupported storage engine: {engine}.\nOnly \"file\" storage is supported. Use sync.github for GitHub integration."
        );
    }
    let text = |key: &str| {
        lookup(&merged, key)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let flag = |key: &str| lookup(&merged, key).and_then(Value::as_bool);
    let number = |key: &str| lookup(&merged, key).and_then(Value::as_integer);
    let integration = |name: &str| IntegrationConfig {
        enabled: flag(&format!("sync.{name}.enabled")).unwrap_or(false),
        token_env: text(&format!("sync.{name}.token_env")),
        label_prefix: text(&format!("sync.{name}.label_prefix")),
        label: text(&format!("sync.{name}.label")),
        team: text(&format!("sync.{name}.team")),
        workspace: text(&format!("sync.{name}.workspace")),
        workflow: lookup(&merged, &format!("sync.{name}.workflow")).map(|value| match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        }),
        auto_on_change: flag(&format!("sync.{name}.auto.on_change")),
        auto_max_age: text(&format!("sync.{name}.auto.max_age")),
    };
    let defaults = ArchiveConfig::default();
    Ok(Config {
        engine,
        storage_path: text("storage.file.path").map(PathBuf::from),
        centralized: text("storage.file.mode").as_deref() == Some("centralized"),
        github: integration("github"),
        shortcut: integration("shortcut"),
        archive: ArchiveConfig {
            auto: flag("archive.auto").unwrap_or(defaults.auto),
            age_days: number("archive.age_days").unwrap_or(defaults.age_days),
            keep_recent: number("archive.keep_recent")
                .map(|value| value.max(0) as usize)
                .unwrap_or(defaults.keep_recent),
        },
    })
}

pub fn read_file(path: &Path) -> anyhow::Result<Value> {
    match fs::read_to_string(path) {
        Ok(contents) => contents
            .parse::<toml::Table>()
            .map(Value::Table)
            .with_context(|| format!("Failed to parse config file at {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(Value::Table(Default::default()))
        }
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

pub fn write_file(path: &Path, value: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("config root must be a table"))?;
    fs::write(path, toml::to_string(table)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

pub fn spec(key: &str) -> anyhow::Result<&'static KeySpec> {
    SCHEMA.iter().find(|spec| spec.key == key).ok_or_else(|| {
        anyhow!("Unknown config key: {key}\nRun dex config --help for available keys.")
    })
}

pub fn parse_value(spec: &KeySpec, raw: &str) -> anyhow::Result<Value> {
    match spec.kind {
        Kind::Bool => match raw {
            "true" | "1" | "yes" => Ok(Value::Boolean(true)),
            "false" | "0" | "no" => Ok(Value::Boolean(false)),
            _ => bail!("Invalid boolean value: \"{raw}\". Use true/false, 1/0, or yes/no."),
        },
        Kind::Number => raw
            .parse::<i64>()
            .map(Value::Integer)
            .map_err(|_| anyhow!("Invalid number value: \"{raw}\"")),
        Kind::Enum(options) if !options.contains(&raw) => bail!(
            "Invalid value \"{raw}\" for {}. Valid options: {}",
            spec.key,
            options.join(", ")
        ),
        Kind::Enum(_) | Kind::String => Ok(Value::String(raw.to_string())),
    }
}

pub fn format_value(value: Option<&Value>) -> String {
    match value {
        None => "(not set)".to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

pub fn lookup<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.')
        .try_fold(root, |current, part| current.get(part))
}

pub fn set(root: &mut Value, key: &str, value: Value) {
    let mut current = root;
    let parts: Vec<&str> = key.split('.').collect();
    for part in &parts[..parts.len() - 1] {
        let table = current
            .as_table_mut()
            .expect("config root and intermediate nodes are tables");
        current = table
            .entry(part.to_string())
            .or_insert_with(|| Value::Table(Default::default()));
        if !current.is_table() {
            *current = Value::Table(Default::default());
        }
    }
    current
        .as_table_mut()
        .expect("parent node is a table")
        .insert(parts[parts.len() - 1].to_string(), value);
}

pub fn unset(root: &mut Value, key: &str) -> bool {
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = root;
    for part in &parts[..parts.len() - 1] {
        match current.get_mut(part) {
            Some(next) => current = next,
            None => return false,
        }
    }
    current
        .as_table_mut()
        .and_then(|table| table.remove(parts[parts.len() - 1]))
        .is_some()
}

fn merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Table(base), Value::Table(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) if existing.is_table() && value.is_table() => {
                        merge(existing, value)
                    }
                    _ => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}
