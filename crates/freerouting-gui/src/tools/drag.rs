//! Drag mode (G09.3): Java's `DragMenuState.mousePressed` →
//! `DragState.getInstance`, `DragItemState`/`MakeSpaceState.mouseDragged`
//! and the button release, as GUI input.
//!
//! A press, the moves with the button down and the release are the editor
//! commands `Press`, `DragTo` and `Release`. Which item a press picks (the
//! current layer first, then the other visible layers; traces are skipped,
//! components only with "drag components"; the first drill item wins),
//! whether nothing movable under the cursor means make-space, and how the
//! moves shove are I02's. The scene is patched from the journal after each
//! move (`tools::host`), so a drag rebuilds only the chunks it touches.

use freerouting::interactive::Command as EditorCommand;

use crate::app::App;
use crate::scene::P;
use crate::tools::ToolMode;

impl App {
    /// True when the left button drags: the tool is Drag, nothing is
    /// selected and no other session is under way.
    pub(crate) fn drag_tool_active(&self) -> bool {
        self.tools.mode == ToolMode::Drag && self.selection.is_empty() && !self.route_like_busy()
    }

    /// A session other than a drag is under way.
    fn route_like_busy(&self) -> bool {
        use freerouting::interactive::ModeKind;
        self.editor_busy()
            && !matches!(
                self.editor_mode(),
                Some(ModeKind::Dragging | ModeKind::MakingSpace)
            )
    }

    /// The left button went down at `at`.
    pub(crate) fn drag_start(&mut self, at: P) {
        self.tools.popup = None;
        if !self.drag_tool_active() || self.tools.keepout.is_some() {
            return;
        }
        let Some(point) = self.board_point_of(at) else {
            return;
        };
        self.sync_tool();
        self.editor_apply(EditorCommand::Press(point));
    }

    /// The pointer moved with the button down.
    pub(crate) fn drag_move(&mut self, at: P) {
        use freerouting::interactive::ModeKind;
        if !matches!(
            self.editor_mode(),
            Some(ModeKind::Dragging | ModeKind::MakingSpace)
        ) {
            return;
        }
        if let Some(point) = self.board_point_of(at) {
            self.editor_apply(EditorCommand::DragTo(point));
        }
    }

    /// The button went up.
    pub(crate) fn drag_end(&mut self) {
        use freerouting::interactive::ModeKind;
        if matches!(
            self.editor_mode(),
            Some(ModeKind::Dragging | ModeKind::MakingSpace)
        ) {
            self.editor_apply(EditorCommand::Release);
        }
    }
}
