mod classification;
mod duplicates;
mod model;
mod operations;
mod plan;
mod scan;
mod settings;

pub use duplicates::find_duplicates;
pub use model::{
    Category, ConflictStatus, DuplicateFileStatus, DuplicateFileStatusKind, DuplicateGroup,
    DuplicateScanResult, EngineError, EngineErrorKind, FileIdentity, FileMetadata,
    FileOperationError, FileOperationOutcome, FileOperationRecord, OperationRecord,
    OperationStatus, OrganizationPlan, PlanEntry, PlanStatus, RelativePath, ScanIssue,
    ScanIssueKind, ScanResult, ScanWarning, ScanWarningKind,
};
pub use operations::{
    JournalLock, advance_operation, advance_undo, cancel_operation, load_and_recover,
    read_operation_history, resume_operation, start_operation,
};
pub use plan::{generate_plan, generate_plan_with_settings};
pub use scan::scan_folder;
pub use settings::{CategoryRule, OrganizationSettings, UnknownFileTypeBehavior};

#[cfg(test)]
mod tests;
