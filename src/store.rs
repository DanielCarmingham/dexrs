use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, anyhow};

pub fn resolve_store_dir(cwd: &Path, env_path: Option<&OsStr>) -> anyhow::Result<PathBuf> {
    if let Some(path) = env_path {
        return Ok(PathBuf::from(path));
    }

    if let Some(git_root) = git_root(cwd)? {
        return Ok(git_root.join(".dex"));
    }

    home_config_fallback()
}

pub fn init_store(store_dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(store_dir)
        .with_context(|| format!("failed to create store directory {}", store_dir.display()))?;

    let task_file = store_dir.join("tasks.jsonl");
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&task_file)
        .with_context(|| format!("failed to create task file {}", task_file.display()))?;

    Ok(())
}

fn git_root(cwd: &Path) -> anyhow::Result<Option<PathBuf>> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to run git from {}", cwd.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout =
        String::from_utf8(output.stdout).context("git returned non-utf8 repository path")?;
    let path = stdout.trim();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(path)))
    }
}

fn home_config_fallback() -> anyhow::Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/dex/local"))
}
