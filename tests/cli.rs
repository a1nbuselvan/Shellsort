use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

fn run(home: &TempDir, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shellsort"));
    command.args(args).env("HOME", home.path());
    #[cfg(target_os = "windows")]
    command
        .env("USERPROFILE", home.path())
        .env("LOCALAPPDATA", home.path().join("AppData/Local"))
        .env("APPDATA", home.path().join("AppData/Roaming"));
    #[cfg(target_os = "linux")]
    command.env("XDG_DATA_HOME", home.path().join(".local/share"));
    command.output().expect("launch shellsort CLI")
}

fn prepare_app_data_parent(home: &TempDir) {
    fs::create_dir_all(journal_path(home).parent().unwrap().parent().unwrap())
        .expect("create isolated application-data parent");
}

fn journal_path(home: &TempDir) -> PathBuf {
    #[cfg(target_os = "macos")]
    let base = home
        .path()
        .join("Library/Application Support/app.shellsort.desktop");
    #[cfg(target_os = "windows")]
    let base = home.path().join("AppData/Local/app.shellsort.desktop");
    #[cfg(target_os = "linux")]
    let base = home.path().join(".local/share/app.shellsort.desktop");
    base.join("operations.json")
}

#[test]
fn first_run_history_and_recover_do_not_create_app_data_files() {
    let home = TempDir::new().unwrap();
    let output = run(&home, &["history", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        Value::Array(vec![])
    );

    let recovery = run(&home, &["recover", "--json"]);
    assert!(
        recovery.status.success(),
        "{}",
        String::from_utf8_lossy(&recovery.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&recovery.stdout).unwrap(),
        Value::Array(vec![])
    );
    assert!(!journal_path(&home).parent().unwrap().exists());
}

#[test]
fn empty_existing_app_data_directory_remains_empty_for_history_and_recover() {
    let home = TempDir::new().unwrap();
    let app_data = journal_path(&home).parent().unwrap().to_path_buf();
    fs::create_dir_all(&app_data).unwrap();

    for args in [&["history", "--json"][..], &["recover", "--json"][..]] {
        let output = run(&home, args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            Value::Array(vec![])
        );
    }
    assert_eq!(fs::read_dir(&app_data).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn history_rejects_a_symlinked_app_data_parent_without_touching_target() {
    use std::os::unix::fs::symlink;

    let home = TempDir::new().unwrap();
    let target = TempDir::new().unwrap();
    prepare_app_data_parent(&home);
    symlink(target.path(), journal_path(&home).parent().unwrap()).unwrap();

    let output = run(&home, &["history", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(serde_json::from_slice::<Value>(&output.stdout).is_ok());
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
}

fn file(root: &Path, relative: &str, contents: &[u8]) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("create disposable test folder");
    fs::write(&path, contents).expect("write disposable test file");
    path
}

#[test]
fn default_command_is_read_only_and_supports_spaces_and_unicode() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let path = file(root.path(), "Notes & Études.pdf", b"safe");

    let output = run(&home, &[root.path().to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Notes & Études.pdf"));
    assert!(stdout.contains("Documents/Notes & Études.pdf"));
    assert!(path.exists(), "default preview must not move files");
    assert!(!root.path().join("Documents").exists());
}

#[test]
fn json_preview_is_valid_json_without_progress_text() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    file(root.path(), "photo.JPG", b"image");

    let output = run(&home, &[root.path().to_str().unwrap(), "--json"]);
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["entries"][0]["category"], "images");
    assert_eq!(json["entries"][0]["destination"], "Images/photo.JPG");
}

#[test]
#[cfg(target_os = "macos")]
fn cli_excludes_macos_metadata_from_preview_and_organization() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let metadata = file(root.path(), ".DS_Store", b"keep metadata");
    let sidecar = file(root.path(), "._notes.txt", b"keep sidecar");
    let ordinary = file(root.path(), "notes.txt", b"organize this");

    let preview = run(&home, &[root.path().to_str().unwrap(), "--json"]);
    assert!(preview.status.success());
    let json: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(json["entries"].as_array().unwrap().len(), 1);
    assert_eq!(json["entries"][0]["source"], "notes.txt");

    let organized = run(&home, &["organize", root.path().to_str().unwrap(), "--yes"]);
    assert!(
        organized.status.success(),
        "{}",
        String::from_utf8_lossy(&organized.stderr)
    );
    assert!(metadata.exists() && sidecar.exists());
    assert!(!ordinary.exists());
    assert_eq!(
        fs::read(root.path().join("Documents/notes.txt")).unwrap(),
        b"organize this"
    );
    assert!(!root.path().join("Other/.DS_Store").exists());
    assert!(!root.path().join("Documents/._notes.txt").exists());
}

#[test]
fn duplicates_command_reports_exact_matches_without_mutating_files() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let first = file(root.path(), "one.bin", b"same bytes");
    let second = file(root.path(), "nested/two.bin", b"same bytes");

    let output = run(
        &home,
        &["duplicates", root.path().to_str().unwrap(), "--json"],
    );
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["groups"][0]["files"].as_array().unwrap().len(), 2);
    assert!(first.exists() && second.exists());
}

#[test]
fn invalid_path_returns_nonzero_and_json_error_in_json_mode() {
    let home = TempDir::new().unwrap();
    let missing = home.path().join("missing folder");
    let output = run(&home, &[missing.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(2));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json["error"].as_str().unwrap().contains("folder"));
}

#[test]
fn conflicts_are_shown_and_never_replaced() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    file(root.path(), "report.pdf", b"source");
    file(root.path(), "Documents/report.pdf", b"existing");

    let preview = run(&home, &[root.path().to_str().unwrap()]);
    assert!(preview.status.success());
    assert!(
        String::from_utf8(preview.stdout)
            .unwrap()
            .contains("CONFLICT")
    );

    let organize = run(&home, &["organize", root.path().to_str().unwrap(), "--yes"]);
    assert!(organize.status.success());
    assert_eq!(fs::read(root.path().join("report.pdf")).unwrap(), b"source");
    assert_eq!(
        fs::read(root.path().join("Documents/report.pdf")).unwrap(),
        b"existing"
    );
}

#[test]
#[cfg(target_os = "macos")]
fn organize_refuses_noninteractive_confirmation_then_moves_after_yes() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let source = file(root.path(), "annual report.pdf", b"document");

    let refused = run(&home, &["organize", root.path().to_str().unwrap()]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(source.exists());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("pass --yes"));

    let confirmed = run(&home, &["organize", root.path().to_str().unwrap(), "--yes"]);
    assert!(
        confirmed.status.success(),
        "{}",
        String::from_utf8_lossy(&confirmed.stderr)
    );
    assert!(!source.exists());
    assert_eq!(
        fs::read(root.path().join("Documents/annual report.pdf")).unwrap(),
        b"document"
    );
    let journal = journal_path(&home);
    let records: Value = serde_json::from_slice(&fs::read(journal).unwrap()).unwrap();
    assert_eq!(records["operations"][0]["status"], "completed");
}

#[test]
#[cfg(target_os = "macos")]
fn organize_initializes_a_fresh_app_data_tree_before_moving_files() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let source = file(root.path(), "fresh.pdf", b"contents");
    assert!(!journal_path(&home).parent().unwrap().exists());

    let output = run(&home, &["organize", root.path().to_str().unwrap(), "--yes"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!source.exists());
    assert_eq!(
        fs::read(root.path().join("Documents/fresh.pdf")).unwrap(),
        b"contents"
    );
    let journal = journal_path(&home);
    let records: Value = serde_json::from_slice(&fs::read(journal).unwrap()).unwrap();
    assert_eq!(records["operations"][0]["files"][0]["outcome"], "completed");
}

#[test]
fn organize_rejects_invalid_app_data_parent_before_moving_files() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let source = file(root.path(), "safe.pdf", b"contents");
    #[cfg(target_os = "macos")]
    let invalid_parent = home.path().join("Library");
    #[cfg(target_os = "windows")]
    let invalid_parent = home.path().join("AppData");
    #[cfg(target_os = "linux")]
    let invalid_parent = home.path().join(".local");
    fs::write(invalid_parent, b"not a directory").unwrap();

    let output = run(&home, &["organize", root.path().to_str().unwrap(), "--yes"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(source.exists());
    assert!(!root.path().join("Documents/safe.pdf").exists());
}

#[cfg(unix)]
#[test]
fn organize_rejects_symlinked_app_data_before_moving_files() {
    use std::os::unix::fs::symlink;

    let home = TempDir::new().unwrap();
    let target = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let source = file(root.path(), "safe.pdf", b"contents");
    prepare_app_data_parent(&home);
    symlink(target.path(), journal_path(&home).parent().unwrap()).unwrap();

    let output = run(&home, &["organize", root.path().to_str().unwrap(), "--yes"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(source.exists());
    assert!(!root.path().join("Documents/safe.pdf").exists());
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
}

#[test]
#[cfg(target_os = "macos")]
fn history_and_undo_reuse_the_shared_journal_and_engine() {
    let home = TempDir::new().unwrap();
    prepare_app_data_parent(&home);
    let root = TempDir::new().unwrap();
    let source = file(root.path(), "notes.txt", b"notes");
    let organized = run(&home, &["organize", root.path().to_str().unwrap(), "--yes"]);
    assert!(
        organized.status.success(),
        "{}",
        String::from_utf8_lossy(&organized.stderr)
    );

    let history = run(&home, &["history", "--json"]);
    assert!(
        history.status.success(),
        "{}",
        String::from_utf8_lossy(&history.stderr)
    );
    let records: Value = serde_json::from_slice(&history.stdout).unwrap();
    let operation_id = records[0]["operation_id"].as_str().unwrap();
    let recovery = run(&home, &["recover", "--json"]);
    assert!(recovery.status.success());
    assert!(root.path().join("Documents/notes.txt").exists());
    let refused_undo = run(
        &home,
        &["undo", root.path().to_str().unwrap(), operation_id],
    );
    assert_eq!(refused_undo.status.code(), Some(2));
    assert!(root.path().join("Documents/notes.txt").exists());
    let undo = run(
        &home,
        &["undo", root.path().to_str().unwrap(), operation_id, "--yes"],
    );
    assert!(
        undo.status.success(),
        "{}",
        String::from_utf8_lossy(&undo.stderr)
    );
    assert_eq!(fs::read(source).unwrap(), b"notes");
}

#[test]
#[cfg(target_os = "macos")]
fn two_file_organization_undo_restores_both_sources_and_history_entries() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let notes = file(root.path(), "Example Folder/notes.txt", b"notes");
    let copy = file(root.path(), "copy.txt", b"copy");
    let organized = run(&home, &["organize", root.path().to_str().unwrap(), "--yes"]);
    assert!(
        organized.status.success(),
        "{}",
        String::from_utf8_lossy(&organized.stderr)
    );

    let history = run(&home, &["history", "--json"]);
    assert!(history.status.success());
    let operations: Value = serde_json::from_slice(&history.stdout).unwrap();
    let operation_id = operations[0]["operation_id"].as_str().unwrap();
    let undone = run(
        &home,
        &["undo", root.path().to_str().unwrap(), operation_id, "--yes"],
    );
    assert!(
        undone.status.success(),
        "{}",
        String::from_utf8_lossy(&undone.stderr)
    );
    let output = String::from_utf8(undone.stdout).unwrap();
    assert!(output.contains("0 currently moved, 2 reverted, 0 skipped, 0 failed"));
    assert!(notes.exists() && copy.exists());
    assert!(!root.path().join("Documents/notes.txt").exists());
    assert!(!root.path().join("Documents/copy.txt").exists());

    let history = run(&home, &["history", "--json"]);
    let operations: Value = serde_json::from_slice(&history.stdout).unwrap();
    assert_eq!(operations[0]["status"], "reverted");
    let outcomes = operations[0]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["outcome"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outcomes, vec!["reverted", "reverted"]);
}

#[test]
#[cfg(target_os = "macos")]
fn partial_undo_conflict_is_reported_and_can_be_retried_safely() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let source = file(root.path(), "report.pdf", b"original");
    let organized = run(&home, &["organize", root.path().to_str().unwrap(), "--yes"]);
    assert!(organized.status.success());
    file(root.path(), "report.pdf", b"keep existing file");

    let history = run(&home, &["history", "--json"]);
    let operations: Value = serde_json::from_slice(&history.stdout).unwrap();
    let operation_id = operations[0]["operation_id"].as_str().unwrap();
    let partial = run(
        &home,
        &["undo", root.path().to_str().unwrap(), operation_id, "--yes"],
    );
    assert_eq!(partial.status.code(), Some(2));
    let partial_output = String::from_utf8(partial.stdout).unwrap();
    assert!(partial_output.contains("PartiallyReverted"));
    assert!(partial_output.contains("1 currently moved, 0 reverted, 0 skipped, 0 failed"));
    assert_eq!(fs::read(&source).unwrap(), b"keep existing file");
    assert_eq!(
        fs::read(root.path().join("Documents/report.pdf")).unwrap(),
        b"original"
    );

    fs::remove_file(&source).unwrap();
    let retried = run(
        &home,
        &["undo", root.path().to_str().unwrap(), operation_id, "--yes"],
    );
    assert!(
        retried.status.success(),
        "{}",
        String::from_utf8_lossy(&retried.stderr)
    );
    assert!(source.exists());
    assert!(!root.path().join("Documents/report.pdf").exists());
    assert!(
        String::from_utf8(retried.stdout)
            .unwrap()
            .contains("0 currently moved, 1 reverted, 0 skipped, 0 failed")
    );

    let history = run(&home, &["history", "--json"]);
    let operations: Value = serde_json::from_slice(&history.stdout).unwrap();
    assert_eq!(operations[0]["status"], "reverted");
    assert_eq!(operations[0]["files"][0]["outcome"], "reverted");
    assert!(operations[0]["files"][0]["undo_error"].is_null());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn organization_is_explicitly_refused_off_macos_without_touching_fixture_or_journal() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let source = file(root.path(), "safe.pdf", b"preserve this");

    let output = run(&home, &["organize", root.path().to_str().unwrap(), "--yes"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("only on macOS"));
    assert_eq!(fs::read(source).unwrap(), b"preserve this");
    assert!(!root.path().join("Documents/safe.pdf").exists());
    assert!(!journal_path(&home).parent().unwrap().exists());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn recovery_refuses_existing_journal_before_creating_a_lock_file() {
    let home = TempDir::new().unwrap();
    let journal = journal_path(&home);
    fs::create_dir_all(journal.parent().unwrap()).unwrap();
    let original = br#"{"version":1,"operations":[]}"#;
    fs::write(&journal, original).unwrap();

    let output = run(&home, &["recover", "--json"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("only on macOS"));
    assert_eq!(fs::read(&journal).unwrap(), original);
    assert!(!journal.with_extension("lock").exists());
}

#[test]
fn help_and_version_are_available() {
    let home = TempDir::new().unwrap();
    assert!(run(&home, &["--help"]).status.success());
    assert!(run(&home, &["--version"]).status.success());
}
