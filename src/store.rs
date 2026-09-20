use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use fs4::fs_std::FileExt;

use crate::archive::{ArchivedTask, parse_archive_jsonl, serialize_archive_jsonl};
use crate::task::{Task, parse_tasks_jsonl, serialize_tasks_jsonl};
use crate::validate::validate_tasks;

pub struct Resolution<'a> {
    pub cli_storage_path: Option<&'a Path>,
    pub cli_config_path: Option<&'a Path>,
    pub env_storage_path: Option<&'a OsStr>,
}

/// Precedence follows original dex: --storage-path, then storage.file.path
/// from config, then DEX_STORAGE_PATH, then the mode default.
pub fn resolve_store_dir(cwd: &Path, resolution: &Resolution<'_>) -> anyhow::Result<PathBuf> {
    if let Some(path) = resolution.cli_storage_path {
        return Ok(path.to_path_buf());
    }
    let config = crate::config::load(cwd, resolution.cli_config_path)?;
    resolve_with_config(cwd, resolution, &config)
}

pub fn resolve_with_config(
    cwd: &Path,
    resolution: &Resolution<'_>,
    config: &crate::config::Config,
) -> anyhow::Result<PathBuf> {
    if let Some(path) = resolution.cli_storage_path {
        return Ok(path.to_path_buf());
    }
    if let Some(path) = &config.storage_path {
        return Ok(path.clone());
    }
    if let Some(path) = resolution.env_storage_path {
        return Ok(PathBuf::from(path));
    }
    if config.centralized {
        return Ok(crate::config::dex_home()?
            .join("projects")
            .join(project_key(cwd)?));
    }
    if let Some(git_root) = git_root(cwd)? {
        return Ok(git_root.join(".dex"));
    }
    Ok(crate::config::dex_home()?.join("local"))
}

pub fn project_key(cwd: &Path) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to run git from {}", cwd.display()))?;
    let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && !remote.is_empty() {
        return Ok(normalize_git_url(&remote));
    }
    Ok(format!("path-{}", short_hash(&cwd.display().to_string())))
}

fn normalize_git_url(url: &str) -> String {
    let (host, path) = if let Some(rest) = url.strip_prefix("git@") {
        match rest.split_once(':') {
            Some(pair) => pair,
            None => return format!("url-{}", short_hash(url)),
        }
    } else {
        let without_scheme = match url.split_once("://") {
            Some((_, rest)) => rest,
            None => return format!("url-{}", short_hash(url)),
        };
        let without_user = without_scheme
            .rsplit_once('@')
            .map_or(without_scheme, |(_, rest)| rest);
        match without_user.split_once('/') {
            Some(pair) => pair,
            None => return format!("url-{}", short_hash(url)),
        }
    };
    let path = path.trim_start_matches('/').trim_end_matches(".git");
    format!("{host}-{}", path.replace('/', "-"))
}

fn short_hash(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(input.as_bytes());
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .take(6)
        .collect()
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

#[derive(Debug, Default, Clone)]
pub struct WriteOptions {
    pub auto_archive: Option<crate::config::ArchiveConfig>,
}

pub fn transact<F, T>(store_dir: &Path, f: F) -> anyhow::Result<T>
where
    F: FnOnce(&mut Vec<Task>) -> anyhow::Result<T>,
{
    transact_with(store_dir, &WriteOptions::default(), f)
}

pub fn transact_with<F, T>(store_dir: &Path, options: &WriteOptions, f: F) -> anyhow::Result<T>
where
    F: FnOnce(&mut Vec<Task>) -> anyhow::Result<T>,
{
    transact_with_archive(store_dir, options, |tasks, _| f(tasks))
}

/// Like [`transact_with`], but the closure may also append to the archive.
/// Both files are rewritten under the same lock; the archive is written first
/// so a crash between the two writes duplicates a record rather than losing
/// one. Auto-archiving, when configured, runs after the closure like the
/// original's write hook.
pub fn transact_with_archive<F, T>(
    store_dir: &Path,
    options: &WriteOptions,
    f: F,
) -> anyhow::Result<T>
where
    F: FnOnce(&mut Vec<Task>, &mut Vec<ArchivedTask>) -> anyhow::Result<T>,
{
    let _lock = lock_store(store_dir)?;
    let mut tasks = read_tasks(store_dir)?;
    let mut archive = read_archive(store_dir)?;
    let archive_len = archive.len();
    let result = f(&mut tasks, &mut archive)?;
    if let Some(config) = &options.auto_archive {
        let cwd = std::env::current_dir()?;
        let roots = crate::archive::auto_archive(&mut tasks, &mut archive, config, &cwd);
        if !roots.is_empty() {
            append_archive_log(store_dir, &roots);
        }
    }
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

fn append_archive_log(store_dir: &Path, roots: &[(String, String)]) {
    let stamp = crate::task::timestamp();
    let lines: String = roots
        .iter()
        .map(|(id, name)| format!("{stamp} AUTO-ARCHIVED {id}: {name}\n"))
        .collect();
    if let Ok(mut log) = OpenOptions::new()
        .append(true)
        .create(true)
        .open(store_dir.join("archive.log"))
    {
        let _ = log.write_all(lines.as_bytes());
    }
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

pub fn git_root(cwd: &Path) -> anyhow::Result<Option<PathBuf>> {
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
