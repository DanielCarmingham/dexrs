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
