# Shellsort

Shellsort is a native Rust command-line file organizer for macOS. A folder scan prints a read-only preview; files move only after a fresh plan and explicit confirmation. No desktop app, web server, or background service is required.

The original `organizer.sh` name remains available as a compatibility wrapper. It calls `shellsort organize` from `PATH`, displaying a fresh plan and requiring confirmation before moving files.

## Install the macOS beta

No beta release archives or Homebrew tap have been published yet. Once the first beta is available, download the matching archive and `.sha256` file from the GitHub Releases page. Choose `aarch64-apple-darwin` for Apple Silicon (`uname -m` prints `arm64`) or `x86_64-apple-darwin` for Intel (`uname -m` prints `x86_64`). The archives contain `shellsort`, `organizer.sh`, and `LICENSE`.

The current build metadata indicates macOS 11.0 or later for Apple Silicon and macOS 10.12 or later for Intel. These minimum versions will be rechecked on the final release artifacts.

Verify the downloaded archive before extracting it. Set `ARCHIVE` to its exact downloaded filename:

```sh
ARCHIVE="shellsort-vX.Y.Z-beta.N-aarch64-apple-darwin.tar.gz"
shasum -a 256 -c "$ARCHIVE.sha256"
```

After verification, extract and install to a user-owned directory. These commands stop if either executable already exists, so they will not replace an existing installation:

```sh
mkdir -p "$HOME/.local/bin"
test ! -e "$HOME/.local/bin/shellsort" && test ! -e "$HOME/.local/bin/organizer.sh" || {
  echo "An executable already exists in $HOME/.local/bin; back it up or choose another install directory." >&2
  exit 1
}
tar -xzf "$ARCHIVE"
install -m 755 shellsort "$HOME/.local/bin/shellsort"
install -m 755 organizer.sh "$HOME/.local/bin/organizer.sh"
```

If `~/.local/bin` is not on `PATH`, add this line to `~/.zshrc`, then open a new terminal:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Verify installation with `shellsort --version`. For local development builds instead, use the instructions below.

## Install for local development

Requirements: Rust stable via [rustup](https://rustup.rs/) and Xcode Command Line Tools on macOS when required by the Rust toolchain.

From the repository root:

```sh
cargo install --locked --path . --root "$HOME/.local"
install -m 755 organizer.sh "$HOME/.local/bin/organizer.sh"
export PATH="$HOME/.local/bin:$PATH"
shellsort --version
```

This installs a local development build. Shellsort is not currently published on Homebrew. See [RELEASE.md](RELEASE.md) for the beta release process and custom-tap requirements.

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

`organizer.sh` is a compatibility wrapper for the explicit organize command. It displays a fresh plan and asks for confirmation; it does not perform a read-only preview. It can be run from any working directory after installing both commands:

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

## Uninstall

To remove the two executables installed in `~/.local/bin`:

```sh
rm "$HOME/.local/bin/shellsort" "$HOME/.local/bin/organizer.sh"
```

Uninstall leaves the local operation journal and history at `~/Library/Application Support/app.shellsort.desktop/`. Back it up before removing it manually if you want to preserve history or may need recovery information.

## Safety and behavior

- Scanning and planning inspect file metadata and do not read file contents.
- Exact duplicate detection is an explicit command and reads file bytes locally for matching candidates; no data is uploaded.
- Symbolic links and special filesystem entries are rejected or skipped. Unsafe roots and paths are refused.
- Existing destinations are never overwritten. Cross-filesystem moves are rejected.
- Operations use a local durable journal and support recovery and safe undo.
- File moves, recovery, resume, and undo are enabled only on macOS. Other targets refuse those operations without modifying the selected folder.
- `.DS_Store`, AppleDouble sidecars (`._*`), and known macOS metadata paths are excluded from organization.
- A preview is informational and does not authorize a later move. Organize only after reviewing the fresh plan and the typed confirmation prompt.
- Shellsort has no telemetry or cloud upload. Its operation journal is local and records folder paths and filenames; exact duplicate detection reads candidate file bytes locally.

## Development checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
git diff --check
```

Tests use temporary directories and disposable files. For architecture targets, release artifacts, checksums, and Homebrew tap instructions, see [RELEASE.md](RELEASE.md).
