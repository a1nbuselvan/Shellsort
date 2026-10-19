mod classification;
mod model;
mod plan;
mod scan;

pub use model::{
    Category, ConflictStatus, EngineError, EngineErrorKind, FileMetadata, OrganizationPlan,
    PlanEntry, RelativePath, ScanIssue, ScanIssueKind, ScanResult, ScanWarning, ScanWarningKind,
};
pub use plan::generate_plan;
pub use scan::scan_folder;

#[cfg(test)]
mod tests;
