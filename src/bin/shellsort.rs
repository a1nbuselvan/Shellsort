use serde::Serialize;
use shellsort_lib::organizer::{
    FileOperationOutcome, JournalLock, OperationRecord, OperationStatus, OrganizationPlan,
    OrganizationSettings, PlanStatus, advance_operation, advance_undo, find_duplicates,
    generate_plan_with_settings, load_and_recover, read_operation_history, resume_operation,
    start_operation,
};
use std::{
    collections::HashSet,
    env,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("shellsort {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    let json_errors = args.iter().any(|arg| arg == "--json");
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            if json_errors {
                println!("{}", serde_json::json!({"error": message}));
            }
            eprintln!("shellsort: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("expected a folder path or a command; use --help for usage".to_owned());
    }
    match args[0].as_str() {
        "duplicates" => command_duplicates(args),
        "organize" => command_organize(args),
        "history" => command_history(args),
        "recover" => command_recover(args),
        "resume" => command_resume(args),
        "undo" => command_undo(args),
        value if value.starts_with('-') => Err(format!("unknown option {value}; use --help")),
        _ => command_preview(args),
    }
}

fn command_preview(args: &[String]) -> Result<(), String> {
    let (root, json) = folder_and_json(args, 0)?;
    let plan = fresh_plan(&root)?;
    if json {
        print_json(&plan)
    } else {
        print_plan(&plan);
        Ok(())
    }
}

fn command_duplicates(args: &[String]) -> Result<(), String> {
    let (root, json) = folder_and_json(args, 1)?;
    let result = find_duplicates(&root).map_err(|error| error.to_string())?;
    if json {
        print_json(&result)
    } else {
        println!(
            "Checked {} files; {} size-matched candidates.",
            result.files_scanned, result.size_candidates
        );
        if result.groups.is_empty() {
            println!("No exact duplicate groups found.");
        }
        for group in result.groups {
            println!("\nExact duplicates ({} bytes):", group.size_bytes);
            for file in group.files {
                println!("  {:?}", file.as_str());
            }
        }
        for issue in result.uncertain_files {
            eprintln!(
                "Could not verify {}: {}",
                issue.path.as_str(),
                issue.message
            );
        }
        Ok(())
    }
}

fn command_organize(args: &[String]) -> Result<(), String> {
    let (root, confirm) = confirmed_folder(args, 1, "organize")?;
    let plan = fresh_plan(&root)?;
    print_plan(&plan);
    let (ready, _) = plan_counts(&plan);
    if ready == 0 {
        println!("No conflict-free files are eligible to move.");
        return Ok(());
    }
    confirm_action(
        confirm,
        &format!("Move {ready} files using this fresh plan?"),
        "organize",
    )?;
    run_fresh_operation(&root, &plan)
}

fn command_resume(args: &[String]) -> Result<(), String> {
    ensure_mutation_platform()?;
    let (root, operation_id, confirm) = folder_operation_confirm(args, 1, "resume")?;
    let journal = journal_path()?;
    let recovery_lock =
        JournalLock::acquire_if_journal_exists(&journal).map_err(|error| error.to_string())?;
    let history = if recovery_lock.is_some() {
        load_and_recover(&journal).map_err(|error| error.to_string())?
    } else {
        read_operation_history(&journal).map_err(|error| error.to_string())?
    };
    drop(recovery_lock);
    let record = find_operation(history, &operation_id)?;
    if !matches!(
        record.status,
        OperationStatus::Interrupted | OperationStatus::Cancelled
    ) {
        return Err(format!(
            "operation {operation_id} is {:?} and cannot be resumed",
            record.status
        ));
    }
    let pending = record
        .files
        .iter()
        .filter(|file| {
            matches!(
                file.outcome,
                shellsort_lib::organizer::FileOperationOutcome::Planned
                    | shellsort_lib::organizer::FileOperationOutcome::Cancelled
            )
        })
        .count();
    confirm_action(
        confirm,
        &format!("Resume operation {operation_id} for up to {pending} pending file(s)?"),
        "resume",
    )?;
    let _journal_lock = JournalLock::acquire(&journal).map_err(|error| error.to_string())?;
    let _ = load_and_recover(&journal).map_err(|error| error.to_string())?;
    let latest = find_operation(
        read_operation_history(&journal).map_err(|error| error.to_string())?,
        &operation_id,
    )?;
    if !matches!(
        latest.status,
        OperationStatus::Interrupted | OperationStatus::Cancelled
    ) {
        return Err(format!(
            "operation {operation_id} changed to {:?} and is no longer resumable",
            latest.status
        ));
    }
    let record =
        resume_operation(&root, &operation_id, &journal).map_err(|error| error.to_string())?;
    run_operation_to_completion(&root, record, &journal)
}

fn command_undo(args: &[String]) -> Result<(), String> {
    ensure_mutation_platform()?;
    let (root, operation_id, confirm) = folder_operation_confirm(args, 1, "undo")?;
    let journal = journal_path()?;
    let history = read_operation_history(&journal).map_err(|error| error.to_string())?;
    let record = find_operation(history, &operation_id)?;
    let undoable = record
        .files
        .iter()
        .filter(|file| file.outcome == FileOperationOutcome::Completed)
        .count();
    if undoable == 0 {
        return Err(format!(
            "operation {operation_id} has no safely undoable completed moves"
        ));
    }
    confirm_action(
        confirm,
        &format!("Attempt to undo {undoable} completed move(s) in {operation_id}?"),
        "undo",
    )?;
    let _journal_lock = JournalLock::acquire(&journal).map_err(|error| error.to_string())?;
    let record = find_operation(
        read_operation_history(&journal).map_err(|error| error.to_string())?,
        &operation_id,
    )?;
    let latest_undoable = record
        .files
        .iter()
        .filter(|file| file.outcome == FileOperationOutcome::Completed)
        .count();
    if latest_undoable == 0 {
        return Err(format!(
            "operation {operation_id} no longer has safely undoable moves"
        ));
    }
    if latest_undoable != undoable {
        return Err(format!(
            "operation {operation_id} changed after confirmation; it now has {latest_undoable} safely undoable move(s). Review history and try again"
        ));
    }
    let mut reported = HashSet::new();
    let current = loop {
        let current =
            advance_undo(&root, &operation_id, &journal).map_err(|error| error.to_string())?;
        print_operation_progress(&current, "Undo", &mut reported);
        if !matches!(
            current.status,
            OperationStatus::UndoInProgress | OperationStatus::UndoInterrupted
        ) {
            break current;
        }
    };
    print_operation_summary(&current);
    if matches!(
        current.status,
        OperationStatus::PartiallyReverted | OperationStatus::UndoInterrupted
    ) {
        return Err(format!(
            "undo for operation {operation_id} finished as {:?}; inspect `shellsort history` before retrying",
            current.status
        ));
    }
    Ok(())
}

fn command_history(args: &[String]) -> Result<(), String> {
    let json = no_folder_command(args, 2)?;
    let journal = journal_path()?;
    let _journal_lock =
        JournalLock::acquire_if_journal_exists(&journal).map_err(|error| error.to_string())?;
    let records = read_operation_history(&journal).map_err(|error| error.to_string())?;
    if json {
        print_json(&records)
    } else {
        print_history(&records);
        Ok(())
    }
}

fn command_recover(args: &[String]) -> Result<(), String> {
    let json = no_folder_command(args, 2)?;
    let journal = journal_path()?;
    #[cfg(not(target_os = "macos"))]
    match std::fs::symlink_metadata(&journal) {
        Ok(_) => ensure_mutation_platform()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
    let journal_lock =
        JournalLock::acquire_if_journal_exists(&journal).map_err(|error| error.to_string())?;
    let records = if journal_lock.is_some() {
        ensure_mutation_platform()?;
        load_and_recover(&journal).map_err(|error| error.to_string())?
    } else {
        read_operation_history(&journal).map_err(|error| error.to_string())?
    };
    if json {
        print_json(&records)
    } else {
        println!("Recovery check complete. No files were moved by this command.");
        print_history(&records);
        Ok(())
    }
}

fn run_fresh_operation(root: &str, plan: &OrganizationPlan) -> Result<(), String> {
    ensure_mutation_platform()?;
    let settings = OrganizationSettings::default();
    let journal = journal_path()?;
    let _journal_lock = JournalLock::acquire(&journal).map_err(|error| error.to_string())?;
    let record =
        start_operation(root, plan, &settings, &journal).map_err(|error| error.to_string())?;
    run_operation_to_completion(root, record, &journal)
}

fn ensure_mutation_platform() -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    return Err(
        "file organization, recovery, resume, and undo are currently supported only on macOS; no files were changed"
            .to_owned(),
    );
    #[cfg(target_os = "macos")]
    Ok(())
}

fn run_operation_to_completion(
    root: &str,
    mut record: OperationRecord,
    journal: &std::path::Path,
) -> Result<(), String> {
    let mut reported = HashSet::new();
    loop {
        record = advance_operation(root, &record.operation_id, journal)
            .map_err(|error| error.to_string())?;
        print_operation_progress(&record, "Move", &mut reported);
        if !matches!(
            record.status,
            OperationStatus::Planned | OperationStatus::InProgress
        ) {
            break;
        }
    }
    print_operation_summary(&record);
    if matches!(
        record.status,
        OperationStatus::Failed
            | OperationStatus::PartiallyCompleted
            | OperationStatus::Interrupted
            | OperationStatus::Cancelled
    ) {
        return Err(format!(
            "operation {} finished as {:?}; inspect `shellsort history` for per-file results",
            record.operation_id, record.status
        ));
    }
    Ok(())
}

fn fresh_plan(root: &str) -> Result<OrganizationPlan, String> {
    let plan = generate_plan_with_settings(root, &OrganizationSettings::default())
        .map_err(|error| error.to_string())?;
    if plan.is_incomplete {
        return Err("the folder scan was incomplete; inspect permissions and retry".to_owned());
    }
    Ok(plan)
}

fn print_plan(plan: &OrganizationPlan) {
    let (ready, conflicts) = plan_counts(plan);
    println!("Preview for: {:?}", plan.root_path);
    for entry in &plan.entries {
        let destination = entry
            .destination
            .as_ref()
            .map(|path| path.as_str())
            .unwrap_or("(kept in place)");
        let state = match entry.status {
            PlanStatus::Ready => "READY",
            PlanStatus::Conflict => "CONFLICT",
            PlanStatus::Unchanged => "UNCHANGED",
            PlanStatus::CategoryDisabled => "RULE DISABLED",
            PlanStatus::UnknownTypeKeptInPlace => "UNKNOWN TYPE",
            PlanStatus::NoDestinationConfigured => "NO DESTINATION",
        };
        println!(
            "{state:16} {:?} -> {:?}",
            entry.source.as_str(),
            destination
        );
        if let Some(reason) = &entry.reason {
            println!("                 {reason:?}");
        }
    }
    for warning in &plan.warnings {
        eprintln!("Warning: {:?}", warning.message);
    }
    for issue in &plan.issues {
        eprintln!("Issue: {:?}", issue.message);
    }
    println!(
        "\nSummary: {} file(s) eligible to move; {conflicts} conflict(s).",
        ready
    );
}

fn plan_counts(plan: &OrganizationPlan) -> (usize, usize) {
    let ready = plan
        .entries
        .iter()
        .filter(|entry| entry.status == PlanStatus::Ready)
        .count();
    let conflicts = plan
        .entries
        .iter()
        .filter(|entry| entry.status == PlanStatus::Conflict)
        .count();
    (ready, conflicts)
}

fn print_operation_progress(record: &OperationRecord, verb: &str, reported: &mut HashSet<String>) {
    if let Some(file) = record.files.iter().find(|file| {
        let relevant = if verb == "Undo" {
            file.outcome == FileOperationOutcome::Reverted
        } else {
            matches!(
                file.outcome,
                FileOperationOutcome::Completed
                    | FileOperationOutcome::Skipped
                    | FileOperationOutcome::Failed
            )
        };
        relevant && reported.insert(file.original_path.as_str().to_owned())
    }) {
        eprintln!(
            "{verb}: {:?} → {:?} ({:?})",
            file.original_path.as_str(),
            file.destination_path.as_str(),
            file.outcome
        );
    }
}

fn print_operation_summary(record: &OperationRecord) {
    let still_moved = record
        .files
        .iter()
        .filter(|file| file.outcome == FileOperationOutcome::Completed)
        .count();
    let reverted = record
        .files
        .iter()
        .filter(|file| file.outcome == FileOperationOutcome::Reverted)
        .count();
    let skipped = record
        .files
        .iter()
        .filter(|file| {
            matches!(
                file.outcome,
                shellsort_lib::organizer::FileOperationOutcome::Skipped
            )
        })
        .count();
    let failed = record
        .files
        .iter()
        .filter(|file| {
            matches!(
                file.outcome,
                shellsort_lib::organizer::FileOperationOutcome::Failed
            )
        })
        .count();
    println!(
        "Operation {}: {:?} · {still_moved} currently moved, {reverted} reverted, {skipped} skipped, {failed} failed",
        record.operation_id, record.status,
    );
    for file in &record.files {
        if let Some(error) = file.error.as_ref().or(file.undo_error.as_ref()) {
            eprintln!("{:?}: {:?}", file.original_path.as_str(), error.message);
        }
    }
}

fn print_history(records: &[OperationRecord]) {
    if records.is_empty() {
        println!("No operation history found.");
        return;
    }
    for record in records {
        println!(
            "{}  {:?}  {} file(s)  {:?}",
            record.operation_id,
            record.status,
            record.files.len(),
            record.root_path
        );
        for file in &record.files {
            if let Some(error) = file.error.as_ref().or(file.undo_error.as_ref()) {
                println!(
                    "  {:?}: {:?} → {:?} — {:?}",
                    file.outcome,
                    file.original_path.as_str(),
                    file.destination_path.as_str(),
                    error.message
                );
            }
        }
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

fn folder_and_json(args: &[String], folder_index: usize) -> Result<(String, bool), String> {
    let folder = args
        .get(folder_index)
        .ok_or_else(|| "a folder path is required".to_owned())?;
    let mut json = false;
    for option in &args[folder_index + 1..] {
        if option == "--json" {
            json = true;
        } else {
            return Err(format!("unexpected argument {option:?}"));
        }
    }
    Ok((folder.clone(), json))
}

fn confirmed_folder(
    args: &[String],
    folder_index: usize,
    action: &str,
) -> Result<(String, bool), String> {
    let folder = args
        .get(folder_index)
        .ok_or_else(|| format!("`{action}` requires a folder path"))?;
    let mut confirm = false;
    for option in &args[folder_index + 1..] {
        if option == "--yes" || option == "--confirm" {
            confirm = true;
        } else {
            return Err(format!("unexpected argument {option:?}"));
        }
    }
    Ok((folder.clone(), confirm))
}

fn folder_operation_confirm(
    args: &[String],
    start: usize,
    action: &str,
) -> Result<(String, String, bool), String> {
    let root = args
        .get(start)
        .ok_or_else(|| format!("`{action}` requires a folder path and operation ID"))?;
    let id = args
        .get(start + 1)
        .ok_or_else(|| format!("`{action}` requires an operation ID"))?;
    let mut confirm = false;
    for option in &args[start + 2..] {
        if option == "--yes" || option == "--confirm" {
            confirm = true;
        } else {
            return Err(format!("unexpected argument {option:?}"));
        }
    }
    Ok((root.clone(), id.clone(), confirm))
}

fn no_folder_command(args: &[String], expected_len: usize) -> Result<bool, String> {
    let mut json = false;
    for arg in args.iter().skip(1) {
        if arg == "--json" {
            json = true;
        } else {
            return Err(format!("unexpected argument {arg:?}"));
        }
    }
    if args.len() > expected_len {
        return Err("too many arguments".to_owned());
    }
    Ok(json)
}

fn confirm_action(explicit: bool, question: &str, keyword: &str) -> Result<(), String> {
    if explicit {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        return Err(format!(
            "refusing to {keyword} without an interactive terminal; pass --yes to explicitly confirm"
        ));
    }
    eprint!("{question}\nType '{keyword}' to continue: ");
    io::stderr().flush().map_err(|error| error.to_string())?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|error| error.to_string())?;
    if answer.trim() != keyword {
        return Err("confirmation did not match; no operation was started".to_owned());
    }
    Ok(())
}

fn find_operation(records: Vec<OperationRecord>, id: &str) -> Result<OperationRecord, String> {
    records
        .into_iter()
        .find(|record| record.operation_id == id)
        .ok_or_else(|| format!("operation {id:?} was not found in Shellsort's local history"))
}

fn journal_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    let base = {
        let home = env::var_os("HOME").ok_or_else(|| {
            "HOME is not set; cannot locate the local operation journal".to_owned()
        })?;
        PathBuf::from(home).join("Library/Application Support/app.shellsort.desktop")
    };
    #[cfg(target_os = "windows")]
    let base = {
        let data = env::var_os("LOCALAPPDATA")
            .or_else(|| env::var_os("APPDATA"))
            .or_else(|| env::var_os("USERPROFILE"))
            .or_else(|| env::var_os("HOME"))
            .ok_or_else(|| "Could not locate the local operation journal directory".to_owned())?;
        PathBuf::from(data).join("app.shellsort.desktop")
    };
    #[cfg(target_os = "linux")]
    let base = {
        let home = env::var_os("HOME").ok_or_else(|| {
            "HOME is not set; cannot locate the local operation journal".to_owned()
        })?;
        let data = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| PathBuf::from(home).join(".local/share"));
        data.join("app.shellsort.desktop")
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    compile_error!("Shellsort journal storage is not configured for this target");
    Ok(base.join("operations.json"))
}

fn print_help() {
    println!(
        "Shellsort {} — safe file organization for macOS\n\nUSAGE:\n  shellsort <folder> [--json]\n  shellsort duplicates <folder> [--json]\n  shellsort organize <folder> [--yes]\n  shellsort history [--json]\n  shellsort recover [--json]\n  shellsort resume <folder> <operation-id> [--yes]\n  shellsort undo <folder> <operation-id> [--yes]\n\nThe default command scans and prints a read-only preview. It never moves files.\n`organize`, `resume`, and `undo` require an interactive typed confirmation or explicit --yes.\n`recover` reconciles interrupted journal state and does not move files.\nOperation history is stored locally under the Shellsort application-data folder.\nCross-filesystem moves are rejected; existing destinations are never replaced.",
        env!("CARGO_PKG_VERSION")
    );
}
