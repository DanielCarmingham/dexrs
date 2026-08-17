use dexrs::task::{Task, parse_tasks_jsonl};
use predicates::prelude::PredicateBooleanExt;

fn read_tasks(store: &std::path::Path) -> Vec<Task> {
    parse_tasks_jsonl(&std::fs::read_to_string(store.join("tasks.jsonl")).unwrap()).unwrap()
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
            .args([command, &id, result])
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
            "low",
        ])
        .assert()
        .success();

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env("DEX_STORAGE_PATH", &store)
        .args(["update", &id, "--priority", "high"])
        .assert()
        .success();

    let task = read_tasks(&store).pop().unwrap();
    assert_eq!(task.name, "New");
    assert_eq!(task.description.as_deref(), Some("Details"));
    assert_eq!(task.priority.as_deref(), Some("high"));
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
