# AI Integration and Markdown Improvements

This document covers the features implemented on January 25, 2026, including Claude API integration for AI-powered features and improvements to markdown rendering.

## Table of Contents

1. [Claude API Integration](#claude-api-integration)
2. [AI Configuration File](#ai-configuration-file)
3. [Default Markdown View Mode](#default-markdown-view-mode)
4. [Markdown Bold/Italic Rendering Fix](#markdown-bolditalic-rendering-fix)

---

## Claude API Integration

### Overview

Integrated the Anthropic Claude API to provide AI-powered filename suggestions when pinning notes. When a user pins a tab, the AI analyzes the document content and suggests an appropriate filename.

### Components

#### New Files

- **`src/ai.rs`** - AI module containing:
  - `AiConfig` struct for configuration
  - `FilenameSuggestionConfig` with model, max_tokens, max_content_length, and prompt
  - `load_config()` - Loads configuration from TOML file
  - `suggest_filename()` - Async function that calls Claude API
  - `sanitize_suggested_filename()` - Cleans AI response for use as filename

- **`src/ai_config.toml`** - Editable configuration file for AI settings

#### Modified Files

- **`Cargo.toml`** - Added dependencies:
  - `misanthropy = "0.0.8"` - Rust bindings for Anthropic API
  - `toml = "0.8"` - TOML parsing for config file

- **`src/config.rs`** - Added config fields:
  - `anthropic_api_key: Option<String>` - API key storage
  - `ai_max_content_size: usize` - Max file size for AI analysis (default 1MB)

- **`src/main.rs`** - Added:
  - `ai_suggesting: bool` field - Tracks loading state
  - `ai_suggestion_received: bool` field - Prevents overwriting user edits
  - Settings UI for entering API key
  - Loading indicator in pin dialog
  - Message handlers for AI suggestion flow

- **`i18n/en/cosmic_edit.ftl`** - Added strings:
  - `ai-features` - Section title
  - `anthropic-api-key` - Setting label
  - `anthropic-api-key-placeholder` - Input placeholder
  - `anthropic-api-key-description` - Setting description
  - `ai-suggesting` - Loading indicator text

### User Flow

1. User enters Anthropic API key in Settings → AI Features
2. User right-clicks a tab and selects "Pin Tab"
3. Pin dialog opens with loading indicator ("Suggesting name...")
4. AI analyzes document content and suggests a filename
5. Suggestion appears in the text field
6. User can edit or accept the suggestion
7. If user starts typing before AI responds, their input is preserved

### Configuration

The AI configuration file (`ai_config.toml`) can be customized:

```toml
[filename_suggestion]
# Model to use (haiku is fast and cheap)
model = "claude-3-5-haiku-latest"

# Maximum tokens for response
max_tokens = 50

# Maximum content length to analyze (bytes)
max_content_length = 50000

# Prompt template ({content} is replaced with document content)
prompt = """Based on the following document content, suggest a short, descriptive filename...
{content}
---"""
```

**Config file locations (checked in order):**
1. `$CARGO_MANIFEST_DIR/src/ai_config.toml` (development)
2. `~/.config/com.system76.CosmicEdit/ai_config.toml` (user config)
3. `./ai_config.toml` (current directory)

---

## AI Configuration File

### Purpose

The AI configuration is stored in an editable TOML file rather than hardcoded, allowing users to:
- Change the AI model (e.g., switch to Sonnet for smarter suggestions)
- Adjust token limits
- Customize the prompt template
- Set content length limits

### Default Configuration

```toml
# AI Configuration for Cosmic Edit
# Copy this file to ~/.config/com.system76.CosmicEdit/ai_config.toml

[filename_suggestion]
model = "claude-3-5-haiku-latest"
max_tokens = 50
max_content_length = 50000
prompt = """Based on the following document content, suggest a short, descriptive filename (without extension).
The filename should:
- Be 2-5 words, lowercase, separated by hyphens
- Capture the main topic or purpose of the document
- Be suitable for a markdown note file

Respond with ONLY the suggested filename, nothing else. No quotes, no extension, no explanation.

Document content:
---
{content}
---"""
```

---

## Default Markdown View Mode

### Overview

Added a setting to control how markdown files open by default. Previously, markdown files always opened in Raw mode. Now users can choose their preferred default view.

### Configuration

New config field in `src/config.rs`:
```rust
pub enum DefaultMarkdownViewMode {
    Raw,       // Text editing view
    Rendered,  // Read-only rendered view (NEW DEFAULT)
    Split,     // Side-by-side editor and preview
}
```

### Settings UI

Located in Settings → App Settings → "Default markdown view"

Options:
- **Raw** - Opens in text editing mode
- **Rendered** - Opens in rendered preview mode (default)
- **Split** - Opens in split view with editor and preview

### Implementation

- Added `DefaultMarkdownViewMode` enum to `config.rs`
- Added `From` trait implementation in `tab.rs` to convert config enum to tab enum
- Applied default view mode in three locations where files are opened:
  - Session restore
  - File open dialog
  - Opening from pinned notes sidebar

---

## Markdown Bold/Italic Rendering Fix

### Problem

Markdown inline formatting (`**bold**` and `*italic*`) was not being rendered in the preview. The text would appear with the asterisks visible instead of formatted.

### Root Cause

The markdown renderer (`src/markdown_view.rs`) was tracking `Tag::Strong` and `Tag::Emphasis` events but not applying the formatting when rendering text. The text was accumulated into strings and rendered at paragraph end, losing formatting context.

### Solution

1. **Added formatting state tracking:**
   ```rust
   struct RenderState {
       // ... existing fields ...
       in_strong: bool,   // Inside **bold**
       in_emphasis: bool, // Inside *italic*
   }
   ```

2. **Track formatting in event handlers:**
   ```rust
   Tag::Strong => state.in_strong = true,
   Tag::Emphasis => state.in_emphasis = true,
   TagEnd::Strong => state.in_strong = false,
   TagEnd::Emphasis => state.in_emphasis = false,
   ```

3. **Apply formatting when rendering:**
   - For paragraphs: Use marker characters to track formatting spans, then render with appropriate fonts
   - For list items: Pass formatting flags to render function
   - For standalone text: Apply bold/italic font directly

4. **New render functions:**
   - `render_rich_paragraph()` - Parses formatting markers and renders text segments
   - `render_text_with_formatting()` - Renders text with bold/italic font
   - Updated `render_list_item()` to accept formatting flags

### Result

- `**bold text**` now renders with bold font weight
- `*italic text*` now renders with italic font style
- Works in paragraphs, list items, headings, and standalone text

---

## Files Changed Summary

| File | Changes |
|------|---------|
| `Cargo.toml` | Added `misanthropy` and `toml` dependencies |
| `src/ai.rs` | New file - AI module |
| `src/ai_config.toml` | New file - AI configuration |
| `src/config.rs` | Added API key, AI settings, and markdown view mode config |
| `src/main.rs` | AI integration, settings UI, message handlers |
| `src/markdown_view.rs` | Fixed bold/italic rendering |
| `src/tab.rs` | Added `From` trait for view mode conversion |
| `i18n/en/cosmic_edit.ftl` | Added AI and markdown view mode strings |

---

## Testing

### AI Integration
1. Open Settings → AI Features
2. Enter your Anthropic API key
3. Open a document with content
4. Right-click the tab → Pin Tab
5. Observe loading indicator and AI suggestion

### Markdown View Mode
1. Open Settings → App Settings
2. Change "Default markdown view" to your preference
3. Open a `.md` file
4. Verify it opens in the selected view mode

### Bold/Italic Rendering
1. Create a markdown file with:
   ```markdown
   This is **bold text** and this is *italic text*.

   **Bold paragraph**

   - **Bold list item**
   - *Italic list item*
   ```
2. Switch to Rendered or Split view
3. Verify formatting appears correctly
