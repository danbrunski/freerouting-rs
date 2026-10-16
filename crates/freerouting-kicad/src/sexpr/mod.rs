//! The s-expression layer: [`lexer`] and [`tree`].
//!
//! ```
//! use freerouting_kicad::sexpr::Tree;
//!
//! let tree = Tree::parse(r#"(via (at 1.5 -2) (net "GND"))"#).unwrap();
//! let via = tree.root().item(0).unwrap();
//! assert_eq!(via.head(), Some("via"));
//! assert_eq!(via.child("at").unwrap().f64(2), Some(-2.0));
//! assert_eq!(via.child("net").unwrap().str_value(1).unwrap(), "GND");
//! ```

pub mod lexer;
pub mod tree;

pub use lexer::{Lexer, Token, TokenKind, decode, unescape};
pub use tree::{Items, Node, NodeKind, NodeRef, Span, Tree};
