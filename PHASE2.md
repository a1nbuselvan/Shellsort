# Shellsort Phase 2: Read-only scan and plan

Phase 2 adds a Rust metadata scanner and organization-plan generator. The app can scan a selected folder and show proposed destinations. It cannot move, rename, delete, overwrite, create folders, execute files, or undo changes.

## Scan policy

- The selected root is checked with non-following metadata and canonicalized. A root that is itself a symbolic link is rejected. The selected root is scanned even if its own name is hidden or is `node_modules`/`target`; exclusions apply to child directories encountered during the scan.
- Files are scanned recursively. Nested hidden directories (names beginning with `.`) are skipped. `.git` is therefore skipped when it is inside the selected root.
- `node_modules` and `target` child directories are skipped at any depth. These and hidden-directory skips are reported as warnings.
- Hidden regular files are included. A single-leading-dot filename such as `.gitignore`, a name with no dot, or a trailing-dot name has no extension and is classified as `Other`. `.DS_Store` is included as an `Other` file.
- Symbolic links and non-regular filesystem entries are skipped. Symlinked files and directories are never intentionally followed.
- Filenames that are not valid Unicode are skipped with a warning because the JSON UI protocol uses UTF-8 paths. Unicode names, spaces, and nested relative paths are supported.
- The scanner reads directory entries and filesystem metadata only. It never opens file contents. Inaccessible subfolders and metadata failures become structured issues; the returned plan is marked incomplete.

## Categories and plan behavior

Extensions are matched case-insensitively. Destinations use a category folder at the selected root (`Documents/`, `Images/`, `Videos/`, `Audio/`, `Archives/`, `Code/`, `Spreadsheets/`, `Presentations/`, or `Other/`) plus the source filename. The scanner does not create these folders.

Each plan entry contains a root-relative source and destination, category, extension, size, conflict state, and explanation. The plan marks files whose source already equals their category destination as `already_correct`. Proposed paths are grouped after Unicode canonical decomposition and lowercase conversion so case-only or canonically equivalent names are conservatively flagged on typical macOS volumes. It reports existing destination entries, multiple proposed files with the same destination, both kinds of collision together, or destinations that could not be checked. No collision is treated as permission to overwrite.

The root is canonicalized and relative paths are validated to contain only normal components. Entries are obtained without following links, and destination checks use `symlink_metadata` component by component, treating symlinks as occupied. Directories are rechecked and canonicalized before scanning. A path-based read-only scan cannot provide an atomic snapshot if another process changes the filesystem concurrently; re-run the plan after any such change. Proposed-path normalization is intentionally conservative and can report a collision on a case-sensitive volume even when both names could coexist. Phase 2 has no execution operation.

## Development and verification

From the repository root:

```sh
export PATH="$(dirname "$(rustup which cargo)"):$PATH"
npm install
npm run tauri dev
```

Verification commands:

```sh
npm run typecheck
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

The Tauri frontend receives only the `scan_folder` and `generate_organization_plan` commands for the engine. The existing native dialog permission remains the only plugin permission; no filesystem plugin permission or shell permission is granted.
