use dexrs::task::Task;
use predicates::prelude::PredicateBooleanExt;

fn read_tasks(store: &std::path::Path) -> Vec<Task> {
    dexrs::store::read_tasks(store).unwrap()
}

#[test]
fn dexrs_dir_prints_store_path_from_env() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let output = assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .arg("dir")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!("{}\n", store.display())
    );
}

#[test]
fn create_and_add_write_new_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    for (command, name) in [("create", "First task"), ("add", "Second task")] {
        assert_cmd::Command::cargo_bin("dexrs")
            .unwrap()
            .env("DEX_STORAGE_PATH", &store)
            .args([command, name])
            .assert()
            .success();
    }

    let tasks = read_tasks(&store);
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().any(|task| task.name == "First task"));
    assert!(tasks.iter().any(|task| task.name == "Second task"));
    assert!(tasks.iter().all(|task| task.id.len() == 8));
}

#[test]
fn start_marks_task_in_progress() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["create", "Start me"])
        .assert()
        .success();
    let id = read_tasks(&store)[0].id.clone();

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["start", &id])
        .assert()
        .success();

    let task = read_tasks(&store).pop().unwrap();
    assert_eq!(task.id, id);
    assert!(task.started_at.is_some());
    assert!(!task.completed);
}

#[test]
fn complete_and_done_mark_task_complete_with_result() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    for (name, command, result) in [
        ("Complete me", "complete", "finished"),
        ("Done me", "done", "also finished"),
    ] {
        assert_cmd::Command::cargo_bin("dexrs")
            .unwrap()
            .env("DEX_STORAGE_PATH", &store)
            .args(["create", name])
            .assert()
            .success();
        let id = read_tasks(&store)
            .into_iter()
            .find(|task| task.name == name)
            .unwrap()
            .id;

        assert_cmd::Command::cargo_bin("dexrs")
            .unwrap()
            .env("DEX_STORAGE_PATH", &store)
            .args([command, &id, "--result", result])
            .assert()
            .success();
    }

    let tasks = read_tasks(&store);
    assert!(tasks.iter().all(|task| task.completed));
    assert!(tasks.iter().all(|task| task.completed_at.is_some()));
    assert!(
        tasks
            .iter()
            .any(|task| task.result.as_deref() == Some("finished"))
    );
    assert!(
        tasks
            .iter()
            .any(|task| task.result.as_deref() == Some("also finished"))
    );
}

#[test]
fn edit_and_update_change_task_fields() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["create", "Old"])
        .assert()
        .success();
    let id = read_tasks(&store)[0].id.clone();

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args([
            "edit",
            &id,
            "--name",
            "New",
            "--description",
            "Details",
            "--priority",
            "3",
        ])
        .assert()
        .success();

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["update", &id, "--priority", "2"])
        .assert()
        .success();

    let task = read_tasks(&store).pop().unwrap();
    assert_eq!(task.name, "New");
    assert_eq!(task.description, "Details");
    assert_eq!(task.priority, 2);
}

#[test]
fn delete_remove_and_rm_delete_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    for (name, command) in [
        ("Delete me", "delete"),
        ("Remove me", "remove"),
        ("Rm me", "rm"),
    ] {
        assert_cmd::Command::cargo_bin("dexrs")
            .unwrap()
            .env("DEX_STORAGE_PATH", &store)
            .args(["create", name])
            .assert()
            .success();
        let id = read_tasks(&store)
            .into_iter()
            .find(|task| task.name == name)
            .unwrap()
            .id;

        assert_cmd::Command::cargo_bin("dexrs")
            .unwrap()
            .env("DEX_STORAGE_PATH", &store)
            .args([command, &id])
            .assert()
            .success();
    }

    assert!(read_tasks(&store).is_empty());
}

#[test]
fn status_reports_empty_and_non_empty_counts() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .arg("status")
        .assert()
        .success()
        .stdout("0 todo, 0 in progress, 0 done\n");

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["create", "Visible"])
        .assert()
        .success();
    let id = read_tasks(&store)[0].id.clone();
    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["start", &id])
        .assert()
        .success();

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .arg("status")
        .assert()
        .success()
        .stdout("0 todo, 1 in progress, 0 done\n");
}

#[test]
fn list_and_ls_print_status_icons() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["create", "List me"])
        .assert()
        .success();

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::contains("[ ]").and(predicates::str::contains("List me")));

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicates::str::contains("List me"));
}

#[test]
fn show_prints_task_details_by_id() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["create", "Show me", "--description", "Details"])
        .assert()
        .success();
    let id = read_tasks(&store)[0].id.clone();

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["show", &id])
        .assert()
        .success()
        .stdout(predicates::str::contains(&id).and(predicates::str::contains("Details")));
}

#[test]
fn read_commands_support_json_output() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["create", "Json task"])
        .assert()
        .success();
    let id = read_tasks(&store)[0].id.clone();

    for args in [
        vec!["status", "--json"],
        vec!["list", "--json"],
        vec!["show", &id, "--json"],
    ] {
        let output = assert_cmd::Command::cargo_bin("dexrs")
            .unwrap()
            .env("DEX_STORAGE_PATH", &store)
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice::<serde_json::Value>(&output).unwrap();
    }
}

#[test]
fn dex_binary_matches_dexrs_for_dir() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    let dexrs_output = assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .arg("dir")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let dex_output = assert_cmd::Command::cargo_bin("dex")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .arg("dir")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(dex_output, dexrs_output);
}

#[test]
fn init_creates_store_directory_and_empty_task_file() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .arg("init")
        .assert()
        .success();

    assert!(store.is_dir());
    assert_eq!(
        std::fs::read_to_string(store.join("tasks.jsonl")).unwrap(),
        ""
    );
}

#[test]
fn create_writes_record_the_original_dex_schema_accepts() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["create", "Bare"])
        .assert()
        .success();

    let raw = std::fs::read_to_string(store.join("tasks.jsonl")).unwrap();
    assert!(raw.contains(r#""description":"""#), "{raw}");
    assert!(raw.contains(r#""priority":1"#), "{raw}");

    let Some(reference) = std::env::var_os("DEX_REFERENCE_BIN") else {
        return;
    };
    let output = std::process::Command::new(reference)
        .current_dir(temp.path())
        .env("DEX_STORAGE_PATH", &store)
        .env("NO_COLOR", "1")
        .args(["list", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "reference dex rejected the store:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Bare"));
}

fn dexrs(store: &std::path::Path) -> assert_cmd::Command {
    let mut command = assert_cmd::Command::cargo_bin("dexrs").unwrap();
    command.env("DEX_STORAGE_PATH", store);
    command
}

fn create(store: &std::path::Path, args: &[&str]) -> String {
    let output = dexrs(store)
        .arg("create")
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output)
        .unwrap()
        .trim()
        .rsplit(' ')
        .next()
        .unwrap()
        .to_string()
}

#[test]
fn complete_accepts_result_flag_and_marks_started() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let id = create(&store, &["Finish me"]);

    dexrs(&store)
        .args(["complete", &id, "--result", "all done"])
        .assert()
        .success();

    let task = read_tasks(&store).pop().unwrap();
    assert!(task.completed);
    assert_eq!(task.result.as_deref(), Some("all done"));
    assert!(task.started_at.is_some());
}

#[test]
fn complete_short_result_flag_and_no_commit_are_accepted() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let id = create(&store, &["Finish me"]);

    dexrs(&store)
        .args(["complete", &id, "-r", "short", "--no-commit"])
        .assert()
        .success();

    assert_eq!(read_tasks(&store)[0].result.as_deref(), Some("short"));
}

#[test]
fn complete_requires_result() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let id = create(&store, &["Finish me"]);

    dexrs(&store)
        .args(["complete", &id])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--result"));

    assert!(!read_tasks(&store)[0].completed);
}

fn git_repo_with_commit(dir: &std::path::Path) -> String {
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q", "-b", "main", "."]);
    git(&["commit", "-q", "--allow-empty", "-m", "first change"]);
    git(&["rev-parse", "HEAD"])
}

#[test]
fn complete_with_commit_records_commit_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let sha = git_repo_with_commit(temp.path());
    let id = create(&store, &["Finish me"]);

    dexrs(&store)
        .current_dir(temp.path())
        .args(["complete", &id, "-r", "done", "--commit", &sha[..7]])
        .assert()
        .success();

    let task = read_tasks(&store).pop().unwrap();
    let commit = &task.metadata.unwrap()["commit"];
    assert_eq!(commit["sha"], sha);
    assert_eq!(commit["message"], "first change");
    assert_eq!(commit["branch"], "main");
    assert!(commit["timestamp"].is_string());
}

#[test]
fn complete_with_unknown_commit_fails_without_changes() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    git_repo_with_commit(temp.path());
    let id = create(&store, &["Finish me"]);

    dexrs(&store)
        .current_dir(temp.path())
        .args(["complete", &id, "-r", "done", "--commit", "0000000"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not found"));

    assert!(!read_tasks(&store)[0].completed);
}

fn task(store: &std::path::Path, id: &str) -> Task {
    read_tasks(store)
        .into_iter()
        .find(|task| task.id == id)
        .unwrap()
}

#[test]
fn create_with_parent_links_both_directions() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let parent = create(&store, &["Parent"]);

    let child = create(&store, &["-n", "Child", "--parent", &parent]);

    assert_eq!(task(&store, &child).parent_id.as_deref(), Some(parent.as_str()));
    assert_eq!(task(&store, &parent).children, vec![child]);
}

#[test]
fn create_with_missing_parent_fails() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    dexrs(&store)
        .args(["create", "Orphan", "--parent", "nope1234"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("nope1234"));

    assert!(read_tasks(&store).is_empty());
}

#[test]
fn create_with_blocked_by_links_both_directions() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let first = create(&store, &["First"]);
    let second = create(&store, &["Second"]);

    let blocked = create(
        &store,
        &["Blocked", "--blocked-by", &format!("{first},{second}")],
    );

    assert_eq!(task(&store, &blocked).blocked_by, vec![first.clone(), second.clone()]);
    assert_eq!(task(&store, &first).blocks, vec![blocked.clone()]);
    assert_eq!(task(&store, &second).blocks, vec![blocked]);
}

#[test]
fn edit_parent_moves_task_between_parents() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let old_parent = create(&store, &["Old"]);
    let new_parent = create(&store, &["New"]);
    let child = create(&store, &["Child", "--parent", &old_parent]);

    dexrs(&store)
        .args(["edit", &child, "--parent", &new_parent])
        .assert()
        .success();

    assert_eq!(task(&store, &child).parent_id.as_deref(), Some(new_parent.as_str()));
    assert!(task(&store, &old_parent).children.is_empty());
    assert_eq!(task(&store, &new_parent).children, vec![child]);
}

#[test]
fn edit_adds_and_removes_blockers() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let blocker = create(&store, &["Blocker"]);
    let other = create(&store, &["Other"]);
    let blocked = create(&store, &["Blocked"]);

    dexrs(&store)
        .args(["edit", &blocked, "--add-blocker", &format!("{blocker},{other}")])
        .assert()
        .success();
    assert_eq!(task(&store, &blocked).blocked_by, vec![blocker.clone(), other.clone()]);
    assert_eq!(task(&store, &blocker).blocks, vec![blocked.clone()]);

    dexrs(&store)
        .args(["edit", &blocked, "--remove-blocker", &blocker])
        .assert()
        .success();
    assert_eq!(task(&store, &blocked).blocked_by, vec![other]);
    assert!(task(&store, &blocker).blocks.is_empty());
}

#[test]
fn edit_short_name_flag_and_commit_link() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let sha = git_repo_with_commit(temp.path());
    let id = create(&store, &["Before"]);

    dexrs(&store)
        .current_dir(temp.path())
        .args(["edit", &id, "-n", "After", "--commit", &sha])
        .assert()
        .success();

    let task = task(&store, &id);
    assert_eq!(task.name, "After");
    assert_eq!(task.metadata.unwrap()["commit"]["sha"], sha);
}
