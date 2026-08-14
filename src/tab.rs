// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    iced::{Point, advanced::graphics::text::font_system},
    widget::icon,
};
use cosmic_files::mime_icon::{FALLBACK_MIME_ICON, mime_for_path, mime_icon};
use cosmic_text::{
    Attrs, Buffer, Cursor, Edit, LineEnding, Selection, Shaping, SyntaxEditor, ViEditor, Wrap,
};
use regex::Regex;
use std::{
    fs,
    io::{self, Write},
    path::{self, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

use crate::{Config, SYNTAX_SYSTEM, fl, git::GitDiff, json_scan, json_tree::JsonViewState};

/// Indent used by the JSON Format command.
const JSON_FORMAT_INDENT: &str = "  ";

fn editor_text(editor: &ViEditor<'static, 'static>) -> String {
    editor.with_buffer(|buffer| {
        let mut text = String::new();
        for i in 0..buffer.line_count() {
            if let Some(cow) = buffer.line_text_cow(i) {
                text.push_str(&cow);
            }
            if let Some(ending) = buffer.line_ending(i) {
                text.push_str(ending.as_str());
            }
        }
        text
    })
}

/// Map parsed-text lines to buffer rows for fold application: `row_of[p]`
/// is the first buffer row of parsed line `p`, with a trailing sentinel of
/// `line_count`, so parsed line `p` occupies rows `row_of[p]..row_of[p+1]`.
///
/// A buffer row continues the previous parsed line iff the previous row's
/// ending is [`LineEnding::None`] — a display-chunk join. The final row of a
/// no-trailing-newline document also ends in None but has no follower, so it
/// never miscounts (same rule as the gutter's chunk map and minified
/// detection). Identity (plus sentinel) when no line is chunked.
///
/// Returns `None` when the buffer's real-line count differs from
/// `parsed_lines` — the buffer drifted from the text the ranges were parsed
/// from (mid-debounce edit) — or for empty buffers. Full-arm only; callers
/// check `is_rope` first.
fn fold_row_map(buffer: &Buffer, parsed_lines: usize) -> Option<Vec<usize>> {
    let count = buffer.line_count();
    if count == 0 {
        return None;
    }
    let mut row_of = Vec::with_capacity(parsed_lines + 1);
    row_of.push(0usize);
    for row in 0..count - 1 {
        if buffer.line_ending(row) != Some(LineEnding::None) {
            row_of.push(row + 1);
        }
    }
    if row_of.len() != parsed_lines {
        return None;
    }
    row_of.push(count);
    Some(row_of)
}

pub enum Tab {
    Editor(EditorTab),
    GitDiff(GitDiffTab),
}

impl Tab {
    pub fn title(&self) -> String {
        match self {
            Self::Editor(tab) => tab.title(),
            Self::GitDiff(tab) => tab.title.clone(),
        }
    }
}

pub struct GitDiffTab {
    pub title: String,
    pub diff: GitDiff,
}

pub struct EditorTab {
    pub path_opt: Option<PathBuf>,
    attrs: Attrs<'static>,
    pub editor: Mutex<ViEditor<'static, 'static>>,
    pub context_menu: Option<Point>,
    pub zoom_adj: i8,
    /// JSON support state; `Some` when this tab holds a `.json` document.
    pub json_view: Option<JsonViewState>,
}

impl EditorTab {
    pub fn new(config: &Config) -> Self {
        let attrs = crate::monospace_attrs();
        let zoom_adj = Default::default();
        let mut buffer = Buffer::new_empty(config.metrics(zoom_adj));
        // Set a minimal size before it is updated by draw
        buffer.set_size(Some(0.0), Some(0.0));
        buffer.set_text("", &attrs, Shaping::Advanced, None);

        let editor = SyntaxEditor::new(
            Arc::new(buffer),
            SYNTAX_SYSTEM.get().unwrap(),
            config.syntax_theme(),
        )
        .unwrap();

        let mut tab = Self {
            path_opt: None,
            attrs,
            editor: Mutex::new(ViEditor::new(editor)),
            context_menu: None,
            zoom_adj,
            json_view: None,
        };

        // Update any other config settings
        tab.set_config(config);

        tab
    }

    pub fn set_config(&mut self, config: &Config) {
        let mut editor = self.editor.lock().unwrap();
        let mut font_system = font_system().write().unwrap();
        let mut editor = editor.borrow_with(font_system.raw());
        editor.set_auto_indent(config.auto_indent);
        editor.set_passthrough(!config.vim_bindings);
        editor.set_tab_width(config.tab_width);
        editor.with_buffer_mut(|buffer| {
            buffer.set_wrap(if config.word_wrap {
                Wrap::WordOrGlyph
            } else {
                Wrap::None
            })
        });
        //TODO: dynamically discover light/dark changes
        editor.update_theme(config.syntax_theme());
    }

    pub fn open(&mut self, path: PathBuf) {
        let absolute = match fs::canonicalize(&path) {
            Ok(ok) => ok,
            Err(err) => match path::absolute(&path) {
                Ok(ok) => ok,
                Err(_) => {
                    log::error!("failed to canonicalize {:?}: {}", path, err);
                    path
                }
            },
        };
        let byte_len = fs::metadata(&absolute).map(|meta| meta.len()).unwrap_or(0);
        // Scoped so the editor lock is released before the minified-JSON
        // check below re-locks it.
        {
            let mut editor = self.editor.lock().unwrap();
            let mut font_system = font_system().write().unwrap();
            let mut editor = editor.borrow_with(font_system.raw());
            match editor.load_text(&absolute, self.attrs.clone()) {
                Ok(()) => {
                    log::info!("opened {:?}", absolute);
                    self.path_opt = Some(absolute);
                }
                Err(err) => {
                    if err.kind() == io::ErrorKind::NotFound {
                        log::warn!("opened non-existant file {:?}", absolute);
                        self.path_opt = Some(absolute);
                        editor.set_changed(true);
                    } else {
                        log::error!("failed to open {:?}: {}", absolute, err);
                        self.path_opt = None;
                    }
                }
            }
        }

        self.detect_minified_json(byte_len);
    }

    /// JSON support setup, run when a document is (re)loaded: builds the
    /// per-tab [`JsonViewState`] (the cached span AST behind the tree pane)
    /// for every `.json` document, and raises the one-time Format banner
    /// when the document is minified — essentially unlined (≤3 lines
    /// carrying >4KiB).
    ///
    /// `byte_len` is the total document size; the caller already knows it
    /// (file metadata at open) so it is not recomputed from the buffer.
    ///
    /// Known cost: the AST build walks the whole document once at load
    /// (`text()` is linear, the parse is node-budgeted).
    pub fn detect_minified_json(&mut self, byte_len: u64) {
        const MINIFIED_MIN_BYTES: u64 = 4096;
        const MINIFIED_MAX_LINES: usize = 3;

        if !self.is_json() {
            self.json_view = None;
            return;
        }
        let minified = {
            let editor = self.editor.lock().unwrap();
            editor.with_buffer(|buffer| {
                let count = buffer.line_count();
                if count <= MINIFIED_MAX_LINES && byte_len > MINIFIED_MIN_BYTES {
                    true
                } else if buffer.is_rope() {
                    // Rope lines split on real endings only — display
                    // chunking is set_text (Full arm) behavior — so skip
                    // the O(lines) cold ending walk.
                    false
                } else {
                    // Any chunk join: an ending of LineEnding::None with a
                    // line after it. The final line of a no-trailing-newline
                    // document also has ending None but no follower, so it
                    // does not trip this.
                    (0..count.saturating_sub(1))
                        .any(|i| buffer.line_ending(i) == Some(LineEnding::None))
                }
            })
        };
        self.json_view = Some(JsonViewState::from_text(&self.text(), minified));
    }

    /// Map an absolute byte offset to a buffer cursor by walking the
    /// buffer's own lines. This is the fallback for documents whose buffer
    /// lines do not correspond 1:1 to source lines — O(lines) and used for
    /// one-shot jumps only. Offsets landing inside a line ending clamp to
    /// the end of that line's text.
    pub fn cursor_for_byte_offset(&self, offset: usize) -> (usize, usize) {
        let editor = self.editor.lock().unwrap();
        editor.with_buffer(|buffer| {
            let mut start = 0;
            let count = buffer.line_count();
            for i in 0..count {
                let text_len = buffer.line_text_cow(i).map(|t| t.len()).unwrap_or(0);
                let ending_len = buffer.line_ending(i).map(|e| e.as_str().len()).unwrap_or(0);
                if offset < start + text_len + ending_len || i + 1 == count {
                    return (i, offset.saturating_sub(start).min(text_len));
                }
                start += text_len + ending_len;
            }
            (0, 0)
        })
    }

    /// The full document text (lines joined with their endings).
    pub fn text(&self) -> String {
        let editor = self.editor.lock().unwrap();
        editor_text(&editor)
    }

    /// Number of lines the buffer holds.
    pub fn total_line_count(&self) -> usize {
        let editor = self.editor.lock().unwrap();
        editor.with_buffer(|buffer| buffer.line_count())
    }

    /// Whether this tab is backed by a rope store (large file).
    pub fn uses_rope_buffer(&self) -> bool {
        let editor = self.editor.lock().unwrap();
        editor.with_buffer(|buffer| buffer.is_rope())
    }

    /// Set the cursor position (clamped to valid range).
    pub fn set_cursor(&mut self, line: usize, index: usize) {
        let mut editor = self.editor.lock().unwrap();
        let cursor = editor.with_buffer(|buffer| {
            let line = line.min(buffer.line_count().saturating_sub(1));
            let index = index.min(buffer.line_text_cow(line).map(|t| t.len()).unwrap_or(0));
            Cursor::new(line, index)
        });
        editor.set_cursor(cursor);
        editor.set_selection(Selection::None);
    }

    /// Rewrite the document through [`json_scan::pretty_print`] as ONE
    /// undoable edit: a single change record covering the select-all delete
    /// plus the insert of the formatted text, so a single undo restores the
    /// original bytes exactly. Malformed JSON is refused — `pretty_print`
    /// returns `None` and the buffer is left untouched.
    ///
    /// Returns true when the buffer was modified.
    pub fn format_json(&mut self) -> bool {
        let text = self.text();
        let Some(formatted) = json_scan::pretty_print(&text, JSON_FORMAT_INDENT) else {
            log::warn!(
                "json format: refused (parse error or pathological nesting), leaving the document untouched"
            );
            return false;
        };
        if formatted == text {
            // Already formatted: nothing to rewrite, nothing to undo. The
            // banner's offer is fulfilled either way.
            if let Some(json_view) = &mut self.json_view {
                json_view.banner = false;
            }
            return false;
        }
        {
            let mut editor = self.editor.lock().unwrap();

            // Store the entire operation as a single change for undo
            editor.start_change();

            // Grab everything in the buffer (same select-all shape as reload)
            let cursor_start = Cursor::new(0, 0);
            let cursor_end = editor.with_buffer(|buffer| {
                let last_line = buffer.line_count().saturating_sub(1);
                Cursor::new(
                    last_line,
                    buffer
                        .line_text_cow(last_line)
                        .map(|text| text.len())
                        .unwrap_or(0),
                )
            });
            editor.delete_range(cursor_start, cursor_end);
            editor.insert_at(cursor_start, &formatted, None);

            // Land at the top of the newly formatted document
            editor.set_cursor(cursor_start);
            editor.set_selection(Selection::None);

            editor.finish_change();
        }
        if let Some(json_view) = &mut self.json_view {
            json_view.banner = false;
            // The default fold level is applied by the caller AFTER it
            // rebuilds the view against the formatted text — the fold
            // ranges must describe the new line space, not this stale one.
        }
        true
    }

    /// Apply the tab's fold state ([`JsonViewState::fold`]) to the buffer's
    /// per-line hidden flags.
    ///
    /// Fold ranges live in *parsed-text* line space; buffer rows differ when
    /// display chunking split an over-long line into rows joined by
    /// [`LineEnding::None`]. The row map below sends parsed line `p` to its
    /// first buffer row; a folded range then hides buffer rows
    /// `[row_of[start+1], row_of[end+1])` — every row of every interior
    /// parsed line, chunk continuations included.
    ///
    /// Every row's flag is written on every sync (the mask is the full union
    /// over folded ranges), which is what makes fold/unfold idempotent,
    /// re-applies nested folds when an outer range unfolds, and clears
    /// orphaned flags after ranges change.
    ///
    /// Inert on rope tabs — the rope arm has no hidden storage and the
    /// fold UI is not offered there. When the buffer's real-line structure
    /// no longer matches the parsed line count (edits during the rebuild
    /// debounce), all flags clear instead: never hide rows the ranges don't
    /// describe. A cursor swallowed by a new fold lands at the end of the
    /// nearest visible row above, mirroring the hidden-line motion rule.
    pub fn sync_folds(&mut self) {
        let Some(view) = &self.json_view else {
            return;
        };
        let parsed_lines = view.aligned_line_count();
        let mask = view.fold.hidden_mask(parsed_lines);
        let mut editor = self.editor.lock().unwrap();
        let applied = editor.with_buffer_mut(|buffer| {
            if buffer.is_rope() {
                return false;
            }
            let count = buffer.line_count();
            match fold_row_map(buffer, parsed_lines) {
                Some(row_of) => {
                    for (parsed, hide) in mask.iter().enumerate() {
                        for row in row_of[parsed]..row_of[parsed + 1] {
                            buffer.set_line_hidden(row, *hide);
                        }
                    }
                    true
                }
                None => {
                    for row in 0..count {
                        buffer.set_line_hidden(row, false);
                    }
                    false
                }
            }
        });
        if applied {
            let cursor = editor.cursor();
            let clamp = editor.with_buffer(|buffer| {
                if !buffer.line_hidden(cursor.line) {
                    return None;
                }
                // Interiors start at start_line + 1, so row 0 is always
                // visible and this walk terminates.
                let mut line = cursor.line;
                while line > 0 && buffer.line_hidden(line) {
                    line -= 1;
                }
                let index = buffer.line_text_cow(line).map(|t| t.len()).unwrap_or(0);
                Some(Cursor::new(line, index))
            });
            if let Some(cursor) = clamp {
                editor.set_cursor(cursor);
            }
        }
    }

    /// Re-derive which ranges are folded from the buffer's hidden flags,
    /// then re-apply the mask. Run after [`JsonViewState::rebuild`]: the
    /// flags moved with their lines through the edit (they live on the
    /// `BufferLine`s), so they are the ground truth for what the user had
    /// folded, while the re-parsed ranges describe the new line space. A
    /// range is folded iff the first row of its interior is hidden; the
    /// follow-up sync clears any hidden rows no range explains anymore.
    pub fn resync_folds_from_buffer(&mut self) {
        let Some(view) = &self.json_view else {
            return;
        };
        let parsed_lines = view.aligned_line_count();
        let ranges = view.fold.ranges.clone();
        let folded = {
            let editor = self.editor.lock().unwrap();
            editor.with_buffer(|buffer| {
                if buffer.is_rope() {
                    return None;
                }
                let row_of = fold_row_map(buffer, parsed_lines)?;
                Some(
                    ranges
                        .iter()
                        .filter(|range| buffer.line_hidden(row_of[range.start_line as usize + 1]))
                        .map(|range| range.start_line)
                        .collect::<std::collections::HashSet<u32>>(),
                )
            })
        };
        if let Some(view) = self.json_view.as_mut() {
            view.fold.folded = folded.unwrap_or_default();
        }
        self.sync_folds();
    }

    /// Unfold whatever hides the cursor's row, then re-sync the hidden
    /// flags so the row is visible. Search (Find Next / Find Previous) and
    /// programmatic jumps set the cursor with no fold awareness; a match
    /// inside a folded region otherwise selects an invisible row — the
    /// cursor and highlight render nowhere and repeated Find Next looks
    /// dead. Same auto-unfold rule as a tree jump: ancestors open, sibling
    /// folds stay folded.
    ///
    /// O(1) when the cursor row is visible (the common case) or on rope
    /// tabs (nothing is ever hidden there). When the row map has drifted
    /// from the parsed line space, the sync alone still reveals: it clears
    /// every flag the ranges no longer describe.
    pub fn reveal_cursor_folds(&mut self) {
        let Some(view) = &self.json_view else {
            return;
        };
        let parsed_lines = view.aligned_line_count();
        let hidden_parsed = {
            let editor = self.editor.lock().unwrap();
            let row = editor.cursor().line;
            editor.with_buffer(|buffer| {
                if buffer.is_rope() || !buffer.line_hidden(row) {
                    return None;
                }
                // Same row map as sync_folds; chunk continuation rows map
                // back to the parsed line they belong to.
                Some(fold_row_map(buffer, parsed_lines).map(|row_of| {
                    (row_of.partition_point(|&first_row| first_row <= row) - 1) as u32
                }))
            })
        };
        let Some(parsed) = hidden_parsed else {
            return;
        };
        if let Some(line) = parsed
            && let Some(view) = self.json_view.as_mut()
        {
            view.fold.unfold_lines_containing(line);
        }
        self.sync_folds();
    }

    /// Check if this tab contains a JSON file.
    pub fn is_json(&self) -> bool {
        self.path_opt
            .as_ref()
            .and_then(|p| p.extension())
            .map(|ext| ext == "json")
            .unwrap_or(false)
    }

    pub fn reload(&mut self) {
        let mut editor = self.editor.lock().unwrap();
        let mut font_system = font_system().write().unwrap();
        let mut editor = editor.borrow_with(font_system.raw());
        if let Some(path) = &self.path_opt {
            // Save scroll
            let scroll = editor.with_buffer(|buffer| buffer.scroll());
            //TODO: save/restore more?

            match std::fs::read_to_string(path) {
                Ok(file_content) => {
                    log::info!("reloaded {:?}", path);

                    //TODO: compare using line iterator to prevent allocations
                    if file_content == editor_text(&editor) {
                        log::info!("text not changed");
                        return;
                    }

                    // Store the entire operation as a single change for undo
                    editor.start_change();

                    // Grab everything in the buffer
                    let cursor_start: Cursor = cosmic_text::Cursor::new(0, 0);
                    let cursor_end = editor.with_buffer(|buffer| {
                        let last_line = buffer.line_count().saturating_sub(1);
                        cosmic_text::Cursor::new(
                            last_line,
                            buffer
                                .line_text_cow(last_line)
                                .map(|text| text.len())
                                .unwrap_or(0),
                        )
                    });

                    // Replace everything in the buffer with the content from disk
                    editor.delete_range(cursor_start, cursor_end);
                    editor.insert_at(cursor_start, &file_content, None);

                    // Adjust cursor to closest position
                    let mut cursor = editor.cursor();
                    editor.with_buffer(|buffer| {
                        cursor.line = cursor.line.min(buffer.line_count().saturating_sub(1));
                        cursor.index = if let Some(text) = buffer.line_text_cow(cursor.line) {
                            let mut closest = text.len();
                            for (i, _) in text.char_indices().rev() {
                                if i >= cursor.index {
                                    closest = i;
                                } else {
                                    // i < cursor.index
                                    if cursor.index - i < closest - cursor.index {
                                        closest = i;
                                    }
                                    break;
                                }
                            }
                            closest
                        } else {
                            0
                        }
                    });
                    editor.set_cursor(cursor);

                    editor.finish_change();
                    editor.set_changed(false);
                }
                Err(err) => {
                    log::error!("failed to reload {:?}: {}", path, err);
                }
            }

            // Restore scroll
            editor.with_buffer_mut(|buffer| buffer.set_scroll(scroll));
        } else {
            log::warn!("tried to reload with no path");
        }
    }

    pub fn save(&mut self) {
        if let Some(path) = &self.path_opt {
            let mut editor = self.editor.lock().unwrap();
            let text = editor_text(&editor);
            match fs::write(path, &text) {
                Ok(()) => {
                    editor.save_point();
                    log::info!("saved {:?}", path);
                }
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::PermissionDenied {
                        log::warn!("Permission denied. Attempting to save with pkexec.");

                        if let Ok(mut output) = Command::new("pkexec")
                            .arg("tee")
                            .arg(path)
                            .stdin(Stdio::piped())
                            .stdout(Stdio::null()) // Redirect stdout to /dev/null
                            .stderr(Stdio::inherit()) // Retain stderr for error visibility
                            .spawn()
                        {
                            if let Some(mut stdin) = output.stdin.take() {
                                if let Err(e) = stdin.write_all(text.as_bytes()) {
                                    log::error!("Failed to write to stdin: {}", e);
                                }
                            } else {
                                log::error!("Failed to access stdin of pkexec process.");
                            }

                            // Ensure the child process is reaped
                            match output.wait() {
                                Ok(status) => {
                                    if status.success() {
                                        // Mark the editor's state as saved if the process succeeds
                                        editor.save_point();
                                        log::info!("File saved successfully with pkexec.");
                                    } else {
                                        log::error!(
                                            "pkexec process exited with a non-zero status: {:?}",
                                            status
                                        );
                                    }
                                }
                                Err(e) => {
                                    log::error!("Failed to wait on pkexec process: {}", e);
                                }
                            }
                        } else {
                            log::error!(
                                "Failed to spawn pkexec process. Check permissions or path."
                            );
                        }
                    }
                }
            }
        } else {
            log::warn!("tab has no path yet");
        }
    }

    pub fn changed(&self) -> bool {
        let editor = self.editor.lock().unwrap();
        editor.changed()
    }

    pub fn icon(&self, size: u16) -> icon::Icon {
        match &self.path_opt {
            Some(path) => icon::icon(mime_icon(mime_for_path(path, None, false), size)).size(size),
            None => icon::from_name(FALLBACK_MIME_ICON).size(size).icon(),
        }
    }

    pub fn title(&self) -> String {
        //TODO: show full title when there is a conflict
        if let Some(path) = &self.path_opt {
            match path.file_name() {
                Some(file_name_os) => match file_name_os.to_str() {
                    Some(file_name) => match file_name {
                        "mod.rs" => title_with_parent(path, file_name),
                        _ => file_name.to_string(),
                    },
                    None => format!("{}", path.display()),
                },
                None => format!("{}", path.display()),
            }
        } else {
            fl!("new-document")
        }
    }

    pub fn replace(&self, regex: &Regex, replace: &str, wrap_around: bool) -> bool {
        let mut editor = self.editor.lock().unwrap();
        let mut cursor = editor.cursor();
        let mut wrapped = false; // Keeps track of whether the search has wrapped around yet.
        let start_line = cursor.line;
        while cursor.line < editor.with_buffer(|buffer| buffer.line_count()) {
            if let Some((index, len)) = editor.with_buffer(|buffer| {
                let text = buffer
                    .line_text_cow(cursor.line)
                    .expect("cursor line in bounds");
                regex
                    .find_iter(&text)
                    .filter_map(|m| {
                        if cursor.line != start_line
                            || m.start() >= cursor.index
                            || m.start() < cursor.index && wrapped == true
                        {
                            Some((m.start(), m.len()))
                        } else {
                            None
                        }
                    })
                    .next()
            }) {
                cursor.index = index;
                let mut end = cursor;
                end.index = index + len;

                editor.start_change();
                // if index = 0 and len = 0, we are targeting and deleting an empty line
                // we'll move either cursor or end to delete the newline
                if index == 0 && len == 0 {
                    if cursor.line > 0 {
                        // move the cursor up one line
                        cursor.line -= 1;
                        cursor.index = editor.with_buffer(|buffer| {
                            buffer
                                .line_text_cow(cursor.line)
                                .expect("cursor line in bounds")
                                .len()
                        });
                    } else if cursor.line + 1 < editor.with_buffer(|buffer| buffer.line_count()) {
                        // move the end down one line
                        end.line += 1;
                        end.index = 0;
                    }
                }
                editor.delete_range(cursor, end);
                cursor = editor.insert_at(cursor, replace, None);
                editor.set_cursor(cursor);
                // Need to disable selection to prevent the new cursor showing selection to old location
                editor.set_selection(Selection::None);
                editor.finish_change();
                return true;
            }

            cursor.line += 1;

            // If we haven't wrapped yet and we've reached the last line, reset cursor line to 0 and
            // set wrapped to true so we don't wrap again
            if wrap_around
                && !wrapped
                && cursor.line == editor.with_buffer(|buffer| buffer.line_count())
            {
                cursor.line = 0;
                wrapped = true;
            }
        }
        false
    }

    pub fn zoom_adj(&self) -> i8 {
        self.zoom_adj
    }

    pub fn set_zoom_adj(&mut self, value: i8) {
        self.zoom_adj = value;
    }

    // Code adapted from cosmic-text ViEditor search
    pub fn search(&self, regex: &Regex, forwards: bool, wrap_around: bool) -> bool {
        let mut editor = self.editor.lock().unwrap();
        let mut cursor = editor.cursor();
        let mut wrapped = false; // Keeps track of whether the search has wrapped around yet.
        let start_line = cursor.line;
        let current_selection = editor.selection();

        if forwards {
            while cursor.line < editor.with_buffer(|buffer| buffer.line_count()) {
                if let Some((start, end)) = editor.with_buffer(|buffer| {
                    let text = buffer
                        .line_text_cow(cursor.line)
                        .expect("cursor line in bounds");
                    regex
                        .find_iter(&text)
                        .filter_map(|m| {
                            if cursor.line != start_line
                                || m.start() > cursor.index
                                || m.start() == cursor.index && current_selection == Selection::None
                                || m.start() < cursor.index && wrapped == true
                            {
                                Some((m.start(), m.end()))
                            } else {
                                None
                            }
                        })
                        .next()
                }) {
                    cursor.index = start;
                    editor.set_cursor(cursor);

                    // Highlight searched text
                    let selection = Selection::Normal(Cursor::new(cursor.line, end));
                    editor.set_selection(selection);

                    return true;
                }

                cursor.line += 1;

                // If we haven't wrapped yet and we've reached the last line, reset cursor line to 0 and
                // set wrapped to true so we don't wrap again
                if wrap_around
                    && !wrapped
                    && cursor.line == editor.with_buffer(|buffer| buffer.line_count())
                {
                    cursor.line = 0;
                    wrapped = true;
                }
            }
        } else {
            cursor.line += 1;
            while cursor.line > 0 {
                cursor.line -= 1;

                if let Some((start, end)) = editor.with_buffer(|buffer| {
                    let text = buffer
                        .line_text_cow(cursor.line)
                        .expect("cursor line in bounds");
                    regex
                        .find_iter(&text)
                        .filter_map(|m| {
                            if cursor.line != start_line
                                || m.start() < cursor.index
                                || m.start() == cursor.index && current_selection == Selection::None
                                || m.start() > cursor.index && wrapped == true
                            {
                                Some((m.start(), m.end()))
                            } else {
                                None
                            }
                        })
                        .last()
                }) {
                    cursor.index = start;
                    editor.set_cursor(cursor);

                    // Highlight searched text
                    let selection = Selection::Normal(Cursor::new(cursor.line, end));
                    editor.set_selection(selection);

                    return true;
                }

                // If we haven't wrapped yet and we've reached the first line, reset cursor line to the
                // last line and set wrapped to true so we don't wrap again
                if wrap_around && !wrapped && cursor.line == 0 {
                    cursor.line = editor.with_buffer(|buffer| buffer.line_count());
                    wrapped = true;
                }
            }
        }
        false
    }
}

/// Includes parent name in tab title
///
/// Useful for distinguishing between Rust modules named `mod.rs`
fn title_with_parent(path: &std::path::Path, file_name: &str) -> String {
    let parent_name = path
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|os_str| os_str.to_str());

    match parent_name {
        Some(parent) => [parent, "/", file_name].concat(),
        None => file_name.to_string(),
    }
}
