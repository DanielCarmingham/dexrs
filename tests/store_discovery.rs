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

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
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
        .expect("git should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn dir_output(cwd: &std::path::Path, home: &std::path::Path, path_env: Option<&str>) -> String {
    let mut command = assert_cmd::Command::cargo_bin("dexrs").unwrap();
    command
        .current_dir(cwd)
        .env_remove("DEX_STORAGE_PATH")
        .env("DEX_HOME", home)
        .arg("dir");
    if let Some(path) = path_env {
        command.env("PATH", path);
    }
    let output = command.assert().success().get_output().stdout.clone();
    String::from_utf8(output).unwrap().trim().to_string()
}

#[test]
fn dir_finds_the_repo_root_without_spawning_git() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let nested = repo.join("a").join("b");
    std::fs::create_dir_all(&nested).unwrap();
    git(&repo, &["init", "-q", "."]);
    let empty_bin = temp.path().join("empty-bin");
    std::fs::create_dir_all(&empty_bin).unwrap();

    let printed = dir_output(
        &nested,
        &temp.path().join("home"),
        Some(empty_bin.to_str().unwrap()),
    );

    assert_eq!(
        printed,
        repo.canonicalize()
            .unwrap()
            .join(".dex")
            .display()
            .to_string()
    );
}

#[test]
fn dir_treats_a_worktree_git_file_as_the_root() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main", "."]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let worktree = temp.path().join("wt");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            worktree.to_str().unwrap(),
            "-b",
            "feature",
        ],
    );
    assert!(
        worktree.join(".git").is_file(),
        "a linked worktree has a .git file, not a directory"
    );
    let inside = worktree.join("src");
    std::fs::create_dir_all(&inside).unwrap();

    let printed = dir_output(&inside, &temp.path().join("home"), None);

    assert_eq!(
        printed,
        worktree
            .canonicalize()
            .unwrap()
            .join(".dex")
            .display()
            .to_string()
    );
}
