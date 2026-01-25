# Pinned Tabs with Markdown Rendering for cosmic-edit

## Overview

This document describes the implementation of pinned tabs functionality with markdown viewing/rendering support. Pinned tabs are saved as `.md` files in `~/Documents/cosmic-pinned-notes/` and automatically load on startup. Markdown files can be viewed in three modes: raw (editable), rendered (read-only), and split view.

**Rendering Approach**: Widget-based using libcosmic/iced widgets. No cosmic-text modifications required.

## Features

### Pinned Tabs
- Pin any tab to save it as a markdown file in the pinned notes directory
- Pinned tabs show a pin icon indicator
- Pinned tabs automatically load on startup
- Right-click context menu for pin/unpin operations

### Markdown View Modes
- **Raw**: Standard editable text editor view (default)
- **Rendered**: Read-only rendered markdown view using libcosmic widgets
- **Split**: Side-by-side editor and rendered view

### Configuration
- `pinned_notes_dir`: Directory for pinned notes (default: `~/Documents/cosmic-pinned-notes/`)

## Architecture

### Data Structures

#### MarkdownViewMode (tab.rs)
```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum MarkdownViewMode {
    #[default]
    Raw,
    Rendered,
    Split,
}
```

#### EditorTab Extensions (tab.rs)
- `is_pinned: bool` - Whether this tab is pinned
- `markdown_view_mode: MarkdownViewMode` - Current view mode for markdown files

#### SessionTab Extensions (hotexit.rs)
- `is_pinned: bool` - Persisted pinned state

### New Module: pinned.rs

Provides file operations for pinned tabs:
- `pin_tab()` - Move/save file to pinned notes directory
- `unpin_tab()` - Remove pinned status (file stays in place)
- `scan_pinned_notes()` - Scan directory for .md files at startup
- `PinError` - Error handling enum

### Markdown Rendering (markdown_view.rs)

Widget-based rendering using pulldown-cmark + libcosmic widgets:
- Headings: `widget::text().size()` based on level
- Paragraphs: `widget::text()` with word wrap
- Code blocks: `widget::container(widget::text::monotext())`
- Inline code: `widget::text::monotext()` with background
- Lists: `widget::column()` with bullet/number prefixes
- Links: `widget::button::link()` or styled text

## Messages

New messages for pinned tabs functionality:
- `PromptPinName(Entity)` - Show dialog to enter pin name
- `PinNameValueChanged(String)` - Dialog input changed
- `PinNameConfirmed(Entity, String)` - User confirmed pin name
- `PinNameCancelled` - User cancelled dialog
- `TabPin(Entity)` - Pin a tab
- `TabUnpin(Entity)` - Unpin a tab
- `SetMarkdownViewMode(Entity, MarkdownViewMode)` - Set view mode
- `ToggleMarkdownViewMode(Entity)` - Cycle through view modes

## Keyboard Shortcuts

- `Ctrl+Shift+M` - Toggle markdown view mode for current tab

## Implementation Phases

1. **Phase 1**: Documentation & Project Setup
2. **Phase 2**: Core Data Structures (tab.rs, hotexit.rs)
3. **Phase 3**: Configuration (config.rs)
4. **Phase 4**: Pin/Unpin File Operations (pinned.rs)
5. **Phase 5**: UI - Pin Dialog (main.rs, i18n)
6. **Phase 6**: Tab Context Menu & Visual Indicators (menu.rs, main.rs)
7. **Phase 7**: Markdown Rendering Widget (markdown_view.rs)
8. **Phase 8**: View Mode Toggle & Split View (main.rs, menu.rs)
9. **Phase 9**: Startup Loading (main.rs)
10. **Phase 10**: Polish & Edge Cases

## Testing

### Manual Testing Checklist

1. **Pin a tab**:
   - Open a new untitled tab, add content
   - Right-click tab → Pin Tab
   - Enter name in dialog
   - Verify file created in `~/Documents/cosmic-pinned-notes/name.md`
   - Verify tab shows pin icon

2. **Unpin a tab**:
   - Right-click pinned tab → Unpin Tab
   - Verify pin icon removed
   - File stays in pinned notes directory

3. **Startup loading**:
   - Close cosmic-edit
   - Re-open cosmic-edit
   - Verify pinned tabs auto-load

4. **Markdown view modes**:
   - Open a .md file
   - Toggle between Raw/Rendered/Split
   - Verify Raw is editable, Rendered is read-only
   - Verify Split shows both panes

5. **Persistence**:
   - Pin a tab, set to Split view, close app
   - Re-open, verify tab loads in Split view

### Build Verification

```bash
cd /home/lcstyle/Documents/RustroverProjects/cosmic-edit
cargo build --release
cargo clippy
cargo test
```

## File Summary

| File | Purpose |
|------|---------|
| `src/tab.rs` | EditorTab struct, MarkdownViewMode enum |
| `src/main.rs` | Messages, App state, view rendering, startup logic |
| `src/hotexit.rs` | SessionTab persistence |
| `src/config.rs` | Pinned notes directory config, AI settings, default view mode |
| `src/pinned.rs` | Pin/unpin file operations |
| `src/menu.rs` | Tab context menu |
| `src/markdown_view.rs` | Markdown rendering widget |
| `src/ai.rs` | AI-powered filename suggestions |
| `src/ai_config.toml` | AI configuration (model, prompt, limits) |
| `Cargo.toml` | pulldown-cmark, misanthropy, toml dependencies |
| `i18n/en/cosmic_edit.ftl` | Localization strings |

---

## AI-Powered Filename Suggestions (January 2026)

### Overview

When pinning a tab, the AI analyzes document content and suggests an appropriate filename using the Claude API.

### Components

#### New Files

- **`src/ai.rs`** - AI module with:
  - Configuration loading from TOML
  - `suggest_filename()` async function
  - `sanitize_suggested_filename()` for cleaning responses

- **`src/ai_config.toml`** - Editable configuration:
  ```toml
  [filename_suggestion]
  model = "claude-3-5-haiku-latest"  # Fast model for suggestions
  max_tokens = 50
  max_content_length = 50000
  prompt = """..."""  # Customizable prompt template
  ```

#### Configuration

New fields in `config.rs`:
- `anthropic_api_key: Option<String>` - API key (entered in Settings)
- `ai_max_content_size: usize` - Max file size for AI analysis (default 1MB)

Config file locations (checked in order):
1. `$CARGO_MANIFEST_DIR/src/ai_config.toml` (development)
2. `~/.config/com.system76.CosmicEdit/ai_config.toml` (user config)
3. `./ai_config.toml` (current directory)

### User Flow

1. Enter API key in Settings → AI Features
2. Right-click tab → Pin Tab
3. Loading indicator shows "Suggesting name..."
4. AI suggestion populates the name field
5. User can edit or accept the suggestion

### Messages

- `AiSuggestionReceived(String)` - AI returned a suggestion
- `AnthropicApiKeyChanged(String)` - API key changed in settings

### UI State

- `ai_suggesting: bool` - Shows loading indicator
- `ai_suggestion_received: bool` - Prevents overwriting user edits

---

## Default Markdown View Mode (January 2026)

### Overview

Users can now set the default view mode for opening markdown files.

### Configuration

New enum in `config.rs`:
```rust
pub enum DefaultMarkdownViewMode {
    Raw,       // Text editing view
    Rendered,  // Read-only preview (NEW DEFAULT)
    Split,     // Side-by-side
}
```

### Settings UI

Settings → App Settings → "Default markdown view" dropdown

### Implementation

- Added `From<DefaultMarkdownViewMode>` trait in `tab.rs`
- Applied when opening files in:
  - Session restore
  - File open dialog
  - Pinned notes sidebar

---

## Markdown Bold/Italic Rendering Fix (January 2026)

### Problem

Inline formatting (`**bold**`, `*italic*`) was not rendering in preview mode.

### Solution

Updated `markdown_view.rs` to:

1. Track formatting state:
   ```rust
   in_strong: bool,  // Inside **bold**
   in_emphasis: bool, // Inside *italic*
   ```

2. Apply formatting on tag events:
   ```rust
   Tag::Strong => state.in_strong = true,
   TagEnd::Strong => state.in_strong = false,
   ```

3. Render with proper fonts:
   - Bold: `font::Weight::Bold`
   - Italic: `font::Style::Italic`

### New Functions

- `render_rich_paragraph()` - Parses formatting markers
- `render_text_with_formatting()` - Applies bold/italic fonts
- Updated `render_list_item()` with formatting flags

---

## Dependencies Added

```toml
misanthropy = "0.0.8"  # Anthropic Claude API
toml = "0.8"           # TOML parsing for AI config
```

---

## Testing Updates

### AI Integration
1. Settings → AI Features → Enter API key
2. Open document with content
3. Right-click tab → Pin Tab
4. Verify loading indicator and AI suggestion

### Default View Mode
1. Settings → App Settings → Change "Default markdown view"
2. Open a `.md` file
3. Verify it opens in selected mode

### Bold/Italic Rendering
1. Create markdown with `**bold**` and `*italic*`
2. Switch to Rendered or Split view
3. Verify formatting displays correctly
