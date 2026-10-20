use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Documents,
    Images,
    Videos,
    Audio,
    Archives,
    Code,
    Spreadsheets,
    Presentations,
    Other,
}

impl Category {
    pub const fn all() -> [Self; 9] {
        [
            Self::Documents,
            Self::Images,
            Self::Videos,
            Self::Audio,
            Self::Archives,
            Self::Code,
            Self::Spreadsheets,
            Self::Presentations,
            Self::Other,
        ]
    }

    pub fn folder_name(self) -> &'static str {
        match self {
            Self::Documents => "Documents",
            Self::Images => "Images",
            Self::Videos => "Videos",
            Self::Audio => "Audio",
            Self::Archives => "Archives",
            Self::Code => "Code",
            Self::Spreadsheets => "Spreadsheets",
            Self::Presentations => "Presentations",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelativePath(String);

impl RelativePath {
    pub(crate) fn from_path(path: &Path) -> Result<Self, EngineError> {
        use std::path::Component;

        let components = path
            .components()
            .map(|component| match component {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        let Some(components) = components.filter(|components| !components.is_empty()) else {
            return Err(EngineError::new(
                EngineErrorKind::InvalidRelativePath,
                "Path must contain only normal relative components.",
            ));
        };

        Ok(Self(components.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn as_path(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileMetadata {
    pub source: RelativePath,
    pub file_name: String,
    pub extension: Option<String>,
    pub category: Category,
    pub size_bytes: u64,
    pub identity: FileIdentity,
    pub modified_unix_seconds: Option<u64>,
    pub hidden: bool,
}

/// Filesystem identity snapshot used to reject stale or replaced entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
    pub size_bytes: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
    pub changed_seconds: i64,
    pub changed_nanoseconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanWarningKind {
    SymbolicLinkSkipped,
    HiddenDirectorySkipped,
    ExcludedDirectorySkipped,
    InvalidUnicodeNameSkipped,
    NonRegularEntrySkipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanWarning {
    pub path: Option<RelativePath>,
    pub kind: ScanWarningKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanIssueKind {
    DirectoryReadFailed,
    EntryReadFailed,
    MetadataReadFailed,
    DestinationCheckFailed,
    PathOutsideRoot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanIssue {
    pub path: Option<RelativePath>,
    pub kind: ScanIssueKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanResult {
    pub root_path: String,
    pub root_identity: FileIdentity,
    pub files: Vec<FileMetadata>,
    pub warnings: Vec<ScanWarning>,
    pub issues: Vec<ScanIssue>,
    pub is_incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateFileStatusKind {
    Unreadable,
    ChangedDuringHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DuplicateFileStatus {
    pub path: RelativePath,
    pub kind: DuplicateFileStatusKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DuplicateGroup {
    pub size_bytes: u64,
    pub files: Vec<RelativePath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DuplicateScanResult {
    pub root_path: String,
    pub files_scanned: usize,
    pub size_candidates: usize,
    pub groups: Vec<DuplicateGroup>,
    pub uncertain_files: Vec<DuplicateFileStatus>,
    pub warnings: Vec<ScanWarning>,
    pub issues: Vec<ScanIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStatus {
    None,
    ExistingDestination,
    ProposedCollision,
    ExistingAndProposedCollision,
    AlreadyCorrect,
    UnverifiableDestination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Ready,
    Conflict,
    Unchanged,
    CategoryDisabled,
    UnknownTypeKeptInPlace,
    NoDestinationConfigured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanEntry {
    pub source: RelativePath,
    pub destination: Option<RelativePath>,
    pub file_name: String,
    pub extension: Option<String>,
    pub category: Category,
    pub size_bytes: u64,
    pub source_identity: FileIdentity,
    pub status: PlanStatus,
    pub conflict_status: ConflictStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationPlan {
    pub root_path: String,
    pub root_identity: FileIdentity,
    pub entries: Vec<PlanEntry>,
    pub warnings: Vec<ScanWarning>,
    pub issues: Vec<ScanIssue>,
    pub is_incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Planned,
    InProgress,
    UndoInProgress,
    Completed,
    PartiallyCompleted,
    PartiallyReverted,
    Failed,
    Reverted,
    Cancelled,
    Interrupted,
    UndoInterrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileOperationOutcome {
    Planned,
    InProgress,
    UndoInProgress,
    Completed,
    Failed,
    Skipped,
    Reverted,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOperationError {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOperationRecord {
    pub original_path: RelativePath,
    pub destination_path: RelativePath,
    pub identity: FileIdentity,
    pub destination_identity: Option<FileIdentity>,
    pub outcome: FileOperationOutcome,
    pub error: Option<FileOperationError>,
    pub undo_error: Option<FileOperationError>,
    pub source_size_bytes_before: Option<u64>,
    pub source_modified_unix_seconds_before: Option<u64>,
    pub destination_size_bytes_after: Option<u64>,
    pub destination_modified_unix_seconds_after: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRecord {
    pub operation_id: String,
    pub timestamp_unix_seconds: u64,
    pub root_path: String,
    pub root_identity: FileIdentity,
    pub status: OperationStatus,
    pub files: Vec<FileOperationRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineErrorKind {
    RootNotFound,
    RootNotDirectory,
    RootIsSymbolicLink,
    PermissionDenied,
    InvalidRootPath,
    InvalidRelativePath,
    InvalidSettings,
    WorkerFailed,
    IoError,
    StalePlan,
    UnsafeOperation,
    JournalError,
    OperationNotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineError {
    pub kind: EngineErrorKind,
    pub message: String,
}

impl EngineError {
    pub(crate) fn new(kind: EngineErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl Error for EngineError {}
