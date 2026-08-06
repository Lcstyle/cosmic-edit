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
