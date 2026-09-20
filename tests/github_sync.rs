use dexrs::sync::github::remote::GitHubRepo;
use dexrs::sync::github::service::{GitHubSyncService, Phase};
use dexrs::task::Task;
use mockito::{Matcher, Server, ServerGuard};

fn task(id: &str, name: &str, parent: Option<&str>) -> Task {
    let mut task = Task::new(id.to_string(), name.to_string(), None, None);
    task.parent_id = parent.map(str::to_string);
    task.created_at = Some("2026-01-01T00:00:00.000Z".into());
    task.updated_at = Some("2026-01-02T00:00:00.000Z".into());
    task
}

fn link(parent: &mut Task, child: &Task) {
    parent.children.push(child.id.clone());
}

fn service(server: &ServerGuard, cwd: &std::path::Path) -> GitHubSyncService {
    GitHubSyncService::new(
        server.url(),
        "token-123".into(),
        GitHubRepo {
            owner: "acme".into(),
            repo: "widgets".into(),
        },
        None,
        cwd.to_path_buf(),
    )
}

fn issue_json(
    number: u64,
    title: &str,
    body: &str,
    state: &str,
    labels: &[&str],
) -> serde_json::Value {
    serde_json::json!({
        "number": number,
        "title": title,
        "body": body,
        "state": state,
        "html_url": format!("https://github.com/acme/widgets/issues/{number}"),
        "labels": labels.iter().map(|label| serde_json::json!({"name": label})).collect::<Vec<_>>(),
    })
}

fn empty_list(server: &mut ServerGuard) -> mockito::Mock {
    server
        .mock("GET", "/repos/acme/widgets/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("labels".into(), "dex".into()),
            Matcher::UrlEncoded("state".into(), "all".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_body("[]")
        .create()
}

#[test]
fn sync_task_creates_issue_with_hierarchy_body_and_labels() {
    let mut server = Server::new();
    let temp = tempfile::tempdir().unwrap();
    let mut root = task("root0001", "Root task", None);
    root.description = "root context".into();
    root.priority = 2;
    let mut child = task("child001", "Child", Some("root0001"));
    child.description = "child context".into();
    link(&mut root, &child);
    let tasks = vec![root.clone(), child.clone()];

    let list = empty_list(&mut server);
    let create = server
        .mock("POST", "/repos/acme/widgets/issues")
        .match_header("authorization", "Bearer token-123")
        .match_body(Matcher::PartialJson(serde_json::json!({
            "title": "Root task",
            "labels": ["dex", "dex:priority-2", "dex:pending"],
        })))
        .match_body(Matcher::Regex(
            r"<!-- dex:task:id:root0001 -->\\n<!-- dex:task:priority:2 -->".into(),
        ))
        .match_body(Matcher::Regex(
            r"## Tasks\\n\\n<details>\\n<summary><b>Child</b></summary>".into(),
        ))
        .with_status(201)
        .with_body(issue_json(7, "Root task", "", "open", &[]).to_string())
        .create();

    let result = service(&server, temp.path())
        .sync_task(&child, &tasks)
        .unwrap()
        .unwrap();

    list.assert();
    create.assert();
    assert_eq!(result.task_id, "root0001");
    assert!(result.created);
    assert_eq!(result.metadata["issueNumber"], 7);
    assert_eq!(
        result.metadata["issueUrl"],
        "https://github.com/acme/widgets/issues/7"
    );
    assert_eq!(result.metadata["repo"], "acme/widgets");
    assert_eq!(result.metadata["state"], "open");
    assert!(result.issue_not_closing_reason.is_none());
}

#[test]
fn completed_task_without_pushed_commit_stays_open_with_reason() {
    let mut server = Server::new();
    let temp = tempfile::tempdir().unwrap();
    let mut root = task("root0001", "Done", None);
    root.completed = true;
    root.completed_at = Some("2026-01-03T00:00:00.000Z".into());
    root.result = Some("finished".into());
    root.metadata = Some(serde_json::json!({"commit": {"sha": "abc1234def"}}));
    let tasks = vec![root.clone()];

    let _list = empty_list(&mut server);
    let create = server
        .mock("POST", "/repos/acme/widgets/issues")
        .match_body(Matcher::PartialJson(serde_json::json!({
            "labels": ["dex", "dex:priority-1", "dex:pending"],
        })))
        .with_status(201)
        .with_body(issue_json(8, "Done", "", "open", &[]).to_string())
        .create();

    let result = service(&server, temp.path())
        .sync_task(&root, &tasks)
        .unwrap()
        .unwrap();

    create.assert();
    assert_eq!(result.metadata["state"], "open");
    assert_eq!(
        result.issue_not_closing_reason.as_deref(),
        Some("commit abc1234 not pushed to remote")
    );

    let mut no_commit = root.clone();
    no_commit.metadata = None;
    let reason =
        service(&server, temp.path()).issue_not_closing_reason(&no_commit, &[no_commit.clone()]);
    assert_eq!(
        reason.as_deref(),
        Some("completed without commit (use --no-commit to close manually)")
    );
}

#[test]
fn existing_issue_is_skipped_when_unchanged_and_patched_when_changed() {
    let mut server = Server::new();
    let temp = tempfile::tempdir().unwrap();
    let mut root = task("root0001", "Root", None);
    root.description = "context".into();
    root.metadata = Some(
        serde_json::json!({"github": {"issueNumber": 5, "issueUrl": "u", "repo": "acme/widgets", "state": "open"}}),
    );
    let tasks = vec![root.clone()];
    let svc = service(&server, temp.path());
    let expected_body = dexrs::sync::github::body::render_root_body(&root, &[]);

    let unchanged = server
        .mock("GET", "/repos/acme/widgets/issues/5")
        .with_status(200)
        .with_body(
            issue_json(
                5,
                "Root",
                &expected_body,
                "open",
                &["dex", "dex:priority-1", "dex:pending", "unrelated"],
            )
            .to_string(),
        )
        .expect(1)
        .create();
    let result = svc.sync_task(&root, &tasks).unwrap().unwrap();
    unchanged.assert();
    assert!(result.skipped);
    assert!(!result.created);
    assert_eq!(result.metadata["issueNumber"], 5);

    let mut renamed = root.clone();
    renamed.name = "Renamed".into();
    let changed = server
        .mock("GET", "/repos/acme/widgets/issues/5")
        .with_status(200)
        .with_body(
            issue_json(
                5,
                "Root",
                &expected_body,
                "open",
                &["dex", "dex:priority-1", "dex:pending"],
            )
            .to_string(),
        )
        .expect(1)
        .create();
    let patch = server
        .mock("PATCH", "/repos/acme/widgets/issues/5")
        .match_body(Matcher::PartialJson(serde_json::json!({
            "title": "Renamed",
            "labels": ["dex", "dex:priority-1", "dex:pending"],
            "state": "open",
        })))
        .with_status(200)
        .with_body(issue_json(5, "Renamed", "", "open", &[]).to_string())
        .create();
    let result = svc
        .sync_task(&renamed, &[renamed.clone()])
        .unwrap()
        .unwrap();
    changed.assert();
    patch.assert();
    assert!(!result.skipped);
    assert_eq!(result.metadata["state"], "open");
}

#[test]
fn sync_all_pulls_newer_remote_state_and_reports_progress() {
    let mut server = Server::new();
    let temp = tempfile::tempdir().unwrap();
    let mut root = task("root0001", "Root", None);
    root.description = "context".into();
    let child = task("child001", "Child", Some("root0001"));
    link(&mut root, &child);
    let mut unrelated = task("other001", "Other", None);
    unrelated.priority = 1;
    unrelated.description = "other".into();
    let tasks = vec![root.clone(), child.clone(), unrelated.clone()];

    let mut remote_root = root.clone();
    remote_root.updated_at = Some("2026-02-01T00:00:00.000Z".into());
    remote_root.completed = true;
    remote_root.completed_at = Some("2026-02-01T00:00:00.000Z".into());
    remote_root.result = Some("closed on github".into());
    let mut remote_child = child.clone();
    remote_child.updated_at = Some("2026-02-01T00:00:00.000Z".into());
    remote_child.completed = true;
    remote_child.completed_at = Some("2026-02-01T00:00:00.000Z".into());
    remote_child.result = Some("child done remotely".into());
    let descendants = dexrs::sync::github::body::collect_descendants(
        &[remote_root.clone(), remote_child.clone()],
        "root0001",
    );
    let remote_body = dexrs::sync::github::body::render_root_body(&remote_root, &descendants);

    let list = server
        .mock("GET", "/repos/acme/widgets/issues")
        .match_query(Matcher::UrlEncoded("labels".into(), "dex".into()))
        .with_status(200)
        .with_body(serde_json::json!([
            issue_json(3, "Root", &remote_body, "closed", &["dex", "dex:priority-1", "dex:completed"]),
            {"number": 99, "title": "PR", "body": "<!-- dex:task:id:other001 -->", "state": "open", "html_url": "x", "labels": [], "pull_request": {}},
        ]).to_string())
        .expect_at_least(1)
        .create();
    let create_other = server
        .mock("POST", "/repos/acme/widgets/issues")
        .match_body(Matcher::PartialJson(serde_json::json!({"title": "Other"})))
        .with_status(201)
        .with_body(issue_json(10, "Other", "", "open", &[]).to_string())
        .create();

    let mut phases = Vec::new();
    let results = service(&server, temp.path())
        .sync_all(&tasks, &mut |progress| {
            phases.push((progress.task_id.clone(), progress.phase))
        })
        .unwrap();

    list.assert();
    create_other.assert();
    assert_eq!(results.len(), 2);
    let pulled = results
        .iter()
        .find(|result| result.task_id == "root0001")
        .unwrap();
    assert!(pulled.pulled_from_remote && pulled.skipped);
    assert_eq!(pulled.metadata["issueNumber"], 3);
    let updates = pulled.local_updates.as_ref().unwrap();
    assert_eq!(updates.completed, Some(true));
    assert_eq!(updates.result.as_deref(), Some("closed on github"));
    assert_eq!(
        updates.updated_at.as_deref(),
        Some("2026-02-01T00:00:00.000Z")
    );
    let sub = &pulled.subtask_results[0];
    assert_eq!(sub.task_id, "child001");
    assert_eq!(
        sub.local_updates.as_ref().unwrap().result.as_deref(),
        Some("child done remotely")
    );
    let created = results
        .iter()
        .find(|result| result.task_id == "other001")
        .unwrap();
    assert!(created.created);
    assert_eq!(
        phases,
        vec![
            ("root0001".to_string(), Phase::Checking),
            ("root0001".to_string(), Phase::Skipped),
            ("other001".to_string(), Phase::Checking),
            ("other001".to_string(), Phase::Creating),
        ]
    );
}

#[test]
fn close_remote_patches_issue_closed_and_missing_token_is_reported() {
    let mut server = Server::new();
    let temp = tempfile::tempdir().unwrap();
    let mut root = task("root0001", "Root", None);
    root.metadata = Some(serde_json::json!({"github": {"issueNumber": 4}}));
    let patch = server
        .mock("PATCH", "/repos/acme/widgets/issues/4")
        .match_body(Matcher::PartialJson(serde_json::json!({"state": "closed"})))
        .with_status(200)
        .with_body(issue_json(4, "Root", "", "closed", &[]).to_string())
        .create();

    service(&server, temp.path()).close_remote(&root).unwrap();
    patch.assert();

    let failing = server
        .mock("PATCH", "/repos/acme/widgets/issues/4")
        .with_status(401)
        .with_body(r#"{"message":"Bad credentials"}"#)
        .create();
    let error = service(&server, temp.path())
        .close_remote(&root)
        .unwrap_err()
        .to_string();
    failing.assert();
    assert!(
        error.contains("401") && error.contains("Bad credentials"),
        "{error}"
    );
}
