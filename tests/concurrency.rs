use dexrs::store::{read_tasks, transact};
use dexrs::task::Task;
use std::process::{Command, Stdio};

fn task(id: &str) -> Task {
    Task {
        id: id.to_string(),
        parent_id: None,
        name: format!("Task {id}"),
        description: None,
        priority: None,
        completed: false,
        result: None,
        metadata: None,
        created_at: None,
        updated_at: None,
        started_at: None,
        completed_at: None,
        blocked_by: Vec::new(),
        blocks: Vec::new(),
        children: Vec::new(),
    }
}

#[test]
fn transaction_rolls_back_on_error() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join(".dex");
    std::fs::create_dir(&store).unwrap();
    std::fs::write(
        store.join("tasks.jsonl"),
        serde_json::to_string(&task("base0001")).unwrap() + "\n",
    )
    .unwrap();

    let result: anyhow::Result<()> = transact(&store, |tasks| {
        tasks.push(task("bad00001"));
        anyhow::bail!("stop");
    });

    assert!(result.is_err());
    let tasks = read_tasks(&store).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "base0001");
}

#[test]
fn concurrent_transactions_keep_all_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join(".dex");
    std::fs::create_dir(&store).unwrap();
    std::fs::write(store.join("tasks.jsonl"), "").unwrap();

    std::thread::scope(|scope| {
        for index in 0..24 {
            let store = &store;
            scope.spawn(move || {
                transact(store, |tasks| {
                    tasks.push(task(&format!("task{index:04}")));
                    Ok(())
                })
                .unwrap();
            });
        }
    });

    let tasks = read_tasks(&store).unwrap();
    assert_eq!(tasks.len(), 24);
    for index in 0..24 {
        assert!(
            tasks
                .iter()
                .any(|task| task.id == format!("task{index:04}"))
        );
    }
}

#[test]
fn concurrent_create_processes_keep_all_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join(".dex");

    let mut children = Vec::new();
    for index in 0..20 {
        let mut command = Command::new(assert_cmd::cargo::cargo_bin("dexrs"));
        children.push(
            command
                .env("DEX_STORAGE_PATH", &store)
                .args(["create", &format!("Process task {index}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
    }

    for mut child in children {
        assert!(child.wait().unwrap().success());
    }

    let tasks = read_tasks(&store).unwrap();
    assert_eq!(tasks.len(), 20);
    for index in 0..20 {
        assert!(
            tasks
                .iter()
                .any(|task| task.name == format!("Process task {index}"))
        );
    }
}
