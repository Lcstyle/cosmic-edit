# COSMIC Edit - Lcstyle's Enhanced Fork

An extensively enhanced fork of [COSMIC Edit](https://github.com/pop-os/cosmic-edit) featuring large file handling, session management, pinned notes with AI-powered naming, markdown rendering, and numerous UX improvements.

![Screenshot](res/screenshots/screenshot-1.png)

## Overview of Enhanced Features

This fork has diverged significantly from upstream with the following major additions:

### Pinned Notes with Markdown Rendering
- **Pinned Notes Sidebar**: Dedicated collapsible sidebar for quick access to pinned markdown notes
- **AI-Powered Filename Suggestions**: Claude API integration suggests descriptive filenames when pinning tabs (`1c71ee2`)
- **Markdown View Modes**: Raw, Rendered, and Split view modes with bold/italic formatting support (`f801169`, `3be42ff`)
- **Double-Click Navigation**: Double-click in rendered view switches to split; double-click preview in split returns to rendered (`6328670`)
- **Unpin to Documents**: Unpinning moves file from pinned notes folder to ~/Documents (`6328670`)

> **Note**: Pinned tabs initially auto-loaded as regular tabs on startup (`04c1eda`), but this was later refactored to use a dedicated sidebar that loads notes on-demand (`0d64f96`). The sidebar approach provides better organization without cluttering the tab bar.

### Tab Bar Enhancements
- **Mouse Wheel Scrolling**: Scroll through tabs using mouse wheel on the tab bar (`6328670`)
- **Navigation Buttons**: `<` `>` buttons for users without scroll wheels (`6328670`)
- **Auto-Scroll to Active**: Tab bar automatically scrolls to keep the active tab visible (`6328670`)

### Session Management & Hot Exit
- **Multi-Window Session Restore**: Automatically saves and restores all windows, tabs, and cursor positions (`502b747`)
- **Auto-Save & Backup**: Configurable automatic backup with crash recovery (`502b747`)
- **Single Window Mode**: Option to restore all tabs into a single window (`468b3d5`)
- **Silent Close UX**: Clean window closing without prompts when hot exit is enabled (`f848be6`)

### Large File Support
- **Memory Explosion Fix**: Prevents OOM crashes on 100K+ line files - reduces memory from 18GB+ to ~200MB (`5354354`, `31a36bf`)
- **Rope Buffer Integration**: Efficient viewing of very large files (>1MB) with windowed loading (`9b823da`)
- **Content Hash Skip**: Skips expensive hash computation for large files (`2963083`)
- **Scrollbar Improvements**: Proper thumb sizing and click-to-jump for large files (`404f697`)

### Find Dialog Enhancements
- **List Matching Lines**: New option to list all matches in a resizable results window (`addbd4d`)
- **Resizable Results**: Draggable results window with context menu (`05d82b0`)
- **Scroll Position Fixes**: Proper scroll handling when navigating results (`05d82b0`)

### Additional Fixes
- **Save As Duplicate Prevention**: Prevents creating duplicate tabs when using Save As (PR #479) (`9d32449`)
- **Path Normalization**: Stabilizes tab updates with consistent file path handling (`a1d7f38`)

---

## Branch Comparison

| Branch | Base | Purpose | Key Commits |
|--------|------|---------|-------------|
| `feature/integrated-enhancements` | `3fa04ec` | **Recommended** - All features integrated | All commits below |
| `feature/list-matching-lines` | `3fa04ec` | Find dialog enhancements only | `addbd4d`, `05d82b0` |
| `feature/rope-buffer-integration` | `fed3a59` | Rope buffer + memory fix only | `9b823da`, `5354354` |
| `fix/large-file-memory` | `fed3a59` | Minimal 23-line memory fix | `5354354` |
| `fix/skip-large-file-hash` | - | Hash computation skip only | `2963083` |
| `master` | upstream | Synced with upstream | - |

### Branch Ancestry

```
upstream/master (fed3a59)
    │
    ├── fix/large-file-memory (5354354)
    │       └── Fix large file memory explosion
    │
    ├── feature/rope-buffer-integration (3fa04ec)
    │       ├── Fix large file memory explosion
    │       ├── Rope buffer integration
    │       └── Fork build instructions
    │
    └── feature/integrated-enhancements (6328670) ← RECOMMENDED
            ├── All rope buffer features
            ├── Hot exit & session restore
            ├── Save As fix (PR #479)
            ├── Find dialog enhancements
            ├── Pinned notes & markdown
            ├── AI filename suggestions
            └── Tab bar scrolling
```

### Common Ancestors

| Branches | Common Ancestor |
|----------|-----------------|
| `integrated-enhancements` ↔ `rope-buffer-integration` | `3fa04ec` (Add fork build instructions) |
| `integrated-enhancements` ↔ `master` | `fed3a59` (Merge weblate translations) |
| `list-matching-lines` ↔ `rope-buffer-integration` | `3fa04ec` (Add fork build instructions) |

---

## Detailed Commit History

### Since Rope Buffer Integration (`9b823da`)

Listed chronologically (oldest to newest):

| Commit | Description |
|--------|-------------|
| `a1d7f38` | Normalize file paths and stabilize tab updates |
| `9d32449` | Prevent Save As from creating duplicate tabs (PR #479) |
| `502b747` | **Multi-window session hot exit restore with auto-save** |
| `0758525` | Merge PR #479 (Save As fix) |
| `31a36bf` | Fix large file memory explosion (Issue #457) |
| `2963083` | Skip content hash computation for large files |
| `db9eca6` | Update build config for forked cosmic-text |
| `404f697` | Merge rope buffer and scrollbar improvements |
| `6b4f9b1` | Update README with fork build instructions |
| `f848be6` | Fix close UX for hot exit: silent close, delete backup on discard |
| `addbd4d` | **Add "List matches" option to Find dialog** |
| `05d82b0` | Resizable results window, context menu, scroll fixes |
| `81c6139` | Add pinned tabs implementation plan |
| `cef42cd` | Add pinned and markdown view mode fields to tab |
| `0b3ef37` | Add pinned notes directory config setting |
| `39ab116` | Add pin/unpin file operations module |
| `7e73706` | Add pin tab dialog UI |
| `e8e4eed` | Add tab context menu with pin/unpin options |
| `f801169` | **Add markdown rendering widget** |
| `1bd09e2` | Add view mode toggle and split view |
| `04c1eda` | Load pinned tabs on startup *(later changed to sidebar)* |
| `5b90e7c` | Polish: edge cases and keyboard shortcuts |
| `468b3d5` | Add single window restore mode, fix duplication bug |
| `0d64f96` | **Refactor pinned notes to dedicated sidebar** |
| `91e81e3` | Add Notepad++ session restore guide |
| `1c71ee2` | **Add Claude API integration for filename suggestions** |
| `3be42ff` | Add default view mode setting, fix bold/italic rendering |
| `0057711` | Documentation for AI integration and markdown improvements |
| `c402f66` | Consolidate AI integration docs |
| `6328670` | **Add tab bar scrolling, double-click view modes, unpin file move** |

### Feature Highlights by Commit

#### Hot Exit & Session Restore (`502b747`)
- Automatic session backup on close
- Crash recovery for unsaved work
- Multi-window session management
- Configurable auto-save interval

#### List Matching Lines (`addbd4d`, `05d82b0`)
- Find dialog gains "List matches" option
- Results shown in resizable floating window
- Context menu with copy/select all
- Click to jump to match

#### Markdown Rendering (`f801169`, `1bd09e2`, `3be42ff`)
- pulldown-cmark for parsing
- libcosmic widgets for rendering
- Bold, italic, code blocks, lists, headings
- Default view mode setting (Raw/Rendered/Split)

#### Pinned Notes Sidebar (`0d64f96`)
- Replaced auto-loading tabs with dedicated sidebar
- Collapsible section above project navigator
- Click to open, pin icon in tab title
- Refresh on pin/unpin operations

#### AI Filename Suggestions (`1c71ee2`)
- Claude API integration (misanthropy crate)
- Configurable via `ai_config.toml`
- Loading indicator in pin dialog
- Sanitizes suggestions for filesystem

#### Tab Bar Scrolling (`6328670`)
- Mouse wheel navigates tabs
- `<` `>` buttons for accessibility
- Auto-scroll keeps active tab visible
- Double-click view mode switching

---

## Quick Start

```bash
# Clone the fork
git clone https://github.com/Lcstyle/cosmic-edit.git
cd cosmic-edit

# Use the recommended branch
git checkout feature/integrated-enhancements

# Remove stale lockfile and build
rm Cargo.lock
cargo build --release

# Run
./target/release/cosmic-edit
```

## Prerequisites

- **Rust 1.85+** (Edition 2024)
- COSMIC desktop development dependencies

### Fedora
```bash
sudo dnf install gtk3-devel libxkbcommon-devel wayland-devel
```

### Ubuntu/Debian
```bash
sudo apt install libgtk-3-dev libxkbcommon-dev libwayland-dev
```

---

## Build Details

### Why Remove Cargo.lock?

The lock file may reference upstream packages that conflict with our forked dependencies. Removing it forces fresh dependency resolution.

### Required Patches (Cargo.toml)

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

---

## Configuration

### App Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `reopen_on_start` | true | Restore session on startup |
| `session_restore_mode` | `multi_window` | `multi_window` or `single_window` |
| `auto_save` | false | Auto-save after inactivity |
| `auto_save_interval_secs` | 2 | Auto-save interval |
| `pinned_notes_dir` | `~/Documents/cosmic-pinned-notes/` | Pinned notes location |
| `default_markdown_view_mode` | `Rendered` | Default view for .md files |
| `anthropic_api_key` | None | API key for AI features |

### AI Configuration (`ai_config.toml`)

Located at `~/.config/com.system76.CosmicEdit/ai_config.toml`:

```toml
[filename_suggestion]
model = "claude-3-5-haiku-latest"
max_tokens = 50
max_content_length = 50000
prompt = """..."""
```

---

## Troubleshooting

### Version Mismatch Errors
```
error: failed to select a version for `cosmic-text`
```
**Solution**: Delete `Cargo.lock` and rebuild.

### `matches` method not found
```
error[E0599]: no method named `matches` found
```
**Solution**: Ensure patch section exists in Cargo.toml, delete Cargo.lock.

### AI Suggestions Not Working
- Verify API key in Settings → AI Features
- Check `~/.config/com.system76.CosmicEdit/ai_config.toml`
- View logs with `RUST_LOG=debug cargo run`

---

## Related Repositories

- **cosmic-text fork**: https://github.com/Lcstyle/cosmic-text.git
  - Branch: `feature/rope-buffer`
  - Adds: `RopeBuffer`, `RopeText`, `SparseMetadata`, `LineCache`

---

## Debugging

```bash
RUST_LOG=debug cargo run      # Debug output
RUST_LOG=trace cargo run      # Verbose output
RUST_LOG=cosmic_edit=debug cargo run  # App-specific
```

---

## Upstream

This fork is based on [pop-os/cosmic-edit](https://github.com/pop-os/cosmic-edit). See upstream for the original project documentation.

---

## License

GPL-3.0-only (same as upstream)
