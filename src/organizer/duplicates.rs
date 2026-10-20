use super::{
    DuplicateFileStatus, DuplicateFileStatusKind, DuplicateGroup, DuplicateScanResult, EngineError,
    RelativePath, scan::scan_folder_with_root,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::Path,
};

/// Finds byte-identical regular files without changing the selected folder.
/// Size is used only to reduce the candidates passed to SHA-256; hashes decide matches.
pub fn find_duplicates(root_path: &str) -> Result<DuplicateScanResult, EngineError> {
    let (root, scan) = scan_folder_with_root(root_path)?;
    let mut by_size: BTreeMap<u64, Vec<RelativePath>> = BTreeMap::new();
    for file in &scan.files {
        by_size
            .entry(file.size_bytes)
            .or_default()
            .push(file.source.clone());
    }

    let candidates: Vec<_> = by_size
        .values()
        .filter(|paths| paths.len() > 1)
        .flatten()
        .cloned()
        .collect();
    let mut by_hash: BTreeMap<(u64, [u8; 32]), Vec<RelativePath>> = BTreeMap::new();
    let mut uncertain_files = Vec::new();

    for path in &candidates {
        match hash_stable_file(&root, path) {
            Ok((size, digest)) => by_hash.entry((size, digest)).or_default().push(path.clone()),
            Err(HashFileError::Changed) => uncertain_files.push(DuplicateFileStatus {
                path: path.clone(),
                kind: DuplicateFileStatusKind::ChangedDuringHash,
                message: "The file changed while it was being checked, so Shellsort cannot confirm whether it is a duplicate. Run the check again.".to_owned(),
            }),
            Err(HashFileError::Io(error)) => uncertain_files.push(DuplicateFileStatus {
                path: path.clone(),
                kind: DuplicateFileStatusKind::Unreadable,
                message: format!("Shellsort could not read this file to compare it: {error}"),
            }),
        }
    }

    let groups = by_hash
        .into_iter()
        .filter_map(|((size_bytes, _), mut files)| {
            if files.len() < 2 {
                return None;
            }
            files.sort_by(|left, right| left.as_str().cmp(right.as_str()));
            Some(DuplicateGroup { size_bytes, files })
        })
        .collect();
    uncertain_files.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));

    Ok(DuplicateScanResult {
        root_path: scan.root_path,
        files_scanned: scan.files.len(),
        size_candidates: candidates.len(),
        groups,
        uncertain_files,
        warnings: scan.warnings,
        issues: scan.issues,
    })
}

enum HashFileError {
    Changed,
    Io(io::Error),
}

#[derive(Debug, PartialEq, Eq)]
struct StableMetadata {
    size: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl StableMetadata {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            size: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
        }
    }
}

#[cfg(unix)]
fn hash_stable_file(
    root: &Path,
    relative: &RelativePath,
) -> Result<(u64, [u8; 32]), HashFileError> {
    let (parent, name, mut file) = open_candidate(root, relative)?;
    let opened = file.metadata().map_err(HashFileError::Io)?;
    if !opened.is_file() {
        return Err(HashFileError::Changed);
    }
    let before = StableMetadata::from_metadata(&opened);
    let path_before = open_candidate_again(&parent, &name)?
        .metadata()
        .map_err(HashFileError::Io)?;
    if StableMetadata::from_metadata(&path_before) != before {
        return Err(HashFileError::Changed);
    }

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(HashFileError::Io)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    let after_handle = file.metadata().map_err(HashFileError::Io)?;
    let after_path = open_candidate_again(&parent, &name)?
        .metadata()
        .map_err(HashFileError::Io)?;
    if !after_path.is_file()
        || StableMetadata::from_metadata(&after_handle) != before
        || StableMetadata::from_metadata(&after_path) != before
    {
        return Err(HashFileError::Changed);
    }

    Ok((before.size, hasher.finalize().into()))
}

#[cfg(not(unix))]
fn hash_stable_file(
    root: &Path,
    relative: &RelativePath,
) -> Result<(u64, [u8; 32]), HashFileError> {
    let path = root.join(relative.as_path());
    let parent = path.parent().ok_or(HashFileError::Changed)?;
    let canonical_parent = parent.canonicalize().map_err(HashFileError::Io)?;
    if !canonical_parent.starts_with(root) {
        return Err(HashFileError::Changed);
    }
    let before_path = fs::symlink_metadata(&path).map_err(HashFileError::Io)?;
    if before_path.file_type().is_symlink() || !before_path.is_file() {
        return Err(HashFileError::Changed);
    }
    let before = StableMetadata::from_metadata(&before_path);
    let mut file = fs::OpenOptions::new()
        .read(true)
        .open(&path)
        .map_err(HashFileError::Io)?;
    if StableMetadata::from_metadata(&file.metadata().map_err(HashFileError::Io)?) != before {
        return Err(HashFileError::Changed);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(HashFileError::Io)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let after_path = fs::symlink_metadata(&path).map_err(HashFileError::Io)?;
    if after_path.file_type().is_symlink()
        || StableMetadata::from_metadata(&file.metadata().map_err(HashFileError::Io)?) != before
        || StableMetadata::from_metadata(&after_path) != before
    {
        return Err(HashFileError::Changed);
    }
    Ok((before.size, hasher.finalize().into()))
}

#[cfg(unix)]
fn open_candidate(
    root: &Path,
    relative: &RelativePath,
) -> Result<(std::fs::File, std::ffi::CString, std::fs::File), HashFileError> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
        path::Component,
    };

    let relative_path = relative.as_path();
    let components: Vec<_> = relative_path.components().collect();
    if components.is_empty()
        || components
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(HashFileError::Changed);
    }
    let root_name =
        CString::new(root.as_os_str().as_bytes()).map_err(|_| HashFileError::Changed)?;
    let root_fd = unsafe {
        libc::open(
            root_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if root_fd < 0 {
        return Err(map_open_error(io::Error::last_os_error()));
    }
    // SAFETY: open returned a fresh owned descriptor.
    let mut directory = unsafe { std::fs::File::from_raw_fd(root_fd) };
    for component in &components[..components.len() - 1] {
        let Component::Normal(part) = component else {
            return Err(HashFileError::Changed);
        };
        let name = CString::new(part.as_bytes()).map_err(|_| HashFileError::Changed)?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(map_open_error(io::Error::last_os_error()));
        }
        // SAFETY: openat returned a fresh owned descriptor.
        directory = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    let Component::Normal(last) = components[components.len() - 1] else {
        return Err(HashFileError::Changed);
    };
    let name = CString::new(last.as_bytes()).map_err(|_| HashFileError::Changed)?;
    let file = open_candidate_again(&directory, &name)?;
    Ok((directory, name, file))
}

#[cfg(unix)]
fn open_candidate_again(
    parent: &std::fs::File,
    name: &std::ffi::CString,
) -> Result<std::fs::File, HashFileError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(map_open_error(io::Error::last_os_error()));
    }
    // SAFETY: openat returned a fresh owned descriptor.
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn map_open_error(error: io::Error) -> HashFileError {
    if error.raw_os_error() == Some(libc::ELOOP) {
        HashFileError::Changed
    } else {
        HashFileError::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::find_duplicates;
    use std::{fs, path::Path, time::UNIX_EPOCH};
    use tempfile::tempdir;

    fn snapshot(root: &Path) -> Vec<(String, Vec<u8>, u64)> {
        fn visit(root: &Path, current: &Path, output: &mut Vec<(String, Vec<u8>, u64)>) {
            for entry in fs::read_dir(current).expect("read temp directory") {
                let entry = entry.expect("directory entry");
                let path = entry.path();
                if entry.file_type().expect("entry type").is_dir() {
                    visit(root, &path, output);
                } else if entry.file_type().expect("entry type").is_file() {
                    let relative = path.strip_prefix(root).expect("under root");
                    let modified = fs::symlink_metadata(&path)
                        .expect("metadata")
                        .modified()
                        .expect("modification time")
                        .duration_since(UNIX_EPOCH)
                        .expect("after epoch")
                        .as_nanos() as u64;
                    output.push((
                        relative.to_string_lossy().into_owned(),
                        fs::read(path).expect("contents"),
                        modified,
                    ));
                }
            }
        }
        let mut output = Vec::new();
        visit(root, root, &mut output);
        output.sort_by(|left, right| left.0.cmp(&right.0));
        output
    }

    #[test]
    fn detects_exact_matches_including_empty_files_and_ignores_same_size_differences() {
        let root = tempdir().expect("temp root");
        fs::write(root.path().join("first.bin"), b"same bytes").expect("first");
        fs::write(root.path().join("nested copy.bin"), b"same bytes").expect("duplicate");
        fs::write(root.path().join("different.bin"), b"diff bytes")
            .expect("different same-size file");
        fs::write(root.path().join("empty one"), b"").expect("empty first");
        fs::write(root.path().join("empty two"), b"").expect("empty duplicate");
        fs::create_dir(root.path().join("nested")).expect("nested directory");
        fs::write(root.path().join("nested/third.bin"), b"same bytes").expect("nested duplicate");

        let before = snapshot(root.path());
        let result =
            find_duplicates(root.path().to_str().expect("root path")).expect("duplicate scan");
        let after = snapshot(root.path());

        assert_eq!(before, after, "hashing must not mutate selected files");
        assert_eq!(result.files_scanned, 6);
        assert_eq!(result.size_candidates, 6);
        assert_eq!(result.groups.len(), 2);
        assert!(
            result
                .groups
                .iter()
                .any(|group| group.files.len() == 3 && group.size_bytes == 10)
        );
        assert!(
            result
                .groups
                .iter()
                .any(|group| group.files.len() == 2 && group.size_bytes == 0)
        );
        assert!(result.uncertain_files.is_empty());
    }

    #[test]
    fn excludes_size_singletons_from_content_reads_and_reports_empty_folder() {
        let empty = tempdir().expect("empty root");
        let result = find_duplicates(empty.path().to_str().expect("root path")).expect("scan");
        assert_eq!(result.files_scanned, 0);
        assert_eq!(result.size_candidates, 0);
        assert!(result.groups.is_empty());

        fs::write(empty.path().join("one"), b"singleton").expect("file");
        let result = find_duplicates(empty.path().to_str().expect("root path")).expect("scan");
        assert_eq!(result.files_scanned, 1);
        assert_eq!(result.size_candidates, 0);
    }

    #[test]
    fn compares_large_files_with_streaming_hashes() {
        let root = tempdir().expect("temp root");
        let block: Vec<u8> = (0..=255).cycle().take(64 * 1024).collect();
        let mut bytes = Vec::with_capacity(4 * 1024 * 1024);
        for _ in 0..64 {
            bytes.extend_from_slice(&block);
        }
        fs::write(root.path().join("large-a.bin"), &bytes).expect("first large file");
        fs::write(root.path().join("large-b.bin"), &bytes).expect("second large file");

        let result = find_duplicates(root.path().to_str().expect("root path")).expect("scan");
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.groups[0].size_bytes, 4 * 1024 * 1024);
        assert_eq!(result.groups[0].files.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn skips_symbolic_links_using_the_existing_scan_policy() {
        use std::os::unix::fs::symlink;
        let root = tempdir().expect("temp root");
        fs::write(root.path().join("original"), b"same").expect("original");
        symlink(root.path().join("original"), root.path().join("link")).expect("symlink");
        let result = find_duplicates(root.path().to_str().expect("root path")).expect("scan");
        assert_eq!(result.files_scanned, 1);
        assert!(result.groups.is_empty());
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.kind == super::super::ScanWarningKind::SymbolicLinkSkipped)
        );
    }

    #[cfg(unix)]
    #[test]
    fn anchored_hash_open_rejects_a_symlinked_parent_even_if_given_a_path() {
        use super::hash_stable_file;
        use crate::organizer::RelativePath;
        use std::os::unix::fs::symlink;

        let root = tempdir().expect("temp root");
        let outside = tempdir().expect("outside temp folder");
        fs::write(outside.path().join("secret.bin"), b"outside bytes").expect("outside file");
        symlink(outside.path(), root.path().join("linked folder")).expect("directory symlink");

        let relative = RelativePath::from_path(Path::new("linked folder/secret.bin"))
            .expect("safe relative path for lower-level race test");
        assert!(hash_stable_file(root.path(), &relative).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symbolic_link_as_the_selected_root() {
        use std::os::unix::fs::symlink;
        let parent = tempdir().expect("temp parent");
        let root = parent.path().join("root");
        fs::create_dir(&root).expect("root");
        let link = parent.path().join("root-link");
        symlink(&root, &link).expect("root symlink");
        assert_eq!(
            find_duplicates(link.to_str().expect("link path"))
                .expect_err("reject root link")
                .kind,
            super::super::EngineErrorKind::RootIsSymbolicLink
        );
    }
}
