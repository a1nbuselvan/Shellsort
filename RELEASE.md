# Shellsort CLI development and release guide

Shellsort is a native Rust command-line file organizer for macOS. The default command scans metadata and prints a proposed plan without changing files. Organization requires a fresh plan and an explicit confirmation. The shared Rust engine validates roots and paths, rejects symlinks and unsafe entries, avoids overwrites, journals operations, and supports recovery and undo. Cross-filesystem moves are rejected. `.DS_Store`, AppleDouble sidecars (`._*`), and known macOS metadata markers/directories are excluded by default. Duplicate comparison reads file bytes locally; it does not upload them. No web server, desktop window, background watcher, or network service is required.

The CI workflow is configured to compile and test on macOS arm64, macOS Intel, Ubuntu Linux, and Windows; those jobs have not yet run for this uncommitted workflow. This does not by itself declare all-platform product support: safe organization moves, journal recovery, resume, and undo are enabled only on macOS. Linux and Windows jobs are intended to verify compilation, tests, preview/hash behavior, and safe refusal of unsupported mutations. Do not claim full Linux or Windows support until the platform-specific filesystem behavior and CI results have been reviewed.

## Development

Requirements: Rust stable via [rustup](https://rustup.rs/) and the Rust target for the build architecture. On macOS, install Xcode Command Line Tools if required by the toolchain.

From the repository root:

```sh
cargo run -- --help
cargo run -- "/path/to/folder with spaces"
```

The CLI supports read-only preview, exact duplicate detection, explicit organization, journal history and recovery, resume, and undo:

```sh
shellsort ~/Downloads
shellsort ~/Downloads --json
shellsort duplicates ~/Downloads
shellsort organize ~/Downloads
shellsort history
shellsort recover
shellsort resume ~/Downloads <operation-id>
shellsort undo ~/Downloads <operation-id>
```

Mutating operations show a fresh plan and require typing the action name at an interactive prompt. `--yes` is an explicit non-interactive confirmation. Never use real personal folders for development tests.

The compatibility script delegates to the installed executable found on `PATH`, and works from any current directory:

```sh
organizer.sh ~/Downloads
```

For local testing, create a disposable fixture and install to a user-owned prefix:

```sh
cargo install --locked --path . --root "$HOME/.local"
install -m 755 organizer.sh "$HOME/.local/bin/organizer.sh"
export PATH="$HOME/.local/bin:$PATH"
shellsort --version
```

The operation journal remains under `Library/Application Support/app.shellsort.desktop/` on macOS to retain compatibility with existing journal data. It is local; operation history contains folder paths and filenames. Do not edit the journal while Shellsort is running. Malformed or unsupported journals are reported and left untouched.

## Verification

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
git diff --check
```

CLI integration tests use temporary directories and disposable files. Do not point tests or organization commands at the real Downloads folder or personal documents.

## Versioned macOS CLI releases

The intended release targets are Apple Silicon (`aarch64-apple-darwin`) and Intel (`x86_64-apple-darwin`). Build and test each architecture on a native or otherwise verified matching macOS host. A successful Apple Silicon build does not verify Intel compatibility. For a tagged version `vX.Y.Z`, build each binary and stage a versioned archive containing `shellsort`, executable `organizer.sh`, and `LICENSE`:

```sh
cargo build --locked --release --target aarch64-apple-darwin --bin shellsort
cargo build --locked --release --target x86_64-apple-darwin --bin shellsort
```

Use archive names such as `shellsort-vX.Y.Z-aarch64-apple-darwin.tar.gz` and `shellsort-vX.Y.Z-x86_64-apple-darwin.tar.gz`. Generate SHA-256 checksums with `shasum -a 256`, verify them after upload, and publish the checksum values with the exact assets. Do not reuse a release tag or replace an asset after publishing its checksum.

For local archive smoke testing, extract into a temporary directory, put that directory first on `PATH`, verify `shellsort --version`, and run preview, explicit-confirmation, history, recovery, and undo checks only against disposable fixtures.

## Homebrew custom tap preparation

`packaging/homebrew-tap/Formula/shellsort.rb` is a placeholder template. The intended layout in a separately created custom-tap repository is:

```text
homebrew-shellsort/
└── Formula/
    └── shellsort.rb
```

After a real project homepage/repository and versioned release archives exist, copy the formula layout into the tap and replace all URL and SHA-256 placeholders with verified values. Then run `brew style` and `brew audit --strict --online` against the completed formula and test installation on both architectures. This repository does not currently define the public project URL or contain published artifacts/checksums; `brew tap` and `brew install shellsort` are not available and must not be advertised as working.

## Release blockers

Before public distribution, publish immutable versioned archives for both supported architectures, verify their checksums and local installation behavior, complete manual tests on disposable folders, choose and document the real project/tap repository URLs, and replace and validate the Homebrew formula placeholders. Do not claim release readiness or public Homebrew availability before those steps are complete.
