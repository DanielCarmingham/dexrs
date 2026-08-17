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
