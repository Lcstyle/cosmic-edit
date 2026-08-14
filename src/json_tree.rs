// SPDX-License-Identifier: GPL-3.0-only

//! JSON tree pane: per-tab view state (expansion, filter, follow-cursor
//! resolution) over the span AST from [`crate::json_scan`], plus the pane's
//! widget builder for the split view.
//!
//! Node identity is the **pre-order index** over the parsed tree (root = 0,
//! then depth-first in document order). Ids are stable for a given parse;
//! across rebuilds expansion is carried over by hashing each node's key/index
//! path instead ([`JsonViewState::rebuild`]).
//!
//! Everything above the "view" section is pure state logic with no UI
//! imports, exercised directly by the unit tests at the bottom.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::json_scan::{self, JsonKind, JsonNode, JsonTree};

/// Pre-order index of a node in the parsed tree.
pub type NodeId = usize;

/// Fixed height of one pane row, so scroll positions are exact row math.
pub const TREE_ROW_HEIGHT: f32 = 24.0;

/// Hard cap on rows built per frame. The parse budget already caps the tree
/// at 100k nodes, but expanding a huge array would still put every child in
/// the column; past this the pane shows one "… N more" row instead.
pub const MAX_RENDERED_ROWS: usize = 2_000;

/// Per-node derived data, indexed by [`NodeId`] in pre-order.
#[derive(Clone, Debug)]
struct NodeMeta {
    parent: Option<NodeId>,
    /// Node count of the subtree rooted here, including self. Lets child ids
    /// be computed incrementally: first child = id + 1, next sibling =
    /// child + subtree.
    subtree: usize,
    /// Hash of the key/index path from the root; how expansion survives a
    /// rebuild after the ids shift.
    path_hash: u64,
    depth: u16,
}

/// Per-tab JSON support state: the minified-file banner flag (Phase B), the
/// cached span AST and the tree-pane interaction state. The fold phases
/// extend it further.
#[derive(Clone, Debug)]
pub struct JsonViewState {
    /// Show the "minified JSON" banner row above the text box. Cleared by
    /// the banner's dismiss button or by a successful Format.
    pub banner: bool,
    pub tree: JsonTree,
    /// Nodes whose children are shown. Default: root + its direct children,
    /// so top-level keys are visible and everything nested starts collapsed.
    pub expanded: HashSet<NodeId>,
    /// Case-insensitive key filter; empty = off.
    pub filter: String,
    /// Row highlighted by follow-cursor or the last jump.
    pub selected: Option<NodeId>,
    /// Row targeted by the open context menu.
    pub context_node: Option<NodeId>,
    meta: Vec<NodeMeta>,
    /// Byte offset where each source line starts, same line-break pairing as
    /// the scanner ("\r\n" / "\n\r" are single breaks). Maps buffer cursor
    /// (line, byte index) ↔ absolute offset while the buffer's lines line up
    /// 1:1 with the parsed text (they do unless display-chunking is active).
    line_starts: Vec<usize>,
    text_len: usize,
    /// When filtering: ids visible under the filter (matches + their
    /// ancestors). `None` when the filter is empty.
    filter_visible: Option<HashSet<NodeId>>,
}

impl Default for JsonViewState {
    fn default() -> Self {
        Self {
            banner: false,
            tree: JsonTree {
                root: None,
                truncated: 0,
            },
            expanded: HashSet::new(),
            filter: String::new(),
            selected: None,
            context_node: None,
            meta: Vec::new(),
            line_starts: Vec::new(),
            text_len: 0,
            filter_visible: None,
        }
    }
}

/// One visible row of the pane, produced by [`JsonViewState::visible_rows`].
#[derive(Debug)]
pub struct TreeRow<'a> {
    pub id: NodeId,
    pub depth: u16,
    pub node: &'a JsonNode,
    pub has_children: bool,
    pub expanded: bool,
}

impl JsonViewState {
    /// Banner-only state with no parsed tree (tests and error paths).
    pub fn with_banner(banner: bool) -> Self {
        Self {
            banner,
            ..Self::default()
        }
    }

    /// Parse `text` and build the full pane state with default expansion.
    pub fn from_text(text: &str, banner: bool) -> Self {
        let mut state = Self::with_banner(banner);
        state.rebuild(text);
        state
    }

    /// Whether the pane has anything to show.
    pub fn has_tree(&self) -> bool {
        self.tree.root.is_some()
    }

    /// Re-parse after a buffer change. Expansion and selection are carried
    /// over by path-hash where the same path still exists; when nothing
    /// survives (or there was no tree before) expansion resets to the
    /// default. Ids from before this call are invalid afterwards.
    pub fn rebuild(&mut self, text: &str) {
        let old_expanded: HashSet<u64> = self
            .expanded
            .iter()
            .filter_map(|id| self.meta.get(*id).map(|m| m.path_hash))
            .collect();
        let old_selected = self
            .selected
            .and_then(|id| self.meta.get(id).map(|m| m.path_hash));
        let had_tree = self.has_tree();

        self.tree = json_scan::parse_tree(text, json_scan::DEFAULT_NODE_BUDGET);
        self.meta = build_meta(&self.tree);
        self.line_starts = compute_line_starts(text);
        self.text_len = text.len();

        let carried: HashSet<NodeId> = self
            .meta
            .iter()
            .enumerate()
            .filter(|(_, m)| old_expanded.contains(&m.path_hash))
            .map(|(id, _)| id)
            .collect();
        // The root's path always exists, so it always carries; a carry of
        // *only* the root means no real path survived — reset to default.
        let meaningful = carried.iter().any(|&id| id != 0);
        self.expanded = if !meaningful || !had_tree {
            self.default_expanded()
        } else {
            carried
        };
        self.selected =
            old_selected.and_then(|hash| self.meta.iter().position(|m| m.path_hash == hash));
        self.context_node = None;
        // Recompute the filter's visible set against the new ids.
        let filter = std::mem::take(&mut self.filter);
        self.set_filter(filter);
    }

    /// The user's "top level only" rule: root + its direct children are
    /// expanded, so top-level keys show while everything nested starts
    /// collapsed.
    fn default_expanded(&self) -> HashSet<NodeId> {
        let mut expanded = HashSet::new();
        let Some(root) = &self.tree.root else {
            return expanded;
        };
        expanded.insert(0);
        let mut child_id = 1;
        for _ in &root.children {
            expanded.insert(child_id);
            child_id += self.meta[child_id].subtree;
        }
        expanded
    }

    pub fn toggle_expanded(&mut self, id: NodeId) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
    }

    /// Set the key filter. Non-empty: rows shown are nodes whose key
    /// contains the needle (case-insensitive) plus all their ancestors,
    /// regardless of expansion state.
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.filter_visible = if self.filter.is_empty() {
            None
        } else {
            let needle = self.filter.to_lowercase();
            let mut visible = HashSet::new();
            let mut id = 0;
            let mut stack: Vec<&JsonNode> = self.tree.root.iter().collect();
            // Children are pushed reversed, so LIFO pop order is exactly
            // pre-order and the running `id` matches each popped node.
            while let Some(node) = stack.pop() {
                let matches = node
                    .key
                    .as_deref()
                    .is_some_and(|key| key.to_lowercase().contains(&needle));
                if matches {
                    let mut cur = Some(id);
                    while let Some(node_id) = cur {
                        if !visible.insert(node_id) {
                            break; // ancestors above are already in
                        }
                        cur = self.meta[node_id].parent;
                    }
                }
                id += 1;
                for child in node.children.iter().rev() {
                    stack.push(child);
                }
            }
            Some(visible)
        };
    }

    pub fn clear_filter(&mut self) {
        self.set_filter(String::new());
    }

    /// Node lookup by pre-order id: descend from the root using subtree
    /// sizes. O(depth × fanout).
    pub fn node(&self, id: NodeId) -> Option<&JsonNode> {
        self.path_to(id).map(|path| *path.last().unwrap())
    }

    /// The root→node chain of tree references for `id` (for
    /// [`json_scan::json_path`] and value spans).
    pub fn path_to(&self, id: NodeId) -> Option<Vec<&JsonNode>> {
        if id >= self.meta.len() {
            return None;
        }
        let mut node = self.tree.root.as_ref()?;
        let mut node_id = 0;
        let mut path = vec![node];
        while node_id != id {
            let mut child_id = node_id + 1;
            let mut descended = false;
            for child in &node.children {
                let size = self.meta[child_id].subtree;
                if id >= child_id && id < child_id + size {
                    node = child;
                    node_id = child_id;
                    path.push(node);
                    descended = true;
                    break;
                }
                child_id += size;
            }
            if !descended {
                return None;
            }
        }
        Some(path)
    }

    /// Ancestor ids root→`id` inclusive, via parent links.
    pub fn ancestor_chain(&self, id: NodeId) -> Vec<NodeId> {
        let mut chain = Vec::new();
        let mut cur = Some(id);
        while let Some(node_id) = cur {
            chain.push(node_id);
            cur = self.meta.get(node_id).and_then(|m| m.parent);
        }
        chain.reverse();
        chain
    }

    /// Deepest node containing the absolute byte `offset`, as the root→node
    /// id chain. Offsets outside the root's span resolve to the root.
    pub fn resolve_offset(&self, offset: usize) -> Option<Vec<NodeId>> {
        let root = self.tree.root.as_ref()?;
        let mut chain = vec![0];
        let mut node = root;
        let mut node_id = 0;
        loop {
            let mut child_id = node_id + 1;
            let mut descended = false;
            for child in &node.children {
                if child.offset > offset {
                    break; // children are in document order
                }
                if offset < child.offset + child.len {
                    node = child;
                    node_id = child_id;
                    chain.push(node_id);
                    descended = true;
                    break;
                }
                child_id += self.meta[child_id].subtree;
            }
            if !descended {
                return Some(chain);
            }
        }
    }

    /// Number of source lines the state was built from. When this equals the
    /// buffer's line count the two line spaces are 1:1 and cursor mapping is
    /// pure arithmetic; a mismatch means display chunking (or a stale tree)
    /// and callers fall back to walking the buffer.
    pub fn aligned_line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Absolute byte offset of a buffer cursor, valid in the aligned case.
    pub fn offset_for_position(&self, line: usize, index: usize) -> Option<usize> {
        let start = *self.line_starts.get(line)?;
        Some((start + index).min(self.text_len))
    }

    /// Editor target for a node in the aligned case: (line, byte column).
    pub fn jump_target(&self, id: NodeId) -> Option<(usize, usize)> {
        let node = self.node(id)?;
        let line = node.line as usize;
        let col = node.offset.checked_sub(*self.line_starts.get(line)?)?;
        Some((line, col))
    }

    /// Byte span (offset, len) of a node's value in the parsed text.
    pub fn node_span(&self, id: NodeId) -> Option<(usize, usize)> {
        self.node(id).map(|node| (node.offset, node.len))
    }

    /// Follow-cursor resolution: select the deepest node containing
    /// `offset`, expanding its ancestors. Returns the selected row's index
    /// among the visible rows when the selection *changed* (callers scroll
    /// the pane there), `None` when it stayed put or nothing resolved.
    /// Inert while a filter is active — the filtered skeleton is an explicit
    /// browse mode and follow would fight it.
    pub fn follow_offset(&mut self, offset: usize) -> Option<usize> {
        if self.filter_visible.is_some() {
            return None;
        }
        let chain = self.resolve_offset(offset)?;
        let deepest = *chain.last()?;
        if self.selected == Some(deepest) {
            return None;
        }
        for id in &chain[..chain.len() - 1] {
            self.expanded.insert(*id);
        }
        self.selected = Some(deepest);
        self.visible_rows().iter().position(|row| row.id == deepest)
    }

    /// The rows the pane shows, in document order. Without a filter: nodes
    /// whose ancestors are all expanded. With one: matches + ancestors.
    pub fn visible_rows(&self) -> Vec<TreeRow<'_>> {
        let mut rows = Vec::new();
        let Some(root) = &self.tree.root else {
            return rows;
        };
        match &self.filter_visible {
            None => self.push_expanded(root, 0, &mut rows),
            Some(visible) => self.push_filtered(root, 0, visible, &mut rows),
        }
        rows
    }

    fn push_expanded<'a>(&'a self, node: &'a JsonNode, id: NodeId, rows: &mut Vec<TreeRow<'a>>) {
        let expanded = self.expanded.contains(&id);
        rows.push(TreeRow {
            id,
            depth: self.meta[id].depth,
            node,
            has_children: !node.children.is_empty(),
            expanded,
        });
        if expanded {
            let mut child_id = id + 1;
            for child in &node.children {
                self.push_expanded(child, child_id, rows);
                child_id += self.meta[child_id].subtree;
            }
        }
    }

    fn push_filtered<'a>(
        &'a self,
        node: &'a JsonNode,
        id: NodeId,
        visible: &HashSet<NodeId>,
        rows: &mut Vec<TreeRow<'a>>,
    ) {
        // Any visible descendant puts its ancestors in `visible`, so not
        // being in the set means the whole subtree is dark.
        if !visible.contains(&id) {
            return;
        }
        rows.push(TreeRow {
            id,
            depth: self.meta[id].depth,
            node,
            has_children: !node.children.is_empty(),
            expanded: true,
        });
        let mut child_id = id + 1;
        for child in &node.children {
            self.push_filtered(child, child_id, visible, rows);
            child_id += self.meta[child_id].subtree;
        }
    }

}

/// Pre-order walk assigning ids and computing parent/subtree/path-hash.
fn build_meta(tree: &JsonTree) -> Vec<NodeMeta> {
    fn walk(
        node: &JsonNode,
        parent: Option<NodeId>,
        parent_hash: u64,
        depth: u16,
        meta: &mut Vec<NodeMeta>,
    ) -> usize {
        let id = meta.len();
        let path_hash = segment_hash(parent_hash, node);
        meta.push(NodeMeta {
            parent,
            subtree: 0,
            path_hash,
            depth,
        });
        let mut size = 1;
        for child in &node.children {
            size += walk(child, Some(id), path_hash, depth + 1, meta);
        }
        meta[id].subtree = size;
        size
    }
    let mut meta = Vec::new();
    if let Some(root) = &tree.root {
        walk(root, None, 0, 0, &mut meta);
    }
    meta
}

fn segment_hash(parent_hash: u64, node: &JsonNode) -> u64 {
    let mut hasher = DefaultHasher::new();
    parent_hash.hash(&mut hasher);
    if let Some(index) = node.index {
        1u8.hash(&mut hasher);
        index.hash(&mut hasher);
    } else if let Some(key) = node.key.as_deref() {
        2u8.hash(&mut hasher);
        key.hash(&mut hasher);
    } else {
        3u8.hash(&mut hasher); // root
    }
    hasher.finish()
}

/// Byte offsets where lines start, with the scanner's line-break pairing:
/// "\r\n" and "\n\r" count as one break (matches cosmic-text's `LineIter`).
fn compute_line_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut starts = vec![0];
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                i += 1;
                if bytes.get(i) == Some(&b'\r') {
                    i += 1;
                }
                starts.push(i);
            }
            b'\r' => {
                i += 1;
                if bytes.get(i) == Some(&b'\n') {
                    i += 1;
                }
                starts.push(i);
            }
            _ => i += 1,
        }
    }
    starts
}

/// Short type label for the row badge.
pub fn kind_label(kind: JsonKind) -> &'static str {
    match kind {
        JsonKind::Object { .. } => "obj",
        JsonKind::Array { .. } => "arr",
        JsonKind::Str => "str",
        JsonKind::Num => "num",
        JsonKind::Bool => "bool",
        JsonKind::Null => "null",
    }
}

/// Row label: member key, `[i]` for array items, `$` for the root.
pub fn row_label(node: &JsonNode) -> String {
    if let Some(index) = node.index {
        format!("[{index}]")
    } else if let Some(key) = node.key.as_deref() {
        key.to_string()
    } else {
        "$".to_string()
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

use cosmic::iced::{Alignment, Background, Length};
use cosmic::widget::menu::Item as MenuItem;
use cosmic::widget::menu::key_bind::KeyBind;
use cosmic::widget::segmented_button::Entity;
use cosmic::{Element, widget};
use std::collections::HashMap;

use crate::{Message, fl};

/// Actions of the row context menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonTreeContextAction {
    CopyPath(Entity, NodeId),
    CopyValue(Entity, NodeId),
}

impl widget::menu::Action for JsonTreeContextAction {
    type Message = Message;

    fn message(&self) -> Message {
        match *self {
            Self::CopyPath(entity, id) => Message::JsonTreeCopyPath(entity, id),
            Self::CopyValue(entity, id) => Message::JsonTreeCopyValue(entity, id),
        }
    }
}

/// Context-menu items for a row, consumed by `widget::context_menu`.
pub fn context_menu_items(entity: Entity, id: NodeId) -> Vec<widget::menu::Tree<Message>> {
    let empty_key_binds: HashMap<KeyBind, JsonTreeContextAction> = HashMap::new();
    widget::menu::items(
        &empty_key_binds,
        vec![
            MenuItem::Button(
                fl!("json-tree-copy-path"),
                None,
                JsonTreeContextAction::CopyPath(entity, id),
            ),
            MenuItem::Button(
                fl!("json-tree-copy-value"),
                None,
                JsonTreeContextAction::CopyValue(entity, id),
            ),
        ],
    )
}

/// Build the tree pane: filter box on top, capped row list in a scrollable.
pub fn tree_pane<'a>(
    state: &'a JsonViewState,
    entity: Entity,
    scroll_id: widget::Id,
) -> Element<'a, Message> {
    let filter_input = widget::text_input::search_input(fl!("json-tree-filter"), &state.filter)
        .on_input(move |value| Message::JsonTreeFilter(entity, value))
        .on_clear(Message::JsonTreeFilter(entity, String::new()));

    let rows = state.visible_rows();
    let overflow = rows.len().saturating_sub(MAX_RENDERED_ROWS);
    let mut column = widget::column::with_capacity(rows.len().min(MAX_RENDERED_ROWS) + 2);
    for row in rows.iter().take(MAX_RENDERED_ROWS) {
        column = column.push(tree_row(entity, state.selected, row));
    }
    if overflow > 0 {
        column = column.push(widget::text::caption(fl!(
            "json-tree-more",
            count = overflow
        )));
    }
    if state.tree.truncated > 0 {
        column = column.push(widget::text::caption(fl!(
            "json-tree-truncated",
            count = state.tree.truncated
        )));
    }

    let scroll = widget::scrollable(column.width(Length::Fill))
        .id(scroll_id)
        .height(Length::Fill);

    widget::column::with_capacity(2)
        .push(filter_input)
        .push(scroll)
        .spacing(4)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn tree_row<'a>(
    entity: Entity,
    selected: Option<NodeId>,
    row: &TreeRow<'a>,
) -> Element<'a, Message> {
    let id = row.id;
    let indent = f32::from(row.depth) * 12.0;

    let chevron: Element<'a, Message> = if row.has_children {
        widget::mouse_area(
            widget::container(widget::text::body(if row.expanded {
                "\u{25be}" // ▾
            } else {
                "\u{25b8}" // ▸
            }))
            .width(Length::Fixed(16.0))
            .align_x(Alignment::Center),
        )
        .on_press(Message::JsonTreeToggle(entity, id))
        .into()
    } else {
        widget::Space::new().width(Length::Fixed(16.0)).into()
    };

    let content = widget::row::with_capacity(5)
        .push(widget::Space::new().width(Length::Fixed(indent)))
        .push(chevron)
        .push(widget::text::body(row_label(row.node)))
        .push(widget::text::caption(kind_label(row.node.kind)))
        .push(widget::text::caption(&row.node.preview))
        .align_y(Alignment::Center)
        .spacing(6);

    let is_selected = selected == Some(id);
    let container = widget::container(content)
        .width(Length::Fill)
        .height(Length::Fixed(TREE_ROW_HEIGHT))
        .style(move |theme: &cosmic::Theme| {
            let mut style = widget::container::Style::default();
            if is_selected {
                let mut color: cosmic::iced::Color = theme.cosmic().accent_color().into();
                color.a = 0.2;
                style.background = Some(Background::Color(color));
            }
            style
        });

    widget::mouse_area(container)
        .on_press(Message::JsonTreeJump(entity, id))
        .on_right_press(Message::JsonTreeRowContext(entity, id))
        .into()
}

// ---------------------------------------------------------------------------
// Tests — pure state logic only
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const NESTED: &str = r#"{
  "db": {
    "posts": [
      {"slug": "first", "html": "<p>a</p>"},
      {"slug": "second", "html": "<p>b</p>"}
    ],
    "meta": {"version": 5}
  },
  "title": "site"
}"#;

    fn ids_by_label(state: &JsonViewState) -> Vec<(NodeId, String)> {
        state
            .visible_rows()
            .iter()
            .map(|row| (row.id, row_label(row.node)))
            .collect()
    }

    fn id_of(state: &JsonViewState, label: &str) -> NodeId {
        ids_by_label(state)
            .into_iter()
            .find(|(_, l)| l == label)
            .unwrap_or_else(|| panic!("no visible row labeled {label:?}"))
            .0
    }

    /// Default expansion is root + its direct children: top-level keys and
    /// their immediate children are rows, deeper levels are not.
    #[test]
    fn default_expansion_shows_two_levels() {
        let state = JsonViewState::from_text(NESTED, false);
        let labels: Vec<String> = state
            .visible_rows()
            .iter()
            .map(|row| row_label(row.node))
            .collect();
        assert_eq!(labels, ["$", "db", "posts", "meta", "title"]);

        // "posts" (a grandchild of the root) is visible but collapsed: its
        // array items are not rows.
        let posts = state
            .visible_rows()
            .into_iter()
            .find(|row| row_label(row.node) == "posts")
            .unwrap();
        assert!(posts.has_children);
        assert!(!posts.expanded);
    }

    /// NodeIds are pre-order over the whole tree: expanding a node deepens
    /// the row list without renumbering anything.
    #[test]
    fn node_ids_are_preorder_and_stable_across_expansion() {
        let mut state = JsonViewState::from_text(NESTED, false);
        // Pre-order: 0=$ 1=db 2=posts 3=[0] 4=slug 5=html 6=[1] ...
        assert_eq!(id_of(&state, "db"), 1);
        assert_eq!(id_of(&state, "posts"), 2);
        let title_before = id_of(&state, "title");

        state.toggle_expanded(2); // expand posts
        assert_eq!(id_of(&state, "[0]"), 3);
        assert_eq!(id_of(&state, "[1]"), 6);
        assert_eq!(
            id_of(&state, "title"),
            title_before,
            "expansion must not renumber"
        );

        // node() agrees with the visible-row labeling
        assert_eq!(state.node(2).unwrap().key.as_deref(), Some("posts"));
        assert_eq!(state.node(3).unwrap().index, Some(0));
    }

    #[test]
    fn toggle_collapses_and_restores() {
        let mut state = JsonViewState::from_text(NESTED, false);
        let db = id_of(&state, "db");
        state.toggle_expanded(db);
        let labels: Vec<String> = state
            .visible_rows()
            .iter()
            .map(|row| row_label(row.node))
            .collect();
        assert_eq!(labels, ["$", "db", "title"], "collapsed db hides its keys");
        state.toggle_expanded(db);
        assert_eq!(ids_by_label(&state).len(), 5, "re-expand restores");
    }

    /// Filter: case-insensitive key match; matches and their ancestors are
    /// visible even where expansion had them hidden; clearing restores the
    /// expansion-based view.
    #[test]
    fn filter_shows_matches_with_ancestors() {
        let mut state = JsonViewState::from_text(NESTED, false);
        // "slug" lives two levels below the deepest expanded node.
        state.set_filter("SLUG".to_string());
        let labels: Vec<String> = state
            .visible_rows()
            .iter()
            .map(|row| row_label(row.node))
            .collect();
        assert_eq!(
            labels,
            ["$", "db", "posts", "[0]", "slug", "[1]", "slug"],
            "matches plus ancestor chain, nothing else"
        );

        state.clear_filter();
        let labels: Vec<String> = state
            .visible_rows()
            .iter()
            .map(|row| row_label(row.node))
            .collect();
        assert_eq!(labels, ["$", "db", "posts", "meta", "title"]);
    }

    #[test]
    fn filter_with_no_matches_shows_nothing() {
        let mut state = JsonViewState::from_text(NESTED, false);
        state.set_filter("zzz-not-here".to_string());
        assert!(state.visible_rows().is_empty());
    }

    /// Follow: a byte offset resolves to the deepest containing node, its
    /// ancestors get expanded, and the row index comes back for scrolling.
    /// Re-following the same node reports no change.
    #[test]
    fn follow_resolves_deepest_and_expands_ancestors() {
        let mut state = JsonViewState::from_text(NESTED, false);
        let offset = NESTED.find("\"first\"").unwrap();
        let row_idx = state.follow_offset(offset).expect("selection must change");

        let selected = state.selected.expect("follow selects");
        let node = state.node(selected).unwrap();
        assert_eq!(node.key.as_deref(), Some("slug"));
        assert_eq!(
            state.visible_rows()[row_idx].id,
            selected,
            "returned index must point at the selected row"
        );
        // Ancestors got expanded on the way down.
        let chain = state.ancestor_chain(selected);
        for id in &chain[..chain.len() - 1] {
            assert!(state.expanded.contains(id), "ancestor {id} must expand");
        }

        assert_eq!(
            state.follow_offset(offset),
            None,
            "same node again: no change, no re-scroll"
        );
    }

    #[test]
    fn follow_is_inert_while_filtering() {
        let mut state = JsonViewState::from_text(NESTED, false);
        state.set_filter("slug".to_string());
        let offset = NESTED.find("\"first\"").unwrap();
        assert_eq!(state.follow_offset(offset), None);
        assert_eq!(state.selected, None);
    }

    /// Cursor mapping in the aligned case, both directions, including CRLF.
    #[test]
    fn cursor_offset_mapping_handles_crlf() {
        let text = "{\r\n  \"a\": 1,\r\n  \"b\": [true]\r\n}";
        let state = JsonViewState::from_text(text, false);
        assert_eq!(state.aligned_line_count(), 4);

        // Line 2 starts after two CRLF breaks.
        let line2_start = text.find("  \"b\"").unwrap();
        assert_eq!(state.offset_for_position(2, 0), Some(line2_start));

        // resolve at the "true" literal → deepest is the bool inside b.
        let true_offset = text.find("true").unwrap();
        let chain = state.resolve_offset(true_offset).unwrap();
        let deepest = state.node(*chain.last().unwrap()).unwrap();
        assert_eq!(deepest.kind, JsonKind::Bool);

        // jump_target inverts: the "b" member's value span starts at '['.
        let b_id = {
            let mut s = JsonViewState::from_text(text, false);
            s.set_filter("b".to_string());
            id_of(&s, "b")
        };
        let (line, col) = state.jump_target(b_id).unwrap();
        assert_eq!(line, 2);
        assert_eq!(line2_start + col, text.find('[').unwrap());
    }

    /// Rebuild keeps expansion for paths that still exist (by path-hash) and
    /// falls back to the default when nothing carries over.
    #[test]
    fn rebuild_preserves_expansion_by_path() {
        let mut state = JsonViewState::from_text(NESTED, false);
        let posts = id_of(&state, "posts");
        state.toggle_expanded(posts); // deep expansion beyond the default
        let visible_before = state.visible_rows().len();

        // Same structure, edited scalar: ids shift nowhere, paths identical.
        let edited = NESTED.replace("\"site\"", "\"new site title\"");
        state.rebuild(&edited);
        assert_eq!(
            state.visible_rows().len(),
            visible_before,
            "expansion must survive a value edit"
        );
        let posts_row = state
            .visible_rows()
            .into_iter()
            .find(|row| row_label(row.node) == "posts")
            .unwrap();
        assert!(posts_row.expanded, "posts stays expanded by path");

        // Structure replaced wholesale: nothing to carry, reset to default.
        state.rebuild("{\"completely\": {\"different\": [1, 2]}}");
        let labels: Vec<String> = state
            .visible_rows()
            .iter()
            .map(|row| row_label(row.node))
            .collect();
        assert_eq!(labels, ["$", "completely", "different"]);
    }

    /// A collapsed node the user closed stays closed through a rebuild —
    /// path-hash carry-over is not the default set.
    #[test]
    fn rebuild_keeps_user_collapse() {
        let mut state = JsonViewState::from_text(NESTED, false);
        let db = id_of(&state, "db");
        state.toggle_expanded(db); // user collapses a default-expanded node
        let edited = NESTED.replace('5', "6");
        state.rebuild(&edited);
        let db_row = state
            .visible_rows()
            .into_iter()
            .find(|row| row_label(row.node) == "db")
            .unwrap();
        assert!(!db_row.expanded, "user collapse must survive the rebuild");
    }

    #[test]
    fn path_to_supports_json_path() {
        let mut state = JsonViewState::from_text(NESTED, false);
        state.toggle_expanded(id_of(&state, "posts"));
        state.toggle_expanded(id_of(&state, "[0]"));
        let slug = id_of(&state, "slug");
        let path = state.path_to(slug).unwrap();
        assert_eq!(json_scan::json_path(&path), "db.posts[0].slug");
        let (offset, len) = state.node_span(slug).unwrap();
        assert_eq!(&NESTED[offset..offset + len], "\"first\"");
    }

    /// Unparseable text: no tree, pane stays out of the way, nothing panics.
    #[test]
    fn no_root_means_no_rows() {
        let mut state = JsonViewState::from_text("", false);
        assert!(!state.has_tree());
        assert!(state.visible_rows().is_empty());
        assert_eq!(state.resolve_offset(0), None);
        assert_eq!(state.follow_offset(0), None);
        state.set_filter("x".to_string());
        assert!(state.visible_rows().is_empty());
    }

    /// Huge child lists produce more visible rows than the widget layer
    /// will render — `visible_rows` itself reports everything and the cap
    /// (with its "… N more" row) applies at build time.
    #[test]
    fn wide_arrays_exceed_render_cap() {
        let mut text = String::from("[");
        for i in 0..3000 {
            if i > 0 {
                text.push(',');
            }
            text.push_str(&i.to_string());
        }
        text.push(']');
        let state = JsonViewState::from_text(&text, false);
        assert_eq!(state.tree.truncated, 0, "3001 nodes fit the parse budget");
        // Root + 3000 items visible with the default expansion.
        assert_eq!(state.visible_rows().len(), 3001);
        assert!(state.visible_rows().len() > MAX_RENDERED_ROWS);
    }
}
