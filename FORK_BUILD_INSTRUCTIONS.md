# Fork Build Instructions

This document explains how to build this forked version of cosmic-edit with all integrated features.

## Overview

This fork includes:
- **PR #497**: Multi-window session hot exit restore with auto save/backup
- **PR #479**: Fix Save As duplicate editor tabs
- **Large file memory fix** (Issue #457): Prevents memory explosion when opening 100K+ line files
- **Skip content hash for large files**: Performance optimization
- **Rope buffer support**: Experimental feature for viewing very large files

## Prerequisites

- Rust 1.85 or later (Edition 2024)
- Standard COSMIC desktop development dependencies

On Fedora:
```bash
sudo dnf install gtk3-devel libxkbcommon-devel wayland-devel
```

## Build Steps

### 1. Clone the repository

```bash
git clone https://github.com/Lcstyle/cosmic-edit.git
cd cosmic-edit
git checkout feature/integrated-enhancements
```

### 2. Remove stale lock file

The Cargo.lock may contain references to upstream packages that conflict with our patches. Remove it to force regeneration:

```bash
rm Cargo.lock
```

### 3. Verify Cargo.toml patches

Ensure `Cargo.toml` has the following configuration for the forked cosmic-text:

```toml
# Direct dependency on forked cosmic-text
[dependencies.cosmic-text]
git = "https://github.com/Lcstyle/cosmic-text.git"
branch = "feature/rope-buffer"
features = ["syntect", "vi", "rope-buffer"]

# Patch to redirect transitive dependencies (e.g., via libcosmic/iced_glyphon)
[patch.'https://github.com/pop-os/cosmic-text.git']
cosmic-text = { git = "https://github.com/Lcstyle/cosmic-text.git", branch = "feature/rope-buffer" }

# Patch for onig (syntect dependency)
[patch.crates-io]
onig = { git = "https://github.com/rust-onig/rust-onig.git", branch = "main" }
onig_sys = { git = "https://github.com/rust-onig/rust-onig.git", branch = "main" }
```

### 4. Build

```bash
cargo build --release
```

The build will automatically:
- Fetch the forked cosmic-text with the `matches()` method fix and rope-buffer feature
- Apply patches to redirect all cosmic-text dependencies to the fork
- Compile with all integrated features

## Why These Patches Are Needed

### cosmic-text fork

The upstream cosmic-text removed or changed the `Attrs::matches()` method. Our fork:
1. Restores this method for font matching compatibility
2. Adds the `rope-buffer` feature for efficient large file handling
3. Includes `RopeBuffer`, `RopeText`, `SparseMetadata`, and `LineCache` types

### onig patches

The syntect dependency requires onig for syntax highlighting. The crates.io version has compatibility issues, so we use the git version.

## Features Included

### Session Restore & Auto-Save (PR #497)
- Automatic session backup on close
- Crash recovery for unsaved work
- Configurable auto-save interval
- Multi-window session management

### Save As Fix (PR #479)
- Prevents duplicate tabs when using Save As to an already-open file
- Switches to existing tab instead of creating duplicates

### Large File Memory Fix (Issue #457)
- Prevents OOM crashes when opening 100K+ line files
- Reduces memory from 18+ GB to ~200MB for 60MB files
- Sets minimal buffer height before loading to limit initial shaping

### Rope Buffer (Experimental)
- Efficient viewing of very large files (100MB+)
- Only loads ~500 lines at a time into the editor
- Note: Currently read-only - editing/saving large files is not fully implemented

## Branches

| Branch | Description |
|--------|-------------|
| `feature/integrated-enhancements` | All features integrated (recommended) |
| `fix/large-file-memory` | Minimal 23-line fix for memory explosion only |
| `feature/rope-buffer-integration` | Rope buffer integration only |

## Related Repositories

- **cosmic-text fork**: https://github.com/Lcstyle/cosmic-text.git (branch: `feature/rope-buffer`)

## Troubleshooting

### Version Mismatch Errors

If you see errors about conflicting cosmic-text versions:

```
error: failed to select a version for `cosmic-text`
```

Delete `Cargo.lock` and rebuild:

```bash
rm Cargo.lock
cargo build --release
```

### `matches` method not found

If you see:
```
error[E0599]: no method named `matches` found for struct `Attrs<'a>`
```

Ensure:
1. The `[patch.'https://github.com/pop-os/cosmic-text.git']` section exists in Cargo.toml
2. Delete Cargo.lock and rebuild

### Missing Dependencies

Ensure you have the COSMIC desktop development dependencies installed for your distro.

## Running

```bash
./target/release/cosmic-edit
```

Or for testing with a large file:

```bash
./target/release/cosmic-edit /path/to/large/file.txt
```

## Configuration

New settings are available in the app settings:
- `reopen_on_start`: Reopen projects and tabs on start (default: true)
- `auto_save`: Automatically save files after inactivity (default: false)
- `auto_save_interval_secs`: Interval between auto-saves (default: 2)
