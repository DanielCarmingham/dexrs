use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::task::timestamp;

pub fn commit_metadata(cwd: &Path, reference: &str) -> anyhow::Result<Value> {
    let sha = match git(
        cwd,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )? {
        Some(sha) => sha,
        None => bail!(
            "commit {reference} not found in local repository\n  \
             Verify the SHA exists with: git rev-parse --verify {reference}"
        ),
    };
    let message = git(cwd, &["log", "-1", "--format=%s", &sha])?.unwrap_or_default();
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?.unwrap_or_default();

    Ok(json!({
        "sha": sha,
        "message": message,
        "branch": branch,
        "timestamp": timestamp(),
    }))
}

fn git(cwd: &Path, args: &[&str]) -> anyhow::Result<Option<String>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to run git from {}", cwd.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8(output.stdout).context("git returned non-utf8 output")?;
    Ok(Some(stdout.trim().to_string()))
}
