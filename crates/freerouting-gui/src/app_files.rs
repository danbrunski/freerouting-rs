//! `App`'s side of saving, the prompts and the front-end mode (G08). The
//! writers are in [`crate::files`], the countdown in
//! [`crate::dialogs::autostart`].
//!
//! **Save As** (`Ctrl+S`, File > Save as...) is Java's only save: it opens the
//! file dialog in save mode, writes a session or a design, and after a session
//! asks `confirm_rules_save` (default No) before writing `<output>.rules`.
//!
//! **Prompts.** At most one question is shown at a time ([`Prompt`]):
//! - *Exit* (`confirm_cancel`) when the window is closed with a changed board;
//! - *Unsaved changes* (`confirm_save_changes`) before an open or a drop
//!   replaces a changed board; "Save" opens Save As and loads the new file
//!   after a successful save (DIVERGENCES: Java proceeds without saving);
//! - *Save rules* after a session was saved;
//! - *Import rules* (`confirm_import_rules`) at start when `<design>.rules`
//!   sits next to the design.
//!
//! **Front-end mode** (`-de board.dsn -do out.ses`, what the KiCad plugin's
//! "Route in GUI" starts): the stale output is deleted at start, a countdown
//! starts routing, the output is written when the job ends (completed or
//! stopped) and the app exits with 0 for a written file and 1 for a failed
//! write. Closing the window stops the job and writes the output when the
//! board changed or has traces, without a prompt. An app started without `-do`
//! has none of this.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};

use eframe::egui;
use freerouting::io::specctra::read_rules;

use crate::app::App;
use crate::args::{Args, FrontEndOptions};
use crate::commands::{Answer, Command};
use crate::dialogs::autostart::{Autostart, Choice};
use crate::files::{self, SaveKind};
use crate::panels::log::Level;
use crate::text;

/// The process exit code the front-end mode asks for: 0, or 1 once a write
/// failed. `main` reads it after the window closed.
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

/// The exit code of the process (see [`EXIT_CODE`]).
pub fn exit_code() -> i32 {
    EXIT_CODE.load(Ordering::SeqCst)
}

/// A question the app is asking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// The window is closing and the board has unsaved changes.
    Exit,
    /// A file is about to replace a board with unsaved changes.
    UnsavedBeforeLoad,
    /// A session was saved to `output`; also save the rules?
    SaveRules { output: PathBuf },
    /// `path` holds rules for the design just opened; import them?
    ImportRules { path: PathBuf },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Startup {
    /// The rules file next to the design (or `-dr`) is looked at.
    Rules,
    /// The auto-start countdown.
    AutoStart,
}

/// The front-end mode's state.
#[derive(Default)]
pub(crate) struct FrontEnd {
    pub options: FrontEndOptions,
    /// Set by Start Now (or a `-dct` of 0); a later Finished exits.
    pub exit_when_finished: bool,
    /// The window is being closed and the output is written first.
    pub closing: bool,
}

/// Everything G08 keeps in `App`.
#[derive(Default)]
pub(crate) struct FilesState {
    pub prompt: Option<Prompt>,
    /// The last file Save As wrote, the default of the next one.
    pub last_output: Option<PathBuf>,
    /// Files waiting for the unsaved-changes prompt or the save it asked for.
    pub pending_load: Option<Vec<PathBuf>>,
    /// A close request was let through, or sent by the app.
    pub close_allowed: bool,
    /// The app has asked the window to close (tests read this).
    pub close_sent: bool,
    startup: Option<Startup>,
    pub frontend: FrontEnd,
    pub autostart: Option<Autostart>,
    /// 1 once a write or the load failed in front-end mode.
    pub exit_code: i32,
}

/// The lower-case extension of `path`, empty if none.
fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

impl App {
    /// Sets up the front-end state from the command line (called by
    /// `App::build` before the load starts).
    pub(crate) fn init_files(&mut self, args: &Args) {
        self.files.frontend.options = args.frontend.clone();
        for warning in &args.frontend.router_warnings {
            self.log.push(Level::Warning, warning.clone());
        }
        if args.open.is_empty() {
            return;
        }
        // A scripted run (bench, screenshot) asks nothing.
        if !args.scripted() {
            self.files.startup = Some(Startup::Rules);
        }
        // Java deletes an existing output before starting, so a stale file is
        // never mistaken for the result.
        if let Some(output) = &args.frontend.output
            && output.exists()
            && let Err(e) = std::fs::remove_file(output)
        {
            self.log.push(
                Level::Warning,
                format!("Couldn't delete the file '{}': {e}", output.display()),
            );
        }
    }

    /// The question being asked, if any.
    pub fn prompt(&self) -> Option<&Prompt> {
        self.files.prompt.as_ref()
    }

    /// The text of the auto-start countdown's Start Now button, while the
    /// countdown is shown.
    pub fn autostart_label(&self) -> Option<&str> {
        self.files.autostart.as_ref().map(Autostart::label)
    }

    /// The exit code the front-end mode asks for: 0, or 1 once its write or
    /// its load failed. (The process-wide [`exit_code`] is the same value.)
    pub fn frontend_exit_code(&self) -> i32 {
        self.files.exit_code
    }

    fn exit_failed(&mut self) {
        self.files.exit_code = 1;
        EXIT_CODE.store(1, Ordering::SeqCst);
    }

    /// True once the app has asked the window to close.
    pub fn close_sent(&self) -> bool {
        self.files.close_sent
    }

    /// Whether the app exits when the job it starts ends (front-end mode).
    pub fn exit_when_finished(&self) -> bool {
        self.files.frontend.exit_when_finished
    }

    /// True if the save dialog is open.
    pub fn is_saving_dialog_open(&self) -> bool {
        self.dialog.is_saving()
    }

    /// The save dialog's file name field (tests).
    pub fn save_dialog_name(&self) -> &str {
        self.dialog.file_name()
    }

    /// True if the board differs from the loaded or saved one, or a job is
    /// running (its result is not saved yet).
    pub fn is_board_changed(&self) -> bool {
        self.routing.is_some() || self.loaded.as_ref().is_some_and(|b| b.session.is_changed())
    }

    // ---- Save As -------------------------------------------------------

    /// File > Save as...: the file dialog in save mode.
    pub(crate) fn save_as_dialog(&mut self) {
        let Some(b) = &self.loaded else {
            self.notice = Some(text::OPEN_A_BOARD.to_string());
            return;
        };
        if self.routing.is_some() {
            self.notice = Some(text::SAVE_WHILE_ROUTING.to_string());
            return;
        }
        let default =
            files::default_save_path(self.files.last_output.as_deref(), &b.session.paths.design);
        self.dialog.show_save(&default);
    }

    /// Writes the session or design to `path` and, after a session, asks about
    /// the rules.
    pub(crate) fn save_as(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.routing.is_some() {
            self.notice = Some(text::SAVE_WHILE_ROUTING.to_string());
            return;
        }
        let Some(kind) = SaveKind::of_path(&path) else {
            self.notice = Some(text::fill(
                text::SAVE_UNSUPPORTED_EXTENSION,
                &[&path.to_string_lossy()],
            ));
            self.files.pending_load = None;
            return;
        };
        // The save dialog, if the path came from it, is done.
        if self.dialog.is_saving() {
            self.dialog.open = false;
        }
        let Some(b) = self.loaded.as_mut() else {
            return;
        };
        let name = files::save_design_name(&b.session.paths.design);
        match files::save_as(&b.session.board, &name, &path, kind) {
            Ok(size) => {
                b.session.mark_saved();
                let message = match kind {
                    SaveKind::Ses => {
                        text::fill(text::MESSAGE_SPECCTRA_SES_SAVED, &[&path.to_string_lossy()])
                    }
                    SaveKind::Dsn => text::SAVE_MESSAGE.to_string(),
                };
                self.log
                    .push(Level::Info, format!("{message} ({size} bytes)"));
                self.notice = Some(message);
                self.files.last_output = Some(path.clone());
                if kind == SaveKind::Ses {
                    self.files.prompt = Some(Prompt::SaveRules { output: path });
                }
                self.proceed_pending(ctx);
            }
            Err(e) => {
                self.log.push(Level::Error, e.to_string());
                self.notice = Some(e.message);
                self.files.pending_load = None;
            }
        }
    }

    /// Writes `<output>.rules` (`BoardExportActions.saveRulesAs`).
    fn save_rules_for(&mut self, output: &Path) {
        let Some(b) = &self.loaded else { return };
        let rules = files::rules_file_for(output);
        let name = files::save_design_name(&b.session.paths.design);
        match files::save_rules(&b.session.board, &name, &rules) {
            Ok(_) => {
                let message = text::fill(text::MESSAGE_RULES_SAVED, &[&rules.to_string_lossy()]);
                self.log.push(Level::Info, message.clone());
                self.notice = Some(message);
            }
            Err(e) => {
                self.log.push(Level::Error, e.to_string());
                self.notice = Some(e.message);
            }
        }
    }

    // ---- Unsaved changes ------------------------------------------------

    /// Opens `paths` unless the board has unsaved changes, in which case the
    /// question is asked first (`BoardFrame.confirmSaveBeforeLoad`).
    pub(crate) fn open_checked(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        if self.loaded.is_some() && self.is_board_changed() {
            self.files.pending_load = Some(paths);
            self.files.prompt = Some(Prompt::UnsavedBeforeLoad);
            return;
        }
        let current = self.current();
        self.start_load(ctx, paths, current);
    }

    /// Loads the files that waited for a question, once none is open.
    fn proceed_pending(&mut self, ctx: &egui::Context) {
        if self.files.prompt.is_some() || self.dialog.is_saving() {
            return;
        }
        if let Some(paths) = self.files.pending_load.take() {
            let current = self.current();
            self.start_load(ctx, paths, current);
        }
    }

    /// Runs the files commands. The caller passes only those.
    pub(crate) fn execute_files(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::SaveAsDialog => self.save_as_dialog(),
            Command::SaveAs(path) => self.save_as(ctx, path),
            Command::SaveCancelled => self.files.pending_load = None,
            Command::AnswerPrompt(answer) => self.answer_prompt(ctx, answer),
            _ => {}
        }
    }

    fn answer_prompt(&mut self, ctx: &egui::Context, answer: Answer) {
        let Some(prompt) = self.files.prompt.take() else {
            return;
        };
        match (prompt, answer) {
            (Prompt::Exit, Answer::Yes) => self.close_window(ctx),
            (Prompt::UnsavedBeforeLoad, Answer::Yes) => {
                // Save, and load the new file once the save succeeded.
                self.save_as_dialog();
                if !self.dialog.is_saving() {
                    self.files.pending_load = None;
                }
            }
            (Prompt::UnsavedBeforeLoad, Answer::No) => self.proceed_pending(ctx),
            (Prompt::UnsavedBeforeLoad, _) => self.files.pending_load = None,
            (Prompt::SaveRules { output }, Answer::Yes) => {
                self.save_rules_for(&output);
                self.proceed_pending(ctx);
            }
            (Prompt::SaveRules { .. }, _) => self.proceed_pending(ctx),
            (Prompt::ImportRules { path }, Answer::Yes) => self.import_rules(&path),
            _ => {}
        }
    }

    /// Lets the next close request through and asks the window to close.
    fn close_window(&mut self, ctx: &egui::Context) {
        self.files.close_allowed = true;
        self.files.close_sent = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    // ---- The frame -------------------------------------------------------

    /// Everything G08 does in a frame: the close request, the start-up steps,
    /// the questions and the countdown.
    pub(crate) fn files_frame(&mut self, ctx: &egui::Context) -> Vec<Command> {
        self.close_request(ctx);
        self.advance_startup(ctx);
        let mut commands = self.prompt_ui(ctx);
        if let Some(picked) = self.dialog.ui(ctx) {
            use crate::file_dialog::Picked;
            commands.push(match picked {
                Picked::Open(path) => Command::Open(path),
                Picked::Save(path) => Command::SaveAs(path),
                Picked::Cancelled => Command::SaveCancelled,
            });
        }
        if let Some(choice) = self.autostart_step(ctx) {
            self.autostart_chosen(ctx, choice);
        }
        commands
    }

    /// The window's close button (and Quit): ask first if the board changed.
    fn close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.files.close_allowed {
            return;
        }
        if self.files.frontend.options.output.is_some() && self.loaded.is_some() {
            // Java's `windowClosing`: stop the router and write the output if
            // the board changed or has traces; no question.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.files.frontend.closing = true;
            if self.routing.is_some() {
                self.stop_routing();
                return;
            }
            self.write_output_on_close();
            self.close_window(ctx);
            return;
        }
        if self.loaded.is_some() && self.is_board_changed() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.files.prompt = Some(Prompt::Exit);
        }
    }

    fn write_output_on_close(&mut self) {
        let Some(output) = self.files.frontend.options.output.clone() else {
            return;
        };
        let Some(b) = &self.loaded else { return };
        if b.session.is_changed() || !b.session.board.traces().is_empty() {
            self.write_output(&output);
        }
    }

    /// Writes the front-end output and records the result. Returns whether it
    /// was written.
    fn write_output(&mut self, output: &Path) -> bool {
        let Some(b) = self.loaded.as_mut() else {
            return false;
        };
        match files::export_output(&b.session.board, output) {
            Ok(size) => {
                b.session.mark_saved();
                let message = format!("Saved {size} bytes to '{}'.", output.display());
                self.log.push(Level::Info, message.clone());
                self.notice = Some(message);
                true
            }
            Err(message) => {
                self.exit_failed();
                self.log.push(Level::Error, message.clone());
                self.notice = Some(message);
                false
            }
        }
    }

    /// The job ended (`finish_routing`, `routing_failed`): in front-end mode
    /// write the output, and exit if that is what is wanted.
    pub(crate) fn frontend_job_ended(&mut self, ctx: &egui::Context) {
        let Some(output) = self.files.frontend.options.output.clone() else {
            return;
        };
        if self.loaded.is_none() {
            return;
        }
        self.write_output(&output);
        if self.files.frontend.exit_when_finished || self.files.frontend.closing {
            self.close_window(ctx);
        }
    }

    // ---- Start-up --------------------------------------------------------

    /// Steps the start-up sequence once the board is open and no question is
    /// open: the rules next to the design, then the auto-start.
    fn advance_startup(&mut self, ctx: &egui::Context) {
        if self.files.startup.is_none() {
            return;
        }
        if self.loading.is_some() {
            return;
        }
        if self.loaded.is_none() {
            // The load failed: the front-end mode has nothing to start.
            if self.error.is_some() {
                self.files.startup = None;
                if self.files.frontend.options.output.is_some() {
                    self.exit_failed();
                }
            }
            return;
        }
        if self.files.prompt.is_some() {
            return;
        }
        match self.files.startup {
            Some(Startup::Rules) => {
                self.files.startup = Some(Startup::AutoStart);
                self.startup_rules();
            }
            Some(Startup::AutoStart) => {
                self.files.startup = None;
                self.startup_autostart(ctx);
            }
            None => {}
        }
    }

    /// `GuiManager.createBoardFrame`: for a DSN input, `-dr <file>` is read
    /// without asking, else `<design name>.rules` beside the design is read
    /// after the user confirms (File > Open does not look for it, as in Java).
    fn startup_rules(&mut self) {
        let Some(b) = &self.loaded else { return };
        let design = b.session.paths.design.clone();
        let from_host = b
            .session
            .board
            .communication
            .specctra_parser_info
            .as_ref()
            .is_none_or(|info| info.dsn_file_generated_by_host);
        if extension(&design) != "dsn" {
            return;
        }
        if let Some(path) = self.files.frontend.options.rules.clone() {
            if path.is_file() {
                self.import_rules(&path);
            } else {
                self.log.push(
                    Level::Warning,
                    text::fill(text::RULES_NOT_FOUND, &[&path.to_string_lossy()]),
                );
            }
            return;
        }
        let stem = design
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let adjacent = design.with_file_name(format!("{stem}.rules"));
        if !adjacent.is_file() || !from_host {
            return;
        }
        let options = &self.files.frontend.options;
        if options.auto_start() && options.confirmation_timeout == 0 {
            // `-dct 0` asks for a run without questions; the CLI reads the
            // adjacent rules file as well.
            self.import_rules(&adjacent);
        } else {
            self.files.prompt = Some(Prompt::ImportRules { path: adjacent });
        }
    }

    /// Reads a rules file into the open board (`RulesReader.read`): the
    /// clearance matrix, net classes, vias and via rules; a rules file's
    /// `autoroute_settings` become the settings of the next job.
    pub(crate) fn import_rules(&mut self, path: &Path) {
        let Some(b) = &self.loaded else { return };
        let design_name = b
            .session
            .paths
            .design
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                let message = text::fill(
                    text::RULES_IMPORT_FAILED,
                    &[&path.to_string_lossy(), &e.to_string()],
                );
                self.log.push(Level::Error, message.clone());
                self.notice = Some(message);
                return;
            }
        };
        // The settings the next job would have started with (the DSN's), so
        // that the rules' autoroute settings go on top of them.
        let baseline = self.autoroute_patch();
        let Some(b) = self.loaded.as_mut() else {
            return;
        };
        let rules_text = freerouting::dsn::decode(&bytes);
        let (board, result) = read_rules((*b.session.board).clone(), &rules_text, &design_name);
        match result {
            Ok(outcome) => {
                b.session.board = std::sync::Arc::new(board);
                b.session.edit_generation += 1;
                for line in &outcome.log {
                    self.log.push(Level::Warning, line.text.clone());
                }
                if let (Some(mut patch), Some(rules)) = (baseline, outcome.autoroute) {
                    patch.apply_from(&rules);
                    self.autoroute.applied = Some(patch);
                }
                let message = text::fill(text::RULES_IMPORTED, &[&path.to_string_lossy()]);
                self.log.push(Level::Info, message.clone());
                self.notice = Some(message);
                self.after_board_edit();
            }
            Err(e) => {
                let message = text::fill(
                    text::RULES_IMPORT_FAILED,
                    &[&path.to_string_lossy(), &e.to_string()],
                );
                self.log.push(Level::Error, message.clone());
                self.notice = Some(message);
            }
        }
    }

    /// With an input and an output file: the countdown, or at once for `-dct 0`.
    fn startup_autostart(&mut self, ctx: &egui::Context) {
        let options = &self.files.frontend.options;
        if !options.auto_start() {
            return;
        }
        if options.confirmation_timeout == 0 {
            self.files.frontend.exit_when_finished = true;
            self.execute(ctx, Command::StartRouting);
        } else {
            self.files.autostart = Some(Autostart::new(options.confirmation_timeout));
        }
    }

    fn autostart_step(&mut self, ctx: &egui::Context) -> Option<Choice> {
        // The countdown waits for questions and runs only with a board.
        if self.files.prompt.is_some() || self.loaded.is_none() {
            return None;
        }
        self.files.autostart.as_mut()?.ui(ctx)
    }

    fn autostart_chosen(&mut self, ctx: &egui::Context, choice: Choice) {
        self.files.autostart = None;
        match choice {
            Choice::StartNow => {
                self.files.frontend.exit_when_finished = true;
                self.execute(ctx, Command::StartRouting);
            }
            Choice::Cancel => self.files.frontend.exit_when_finished = false,
        }
    }

    // ---- The questions' windows ------------------------------------------

    fn prompt_ui(&mut self, ctx: &egui::Context) -> Vec<Command> {
        let Some(prompt) = self.files.prompt.clone() else {
            return Vec::new();
        };
        let (title, message, buttons): (&str, &str, &[(&str, Answer)]) = match &prompt {
            Prompt::Exit => (
                text::CONFIRM_EXIT_TITLE,
                text::CONFIRM_EXIT,
                &[
                    (text::CONFIRM_EXIT_YES, Answer::Yes),
                    (text::CONFIRM_EXIT_NO, Answer::No),
                ],
            ),
            Prompt::UnsavedBeforeLoad => (
                text::CONFIRM_SAVE_TITLE,
                text::CONFIRM_SAVE_CHANGES,
                &[
                    (text::CONFIRM_SAVE_YES, Answer::Yes),
                    (text::CONFIRM_SAVE_NO, Answer::No),
                    (text::CONFIRM_SAVE_CANCEL, Answer::Cancel),
                ],
            ),
            Prompt::SaveRules { .. } => (
                text::SELECT_AN_OPTION,
                text::CONFIRM_RULES_SAVE,
                &[
                    (text::CONFIRM_EXIT_YES, Answer::Yes),
                    (text::CONFIRM_EXIT_NO, Answer::No),
                ],
            ),
            Prompt::ImportRules { .. } => (
                text::SELECT_AN_OPTION,
                text::CONFIRM_IMPORT_RULES,
                &[
                    (text::CONFIRM_EXIT_YES, Answer::Yes),
                    (text::CONFIRM_EXIT_NO, Answer::No),
                ],
            ),
        };
        let mut out = Vec::new();
        egui::Window::new(title)
            .id(egui::Id::new("files-prompt"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(440.0);
                ui.label(message);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for (label, answer) in buttons {
                        if ui.button(*label).clicked() {
                            out.push(Command::AnswerPrompt(*answer));
                        }
                    }
                });
            });
        out
    }
}
