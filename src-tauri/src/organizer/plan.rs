use super::{
    model::{
        ConflictStatus, EngineError, OrganizationPlan, PlanEntry, RelativePath, ScanIssue,
        ScanIssueKind, ScanResult,
    },
    scan::scan_folder_with_root,
};
use std::{
    collections::HashMap,
    fs, io,
    path::{Component, Path, PathBuf},
};
use unicode_normalization::UnicodeNormalization;

pub fn generate_plan(root_path: &str) -> Result<OrganizationPlan, EngineError> {
    let (root, scan) = scan_folder_with_root(root_path)?;
    plan_from_scan(root, scan)
}

fn plan_from_scan(root: PathBuf, mut scan: ScanResult) -> Result<OrganizationPlan, EngineError> {
    let mut destinations = Vec::with_capacity(scan.files.len());
    let mut proposed_counts: HashMap<String, usize> = HashMap::new();

    for file in &scan.files {
        let destination_path = Path::new(file.category.folder_name()).join(&file.file_name);
        let destination = RelativePath::from_path(&destination_path)?;
        let already_correct = destination == file.source;
        if !already_correct {
            *proposed_counts
                .entry(destination_collision_key(&destination))
                .or_default() += 1;
        }
        destinations.push((destination, already_correct));
    }

    let mut entries = Vec::with_capacity(scan.files.len());
    for (file, (destination, already_correct)) in scan.files.into_iter().zip(destinations) {
        if already_correct {
            entries.push(PlanEntry {
                source: file.source,
                destination,
                file_name: file.file_name,
                extension: file.extension,
                category: file.category,
                size_bytes: file.size_bytes,
                conflict_status: ConflictStatus::AlreadyCorrect,
                reason: Some("This file is already in its category folder.".to_owned()),
            });
            continue;
        }

        let is_proposed_collision = proposed_counts
            .get(&destination_collision_key(&destination))
            .copied()
            .unwrap_or(0)
            > 1;
        let destination_probe = probe_destination(&root, &destination);
        let conflict_status = match (is_proposed_collision, destination_probe) {
            (true, DestinationProbe::Occupied) => ConflictStatus::ExistingAndProposedCollision,
            (true, DestinationProbe::Unverifiable(message)) => {
                scan.issues.push(ScanIssue {
                    path: Some(file.source.clone()),
                    kind: ScanIssueKind::DestinationCheckFailed,
                    message: format!("Could not safely check the proposed destination: {message}"),
                });
                ConflictStatus::UnverifiableDestination
            }
            (true, DestinationProbe::Absent) => ConflictStatus::ProposedCollision,
            (false, DestinationProbe::Occupied) => ConflictStatus::ExistingDestination,
            (false, DestinationProbe::Unverifiable(message)) => {
                scan.issues.push(ScanIssue {
                    path: Some(file.source.clone()),
                    kind: ScanIssueKind::DestinationCheckFailed,
                    message: format!("Could not safely check the proposed destination: {message}"),
                });
                ConflictStatus::UnverifiableDestination
            }
            (false, DestinationProbe::Absent) => ConflictStatus::None,
        };

        let reason = match conflict_status {
            ConflictStatus::None => None,
            ConflictStatus::ExistingDestination => Some(
                "A filesystem entry already occupies this destination; no overwrite is proposed."
                    .to_owned(),
            ),
            ConflictStatus::ProposedCollision => Some(
                "Another scanned file has the same proposed destination; resolve this collision first."
                    .to_owned(),
            ),
            ConflictStatus::ExistingAndProposedCollision => Some(
                "The destination already exists and multiple scanned files propose it; no overwrite is proposed."
                    .to_owned(),
            ),
            ConflictStatus::UnverifiableDestination => Some(
                "The destination could not be checked safely; treat it as a conflict."
                    .to_owned(),
            ),
            ConflictStatus::AlreadyCorrect => unreachable!("handled above"),
        };

        entries.push(PlanEntry {
            source: file.source,
            destination,
            file_name: file.file_name,
            extension: file.extension,
            category: file.category,
            size_bytes: file.size_bytes,
            conflict_status,
            reason,
        });
    }

    scan.issues.sort_by(|left, right| {
        left.path
            .as_ref()
            .map(RelativePath::as_str)
            .cmp(&right.path.as_ref().map(RelativePath::as_str))
    });
    scan.is_incomplete = !scan.issues.is_empty();

    Ok(OrganizationPlan {
        root_path: scan.root_path,
        entries,
        warnings: scan.warnings,
        issues: scan.issues,
        is_incomplete: scan.is_incomplete,
    })
}

fn destination_collision_key(destination: &RelativePath) -> String {
    destination
        .as_str()
        .nfd()
        .flat_map(char::to_lowercase)
        .collect()
}

enum DestinationProbe {
    Absent,
    Occupied,
    Unverifiable(String),
}

fn probe_destination(root: &Path, destination: &RelativePath) -> DestinationProbe {
    let relative = destination.as_path();
    let components: Vec<_> = relative.components().collect();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return DestinationProbe::Unverifiable(
            "destination is not a safe relative path".to_owned(),
        );
    }

    let mut current = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let is_last = index + 1 == components.len();
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return DestinationProbe::Absent;
            }
            Err(error) => return DestinationProbe::Unverifiable(error.to_string()),
        };

        if metadata.file_type().is_symlink() {
            return DestinationProbe::Occupied;
        }
        if is_last || !metadata.is_dir() {
            return DestinationProbe::Occupied;
        }
    }

    DestinationProbe::Absent
}

#[cfg(test)]
mod tests {
    use super::{DestinationProbe, probe_destination};
    use crate::organizer::RelativePath;
    use std::{fs, path::Path};
    use tempfile::tempdir;

    #[test]
    fn destination_probe_does_not_follow_symbolic_link() {
        let root = tempdir().expect("temporary root");
        let outside = tempdir().expect("temporary outside folder");
        fs::write(outside.path().join("report.pdf"), b"outside").expect("outside file");
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), root.path().join("Documents"))
            .expect("destination directory symlink");

        #[cfg(unix)]
        {
            let relative = RelativePath::from_path(Path::new("Documents/report.pdf"))
                .expect("relative destination");
            assert!(matches!(
                probe_destination(root.path(), &relative),
                DestinationProbe::Occupied
            ));
        }
    }
}
