# COSMIC Edit - Enhanced Fork

This is an enhanced fork of [COSMIC Edit](https://github.com/pop-os/cosmic-edit) with integrated features for large file handling and session management.

![Screenshot](res/screenshots/screenshot-1.png)

## Features

This fork includes:
- **Session Restore & Auto-Save** (PR #497): Multi-window session hot exit restore with automatic backup
- **Save As Fix** (PR #479): Prevents duplicate editor tabs
- **Large File Memory Fix** (Issue #457): Prevents memory explosion when opening 100K+ line files
- **Rope Buffer**: Experimental windowed viewing for very large files (100MB+)
- **Scrollbar Improvements**: Proper thumb sizing and positioning for large files

## Quick Start

```bash
git clone https://github.com/Lcstyle/cosmic-edit.git
cd cosmic-edit
git checkout feature/integrated-enhancements
rm Cargo.lock
cargo build --release
./target/release/cosmic-edit
```

## Prerequisites

- Rust 1.85 or later (Edition 2024)
- COSMIC desktop development dependencies

On Fedora:
```bash
sudo dnf install gtk3-devel libxkbcommon-devel wayland-devel
```

## Build Details

### Why Remove Cargo.lock?

The Cargo.lock may contain references to upstream packages that conflict with our patches. Removing it forces fresh dependency resolution with our forked cosmic-text.

### Required Patches

This build uses patched dependencies in `Cargo.toml`:

```toml
# Forked cosmic-text with rope-buffer feature
[dependencies.cosmic-text]
git = "https://github.com/Lcstyle/cosmic-text.git"
branch = "feature/rope-buffer"
features = ["syntect", "vi", "rope-buffer"]

# Redirect transitive dependencies to our fork
[patch.'https://github.com/pop-os/cosmic-text.git']
cosmic-text = { git = "https://github.com/Lcstyle/cosmic-text.git", branch = "feature/rope-buffer" }

# Fix for syntect's onig dependency
[patch.crates-io]
onig = { git = "https://github.com/rust-onig/rust-onig.git", branch = "main" }
onig_sys = { git = "https://github.com/rust-onig/rust-onig.git", branch = "main" }
```

## Feature Details

### Session Restore & Auto-Save
- Automatic session backup on close
- Crash recovery for unsaved work
- Configurable auto-save interval
- Multi-window session management

### Large File Memory Fix
- Prevents OOM crashes when opening 100K+ line files
- Reduces memory from 18+ GB to ~200MB for 60MB files
- Sets minimal buffer height before loading to limit initial text shaping

### Rope Buffer (Experimental)
- Efficient viewing of very large files (>1MB uses rope buffer)
- Only loads ~500 lines at a time into the editor
- Scrollbar shows position relative to full file
- **Note**: Currently optimized for viewing - editing/saving large files uses windowed buffer

### Scrollbar Improvements
- Minimum thumb size (20px) for visibility on very large files
- Proper position calculation relative to total file size
- Click-to-jump works correctly with windowed buffers
- Drag scrolling fires refresh callbacks for large files

## Branches

| Branch | Description |
|--------|-------------|
| `feature/integrated-enhancements` | All features integrated (recommended) |
| `fix/large-file-memory` | Minimal 23-line fix for memory explosion only |
| `feature/rope-buffer-integration` | Rope buffer integration only |

## Related Repositories

- **cosmic-text fork**: https://github.com/Lcstyle/cosmic-text.git (branch: `feature/rope-buffer`)
  - Adds `RopeBuffer`, `RopeText`, `SparseMetadata`, `LineCache` types
  - Behind the `rope-buffer` feature flag

## Troubleshooting

### Version Mismatch Errors

```
error: failed to select a version for `cosmic-text`
```

Delete `Cargo.lock` and rebuild:
```bash
rm Cargo.lock
cargo build --release
```

### `matches` method not found

```
error[E0599]: no method named `matches` found for struct `Attrs<'a>`
```

Ensure:
1. The `[patch.'https://github.com/pop-os/cosmic-text.git']` section exists in Cargo.toml
2. Delete Cargo.lock and rebuild

## Configuration

New settings available in app settings:
- `reopen_on_start`: Reopen projects and tabs on start (default: true)
- `auto_save`: Automatically save files after inactivity (default: false)
- `auto_save_interval_secs`: Interval between auto-saves (default: 2)

## Upstream

This is a fork of [pop-os/cosmic-edit](https://github.com/pop-os/cosmic-edit). See the upstream repository for the original project and documentation.

## Debugging

You can get more detailed errors by using the `RUST_LOG` environment variable:
```bash
RUST_LOG=debug cargo run
RUST_LOG=trace cargo run  # Even more detail
```
