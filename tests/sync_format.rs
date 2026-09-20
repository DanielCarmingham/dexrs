use dexrs::sync::github::body::{
    collect_descendants, parse_hierarchical_issue_body, parse_root_task_metadata,
    render_issue_body, render_task_metadata_comments,
};
use dexrs::sync::github::remote::{GitHubRepo, parse_github_url, parse_issue_ref};
use dexrs::sync::shortcut::story::{parse_story_description, render_story_description};
use dexrs::sync::state::{parse_duration_ms, read_sync_state, write_sync_state};
use dexrs::sync::{decode_metadata_value, encode_metadata_value};
use dexrs::task::Task;

fn task(id: &str, name: &str, parent: Option<&str>) -> Task {
    let mut task = Task::new(id.to_string(), name.to_string(), None, None);
    task.parent_id = parent.map(str::to_string);
    task.created_at = Some("2026-01-01T00:00:00.000Z".to_string());
    task.updated_at = Some("2026-01-02T00:00:00.000Z".to_string());
    task
}

#[test]
fn metadata_values_round_trip_through_base64_when_needed() {
    assert_eq!(encode_metadata_value("plain"), "plain");
    assert_eq!(encode_metadata_value("two\nlines"), "base64:dHdvCmxpbmVz");
    assert_eq!(encode_metadata_value("ends -->"), "base64:ZW5kcyAtLT4=");
    assert_eq!(encode_metadata_value("base64:x"), "base64:YmFzZTY0Ong=");
    for value in ["plain", "two\nlines", "ends -->", "base64:x"] {
        assert_eq!(decode_metadata_value(&encode_metadata_value(value)), value);
    }
}

#[test]
fn github_url_and_issue_ref_parsing_match_original() {
    let repo = GitHubRepo {
        owner: "acme".into(),
        repo: "widgets".into(),
    };
    assert_eq!(
        parse_github_url("git@github.com:acme/widgets.git"),
        Some(repo.clone())
    );
    assert_eq!(
        parse_github_url("https://github.com/acme/widgets"),
        Some(repo.clone())
    );
    assert_eq!(
        parse_github_url("https://github.com/acme/widgets.git"),
        Some(repo.clone())
    );
    assert_eq!(parse_github_url("https://gitlab.com/acme/widgets"), None);
    assert_eq!(parse_github_url("nonsense"), None);

    let issue = parse_issue_ref("https://github.com/other/repo/issues/12", Some(&repo)).unwrap();
    assert_eq!(
        (issue.owner.as_str(), issue.repo.as_str(), issue.number),
        ("other", "repo", 12)
    );
    let issue = parse_issue_ref("owner/name#7", None).unwrap();
    assert_eq!(
        (issue.owner.as_str(), issue.repo.as_str(), issue.number),
        ("owner", "name", 7)
    );
    let issue = parse_issue_ref("#42", Some(&repo)).unwrap();
    assert_eq!((issue.owner.as_str(), issue.number), ("acme", 42));
    assert!(parse_issue_ref("42", Some(&repo)).is_some());
    assert!(parse_issue_ref("#42", None).is_none());
    assert!(parse_issue_ref("garbage", Some(&repo)).is_none());
}

#[test]
fn root_metadata_comments_render_and_parse_round_trip() {
    let mut root = task("abc12345", "Root", None);
    root.priority = 2;
    root.completed = true;
    root.started_at = Some("2026-01-01T01:00:00.000Z".into());
    root.completed_at = Some("2026-01-03T00:00:00.000Z".into());
    root.blocked_by = vec!["blk00001".into()];
    root.result = Some("done\nwith newline".into());
    root.metadata = Some(
        serde_json::json!({"commit": {"sha": "deadbeef", "message": "fix: it", "branch": "main", "timestamp": "2026-01-03T00:00:00.000Z"}}),
    );

    let lines = render_task_metadata_comments(&root, "task", None);
    assert_eq!(
        lines,
        vec![
            "<!-- dex:task:id:abc12345 -->",
            "<!-- dex:task:priority:2 -->",
            "<!-- dex:task:completed:true -->",
            "<!-- dex:task:created_at:2026-01-01T00:00:00.000Z -->",
            "<!-- dex:task:updated_at:2026-01-02T00:00:00.000Z -->",
            "<!-- dex:task:started_at:2026-01-01T01:00:00.000Z -->",
            "<!-- dex:task:completed_at:2026-01-03T00:00:00.000Z -->",
            "<!-- dex:task:blockedBy:[\"blk00001\"] -->",
            "<!-- dex:task:blocks:[] -->",
            "<!-- dex:task:result:base64:ZG9uZQp3aXRoIG5ld2xpbmU= -->",
            "<!-- dex:task:commit_sha:deadbeef -->",
            "<!-- dex:task:commit_message:fix: it -->",
            "<!-- dex:task:commit_branch:main -->",
            "<!-- dex:task:commit_timestamp:2026-01-03T00:00:00.000Z -->",
        ]
    );

    let parsed = parse_root_task_metadata(&lines.join("\n")).unwrap();
    assert_eq!(parsed.id.as_deref(), Some("abc12345"));
    assert_eq!(parsed.priority, Some(2));
    assert_eq!(parsed.completed, Some(true));
    assert_eq!(
        parsed.started_at,
        Some(Some("2026-01-01T01:00:00.000Z".into()))
    );
    assert_eq!(
        parsed.completed_at,
        Some(Some("2026-01-03T00:00:00.000Z".into()))
    );
    assert_eq!(parsed.blocked_by, Some(vec!["blk00001".into()]));
    assert_eq!(parsed.result.as_deref(), Some("done\nwith newline"));
    assert_eq!(parsed.commit.as_ref().unwrap()["sha"], "deadbeef");
    assert_eq!(parsed.commit.as_ref().unwrap()["message"], "fix: it");

    assert_eq!(
        parse_root_task_metadata("<!-- dex:task:legacy99 -->")
            .unwrap()
            .id
            .as_deref(),
        Some("legacy99")
    );
    assert!(parse_root_task_metadata("no comments here").is_none());
}

#[test]
fn hierarchical_issue_body_renders_details_blocks_and_parses_back() {
    let root = task("root0001", "Root", None);
    let mut child = task("child001", "Child", Some("root0001"));
    child.description = "child desc".into();
    let mut grandchild = task("grand001", "Grandchild", Some("child001"));
    grandchild.completed = true;
    grandchild.result = Some("gc result".into());
    grandchild.priority = 3;
    let mut other_root = task("other001", "Other", None);
    other_root.priority = 0;
    let tasks = vec![root.clone(), child.clone(), grandchild.clone(), other_root];

    let descendants = collect_descendants(&tasks, "root0001");
    let summary: Vec<(String, usize, String)> = descendants
        .iter()
        .map(|d| (d.task.id.clone(), d.depth, d.parent_id.clone()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("child001".to_string(), 0, "root0001".to_string()),
            ("grand001".to_string(), 1, "child001".to_string()),
        ]
    );

    let body = render_issue_body("root context", &descendants);
    assert!(body.starts_with("root context\n\n## Tasks\n\n<details>\n<summary><b>Child</b></summary>\n\n<!-- dex:subtask:id:child001 -->\n<!-- dex:subtask:parent:root0001 -->\n"), "{body}");
    assert!(body.contains("\n### Description\nchild desc\n\n</details>\n\n<details>\n<summary>✅ └─ <b>Grandchild</b></summary>\n\n<!-- dex:subtask:id:grand001 -->\n<!-- dex:subtask:parent:child001 -->\n"), "{body}");
    assert!(
        body.contains("\n### Result\ngc result\n\n</details>\n"),
        "{body}"
    );
    assert!(body.ends_with("</details>\n"), "{body}");
    assert_eq!(render_issue_body("just context", &[]), "just context");

    let parsed = parse_hierarchical_issue_body(&body);
    assert_eq!(parsed.description, "root context");
    assert_eq!(parsed.subtasks.len(), 2);
    let first = &parsed.subtasks[0];
    assert_eq!(first.task.id, "child001");
    assert_eq!(first.task.name, "Child");
    assert_eq!(first.task.description, "child desc");
    assert_eq!(first.parent_id.as_deref(), Some("root0001"));
    assert!(!first.task.completed);
    let second = &parsed.subtasks[1];
    assert_eq!(second.task.id, "grand001");
    assert!(second.task.completed);
    assert_eq!(second.task.priority, 3);
    assert_eq!(second.task.result.as_deref(), Some("gc result"));
    assert_eq!(second.parent_id.as_deref(), Some("child001"));

    let legacy = "context\n\n## Subtasks\n\n<details>\n<summary>[x] ↳ <b>Old</b> <code>old00001</code></summary>\n<!-- dex:subtask:id:old00001 -->\n<!-- dex:subtask:status:completed -->\n### Context\nold body\n</details>";
    let parsed = parse_hierarchical_issue_body(legacy);
    assert_eq!(parsed.description, "context");
    assert_eq!(parsed.subtasks[0].task.name, "Old");
    assert!(parsed.subtasks[0].task.completed);
    assert_eq!(parsed.subtasks[0].task.description, "old body");
}

#[test]
fn story_description_round_trips_metadata_and_context() {
    let mut task = task("story001", "Story", Some("parent01"));
    task.description = "Do the thing".into();
    task.result = Some("line1\nline2".into());
    task.metadata = Some(serde_json::json!({"commit": {"sha": "abc", "branch": "main"}}));

    let rendered = render_story_description(&task);
    assert_eq!(
        rendered,
        "<!-- dex:task:id:story001 -->\n<!-- dex:task:parent_id:parent01 -->\n<!-- dex:task:priority:1 -->\n<!-- dex:task:completed:false -->\n<!-- dex:task:created_at:2026-01-01T00:00:00.000Z -->\n<!-- dex:task:updated_at:2026-01-02T00:00:00.000Z -->\n<!-- dex:task:completed_at:null -->\n<!-- dex:task:result:base64:bGluZTEKbGluZTI= -->\n<!-- dex:task:commit_sha:abc -->\n<!-- dex:task:commit_branch:main -->\n\nDo the thing"
    );

    let parsed = parse_story_description(&rendered);
    assert_eq!(parsed.context, "Do the thing");
    let metadata = parsed.metadata.unwrap();
    assert_eq!(metadata.id.as_deref(), Some("story001"));
    assert_eq!(metadata.parent_id.as_deref(), Some("parent01"));
    assert_eq!(metadata.result.as_deref(), Some("line1\nline2"));
    assert_eq!(metadata.commit.unwrap()["sha"], "abc");
    assert!(parse_story_description("plain text").metadata.is_none());
}

#[test]
fn sync_state_file_and_duration_parsing() {
    let temp = tempfile::tempdir().unwrap();
    assert_eq!(read_sync_state(temp.path()).last_sync, None);
    write_sync_state(temp.path(), "2026-01-01T00:00:00.000Z").unwrap();
    assert_eq!(
        read_sync_state(temp.path()).last_sync.as_deref(),
        Some("2026-01-01T00:00:00.000Z")
    );
    let raw = std::fs::read_to_string(temp.path().join("sync-state.json")).unwrap();
    assert_eq!(raw, "{\n  \"lastSync\": \"2026-01-01T00:00:00.000Z\"\n}\n");

    assert_eq!(parse_duration_ms("30s"), Some(30_000));
    assert_eq!(parse_duration_ms("30m"), Some(1_800_000));
    assert_eq!(parse_duration_ms("2h"), Some(7_200_000));
    assert_eq!(parse_duration_ms("1d"), Some(86_400_000));
    assert_eq!(parse_duration_ms("1w"), None);
    assert_eq!(parse_duration_ms("abc"), None);
}

#[test]
fn commit_on_remote_falls_back_to_upstream_when_origin_head_is_unset() {
    let temp = tempfile::tempdir().unwrap();
    let bare = temp.path().join("origin.git");
    let work = temp.path().join("work");
    let git = |dir: &std::path::Path, args: &[&str]| {
        let output = std::process::Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare", "-b", "main", "."]);
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main", "."]);
    git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&work, &["commit", "-q", "--allow-empty", "-m", "pushed"]);
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    let pushed = git(&work, &["rev-parse", "HEAD"]);
    let has_origin_head = std::process::Command::new("git")
        .current_dir(&work)
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .output()
        .unwrap()
        .status
        .success();
    assert!(
        !has_origin_head,
        "fresh remote has no origin/HEAD, which is the case under test"
    );

    assert!(dexrs::sync::is_commit_on_remote(&work, &pushed));

    git(
        &work,
        &["commit", "-q", "--allow-empty", "-m", "local only"],
    );
    let local = git(&work, &["rev-parse", "HEAD"]);
    assert!(!dexrs::sync::is_commit_on_remote(&work, &local));
    assert!(!dexrs::sync::is_commit_on_remote(&work, "0000000"));
}
