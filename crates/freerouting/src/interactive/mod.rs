//! Interactive editing as engine code (I series, decision D7: the engine
//! owns behaviour, the GUI owns input).
//!
//! Port of the board logic of Java's `gui.interactive` at `a773a70ec`,
//! without UI types: commands come in board coordinates, results are typed
//! values with Java's message keys ([`Status`]) and drawable data without
//! colours or pixels ([`overlay`]). The GUI maps keys and the mouse to these
//! calls and draws the results.
//!
//! - [`settings`] (I01.1): [`EditSettings`], the engine subset of Java's
//!   `WorkspaceSettings`, and the route rules ([`resolve_route_params`]).
//! - [`route`] (I01.2-I01.5): [`RouteSession`], an interactive route with
//!   push-and-shove, neckdown, SMD snap and layer changes with vias, in
//!   dynamic or stitch mode.
//! - [`overlay`] (I01.6): [`RouteOverlay`], what Java's `Route.draw` shows.
//!
//! Everything is deterministic: Java's time limits are work budgets
//! ([`RouteBudget`]), and the same board and inputs give the same result.

pub mod overlay;
pub mod route;
pub mod settings;

pub use overlay::{LengthEllipse, RouteIncomplete, RouteOverlay};
pub use route::{
    LayerChange, RouteBudget, RouteEvent, RouteInput, RouteMode, RouteSession, RouteStep,
    StartError, StartInfo,
};
pub use settings::{EditSettings, LayerMask, ManualRules, RouteParams, resolve_route_params};

/// A status message: Java's resource key and its arguments. The GUI looks
/// the text up and formats it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub key: &'static str,
    pub args: Vec<String>,
}

impl Status {
    pub fn new<const N: usize>(key: &'static str, args: [String; N]) -> Self {
        Status {
            key,
            args: args.into(),
        }
    }
}

// I02: `move_component` (I02.1), [`MoveComponent`], Java's
// `board.actions.MoveComponent`; `drag` (I02.2, I02.3), [`DragSession`], the
// drag of drill items and components and make-space.
pub mod drag;
pub mod move_component;

pub use drag::{
    DragBudget, DragRelease, DragSession, DragSettings, DragStep, ItemDrag, MakeSpaceDrag,
    find_item_to_move,
};
pub use move_component::{MoveBudget, MoveComponent, SortedItem, requires_drill_item_mover};

// I03: the editor. `editor` (I03.1), [`Editor::apply`] with [`Command`] and
// [`Response`]; `select` (I03.2), the selection queries; `pin_swap` (I03.3);
// `construct` (I03.4), keepouts and holes; `history` (I03.5), undo and redo;
// `edit_ops` (I03.6), delete and fix/unfix of the selection.
pub mod construct;
pub mod edit_ops;
pub mod editor;
pub mod history;
pub mod pin_swap;
pub mod select;

pub use construct::{
    ConstructClick, ConstructOutcome, Construction, KeepoutKind, KeepoutShape, add_hole,
    insert_keepout,
};
pub use edit_ops::{EditOutcome, delete_selection, fix_selection, set_fixed_state_undoable};
pub use editor::{
    Command, ConstructionOverlay, Editor, EditorOptions, ModeKind, Overlay, Response, Tool,
};
pub use history::HistoryStep;
pub use pin_swap::{PinSwap, SwapOutcome};
pub use select::Extent;
