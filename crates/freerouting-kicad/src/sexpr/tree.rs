//! The arena tree.
//!
//! One `Vec<Node>` in pre-order plus one `Vec<u32>` of child indices. Atoms
//! and strings are byte spans into the source (borrowed, never copied);
//! strings stay escaped until [`NodeRef::str_value`] is called, numbers stay
//! text until [`NodeRef::f64`] is called. The parser uses an explicit stack, so
//! nesting depth cannot overflow the thread stack.
//!
//! Node 0 is an implicit list holding the top-level items of the file, so a
//! board is `tree.root().child("kicad_pcb")`.
//!
//! Children are kept in file order and every accessor iterates in file order;
//! K05 and K06 rely on that to reproduce KiCad's item order.
//!
//! The legacy conversions ([`crate::legacy`]) add synthesised atoms to a side
//! buffer. A span whose start lies at or beyond `src.len()` refers to that
//! buffer, so one `u32` pair addresses both.

use std::borrow::Cow;

use super::lexer::{Lexer, TokenKind, unescape};
use crate::error::{KicadError, line_col};

/// A byte range of the source (or of the synthesised-text buffer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

/// One node of the arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Node {
    /// A list; its items are `children[first..first + len]`.
    List { first: u32, len: u32 },
    /// A bare atom (symbol or number).
    Atom(Span),
    /// A quoted string; the span excludes the quotes and is still escaped.
    Str(Span),
}

/// The three node kinds, without their payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    List,
    Atom,
    Str,
}

/// A parsed s-expression file.
#[derive(Clone, Debug)]
pub struct Tree<'a> {
    src: &'a str,
    extra: String,
    nodes: Vec<Node>,
    children: Vec<u32>,
}

impl<'a> Tree<'a> {
    /// Parses `src`. Errors are [`crate::ErrorKind::Syntax`] with the position of the
    /// offending token.
    pub fn parse(src: &'a str) -> Result<Tree<'a>, KicadError> {
        let guess = src.len() / 6 + 16;
        let mut nodes: Vec<Node> = Vec::with_capacity(guess);
        let mut children: Vec<u32> = Vec::with_capacity(guess);
        let mut pending: Vec<u32> = Vec::new();
        // (list node index, start of its pending children)
        let mut stack: Vec<(u32, u32)> = Vec::new();
        nodes.push(Node::List { first: 0, len: 0 });
        stack.push((0, 0));
        for token in Lexer::new(src) {
            let t = token?;
            match t.kind {
                TokenKind::Open => {
                    let idx = nodes.len() as u32;
                    nodes.push(Node::List { first: 0, len: 0 });
                    pending.push(idx);
                    stack.push((idx, pending.len() as u32));
                }
                TokenKind::Close => {
                    // The lexer rejects a ')' at depth 0, so the root frame stays.
                    let (idx, from) = stack.pop().unwrap_or((0, 0));
                    let first = children.len() as u32;
                    children.extend(pending.drain(from as usize..));
                    nodes[idx as usize] = Node::List {
                        first,
                        len: children.len() as u32 - first,
                    };
                }
                TokenKind::Atom | TokenKind::Str => {
                    let idx = nodes.len() as u32;
                    let span = Span {
                        start: t.start,
                        end: t.end,
                    };
                    nodes.push(if t.kind == TokenKind::Atom {
                        Node::Atom(span)
                    } else {
                        Node::Str(span)
                    });
                    pending.push(idx);
                }
            }
        }
        let first = children.len() as u32;
        children.append(&mut pending);
        nodes[0] = Node::List {
            first,
            len: children.len() as u32 - first,
        };
        Ok(Tree {
            src,
            extra: String::new(),
            nodes,
            children,
        })
    }

    /// The source text this tree borrows.
    pub fn source(&self) -> &'a str {
        self.src
    }

    /// The implicit list of top-level items.
    pub fn root(&self) -> NodeRef<'_> {
        NodeRef { tree: self, idx: 0 }
    }

    /// Node `idx` (an index into the arena, pre-order for parsed nodes).
    pub fn node(&self, idx: u32) -> NodeRef<'_> {
        NodeRef { tree: self, idx }
    }

    /// Number of nodes, the implicit root included.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True when the file had no tokens at all.
    pub fn is_empty(&self) -> bool {
        self.nodes.len() == 1 && self.children.is_empty()
    }

    fn text(&self, span: Span) -> &str {
        let (s, e) = (span.start as usize, span.end as usize);
        if s >= self.src.len() {
            let base = self.src.len();
            &self.extra[s - base..e - base]
        } else {
            &self.src[s..e]
        }
    }

    // ---- mutation, for the legacy conversions -------------------------

    /// Appends a synthesised atom and returns its index.
    pub(crate) fn add_atom(&mut self, text: &str) -> u32 {
        let span = self.add_text(text);
        self.nodes.push(Node::Atom(span));
        self.nodes.len() as u32 - 1
    }

    /// Appends a synthesised string (given unescaped; stored escaped) and
    /// returns its index.
    pub(crate) fn add_str(&mut self, unescaped: &str) -> u32 {
        let mut esc = String::with_capacity(unescaped.len());
        for c in unescaped.chars() {
            match c {
                '"' => esc.push_str("\\\""),
                '\\' => esc.push_str("\\\\"),
                '\n' => esc.push_str("\\n"),
                '\r' => esc.push_str("\\r"),
                '\t' => esc.push_str("\\t"),
                c => esc.push(c),
            }
        }
        let span = self.add_text(&esc);
        self.nodes.push(Node::Str(span));
        self.nodes.len() as u32 - 1
    }

    fn add_text(&mut self, text: &str) -> Span {
        let base = self.src.len();
        let start = base + self.extra.len();
        self.extra.push_str(text);
        Span {
            start: start as u32,
            end: (start + text.len()) as u32,
        }
    }

    /// Replaces the items of list `list` with `items` (appended to the child
    /// array; the old run becomes unreachable).
    pub(crate) fn set_items(&mut self, list: u32, items: &[u32]) {
        let first = self.children.len() as u32;
        self.children.extend_from_slice(items);
        self.nodes[list as usize] = Node::List {
            first,
            len: items.len() as u32,
        };
    }

    /// Appends a synthesised list with the given items and returns its index.
    pub(crate) fn add_list(&mut self, items: &[u32]) -> u32 {
        self.nodes.push(Node::List { first: 0, len: 0 });
        let idx = self.nodes.len() as u32 - 1;
        self.set_items(idx, items);
        idx
    }

    /// Item indices of list `list` (empty for other nodes).
    pub(crate) fn item_indices(&self, list: u32) -> Vec<u32> {
        match self.nodes[list as usize] {
            Node::List { first, len } => {
                self.children[first as usize..first as usize + len as usize].to_vec()
            }
            _ => Vec::new(),
        }
    }
}

impl PartialEq for Tree<'_> {
    /// Structural equality: same shape, same kinds, same token text. Spans
    /// and whitespace do not matter.
    fn eq(&self, other: &Self) -> bool {
        let mut work: Vec<(u32, u32)> = vec![(0, 0)];
        while let Some((a, b)) = work.pop() {
            let (na, nb) = (self.node(a), other.node(b));
            if na.kind() != nb.kind() {
                return false;
            }
            match na.kind() {
                NodeKind::List => {
                    if na.len() != nb.len() {
                        return false;
                    }
                    work.extend(na.items().zip(nb.items()).map(|(x, y)| (x.idx, y.idx)));
                }
                _ => {
                    if na.text() != nb.text() {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// A cheap handle on one node. `Copy`; all text it returns borrows the tree.
#[derive(Clone, Copy, Debug)]
pub struct NodeRef<'a> {
    tree: &'a Tree<'a>,
    idx: u32,
}

/// Iterator over the items of a list, in file order.
#[derive(Clone, Debug)]
pub struct Items<'a> {
    tree: &'a Tree<'a>,
    it: std::slice::Iter<'a, u32>,
}

impl<'a> Iterator for Items<'a> {
    type Item = NodeRef<'a>;
    fn next(&mut self) -> Option<NodeRef<'a>> {
        self.it.next().map(|&idx| NodeRef {
            tree: self.tree,
            idx,
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.it.size_hint()
    }
}

impl DoubleEndedIterator for Items<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.it.next_back().map(|&idx| NodeRef {
            tree: self.tree,
            idx,
        })
    }
}

impl ExactSizeIterator for Items<'_> {}

impl<'a> NodeRef<'a> {
    /// The arena index.
    pub fn index(&self) -> u32 {
        self.idx
    }

    fn node(&self) -> Node {
        self.tree.nodes[self.idx as usize]
    }

    pub fn kind(&self) -> NodeKind {
        match self.node() {
            Node::List { .. } => NodeKind::List,
            Node::Atom(_) => NodeKind::Atom,
            Node::Str(_) => NodeKind::Str,
        }
    }

    pub fn is_list(&self) -> bool {
        self.kind() == NodeKind::List
    }

    /// Raw token text of an atom or string (a string is still escaped); empty
    /// for a list.
    pub fn text(&self) -> &'a str {
        match self.node() {
            Node::Atom(s) | Node::Str(s) => self.tree.text(s),
            Node::List { .. } => "",
        }
    }

    /// Number of items of a list; 0 for an atom or string.
    pub fn len(&self) -> usize {
        match self.node() {
            Node::List { len, .. } => len as usize,
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The items of a list in file order (empty for an atom or string).
    pub fn items(&self) -> Items<'a> {
        let slice: &'a [u32] = match self.node() {
            Node::List { first, len } => {
                &self.tree.children[first as usize..first as usize + len as usize]
            }
            _ => &[],
        };
        Items {
            tree: self.tree,
            it: slice.iter(),
        }
    }

    /// Item `i` (0 is the head).
    pub fn item(&self, i: usize) -> Option<NodeRef<'a>> {
        match self.node() {
            Node::List { first, len } if i < len as usize => Some(NodeRef {
                tree: self.tree,
                idx: self.tree.children[first as usize + i],
            }),
            _ => None,
        }
    }

    /// The head of a list: its first item when that is a bare atom.
    pub fn head(&self) -> Option<&'a str> {
        let first = self.item(0)?;
        match first.node() {
            Node::Atom(s) => Some(self.tree.text(s)),
            _ => None,
        }
    }

    /// The first child list whose head is `name`.
    pub fn child(&self, name: &str) -> Option<NodeRef<'a>> {
        self.items().find(|c| c.is_list() && c.head() == Some(name))
    }

    /// All child lists whose head is `name`, in file order.
    pub fn children<'n>(&self, name: &'n str) -> impl Iterator<Item = NodeRef<'a>> + use<'a, 'n> {
        self.items()
            .filter(move |c| c.is_list() && c.head() == Some(name))
    }

    /// Item `i` as a bare atom (0 is the head, so `(xy 1 2)` has atoms 0, 1, 2).
    pub fn atom(&self, i: usize) -> Option<&'a str> {
        let it = self.item(i)?;
        match it.node() {
            Node::Atom(s) => Some(self.tree.text(s)),
            _ => None,
        }
    }

    /// Raw text of item `i`, atom or string (a string is still escaped).
    pub fn raw(&self, i: usize) -> Option<&'a str> {
        let it = self.item(i)?;
        match it.node() {
            Node::List { .. } => None,
            _ => Some(it.text()),
        }
    }

    /// Item `i` as a string value: a quoted string unescaped (KiCad's rules),
    /// a bare atom as is. KiCad accepts both for names.
    pub fn str_value(&self, i: usize) -> Option<Cow<'a, str>> {
        let it = self.item(i)?;
        match it.node() {
            Node::Str(s) => Some(unescape(self.tree.text(s))),
            Node::Atom(s) => Some(Cow::Borrowed(self.tree.text(s))),
            Node::List { .. } => None,
        }
    }

    /// Item `i` parsed as a number. Only bare atoms count.
    pub fn f64(&self, i: usize) -> Option<f64> {
        self.atom(i)?.parse::<f64>().ok()
    }

    /// Byte offset of this node in the source, or `None` when unknown
    /// (synthesised nodes, empty lists, the root).
    fn offset(&self) -> Option<usize> {
        let src = self.tree.src;
        let offset = match self.node() {
            Node::Atom(s) => s.start as usize,
            Node::Str(s) => (s.start as usize).checked_sub(1)?,
            Node::List { len: 0, .. } => return None,
            Node::List { .. } => {
                // The '(' is the last non-whitespace byte before the first item.
                let first = self.item(0)?;
                let start = match first.node() {
                    Node::Atom(s) => s.start as usize,
                    Node::Str(s) => (s.start as usize).checked_sub(1)?,
                    // The first item is itself a list: its '(' is the one we want
                    // minus one level; report the inner list's position instead.
                    Node::List { .. } => return first.offset(),
                };
                let bytes = src.as_bytes();
                let mut i = start.min(bytes.len());
                while i > 0 && bytes[i - 1] <= b' ' {
                    i -= 1;
                }
                i.checked_sub(1)?
            }
        };
        (offset < src.len()).then_some(offset)
    }

    /// 1-based `(line, column)` of this node, or `(0, 0)` when unknown. Counts
    /// from the start of the file, so use it for messages only.
    pub fn position(&self) -> (u32, u32) {
        match self.offset() {
            Some(o) => line_col(self.tree.src, o),
            None => (0, 0),
        }
    }

    /// 1-based source line of this node, or 0 when unknown.
    pub fn line(&self) -> u32 {
        self.position().0
    }

    /// An error of `kind` positioned at this node.
    pub fn error(&self, kind: crate::error::ErrorKind, message: impl Into<String>) -> KicadError {
        let (line, column) = self.position();
        KicadError {
            kind,
            line,
            column,
            message: message.into(),
        }
    }
}
