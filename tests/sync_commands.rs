use dexrs::task::Task;
use mockito::{Matcher, Server, ServerGuard};
use predicates::prelude::*;

struct Fixture {
    temp: tempfile::TempDir,
    server: ServerGuard,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(temp.path())
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
        };
        git(&["init", "-q", "-b", "main", "."]);
        git(&[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ]);
        git(&["commit", "-q", "--allow-empty", "-m", "first"]);
        Self {
            temp,
            server: Server::new(),
        }
    }

    fn enable_github(&self, auto_on_change: bool) {
        std::fs::create_dir_all(self.store()).unwrap();
        std::fs::write(
            self.store().join("config.toml"),
            format!("[sync.github]\nenabled = true\n\n[sync.github.auto]\non_change = {auto_on_change}\n"),
        )
        .unwrap();
    }

    fn store(&self) -> std::path::PathBuf {
        self.temp.path().join(".dex")
    }

    fn cmd(&self) -> assert_cmd::Command {
        let mut command = assert_cmd::Command::cargo_bin("dexrs").unwrap();
        command
            .current_dir(self.temp.path())
            .env("DEX_HOME", self.temp.path().join("dex-home"))
            .env("GITHUB_TOKEN", "token-123")
            .env("DEX_GITHUB_API_URL", self.server.url())
            .env("NO_COLOR", "1")
            .env_remove("DEX_STORAGE_PATH");
        command
    }

    fn create(&self, args: &[&str]) -> String {
        let out = self
            .cmd()
            .arg("create")
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .rsplit(' ')
            .next()
            .unwrap()
            .to_string()
    }

    fn tasks(&self) -> Vec<Task> {
        dexrs::store::read_tasks(&self.store()).unwrap()
    }

    fn task(&self, id: &str) -> Task {
        self.tasks().into_iter().find(|task| task.id == id).unwrap()
    }

    fn issue(&self, number: u64, title: &str, body: &str, state: &str) -> serde_json::Value {
        serde_json::json!({
            "number": number, "title": title, "body": body, "state": state,
            "html_url": format!("https://github.com/acme/widgets/issues/{number}"),
            "labels": [{"name": "dex"}],
        })
    }

    fn mock_list(&mut self, issues: serde_json::Value) -> mockito::Mock {
        self.server
            .mock("GET", "/repos/acme/widgets/issues")
            .match_query(Matcher::UrlEncoded("labels".into(), "dex".into()))
            .with_body(issues.to_string())
            .expect_at_least(1)
            .create()
    }
}

#[test]
fn sync_requires_a_configured_service() {
    let fx = Fixture::new();
    fx.create(&["Task"]);
    fx.cmd()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicates::str::contains("No sync services available"));
    fx.cmd()
        .args(["sync", "--github"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("GitHub sync not available"));
}

#[test]
fn sync_dry_run_lists_actions_without_calling_the_api() {
    let fx = Fixture::new();
    let a = fx.create(&["Alpha"]);
    let b = fx.create(&["Beta"]);
    let mut tasks = fx.tasks();
    tasks.iter_mut().find(|task| task.id == b).unwrap().metadata =
        Some(serde_json::json!({"github": {"issueNumber": 3}}));
    std::fs::write(
        fx.store().join("tasks.jsonl"),
        dexrs::task::serialize_tasks_jsonl(&tasks).unwrap(),
    )
    .unwrap();
    fx.enable_github(false);

    let out = fx
        .cmd()
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();
    assert!(
        out.starts_with("Would sync 2 task(s) to GitHub acme/widgets:\n"),
        "{out}"
    );
    assert!(out.contains(&format!("  [create] {a}: Alpha\n")), "{out}");
    assert!(out.contains(&format!("  [update] {b}: Beta\n")), "{out}");

    fx.cmd()
        .args(["sync", &a, "--dry-run"])
        .assert()
        .success()
        .stdout(format!(
            "Would sync to GitHub acme/widgets:\n  [create] {a}: Alpha\n"
        ));
}

#[test]
fn sync_creates_issues_saves_metadata_and_records_state() {
    let mut fx = Fixture::new();
    let root = fx.create(&["Root", "-d", "root details"]);
    let child = fx.create(&["Child", "--parent", &root]);
    fx.enable_github(false);
    let _list = fx.mock_list(serde_json::json!([]));
    let create = fx
        .server
        .mock("POST", "/repos/acme/widgets/issues")
        .match_body(Matcher::PartialJson(serde_json::json!({"title": "Root"})))
        .with_status(201)
        .with_body(fx.issue(11, "Root", "", "open").to_string())
        .create();

    let out = fx
        .cmd()
        .arg("sync")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();

    create.assert();
    assert!(
        out.contains("Syncing 1 task(s) to GitHub acme/widgets...\n"),
        "{out}"
    );
    assert!(
        out.contains("Synced to GitHub acme/widgets\n  (1 created)\n"),
        "{out}"
    );
    let saved = fx.task(&root).metadata.unwrap();
    assert_eq!(saved["github"]["issueNumber"], 11);
    assert_eq!(saved["github"]["repo"], "acme/widgets");
    assert_eq!(saved["github"]["state"], "open");
    assert!(fx.task(&child).metadata.is_none());
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fx.store().join("sync-state.json")).unwrap())
            .unwrap();
    assert!(state["lastSync"].is_string());

    let get = fx
        .server
        .mock("GET", "/repos/acme/widgets/issues/11")
        .with_body(fx.issue(11, "Root", "stale", "open").to_string())
        .create();
    let patch = fx
        .server
        .mock("PATCH", "/repos/acme/widgets/issues/11")
        .with_body(fx.issue(11, "Root", "", "open").to_string())
        .create();
    let out = fx
        .cmd()
        .args(["sync", &child])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();
    get.assert();
    patch.assert();
    assert!(out.starts_with(&format!("Synced task {root} to GitHub acme/widgets\n  https://github.com/acme/widgets/issues/11\n")), "{out}");
}

#[test]
fn auto_sync_runs_on_mutations_and_delete_closes_the_issue() {
    let mut fx = Fixture::new();
    fx.enable_github(true);
    let _list = fx.mock_list(serde_json::json!([]));
    let create = fx
        .server
        .mock("POST", "/repos/acme/widgets/issues")
        .with_status(201)
        .with_body(fx.issue(21, "Auto", "", "open").to_string())
        .create();

    let id = fx.create(&["Auto"]);
    create.assert();
    assert_eq!(fx.task(&id).metadata.unwrap()["github"]["issueNumber"], 21);

    let get = fx
        .server
        .mock("GET", "/repos/acme/widgets/issues/21")
        .with_body(fx.issue(21, "Auto", "stale body", "open").to_string())
        .create();
    let patch = fx
        .server
        .mock("PATCH", "/repos/acme/widgets/issues/21")
        .match_body(Matcher::PartialJson(
            serde_json::json!({"title": "Renamed"}),
        ))
        .with_body(fx.issue(21, "Renamed", "", "open").to_string())
        .create();
    fx.cmd()
        .args(["edit", &id, "-n", "Renamed"])
        .assert()
        .success();
    get.assert();
    patch.assert();

    let close = fx
        .server
        .mock("PATCH", "/repos/acme/widgets/issues/21")
        .match_body(Matcher::Json(serde_json::json!({"state": "closed"})))
        .with_body(fx.issue(21, "Renamed", "", "closed").to_string())
        .create();
    fx.cmd().args(["delete", &id]).assert().success();
    close.assert();
    assert!(fx.tasks().is_empty());
}

#[test]
fn auto_sync_failures_warn_but_do_not_block_the_mutation() {
    let mut fx = Fixture::new();
    fx.enable_github(true);
    let _list = fx.mock_list(serde_json::json!([]));
    let _create = fx
        .server
        .mock("POST", "/repos/acme/widgets/issues")
        .with_status(500)
        .with_body(r#"{"message":"boom"}"#)
        .create();

    let out = fx
        .cmd()
        .args(["create", "Fragile"])
        .assert()
        .success()
        .stderr(
            predicates::str::contains("GitHub sync failed:").and(predicates::str::contains("boom")),
        );
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.starts_with("Created task "), "{stdout}");
    assert_eq!(fx.tasks().len(), 1);
}

#[test]
fn import_creates_task_from_issue_with_subtasks_and_respects_update() {
    let mut fx = Fixture::new();
    let body = "<!-- dex:task:id:abc12345 -->\n<!-- dex:task:priority:2 -->\n<!-- dex:task:completed:false -->\nImported context\n\n## Tasks\n\n<details>\n<summary><b>Sub one</b></summary>\n\n<!-- dex:subtask:id:sub00001 -->\n<!-- dex:subtask:parent:abc12345 -->\n<!-- dex:subtask:priority:1 -->\n<!-- dex:subtask:completed:false -->\n\n### Description\nsub details\n\n</details>\n";
    let get = fx
        .server
        .mock("GET", "/repos/acme/widgets/issues/42")
        .with_body(fx.issue(42, "Imported", body, "open").to_string())
        .expect_at_least(1)
        .create();

    fx.cmd()
        .args(["import", "#42", "--dry-run"])
        .assert()
        .success()
        .stdout("Would import from GitHub acme/widgets:\n  #42: Imported\n  (1 subtasks)\n");
    assert!(fx.tasks().is_empty());

    fx.cmd()
        .args(["import", "#42"])
        .assert()
        .success()
        .stdout("Imported issue #42 as task abc12345: \"Imported\"\n  Created 1 subtask(s)\n");
    get.assert();
    let root = fx.task("abc12345");
    assert_eq!(root.description, "Imported context");
    assert_eq!(root.priority, 2);
    assert_eq!(root.metadata.as_ref().unwrap()["github"]["issueNumber"], 42);
    assert_eq!(
        root.metadata.as_ref().unwrap()["github"]["issueUrl"],
        "https://github.com/acme/widgets/issues/42"
    );
    let sub = fx.task("sub00001");
    assert_eq!(sub.parent_id.as_deref(), Some("abc12345"));
    assert_eq!(sub.description, "sub details");
    assert_eq!(root.children, vec!["sub00001"]);

    fx.cmd()
        .args(["import", "#42"])
        .assert()
        .success()
        .stdout("Skipped GitHub issue #42: already imported as task abc12345\n  Use --update to refresh from GitHub\n");

    let renamed = fx
        .server
        .mock("GET", "/repos/acme/widgets/issues/42")
        .with_body(fx.issue(42, "Renamed upstream", body, "closed").to_string())
        .create();
    fx.cmd()
        .args([
            "import",
            "https://github.com/acme/widgets/issues/42",
            "--update",
        ])
        .assert()
        .success()
        .stdout("Updated task abc12345 from GitHub issue #42\n");
    renamed.assert();
    let root = fx.task("abc12345");
    assert_eq!(root.name, "Renamed upstream");
    assert!(root.completed);
    assert_eq!(
        root.result.as_deref(),
        Some("Updated from closed GitHub issue")
    );
}

#[test]
fn import_all_imports_labelled_issues_and_skips_known_ones() {
    let mut fx = Fixture::new();
    let known = fx.create(&["Known"]);
    let mut tasks = fx.tasks();
    tasks[0].metadata = Some(serde_json::json!({"github": {"issueNumber": 1}}));
    std::fs::write(
        fx.store().join("tasks.jsonl"),
        dexrs::task::serialize_tasks_jsonl(&tasks).unwrap(),
    )
    .unwrap();
    let _list = fx.mock_list(serde_json::json!([
        fx.issue(1, "Known", "", "open"),
        fx.issue(2, "Fresh", "plain body", "open"),
        {"number": 3, "title": "PR", "body": "", "state": "open", "html_url": "x", "labels": [], "pull_request": {}},
    ]));

    let out = fx
        .cmd()
        .args(["import", "--all", "--github"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();

    let fresh = fx
        .tasks()
        .into_iter()
        .find(|task| task.name == "Fresh")
        .unwrap();
    assert_eq!(fresh.description, "plain body");
    assert_eq!(fresh.metadata.unwrap()["github"]["issueNumber"], 2);
    assert!(
        out.contains(&format!("Imported GitHub #2 as {}\n", fresh.id)),
        "{out}"
    );
    assert!(out.contains("\nGitHub: Imported 1, updated 0 issue(s) from acme/widgets\nSkipped 1 already imported (use --update to refresh)\n"), "{out}");
    assert_eq!(fx.task(&known).name, "Known");
}

#[test]
fn export_creates_issue_without_saving_metadata_and_skips_synced_tasks() {
    let mut fx = Fixture::new();
    let fresh = fx.create(&["Fresh"]);
    let synced = fx.create(&["Synced"]);
    let mut tasks = fx.tasks();
    tasks
        .iter_mut()
        .find(|task| task.id == synced)
        .unwrap()
        .metadata = Some(serde_json::json!({"github": {"issueNumber": 9}}));
    std::fs::write(
        fx.store().join("tasks.jsonl"),
        dexrs::task::serialize_tasks_jsonl(&tasks).unwrap(),
    )
    .unwrap();
    let _list = fx.mock_list(serde_json::json!([]));
    let create = fx
        .server
        .mock("POST", "/repos/acme/widgets/issues")
        .match_body(Matcher::PartialJson(serde_json::json!({"title": "Fresh"})))
        .with_status(201)
        .with_body(fx.issue(30, "Fresh", "", "open").to_string())
        .create();

    fx.cmd()
        .args(["export", &fresh, "--dry-run"])
        .assert()
        .success()
        .stdout(format!(
            "Would export to acme/widgets:\n  [create] {fresh}: Fresh\n"
        ));

    let out = fx
        .cmd()
        .args(["export", &fresh, &synced])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();
    create.assert();
    assert_eq!(
        out,
        format!(
            "Exported task {fresh} to acme/widgets\n  https://github.com/acme/widgets/issues/30\nSkipped {synced}: already synced to GitHub\n\n1 exported, 1 skipped\n"
        )
    );
    assert!(
        fx.task(&fresh).metadata.is_none(),
        "export must not save metadata"
    );

    fx.cmd()
        .arg("export")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "At least one task ID is required",
        ));
}
