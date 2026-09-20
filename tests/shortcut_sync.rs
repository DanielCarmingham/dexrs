use dexrs::sync::shortcut::service::ShortcutSyncService;
use dexrs::sync::shortcut::story::render_story_description;
use dexrs::task::Task;
use mockito::{Matcher, Server, ServerGuard};

fn task(id: &str, name: &str, parent: Option<&str>) -> Task {
    let mut task = Task::new(id.to_string(), name.to_string(), None, None);
    task.parent_id = parent.map(str::to_string);
    task.created_at = Some("2026-01-01T00:00:00.000Z".into());
    task.updated_at = Some("2026-01-02T00:00:00.000Z".into());
    task
}

fn service(server: &ServerGuard, cwd: &std::path::Path) -> ShortcutSyncService {
    ShortcutSyncService::new(
        server.url(),
        "sc-token".into(),
        "acme".into(),
        "engineering".into(),
        None,
        None,
        cwd.to_path_buf(),
    )
}

fn workflow_mocks(server: &mut ServerGuard) -> Vec<mockito::Mock> {
    vec![
        server
            .mock("GET", "/groups")
            .match_header("shortcut-token", "sc-token")
            .with_body(r#"[{"id":"team-uuid","name":"Engineering","mention_name":"engineering","workflow_ids":[500]}]"#)
            .create(),
        server
            .mock("GET", "/groups/team-uuid")
            .with_body(r#"{"id":"team-uuid","name":"Engineering","mention_name":"engineering","workflow_ids":[500]}"#)
            .create(),
        server
            .mock("GET", "/workflows/500")
            .with_body(r#"{"id":500,"states":[{"id":1,"type":"unstarted"},{"id":2,"type":"started"},{"id":3,"type":"done"}]}"#)
            .create(),
    ]
}

fn story_json(
    id: u64,
    name: &str,
    description: &str,
    completed: bool,
    state: u64,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "description": description,
        "completed": completed,
        "workflow_state_id": state,
        "labels": [{"name": "dex"}],
        "story_links": [],
        "app_url": format!("https://app.shortcut.com/acme/story/{id}"),
    })
}

#[test]
fn sync_task_creates_story_subtask_and_blocker_link() {
    let mut server = Server::new();
    let temp = tempfile::tempdir().unwrap();
    let mut root = task("root0001", "Root", None);
    root.description = "root context".into();
    let mut child = task("child001", "Child", Some("root0001"));
    child.started_at = Some("2026-01-02T00:00:00.000Z".into());
    root.children.push(child.id.clone());
    let mut blocker = task("blk00001", "Blocker", None);
    blocker.metadata = Some(serde_json::json!({"shortcut": {"storyId": 77}}));
    root.blocked_by.push(blocker.id.clone());
    blocker.blocks.push(root.id.clone());
    let tasks = vec![root.clone(), child.clone(), blocker.clone()];
    let _workflow = workflow_mocks(&mut server);

    let search = server
        .mock("GET", "/search/stories")
        .match_query(Matcher::UrlEncoded(
            "query".into(),
            "label:\"dex\" description:\"dex:task:id:root0001\"".into(),
        ))
        .with_body(r#"{"data":[],"total":0}"#)
        .create();
    let create_root = server
        .mock("POST", "/stories")
        .match_body(Matcher::PartialJson(serde_json::json!({
            "name": "Root",
            "description": render_story_description(&root),
            "story_type": "feature",
            "workflow_state_id": 1,
            "labels": [{"name": "dex"}],
            "group_id": "team-uuid",
        })))
        .with_status(201)
        .with_body(story_json(100, "Root", "", false, 1).to_string())
        .create();
    let child_search = server
        .mock("GET", "/search/stories")
        .match_query(Matcher::UrlEncoded(
            "query".into(),
            "label:\"dex\" description:\"dex:task:id:child001\"".into(),
        ))
        .with_body(r#"{"data":[],"total":0}"#)
        .create();
    let create_child = server
        .mock("POST", "/stories")
        .match_body(Matcher::PartialJson(serde_json::json!({
            "name": "Child",
            "story_type": "chore",
            "workflow_state_id": 2,
            "parent_story_id": 100,
            "group_id": "team-uuid",
        })))
        .with_status(201)
        .with_body(story_json(101, "Child", "", false, 2).to_string())
        .create();
    let links = server
        .mock("GET", "/stories/100")
        .with_body(story_json(100, "Root", "", false, 1).to_string())
        .create();
    let link = server
        .mock("POST", "/story-links")
        .match_body(Matcher::Json(serde_json::json!({
            "subject_id": 77, "object_id": 100, "verb": "blocks"
        })))
        .with_status(201)
        .with_body(r#"{"id":1}"#)
        .create();

    let result = service(&server, temp.path())
        .sync_task(&child, &tasks)
        .unwrap()
        .unwrap();

    search.assert();
    create_root.assert();
    child_search.assert();
    create_child.assert();
    links.assert();
    link.assert();
    assert_eq!(result.task_id, "root0001");
    assert!(result.created);
    assert_eq!(result.metadata["storyId"], 100);
    assert_eq!(
        result.metadata["storyUrl"],
        "https://app.shortcut.com/acme/story/100"
    );
    assert_eq!(result.metadata["workspace"], "acme");
    assert_eq!(result.metadata["state"], "unstarted");
    assert_eq!(result.subtask_results.len(), 1);
    assert_eq!(result.subtask_results[0].task_id, "child001");
    assert!(result.subtask_results[0].created);
    assert_eq!(result.subtask_results[0].metadata["storyId"], 101);
    assert_eq!(result.subtask_results[0].metadata["state"], "started");
}

#[test]
fn existing_story_is_skipped_when_unchanged_and_updated_when_changed() {
    let mut server = Server::new();
    let temp = tempfile::tempdir().unwrap();
    let mut root = task("root0001", "Root", None);
    root.description = "context".into();
    root.metadata = Some(
        serde_json::json!({"shortcut": {"storyId": 55, "storyUrl": "u", "workspace": "acme", "state": "unstarted"}}),
    );
    let tasks = vec![root.clone()];
    let _workflow = workflow_mocks(&mut server);
    let svc = service(&server, temp.path());

    let unchanged = server
        .mock("GET", "/stories/55")
        .with_body(story_json(55, "Root", &render_story_description(&root), false, 1).to_string())
        .expect(1)
        .create();
    let result = svc.sync_task(&root, &tasks).unwrap().unwrap();
    unchanged.assert();
    assert!(result.skipped);
    assert_eq!(result.metadata["storyId"], 55);
    assert_eq!(result.metadata["state"], "unstarted");

    let mut renamed = root.clone();
    renamed.name = "Renamed".into();
    let changed = server
        .mock("GET", "/stories/55")
        .with_body(story_json(55, "Root", &render_story_description(&root), false, 1).to_string())
        .expect(1)
        .create();
    let update = server
        .mock("PUT", "/stories/55")
        .match_body(Matcher::PartialJson(serde_json::json!({
            "name": "Renamed",
            "workflow_state_id": 1,
            "labels": [{"name": "dex"}],
        })))
        .with_body(story_json(55, "Renamed", "", false, 1).to_string())
        .create();
    let result = svc
        .sync_task(&renamed, &[renamed.clone()])
        .unwrap()
        .unwrap();
    changed.assert();
    update.assert();
    assert!(!result.skipped);
}

#[test]
fn sync_all_ensures_label_and_uses_search_cache() {
    let mut server = Server::new();
    let temp = tempfile::tempdir().unwrap();
    let mut root = task("root0001", "Root", None);
    root.description = "context".into();
    let tasks = vec![root.clone()];
    let _workflow = workflow_mocks(&mut server);

    let labels = server.mock("GET", "/labels").with_body("[]").create();
    let create_label = server
        .mock("POST", "/labels")
        .match_body(Matcher::Json(serde_json::json!({"name": "dex"})))
        .with_status(201)
        .with_body(r#"{"id":9,"name":"dex"}"#)
        .create();
    let search_all = server
        .mock("GET", "/search/stories")
        .match_query(Matcher::UrlEncoded("query".into(), "label:\"dex\"".into()))
        .with_body(
            serde_json::json!({"data": [story_json(60, "Root", &render_story_description(&root), false, 1)], "total": 1})
                .to_string(),
        )
        .create();

    let results = service(&server, temp.path())
        .sync_all(&tasks, &mut |_| {})
        .unwrap();

    labels.assert();
    create_label.assert();
    search_all.assert();
    assert_eq!(results.len(), 1);
    assert!(results[0].skipped, "cache matched an unchanged story");
    assert_eq!(results[0].metadata["storyId"], 60);
}

#[test]
fn close_remote_moves_story_to_done_state() {
    let mut server = Server::new();
    let temp = tempfile::tempdir().unwrap();
    let mut root = task("root0001", "Root", None);
    root.metadata = Some(serde_json::json!({"shortcut": {"storyId": 12}}));
    let _workflow = workflow_mocks(&mut server);
    let update = server
        .mock("PUT", "/stories/12")
        .match_body(Matcher::Json(serde_json::json!({"workflow_state_id": 3})))
        .with_body(story_json(12, "Root", "", true, 3).to_string())
        .create();

    service(&server, temp.path()).close_remote(&root).unwrap();
    update.assert();
}
