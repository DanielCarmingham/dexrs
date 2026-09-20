use std::process::Command;

#[test]
fn dir_uses_dex_storage_path_when_set() {
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("cwd");
    let store = temp.path().join("custom-store");
    std::fs::create_dir(&cwd).unwrap();

    let output = assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .current_dir(&cwd)
        .env("DEX_HOME", temp.path().join("dex-home"))
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
fn dir_uses_git_root_dex_store_inside_repo() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let nested = repo.join("nested");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&nested).unwrap();
    Command::new("git")
        .arg("init")
        .arg(&repo)
        .output()
        .expect("git init should run");

    let output = assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .current_dir(&nested)
        .env("DEX_HOME", temp.path().join("dex-home"))
        .env_remove("DEX_STORAGE_PATH")
        .arg("dir")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!("{}\n", repo.canonicalize().unwrap().join(".dex").display())
    );
}

#[test]
fn dir_uses_home_config_fallback_outside_git_repo() {
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("cwd");
    let home = temp.path().join("home");
    std::fs::create_dir(&cwd).unwrap();
    std::fs::create_dir(&home).unwrap();

    let output = assert_cmd::Command::cargo_bin("dexrs")
        .unwrap()
        .current_dir(&cwd)
        .env_remove("DEX_STORAGE_PATH")
        .env_remove("DEX_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", &home)
        .arg("dir")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!("{}\n", home.join(".config/dex/local").display())
    );
}
