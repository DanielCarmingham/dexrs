use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, anyhow};
use fs4::fs_std::FileExt;

use crate::archive::{ArchivedTask, parse_archive_jsonl, serialize_archive_jsonl};
use crate::task::{Task, parse_tasks_jsonl, serialize_tasks_jsonl};
use crate::validate::validate_tasks;

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

pub fn read_tasks(store_dir: &Path) -> anyhow::Result<Vec<Task>> {
    let task_file = store_dir.join("tasks.jsonl");
    match fs::read_to_string(&task_file) {
        Ok(contents) => parse_tasks_jsonl(&contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read task file {}", task_file.display()))
        }
    }
}

pub fn read_archive(store_dir: &Path) -> anyhow::Result<Vec<ArchivedTask>> {
    let archive_file = store_dir.join("archive.jsonl");
    match fs::read_to_string(&archive_file) {
        Ok(contents) => parse_archive_jsonl(&contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to read archive file {}", archive_file.display())),
    }
}

pub fn transact<F, T>(store_dir: &Path, f: F) -> anyhow::Result<T>
where
    F: FnOnce(&mut Vec<Task>) -> anyhow::Result<T>,
{
    let _lock = lock_store(store_dir)?;
    let mut tasks = read_tasks(store_dir)?;
    let result = f(&mut tasks)?;
    validate_tasks(&tasks)?;
    write_atomic(store_dir, "tasks.jsonl", &serialize_tasks_jsonl(&tasks)?)?;
    Ok(result)
}

/// Like [`transact`], but the closure may also append to the archive. Both
/// files are rewritten under the same lock; the archive is written first so a
/// crash between the two writes duplicates a record rather than losing one.
pub fn transact_with_archive<F, T>(store_dir: &Path, f: F) -> anyhow::Result<T>
where
    F: FnOnce(&mut Vec<Task>, &mut Vec<ArchivedTask>) -> anyhow::Result<T>,
{
    let _lock = lock_store(store_dir)?;
    let mut tasks = read_tasks(store_dir)?;
    let mut archive = read_archive(store_dir)?;
    let archive_len = archive.len();
    let result = f(&mut tasks, &mut archive)?;
    validate_tasks(&tasks)?;
    if archive.len() != archive_len {
        write_atomic(
            store_dir,
            "archive.jsonl",
            &serialize_archive_jsonl(&archive)?,
        )?;
    }
    write_atomic(store_dir, "tasks.jsonl", &serialize_tasks_jsonl(&tasks)?)?;
    Ok(result)
}

fn lock_store(store_dir: &Path) -> anyhow::Result<File> {
    fs::create_dir_all(store_dir)
        .with_context(|| format!("failed to create store directory {}", store_dir.display()))?;

    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(store_dir.join("tasks.lock"))
        .with_context(|| format!("failed to open lock file in {}", store_dir.display()))?;
    lock_file.lock_exclusive()?;
    Ok(lock_file)
}

fn write_atomic(store_dir: &Path, file_name: &str, payload: &str) -> anyhow::Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(store_dir)
        .with_context(|| format!("failed to create temp file in {}", store_dir.display()))?;
    temp.write_all(payload.as_bytes())?;
    temp.as_file().sync_all()?;

    let target = store_dir.join(file_name);
    temp.persist(&target)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to replace {}", target.display()))?;

    sync_directory(store_dir);
    Ok(())
}

fn sync_directory(store_dir: &Path) {
    if let Ok(directory) = File::open(store_dir) {
        let _ = directory.sync_all();
    }
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
