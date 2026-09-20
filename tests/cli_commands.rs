use dexrs::task::Task;
use predicates::prelude::PredicateBooleanExt;

fn read_tasks(store: &std::path::Path) -> Vec<Task> {
    dexrs::store::read_tasks(store).unwrap()
}

#[test]
fn dexrs_dir_prints_store_path_from_env() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let output = dexrs(&store)
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
        dexrs(&store).args([command, name]).assert().success();
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

    dexrs(&store)
        .args(["create", "Start me"])
        .assert()
        .success();
    let id = read_tasks(&store)[0].id.clone();

    dexrs(&store).args(["start", &id]).assert().success();

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
        dexrs(&store).args(["create", name]).assert().success();
        let id = read_tasks(&store)
            .into_iter()
            .find(|task| task.name == name)
            .unwrap()
            .id;

        dexrs(&store)
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

    dexrs(&store).args(["create", "Old"]).assert().success();
    let id = read_tasks(&store)[0].id.clone();

    dexrs(&store)
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

    dexrs(&store)
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
        dexrs(&store).args(["create", name]).assert().success();
        let id = read_tasks(&store)
            .into_iter()
            .find(|task| task.name == name)
            .unwrap()
            .id;

        dexrs(&store).args([command, &id]).assert().success();
    }

    assert!(read_tasks(&store).is_empty());
}

#[test]
fn list_and_ls_print_status_icons() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    dexrs(&store).args(["create", "List me"]).assert().success();

    dexrs(&store)
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::contains("[ ]").and(predicates::str::contains("List me")));

    dexrs(&store)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicates::str::contains("List me"));
}

#[test]
fn show_prints_task_details_by_id() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    dexrs(&store)
        .args(["create", "Show me", "--description", "Details"])
        .assert()
        .success();
    let id = read_tasks(&store)[0].id.clone();

    dexrs(&store)
        .args(["show", &id])
        .assert()
        .success()
        .stdout(predicates::str::contains(&id).and(predicates::str::contains("Details")));
}

#[test]
fn read_commands_support_json_output() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    dexrs(&store)
        .args(["create", "Json task"])
        .assert()
        .success();
    let id = read_tasks(&store)[0].id.clone();

    for args in [
        vec!["status", "--json"],
        vec!["list", "--json"],
        vec!["show", &id, "--json"],
    ] {
        let output = dexrs(&store)
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

    let dexrs_output = dexrs(&store)
        .arg("dir")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let dex_output = assert_cmd::Command::cargo_bin("dex")
        .unwrap()
        .env("DEX_HOME", temp.path().join("dex-home"))
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
fn create_writes_record_the_original_dex_schema_accepts() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    dexrs(&store).args(["create", "Bare"]).assert().success();

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
    let mut command = bare(store);
    command.env("DEX_STORAGE_PATH", store);
    command
}

fn bare(scratch: &std::path::Path) -> assert_cmd::Command {
    let mut command = assert_cmd::Command::cargo_bin("dexrs").unwrap();
    command.env("DEX_HOME", scratch.join("dex-home"));
    command.env_remove("DEX_STORAGE_PATH");
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

    assert_eq!(
        task(&store, &child).parent_id.as_deref(),
        Some(parent.as_str())
    );
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

    assert_eq!(
        task(&store, &blocked).blocked_by,
        vec![first.clone(), second.clone()]
    );
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

    assert_eq!(
        task(&store, &child).parent_id.as_deref(),
        Some(new_parent.as_str())
    );
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
        .args([
            "edit",
            &blocked,
            "--add-blocker",
            &format!("{blocker},{other}"),
        ])
        .assert()
        .success();
    assert_eq!(
        task(&store, &blocked).blocked_by,
        vec![blocker.clone(), other.clone()]
    );
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

fn list(store: &std::path::Path, args: &[&str]) -> String {
    let output = dexrs(store)
        .arg("list")
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output).unwrap()
}

#[test]
fn list_hides_completed_unless_asked() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let open = create(&store, &["Open"]);
    let done = create(&store, &["Done"]);
    dexrs(&store)
        .args(["complete", &done, "-r", "x"])
        .assert()
        .success();

    let default = list(&store, &[]);
    assert!(
        default.contains(&open) && !default.contains(&done),
        "{default}"
    );

    let all = list(&store, &["--all"]);
    assert!(all.contains(&open) && all.contains(&done), "{all}");

    let completed = list(&store, &["--completed"]);
    assert!(
        !completed.contains(&open) && completed.contains(&done),
        "{completed}"
    );
}

#[test]
fn list_renders_children_as_a_tree_sorted_by_priority_then_id() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let parent = create(&store, &["Parent"]);
    let child_a = create(&store, &["Child A", "--parent", &parent]);
    let child_b = create(&store, &["Child B", "--parent", &parent, "-p", "2"]);
    let later = create(&store, &["Later", "-p", "3"]);

    let output = list(&store, &[]);

    let expected = format!(
        "[ ] {parent}: Parent\n├── [ ] {child_a}: Child A\n└── [ ] {child_b} [p2]: Child B\n[ ] {later} [p3]: Later\n"
    );
    assert_eq!(output, expected);
}

#[test]
fn list_shows_blocker_indicator_only_while_blocker_is_open() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let blocker = create(&store, &["Blocker"]);
    let blocked = create(&store, &["Blocked", "--blocked-by", &blocker]);

    assert!(list(&store, &[]).contains(&format!("{blocked} [B: {blocker}]: Blocked")));

    dexrs(&store)
        .args(["complete", &blocker, "-r", "x"])
        .assert()
        .success();
    assert!(list(&store, &[]).contains(&format!("{blocked}: Blocked")));
}

#[test]
fn list_filters_ready_blocked_and_in_progress() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let ready = create(&store, &["Ready"]);
    let started = create(&store, &["Started"]);
    let blocked = create(&store, &["Blocked", "--blocked-by", &ready]);
    dexrs(&store).args(["start", &started]).assert().success();

    let output = list(&store, &["--ready"]);
    assert!(
        output.contains(&ready) && !output.contains(&started) && !output.contains(&blocked),
        "{output}"
    );

    let output = list(&store, &["--blocked"]);
    assert!(
        !output.contains(": Ready") && output.contains(": Blocked"),
        "{output}"
    );

    let output = list(&store, &["--in-progress"]);
    assert!(
        output.contains(&format!("[>] {started}")) && !output.contains(&ready),
        "{output}"
    );
}

#[test]
fn list_filter_keeps_ancestors_for_context() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let parent = create(&store, &["Parent"]);
    let child = create(&store, &["Child", "--parent", &parent]);
    let sibling = create(&store, &["Sibling", "--parent", &parent]);
    dexrs(&store).args(["start", &child]).assert().success();

    let output = list(&store, &["--in-progress"]);

    assert_eq!(
        output,
        format!("[ ] {parent}: Parent\n└── [>] {child}: Child\n")
    );
    assert!(!output.contains(&sibling));
}

#[test]
fn list_positional_argument_selects_subtree_or_searches() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let parent = create(&store, &["Parent"]);
    let child = create(&store, &["Child", "--parent", &parent]);
    let other = create(&store, &["Other", "-d", "mentions needle here"]);

    let output = list(&store, &[&parent]);
    assert!(
        output.contains(&parent) && output.contains(&child) && !output.contains(&other),
        "{output}"
    );

    let output = list(&store, &["needle"]);
    assert!(
        output.contains(&other) && !output.contains(&parent),
        "{output}"
    );

    let output = list(&store, &["--query", "NEEDLE"]);
    assert!(output.contains(&other), "{output}");
}

#[test]
fn list_flat_drops_tree_prefixes_and_empty_list_says_so() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    assert_eq!(list(&store, &[]), "No tasks found.\n");

    let parent = create(&store, &["Parent"]);
    let child = create(&store, &["Child", "--parent", &parent]);

    let output = list(&store, &["--flat"]);
    let mut lines = [
        format!("[ ] {child}: Child"),
        format!("[ ] {parent}: Parent"),
    ];
    lines.sort();
    assert_eq!(output, format!("{}\n{}\n", lines[0], lines[1]));
}

#[test]
fn show_prints_several_tasks_with_context_sections() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let parent = create(&store, &["Parent", "-d", "parent details"]);
    let child = create(&store, &["Child", "--parent", &parent]);
    dexrs(&store)
        .args(["complete", &child, "-r", "child result"])
        .assert()
        .success();

    let output = dexrs(&store)
        .args(["show", &parent, &child])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).unwrap();

    assert!(
        output.contains(&format!("[ ] {parent}: Parent (1 subtask)  ← viewing")),
        "{output}"
    );
    assert!(
        output.contains(&format!("└── [x] {child}: Child")),
        "{output}"
    );
    assert!(
        output.contains("Description:\n  parent details"),
        "{output}"
    );
    assert!(
        output.contains(&format!("[x] {child}: Child  ← viewing")),
        "{output}"
    );
    assert!(output.contains("Result:\n  child result"), "{output}");
    assert!(output.contains("Created:"), "{output}");
}

#[test]
fn show_truncates_long_text_unless_full() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let long = "x".repeat(2000);
    let id = create(&store, &["Long", "-d", &long]);

    let short = dexrs(&store)
        .args(["show", &id])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let short = String::from_utf8(short).unwrap();
    assert!(
        !short.contains(&long) && short.contains("--full"),
        "{short}"
    );

    for flag in ["--full", "-f", "--expand", "-e"] {
        dexrs(&store).args(["show", &id, flag]).assert().success();
    }
    let full = dexrs(&store)
        .args(["show", &id, "--full"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8(full).unwrap().contains(&long));
}

#[test]
fn start_refuses_in_progress_task_unless_forced() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let id = create(&store, &["Claim me"]);
    dexrs(&store).args(["start", &id]).assert().success();
    let first_start = task(&store, &id).started_at.unwrap();

    dexrs(&store)
        .args(["start", &id])
        .assert()
        .failure()
        .stderr(
            predicates::str::contains("already in progress")
                .and(predicates::str::contains("--force")),
        );
    assert_eq!(task(&store, &id).started_at.unwrap(), first_start);

    std::thread::sleep(std::time::Duration::from_millis(5));
    dexrs(&store)
        .args(["start", &id, "--force"])
        .assert()
        .success();
    assert_ne!(task(&store, &id).started_at.unwrap(), first_start);
    dexrs(&store).args(["start", &id, "-f"]).assert().success();
}

#[test]
fn delete_with_subtasks_requires_force_and_removes_subtree() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let parent = create(&store, &["Parent"]);
    let child = create(&store, &["Child", "--parent", &parent]);
    let grandchild = create(&store, &["Grandchild", "--parent", &child]);
    let other = create(&store, &["Other"]);

    dexrs(&store)
        .args(["delete", &parent])
        .assert()
        .failure()
        .stderr(predicates::str::contains("2 subtasks").and(predicates::str::contains("--force")));
    assert_eq!(read_tasks(&store).len(), 4);

    dexrs(&store)
        .args(["delete", &parent, "-f"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "Deleted task {parent} and 2 subtasks"
        )));
    let remaining: Vec<String> = read_tasks(&store).into_iter().map(|task| task.id).collect();
    assert_eq!(remaining, vec![other]);
    assert!(!remaining.contains(&child) && !remaining.contains(&grandchild));
}

#[test]
fn status_is_the_default_command() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    create(&store, &["Only"]);

    let explicit = dexrs(&store)
        .arg("status")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let default = dexrs(&store).assert().success().get_output().stdout.clone();

    assert_eq!(default, explicit);
    assert!(!explicit.is_empty());
}

fn status(store: &std::path::Path, args: &[&str]) -> String {
    let output = dexrs(store)
        .arg("status")
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output).unwrap()
}

#[test]
fn status_on_empty_store_suggests_creating_a_task() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    assert_eq!(
        status(&store, &[]),
        "No tasks yet. Create one with: dex create \"Task name\" --description \"Details\"\n"
    );
}

fn dashboard_fixture(store: &std::path::Path) -> [String; 5] {
    let parent = create(store, &["Parent"]);
    let started = create(store, &["Child one", "--parent", &parent]);
    let ready = create(store, &["Child two", "--parent", &parent]);
    let blocked = create(store, &["Blocked", "--blocked-by", &started, "-p", "3"]);
    let done = create(store, &["Done already"]);
    dexrs(store)
        .args(["complete", &done, "-r", "finished"])
        .assert()
        .success();
    dexrs(store).args(["start", &started]).assert().success();
    [parent, started, ready, blocked, done]
}

#[test]
fn status_dashboard_groups_tasks_like_original_dex() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let [parent, started, ready, blocked, done] = dashboard_fixture(&store);

    let output = status(&store, &[]);

    let expected = format!(
        "  20%        1        1        2   \n\
         complete   active   ready   blocked\n\
         \n\
         In Progress (1)\n\
         ────────────────────\n\
         [ ] {parent}: Parent\n\
         └── [>] {started}: Child one\n\
         \n\
         Ready to Work (1)\n\
         ────────────────────\n\
         [ ] {parent}: Parent\n\
         └── [ ] {ready}: Child two\n\
         \n\
         Blocked (2)\n\
         ────────────────────\n\
         [ ] {parent}: Parent\n\
         [ ] {blocked} [p3] [B: {started}]: Blocked\n\
         \n\
         Recently Completed\n\
         ────────────────────\n\
         [x] {done}: Done already (0m ago)\n"
    );
    assert_eq!(output, expected);
}

#[test]
fn status_json_matches_original_shape() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let [parent, started, ready, blocked, done] = dashboard_fixture(&store);

    let json: serde_json::Value = serde_json::from_str(&status(&store, &["--json"])).unwrap();

    assert_eq!(
        json["stats"],
        serde_json::json!({
            "total": 5, "pending": 4, "completed": 1,
            "blocked": 2, "ready": 1, "inProgress": 1
        })
    );
    let ids = |key: &str| -> Vec<String> {
        json[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(ids("inProgressTasks"), vec![started.clone()]);
    assert_eq!(ids("readyTasks"), vec![ready]);
    assert_eq!(ids("blockedTasks"), vec![parent, blocked]);
    assert_eq!(ids("recentlyCompleted"), vec![done]);
    assert_eq!(
        json["inProgressTasks"][0]["blockedBy"],
        serde_json::json!([])
    );
}

#[test]
fn plan_creates_task_named_after_first_heading_with_file_as_description() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let plan = temp.path().join("feature-plan.md");
    let body = "# Add user auth\n\nIntro.\n\n## Requirements\n\n- JWT\n";
    std::fs::write(&plan, body).unwrap();

    let output = dexrs(&store)
        .args(["plan", plan.to_str().unwrap(), "-p", "2"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).unwrap();

    let task = read_tasks(&store).pop().unwrap();
    assert_eq!(task.name, "Add user auth");
    assert_eq!(task.description, body);
    assert_eq!(task.priority, 2);
    assert_eq!(
        output,
        format!(
            "Created task {} from plan\n[ ] {} [p2]: Add user auth\n",
            task.id, task.id
        )
    );
}

#[test]
fn plan_without_heading_uses_file_stem_and_links_parent() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let parent = create(&store, &["Epic"]);
    let plan = temp.path().join("rollout-steps.md");
    std::fs::write(&plan, "just a body\n").unwrap();

    dexrs(&store)
        .args(["plan", plan.to_str().unwrap(), "--parent", &parent])
        .assert()
        .success();

    let child = read_tasks(&store)
        .into_iter()
        .find(|task| task.id != parent)
        .unwrap();
    assert_eq!(child.name, "rollout-steps");
    assert_eq!(child.parent_id.as_deref(), Some(parent.as_str()));
    assert_eq!(task(&store, &parent).children, vec![child.id]);
}

#[test]
fn plan_with_missing_file_fails_without_creating_a_task() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");

    dexrs(&store)
        .args(["plan", "nope.md"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("nope.md"));

    assert!(read_tasks(&store).is_empty());
}

fn complete(store: &std::path::Path, id: &str) {
    dexrs(store)
        .args(["complete", id, "-r", &format!("result of {id}")])
        .assert()
        .success();
}

fn archive_records(store: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(store.join("archive.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn archive_moves_completed_task_to_compact_archive_record() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let keep = create(&store, &["Keep", "-d", "still open"]);
    let done = create(
        &store,
        &["Done", "-d", "finished work", "--blocked-by", &keep],
    );
    complete(&store, &done);

    dexrs(&store)
        .args(["archive", &done])
        .assert()
        .success()
        .stdout(predicates::str::starts_with(
            "Archived 1 task\n  Size reduction: ",
        ));

    let remaining = read_tasks(&store);
    assert_eq!(remaining.len(), 1);
    assert!(
        remaining[0].blocks.is_empty(),
        "blocks still reference the archived task"
    );
    let records = archive_records(&store);
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record["id"], done);
    assert_eq!(record["parent_id"], serde_json::Value::Null);
    assert_eq!(record["name"], "Done");
    assert_eq!(record["description"], "finished work");
    assert_eq!(record["result"], format!("result of {done}"));
    assert!(record["completed_at"].is_string());
    assert!(record["archived_at"].is_string());
    assert_eq!(record["metadata"], serde_json::Value::Null);
    assert_eq!(record["archived_children"], serde_json::json!([]));
    assert_eq!(record.as_object().unwrap().len(), 9);
}

#[test]
fn archive_refuses_incomplete_task_descendant_or_ancestor() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let open = create(&store, &["Open"]);
    let parent = create(&store, &["Parent"]);
    let child = create(&store, &["Child", "--parent", &parent]);
    let done_child = create(&store, &["Done child", "--parent", &open]);
    complete(&store, &done_child);

    dexrs(&store)
        .args(["archive", &open])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not completed"));

    dexrs(&store)
        .args(["complete", &parent, "-r", "forced", "--force"])
        .assert()
        .success();
    dexrs(&store)
        .args(["archive", &parent])
        .assert()
        .failure()
        .stderr(predicates::str::contains(&child));

    dexrs(&store)
        .args(["archive", &done_child])
        .assert()
        .failure()
        .stderr(predicates::str::contains("incomplete ancestor"));

    dexrs(&store)
        .args(["archive", "zzzzzzzz"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not found"));

    assert_eq!(read_tasks(&store).len(), 4);
    assert!(archive_records(&store).is_empty());
}

#[test]
fn archive_subtree_writes_a_record_per_task_with_child_summaries() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let parent = create(&store, &["Parent"]);
    let child = create(&store, &["Child", "--parent", &parent]);
    let grandchild = create(&store, &["Grandchild", "--parent", &child]);
    for id in [&grandchild, &child, &parent] {
        complete(&store, id);
    }

    dexrs(&store)
        .args(["archive", &parent])
        .assert()
        .success()
        .stdout(predicates::str::starts_with(
            "Archived 3 tasks\n  Subtasks: 2\n",
        ));

    assert!(read_tasks(&store).is_empty());
    let records = archive_records(&store);
    let by_id = |id: &str| {
        records
            .iter()
            .find(|record| record["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(records.len(), 3);
    assert_eq!(by_id(&child)["parent_id"], parent);
    assert_eq!(
        by_id(&parent)["archived_children"],
        serde_json::json!([{
            "id": child, "name": "Child", "description": "", "result": format!("result of {child}")
        }])
    );
    assert_eq!(
        by_id(&grandchild)["archived_children"],
        serde_json::json!([])
    );
}

#[test]
fn archive_completed_skips_tasks_under_open_parents_and_honours_except() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let open = create(&store, &["Open"]);
    let done_a = create(&store, &["Done A"]);
    let done_b = create(&store, &["Done B"]);
    let kept = create(&store, &["Kept"]);
    let under_open = create(&store, &["Under open", "--parent", &open]);
    for id in [&done_a, &done_b, &kept, &under_open] {
        complete(&store, id);
    }

    dexrs(&store)
        .args(["archive", "--completed", "--except", &kept])
        .assert()
        .success()
        .stdout(predicates::str::starts_with(
            "Archived 2 tasks (2 root tasks)\n",
        ));

    let remaining: Vec<String> = read_tasks(&store).into_iter().map(|task| task.id).collect();
    assert!(
        remaining.contains(&open) && remaining.contains(&kept) && remaining.contains(&under_open)
    );
    assert_eq!(remaining.len(), 3);
    assert_eq!(archive_records(&store).len(), 2);

    dexrs(&store)
        .args(["archive", "--completed"])
        .assert()
        .success()
        .stdout(predicates::str::starts_with(
            "Archived 1 task (1 root task)\n",
        ));
    dexrs(&store)
        .args(["archive", "--completed"])
        .assert()
        .success()
        .stdout("No tasks found to archive.\n");
}

#[test]
fn archive_older_than_filters_by_completion_age_and_validates_duration() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let recent = create(&store, &["Recent"]);
    let old = create(&store, &["Old"]);
    complete(&store, &recent);
    complete(&store, &old);
    let mut tasks = read_tasks(&store);
    tasks
        .iter_mut()
        .find(|task| task.id == old)
        .unwrap()
        .completed_at = Some("2020-01-01T00:00:00Z".to_string());
    std::fs::write(
        store.join("tasks.jsonl"),
        dexrs::task::serialize_tasks_jsonl(&tasks).unwrap(),
    )
    .unwrap();

    dexrs(&store)
        .args(["archive", "--older-than", "5x"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Invalid duration"));

    dexrs(&store)
        .args(["archive", "--older-than", "60d"])
        .assert()
        .success()
        .stdout(predicates::str::starts_with(
            "Archived 1 task (1 root task)\n",
        ));
    assert_eq!(read_tasks(&store)[0].id, recent);
    assert_eq!(archive_records(&store)[0]["id"], old);

    for duration in ["1w", "1m"] {
        dexrs(&store)
            .args(["archive", "--older-than", duration])
            .assert()
            .success()
            .stdout("No tasks found to archive.\n");
    }
}

#[test]
fn archive_dry_run_reports_without_changing_files() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let done = create(&store, &["Done"]);
    complete(&store, &done);
    let before = std::fs::read_to_string(store.join("tasks.jsonl")).unwrap();

    dexrs(&store)
        .args(["archive", "--completed", "--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Would archive 1 task"));

    assert_eq!(
        std::fs::read_to_string(store.join("tasks.jsonl")).unwrap(),
        before
    );
    assert!(!store.join("archive.jsonl").exists());
}

#[test]
fn list_archived_shows_archive_newest_first_and_show_reads_archived_task() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let first = create(&store, &["First"]);
    let parent = create(&store, &["Parent"]);
    let child = create(&store, &["Child", "--parent", &parent]);
    for id in [&first, &child, &parent] {
        complete(&store, id);
    }
    dexrs(&store).args(["archive", &first]).assert().success();
    dexrs(&store).args(["archive", &parent]).assert().success();

    let output = list(&store, &["--archived"]);
    assert!(
        output.starts_with("Showing 3 archived tasks\n\n"),
        "{output}"
    );
    let lines: Vec<&str> = output.lines().skip(2).collect();
    assert_eq!(lines.len(), 3);
    assert!(
        lines[2] == format!("[x] {first}: First (ARCHIVED)"),
        "{output}"
    );
    assert!(
        lines.contains(&format!("[x] {parent}: Parent (1 subtask) (ARCHIVED)").as_str()),
        "{output}"
    );

    let json: serde_json::Value =
        serde_json::from_str(&list(&store, &["--archived", "--json"])).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 3);
    assert!(json[0]["archived_at"].is_string());

    let shown = dexrs(&store)
        .args(["show", &first])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let shown = String::from_utf8(shown).unwrap();
    assert!(shown.starts_with(&format!("[x] {first}: First (ARCHIVED)\n\nDescription:\n  (no description)\n\nResult:\n  result of {first}\n\nCompleted: ")), "{shown}");
    assert!(shown.contains("\nArchived:  "), "{shown}");

    assert_eq!(list(&store, &["--archived", "--flat"]).lines().count(), 5);
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(
        list(&empty.path().join("store"), &["--archived"]),
        "No archived tasks found.\n"
    );
}

#[test]
fn list_marks_completed_tasks_with_relative_age() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let done = create(&store, &["Done"]);
    complete(&store, &done);

    assert_eq!(
        list(&store, &["--completed"]),
        format!("[x] {done}: Done (0m ago)\n")
    );
}

fn write_metadata(store: &std::path::Path, id: &str, metadata: serde_json::Value) {
    let mut tasks = read_tasks(store);
    tasks
        .iter_mut()
        .find(|task| task.id == id)
        .unwrap()
        .metadata = Some(metadata);
    std::fs::write(
        store.join("tasks.jsonl"),
        dexrs::task::serialize_tasks_jsonl(&tasks).unwrap(),
    )
    .unwrap();
}

#[test]
fn list_commit_finds_task_by_sha_prefix_including_completed() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let sha = git_repo_with_commit(temp.path());
    let linked = create(&store, &["Linked"]);
    let other = create(&store, &["Other"]);
    dexrs(&store)
        .current_dir(temp.path())
        .args(["complete", &linked, "-r", "done", "--commit", &sha])
        .assert()
        .success();

    let output = list(&store, &["--commit", &sha[..6].to_uppercase()]);

    assert!(
        output.contains(&format!("[x] {linked}: Linked (")),
        "{output}"
    );
    assert!(!output.contains(&other), "{output}");
    assert_eq!(list(&store, &["--commit", "ffffff"]), "No tasks found.\n");
}

#[test]
fn list_issue_finds_task_by_github_issue_and_shows_indicator() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let linked = create(&store, &["Linked"]);
    let other = create(&store, &["Other", "-p", "2"]);
    write_metadata(
        &store,
        &linked,
        serde_json::json!({"github": {"issueNumber": 42, "issueUrl": "https://example.invalid/42"}}),
    );

    assert_eq!(
        list(&store, &["--issue", "42"]),
        format!("[ ] {linked} [GH-42]: Linked\n")
    );
    assert_eq!(list(&store, &["--issue", "7"]), "No tasks found.\n");
    assert!(list(&store, &[]).contains(&format!("[ ] {other} [p2]: Other")));
}

#[test]
fn completion_generates_script_named_after_invoked_binary() {
    for (binary, shell, marker) in [
        ("dexrs", "zsh", "#compdef dexrs"),
        ("dex", "zsh", "#compdef dex"),
        ("dex", "bash", "complete -F _dex"),
        ("dex", "fish", "complete -c dex"),
    ] {
        assert_cmd::Command::cargo_bin(binary)
            .unwrap()
            .args(["completion", shell])
            .assert()
            .success()
            .stdout(predicates::str::contains(marker));
    }

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .args(["completion", "powershell7"])
        .assert()
        .failure();
}

fn git_repo(dir: &std::path::Path) {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(["init", "-q", "-b", "main", "."])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn version_flag_prints_version() {
    let temp = tempfile::tempdir().unwrap();
    for flag in ["--version", "-V"] {
        bare(temp.path())
            .arg(flag)
            .assert()
            .success()
            .stdout(predicates::str::contains(env!("CARGO_PKG_VERSION")));
    }
}

#[test]
fn dir_global_prints_dex_home_from_env_or_xdg_or_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");

    bare(temp.path())
        .args(["dir", "--global"])
        .assert()
        .success()
        .stdout(format!("{}\n", temp.path().join("dex-home").display()));

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env_remove("DEX_HOME")
        .env("XDG_CONFIG_HOME", temp.path().join("xdg"))
        .env("HOME", &home)
        .args(["dir", "--global"])
        .assert()
        .success()
        .stdout(format!("{}\n", temp.path().join("xdg/dex").display()));

    assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .env_remove("DEX_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", &home)
        .args(["dir", "--global"])
        .assert()
        .success()
        .stdout(format!("{}\n", home.join(".config/dex").display()));
}

#[test]
fn init_writes_default_global_config_once() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("dex-home/dex.toml");

    bare(temp.path())
        .args(["init", "-y"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "Created config file at {}",
            config.display()
        )));
    let contents = std::fs::read_to_string(&config).unwrap();
    assert!(
        contents.contains("[storage]") && contents.contains("engine = \"file\""),
        "{contents}"
    );
    assert!(!temp.path().join(".dex").exists());

    bare(temp.path())
        .arg("init")
        .assert()
        .failure()
        .stderr(predicates::str::contains("already exists"));

    let other = temp.path().join("elsewhere");
    bare(temp.path())
        .args(["init", "--config-dir", other.to_str().unwrap()])
        .assert()
        .success();
    assert!(other.join("dex.toml").is_file());
}

#[test]
fn storage_path_resolution_follows_original_precedence() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git_repo(&repo);
    let dex_home = temp.path().join("dex-home");
    std::fs::create_dir_all(&dex_home).unwrap();
    let dir = |cmd: &mut assert_cmd::Command| -> String {
        let out = cmd
            .arg("dir")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        String::from_utf8(out).unwrap().trim().to_string()
    };

    assert_eq!(
        dir(bare(temp.path()).current_dir(&repo)),
        repo.canonicalize()
            .unwrap()
            .join(".dex")
            .display()
            .to_string()
    );

    std::fs::write(
        dex_home.join("dex.toml"),
        "[storage]\nengine = \"file\"\n[storage.file]\npath = \"/tmp/global-store\"\n",
    )
    .unwrap();
    assert_eq!(
        dir(bare(temp.path()).current_dir(&repo)),
        "/tmp/global-store"
    );

    std::fs::create_dir_all(repo.join(".dex")).unwrap();
    std::fs::write(
        repo.join(".dex/config.toml"),
        "[storage.file]\npath = \"/tmp/local-store\"\n",
    )
    .unwrap();
    assert_eq!(
        dir(bare(temp.path()).current_dir(&repo)),
        "/tmp/local-store"
    );

    assert_eq!(
        dir(bare(temp.path())
            .current_dir(&repo)
            .env("DEX_STORAGE_PATH", "/tmp/env-store")),
        "/tmp/local-store",
        "config path wins over the env var, as in original dex"
    );
    assert_eq!(
        dir(bare(temp.path())
            .current_dir(&repo)
            .args(["--storage-path", "/tmp/flag-store"])),
        "/tmp/flag-store"
    );
    assert_eq!(
        dir(bare(temp.path())
            .current_dir(&repo)
            .args(["--storage-path=/tmp/eq-store"])),
        "/tmp/eq-store"
    );

    let alt = temp.path().join("alt.toml");
    std::fs::write(&alt, "[storage.file]\npath = \"/tmp/alt-store\"\n").unwrap();
    std::fs::remove_file(repo.join(".dex/config.toml")).unwrap();
    assert_eq!(
        dir(bare(temp.path())
            .current_dir(&repo)
            .args(["--config", alt.to_str().unwrap()])),
        "/tmp/alt-store"
    );

    std::fs::write(
        dex_home.join("dex.toml"),
        "[storage]\nengine = \"sqlite\"\n",
    )
    .unwrap();
    bare(temp.path())
        .current_dir(&repo)
        .arg("dir")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "Unsupported storage engine: sqlite",
        ));
}

#[test]
fn centralized_mode_keys_store_by_remote_or_path_hash() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git_repo(&repo);
    let dex_home = temp.path().join("dex-home");
    std::fs::create_dir_all(&dex_home).unwrap();
    std::fs::write(
        dex_home.join("dex.toml"),
        "[storage.file]\nmode = \"centralized\"\n",
    )
    .unwrap();
    let dir = |cmd: &mut assert_cmd::Command| -> String {
        let out = cmd
            .arg("dir")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        String::from_utf8(out).unwrap().trim().to_string()
    };

    let hashed = dir(bare(temp.path()).current_dir(&repo));
    let prefix = dex_home.join("projects/path-").display().to_string();
    assert!(
        hashed.starts_with(&prefix) && hashed.len() == prefix.len() + 12,
        "{hashed}"
    );

    for (url, key) in [
        ("git@github.com:acme/widgets.git", "github.com-acme-widgets"),
        (
            "https://github.com/acme/widgets.git",
            "github.com-acme-widgets",
        ),
    ] {
        std::process::Command::new("git")
            .current_dir(&repo)
            .args(["remote", "remove", "origin"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(&repo)
            .args(["remote", "add", "origin", url])
            .output()
            .unwrap();
        assert_eq!(
            dir(bare(temp.path()).current_dir(&repo)),
            dex_home.join("projects").join(key).display().to_string()
        );
    }
}

#[test]
fn config_command_gets_sets_unsets_and_lists() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git_repo(&repo);
    let config = |args: &[&str]| -> assert_cmd::assert::Assert {
        bare(temp.path())
            .current_dir(&repo)
            .arg("config")
            .args(args)
            .assert()
    };

    config(&["storage.file.mode"])
        .success()
        .stdout("(not set)\n");
    config(&["storage.file.mode=centralized"])
        .success()
        .stdout("Set storage.file.mode = centralized in global config\n");
    config(&["storage.file.mode"])
        .success()
        .stdout("centralized\n");
    assert!(
        std::fs::read_to_string(temp.path().join("dex-home/dex.toml"))
            .unwrap()
            .contains("mode = \"centralized\"")
    );

    config(&["--local", "sync.github.enabled=yes"])
        .success()
        .stdout("Set sync.github.enabled = true in local config\n");
    assert!(
        std::fs::read_to_string(repo.join(".dex/config.toml"))
            .unwrap()
            .contains("enabled = true")
    );
    config(&["--global", "sync.github.enabled=false"]).success();
    config(&["sync.github.enabled"]).success().stdout("true\n");

    config(&["--list"]).success().stdout(
        "Configuration:\n\nstorage.file.mode = centralized [global]\nsync.github.enabled = true [local]\n",
    );

    config(&["--unset", "storage.file.mode"])
        .success()
        .stdout("Unset storage.file.mode in global config\n");
    config(&["--unset", "storage.file.mode"])
        .success()
        .stdout("Key storage.file.mode was not set in global config\n");

    config(&["storage.file.mode=weird"])
        .failure()
        .stderr(predicates::str::contains(
            "Valid options: in-repo, centralized",
        ));
    config(&["sync.github.enabled=maybe"])
        .failure()
        .stderr(predicates::str::contains("Invalid boolean value"));
    config(&["nope.key"])
        .failure()
        .stderr(predicates::str::contains("Unknown config key: nope.key"));
    config(&["--global", "--local", "storage.file.mode"]).failure();
    config(&[])
        .failure()
        .stderr(predicates::str::contains("Missing config key"));
}
