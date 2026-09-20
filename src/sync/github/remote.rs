use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubRepo {
    pub owner: String,
    pub repo: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

pub fn git_remote_url(cwd: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!url.is_empty()).then_some(url)
}

pub fn github_repo(cwd: &Path) -> Option<GitHubRepo> {
    parse_github_url(&git_remote_url(cwd)?)
}

pub fn parse_github_url(url: &str) -> Option<GitHubRepo> {
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let rest = rest.trim_end_matches(".git");
        let (owner, repo) = rest.split_once('/')?;
        if owner.is_empty() || repo.is_empty() {
            return None;
        }
        return Some(GitHubRepo {
            owner: owner.into(),
            repo: repo.into(),
        });
    }
    let (_, rest) = url.split_once("://")?;
    let (host, path) = rest.split_once('/')?;
    if host != "github.com" {
        return None;
    }
    let path = path.trim_end_matches(".git");
    let mut parts = path.split('/');
    let owner = parts.next().filter(|part| !part.is_empty())?;
    let repo = parts.next().filter(|part| !part.is_empty())?;
    Some(GitHubRepo {
        owner: owner.into(),
        repo: repo.into(),
    })
}

pub fn parse_issue_ref(reference: &str, default_repo: Option<&GitHubRepo>) -> Option<IssueRef> {
    let url =
        regex::Regex::new(r"^https?://github\.com/([^/]+)/([^/]+)/issues/(\d+)").expect("static");
    if let Some(capture) = url.captures(reference) {
        return Some(IssueRef {
            owner: capture[1].into(),
            repo: capture[2].into(),
            number: capture[3].parse().ok()?,
        });
    }
    let short = regex::Regex::new(r"^([^/]+)/([^#]+)#(\d+)$").expect("static");
    if let Some(capture) = short.captures(reference) {
        return Some(IssueRef {
            owner: capture[1].into(),
            repo: capture[2].into(),
            number: capture[3].parse().ok()?,
        });
    }
    let number = regex::Regex::new(r"^#?(\d+)$").expect("static");
    let capture = number.captures(reference)?;
    let repo = default_repo?;
    Some(IssueRef {
        owner: repo.owner.clone(),
        repo: repo.repo.clone(),
        number: capture[1].parse().ok()?,
    })
}
