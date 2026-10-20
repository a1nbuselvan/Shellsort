use super::{
    classification::{classify_file_name, is_hidden_file},
    model::{
        EngineError, EngineErrorKind, FileIdentity, FileMetadata, RelativePath, ScanIssue,
        ScanIssueKind, ScanResult, ScanWarning, ScanWarningKind,
    },
};
use std::{
    fs::{self, Metadata},
    io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub fn scan_folder(root_path: &str) -> Result<ScanResult, EngineError> {
    scan_folder_with_root(root_path).map(|(_, scan)| scan)
}

pub(super) fn scan_folder_with_root(root_path: &str) -> Result<(PathBuf, ScanResult), EngineError> {
    let requested_root = Path::new(root_path);
    let root_metadata = fs::symlink_metadata(requested_root)
        .map_err(|error| map_root_error(error, "Could not inspect the selected folder."))?;

    if root_metadata.file_type().is_symlink() {
        return Err(EngineError::new(
            EngineErrorKind::RootIsSymbolicLink,
            "The selected folder is a symbolic link. Choose its actual folder instead.",
        ));
    }
    if !root_metadata.is_dir() {
        return Err(EngineError::new(
            EngineErrorKind::RootNotDirectory,
            "The selected path is not a folder.",
        ));
    }

    let root = fs::canonicalize(requested_root)
        .map_err(|error| map_root_error(error, "Could not resolve the selected folder."))?;
    let root_recheck = fs::symlink_metadata(requested_root)
        .map_err(|error| map_root_error(error, "Could not recheck the selected folder."))?;
    if root_recheck.file_type().is_symlink() || !root_recheck.is_dir() {
        return Err(EngineError::new(
            EngineErrorKind::RootIsSymbolicLink,
            "The selected folder changed while it was being validated. Choose it again.",
        ));
    }
    let root_recheck_canonical = fs::canonicalize(requested_root)
        .map_err(|error| map_root_error(error, "Could not recheck the selected folder."))?;
    if root_recheck_canonical != root {
        return Err(EngineError::new(
            EngineErrorKind::InvalidRootPath,
            "The selected folder changed while it was being validated. Choose it again.",
        ));
    }

    let mut result = ScanResult {
        root_path: root.to_string_lossy().into_owned(),
        root_identity: file_identity(&fs::symlink_metadata(&root).map_err(|error| {
            map_root_error(error, "Could not inspect the selected folder identity.")
        })?),
        files: Vec::new(),
        warnings: Vec::new(),
        issues: Vec::new(),
        is_incomplete: false,
    };

    let mut pending = vec![root.clone()];
    while let Some(directory) = pending.pop() {
        scan_directory(
            &root,
            &directory,
            directory == root,
            &mut pending,
            &mut result,
        )?;
    }
    result
        .files
        .sort_by(|left, right| left.source.as_str().cmp(right.source.as_str()));
    result.warnings.sort_by(|left, right| {
        left.path
            .as_ref()
            .map(RelativePath::as_str)
            .cmp(&right.path.as_ref().map(RelativePath::as_str))
    });
    result.issues.sort_by(|left, right| {
        left.path
            .as_ref()
            .map(RelativePath::as_str)
            .cmp(&right.path.as_ref().map(RelativePath::as_str))
    });
    result.is_incomplete = !result.issues.is_empty();
    Ok((root, result))
}

fn scan_directory(
    root: &Path,
    directory: &Path,
    is_root: bool,
    pending: &mut Vec<PathBuf>,
    result: &mut ScanResult,
) -> Result<(), EngineError> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if is_root {
                return Err(EngineError::new(
                    EngineErrorKind::RootIsSymbolicLink,
                    "The selected folder changed to a symbolic link during scanning.",
                ));
            }
            result.warnings.push(ScanWarning {
                path: relative_path(root, directory).ok(),
                kind: ScanWarningKind::SymbolicLinkSkipped,
                message: "Symbolic links are skipped and are never followed.".to_owned(),
            });
            return Ok(());
        }
        Ok(metadata) if !metadata.is_dir() => {
            if is_root {
                return Err(EngineError::new(
                    EngineErrorKind::RootNotDirectory,
                    "The selected path is no longer a folder.",
                ));
            }
            result.issues.push(issue_for_path(
                root,
                directory,
                ScanIssueKind::MetadataReadFailed,
                "This folder changed type during scanning and was skipped.".to_owned(),
            ));
            return Ok(());
        }
        Ok(_) => {}
        Err(error) if is_root => {
            return Err(map_root_error(
                error,
                "Could not recheck the selected folder.",
            ));
        }
        Err(error) => {
            result.issues.push(issue_for_path(
                root,
                directory,
                ScanIssueKind::MetadataReadFailed,
                format!("Could not recheck this folder: {error}"),
            ));
            return Ok(());
        }
    }

    match fs::canonicalize(directory) {
        // The path being scanned was derived from the canonical root. Requiring
        // exact equality also rejects a symlink that points back inside the root.
        Ok(canonical) if canonical == directory => {}
        Ok(_) => {
            result.issues.push(issue_for_path(
                root,
                directory,
                ScanIssueKind::PathOutsideRoot,
                "This folder resolves through a symbolic link or outside the selected root and was skipped.".to_owned(),
            ));
            return Ok(());
        }
        Err(error) if is_root => {
            return Err(map_root_error(
                error,
                "Could not resolve the selected folder.",
            ));
        }
        Err(error) => {
            result.issues.push(issue_for_path(
                root,
                directory,
                ScanIssueKind::MetadataReadFailed,
                format!("Could not resolve this folder: {error}"),
            ));
            return Ok(());
        }
    }

    // Recheck as close as possible to read_dir. This narrows path replacement
    // races; the scan remains a non-atomic snapshot if another process mutates
    // the folder concurrently.
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            result.issues.push(issue_for_path(
                root,
                directory,
                ScanIssueKind::PathOutsideRoot,
                "This folder changed while scanning and was skipped.".to_owned(),
            ));
            return Ok(());
        }
        Err(error) => {
            result.issues.push(issue_for_path(
                root,
                directory,
                ScanIssueKind::MetadataReadFailed,
                format!("Could not recheck this folder before reading it: {error}"),
            ));
            return Ok(());
        }
    }

    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if is_root => {
            return Err(map_root_error(error, "Could not read the selected folder."));
        }
        Err(error) => {
            result.issues.push(issue_for_path(
                root,
                directory,
                ScanIssueKind::DirectoryReadFailed,
                format!("Could not read this subfolder: {error}"),
            ));
            return Ok(());
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                result.issues.push(issue_for_path(
                    root,
                    directory,
                    ScanIssueKind::EntryReadFailed,
                    format!("Could not read a directory entry: {error}"),
                ));
                continue;
            }
        };

        let path = entry.path();
        let file_name = entry.file_name();
        let Some(file_name_text) = file_name.to_str() else {
            result.warnings.push(ScanWarning {
                path: relative_path(root, directory).ok(),
                kind: ScanWarningKind::InvalidUnicodeNameSkipped,
                message: "Skipped an entry whose name is not valid Unicode.".to_owned(),
            });
            continue;
        };

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                result.issues.push(issue_for_path(
                    root,
                    &path,
                    ScanIssueKind::MetadataReadFailed,
                    format!("Could not inspect this entry: {error}"),
                ));
                continue;
            }
        };

        if is_macos_metadata_name(file_name_text) {
            continue;
        }

        if file_type.is_symlink() {
            result.warnings.push(ScanWarning {
                path: relative_path(root, &path).ok(),
                kind: ScanWarningKind::SymbolicLinkSkipped,
                message: "Symbolic links are skipped and are never followed.".to_owned(),
            });
            continue;
        }

        if file_type.is_dir() {
            if file_name_text.starts_with('.') {
                result.warnings.push(ScanWarning {
                    path: relative_path(root, &path).ok(),
                    kind: ScanWarningKind::HiddenDirectorySkipped,
                    message: "Hidden folders are skipped.".to_owned(),
                });
                continue;
            }
            if matches!(file_name_text, "node_modules" | "target") {
                result.warnings.push(ScanWarning {
                    path: relative_path(root, &path).ok(),
                    kind: ScanWarningKind::ExcludedDirectorySkipped,
                    message: "Generated dependency and build folders are skipped.".to_owned(),
                });
                continue;
            }

            // Recheck without following a link in case the entry changed after readdir.
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    pending.push(path);
                }
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    result.warnings.push(ScanWarning {
                        path: relative_path(root, &path).ok(),
                        kind: ScanWarningKind::SymbolicLinkSkipped,
                        message: "Symbolic links are skipped and are never followed.".to_owned(),
                    });
                }
                Ok(_) => result.warnings.push(ScanWarning {
                    path: relative_path(root, &path).ok(),
                    kind: ScanWarningKind::NonRegularEntrySkipped,
                    message: "This entry changed type while scanning and was skipped.".to_owned(),
                }),
                Err(error) => result.issues.push(issue_for_path(
                    root,
                    &path,
                    ScanIssueKind::MetadataReadFailed,
                    format!("Could not inspect this folder: {error}"),
                )),
            }
            continue;
        }

        if !file_type.is_file() {
            result.warnings.push(ScanWarning {
                path: relative_path(root, &path).ok(),
                kind: ScanWarningKind::NonRegularEntrySkipped,
                message: "Non-regular filesystem entries are skipped.".to_owned(),
            });
            continue;
        }

        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
            Ok(metadata) if metadata.file_type().is_symlink() => {
                result.warnings.push(ScanWarning {
                    path: relative_path(root, &path).ok(),
                    kind: ScanWarningKind::SymbolicLinkSkipped,
                    message: "Symbolic links are skipped and are never followed.".to_owned(),
                });
                continue;
            }
            Ok(_) => {
                result.warnings.push(ScanWarning {
                    path: relative_path(root, &path).ok(),
                    kind: ScanWarningKind::NonRegularEntrySkipped,
                    message: "This entry changed to a non-regular filesystem type and was skipped."
                        .to_owned(),
                });
                continue;
            }
            Err(error) => {
                result.issues.push(issue_for_path(
                    root,
                    &path,
                    ScanIssueKind::MetadataReadFailed,
                    format!("Could not inspect this file: {error}"),
                ));
                continue;
            }
        };

        let relative_source = match relative_path(root, &path) {
            Ok(relative) => relative,
            Err(error) => {
                result.issues.push(ScanIssue {
                    path: None,
                    kind: ScanIssueKind::MetadataReadFailed,
                    message: error.message,
                });
                continue;
            }
        };
        let (extension, category) = classify_file_name(file_name_text);
        result.files.push(file_metadata(
            relative_source,
            file_name_text,
            extension,
            category,
            &metadata,
            is_hidden_file(&path),
        ));
    }

    Ok(())
}

pub(super) fn is_macos_metadata_path(path: &RelativePath) -> bool {
    path.as_path().components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(is_macos_metadata_name)
    })
}

fn is_macos_metadata_name(name: &str) -> bool {
    name.starts_with("._")
        || matches!(
            name,
            ".DS_Store"
                | ".localized"
                | ".metadata_never_index"
                | "Icon\r"
                | ".VolumeIcon.icns"
                | ".LSOverride"
                | ".Spotlight-V100"
                | ".Trashes"
                | ".fseventsd"
                | ".TemporaryItems"
                | ".DocumentRevisions-V100"
                | ".AppleDouble"
                | ".AppleDB"
                | ".AppleDesktop"
                | "Temporary Items"
                | "Network Trash Folder"
                | "__MACOSX"
                | ".com.apple.timemachine.donotpresent"
        )
}

fn file_metadata(
    source: RelativePath,
    file_name: &str,
    extension: Option<String>,
    category: super::Category,
    metadata: &Metadata,
    hidden: bool,
) -> FileMetadata {
    FileMetadata {
        source,
        file_name: file_name.to_owned(),
        extension,
        category,
        size_bytes: metadata.len(),
        identity: file_identity(metadata),
        modified_unix_seconds: metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs()),
        hidden,
    }
}

fn file_identity(metadata: &Metadata) -> FileIdentity {
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;

    let (modified_seconds, modified_nanoseconds) = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| (duration.as_secs() as i64, duration.subsec_nanos() as i64))
        .unwrap_or((0, 0));

    FileIdentity {
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(not(unix))]
        device: 0,
        #[cfg(unix)]
        inode: metadata.ino(),
        #[cfg(not(unix))]
        inode: 0,
        size_bytes: metadata.len(),
        modified_seconds,
        modified_nanoseconds,
        #[cfg(unix)]
        changed_seconds: metadata.ctime(),
        #[cfg(not(unix))]
        changed_seconds: 0,
        #[cfg(unix)]
        changed_nanoseconds: metadata.ctime_nsec(),
        #[cfg(not(unix))]
        changed_nanoseconds: 0,
    }
}

fn relative_path(root: &Path, path: &Path) -> Result<RelativePath, EngineError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        EngineError::new(
            EngineErrorKind::InvalidRelativePath,
            "Scanned entry is outside the selected folder.",
        )
    })?;
    RelativePath::from_path(relative)
}

fn issue_for_path(root: &Path, path: &Path, kind: ScanIssueKind, message: String) -> ScanIssue {
    ScanIssue {
        path: relative_path(root, path).ok(),
        kind,
        message,
    }
}

fn map_root_error(error: io::Error, context: &str) -> EngineError {
    let kind = match error.kind() {
        io::ErrorKind::NotFound => EngineErrorKind::RootNotFound,
        io::ErrorKind::PermissionDenied => EngineErrorKind::PermissionDenied,
        io::ErrorKind::InvalidInput => EngineErrorKind::InvalidRootPath,
        _ => EngineErrorKind::IoError,
    };
    EngineError::new(kind, format!("{context} {error}"))
}
