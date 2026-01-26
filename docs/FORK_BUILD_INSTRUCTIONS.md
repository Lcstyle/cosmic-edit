# Fork Build Instructions

This document provides detailed build instructions for Lcstyle's enhanced fork of COSMIC Edit.

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

### Rust Toolchain

- **Rust 1.85 or later** (Edition 2024 required)
- Install via rustup: https://rustup.rs/

```bash
# Check your Rust version
rustc --version

# Update if needed
rustup update stable
```

### System Dependencies

#### Fedora / RHEL / CentOS
```bash
sudo dnf install \
    gtk3-devel \
    libxkbcommon-devel \
    wayland-devel \
    libinput-devel \
    mesa-libEGL-devel \
    fontconfig-devel \
    freetype-devel
```

#### Ubuntu / Debian
```bash
sudo apt install \
    libgtk-3-dev \
    libxkbcommon-dev \
    libwayland-dev \
    libinput-dev \
    libegl1-mesa-dev \
    libfontconfig1-dev \
    libfreetype6-dev
```

#### Arch Linux
```bash
sudo pacman -S \
    gtk3 \
    libxkbcommon \
    wayland \
    libinput \
    mesa \
    fontconfig \
    freetype2
```

## Branch Selection

| Use Case | Branch |
|----------|--------|
| All features (recommended) | `feature/integrated-enhancements` |
| Just large file support | `feature/rope-buffer-integration` |
| Minimal memory fix only | `fix/large-file-memory` |
| Find dialog enhancements | `feature/list-matching-lines` |

```bash
# List available branches
git branch -a

# Switch branches
git checkout feature/integrated-enhancements
```

## Build Process

### Step 1: Remove Stale Lock File

**Important**: The `Cargo.lock` file may contain references to upstream packages that conflict with our forked dependencies.

```bash
rm Cargo.lock
```

### Step 2: Build

```bash
# Debug build (faster compilation, slower runtime)
cargo build

# Release build (slower compilation, optimized runtime)
cargo build --release
```

### Step 3: Run

```bash
# Debug build
./target/debug/cosmic-edit

# Release build
./target/release/cosmic-edit
```

## Cargo.toml Configuration

This fork requires specific patches in `Cargo.toml`. These are already configured in the repository:

```toml
# Main dependency: forked cosmic-text with rope buffer support
[dependencies.cosmic-text]
git = "https://github.com/Lcstyle/cosmic-text.git"
branch = "feature/rope-buffer"
features = ["syntect", "vi", "rope-buffer"]

# Redirect ALL references to cosmic-text (including transitive deps)
[patch.'https://github.com/pop-os/cosmic-text.git']
cosmic-text = { git = "https://github.com/Lcstyle/cosmic-text.git", branch = "feature/rope-buffer" }

# Fix for syntect's onig dependency (regex engine)
[patch.crates-io]
onig = { git = "https://github.com/rust-onig/rust-onig.git", branch = "main" }
onig_sys = { git = "https://github.com/rust-onig/rust-onig.git", branch = "main" }
```

### Why These Patches?

1. **cosmic-text fork**: Adds rope buffer types for efficient large file handling
2. **patch section**: Ensures all crates (including libcosmic) use our fork
3. **onig patches**: Fixes build issues with the Oniguruma regex engine

## Additional Dependencies (feature/integrated-enhancements)

The integrated enhancements branch adds:

```toml
# Markdown parsing
pulldown-cmark = "0.12"

# AI API integration
misanthropy = "0.0.8"

# TOML config parsing
toml = "0.8"
```

## Installation

### User Installation

```bash
# Copy to local bin
cp target/release/cosmic-edit ~/.local/bin/

# Or system-wide (requires root)
sudo cp target/release/cosmic-edit /usr/local/bin/
```

### Desktop Entry

Create `~/.local/share/applications/cosmic-edit-fork.desktop`:

```desktop
[Desktop Entry]
Name=COSMIC Edit (Fork)
Comment=Enhanced text editor for COSMIC
Exec=/home/YOUR_USERNAME/.local/bin/cosmic-edit %F
Icon=com.system76.CosmicEdit
Terminal=false
Type=Application
Categories=Utility;TextEditor;
MimeType=text/plain;
```

## Configuration Files

After first run, configuration files are created at:

| File | Purpose |
|------|---------|
| `~/.config/com.system76.CosmicEdit/config.ron` | Main app settings |
| `~/.config/com.system76.CosmicEdit/ai_config.toml` | AI feature settings |
| `~/.local/share/com.system76.CosmicEdit/session.ron` | Session data |
| `~/Documents/cosmic-pinned-notes/` | Pinned notes storage |

## Troubleshooting

### Error: "failed to select a version for cosmic-text"

```
error: failed to select a version for `cosmic-text`
```

**Cause**: Cargo.lock references incompatible package versions.

**Solution**:
```bash
rm Cargo.lock
cargo build --release
```

### Error: "no method named `matches` found for struct `Attrs<'a>`"

```
error[E0599]: no method named `matches` found for struct `Attrs<'a>`
```

**Cause**: Using upstream cosmic-text instead of the fork.

**Solution**:
1. Verify `[patch.'https://github.com/pop-os/cosmic-text.git']` section exists in Cargo.toml
2. Delete Cargo.lock and rebuild

### Error: "could not find `rope-buffer` in `cosmic_text`"

**Cause**: The rope-buffer feature is not available in the cosmic-text being used.

**Solution**:
1. Ensure Cargo.toml points to the correct fork and branch
2. Delete Cargo.lock and rebuild

### Linker Errors (undefined references)

**Cause**: Missing system development packages.

**Solution**: Install all prerequisites listed above for your distribution.

### Build Takes Too Long

For faster iteration during development:

```bash
# Use debug build
cargo build

# Use incremental compilation
export CARGO_INCREMENTAL=1

# Use faster linker (if available)
# Install: cargo install mold
export RUSTFLAGS="-C link-arg=-fuse-ld=mold"
```

## Updating

### Pull Latest Changes

```bash
git fetch origin
git pull origin feature/integrated-enhancements

# Rebuild with fresh dependencies
rm Cargo.lock
cargo build --release
```

### Merging Upstream Changes

```bash
# Add upstream remote (once)
git remote add upstream https://github.com/pop-os/cosmic-edit.git

# Fetch upstream
git fetch upstream

# Merge (may require conflict resolution)
git merge upstream/master
```

## Development

### Running with Debug Output

```bash
# All debug logs
RUST_LOG=debug cargo run

# Verbose trace logs
RUST_LOG=trace cargo run

# Specific module
RUST_LOG=cosmic_edit=debug cargo run
RUST_LOG=cosmic_edit::hotexit=debug cargo run
```

### Running Tests

```bash
cargo test
```

### Code Formatting

```bash
cargo fmt
```

### Linting

```bash
cargo clippy
```

## Feature Flags

The forked cosmic-text supports these features:

| Feature | Description |
|---------|-------------|
| `syntect` | Syntax highlighting |
| `vi` | Vi/Vim keybindings |
| `rope-buffer` | Large file support with rope data structure |

All are enabled by default in this fork.

## Related Forks

| Repository | Branch | Purpose |
|------------|--------|---------|
| [Lcstyle/cosmic-edit](https://github.com/Lcstyle/cosmic-edit) | `feature/integrated-enhancements` | This fork |
| [Lcstyle/cosmic-text](https://github.com/Lcstyle/cosmic-text) | `feature/rope-buffer` | Forked text engine with rope buffer |

## Getting Help

- Check existing issues: https://github.com/Lcstyle/cosmic-edit/issues
- Original upstream: https://github.com/pop-os/cosmic-edit
- COSMIC desktop: https://github.com/pop-os/cosmic-epoch
