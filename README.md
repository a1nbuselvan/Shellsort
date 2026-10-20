# Shellsort

Shellsort is a native Rust command-line file organizer for macOS. A folder scan prints a read-only preview; files move only after a fresh plan and explicit confirmation. No desktop app, web server, or background service is required.

The original `organizer.sh` name remains available as a compatibility wrapper. It calls the installed `shellsort` executable from `PATH` and uses the same preview, confirmation, and safety checks.

## Install for local development

Requirements: Rust stable via [rustup](https://rustup.rs/) and Xcode Command Line Tools on macOS when required by the Rust toolchain.

From the repository root:

```sh
cargo install --locked --path . --root "$HOME/.local"
install -m 755 organizer.sh "$HOME/.local/bin/organizer.sh"
export PATH="$HOME/.local/bin:$PATH"
shellsort --version
```

This installs a local development build. Shellsort is not currently published on Homebrew. See [RELEASE.md](RELEASE.md) for release and custom-tap preparation details.

## Use

Preview is the default and does not change files:

```sh
shellsort ~/Downloads
shellsort ~/Downloads --json
shellsort duplicates ~/Downloads
```

Organizing shows a fresh plan and asks for the explicit confirmation phrase:

```sh
shellsort organize ~/Downloads
```

The compatibility script can be run from any working directory after installing both commands:

```sh
organizer.sh ~/Downloads
```

The CLI also provides local operation history, recovery, resume, and undo:

```sh
shellsort history
shellsort recover
shellsort resume ~/Downloads <operation-id>
shellsort undo ~/Downloads <operation-id>
```

Mutating commands require interactive confirmation. Use `--yes` only when explicitly authorizing a non-interactive operation. Test exclusively with disposable folders; do not use personal files for development checks.

## Safety and behavior

- Scanning and planning inspect file metadata and do not read file contents.
- Exact duplicate detection is an explicit command and reads file bytes locally for matching candidates; no data is uploaded.
- Symbolic links and special filesystem entries are rejected or skipped. Unsafe roots and paths are refused.
- Existing destinations are never overwritten. Cross-filesystem moves are rejected.
- Operations use a local durable journal and support recovery and safe undo.
- File moves, recovery, resume, and undo are enabled only on macOS. Other targets refuse those operations without modifying the selected folder.
- `.DS_Store`, AppleDouble sidecars (`._*`), and known macOS metadata paths are excluded from organization.

## Development checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
git diff --check
```

Tests use temporary directories and disposable files. For architecture targets, release artifacts, checksums, and Homebrew tap instructions, see [RELEASE.md](RELEASE.md).
