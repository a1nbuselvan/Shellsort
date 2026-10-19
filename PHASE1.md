# Shellsort Phase 1

Phase 1 established the macOS desktop shell and user interface. At that milestone, the app opened the native folder picker and displayed the selected folder path without scanning it. The current read-only scanner and plan preview are documented in [PHASE2.md](PHASE2.md). No organizing or file mutation behavior is implemented.

## Requirements

- macOS 10.15 or later with Xcode Command Line Tools (`xcode-select --install` if needed).
- Node.js LTS and npm.
- Rust stable via [rustup](https://rustup.rs/), including the `aarch64-apple-darwin` target on Apple Silicon or `x86_64-apple-darwin` on Intel.
- Tauri's macOS prerequisites: [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/).

Check the tools:

```sh
node --version
npm --version
rustc --version
cargo --version
rustfmt --version
```

If the Rust toolchain is installed with rustup but its binaries are not on `PATH`, add the active toolchain's `bin` directory for this terminal session:

```sh
export PATH="$(dirname "$(rustup which cargo)"):$PATH"
```

## Develop

From the repository root:

```sh
npm install
npm run tauri dev
```

This opens the desktop app. The **Choose folder** button invokes the native macOS directory picker. Choosing a folder only displays its path in the app.

## Build and verify

```sh
npm run typecheck
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri -- build --bundles app
```

`npm run build` runs TypeScript checking before creating the frontend bundle. Rust formatting, checking, linting, and tests are separate commands so their results can be reviewed individually.

## Structure

- `src/` — React and TypeScript UI.
- `src-tauri/` — Rust entry point, Tauri configuration, and capabilities.
- `src-tauri/capabilities/default.json` — main-window permissions. The frontend has the default Tauri core permissions and permission to open a native dialog; it has no filesystem plugin or shell permission.

The Rust side only initializes Tauri and the dialog plugin. The organizer engine, file scanning, preview generation, and file moves are intentionally out of scope until a later phase.
