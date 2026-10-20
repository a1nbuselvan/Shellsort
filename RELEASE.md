# Shellsort CLI development and release guide

Shellsort is a native Rust command-line file organizer for macOS. The default command scans metadata and prints a proposed plan without changing files. Organization requires a fresh plan and an explicit confirmation. The shared Rust engine validates roots and paths, rejects symlinks and unsafe entries, avoids overwrites, journals operations, and supports recovery and undo. Cross-filesystem moves are rejected. `.DS_Store`, AppleDouble sidecars (`._*`), and known macOS metadata markers/directories are excluded by default. Duplicate comparison reads file bytes locally; it does not upload them. No web server, desktop window, background watcher, or network service is required.

The CI workflow passed on macOS arm64, macOS Intel, Ubuntu Linux, and Windows for commit `fa7bfef3898c7d5a43ebf0ad2a0475ee9863edc9` ([run 39400104119](https://github.com/a1nbuselvan/Shellsort/actions/runs/39400104119)). This does not declare all-platform mutation support: file moves, journal recovery, resume, and undo remain enabled only on macOS. Linux and Windows verify compilation, tests, preview/hash behavior, and safe refusal of unsupported mutations.

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

## First macOS beta release

The supported release targets are Apple Silicon (`aarch64-apple-darwin`) and Intel (`x86_64-apple-darwin`). Build and smoke-test each binary on a matching native runner. The checked-in [macOS release build workflow](.github/workflows/macos-release-build.yml) uses the same macOS runner labels that passed CI, pins Rust `1.99.1`, builds with `Cargo.lock`, tests the extracted release archive on disposable files, and uploads archives plus checksum files as workflow artifacts. It does not create or publish a GitHub release.

The current local Mach-O metadata reports minimum macOS 11.0 for arm64 and 10.12 for Intel (`xcrun vtool -show-build`). Recheck the minimum OS on each final release artifact; the arm64 and Intel targets have different baselines.

The workflow runs only for a pushed `v*` tag. The tag version must exactly match the package version in `Cargo.toml`. The selected candidate is `0.1.0-beta.1`, now recorded in `Cargo.toml`, the root `shellsort` package entry in `Cargo.lock`, and the formula draft. No beta tag has been created.

For each target, the workflow creates an archive containing `shellsort`, executable `organizer.sh`, and `LICENSE`. It computes SHA-256 after the archive is finalized, then uploads both files as an Actions artifact. The checksum applies to that exact archive. Before attaching any assets to a GitHub draft prerelease, download each artifact and verify its checksum again. Do not replace an archive after publishing its checksum.

The build pins the Rust toolchain and dependency lockfile, but runner images and Apple SDKs can change. Treat the builds as controlled/repeatable, not yet proven bit-for-bit reproducible. Verify reproducibility by building twice on the same runner/toolchain and comparing binary hashes; archive hashes will only match if archive metadata and compression output are also normalized. The local Rust 1.99.1 release build warned that its bundled `rust-objcopy` could not load `libLLVM.dylib` and left DWARF data in the executable. The release workflow therefore uses the native Xcode `strip -S` on the staged copy; this succeeded locally for both the arm64 executable and cross-built Intel executable and did not affect `shellsort --version`. The final archive, not an intermediate binary, is checksummed.

### Signing and notarization

The prepared workflow currently builds unsigned binaries and does not submit anything to Apple. Before recommending direct downloads broadly, decide whether to sign with Developer ID and notarize. Apple recommends notarizing distributed Mac software; its documented custom submission formats include ZIP, PKG, and DMG, while the current Homebrew-oriented asset is `.tar.gz`. The packaging format and ticket verification need to be settled before adding notarization. See [Apple's notarization overview](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) and [supported distribution containers](https://developer.apple.com/developer-id/).

If signing/notarization is approved, the GitHub environment will need an Apple Developer account with a Developer ID Application certificate, the Team ID, and protected Actions secrets. A recommended API-key setup would use `APPLE_DEVELOPER_ID_P12_BASE64`, `APPLE_DEVELOPER_ID_P12_PASSWORD`, `APPLE_TEAM_ID`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID`, and `APPLE_API_PRIVATE_KEY_P8`. No Apple credentials or GitHub secrets were provided or inspected. The current artifact-only workflow needs no release-write permission or repository secret; a maintainer creates the draft prerelease manually in GitHub after reviewing the artifacts.

Manual release steps after review and approval:

1. Review the final diff and run the local checks below. Commit and push the version/workflow/docs changes through the normal approved process; verify CI on that commit.
2. Confirm the remote `main` contains `0.1.0-beta.1`, then create and push the matching tag:

   ```sh
   git tag -a v0.1.0-beta.1 -m "Shellsort v0.1.0-beta.1"
   git push origin v0.1.0-beta.1
   ```

3. Open the Actions run for that tag. Wait for both native jobs to pass; inspect the architecture checks, version output, read-only preview, non-interactive refusal, confirmed organize/undo fixture, and artifact upload.
4. Download both workflow artifacts. Inspect each archive listing and checksum file, then independently verify from the directory containing the downloaded files:

   ```sh
   shasum -a 256 -c shellsort-v0.1.0-beta.1-aarch64-apple-darwin.tar.gz.sha256
   shasum -a 256 -c shellsort-v0.1.0-beta.1-x86_64-apple-darwin.tar.gz.sha256
   ```

   Check that each archive contains only `shellsort`, executable `organizer.sh`, and `LICENSE`, and recheck each binary's minimum OS metadata with `vtool -show-build` on macOS.
5. After maintainer review, create a **draft prerelease** manually at the existing GitHub repository, name it `v0.1.0-beta.1`, mark it as a prerelease, and attach the two exact `.tar.gz` files and their `.sha256` files. Use release notes that state mutation is macOS-only and Linux/Windows refuse it. Do not publish until manual installation, preview, confirmation, organize, history, undo, and uninstall checks pass on both Intel and Apple Silicon Macs using disposable folders.

The workflow does not create the tag, GitHub release, or Homebrew tap. The tag commands above are documentation only; do not run them before review and approval.

The target-specific build commands, when run on matching native hosts, are:

```sh
cargo +1.99.1 build --locked --release --target aarch64-apple-darwin --bin shellsort
cargo +1.99.1 build --locked --release --target x86_64-apple-darwin --bin shellsort
```

For this candidate, expected archive names are `shellsort-v0.1.0-beta.1-aarch64-apple-darwin.tar.gz` and `shellsort-v0.1.0-beta.1-x86_64-apple-darwin.tar.gz`. This repository does not yet have a published beta archive or checksum.

For local archive smoke testing, extract into a temporary directory, put that directory first on `PATH`, verify `shellsort --version`, and run preview, explicit-confirmation, history, and undo checks only against disposable fixtures. Never run an organization command against personal folders during release verification.

## Homebrew custom tap preparation

`packaging/homebrew-tap/Formula/shellsort.rb` is a placeholder template. Its remaining placeholders are two architecture-specific archive URLs and two matching SHA-256 values. The homepage is the confirmed project repository, `https://github.com/a1nbuselvan/Shellsort`. A separate tap repository has not been created or confirmed. The intended layout there is:

```text
homebrew-shellsort/
└── Formula/
    └── shellsort.rb
```

After the beta assets exist, create and confirm a public tap repository (for example, the GitHub repository name `homebrew-shellsort` under the project owner, if available). Copy the formula into `Formula/shellsort.rb`, retain version `0.1.0-beta.1`, and replace all four clearly marked URL/SHA-256 placeholders using the final GitHub release asset URLs and hashes computed from those exact archives. Then run `brew style`, `brew audit --strict --online`, and install/test on both Mac architectures. No tap repository or beta release exists yet; `brew tap` and `brew install shellsort` are not currently available and must not be advertised as working.

## Release blockers

Before public distribution, run the native release workflow for `v0.1.0-beta.1`, inspect and verify immutable archives/checksums, complete manual tests on both Macs using disposable folders, decide and complete signing/notarization, create the tap repository, replace and validate the Homebrew formula placeholders, and then publish an approved draft prerelease. Do not claim public Homebrew availability before a valid tap formula and published assets exist.
