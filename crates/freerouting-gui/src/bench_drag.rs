//! `--bench drag` (G09.7, FT7): the drag tool moves one component back and
//! forth along a fixed path, one pointer step per frame, so the frame time
//! covers the editor's move, the patch of the scene from the change journal
//! and the repaint of the touched chunks.
//!
//! The component is the first placed component whose pad a press drags as a
//! component (`find_item_to_move`); the press is at that pad. The camera
//! stays on the fitted board. Each frame is the pointer path of
//! [`crate::bench::drag_offset`]; a step the board refuses leaves the
//! component where it was, and the drag goes on.

use freerouting::geometry::FloatPoint;
use freerouting::interactive::{
    Command as EditorCommand, DragSettings, EditSettings, Editor, ModeKind, Tool, find_item_to_move,
};

use crate::app::App;
use crate::bench::{WARMUP, drag_offset};
use crate::commands::Command;
use crate::overlays::Frame;
use crate::scene::P;
use crate::tools::ToolMode;

impl App {
    /// The press point for the bench drag in the scene's millimetres: the
    /// first pad of a placed component whose drag moves the component, tried
    /// on a copy of the board (a drag the board refuses would measure
    /// nothing).
    fn bench_drag_press(&self) -> Option<P> {
        let b = self.loaded.as_ref()?;
        let board = &b.session.board;
        let settings = EditSettings::for_board(board.basic());
        let drag = DragSettings::default();
        let frame = Frame::new(board.basic());
        // 0.3 mm in board units
        let step = 0.3 / frame.len(1.0);
        board
            .items()
            .filter(|i| i.is_pin() && i.component_id() > 0)
            .filter_map(|i| i.drill().map(|d| d.center.to_float()))
            .filter(|at| {
                matches!(
                    find_item_to_move(board, &settings, &drag, *at),
                    (Some(id), _) if board.item(id).is_some_and(|i| i.is_pin())
                )
            })
            .take(40)
            .find(|at| {
                let mut editor = Editor::new((**board).clone(), settings.clone());
                editor.apply(EditorCommand::SetTool(Tool::Drag));
                if editor.apply(EditorCommand::Press(*at)).mode != ModeKind::Dragging {
                    return false;
                }
                let to = FloatPoint::new(at.x + step, at.y);
                !editor.apply(EditorCommand::DragTo(to)).delta.is_empty()
            })
            .map(|at| frame.pt(at))
    }

    /// The commands of the coming frame of a `--bench drag` run: choose the
    /// drag tool, press, step along the path, release at the end.
    pub(crate) fn bench_drag_commands(&mut self) -> Vec<Command> {
        let Some(step) = self.scripted.drag_step() else {
            return Vec::new();
        };
        if self.loaded.is_none() || self.view.camera.is_none() {
            return Vec::new();
        }
        let total = self.scripted.bench_total_frames().unwrap_or(usize::MAX);
        // The press comes with the first frame that has a board and a camera.
        let Some(press) = self.bench_press else {
            self.bench_press = self.bench_drag_press();
            return match self.bench_press {
                Some(at) => vec![Command::SetTool(ToolMode::Drag), Command::DragStart(at)],
                None => Vec::new(),
            };
        };
        if step + 1 >= total {
            return vec![Command::DragEnd];
        }
        let offset = drag_offset(step.saturating_sub(WARMUP / 2));
        vec![Command::DragMove(P::new(
            press.x + offset.x,
            press.y + offset.y,
        ))]
    }
}
