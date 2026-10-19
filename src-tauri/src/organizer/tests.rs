use super::{
    Category, ConflictStatus, EngineErrorKind, RelativePath, ScanIssueKind, ScanWarningKind,
    classification::{classify_file_name, extension_for_name},
    generate_plan, scan_folder,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

#[test]
fn classifies_all_required_extensions_case_insensitively() {
    let cases = [
        ("pdf doc docx txt rtf odt", Category::Documents),
        ("jpg jpeg png gif webp svg heic", Category::Images),
        ("mp4 mov mkv avi webm", Category::Videos),
        ("mp3 wav flac aac m4a ogg", Category::Audio),
        ("zip tar gz bz2 xz 7z rar", Category::Archives),
        (
            "rs js ts tsx jsx py go java c cpp h css html json yaml yml toml sh",
            Category::Code,
        ),
        ("csv xls xlsx ods", Category::Spreadsheets),
        ("ppt pptx odp", Category::Presentations),
    ];

    for (extensions, category) in cases {
        for extension in extensions.split_whitespace() {
            for file_name in [
                format!("item.{extension}"),
                format!("ITEM.{}", extension.to_uppercase()),
            ] {
                let (found_extension, found_category) = classify_file_name(&file_name);
                assert_eq!(found_category, category, "{file_name}");
                assert_eq!(found_extension.as_deref(), Some(extension));
            }
        }
    }
    assert_eq!(classify_file_name("unknown.xyz").1, Category::Other);
}

#[test]
fn dotfiles_and_extensionless_names_are_other() {
    assert_eq!(extension_for_name(".gitignore"), None);
    assert_eq!(extension_for_name("README"), None);
    assert_eq!(extension_for_name("trailing."), None);
    assert_eq!(extension_for_name(".env.local").as_deref(), Some("local"));
    assert_eq!(classify_file_name(".gitignore").1, Category::Other);
}

#[test]
fn plan_uses_the_expected_category_destination_folders() {
    let root = tempdir().expect("temporary root");
    for file_name in [
        "a.pdf",
        "b.png",
        "c.mov",
        "d.mp3",
        "e.zip",
        "f.rs",
        "g.csv",
        "h.pptx",
        "i.unknown",
    ] {
        fs::write(root.path().join(file_name), b"sample").expect("sample file");
    }

    let plan = generate_plan(root.path().to_str().expect("root path")).expect("plan");
    for (file_name, folder_name) in [
        ("a.pdf", "Documents"),
        ("b.png", "Images"),
        ("c.mov", "Videos"),
        ("d.mp3", "Audio"),
        ("e.zip", "Archives"),
        ("f.rs", "Code"),
        ("g.csv", "Spreadsheets"),
        ("h.pptx", "Presentations"),
        ("i.unknown", "Other"),
    ] {
        let entry = plan
            .entries
            .iter()
            .find(|entry| entry.file_name == file_name)
            .unwrap();
        assert_eq!(
            entry.destination.as_str(),
            format!("{folder_name}/{file_name}")
        );
    }
}

#[test]
fn scans_unicode_names_spaces_and_nested_relative_paths() {
    let root = tempdir().expect("temporary root");
    let nested = root.path().join("資料 Folder");
    fs::create_dir(&nested).expect("nested directory");
    fs::write(nested.join("résumé final.PDF"), b"pdf bytes").expect("sample file");

    let scan = scan_folder(root.path().to_str().expect("unicode root path")).expect("scan");
    assert_eq!(scan.files.len(), 1);
    assert_eq!(
        scan.files[0].source.as_str(),
        "資料 Folder/résumé final.PDF"
    );
    assert_eq!(scan.files[0].extension.as_deref(), Some("pdf"));
    assert_eq!(scan.files[0].category, Category::Documents);
    assert_eq!(scan.files[0].size_bytes, 9);
}

#[test]
fn empty_folder_returns_empty_scan_and_plan() {
    let root = tempdir().expect("temporary root");
    let path = root.path().to_str().expect("root path");
    let scan = scan_folder(path).expect("scan");
    let plan = generate_plan(path).expect("plan");
    assert!(scan.files.is_empty());
    assert!(plan.entries.is_empty());
}

#[test]
fn hidden_directories_build_directories_and_dotfiles_have_predictable_policy() {
    let root = tempdir().expect("temporary root");
    for directory in [".git", "node_modules", "target"] {
        let ignored = root.path().join(directory);
        fs::create_dir(&ignored).expect("ignored directory");
        fs::write(ignored.join("inside.rs"), b"not included").expect("nested sample");
    }
    fs::write(root.path().join(".DS_Store"), b"metadata").expect("metadata sample");
    fs::write(root.path().join(".gitignore"), b"dotfile").expect("dotfile sample");

    let scan = scan_folder(root.path().to_str().expect("root path")).expect("scan");
    assert_eq!(scan.files.len(), 2);
    assert!(
        scan.files
            .iter()
            .all(|file| file.category == Category::Other)
    );
    assert!(scan.files.iter().any(|file| file.file_name == ".DS_Store"));
    assert!(scan.files.iter().any(|file| file.file_name == ".gitignore"));
    assert!(
        scan.warnings
            .iter()
            .any(|warning| warning.kind == ScanWarningKind::HiddenDirectorySkipped)
    );
    assert_eq!(
        scan.warnings
            .iter()
            .filter(|warning| warning.kind == ScanWarningKind::ExcludedDirectorySkipped)
            .count(),
        2
    );
}

#[cfg(unix)]
#[test]
fn root_and_entries_that_are_symlinks_are_not_followed() {
    use std::os::unix::fs::symlink;

    let root = tempdir().expect("temporary root");
    let outside = tempdir().expect("temporary outside folder");
    fs::write(outside.path().join("outside.pdf"), b"outside").expect("outside file");
    symlink(outside.path(), root.path().join("linked-folder")).expect("directory link");
    symlink(
        outside.path().join("outside.pdf"),
        root.path().join("linked-file.pdf"),
    )
    .expect("file link");

    let scan = scan_folder(root.path().to_str().expect("root path")).expect("scan");
    assert!(scan.files.is_empty());
    assert_eq!(
        scan.warnings
            .iter()
            .filter(|warning| warning.kind == ScanWarningKind::SymbolicLinkSkipped)
            .count(),
        2
    );

    let root_link = root.path().join("root-alias");
    symlink(root.path(), &root_link).expect("root link");
    let error =
        scan_folder(root_link.to_str().expect("root link path")).expect_err("reject symlink root");
    assert_eq!(error.kind, EngineErrorKind::RootIsSymbolicLink);
    assert_eq!(
        fs::read(outside.path().join("outside.pdf")).unwrap(),
        b"outside"
    );
}

#[test]
fn non_unicode_names_are_skipped_with_a_warning_on_unix() {
    use std::os::unix::ffi::OsStrExt;

    let root = tempdir().expect("temporary root");
    let invalid_name = std::ffi::OsStr::from_bytes(b"bad-\xff.pdf");
    let invalid_path = root.path().join(invalid_name);
    if let Err(error) = fs::write(&invalid_path, b"sample") {
        eprintln!("Skipping non-Unicode filename case on this filesystem: {error}");
        return;
    }

    let scan = scan_folder(root.path().to_str().expect("root path")).expect("scan");
    assert!(scan.files.is_empty());
    assert!(
        scan.warnings
            .iter()
            .any(|warning| { warning.kind == ScanWarningKind::InvalidUnicodeNameSkipped })
    );
}

#[test]
fn existing_destination_is_reported_without_overwrite() {
    let root = tempdir().expect("temporary root");
    fs::write(root.path().join("report.pdf"), b"source").expect("source");
    let documents = root.path().join("Documents");
    fs::create_dir(&documents).expect("destination folder");
    fs::write(documents.join("report.pdf"), b"existing").expect("existing destination");

    let plan = generate_plan(root.path().to_str().expect("root path")).expect("plan");
    let entry = plan
        .entries
        .iter()
        .find(|entry| entry.source.as_str() == "report.pdf")
        .unwrap();
    assert_eq!(entry.conflict_status, ConflictStatus::ExistingDestination);
    assert!(entry.reason.as_deref().unwrap().contains("no overwrite"));
    assert_eq!(fs::read(root.path().join("report.pdf")).unwrap(), b"source");
    assert_eq!(fs::read(documents.join("report.pdf")).unwrap(), b"existing");
}

#[test]
fn multiple_sources_with_same_destination_are_all_marked_as_collisions() {
    let root = tempdir().expect("temporary root");
    for directory in ["first", "second"] {
        let path = root.path().join(directory);
        fs::create_dir(&path).expect("source folder");
        fs::write(path.join("same.pdf"), directory.as_bytes()).expect("source file");
    }

    let plan = generate_plan(root.path().to_str().expect("root path")).expect("plan");
    assert_eq!(plan.entries.len(), 2);
    assert!(
        plan.entries
            .iter()
            .all(|entry| entry.conflict_status == ConflictStatus::ProposedCollision)
    );
}

#[test]
fn proposed_names_are_compared_case_and_unicode_normalization_insensitively() {
    let root = tempdir().expect("temporary root");
    fs::create_dir(root.path().join("first")).expect("first folder");
    fs::create_dir(root.path().join("second")).expect("second folder");
    fs::write(root.path().join("first/Résumé.pdf"), b"one").expect("first source");
    fs::write(root.path().join("second/re\u{301}sume\u{301}.PDF"), b"two").expect("second source");

    let plan = generate_plan(root.path().to_str().expect("root path")).expect("plan");
    assert_eq!(plan.entries.len(), 2);
    assert!(
        plan.entries
            .iter()
            .all(|entry| entry.conflict_status == ConflictStatus::ProposedCollision)
    );
}

#[test]
fn existing_and_proposed_collisions_are_both_reported() {
    let root = tempdir().expect("temporary root");
    fs::create_dir(root.path().join("first")).expect("first folder");
    fs::create_dir(root.path().join("second")).expect("second folder");
    fs::create_dir(root.path().join("Documents")).expect("category folder");
    fs::write(root.path().join("first/shared.pdf"), b"one").expect("first source");
    fs::write(root.path().join("second/shared.pdf"), b"two").expect("second source");
    fs::write(root.path().join("Documents/shared.pdf"), b"already there").expect("destination");

    let plan = generate_plan(root.path().to_str().expect("root path")).expect("plan");
    let proposed_collisions: Vec<_> = plan
        .entries
        .iter()
        .filter(|entry| {
            entry.source.as_str().starts_with("first/")
                || entry.source.as_str().starts_with("second/")
        })
        .collect();
    assert_eq!(proposed_collisions.len(), 2);
    assert!(
        proposed_collisions
            .iter()
            .all(|entry| { entry.conflict_status == ConflictStatus::ExistingAndProposedCollision })
    );
    assert!(plan.entries.iter().any(|entry| {
        entry.source.as_str() == "Documents/shared.pdf"
            && entry.conflict_status == ConflictStatus::AlreadyCorrect
    }));
}

#[test]
fn already_correct_files_are_reported_unchanged() {
    let root = tempdir().expect("temporary root");
    fs::create_dir(root.path().join("Documents")).expect("category folder");
    fs::write(root.path().join("Documents/notes.txt"), b"keep").expect("already-correct file");

    let plan = generate_plan(root.path().to_str().expect("root path")).expect("plan");
    assert_eq!(plan.entries.len(), 1);
    assert_eq!(
        plan.entries[0].conflict_status,
        ConflictStatus::AlreadyCorrect
    );
    assert_eq!(plan.entries[0].source, plan.entries[0].destination);
}

#[test]
fn unreadable_nested_directory_is_a_recoverable_issue_when_permissions_enforce_it() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempdir().expect("temporary root");
    let locked = root.path().join("locked");
    fs::create_dir(&locked).expect("nested folder");
    fs::write(locked.join("hidden.pdf"), b"sample").expect("nested file");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0)).expect("remove permissions");

    if fs::read_dir(&locked).is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700))
            .expect("restore permissions");
        eprintln!(
            "Skipping unreadable-directory assertion because this user can still read mode-000 folders."
        );
        return; // Some privileged test users can still read mode-000 directories.
    }

    let scan = scan_folder(root.path().to_str().expect("root path")).expect("recoverable scan");
    assert!(scan.is_incomplete);
    assert!(
        scan.issues
            .iter()
            .any(|issue| issue.kind == ScanIssueKind::DirectoryReadFailed)
    );
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).expect("restore permissions");
}

#[test]
fn unsafe_relative_paths_are_rejected() {
    for path in [
        Path::new("../outside.txt"),
        Path::new("/absolute.txt"),
        Path::new("a/../../b"),
    ] {
        assert!(RelativePath::from_path(path).is_err(), "{}", path.display());
    }
}

#[test]
fn invalid_roots_return_structured_errors() {
    let root = tempdir().expect("temporary root");
    let missing = root.path().join("missing");
    let missing_error =
        scan_folder(missing.to_str().expect("missing path")).expect_err("missing root");
    assert_eq!(missing_error.kind, EngineErrorKind::RootNotFound);

    let file = root.path().join("file.txt");
    fs::write(&file, b"file").expect("regular file");
    let file_error = scan_folder(file.to_str().expect("file path")).expect_err("not a folder");
    assert_eq!(file_error.kind, EngineErrorKind::RootNotDirectory);

    let nul_error = scan_folder("\0").expect_err("invalid root path");
    assert_eq!(nul_error.kind, EngineErrorKind::InvalidRootPath);
}

#[test]
fn scan_and_plan_do_not_change_temp_tree_paths_or_file_bytes() {
    let root = tempdir().expect("temporary root");
    fs::create_dir(root.path().join("nested folder")).expect("nested folder");
    fs::write(
        root.path().join("nested folder/a file.PDF"),
        b"immutable bytes",
    )
    .expect("source file");
    fs::write(root.path().join(".DS_Store"), b"metadata bytes").expect("hidden metadata");
    let before = snapshot(root.path());

    scan_folder(root.path().to_str().expect("root path")).expect("scan");
    generate_plan(root.path().to_str().expect("root path")).expect("plan");

    assert_eq!(snapshot(root.path()), before);
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, current: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).expect("read test directory") {
            let entry = entry.expect("read test entry");
            let path = entry.path();
            let file_type = entry.file_type().expect("entry type");
            if file_type.is_symlink() {
                result.insert(path.strip_prefix(root).unwrap().to_path_buf(), Vec::new());
            } else if file_type.is_dir() {
                visit(root, &path, result);
            } else if file_type.is_file() {
                result.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }

    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}
