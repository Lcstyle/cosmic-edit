// SPDX-License-Identifier: GPL-3.0-only

//! Error-tolerant JSON token scanner and fold-range extraction.
//!
//! Ported from VS Code's `jsonc-parser` scanner and the
//! `jsonFolding.ts` brace/bracket stack algorithm in
//! `microsoft/vscode-json-languageservice`. Pure functions over `&str`:
//! no UI imports, no allocation beyond string unescaping.
//!
//! Line numbers are 0-based and match cosmic-text's `LineIter` splitting:
//! `\r\n` and `\n\r` each count as a single line break, as do lone `\n`
//! and `\r`, so fold ranges line up with buffer line indices.

/// A single JSON token produced by [`scan`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JsonToken {
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Colon,
    Comma,
    String { unescaped: String },
    Number,
    True,
    False,
    Null,
    Unknown,
}

/// A token with its byte span and starting line in the source text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spanned<T> {
    pub token: T,
    /// Byte offset of the token start in the scanned text.
    pub offset: usize,
    /// Byte length of the token.
    pub len: usize,
    /// 0-based line of the token start.
    pub line: u32,
}

/// What kind of container a fold range covers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FoldKind {
    Object,
    Array,
}

/// A foldable region: hiding lines `start_line + 1 ..= end_line` collapses it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FoldRange {
    pub start_line: u32,
    pub end_line: u32,
    pub kind: FoldKind,
}

/// Tokenize `text`, tolerating any malformed input without panicking.
///
/// Strings handle `\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t` and
/// `\uXXXX` escapes (surrogate pairs combined; lone surrogates become
/// U+FFFD; invalid escapes are dropped, matching VS Code). A raw line
/// break or EOF terminates an unclosed string.
pub fn scan(text: &str) -> impl Iterator<Item = Spanned<JsonToken>> + '_ {
    Scanner { text, pos: 0, line: 0 }
}

struct Scanner<'a> {
    text: &'a str,
    pos: usize,
    line: u32,
}

impl Scanner<'_> {
    fn bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }

    /// Skip spaces, tabs and line breaks, counting lines the way
    /// cosmic-text's `LineIter` does: "\r\n" and "\n\r" are single breaks.
    fn skip_whitespace(&mut self) {
        while self.pos < self.text.len() {
            match self.bytes()[self.pos] {
                b' ' | b'\t' => self.pos += 1,
                b'\n' => {
                    self.pos += 1;
                    if self.bytes().get(self.pos) == Some(&b'\r') {
                        self.pos += 1;
                    }
                    self.line += 1;
                }
                b'\r' => {
                    self.pos += 1;
                    if self.bytes().get(self.pos) == Some(&b'\n') {
                        self.pos += 1;
                    }
                    self.line += 1;
                }
                _ => {
                    // Tolerate a byte-order mark anywhere whitespace is legal.
                    if self.text[self.pos..].starts_with('\u{feff}') {
                        self.pos += '\u{feff}'.len_utf8();
                    } else {
                        break;
                    }
                }
            }
        }
    }

    /// Read exactly four hex digits, or leave `pos` untouched and return None.
    fn scan_hex4(&mut self) -> Option<u32> {
        if self.pos + 4 > self.text.len() {
            return None;
        }
        let mut value = 0;
        for i in 0..4 {
            value = value * 16 + (self.bytes()[self.pos + i] as char).to_digit(16)?;
        }
        self.pos += 4;
        Some(value)
    }

    /// `pos` is on the opening quote. Ends at the closing quote, EOF, or a
    /// raw line break (which is not consumed), whichever comes first.
    fn scan_string(&mut self) -> JsonToken {
        self.pos += 1;
        let mut unescaped = String::new();
        let mut run_start = self.pos;
        loop {
            if self.pos >= self.text.len() {
                unescaped.push_str(&self.text[run_start..self.pos]);
                break;
            }
            match self.bytes()[self.pos] {
                b'"' => {
                    unescaped.push_str(&self.text[run_start..self.pos]);
                    self.pos += 1;
                    break;
                }
                b'\n' | b'\r' => {
                    unescaped.push_str(&self.text[run_start..self.pos]);
                    break;
                }
                b'\\' => {
                    unescaped.push_str(&self.text[run_start..self.pos]);
                    self.pos += 1;
                    if self.pos >= self.text.len() {
                        break;
                    }
                    let escape = self.bytes()[self.pos];
                    self.pos += 1;
                    match escape {
                        b'"' => unescaped.push('"'),
                        b'\\' => unescaped.push('\\'),
                        b'/' => unescaped.push('/'),
                        b'b' => unescaped.push('\u{0008}'),
                        b'f' => unescaped.push('\u{000C}'),
                        b'n' => unescaped.push('\n'),
                        b'r' => unescaped.push('\r'),
                        b't' => unescaped.push('\t'),
                        b'u' => {
                            if let Some(unit) = self.scan_hex4() {
                                unescaped.push(self.finish_unicode_escape(unit));
                            }
                            // Invalid \u escapes contribute nothing (VS Code).
                        }
                        // Other invalid escapes are dropped (VS Code).
                        _ => {}
                    }
                    run_start = self.pos;
                }
                _ => self.pos += 1,
            }
        }
        JsonToken::String { unescaped }
    }

    /// Turn a decoded UTF-16 unit into a char, combining a high surrogate
    /// with a following escaped low surrogate; lone surrogates degrade to
    /// U+FFFD.
    fn finish_unicode_escape(&mut self, unit: u32) -> char {
        if (0xD800..=0xDBFF).contains(&unit) {
            let saved = self.pos;
            if self.bytes().get(self.pos) == Some(&b'\\')
                && self.bytes().get(self.pos + 1) == Some(&b'u')
            {
                self.pos += 2;
                if let Some(low) = self.scan_hex4() {
                    if (0xDC00..=0xDFFF).contains(&low) {
                        let combined = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                        if let Some(ch) = char::from_u32(combined) {
                            return ch;
                        }
                    }
                }
                self.pos = saved;
            }
            '\u{FFFD}'
        } else {
            char::from_u32(unit).unwrap_or('\u{FFFD}')
        }
    }

    /// Maximal munch of one JSON number; malformed tails are left behind.
    fn scan_number(&mut self) -> JsonToken {
        if self.bytes()[self.pos] == b'-' {
            self.pos += 1;
        }
        self.skip_digits();
        if self.bytes().get(self.pos) == Some(&b'.') {
            self.pos += 1;
            self.skip_digits();
        }
        if matches!(self.bytes().get(self.pos), Some(b'e' | b'E')) {
            let mut ahead = self.pos + 1;
            if matches!(self.bytes().get(ahead), Some(b'+' | b'-')) {
                ahead += 1;
            }
            if matches!(self.bytes().get(ahead), Some(digit) if digit.is_ascii_digit()) {
                self.pos = ahead;
                self.skip_digits();
            }
        }
        JsonToken::Number
    }

    fn skip_digits(&mut self) {
        while self.pos < self.text.len() && self.bytes()[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
    }

    fn scan_word(&mut self) -> JsonToken {
        let start = self.pos;
        while self.pos < self.text.len()
            && (self.bytes()[self.pos].is_ascii_alphanumeric() || self.bytes()[self.pos] == b'_')
        {
            self.pos += 1;
        }
        match &self.text[start..self.pos] {
            "true" => JsonToken::True,
            "false" => JsonToken::False,
            "null" => JsonToken::Null,
            _ => JsonToken::Unknown,
        }
    }
}

impl Iterator for Scanner<'_> {
    type Item = Spanned<JsonToken>;

    fn next(&mut self) -> Option<Self::Item> {
        self.skip_whitespace();
        if self.pos >= self.text.len() {
            return None;
        }
        let offset = self.pos;
        let line = self.line;
        let token = match self.bytes()[self.pos] {
            b'{' => {
                self.pos += 1;
                JsonToken::OpenBrace
            }
            b'}' => {
                self.pos += 1;
                JsonToken::CloseBrace
            }
            b'[' => {
                self.pos += 1;
                JsonToken::OpenBracket
            }
            b']' => {
                self.pos += 1;
                JsonToken::CloseBracket
            }
            b':' => {
                self.pos += 1;
                JsonToken::Colon
            }
            b',' => {
                self.pos += 1;
                JsonToken::Comma
            }
            b'"' => self.scan_string(),
            b'-' | b'0'..=b'9' => self.scan_number(),
            b'a'..=b'z' | b'A'..=b'Z' => self.scan_word(),
            _ => {
                // Consume one whole char so multi-byte junk stays one token.
                let ch_len = self.text[self.pos..]
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8);
                self.pos += ch_len;
                JsonToken::Unknown
            }
        };
        Some(Spanned {
            token,
            offset,
            len: self.pos - offset,
            line,
        })
    }
}

/// VS Code's algorithm: brace/bracket stack over the token stream; a range is
/// emitted when close is >=2 lines below open; `end_line` = close_line - 1;
/// ranges sharing a start line are deduped (prevStart check). Unclosed or
/// mismatched braces simply produce no range. Result is sorted by
/// `start_line`, outermost first on ties.
pub fn fold_ranges(text: &str) -> Vec<FoldRange> {
    struct Open {
        start_line: u32,
        kind: FoldKind,
    }
    let mut stack: Vec<Open> = Vec::new();
    let mut ranges = Vec::new();
    let mut prev_start = None;
    for spanned in scan(text) {
        let kind = match spanned.token {
            JsonToken::OpenBrace => {
                stack.push(Open {
                    start_line: spanned.line,
                    kind: FoldKind::Object,
                });
                continue;
            }
            JsonToken::OpenBracket => {
                stack.push(Open {
                    start_line: spanned.line,
                    kind: FoldKind::Array,
                });
                continue;
            }
            JsonToken::CloseBrace => FoldKind::Object,
            JsonToken::CloseBracket => FoldKind::Array,
            _ => continue,
        };
        if stack.last().is_some_and(|open| open.kind == kind) {
            let open = stack.pop().expect("just checked non-empty");
            if spanned.line > open.start_line + 1 && prev_start != Some(open.start_line) {
                ranges.push(FoldRange {
                    start_line: open.start_line,
                    end_line: spanned.line - 1,
                    kind,
                });
                prev_start = Some(open.start_line);
            }
        }
    }
    ranges.sort_unstable_by_key(|range| (range.start_line, std::cmp::Reverse(range.end_line)));
    ranges
}

/// Default cap on materialized [`JsonNode`]s: keeps a 12MB export from
/// ballooning the tree pane. Callers may pass any other budget to
/// [`parse_tree`].
pub const DEFAULT_NODE_BUDGET: usize = 100_000;

/// Containers nested deeper than this are skipped (counted into
/// [`JsonTree::truncated`]) so descent recursion cannot overflow the stack
/// on adversarial input like `[[[[…`.
const MAX_DEPTH: usize = 200;

/// Character cap for scalar previews shown in the tree pane.
const PREVIEW_MAX_CHARS: usize = 40;

/// The JSON type of a node; containers carry their source entry count,
/// which stays accurate even when children were dropped by the budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonKind {
    Object { len: usize },
    Array { len: usize },
    Str,
    Num,
    Bool,
    Null,
}

/// One value in the span AST. `offset../len` is the byte span of the whole
/// value in the source (quotes and braces included), so copy-value is
/// `&text[offset..offset + len]`; `line..=end_line` matches buffer lines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonNode {
    /// Unescaped member key; `None` for the root and array items.
    pub key: Option<String>,
    /// `Some(i)` for array items.
    pub index: Option<usize>,
    pub kind: JsonKind,
    /// Truncated scalar value, or `{13}` / `[47]` for containers.
    pub preview: String,
    pub offset: usize,
    pub len: usize,
    pub line: u32,
    pub end_line: u32,
    pub children: Vec<JsonNode>,
}

/// Result of [`parse_tree`]: `root` is `None` only when no value could be
/// found at all; `truncated` counts values dropped by the node budget or
/// the depth cap (the tree pane renders them as one "… N more" row).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonTree {
    pub root: Option<JsonNode>,
    pub truncated: usize,
}

/// Build a span-preserving AST over [`scan`], tolerating malformed input:
/// an unexpected token closes the current container and descent continues
/// in the parent; unclosed containers close at EOF. At most `node_budget`
/// nodes are materialized — further values are skipped and counted into
/// [`JsonTree::truncated`].
pub fn parse_tree(text: &str, node_budget: usize) -> JsonTree {
    let mut parser = TreeParser {
        text,
        tokens: Scanner { text, pos: 0, line: 0 }.peekable(),
        nodes: 0,
        budget: node_budget,
        truncated: 0,
        last_end: 0,
        last_line: 0,
    };
    // Tolerate leading junk: the root is the first value-looking token.
    while let Some(spanned) = parser.tokens.peek() {
        if is_value_start(&spanned.token) {
            break;
        }
        parser.next_token();
    }
    let root = match parser.value(None, None, 0) {
        Parsed::Node(node) => Some(node),
        Parsed::Skipped | Parsed::Absent => None,
    };
    JsonTree {
        root,
        truncated: parser.truncated,
    }
}

/// Display path for a chain of nodes from the root, e.g. `db.posts[3].slug`.
/// Keys that are not identifier-like are bracket-quoted: `["weird key"]`.
pub fn json_path(ancestors: &[&JsonNode]) -> String {
    let mut path = String::new();
    for node in ancestors {
        if let Some(index) = node.index {
            path.push_str(&format!("[{index}]"));
        } else if let Some(key) = node.key.as_deref() {
            if is_path_ident(key) {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(key);
            } else {
                path.push_str("[\"");
                for ch in key.chars() {
                    match ch {
                        '"' => path.push_str("\\\""),
                        '\\' => path.push_str("\\\\"),
                        ch if ch.is_control() => {
                            path.push_str(&format!("\\u{:04x}", ch as u32))
                        }
                        ch => path.push(ch),
                    }
                }
                path.push_str("\"]");
            }
        }
        // The root (no key, no index) contributes nothing.
    }
    path
}

fn is_path_ident(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
        && chars.all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$')
}

/// Re-layout `text` with one member per line at `indent` per level.
/// Tokens are emitted byte-for-byte from the source — key order, duplicate
/// keys, escapes and number spellings are all preserved exactly; only the
/// whitespace between tokens changes. Empty containers stay `{}` / `[]`,
/// output line endings are `\n`, no trailing newline.
///
/// Returns `None` for anything that does not parse cleanly as a single
/// JSON value — formatting must never drop or invent content, so a
/// malformed document is left untouched rather than "repaired".
pub fn pretty_print(text: &str, indent: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len() + text.len() / 3);
    let mut stack: Vec<FoldKind> = Vec::new();
    let mut expect = PrintExpect::Value {
        own_line: false,
        empty_array_ok: false,
    };
    for spanned in scan(text) {
        let raw = &text[spanned.offset..spanned.offset + spanned.len];
        expect = match expect {
            PrintExpect::Value {
                own_line,
                empty_array_ok,
            } => match &spanned.token {
                JsonToken::OpenBrace | JsonToken::OpenBracket => {
                    if own_line {
                        print_line(&mut out, indent, stack.len());
                    }
                    out.push_str(raw);
                    if spanned.token == JsonToken::OpenBrace {
                        stack.push(FoldKind::Object);
                        PrintExpect::KeyOrClose
                    } else {
                        stack.push(FoldKind::Array);
                        PrintExpect::Value {
                            own_line: true,
                            empty_array_ok: true,
                        }
                    }
                }
                JsonToken::CloseBracket if empty_array_ok => {
                    stack.pop();
                    out.push(']');
                    print_after_value(&stack)
                }
                JsonToken::String { .. }
                | JsonToken::Number
                | JsonToken::True
                | JsonToken::False
                | JsonToken::Null => {
                    if own_line {
                        print_line(&mut out, indent, stack.len());
                    }
                    out.push_str(raw);
                    print_after_value(&stack)
                }
                _ => return None,
            },
            PrintExpect::KeyOrClose => match &spanned.token {
                JsonToken::String { .. } => {
                    print_line(&mut out, indent, stack.len());
                    out.push_str(raw);
                    PrintExpect::Colon
                }
                JsonToken::CloseBrace => {
                    stack.pop();
                    out.push('}');
                    print_after_value(&stack)
                }
                _ => return None,
            },
            PrintExpect::Key => match &spanned.token {
                JsonToken::String { .. } => {
                    print_line(&mut out, indent, stack.len());
                    out.push_str(raw);
                    PrintExpect::Colon
                }
                _ => return None,
            },
            PrintExpect::Colon => match &spanned.token {
                JsonToken::Colon => {
                    out.push_str(": ");
                    PrintExpect::Value {
                        own_line: false,
                        empty_array_ok: false,
                    }
                }
                _ => return None,
            },
            PrintExpect::CommaOrClose => match &spanned.token {
                JsonToken::Comma => {
                    out.push(',');
                    match stack.last() {
                        Some(FoldKind::Object) => PrintExpect::Key,
                        Some(FoldKind::Array) => PrintExpect::Value {
                            own_line: true,
                            empty_array_ok: false,
                        },
                        None => return None,
                    }
                }
                JsonToken::CloseBrace if stack.last() == Some(&FoldKind::Object) => {
                    stack.pop();
                    print_line(&mut out, indent, stack.len());
                    out.push('}');
                    print_after_value(&stack)
                }
                JsonToken::CloseBracket if stack.last() == Some(&FoldKind::Array) => {
                    stack.pop();
                    print_line(&mut out, indent, stack.len());
                    out.push(']');
                    print_after_value(&stack)
                }
                _ => return None,
            },
            PrintExpect::End => return None,
        };
    }
    matches!(expect, PrintExpect::End).then_some(out)
}

enum PrintExpect {
    Value { own_line: bool, empty_array_ok: bool },
    KeyOrClose,
    Key,
    Colon,
    CommaOrClose,
    End,
}

fn print_after_value(stack: &[FoldKind]) -> PrintExpect {
    if stack.is_empty() {
        PrintExpect::End
    } else {
        PrintExpect::CommaOrClose
    }
}

fn print_line(out: &mut String, indent: &str, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str(indent);
    }
}

fn is_value_start(token: &JsonToken) -> bool {
    matches!(
        token,
        JsonToken::OpenBrace
            | JsonToken::OpenBracket
            | JsonToken::String { .. }
            | JsonToken::Number
            | JsonToken::True
            | JsonToken::False
            | JsonToken::Null
    )
}

fn string_preview(unescaped: &str) -> String {
    let mut preview = String::new();
    let mut chars = unescaped.chars();
    for ch in chars.by_ref().take(PREVIEW_MAX_CHARS) {
        // Rows are single-line: flatten control characters.
        preview.push(if ch.is_control() { ' ' } else { ch });
    }
    if chars.next().is_some() {
        preview.push('…');
    }
    preview
}

enum Parsed {
    Node(JsonNode),
    /// A value was consumed and counted as truncated (budget or depth cap).
    Skipped,
    /// The next token cannot start a value; nothing was consumed.
    Absent,
}

struct TreeParser<'a> {
    text: &'a str,
    tokens: std::iter::Peekable<Scanner<'a>>,
    nodes: usize,
    budget: usize,
    truncated: usize,
    /// Byte end and line of the last consumed token, for closing container
    /// spans at EOF or on an unexpected token.
    last_end: usize,
    last_line: u32,
}

impl TreeParser<'_> {
    fn next_token(&mut self) -> Option<Spanned<JsonToken>> {
        let spanned = self.tokens.next()?;
        self.last_end = spanned.offset + spanned.len;
        self.last_line = spanned.line;
        Some(spanned)
    }

    fn eat(&mut self, token: &JsonToken) -> bool {
        if self.tokens.peek().is_some_and(|next| next.token == *token) {
            self.next_token();
            true
        } else {
            false
        }
    }

    /// After an entry: `,` continues, the matching close (or EOF) is left
    /// for the container loop, anything else closes the container here.
    fn entry_separator_ok(&mut self, close: &JsonToken) -> bool {
        match self.tokens.peek() {
            Some(next) if next.token == JsonToken::Comma => {
                self.next_token();
                true
            }
            Some(next) if next.token == *close => true,
            None => true,
            Some(_) => false,
        }
    }

    fn value(&mut self, key: Option<String>, index: Option<usize>, depth: usize) -> Parsed {
        match self.tokens.peek() {
            Some(spanned) if is_value_start(&spanned.token) => {}
            _ => return Parsed::Absent,
        }
        if self.nodes >= self.budget || depth > MAX_DEPTH {
            self.skip_value();
            return Parsed::Skipped;
        }
        self.nodes += 1;
        let spanned = self.next_token().expect("peeked a value start");
        let (offset, len, line) = (spanned.offset, spanned.len, spanned.line);
        let scalar = |kind, preview| JsonNode {
            key: None,
            index: None,
            kind,
            preview,
            offset,
            len,
            line,
            end_line: line,
            children: Vec::new(),
        };
        match spanned.token {
            JsonToken::OpenBrace => Parsed::Node(self.object(offset, line, key, index, depth)),
            JsonToken::OpenBracket => Parsed::Node(self.array(offset, line, key, index, depth)),
            JsonToken::String { unescaped } => Parsed::Node(JsonNode {
                key,
                index,
                ..scalar(JsonKind::Str, string_preview(&unescaped))
            }),
            token => {
                let kind = match token {
                    JsonToken::Number => JsonKind::Num,
                    JsonToken::True | JsonToken::False => JsonKind::Bool,
                    _ => JsonKind::Null,
                };
                Parsed::Node(JsonNode {
                    key,
                    index,
                    ..scalar(kind, self.text[offset..offset + len].to_string())
                })
            }
        }
    }

    /// The `{` at `open_offset` has been consumed.
    fn object(
        &mut self,
        open_offset: usize,
        open_line: u32,
        key: Option<String>,
        index: Option<usize>,
        depth: usize,
    ) -> JsonNode {
        let mut children = Vec::new();
        let mut entries = 0;
        let (end_offset, end_line) = loop {
            match self.tokens.peek() {
                None => break (self.last_end, self.last_line),
                Some(next) if next.token == JsonToken::CloseBrace => {
                    let close = self.next_token().expect("peeked");
                    break (close.offset + close.len, close.line);
                }
                Some(next) if matches!(next.token, JsonToken::String { .. }) => {
                    let key_token = self.next_token().expect("peeked");
                    let JsonToken::String { unescaped: child_key } = key_token.token else {
                        unreachable!("peeked a string");
                    };
                    if !self.eat(&JsonToken::Colon) {
                        break (self.last_end, self.last_line);
                    }
                    match self.value(Some(child_key), None, depth + 1) {
                        Parsed::Node(node) => {
                            children.push(node);
                            entries += 1;
                        }
                        Parsed::Skipped => entries += 1,
                        Parsed::Absent => break (self.last_end, self.last_line),
                    }
                    if !self.entry_separator_ok(&JsonToken::CloseBrace) {
                        break (self.last_end, self.last_line);
                    }
                }
                Some(_) => break (self.last_end, self.last_line),
            }
        };
        JsonNode {
            key,
            index,
            kind: JsonKind::Object { len: entries },
            preview: format!("{{{entries}}}"),
            offset: open_offset,
            len: end_offset - open_offset,
            line: open_line,
            end_line,
            children,
        }
    }

    /// The `[` at `open_offset` has been consumed.
    fn array(
        &mut self,
        open_offset: usize,
        open_line: u32,
        key: Option<String>,
        index: Option<usize>,
        depth: usize,
    ) -> JsonNode {
        let mut children = Vec::new();
        let mut entries = 0;
        let (end_offset, end_line) = loop {
            match self.tokens.peek() {
                None => break (self.last_end, self.last_line),
                Some(next) if next.token == JsonToken::CloseBracket => {
                    let close = self.next_token().expect("peeked");
                    break (close.offset + close.len, close.line);
                }
                Some(_) => {
                    match self.value(None, Some(entries), depth + 1) {
                        Parsed::Node(node) => {
                            children.push(node);
                            entries += 1;
                        }
                        Parsed::Skipped => entries += 1,
                        Parsed::Absent => break (self.last_end, self.last_line),
                    }
                    if !self.entry_separator_ok(&JsonToken::CloseBracket) {
                        break (self.last_end, self.last_line);
                    }
                }
            }
        };
        JsonNode {
            key,
            index,
            kind: JsonKind::Array { len: entries },
            preview: format!("[{entries}]"),
            offset: open_offset,
            len: end_offset - open_offset,
            line: open_line,
            end_line,
            children,
        }
    }

    /// Consume one whole value without building nodes, counting every value
    /// it contains into `truncated` (object keys are not values). Iterative:
    /// this is the escape hatch for both the node budget and the depth cap.
    /// Precondition: the next token starts a value.
    fn skip_value(&mut self) {
        let mut depth = 0usize;
        loop {
            let Some(next) = self.tokens.peek() else {
                return;
            };
            match &next.token {
                JsonToken::OpenBrace | JsonToken::OpenBracket => {
                    self.next_token();
                    self.truncated += 1;
                    depth += 1;
                }
                JsonToken::CloseBrace | JsonToken::CloseBracket => {
                    if depth == 0 {
                        // Unmatched close belongs to an ancestor.
                        return;
                    }
                    self.next_token();
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                JsonToken::String { .. } => {
                    self.next_token();
                    let is_key = depth > 0
                        && self
                            .tokens
                            .peek()
                            .is_some_and(|next| next.token == JsonToken::Colon);
                    if !is_key {
                        self.truncated += 1;
                    }
                    if depth == 0 {
                        return;
                    }
                }
                JsonToken::Number | JsonToken::True | JsonToken::False | JsonToken::Null => {
                    self.next_token();
                    self.truncated += 1;
                    if depth == 0 {
                        return;
                    }
                }
                JsonToken::Colon | JsonToken::Comma | JsonToken::Unknown => {
                    if depth == 0 {
                        return;
                    }
                    self.next_token();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn sp(token: JsonToken, offset: usize, len: usize, line: u32) -> Spanned<JsonToken> {
        Spanned {
            token,
            offset,
            len,
            line,
        }
    }

    fn fr(start_line: u32, end_line: u32, kind: FoldKind) -> FoldRange {
        FoldRange {
            start_line,
            end_line,
            kind,
        }
    }

    fn string_token(unescaped: &str) -> JsonToken {
        JsonToken::String {
            unescaped: unescaped.to_string(),
        }
    }

    #[test]
    fn scan_punctuation_offsets_lines() {
        let tokens: Vec<_> = scan("{\n [\n ]\n}").collect();
        assert_eq!(
            tokens,
            vec![
                sp(JsonToken::OpenBrace, 0, 1, 0),
                sp(JsonToken::OpenBracket, 3, 1, 1),
                sp(JsonToken::CloseBracket, 6, 1, 2),
                sp(JsonToken::CloseBrace, 8, 1, 3),
            ]
        );
    }

    #[test]
    fn scan_string_escapes() {
        let tokens: Vec<_> = scan(r#""a\"b\\c\u0041\n""#).collect();
        assert_eq!(tokens, vec![sp(string_token("a\"b\\cA\n"), 0, 17, 0)]);
    }

    #[test]
    fn scan_string_surrogate_pair() {
        let tokens: Vec<_> = scan(r#""\uD83D\uDE00""#).collect();
        assert_eq!(tokens, vec![sp(string_token("\u{1F600}"), 0, 14, 0)]);

        // A lone high surrogate degrades to U+FFFD instead of panicking.
        let tokens: Vec<_> = scan(r#""\uD83D""#).collect();
        assert_eq!(tokens, vec![sp(string_token("\u{FFFD}"), 0, 8, 0)]);
    }

    #[test]
    fn scan_number_forms() {
        let tokens: Vec<_> = scan("-12.5e+3 0 42.0 1e9").collect();
        assert_eq!(
            tokens,
            vec![
                sp(JsonToken::Number, 0, 8, 0),
                sp(JsonToken::Number, 9, 1, 0),
                sp(JsonToken::Number, 11, 4, 0),
                sp(JsonToken::Number, 16, 3, 0),
            ]
        );
    }

    #[test]
    fn scan_literals_and_unknown() {
        let tokens: Vec<_> = scan("true false null nulla @").collect();
        assert_eq!(
            tokens,
            vec![
                sp(JsonToken::True, 0, 4, 0),
                sp(JsonToken::False, 5, 5, 0),
                sp(JsonToken::Null, 11, 4, 0),
                sp(JsonToken::Unknown, 16, 5, 0),
                sp(JsonToken::Unknown, 22, 1, 0),
            ]
        );
    }

    #[test]
    fn scan_unterminated_string_stops_at_line_break() {
        let tokens: Vec<_> = scan("{\n\"abc\ntrue}").collect();
        assert_eq!(
            tokens,
            vec![
                sp(JsonToken::OpenBrace, 0, 1, 0),
                sp(string_token("abc"), 2, 4, 1),
                sp(JsonToken::True, 7, 4, 2),
                sp(JsonToken::CloseBrace, 11, 1, 2),
            ]
        );
    }

    const NESTED: &str = "{\n  \"a\": {\n    \"b\": [\n      1,\n      2\n    ],\n    \"c\": 3\n  }\n}";

    #[test]
    fn fold_nested_object_and_array() {
        assert_eq!(
            fold_ranges(NESTED),
            vec![
                fr(0, 7, FoldKind::Object),
                fr(1, 6, FoldKind::Object),
                fr(2, 4, FoldKind::Array),
            ]
        );
    }

    #[test]
    fn fold_same_line_braces_no_range() {
        assert_eq!(fold_ranges("{ \"a\": {}, \"b\": [] }"), vec![]);
        // Close directly below open leaves nothing to hide: no range.
        assert_eq!(fold_ranges("{\n}"), vec![]);
    }

    #[test]
    fn fold_unclosed_brace_tolerated() {
        // Outer object never closes; the closed inner object still folds.
        assert_eq!(
            fold_ranges("{\n  \"a\": {\n    \"b\": 1\n  }\n"),
            vec![fr(1, 2, FoldKind::Object)]
        );
        // Nothing closes: no ranges, no panic.
        assert_eq!(fold_ranges("{\n\"a\": [1,\n2\n"), vec![]);
    }

    #[test]
    fn fold_mismatched_close_tolerated() {
        // `]` cannot close `{`: ignored, no panic, no bogus range.
        assert_eq!(fold_ranges("[\n  {\n    \"a\": 1\n]\n"), vec![]);
        // Close tokens with an empty stack are ignored.
        assert_eq!(fold_ranges("]}"), vec![]);
    }

    #[test]
    fn fold_shared_start_line_deduped() {
        // Object and array open on the same line; only the first-emitted
        // (innermost) range survives the prevStart check.
        assert_eq!(
            fold_ranges("{ \"a\": [\n    1,\n    2\n] }"),
            vec![fr(0, 2, FoldKind::Array)]
        );
    }

    #[test]
    fn fold_shared_start_line_sibling_resets_dedup() {
        // VS Code parity: prevStart only suppresses the immediately
        // following range; a sibling emitted in between lets the outer
        // object through even though it shares start line 0.
        let text = "{ \"a\": [\n  1\n], \"b\": [\n  2\n] }";
        assert_eq!(
            fold_ranges(text),
            vec![
                fr(0, 3, FoldKind::Object),
                fr(0, 1, FoldKind::Array),
                fr(2, 3, FoldKind::Array),
            ]
        );
    }

    #[test]
    fn fold_crlf() {
        let crlf = NESTED.replace('\n', "\r\n");
        assert_eq!(
            fold_ranges(&crlf),
            vec![
                fr(0, 7, FoldKind::Object),
                fr(1, 6, FoldKind::Object),
                fr(2, 4, FoldKind::Array),
            ]
        );
    }

    #[test]
    fn fold_lfcr_counts_as_single_break() {
        // cosmic-text's LineIter treats "\n\r" as one line ending; the
        // scanner must agree or fold lines drift from buffer lines.
        assert_eq!(
            fold_ranges("{\n\r\"a\": 1\n\r}"),
            vec![fr(0, 1, FoldKind::Object)]
        );
    }

    #[test]
    fn fold_formatted_document() {
        // Shaped like pretty-printer output: ordinary short lines, no
        // display chunking involved.
        let text = concat!(
            "{\n",
            "  \"db\": [\n",
            "    {\n",
            "      \"meta\": {\n",
            "        \"exported_on\": 1739336000000,\n",
            "        \"version\": \"5.0\"\n",
            "      },\n",
            "      \"data\": {\n",
            "        \"posts\": [\n",
            "          {\n",
            "            \"id\": \"63f\",\n",
            "            \"title\": \"First\"\n",
            "          },\n",
            "          {\n",
            "            \"id\": \"640\",\n",
            "            \"title\": \"Second\"\n",
            "          }\n",
            "        ],\n",
            "        \"tags\": []\n",
            "      }\n",
            "    }\n",
            "  ]\n",
            "}",
        );
        assert_eq!(
            fold_ranges(text),
            vec![
                fr(0, 21, FoldKind::Object),
                fr(1, 20, FoldKind::Array),
                fr(2, 19, FoldKind::Object),
                fr(3, 5, FoldKind::Object),
                fr(7, 18, FoldKind::Object),
                fr(8, 16, FoldKind::Array),
                fr(9, 11, FoldKind::Object),
                fr(13, 15, FoldKind::Object),
            ]
        );
    }

    #[test]
    fn fold_one_megabyte_under_50ms() {
        let mut doc = String::from("{\n\"posts\": [\n");
        let mut posts = 0u32;
        while doc.len() < 1_048_576 {
            doc.push_str(&format!(
                "{{\n\"id\": {posts},\n\"title\": \"Post number {posts} with some padding text\",\n\"tags\": [\"alpha\", \"beta\"]\n}},\n"
            ));
            posts += 1;
        }
        doc.push_str("{\"id\": -1}\n]\n}\n");
        assert!(doc.len() >= 1_048_576);

        let start = Instant::now();
        let ranges = fold_ranges(&doc);
        let elapsed = start.elapsed();

        // One range per multi-line post object, plus the posts array and
        // the root object. The trailing single-line object adds none.
        assert_eq!(ranges.len(), posts as usize + 2);
        assert!(
            elapsed < Duration::from_millis(50),
            "fold_ranges took {elapsed:?} on {} bytes",
            doc.len()
        );
    }
}

#[cfg(test)]
mod tree_tests {
    use super::*;

    const INDENT: &str = "  ";

    fn tree(text: &str) -> JsonTree {
        parse_tree(text, DEFAULT_NODE_BUDGET)
    }

    fn root(text: &str) -> JsonNode {
        tree(text).root.expect("input should produce a root node")
    }

    /// Iterative so deep trees cannot overflow the test stack either.
    fn node_count(tree: &JsonTree) -> usize {
        let mut count = 0;
        let mut stack: Vec<&JsonNode> = tree.root.iter().collect();
        while let Some(node) = stack.pop() {
            count += 1;
            stack.extend(&node.children);
        }
        count
    }

    /// Structure signature ignoring spans and lines, for reparse equivalence.
    fn shape(node: &JsonNode, out: &mut String) {
        out.push('(');
        if let Some(key) = &node.key {
            out.push_str(&format!("{key:?}="));
        }
        if let Some(index) = node.index {
            out.push_str(&format!("[{index}]="));
        }
        out.push_str(&format!("{:?} {:?}", node.kind, node.preview));
        for child in &node.children {
            shape(child, out);
        }
        out.push(')');
    }

    fn tree_shape(text: &str) -> String {
        let mut signature = String::new();
        if let Some(root) = &tree(text).root {
            shape(root, &mut signature);
        }
        signature
    }

    /// The raw byte slice of every token, in order: pretty-printing may only
    /// change the whitespace between tokens, never the tokens themselves.
    fn raw_tokens(text: &str) -> Vec<&str> {
        scan(text)
            .map(|spanned| &text[spanned.offset..spanned.offset + spanned.len])
            .collect()
    }

    const KEYED: &str =
        r#"{"zeta":1,"alpha":{"\u00e9clair":"caf\u00e9","alpha":2},"zeta":[true,null]}"#;

    #[test]
    fn pretty_print_preserves_key_order_and_bytes() {
        let out = pretty_print(KEYED, INDENT).expect("valid json");
        let expected = concat!(
            "{\n",
            "  \"zeta\": 1,\n",
            "  \"alpha\": {\n",
            "    \"\\u00e9clair\": \"caf\\u00e9\",\n",
            "    \"alpha\": 2\n",
            "  },\n",
            "  \"zeta\": [\n",
            "    true,\n",
            "    null\n",
            "  ]\n",
            "}",
        );
        // Duplicate "zeta" keys survive in source order and escaped keys
        // keep their original bytes (no \u00e9 -> é normalization).
        assert_eq!(out, expected);
        assert_eq!(raw_tokens(KEYED), raw_tokens(&out));
        // Reparse equivalence and idempotence.
        assert_eq!(tree_shape(KEYED), tree_shape(&out));
        assert_eq!(pretty_print(&out, INDENT).as_deref(), Some(out.as_str()));
    }

    #[test]
    fn pretty_print_unicode_keys() {
        let text = "{\"日本語\":\"emoji 😀 value\",\"café\":[\"π\"]}";
        let out = pretty_print(text, INDENT).expect("valid json");
        let expected = "{\n  \"日本語\": \"emoji 😀 value\",\n  \"café\": [\n    \"π\"\n  ]\n}";
        assert_eq!(out, expected);
        assert_eq!(raw_tokens(text), raw_tokens(&out));
        assert_eq!(tree_shape(text), tree_shape(&out));
    }

    #[test]
    fn pretty_print_scalars_and_crlf() {
        assert_eq!(pretty_print("  42  ", INDENT).as_deref(), Some("42"));
        // Number tokens are emitted byte-for-byte; CRLF input formats to \n.
        let out = pretty_print("{\r\n\"n\": [1e9, -0.0, 0.10]\r\n}", INDENT).unwrap();
        assert_eq!(out, "{\n  \"n\": [\n    1e9,\n    -0.0,\n    0.10\n  ]\n}");
    }

    #[test]
    fn pretty_print_empty_containers_stay_compact() {
        assert_eq!(
            pretty_print(r#"{"a":{},"b":[]}"#, INDENT).as_deref(),
            Some("{\n  \"a\": {},\n  \"b\": []\n}")
        );
        assert_eq!(pretty_print("{}", INDENT).as_deref(), Some("{}"));
        assert_eq!(pretty_print("[]", INDENT).as_deref(), Some("[]"));
    }

    #[test]
    fn pretty_print_rejects_malformed() {
        // Formatting must never rewrite a document it could not fully parse:
        // anything malformed (not just an unparseable root) returns None.
        for bad in [
            "",
            "   ",
            "{",
            r#"{"a":1"#,
            r#"{"a" 1}"#,
            r#"{"a":1,}"#,
            "[1,]",
            "[1 2]",
            "{} {}",
            "NaN",
            r#"{"a":}"#,
            "[1, @, 2]",
            "]",
            r#"{"a"}"#,
        ] {
            assert_eq!(pretty_print(bad, INDENT), None, "input: {bad:?}");
        }
    }

    const NESTED: &str =
        "{\n  \"a\": {\n    \"b\": [\n      1,\n      2\n    ],\n    \"c\": 3\n  }\n}";

    #[test]
    fn parse_tree_spans_kinds_lines() {
        let root = root(NESTED);
        assert_eq!(root.kind, JsonKind::Object { len: 1 });
        assert_eq!(root.preview, "{1}");
        assert_eq!((root.offset, root.len), (0, NESTED.len()));
        assert_eq!((root.line, root.end_line), (0, 8));
        assert_eq!((root.key.as_deref(), root.index), (None, None));

        let a = &root.children[0];
        assert_eq!(a.key.as_deref(), Some("a"));
        assert_eq!(a.kind, JsonKind::Object { len: 2 });
        assert_eq!(
            &NESTED[a.offset..a.offset + a.len],
            "{\n    \"b\": [\n      1,\n      2\n    ],\n    \"c\": 3\n  }"
        );
        assert_eq!((a.line, a.end_line), (1, 7));

        let b = &a.children[0];
        assert_eq!(b.kind, JsonKind::Array { len: 2 });
        assert_eq!(b.preview, "[2]");
        assert_eq!(&NESTED[b.offset..b.offset + b.len], "[\n      1,\n      2\n    ]");
        assert_eq!((b.line, b.end_line), (2, 5));
        let one = &b.children[0];
        assert_eq!(
            (one.key.as_deref(), one.index, one.kind, one.preview.as_str()),
            (None, Some(0), JsonKind::Num, "1")
        );
        assert_eq!((one.line, one.end_line), (3, 3));

        let c = &a.children[1];
        assert_eq!(
            (c.key.as_deref(), c.kind, c.preview.as_str(), c.line),
            (Some("c"), JsonKind::Num, "3", 6)
        );
    }

    #[test]
    fn string_node_span_includes_quotes() {
        let text = r#"{"k":"v\u0041"}"#;
        let root = root(text);
        let k = &root.children[0];
        assert_eq!(k.kind, JsonKind::Str);
        // Copy-value slices the raw span, quotes and escapes included…
        assert_eq!(&text[k.offset..k.offset + k.len], r#""v\u0041""#);
        // …while the preview shows the unescaped text.
        assert_eq!(k.preview, "vA");
    }

    #[test]
    fn preview_truncation() {
        let long = "x".repeat(60);
        let text = format!(r#"{{"s":"{long}"}}"#);
        let s = &root(&text).children[0];
        assert_eq!(s.preview, format!("{}…", "x".repeat(40)));
        assert_eq!(s.preview.chars().count(), 41);

        // Exactly at the cap: no ellipsis. Truncation counts chars, not
        // bytes, so multibyte text cannot split a codepoint.
        let exact = "é".repeat(40);
        let text = format!(r#"{{"s":"{exact}"}}"#);
        assert_eq!(root(&text).children[0].preview, exact);

        let over = "é".repeat(41);
        let text = format!(r#"{{"s":"{over}"}}"#);
        assert_eq!(root(&text).children[0].preview, format!("{}…", "é".repeat(40)));
    }

    #[test]
    fn preview_controls_flattened() {
        // Tree rows are single-line: control characters become spaces.
        let s = &root(r#"{"s":"a\nb\tc"}"#).children[0];
        assert_eq!(s.preview, "a b c");
    }

    #[test]
    fn container_and_literal_previews() {
        let members = (0..13)
            .map(|i| format!(r#""k{i}":{i}"#))
            .collect::<Vec<_>>()
            .join(",");
        let zeros = vec!["0"; 47].join(",");
        let text = format!(
            "{{\"o\":{{{members}}},\"arr\":[{zeros}],\"t\":true,\"n\":null,\"f\":-1.5e3}}"
        );
        let root = root(&text);
        let by_key = |key: &str| {
            root.children
                .iter()
                .find(|node| node.key.as_deref() == Some(key))
                .expect("key present")
        };
        assert_eq!(by_key("o").preview, "{13}");
        assert_eq!(by_key("arr").preview, "[47]");
        assert_eq!(by_key("t").preview, "true");
        assert_eq!(by_key("n").preview, "null");
        assert_eq!(by_key("f").preview, "-1.5e3");
        assert_eq!(by_key("t").kind, JsonKind::Bool);
        assert_eq!(by_key("n").kind, JsonKind::Null);
    }

    // Full parse materializes 8 nodes: root, [1,2,3], 1, 2, 3, {c}, true, "x".
    const BUDGETED: &str = r#"{"a":[1,2,3],"b":{"c":true},"d":"x"}"#;

    #[test]
    fn node_budget_truncation_flagged() {
        let full = parse_tree(BUDGETED, 8);
        assert_eq!(full.truncated, 0);
        assert_eq!(node_count(&full), 8);

        let cut = parse_tree(BUDGETED, 5);
        assert_eq!(node_count(&cut), 5);
        assert_eq!(cut.truncated, 3);
        let root = cut.root.expect("root survives truncation");
        // Entry counts stay source-true even when children were dropped.
        assert_eq!(root.kind, JsonKind::Object { len: 3 });
        assert_eq!(root.preview, "{3}");
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].children.len(), 3);

        let none = parse_tree(BUDGETED, 0);
        assert!(none.root.is_none());
        assert_eq!(none.truncated, 8);
    }

    #[test]
    fn junk_item_closes_array() {
        // Error tolerance rule: an unexpected token closes the current
        // container and descent continues in the parent.
        let root = root("[1, @, 2]");
        assert_eq!(root.kind, JsonKind::Array { len: 1 });
        assert_eq!(root.children[0].preview, "1");
    }

    #[test]
    fn missing_colon_closes_object() {
        let root = root(r#"{"a" 1}"#);
        assert_eq!(root.kind, JsonKind::Object { len: 0 });
        assert!(root.children.is_empty());
    }

    #[test]
    fn missing_comma_closes_object() {
        let root = root(r#"{"a":1 "b":2}"#);
        assert_eq!(root.kind, JsonKind::Object { len: 1 });
        assert_eq!(root.children[0].key.as_deref(), Some("a"));
    }

    #[test]
    fn unclosed_containers_close_at_eof() {
        let text = "{\"a\": {\"b\": 1";
        let root = root(text);
        assert_eq!(root.kind, JsonKind::Object { len: 1 });
        assert_eq!(root.len, text.len());
        let a = &root.children[0];
        assert_eq!(a.kind, JsonKind::Object { len: 1 });
        assert_eq!(&text[a.offset..a.offset + a.len], "{\"b\": 1");
    }

    #[test]
    fn mismatched_close_tolerated() {
        // `]` cannot close the object; the object closes early and the
        // bracket then closes the array normally.
        let root = root(r#"[{"a":1]"#);
        assert_eq!(root.kind, JsonKind::Array { len: 1 });
        assert_eq!(root.children[0].kind, JsonKind::Object { len: 1 });
    }

    #[test]
    fn junk_before_root_skipped() {
        let root = root(", ] {\"a\":1}");
        assert_eq!(root.kind, JsonKind::Object { len: 1 });
        assert!(parse_tree("@ ,", DEFAULT_NODE_BUDGET).root.is_none());
    }

    #[test]
    fn deep_nesting_capped_not_crashed() {
        // 10k unclosed opens: descent recursion caps at MAX_DEPTH and the
        // rest is skipped iteratively — no stack overflow, dropped values
        // are counted.
        let text = "[".repeat(10_000);
        let tree = parse_tree(&text, usize::MAX);
        assert_eq!(node_count(&tree), MAX_DEPTH + 1);
        assert_eq!(tree.truncated, 10_000 - (MAX_DEPTH + 1));

        // A valid deep document still pretty-prints: the printer is
        // iterative and tracks only a kind stack.
        let valid = format!("{}1{}", "[".repeat(500), "]".repeat(500));
        let out = pretty_print(&valid, INDENT).expect("valid deep doc");
        assert_eq!(raw_tokens(&valid), raw_tokens(&out));
    }

    #[test]
    fn json_path_segments_and_quoting() {
        let text = r#"{"db":{"posts":[{"slug":"a"},{"slug":"b"},{"slug":"c"},{"slug":"d","weird key":1,"3d":2,"café":3,"q\"b":4,"":5}]}}"#;
        let tree = tree(text);
        let root = tree.root.as_ref().unwrap();
        let db = &root.children[0];
        let posts = &db.children[0];
        let third = &posts.children[3];
        let slug = &third.children[0];
        assert_eq!(json_path(&[root]), "");
        assert_eq!(json_path(&[root, db]), "db");
        assert_eq!(json_path(&[root, db, posts]), "db.posts");
        assert_eq!(json_path(&[root, db, posts, third]), "db.posts[3]");
        assert_eq!(json_path(&[root, db, posts, third, slug]), "db.posts[3].slug");

        let seg = |i: usize| json_path(&[root, db, posts, third, &third.children[i]]);
        assert_eq!(seg(1), r#"db.posts[3]["weird key"]"#);
        assert_eq!(seg(2), r#"db.posts[3]["3d"]"#);
        assert_eq!(seg(3), "db.posts[3].café");
        assert_eq!(seg(4), r#"db.posts[3]["q\"b"]"#);
        assert_eq!(seg(5), r#"db.posts[3][""]"#);

        // A quoted key at the head takes no leading dot.
        let head = parse_tree(r#"{"weird key":{"x":1}}"#, DEFAULT_NODE_BUDGET);
        let hroot = head.root.as_ref().unwrap();
        let wk = &hroot.children[0];
        let x = &wk.children[0];
        assert_eq!(json_path(&[hroot, wk, x]), r#"["weird key"].x"#);
    }
}
