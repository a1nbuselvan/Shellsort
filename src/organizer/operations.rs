use super::scan::is_macos_metadata_path;
use super::{
    FileIdentity, FileOperationError, FileOperationOutcome, FileOperationRecord, OperationRecord,
    OperationStatus, OrganizationPlan, OrganizationSettings, PlanStatus, RelativePath,
    generate_plan_with_settings,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Cross-process exclusive lock for journal users.
pub struct JournalLock {
    _file: std::fs::File,
}

impl JournalLock {
    /// Locks an existing journal without creating an app-data directory or
    /// lock file when no journal has been recorded yet.
    pub fn acquire_if_journal_exists(
        journal_path: &Path,
    ) -> Result<Option<Self>, super::EngineError> {
        match fs::symlink_metadata(journal_path) {
            Ok(_) => Self::acquire(journal_path).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                validate_absent_journal_parent(journal_path)?;
                let lock_path = journal_path.with_extension("lock");
                match fs::symlink_metadata(&lock_path) {
                    Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                        Err(journal_error(
                            "The operation-history lock is not a regular file.",
                        ))
                    }
                    Ok(_) => Self::acquire(journal_path).map(Some),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(journal_error(format!(
                        "Could not inspect the operation-history lock: {error}"
                    ))),
                }
            }
            Err(error) => Err(journal_error(format!(
                "Could not inspect the operation journal: {error}"
            ))),
        }
    }

    pub fn acquire(journal_path: &Path) -> Result<Self, super::EngineError> {
        let parent = journal_path
            .parent()
            .ok_or_else(|| journal_error("The operation journal has no parent directory."))?;
        ensure_journal_parent(parent)?;
        let lock_path = journal_path.with_extension("lock");
        if let Ok(metadata) = fs::symlink_metadata(&lock_path)
            && (metadata.file_type().is_symlink() || !metadata.is_file())
        {
            return Err(journal_error(
                "The operation-history lock is not a regular file.",
            ));
        }
        #[cfg(unix)]
        let file = {
            use std::os::unix::fs::OpenOptionsExt;
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&lock_path)
                .map_err(|error| {
                    journal_error(format!(
                        "Could not open the operation-history lock: {error}"
                    ))
                })?
        };
        #[cfg(not(unix))]
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                journal_error(format!(
                    "Could not open the operation-history lock: {error}"
                ))
            })?;
        file.lock().map_err(|error| {
            journal_error(format!(
                "Could not acquire the operation-history lock: {error}"
            ))
        })?;
        Ok(Self { _file: file })
    }
}

/// Create the app-data directory chain without following a symlink inside the
/// user's home. The home itself is canonicalized so supported macOS aliases
/// (for example /var) remain valid. Other journal paths use the nearest
/// existing directory as a trusted anchor, which also supports isolated tests.
fn ensure_journal_parent(parent: &Path) -> Result<(), super::EngineError> {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(relative) = parent.strip_prefix(&home)
    {
        let canonical_home = home.canonicalize().map_err(|error| {
            journal_error(format!(
                "Could not validate the operation-history location: {error}"
            ))
        })?;
        if !canonical_home.is_dir() {
            return Err(journal_error(
                "The operation-history home location is not a directory.",
            ));
        }
        return create_checked_directories(&canonical_home, relative);
    }

    let mut anchor = parent;
    loop {
        match fs::symlink_metadata(anchor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(journal_error(
                        "The operation-history parent is not a real directory.",
                    ));
                }
                let canonical_anchor = anchor.canonicalize().map_err(|error| {
                    journal_error(format!(
                        "Could not validate the operation-history location: {error}"
                    ))
                })?;
                let relative = parent
                    .strip_prefix(anchor)
                    .map_err(|_| journal_error("The operation-history location is invalid."))?;
                return create_checked_directories(&canonical_anchor, relative);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                anchor = anchor.parent().ok_or_else(|| {
                    journal_error("Could not find a safe operation-history parent.")
                })?;
            }
            Err(error) => {
                return Err(journal_error(format!(
                    "Could not inspect the operation-history location: {error}"
                )));
            }
        }
    }
}

fn create_checked_directories(anchor: &Path, relative: &Path) -> Result<(), super::EngineError> {
    let mut current = anchor.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(journal_error(
                "The operation-history location contains an unsafe path component.",
            ));
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(journal_error(
                    "The operation-history parent is not a real directory.",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    // Another Shellsort process may have created this component
                    // after our check. Reinspect it below and reject symlinks.
                    Err(create_error)
                        if create_error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(create_error) => {
                        return Err(journal_error(format!(
                            "Could not create the operation-history folder: {create_error}"
                        )));
                    }
                }
                let metadata = fs::symlink_metadata(&current).map_err(|inspect_error| {
                    journal_error(format!(
                        "Could not inspect the operation-history folder: {inspect_error}"
                    ))
                })?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(journal_error(
                        "The operation-history parent is not a real directory.",
                    ));
                }
            }
            Err(error) => {
                return Err(journal_error(format!(
                    "Could not inspect the operation-history folder: {error}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_absent_journal_parent(journal_path: &Path) -> Result<(), super::EngineError> {
    let parent = journal_path
        .parent()
        .ok_or_else(|| journal_error("The operation journal has no parent directory."))?;
    match fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            journal_error("The operation-history folder is not a real directory."),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Check an existing app-data ancestor if present, but do not
            // create the missing directory tree just to inspect empty history.
            if let Some(ancestor) = parent.parent() {
                match fs::symlink_metadata(ancestor) {
                    Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                        return Err(journal_error(
                            "The operation-history parent is not a real directory.",
                        ));
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(journal_error(format!(
                            "Could not inspect the operation-history parent: {error}"
                        )));
                    }
                }
            }
            Ok(())
        }
        Err(error) => Err(journal_error(format!(
            "Could not inspect the operation-history folder: {error}"
        ))),
    }
}

const JOURNAL_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Journal {
    version: u32,
    operations: Vec<OperationRecord>,
}

/// Start only when the user-confirmed preview is byte-for-byte equivalent to
/// a freshly generated Rust plan. Conflicts and incomplete scans are rejected.
pub fn start_operation(
    root_path: &str,
    preview: &OrganizationPlan,
    settings: &OrganizationSettings,
    journal_path: &Path,
) -> Result<OperationRecord, super::EngineError> {
    let current = generate_plan_with_settings(root_path, settings)?;
    if current != *preview || current.is_incomplete || current.root_path != preview.root_path {
        return Err(stale_plan_error());
    }
    let entries: Vec<_> = current
        .entries
        .iter()
        .filter(|entry| entry.status == PlanStatus::Ready && entry.destination.is_some())
        .collect();
    if entries.is_empty() {
        return Err(super::EngineError::new(
            super::EngineErrorKind::UnsafeOperation,
            "There are no conflict-free files in this preview to move.",
        ));
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let operation_id = format!(
        "op-{}-{}-{}",
        timestamp.as_secs(),
        timestamp.subsec_nanos(),
        std::process::id()
    );
    let files = entries
        .into_iter()
        .map(|entry| FileOperationRecord {
            original_path: entry.source.clone(),
            destination_path: entry
                .destination
                .clone()
                .expect("ready entry has destination"),
            identity: entry.source_identity.clone(),
            destination_identity: None,
            outcome: FileOperationOutcome::Planned,
            error: None,
            undo_error: None,
            source_size_bytes_before: Some(entry.size_bytes),
            source_modified_unix_seconds_before: u64::try_from(
                entry.source_identity.modified_seconds,
            )
            .ok(),
            destination_size_bytes_after: None,
            destination_modified_unix_seconds_after: None,
        })
        .collect();
    let record = OperationRecord {
        operation_id,
        timestamp_unix_seconds: timestamp.as_secs(),
        root_path: current.root_path,
        root_identity: current.root_identity,
        status: OperationStatus::Planned,
        files,
    };
    let mut journal = read_journal(journal_path)?;
    journal.operations.insert(0, record.clone());
    write_journal(journal_path, &journal)?;
    Ok(record)
}

/// Processes one file per call so the UI can update progress and cancel at a
/// safe checkpoint between moves.
pub fn advance_operation(
    root_path: &str,
    operation_id: &str,
    journal_path: &Path,
) -> Result<OperationRecord, super::EngineError> {
    let mut journal = read_journal(journal_path)?;
    let (index, snapshot) = {
        let record = find_operation_mut(&mut journal, operation_id)?;
        validate_operation_root(root_path, record)?;
        if !matches!(
            record.status,
            OperationStatus::Planned | OperationStatus::InProgress
        ) {
            return Ok(record.clone());
        }
        let Some(index) = record
            .files
            .iter()
            .position(|file| file.outcome == FileOperationOutcome::Planned)
        else {
            finish_operation(record);
            let result = record.clone();
            write_journal(journal_path, &journal)?;
            return Ok(result);
        };
        record.status = OperationStatus::InProgress;
        record.files[index].outcome = FileOperationOutcome::InProgress;
        record.files[index].error = None;
        (index, record.clone())
    };
    write_journal(journal_path, &journal)?;
    let file = &snapshot.files[index];
    let moved = secure_rename(
        Path::new(&snapshot.root_path),
        &snapshot.root_identity,
        &file.original_path,
        &file.destination_path,
        &file.identity,
        true,
    );

    // Persist the post-rename identity while the outcome remains InProgress.
    // If the process stops before this write, recovery will not offer automatic undo.
    if let Ok(destination_identity) = &moved {
        let record = find_operation_mut(&mut journal, operation_id)?;
        record.files[index].destination_identity = Some(destination_identity.clone());
        write_journal(journal_path, &journal)?;
    }

    let result = {
        let record = find_operation_mut(&mut journal, operation_id)?;
        match moved {
            Ok(destination_identity) => {
                record.files[index].outcome = FileOperationOutcome::Completed;
                record.files[index].error = None;
                record.files[index].destination_size_bytes_after =
                    Some(destination_identity.size_bytes);
                record.files[index].destination_modified_unix_seconds_after =
                    u64::try_from(destination_identity.modified_seconds).ok();
                record.files[index].destination_identity = Some(destination_identity);
            }
            Err(issue) => {
                record.files[index].outcome = if issue.uncertain {
                    record.status = OperationStatus::Interrupted;
                    FileOperationOutcome::InProgress
                } else if issue.skipped {
                    FileOperationOutcome::Skipped
                } else {
                    FileOperationOutcome::Failed
                };
                record.files[index].error = Some(FileOperationError {
                    kind: issue.kind,
                    message: issue.message,
                });
            }
        }
        if record.status != OperationStatus::Interrupted {
            finish_operation(record);
        }
        record.clone()
    };
    write_journal(journal_path, &journal)?;
    Ok(result)
}

pub fn cancel_operation(
    operation_id: &str,
    journal_path: &Path,
) -> Result<OperationRecord, super::EngineError> {
    let mut journal = read_journal(journal_path)?;
    let (result, changed) = {
        let record = find_operation_mut(&mut journal, operation_id)?;
        let changed = record.status == OperationStatus::InProgress;
        if changed {
            for file in &mut record.files {
                if file.outcome == FileOperationOutcome::Planned {
                    file.outcome = FileOperationOutcome::Cancelled;
                }
            }
            record.status = OperationStatus::Cancelled;
        }
        (record.clone(), changed)
    };
    if changed {
        write_journal(journal_path, &journal)?;
    }
    Ok(result)
}

pub fn resume_operation(
    root_path: &str,
    operation_id: &str,
    journal_path: &Path,
) -> Result<OperationRecord, super::EngineError> {
    let mut journal = read_journal(journal_path)?;
    let (result, changed) = {
        let record = find_operation_mut(&mut journal, operation_id)?;
        validate_operation_root(root_path, record)?;
        let changed = matches!(
            record.status,
            OperationStatus::Cancelled | OperationStatus::Interrupted
        );
        if changed {
            for file in &mut record.files {
                if file.outcome == FileOperationOutcome::Cancelled {
                    file.outcome = FileOperationOutcome::Planned;
                    file.error = None;
                }
            }
            record.status = OperationStatus::InProgress;
        }
        (record.clone(), changed)
    };
    if changed {
        write_journal(journal_path, &journal)?;
    }
    Ok(result)
}

pub fn advance_undo(
    root_path: &str,
    operation_id: &str,
    journal_path: &Path,
) -> Result<OperationRecord, super::EngineError> {
    let mut journal = read_journal(journal_path)?;
    let (index, snapshot) = {
        let record = find_operation_mut(&mut journal, operation_id)?;
        validate_operation_root(root_path, record)?;
        if !matches!(
            record.status,
            OperationStatus::Completed
                | OperationStatus::PartiallyCompleted
                | OperationStatus::UndoInProgress
                | OperationStatus::UndoInterrupted
                | OperationStatus::PartiallyReverted
        ) {
            return Ok(record.clone());
        }
        // A new explicit undo request may retry entries that previously
        // failed a safe preflight (for example, an original path conflict).
        // Each retry still revalidates identities and paths in secure_rename.
        // Clear old errors once, then let the normal per-file checkpoint
        // record the result of this attempt.
        if record.status == OperationStatus::PartiallyReverted {
            for file in &mut record.files {
                if file.outcome == FileOperationOutcome::Completed && file.undo_error.is_some() {
                    file.undo_error = None;
                }
            }
        }
        let Some(index) = record.files.iter().rposition(|file| {
            file.outcome == FileOperationOutcome::Completed && file.undo_error.is_none()
        }) else {
            finish_undo(record);
            let result = record.clone();
            write_journal(journal_path, &journal)?;
            return Ok(result);
        };
        record.status = OperationStatus::UndoInProgress;
        record.files[index].outcome = FileOperationOutcome::UndoInProgress;
        (index, record.clone())
    };
    write_journal(journal_path, &journal)?;
    let file = &snapshot.files[index];
    let expected_identity = file.destination_identity.as_ref().unwrap_or(&file.identity);
    let moved = secure_rename(
        Path::new(&snapshot.root_path),
        &snapshot.root_identity,
        &file.destination_path,
        &file.original_path,
        expected_identity,
        false,
    );
    let result = {
        let record = find_operation_mut(&mut journal, operation_id)?;
        match moved {
            Ok(_) => {
                record.files[index].outcome = FileOperationOutcome::Reverted;
                record.files[index].undo_error = None;
            }
            Err(issue) => {
                record.files[index].outcome = if issue.uncertain {
                    record.status = OperationStatus::UndoInterrupted;
                    FileOperationOutcome::UndoInProgress
                } else {
                    FileOperationOutcome::Completed
                };
                record.files[index].undo_error = Some(FileOperationError {
                    kind: issue.kind,
                    message: issue.message,
                });
            }
        }
        if record.status != OperationStatus::UndoInterrupted {
            finish_undo(record);
        }
        record.clone()
    };
    write_journal(journal_path, &journal)?;
    Ok(result)
}

/// Reconciles the only uncertain window: a crash after the filesystem rename
/// but before the completed state reached the durable journal.
pub fn load_and_recover(journal_path: &Path) -> Result<Vec<OperationRecord>, super::EngineError> {
    let mut journal = read_journal(journal_path)?;
    let mut changed = false;
    for record in &mut journal.operations {
        let was_undoing = record.status == OperationStatus::UndoInProgress;
        for file in &mut record.files {
            if file.outcome != FileOperationOutcome::InProgress
                && file.outcome != FileOperationOutcome::UndoInProgress
            {
                continue;
            }
            let undoing = file.outcome == FileOperationOutcome::UndoInProgress;
            let (from, to) = if undoing {
                (&file.destination_path, &file.original_path)
            } else {
                (&file.original_path, &file.destination_path)
            };
            let source =
                inspect_relative(Path::new(&record.root_path), &record.root_identity, from);
            let destination =
                inspect_relative(Path::new(&record.root_path), &record.root_identity, to);
            match (source, destination) {
                (Ok(Some(identity)), Ok(None))
                    if identity == file.identity
                        || file
                            .destination_identity
                            .as_ref()
                            .is_some_and(|expected| *expected == identity) =>
                {
                    file.error = None;
                    file.outcome = if undoing {
                        FileOperationOutcome::Completed
                    } else {
                        FileOperationOutcome::Planned
                    };
                }
                (Ok(None), Ok(Some(identity)))
                    if undoing && same_after_rename(&identity, &file.identity) =>
                {
                    file.undo_error = None;
                    file.outcome = FileOperationOutcome::Reverted;
                }
                (Ok(None), Ok(Some(identity)))
                    if !undoing
                        && file
                            .destination_identity
                            .as_ref()
                            .is_some_and(|expected| *expected == identity) =>
                {
                    file.error = None;
                    file.outcome = FileOperationOutcome::Completed;
                }
                (Ok(None), Ok(Some(identity)))
                    if !undoing && same_after_rename(&identity, &file.identity) =>
                {
                    file.destination_identity = None;
                    file.undo_error = Some(FileOperationError {
                        kind: "recovered_identity_incomplete".to_owned(),
                        message: "The file appears at its destination, but Shellsort stopped before it could durably record the post-move identity. It will not undo this item automatically; verify the destination and move it back manually if needed.".to_owned(),
                    });
                    file.error = None;
                    file.outcome = FileOperationOutcome::Completed;
                }
                _ => {
                    file.outcome = FileOperationOutcome::Failed;
                    let error = FileOperationError {
                        kind: "recovery_uncertain".to_owned(),
                        message: "Shellsort could not prove which path still contains the recorded file. Leave both paths untouched and inspect them manually before continuing.".to_owned(),
                    };
                    if undoing {
                        file.undo_error = Some(error);
                    } else {
                        file.error = Some(error);
                    }
                }
            }
            changed = true;
        }
        if matches!(
            record.status,
            OperationStatus::InProgress | OperationStatus::UndoInProgress
        ) {
            if was_undoing {
                if record
                    .files
                    .iter()
                    .any(|file| file.outcome == FileOperationOutcome::UndoInProgress)
                {
                    record.status = OperationStatus::UndoInterrupted;
                } else {
                    finish_undo(record);
                }
            } else if record.files.iter().any(|file| {
                matches!(
                    file.outcome,
                    FileOperationOutcome::Planned | FileOperationOutcome::InProgress
                )
            }) {
                record.status = OperationStatus::Interrupted;
            } else {
                finish_operation(record);
            }
            changed = true;
        }
        if record.status == OperationStatus::Interrupted
            && !record.files.iter().any(|file| {
                matches!(
                    file.outcome,
                    FileOperationOutcome::Planned | FileOperationOutcome::InProgress
                )
            })
        {
            finish_operation(record);
            changed = true;
        }
    }
    if changed {
        write_journal(journal_path, &journal)?;
    }
    Ok(journal.operations)
}

/// Reads operation history without attempting interrupted-operation recovery.
pub fn read_operation_history(
    journal_path: &Path,
) -> Result<Vec<OperationRecord>, super::EngineError> {
    Ok(read_journal(journal_path)?.operations)
}

fn finish_operation(record: &mut OperationRecord) {
    let pending = record.files.iter().any(|file| {
        file.outcome == FileOperationOutcome::Planned
            || file.outcome == FileOperationOutcome::InProgress
    });
    if pending {
        record.status = OperationStatus::InProgress;
        return;
    }
    let completed = record
        .files
        .iter()
        .filter(|file| file.outcome == FileOperationOutcome::Completed)
        .count();
    let failed = record.files.iter().any(|file| {
        matches!(
            file.outcome,
            FileOperationOutcome::Failed | FileOperationOutcome::Skipped
        )
    });
    record.status = if completed == record.files.len() {
        OperationStatus::Completed
    } else if completed > 0 {
        OperationStatus::PartiallyCompleted
    } else if failed {
        OperationStatus::Failed
    } else {
        OperationStatus::Cancelled
    };
}

fn finish_undo(record: &mut OperationRecord) {
    let pending = record.files.iter().any(|file| {
        file.outcome == FileOperationOutcome::UndoInProgress
            || (file.outcome == FileOperationOutcome::Completed && file.undo_error.is_none())
    });
    if pending {
        record.status = OperationStatus::UndoInProgress;
        return;
    }
    let completed = record
        .files
        .iter()
        .any(|file| file.outcome == FileOperationOutcome::Completed);
    record.status = if completed {
        OperationStatus::PartiallyReverted
    } else {
        OperationStatus::Reverted
    };
}

fn find_operation_mut<'a>(
    journal: &'a mut Journal,
    id: &str,
) -> Result<&'a mut OperationRecord, super::EngineError> {
    journal
        .operations
        .iter_mut()
        .find(|record| record.operation_id == id)
        .ok_or_else(|| {
            super::EngineError::new(
                super::EngineErrorKind::OperationNotFound,
                "This operation is not in the local history.",
            )
        })
}

fn validate_operation_root(
    root_path: &str,
    record: &OperationRecord,
) -> Result<(), super::EngineError> {
    let requested = Path::new(root_path);
    let metadata = fs::symlink_metadata(requested).map_err(|_| stale_plan_error())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(stale_plan_error());
    }
    let canonical = fs::canonicalize(requested).map_err(|_| stale_plan_error())?;
    if canonical.to_string_lossy() != record.root_path {
        return Err(stale_plan_error());
    }
    let identity = file_identity_from_path(&canonical).map_err(|_| stale_plan_error())?;
    if identity.device != record.root_identity.device
        || identity.inode != record.root_identity.inode
    {
        return Err(stale_plan_error());
    }
    Ok(())
}

#[cfg(unix)]
fn file_identity_from_path(path: &Path) -> std::io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path)?;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        size_bytes: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
    })
}

#[cfg(not(unix))]
fn file_identity_from_path(path: &Path) -> std::io::Result<FileIdentity> {
    let metadata = fs::symlink_metadata(path)?;
    let modified = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Ok(FileIdentity {
        device: 0,
        inode: 0,
        size_bytes: metadata.len(),
        modified_seconds: modified.as_secs() as i64,
        modified_nanoseconds: modified.subsec_nanos() as i64,
        changed_seconds: 0,
        changed_nanoseconds: 0,
    })
}

fn read_journal(path: &Path) -> Result<Journal, super::EngineError> {
    if let Some(parent) = path.parent()
        && let Ok(metadata) = fs::symlink_metadata(parent)
        && (metadata.file_type().is_symlink() || !metadata.is_dir())
    {
        return Err(journal_error(
            "The operation-history folder is not a real directory. Shellsort left it untouched.",
        ));
    }
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err(journal_error(
            "The operation journal path is not a regular file. Shellsort left it untouched.",
        ));
    }
    match fs::read(path) {
        Ok(bytes) => {
            let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| journal_error("The operation journal is malformed. Shellsort left it untouched; make a backup and inspect it before recovery."))?;
            if journal.version != JOURNAL_VERSION {
                return Err(journal_error(
                    "This operation journal uses an unsupported version. Shellsort left it untouched.",
                ));
            }
            Ok(journal)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Journal {
            version: JOURNAL_VERSION,
            operations: Vec::new(),
        }),
        Err(error) => Err(journal_error(format!(
            "Could not read the operation journal: {error}"
        ))),
    }
}

fn write_journal(path: &Path, journal: &Journal) -> Result<(), super::EngineError> {
    let requested_parent = path
        .parent()
        .ok_or_else(|| journal_error("The operation journal has no parent directory."))?;
    let ancestor = requested_parent
        .parent()
        .ok_or_else(|| journal_error("The operation-history folder has no trusted parent."))?;
    ensure_journal_parent(requested_parent)?;
    let canonical_ancestor = ancestor.canonicalize().map_err(|error| {
        journal_error(format!(
            "Could not validate the private operation-history location: {error}"
        ))
    })?;
    let parent_name = requested_parent
        .file_name()
        .ok_or_else(|| journal_error("The operation-history folder name is invalid."))?;
    let parent = canonical_ancestor.join(parent_name);
    let journal_path = parent.join(
        path.file_name()
            .ok_or_else(|| journal_error("The operation journal filename is invalid."))?,
    );
    if !parent.exists() {
        fs::create_dir(&parent).map_err(|error| {
            journal_error(format!(
                "Could not create the private operation-history folder: {error}"
            ))
        })?;
    }
    let metadata = fs::symlink_metadata(&parent).map_err(|error| {
        journal_error(format!(
            "Could not inspect the operation-history folder: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(journal_error(
            "The operation-history folder is not a real directory. Shellsort stopped without changing the journal.",
        ));
    }
    if let Ok(metadata) = fs::symlink_metadata(&journal_path)
        && (metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err(journal_error(
            "The journal destination is not a regular file. Shellsort left it untouched.",
        ));
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".operations-{}-{nanos}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(journal).map_err(|error| {
        journal_error(format!("Could not encode the operation journal: {error}"))
    })?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| {
            journal_error(format!("Could not create a safe journal update: {error}"))
        })?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(journal_error(format!(
            "Could not flush the operation journal update: {error}"
        )));
    }
    if let Err(error) = fs::rename(&temporary, &journal_path) {
        let _ = fs::remove_file(&temporary);
        return Err(journal_error(format!(
            "Could not atomically replace the operation journal: {error}"
        )));
    }
    if let Ok(directory) = OpenOptions::new().read(true).open(parent) {
        directory.sync_all().map_err(|error| {
            journal_error(format!(
                "Could not flush the operation-history folder: {error}"
            ))
        })?;
    }
    Ok(())
}

fn journal_error(message: impl Into<String>) -> super::EngineError {
    super::EngineError::new(super::EngineErrorKind::JournalError, message)
}

fn stale_plan_error() -> super::EngineError {
    super::EngineError::new(
        super::EngineErrorKind::StalePlan,
        "This preview no longer matches the selected folder. Rescan the folder and review the new preview before organizing.",
    )
}

#[derive(Debug)]
struct MoveIssue {
    kind: String,
    message: String,
    skipped: bool,
    uncertain: bool,
}

#[cfg(target_os = "macos")]
fn secure_rename(
    root: &Path,
    expected_root: &FileIdentity,
    source: &RelativePath,
    destination: &RelativePath,
    expected: &FileIdentity,
    create_destination_dirs: bool,
) -> Result<FileIdentity, MoveIssue> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    if is_macos_metadata_path(source) || is_macos_metadata_path(destination) {
        return Err(move_issue_skip(
            "macos_metadata_excluded",
            "macOS metadata files are excluded from organization and undo.",
        ));
    }
    let root_name = CString::new(root.as_os_str().as_bytes())
        .map_err(|_| move_issue("unsafe_path", "The selected folder path is invalid."))?;
    let root_fd = unsafe {
        libc::open(
            root_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if root_fd < 0 {
        return Err(last_issue("Could not open the selected folder safely."));
    }
    let root_dir = unsafe { std::fs::File::from_raw_fd(root_fd) };
    let actual_root = file_identity_from_metadata(&root_dir.metadata().map_err(|error| {
        move_issue(
            "root_unavailable",
            format!("Could not inspect the selected folder: {error}"),
        )
    })?);
    if actual_root.device != expected_root.device || actual_root.inode != expected_root.inode {
        return Err(move_issue(
            "root_changed",
            "The selected folder was replaced after the preview. Rescan it before continuing.",
        ));
    }
    let (source_parent, source_name) = open_parent(root_dir.as_raw_fd(), source, false)?;
    let (destination_parent, destination_name) =
        open_parent(root_dir.as_raw_fd(), destination, create_destination_dirs)?;
    let source_c = CString::new(source_name.as_bytes())
        .map_err(|_| move_issue("unsafe_path", "The source filename is invalid."))?;
    let destination_c = CString::new(destination_name.as_bytes())
        .map_err(|_| move_issue("unsafe_path", "The destination filename is invalid."))?;
    let source_identity = identity_at(source_parent.as_raw_fd(), &source_c)?.ok_or_else(|| {
        move_issue_skip("source_missing", "The source file is no longer present.")
    })?;
    if source_identity != *expected {
        return Err(move_issue_skip(
            "source_changed",
            "The source file changed after the preview; it was skipped.",
        ));
    }
    if identity_at(destination_parent.as_raw_fd(), &destination_c)?.is_some() {
        return Err(move_issue_skip(
            "destination_exists",
            "The destination is already occupied; Shellsort never replaces existing entries.",
        ));
    }
    let result = unsafe {
        libc::renameatx_np(
            source_parent.as_raw_fd(),
            source_c.as_ptr(),
            destination_parent.as_raw_fd(),
            destination_c.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(move_issue_skip(
                "destination_exists",
                "The destination became occupied; the file was not replaced.",
            ));
        }
        if error.raw_os_error() == Some(libc::EXDEV) {
            return Err(move_issue(
                "cross_volume",
                "This destination is on another filesystem. Shellsort left the source in place because safe cross-volume moves are not supported.",
            ));
        }
        return Err(move_issue(
            "rename_failed",
            format!("The file could not be moved safely: {error}"),
        ));
    }
    source_parent.sync_all().map_err(|error| uncertain_issue("sync_uncertain", format!("The move completed but its source folder could not be flushed: {error}. Inspect operation recovery before retrying.")))?;
    destination_parent.sync_all().map_err(|error| uncertain_issue("sync_uncertain", format!("The move completed but its destination folder could not be flushed: {error}. Inspect operation recovery before retrying.")))?;
    identity_at(destination_parent.as_raw_fd(), &destination_c)?.ok_or_else(|| uncertain_issue("move_uncertain", "The destination could not be revalidated after the move; inspect operation recovery before retrying."))
}

#[cfg(not(target_os = "macos"))]
fn secure_rename(
    _: &Path,
    _: &FileIdentity,
    _: &RelativePath,
    _: &RelativePath,
    _: &FileIdentity,
    _: bool,
) -> Result<FileIdentity, MoveIssue> {
    Err(move_issue(
        "unsupported_platform",
        "Safe exclusive moves are currently enabled only on macOS.",
    ))
}

#[cfg(target_os = "macos")]
fn open_parent(
    root_fd: std::os::fd::RawFd,
    path: &RelativePath,
    create: bool,
) -> Result<(std::fs::File, std::ffi::OsString), MoveIssue> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
        path::Component,
    };
    let path = path.as_path();
    let parts: Vec<_> = path.components().collect();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(move_issue(
            "unsafe_path",
            "A source or destination path contains an unsafe component.",
        ));
    }
    let mut current = unsafe { std::fs::File::from_raw_fd(libc::dup(root_fd)) };
    for component in &parts[..parts.len() - 1] {
        let name = CString::new(component.as_os_str().as_bytes())
            .map_err(|_| move_issue("unsafe_path", "A path component is invalid."))?;
        let mut fd = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0
            && create
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
        {
            if unsafe { libc::mkdirat(current.as_raw_fd(), name.as_ptr(), 0o755) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(move_issue(
                        "directory_create_failed",
                        format!("Could not create a destination folder safely: {error}"),
                    ));
                }
            }
            fd = unsafe {
                libc::openat(
                    current.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
        }
        if fd < 0 {
            return Err(last_issue(
                "A parent folder is missing, inaccessible, or a symbolic link.",
            ));
        }
        current = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    Ok((
        current,
        parts
            .last()
            .expect("nonempty path")
            .as_os_str()
            .to_os_string(),
    ))
}

#[cfg(target_os = "macos")]
fn identity_at(
    parent_fd: std::os::fd::RawFd,
    name: &std::ffi::CString,
) -> Result<Option<FileIdentity>, MoveIssue> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            parent_fd,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(move_issue(
            "metadata_failed",
            format!("Could not verify a file path safely: {error}"),
        ));
    }
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(move_issue_skip(
            "not_regular_file",
            "The entry is now a symbolic link or special file; Shellsort skipped it.",
        ));
    }
    Ok(Some(FileIdentity {
        device: stat.st_dev as u64,
        inode: stat.st_ino,
        size_bytes: stat.st_size as u64,
        modified_seconds: stat.st_mtime,
        modified_nanoseconds: stat.st_mtime_nsec,
        changed_seconds: stat.st_ctime,
        changed_nanoseconds: stat.st_ctime_nsec,
    }))
}

#[cfg(target_os = "macos")]
fn file_identity_from_metadata(metadata: &std::fs::Metadata) -> FileIdentity {
    use std::os::unix::fs::MetadataExt;
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        size_bytes: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
    }
}

#[cfg(target_os = "macos")]
fn inspect_relative(
    root: &Path,
    root_identity: &FileIdentity,
    path: &RelativePath,
) -> Result<Option<FileIdentity>, MoveIssue> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    let root_name = CString::new(root.as_os_str().as_bytes())
        .map_err(|_| move_issue("unsafe_path", "The selected folder path is invalid."))?;
    let fd = unsafe {
        libc::open(
            root_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(last_issue(
            "Could not open the selected folder during recovery.",
        ));
    }
    let root_file = unsafe { std::fs::File::from_raw_fd(fd) };
    let root_actual = file_identity_from_metadata(
        &root_file
            .metadata()
            .map_err(|error| move_issue("root_unavailable", error.to_string()))?,
    );
    if root_actual.device != root_identity.device || root_actual.inode != root_identity.inode {
        return Err(move_issue(
            "root_changed",
            "The selected folder was replaced; automatic recovery was stopped.",
        ));
    }
    let (parent, name) = open_parent(root_file.as_raw_fd(), path, false)?;
    let name = CString::new(name.as_bytes())
        .map_err(|_| move_issue("unsafe_path", "A path component is invalid."))?;
    identity_at(parent.as_raw_fd(), &name)
}

#[cfg(not(target_os = "macos"))]
fn inspect_relative(
    _: &Path,
    _: &FileIdentity,
    _: &RelativePath,
) -> Result<Option<FileIdentity>, MoveIssue> {
    Err(move_issue(
        "unsupported_platform",
        "Recovery inspection is supported only on macOS.",
    ))
}

fn move_issue(kind: &str, message: impl Into<String>) -> MoveIssue {
    MoveIssue {
        kind: kind.to_owned(),
        message: message.into(),
        skipped: false,
        uncertain: false,
    }
}
fn uncertain_issue(kind: &str, message: impl Into<String>) -> MoveIssue {
    MoveIssue {
        kind: kind.to_owned(),
        message: message.into(),
        skipped: false,
        uncertain: true,
    }
}
fn same_after_rename(left: &FileIdentity, right: &FileIdentity) -> bool {
    left.device == right.device
        && left.inode == right.inode
        && left.size_bytes == right.size_bytes
        && left.modified_seconds == right.modified_seconds
        && left.modified_nanoseconds == right.modified_nanoseconds
}
fn move_issue_skip(kind: &str, message: impl Into<String>) -> MoveIssue {
    MoveIssue {
        kind: kind.to_owned(),
        message: message.into(),
        skipped: true,
        uncertain: false,
    }
}
fn last_issue(context: &str) -> MoveIssue {
    let error = std::io::Error::last_os_error();
    move_issue("io_error", format!("{context} {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organizer::{OrganizationSettings, generate_plan_with_settings};
    use std::{fs, path::PathBuf};
    use tempfile::tempdir;

    fn fixture(name: &str, bytes: &[u8]) -> (tempfile::TempDir, OrganizationPlan) {
        let root = tempdir().unwrap();
        fs::write(root.path().join(name), bytes).unwrap();
        let plan = generate_plan_with_settings(
            root.path().to_str().unwrap(),
            &OrganizationSettings::default(),
        )
        .unwrap();
        (root, plan)
    }

    fn journal(root: &Path) -> PathBuf {
        root.join("journal").join("operations.json")
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn moves_exclusively_and_undoes_without_overwrite() {
        let (root, plan) = fixture("report.pdf", b"original");
        let path = journal(root.path());
        let settings = OrganizationSettings::default();
        let mut op =
            start_operation(root.path().to_str().unwrap(), &plan, &settings, &path).unwrap();
        op = advance_operation(root.path().to_str().unwrap(), &op.operation_id, &path).unwrap();
        assert_eq!(op.status, OperationStatus::Completed);
        assert_eq!(
            fs::read(root.path().join("Documents/report.pdf")).unwrap(),
            b"original"
        );
        op = advance_undo(root.path().to_str().unwrap(), &op.operation_id, &path).unwrap();
        assert_eq!(op.status, OperationStatus::Reverted);
        assert_eq!(
            fs::read(root.path().join("report.pdf")).unwrap(),
            b"original"
        );
        assert!(!root.path().join("Documents/report.pdf").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn two_file_undo_restores_every_entry_and_records_truthful_outcomes() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("Example Folder")).unwrap();
        fs::write(root.path().join("Example Folder/notes.txt"), b"notes").unwrap();
        fs::write(root.path().join("copy.txt"), b"copy").unwrap();
        let plan = generate_plan_with_settings(
            root.path().to_str().unwrap(),
            &OrganizationSettings::default(),
        )
        .unwrap();
        let path = journal(root.path());
        let settings = OrganizationSettings::default();
        let mut operation =
            start_operation(root.path().to_str().unwrap(), &plan, &settings, &path).unwrap();
        while matches!(
            operation.status,
            OperationStatus::Planned | OperationStatus::InProgress
        ) {
            operation = advance_operation(
                root.path().to_str().unwrap(),
                &operation.operation_id,
                &path,
            )
            .unwrap();
        }
        assert_eq!(operation.status, OperationStatus::Completed);

        while matches!(
            operation.status,
            OperationStatus::UndoInProgress | OperationStatus::UndoInterrupted
        ) || operation.status == OperationStatus::Completed
        {
            operation = advance_undo(
                root.path().to_str().unwrap(),
                &operation.operation_id,
                &path,
            )
            .unwrap();
        }

        assert_eq!(operation.status, OperationStatus::Reverted);
        assert!(root.path().join("Example Folder/notes.txt").exists());
        assert!(root.path().join("copy.txt").exists());
        assert!(!root.path().join("Documents/notes.txt").exists());
        assert!(!root.path().join("Documents/copy.txt").exists());
        assert!(
            operation
                .files
                .iter()
                .all(|file| file.outcome == FileOperationOutcome::Reverted)
        );
        let history = read_operation_history(&path).unwrap();
        assert_eq!(history[0].status, OperationStatus::Reverted);
        assert!(
            history[0]
                .files
                .iter()
                .all(|file| file.outcome == FileOperationOutcome::Reverted)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rejects_stale_preview_and_source_changes() {
        let (root, plan) = fixture("report.pdf", b"old");
        let path = journal(root.path());
        let settings = OrganizationSettings::default();
        fs::write(root.path().join("report.pdf"), b"changed size").unwrap();
        assert_eq!(
            start_operation(root.path().to_str().unwrap(), &plan, &settings, &path)
                .unwrap_err()
                .kind,
            super::super::EngineErrorKind::StalePlan
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn source_modified_after_start_is_skipped_and_left_in_place() {
        let (root, plan) = fixture("report.pdf", b"before");
        let path = journal(root.path());
        let operation = start_operation(
            root.path().to_str().unwrap(),
            &plan,
            &OrganizationSettings::default(),
            &path,
        )
        .unwrap();
        fs::write(root.path().join("report.pdf"), b"after, different size").unwrap();
        let result = advance_operation(
            root.path().to_str().unwrap(),
            &operation.operation_id,
            &path,
        )
        .unwrap();
        assert_eq!(result.files[0].outcome, FileOperationOutcome::Skipped);
        assert_eq!(
            fs::read(root.path().join("report.pdf")).unwrap(),
            b"after, different size"
        );
        assert!(!root.path().join("Documents/report.pdf").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn destination_created_after_confirmation_is_skipped_without_replacement() {
        let (root, plan) = fixture("report.pdf", b"original");
        let path = journal(root.path());
        let settings = OrganizationSettings::default();
        let op = start_operation(root.path().to_str().unwrap(), &plan, &settings, &path).unwrap();
        fs::create_dir(root.path().join("Documents")).unwrap();
        fs::write(root.path().join("Documents/report.pdf"), b"keep").unwrap();
        let op = advance_operation(root.path().to_str().unwrap(), &op.operation_id, &path).unwrap();
        assert_eq!(op.files[0].outcome, FileOperationOutcome::Skipped);
        assert_eq!(
            fs::read(root.path().join("Documents/report.pdf")).unwrap(),
            b"keep"
        );
        assert_eq!(
            fs::read(root.path().join("report.pdf")).unwrap(),
            b"original"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn undo_conflict_keeps_both_files_and_reports_manual_resolution() {
        let (root, plan) = fixture("report.pdf", b"original");
        let path = journal(root.path());
        let operation = start_operation(
            root.path().to_str().unwrap(),
            &plan,
            &OrganizationSettings::default(),
            &path,
        )
        .unwrap();
        let operation = advance_operation(
            root.path().to_str().unwrap(),
            &operation.operation_id,
            &path,
        )
        .unwrap();
        fs::write(root.path().join("report.pdf"), b"new file at original path").unwrap();
        let result = advance_undo(
            root.path().to_str().unwrap(),
            &operation.operation_id,
            &path,
        )
        .unwrap();
        assert_eq!(result.status, OperationStatus::PartiallyReverted);
        assert!(result.files[0].undo_error.is_some());
        assert_eq!(
            fs::read(root.path().join("report.pdf")).unwrap(),
            b"new file at original path"
        );
        assert_eq!(
            fs::read(root.path().join("Documents/report.pdf")).unwrap(),
            b"original"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn partial_undo_conflict_is_truthful_and_retries_after_conflict_is_removed() {
        let (root, plan) = fixture("report.pdf", b"original");
        let path = journal(root.path());
        let operation = start_operation(
            root.path().to_str().unwrap(),
            &plan,
            &OrganizationSettings::default(),
            &path,
        )
        .unwrap();
        let operation = advance_operation(
            root.path().to_str().unwrap(),
            &operation.operation_id,
            &path,
        )
        .unwrap();
        fs::write(root.path().join("report.pdf"), b"keep this existing file").unwrap();

        let partial = advance_undo(
            root.path().to_str().unwrap(),
            &operation.operation_id,
            &path,
        )
        .unwrap();
        assert_eq!(partial.status, OperationStatus::PartiallyReverted);
        assert_eq!(partial.files[0].outcome, FileOperationOutcome::Completed);
        assert!(partial.files[0].undo_error.is_some());
        assert_eq!(
            fs::read(root.path().join("report.pdf")).unwrap(),
            b"keep this existing file"
        );
        assert_eq!(
            fs::read(root.path().join("Documents/report.pdf")).unwrap(),
            b"original"
        );

        fs::remove_file(root.path().join("report.pdf")).unwrap();
        let retried = advance_undo(
            root.path().to_str().unwrap(),
            &operation.operation_id,
            &path,
        )
        .unwrap();
        assert_eq!(retried.status, OperationStatus::Reverted);
        assert_eq!(retried.files[0].outcome, FileOperationOutcome::Reverted);
        assert!(retried.files[0].undo_error.is_none());
        assert!(root.path().join("report.pdf").exists());
        assert!(!root.path().join("Documents/report.pdf").exists());
        assert_eq!(
            read_operation_history(&path).unwrap()[0].status,
            OperationStatus::Reverted
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn journal_recovers_move_completed_before_completion_record() {
        let (root, plan) = fixture("report.pdf", b"original");
        let path = journal(root.path());
        let settings = OrganizationSettings::default();
        let op = start_operation(root.path().to_str().unwrap(), &plan, &settings, &path).unwrap();
        let mut state = read_journal(&path).unwrap();
        state.operations[0].status = OperationStatus::InProgress;
        state.operations[0].files[0].outcome = FileOperationOutcome::InProgress;
        write_journal(&path, &state).unwrap();
        secure_rename(
            root.path(),
            &op.root_identity,
            &op.files[0].original_path,
            &op.files[0].destination_path,
            &op.files[0].identity,
            true,
        )
        .unwrap();
        let recovered = load_and_recover(&path).unwrap();
        assert_eq!(
            recovered[0].files[0].outcome,
            FileOperationOutcome::Completed
        );
        assert_eq!(recovered[0].status, OperationStatus::Completed);
        assert!(recovered[0].files[0].destination_identity.is_none());
        assert!(recovered[0].files[0].undo_error.is_some());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn journal_recovers_and_allows_undo_when_post_move_identity_was_flushed() {
        let (root, plan) = fixture("report.pdf", b"original");
        let path = journal(root.path());
        let settings = OrganizationSettings::default();
        let operation =
            start_operation(root.path().to_str().unwrap(), &plan, &settings, &path).unwrap();
        let mut state = read_journal(&path).unwrap();
        state.operations[0].status = OperationStatus::InProgress;
        state.operations[0].files[0].outcome = FileOperationOutcome::InProgress;
        write_journal(&path, &state).unwrap();
        let destination_identity = secure_rename(
            root.path(),
            &operation.root_identity,
            &operation.files[0].original_path,
            &operation.files[0].destination_path,
            &operation.files[0].identity,
            true,
        )
        .unwrap();
        state = read_journal(&path).unwrap();
        state.operations[0].files[0].destination_identity = Some(destination_identity);
        write_journal(&path, &state).unwrap();
        let recovered = load_and_recover(&path).unwrap();
        assert_eq!(recovered[0].status, OperationStatus::Completed);
        assert!(recovered[0].files[0].undo_error.is_none());
        let undone = advance_undo(
            root.path().to_str().unwrap(),
            &operation.operation_id,
            &path,
        )
        .unwrap();
        assert_eq!(undone.status, OperationStatus::Reverted);
        assert!(root.path().join("report.pdf").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unsafe_symlink_destination_parent_is_rejected() {
        let (root, plan) = fixture("report.pdf", b"original");
        let outside = tempdir().unwrap();
        let path = journal(root.path());
        let op = start_operation(
            root.path().to_str().unwrap(),
            &plan,
            &OrganizationSettings::default(),
            &path,
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("Documents")).unwrap();
        let result =
            advance_operation(root.path().to_str().unwrap(), &op.operation_id, &path).unwrap();
        assert_eq!(result.files[0].outcome, FileOperationOutcome::Failed);
        assert!(outside.path().read_dir().unwrap().next().is_none());
        assert!(root.path().join("report.pdf").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn execution_rejects_macos_metadata_paths_even_from_a_stale_journal() {
        let (root, _) = fixture("ordinary.txt", b"ordinary");
        let metadata_path = root.path().join(".DS_Store");
        fs::write(&metadata_path, b"metadata").unwrap();
        let root_identity =
            file_identity_from_metadata(&fs::symlink_metadata(root.path()).expect("root metadata"));
        let metadata_identity =
            file_identity_from_metadata(&fs::symlink_metadata(&metadata_path).unwrap());
        let source = RelativePath::from_path(Path::new(".DS_Store")).unwrap();
        let destination = RelativePath::from_path(Path::new("Documents/.DS_Store")).unwrap();

        let issue = secure_rename(
            root.path(),
            &root_identity,
            &source,
            &destination,
            &metadata_identity,
            true,
        )
        .unwrap_err();

        assert_eq!(issue.kind, "macos_metadata_excluded");
        assert!(metadata_path.exists());
        assert!(!root.path().join("Documents/.DS_Store").exists());
    }

    #[test]
    fn journal_corruption_is_reported_without_overwriting_it() {
        let root = tempdir().unwrap();
        let path = journal(root.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not-json").unwrap();
        assert!(load_and_recover(&path).is_err());
        assert_eq!(fs::read(path).unwrap(), b"not-json");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cancellation_stops_between_files_and_can_be_resumed_explicitly() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("a.pdf"), b"a").unwrap();
        fs::write(root.path().join("b.pdf"), b"b").unwrap();
        let plan = generate_plan_with_settings(
            root.path().to_str().unwrap(),
            &OrganizationSettings::default(),
        )
        .unwrap();
        let path = journal(root.path());
        let root_path = root.path().to_str().unwrap();
        let operation =
            start_operation(root_path, &plan, &OrganizationSettings::default(), &path).unwrap();
        let operation = advance_operation(root_path, &operation.operation_id, &path).unwrap();
        assert_eq!(operation.status, OperationStatus::InProgress);
        let operation = cancel_operation(&operation.operation_id, &path).unwrap();
        assert_eq!(operation.status, OperationStatus::Cancelled);
        assert_eq!(
            operation
                .files
                .iter()
                .filter(|file| file.outcome == FileOperationOutcome::Cancelled)
                .count(),
            1
        );
        let mut operation = resume_operation(root_path, &operation.operation_id, &path).unwrap();
        while operation.status == OperationStatus::InProgress {
            operation = advance_operation(root_path, &operation.operation_id, &path).unwrap();
        }
        assert_eq!(operation.status, OperationStatus::Completed);
        assert!(root.path().join("Documents/a.pdf").exists());
        assert!(root.path().join("Documents/b.pdf").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn partial_failures_are_recorded_without_overwriting_or_claiming_full_success() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("a.pdf"), b"a").unwrap();
        fs::write(root.path().join("b.pdf"), b"b").unwrap();
        let plan = generate_plan_with_settings(
            root.path().to_str().unwrap(),
            &OrganizationSettings::default(),
        )
        .unwrap();
        let path = journal(root.path());
        let root_path = root.path().to_str().unwrap();
        let operation =
            start_operation(root_path, &plan, &OrganizationSettings::default(), &path).unwrap();
        let operation = advance_operation(root_path, &operation.operation_id, &path).unwrap();
        fs::create_dir_all(root.path().join("Documents")).unwrap();
        fs::write(root.path().join("Documents/b.pdf"), b"keep existing").unwrap();
        let operation = advance_operation(root_path, &operation.operation_id, &path).unwrap();
        assert_eq!(operation.status, OperationStatus::PartiallyCompleted);
        assert_eq!(
            operation
                .files
                .iter()
                .filter(|file| file.outcome == FileOperationOutcome::Completed)
                .count(),
            1
        );
        assert_eq!(
            operation
                .files
                .iter()
                .filter(|file| file.outcome == FileOperationOutcome::Skipped)
                .count(),
            1
        );
        assert_eq!(
            fs::read(root.path().join("Documents/b.pdf")).unwrap(),
            b"keep existing"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn symlink_replacing_source_is_skipped_without_following_it() {
        let (root, plan) = fixture("report.pdf", b"inside");
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.pdf"), b"outside").unwrap();
        let path = journal(root.path());
        let root_path = root.path().to_str().unwrap();
        let operation =
            start_operation(root_path, &plan, &OrganizationSettings::default(), &path).unwrap();
        fs::remove_file(root.path().join("report.pdf")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.pdf"),
            root.path().join("report.pdf"),
        )
        .unwrap();
        let operation = advance_operation(root_path, &operation.operation_id, &path).unwrap();
        assert_eq!(operation.files[0].outcome, FileOperationOutcome::Skipped);
        assert_eq!(
            fs::read(outside.path().join("secret.pdf")).unwrap(),
            b"outside"
        );
        assert!(
            root.path()
                .join("report.pdf")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
