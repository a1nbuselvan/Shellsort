use serde::Serialize;
use std::{error::Error, fmt, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RelativePath(String);

impl RelativePath {
    pub(crate) fn from_path(path: &Path) -> Result<Self, EngineError> {
        use std::path::Component;

        if path.as_os_str().is_empty()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(EngineError::new(
                EngineErrorKind::InvalidRelativePath,
                "Path must contain only normal relative components.",
            ));
        }

        let value = path.to_str().ok_or_else(|| {
            EngineError::new(
                EngineErrorKind::InvalidRelativePath,
                "Path cannot be represented as valid Unicode.",
            )
        })?;

        Ok(Self(value.to_owned()))
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
    pub modified_unix_seconds: Option<u64>,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanWarningKind {
    SymbolicLinkSkipped,
    HiddenDirectorySkipped,
    ExcludedDirectorySkipped,
    InvalidUnicodeNameSkipped,
    NonRegularEntrySkipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanWarning {
    pub path: Option<RelativePath>,
    pub kind: ScanWarningKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanIssueKind {
    DirectoryReadFailed,
    EntryReadFailed,
    MetadataReadFailed,
    DestinationCheckFailed,
    PathOutsideRoot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanIssue {
    pub path: Option<RelativePath>,
    pub kind: ScanIssueKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanResult {
    pub root_path: String,
    pub files: Vec<FileMetadata>,
    pub warnings: Vec<ScanWarning>,
    pub issues: Vec<ScanIssue>,
    pub is_incomplete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStatus {
    None,
    ExistingDestination,
    ProposedCollision,
    ExistingAndProposedCollision,
    AlreadyCorrect,
    UnverifiableDestination,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanEntry {
    pub source: RelativePath,
    pub destination: RelativePath,
    pub file_name: String,
    pub extension: Option<String>,
    pub category: Category,
    pub size_bytes: u64,
    pub conflict_status: ConflictStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrganizationPlan {
    pub root_path: String,
    pub entries: Vec<PlanEntry>,
    pub warnings: Vec<ScanWarning>,
    pub issues: Vec<ScanIssue>,
    pub is_incomplete: bool,
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
    IoError,
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
