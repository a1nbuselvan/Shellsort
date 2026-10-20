# Shellsort

I built Shellsort because my Downloads folder kept filling up with files I could never find again. It started as a way to make that everyday mess easier to deal with.

Shellsort is a macOS command-line file organizer. It shows a proposed plan before anything moves, flags conflicts, and asks for confirmation before organizing. The first preview is read-only.

[Project website](https://a1nbuselvan.github.io/Shellsort/) · [Source code](https://github.com/a1nbuselvan/Shellsort)

## Install Shellsort on a Mac

No Homebrew required. This beta is not signed or notarized by Apple. Download only from the official release page, verify the checksum, and review the plan before organizing files.

### 1. Download the right build

Open the [Shellsort v0.1.0-beta.1 release page](https://github.com/a1nbuselvan/Shellsort/releases/tag/v0.1.0-beta.1). Download the archive and its matching `.sha256` file. Choose the build for your Mac:

- **Apple Silicon** (M1, M2, M3, M4 and later): [archive](https://github.com/a1nbuselvan/Shellsort/releases/download/v0.1.0-beta.1/shellsort-v0.1.0-beta.1-aarch64-apple-darwin.tar.gz) · [SHA-256 file](https://github.com/a1nbuselvan/Shellsort/releases/download/v0.1.0-beta.1/shellsort-v0.1.0-beta.1-aarch64-apple-darwin.tar.gz.sha256)
- **Intel Mac**: [archive](https://github.com/a1nbuselvan/Shellsort/releases/download/v0.1.0-beta.1/shellsort-v0.1.0-beta.1-x86_64-apple-darwin.tar.gz) · [SHA-256 file](https://github.com/a1nbuselvan/Shellsort/releases/download/v0.1.0-beta.1/shellsort-v0.1.0-beta.1-x86_64-apple-darwin.tar.gz.sha256)

If you are unsure which Mac you have, open Terminal and run:

```sh
uname -m
```

`arm64` means Apple Silicon; `x86_64` means Intel.

### 2. Verify the download

Keep the archive and checksum file together in Downloads. In Terminal, run the command for the archive you downloaded:

**Apple Silicon**

```sh
cd "$HOME/Downloads"
shasum -a 256 -c shellsort-v0.1.0-beta.1-aarch64-apple-darwin.tar.gz.sha256
```

**Intel Mac**

```sh
cd "$HOME/Downloads"
shasum -a 256 -c shellsort-v0.1.0-beta.1-x86_64-apple-darwin.tar.gz.sha256
```

Continue only if the result says `OK`. If verification fails, do not install that archive; download it again from the official release page and check once more.

### 3. Extract the archive

These commands make a named folder in Downloads and extract the files there. Run the command for your Mac:

**Apple Silicon**

```sh
EXTRACTED_DIR="$HOME/Downloads/shellsort-v0.1.0-beta.1-aarch64-apple-darwin"
if [ -e "$EXTRACTED_DIR" ] || [ -L "$EXTRACTED_DIR" ]; then
  echo "$EXTRACTED_DIR already exists. Keep it or move it aside before extracting." >&2
else
  mkdir -p "$EXTRACTED_DIR" && \
    tar -xzf "$HOME/Downloads/shellsort-v0.1.0-beta.1-aarch64-apple-darwin.tar.gz" -C "$EXTRACTED_DIR"
fi
```

**Intel Mac**

```sh
EXTRACTED_DIR="$HOME/Downloads/shellsort-v0.1.0-beta.1-x86_64-apple-darwin"
if [ -e "$EXTRACTED_DIR" ] || [ -L "$EXTRACTED_DIR" ]; then
  echo "$EXTRACTED_DIR already exists. Keep it or move it aside before extracting." >&2
else
  mkdir -p "$EXTRACTED_DIR" && \
    tar -xzf "$HOME/Downloads/shellsort-v0.1.0-beta.1-x86_64-apple-darwin.tar.gz" -C "$EXTRACTED_DIR"
fi
```

The archive contains `shellsort`, the `organizer.sh` compatibility script, and the project license.

### 4. Install it

Set `EXTRACTED_DIR` to the folder you just created. Use the Apple Silicon path:

```sh
EXTRACTED_DIR="$HOME/Downloads/shellsort-v0.1.0-beta.1-aarch64-apple-darwin"
```

Or the Intel path:

```sh
EXTRACTED_DIR="$HOME/Downloads/shellsort-v0.1.0-beta.1-x86_64-apple-darwin"
```

Then run:

```sh
mkdir -p "$HOME/.local/bin"
if [ -e "$HOME/.local/bin/shellsort" ] || [ -L "$HOME/.local/bin/shellsort" ] || \
   [ -e "$HOME/.local/bin/organizer.sh" ] || [ -L "$HOME/.local/bin/organizer.sh" ]; then
  echo "A Shellsort executable already exists in $HOME/.local/bin. Keep it or move it aside before installing." >&2
else
  install -m 755 "$EXTRACTED_DIR/shellsort" "$HOME/.local/bin/shellsort"
  install -m 755 "$EXTRACTED_DIR/organizer.sh" "$HOME/.local/bin/organizer.sh"
fi
```

The check prevents these commands from replacing an existing `shellsort` or `organizer.sh` executable.

### 5. Add it to PATH

If `shellsort` is not found after installation, add `~/.local/bin` to your zsh PATH. This command adds the line only if it is not already in `~/.zshrc`:

```sh
grep -qxF 'export PATH="$HOME/.local/bin:$PATH"' "$HOME/.zshrc" 2>/dev/null || \
  printf '%s\n' 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.zshrc"
source "$HOME/.zshrc"
```

You can also open a new Terminal window for the updated PATH to take effect.

### 6. Check the install

```sh
shellsort --version
```

It should print:

```text
shellsort 0.1.0-beta.1
```

### 7. Use Shellsort

Start with a read-only preview. This command does not move files:

```sh
shellsort "$HOME/Downloads"
```

Read the plan and check the proposed destinations and conflicts. To organize, run the separate command and follow its confirmation prompt:

```sh
shellsort organize "$HOME/Downloads"
```

Shellsort makes a fresh plan before it asks. It will not silently overwrite an existing file. Back up important files before organizing, especially while using beta software.

Other useful commands:

```sh
shellsort duplicates "$HOME/Downloads" # Find exact duplicate files
shellsort history                      # Review operation history
shellsort recover                      # Check for interrupted operations
organizer.sh "$HOME/Downloads"         # Compatibility wrapper for organize
```

`organizer.sh` also organizes files, so it displays the plan and requires confirmation. It is not a read-only preview command.

## Safety and current limits

- Shellsort scans file metadata for previews; it does not read file contents during a scan. Exact duplicate detection reads candidate file bytes locally to compare them.
- Previewing never moves files. Organizing, recovery, resume, and undo are separate actions with safety checks and a local operation journal.
- Existing files are not overwritten. Unsafe paths, symbolic links, and unsupported file types are handled conservatively.
- File-changing operations are supported only on macOS. Linux and Windows builds refuse those operations.
- `.DS_Store`, AppleDouble sidecars (`._*`), and known macOS metadata paths are excluded from organization.
- The beta is not signed or notarized by Apple. macOS may show a security warning. Do not bypass security protections unless you have independently verified the software and understand the risk.
- The operation journal stays on your Mac and records paths and filenames. Shellsort does not upload files or use telemetry.
- Homebrew installation is not available yet. There is no published tap formula with verified release checksums.

## Uninstall

Remove the installed commands with confirmation:

```sh
rm -i "$HOME/.local/bin/shellsort" "$HOME/.local/bin/organizer.sh"
```

This leaves the local operation journal and history in place. Uninstalling the commands does not remove your organized files.

## Development

Requirements: stable Rust from [rustup](https://rustup.rs/) and Xcode Command Line Tools on macOS if your toolchain requires them.

From a checkout of this repository:

```sh
cargo run -- --help
cargo run -- "$HOME/Downloads" # Read-only preview; use a disposable folder for testing
```

Run the checks before contributing:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
git diff --check
```

Tests use temporary folders and disposable files. Do not use personal folders for development tests.

## Project layout

```text
src/                         Rust organizer and CLI
tests/                       CLI integration tests
.github/workflows/           CI, release builds, and Pages deployment
website/                     Static project website
packaging/homebrew-tap/       Formula draft; not installable yet
organizer.sh                 Compatibility wrapper
RELEASE.md                   Release and distribution notes
```

See [RELEASE.md](RELEASE.md) for release build details and Homebrew plans. Shellsort is open source under the terms in [LICENSE](LICENSE).
