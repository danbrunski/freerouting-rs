//! The main window: menu, side panel, status bar and board canvas.
//!
//! Every action is a [`Command`] run by [`App::execute`]; the panels, the
//! menu and the keys only produce commands.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use eframe::egui;

use crate::analysis::Analysis;
use crate::args::Args;
use crate::capture::{FrameReport, Scripted};
use crate::commands::Command;
use crate::drc_export::{ExportDialog, ExportJob};
use crate::file_dialog::FileDialog;
use crate::input;
use crate::lists::Lists;
use crate::look::{Object, VIRTUAL_LAYERS, virtual_name};
use crate::overlays::OverlayState;
use crate::panels::log::{Level, Log};
use crate::panels::select_settings::SelectionSettings;
use crate::panels::warnings;
use crate::scene::Kind;
use crate::selection::Selection;
use crate::session::{self, CurrentDesign, Loaded};
use crate::settings::PersistedSettings;
use crate::stats::{FrameStats, OverlayCounts, PaintStats};
use crate::tools::Tools;
use crate::viewport::ViewState;
use crate::windows::Windows;

type LoadResult = Result<Loaded, String>;

pub struct App {
    pub(crate) loaded: Option<Loaded>,
    pub(crate) view: ViewState,
    pub(crate) dialog: FileDialog,
    pub(crate) loading: Option<(Vec<PathBuf>, mpsc::Receiver<LoadResult>)>,
    pub(crate) error: Option<String>,
    pub(crate) show_warnings: bool,
    pub(crate) settings: PersistedSettings,
    pub(crate) stats: FrameStats,
    show_stats: bool,
    pub(crate) scripted: Scripted,
    scripted_run: bool,
    // G03: selection, background analysis, overlays, lists, delete all and
    // the DRC export.
    pub(crate) selection: Selection,
    pub(crate) analysis: Analysis,
    pub(crate) overlay: OverlayState,
    pub(crate) overlay_counts: OverlayCounts,
    pub(crate) lists: Lists,
    pub(crate) confirm_delete: bool,
    pub(crate) export_dialog: ExportDialog,
    pub(crate) export_job: Option<ExportJob>,
    /// A message for the status bar that is not an error.
    pub(crate) notice: Option<String>,
    // G04: the display windows and the log.
    pub(crate) windows: Windows,
    pub(crate) log: Log,
    // G05: the tools of the left button, context menus and keepouts.
    pub(crate) tools: Tools,
    // G06: the rules dialogs.
    pub(crate) dialogs: crate::dialogs::Dialogs,
    /// The analysis state a "design rules check finished" line was logged
    /// for, so each result is logged once.
    logged_drc: Option<crate::analysis::Tag>,
    /// Tile the loaded board this many times (`--bench-synthetic`).
    synthetic: Option<(u32, u32)>,
    /// When the load in progress, or the last one, started.
    load_started: Option<Instant>,
    /// Frames that finished painting since the last update.
    done: Vec<crate::stats::FrameCpu>,
    /// The tessellation last counted as a full build.
    seen_tess: Option<u64>,
    /// `--bench drag`: where the press was (scene millimetres).
    pub(crate) bench_press: Option<crate::scene::P>,
    // G07: the autorouter job and its settings.
    pub(crate) routing: Option<crate::routing::RoutingRun>,
    pub(crate) autoroute: crate::routing::Autoroute,
}

impl App {
    /// The app with `open` (a design, then optionally a session) loading.
    pub fn new(cc: &eframe::CreationContext<'_>, open: Vec<PathBuf>) -> Self {
        let args = Args {
            open,
            ..Args::default()
        };
        Self::build(&cc.egui_ctx, cc.storage, args)
    }

    /// The app for a command line, with settings from `storage`.
    pub fn build(ctx: &egui::Context, storage: Option<&dyn eframe::Storage>, args: Args) -> Self {
        ctx.set_visuals(egui::Visuals::dark());
        // A scripted run (bench, screenshot) must not read the user's settings.
        let settings = if args.scripted() {
            PersistedSettings::default()
        } else {
            PersistedSettings::restore(storage)
        };
        let start_dir = args
            .open
            .first()
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
            .or_else(|| settings.last_dir.clone())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let mut view = ViewState::default();
        view.show_labels = settings.show_labels;
        for (i, kind) in Kind::ALL.iter().enumerate() {
            if let Some(&on) = settings.kind_visible.get(kind.label()) {
                view.kind_visible[i] = on;
            }
        }
        restore_look(&mut view, &settings);
        let settings_selection_layers = settings.selection_layers.clone();
        let settings_selectable = settings.selectable_items.clone();
        let mut app = App {
            loaded: None,
            view,
            dialog: FileDialog::new(start_dir),
            loading: None,
            error: None,
            show_warnings: false,
            settings,
            stats: FrameStats::default(),
            show_stats: std::env::var_os("FREEROUTING_GUI_STATS").is_some(),
            scripted: Scripted::new(&args),
            scripted_run: args.scripted(),
            selection: Selection::default(),
            analysis: Analysis::default(),
            overlay: OverlayState::default(),
            overlay_counts: OverlayCounts::default(),
            lists: Lists::default(),
            confirm_delete: false,
            export_dialog: ExportDialog::default(),
            export_job: None,
            notice: None,
            windows: Windows::default(),
            log: Log::default(),
            tools: Tools {
                select: SelectionSettings::from_stored(
                    settings_selection_layers.as_deref(),
                    settings_selectable.as_deref(),
                ),
                ..Tools::default()
            },
            dialogs: crate::dialogs::Dialogs::default(),
            logged_drc: None,
            synthetic: args.synthetic,
            load_started: None,
            done: Vec::new(),
            seen_tess: None,
            bench_press: None,
            routing: None,
            autoroute: crate::routing::Autoroute::default(),
        };
        if !args.open.is_empty() {
            app.start_load(ctx, args.open, None);
        }
        app
    }

    pub fn loaded(&self) -> Option<&Loaded> {
        self.loaded.as_ref()
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn view(&self) -> &ViewState {
        &self.view
    }

    /// The view, for tests and scripts that place the camera.
    pub fn view_mut(&mut self) -> &mut ViewState {
        &mut self.view
    }

    /// The frames that finished painting since the last update, by both
    /// definitions of frame CPU (see [`crate::stats`]).
    pub fn clock(&self) -> &std::sync::Arc<crate::stats::FrameClock> {
        &self.view.renderer.clock
    }

    pub fn stats(&self) -> &FrameStats {
        &self.stats
    }

    /// The selected items.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// The background analysis: ratsnest and design rules check.
    pub fn analysis(&self) -> &Analysis {
        &self.analysis
    }

    /// Which overlays are on.
    pub fn overlay(&self) -> OverlayState {
        self.overlay
    }

    /// The Info list windows.
    pub fn lists(&self) -> &Lists {
        &self.lists
    }

    /// Which of the display windows (visibility, display options, colours,
    /// log, About) are open.
    pub fn windows(&self) -> &Windows {
        &self.windows
    }

    /// The log: loader warnings, engine messages and the app's events.
    pub fn log_entries(&self) -> Vec<crate::panels::log::Entry> {
        self.log.entries(self.loaded.as_ref().map(|b| &b.session))
    }

    /// Whether a rules dialog is open (G06).
    pub fn dialogs_open(&self, dialog: crate::dialogs::RulesDialog) -> bool {
        self.dialogs.is_open(dialog)
    }

    /// The last message that is not an error (an export result).
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// True while a DRC report is being written.
    pub fn is_exporting(&self) -> bool {
        self.export_job.is_some()
    }

    /// The settings as they would be saved now.
    pub fn settings_snapshot(&self) -> PersistedSettings {
        let mut s = self.settings.clone();
        self.collect_settings(&mut s);
        s
    }

    /// Writes the view's state into `s`: what the user chose in this
    /// session, keyed by names so that it carries over to other boards.
    fn collect_settings(&self, s: &mut PersistedSettings) {
        for (i, kind) in Kind::ALL.iter().enumerate() {
            s.kind_visible
                .insert(kind.label().to_string(), self.view.kind_visible[i]);
        }
        s.show_labels = self.view.show_labels;
        let look = &self.view.look;
        for object in Object::ALL {
            s.object_levels
                .insert(object.label().to_string(), look.object_level(object));
        }
        s.dim = (look.dim() != crate::look::DEFAULT_DIM).then_some(look.dim());
        for i in 0..VIRTUAL_LAYERS {
            s.virtual_layers
                .insert(virtual_name(i).to_string(), look.virtual_on(i));
        }
        s.unit = Some(self.view.unit.name().to_string());
        s.crosshair = self.view.crosshair;
        // G05.2: what a click selects. The defaults are stored as `None`.
        let select = &self.tools.select;
        let default = SelectionSettings::default();
        s.selection_layers = (select.layers != default.layers).then(|| select.layers.name().into());
        s.selectable_items =
            (select.selectable != default.selectable).then(|| select.stored_choices());
        // The "other" colours belong to the user, the layer colours to the
        // layers of the board that is open, by name.
        s.colors.retain(|key, _| !key.starts_with("other/"));
        let names: Vec<String> = self
            .loaded
            .iter()
            .flat_map(|b| b.scene.layers.iter().map(|l| l.name.clone()))
            .collect();
        for name in &names {
            let prefix = format!("layer/{name}/");
            s.colors.retain(|key, _| !key.starts_with(&prefix));
        }
        s.colors.extend(look.changed_colors(&names));
        if let Some(b) = &self.loaded {
            for ((layer, &on), &level) in b
                .scene
                .layers
                .iter()
                .zip(&self.view.layer_visible)
                .zip(look.layer_levels())
            {
                s.layer_visible_by_name.insert(layer.name.clone(), on);
                s.layer_level_by_name.insert(layer.name.clone(), level);
            }
        }
    }

    /// Runs one command: the single path for menus, keys and tests.
    pub fn execute(&mut self, ctx: &egui::Context, command: Command) {
        // G07: while the autorouter runs, Cancel and a click stop it and
        // edits are refused.
        if self.routing_filter(&command) {
            return;
        }
        // G09: while a route or a copy is under way the layer keys and the
        // layers panel ask the editor to change the layer.
        if self.editor_layer(&command) {
            return;
        }
        // A command that changes the board in another way ends the session
        // first: the session refers to items by id.
        if self.editor_busy()
            && matches!(
                command,
                Command::ConfirmDeleteAll
                    | Command::EditRules(_)
                    | Command::DeleteSelection
                    | Command::FixSelection
                    | Command::UnfixSelection
                    | Command::StartKeepout { .. }
                    | Command::Reload
            )
        {
            self.end_editor_session();
        }
        match command {
            Command::Open(path) => self.start_load(ctx, vec![path], self.current()),
            Command::OpenDialog => self.open_dialog(),
            Command::Reload => {
                if let Some(b) = &self.loaded {
                    let paths = &b.session.paths;
                    let files = vec![paths.session.clone().unwrap_or(paths.design.clone())];
                    self.loaded = None;
                    self.start_load(ctx, files, None);
                }
            }
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::ShowWarnings => self.show_warnings = true,
            Command::ShowWindow(kind) => self.windows.set(kind, true),
            Command::CloseWindow(kind) => self.windows.set(kind, false),
            Command::RecalculateList(kind) => self.lists.window(kind).invalidate(),
            Command::Cancel => self.cancel(),
            Command::ShowRulesDialog(dialog) => self.dialogs.open(dialog, true),
            Command::CloseRulesDialog(dialog) => self.dialogs.open(dialog, false),
            Command::EditRules(edit) => self.execute_rules_edit(edit),
            command @ (Command::StartRouting
            | Command::StopRouting
            | Command::ShowAutorouteSettings
            | Command::CloseAutorouteSettings
            | Command::ApplyAutorouteSettings(_)
            | Command::CloseRoutingSummary
            | Command::SetShowRoutingSummary(_)) => self.execute_routing(ctx, command),
            Command::Notice(message) => self.notice = Some(message),
            command @ (Command::CanvasClick { .. }
            | Command::SelectRegion { .. }
            | Command::SelectAt(_)
            | Command::ExtendSelection(_)
            | Command::ToggleSelectionViolations
            | Command::ShowItemInfo(_)
            | Command::ShowSelectionSettings
            | Command::SetSelectionLayers(_)
            | Command::SetSelectable(..)
            | Command::DeleteSelection
            | Command::FixSelection
            | Command::UnfixSelection
            | Command::OpenPopup { .. }
            | Command::ClosePopup
            | Command::CenterAt(_)
            | Command::ZoomInAt(_)
            | Command::ZoomOutAt(_)
            | Command::StartKeepout { .. }
            | Command::Complete
            | Command::SetTool(_)
            | Command::StartRoute { .. }
            | Command::StartPinSwap { .. }
            | Command::TogglePush
            | Command::PointerMoved(_)
            | Command::DragStart(_)
            | Command::DragMove(_)
            | Command::DragEnd
            | Command::ChangeRouteLayer(_)
            | Command::UpdateInteractive(_)
            | Command::SetManualTraceWidth { .. }
            | Command::SetManualClearanceClass(_)
            | Command::SetManualViaRule(_)
            | Command::MoveSelection { .. }
            | Command::RotateSelection(_)
            | Command::MirrorSelection
            | Command::ResetRotation
            | Command::CopySelection { .. }
            | Command::SetComponentGrid { .. }) => self.execute_tool(ctx, command),
            command @ (Command::Select(_)
            | Command::ClearSelection
            | Command::ZoomSelection
            | Command::ToggleRatsnest
            | Command::ToggleViolations
            | Command::RunDrc
            | Command::ShowList(_)
            | Command::DeleteAll
            | Command::ConfirmDeleteAll
            | Command::Undo
            | Command::Redo
            | Command::ExportDrcDialog
            | Command::ExportDrcReport(_)) => self.execute_analysis(ctx, command),
            view_command => {
                let signal: Vec<bool> = self
                    .loaded
                    .iter()
                    .flat_map(|b| b.scene.layers.iter().map(|l| l.signal))
                    .collect();
                self.view.apply(&view_command, &signal);
            }
        }
    }

    fn current(&self) -> Option<CurrentDesign> {
        let s = &self.loaded.as_ref()?.session;
        Some((s.design.clone(), s.paths.design.clone()))
    }

    fn open_dialog(&mut self) {
        let dir = self
            .loaded
            .as_ref()
            .and_then(|b| b.session.paths.design.parent().map(PathBuf::from));
        match dir {
            Some(dir) => self.dialog.show_in(&dir),
            None => self.dialog.open = true,
        }
    }

    fn start_load(
        &mut self,
        ctx: &egui::Context,
        paths: Vec<PathBuf>,
        current: Option<CurrentDesign>,
    ) {
        let (tx, rx) = mpsc::channel();
        let (ctx, files, synthetic) = (ctx.clone(), paths.clone(), self.synthetic);
        self.load_started = Some(Instant::now());
        self.view.renderer.clock.reset_first_paint();
        std::thread::spawn(move || {
            let loaded = session::load_all(&files, current).map(|mut loaded| {
                if let Some((nx, ny)) = synthetic {
                    crate::bench_synthetic::tile_loaded(&mut loaded, nx, ny);
                }
                loaded
            });
            let _ = tx.send(loaded);
            ctx.request_repaint();
        });
        self.loading = Some((paths, rx));
    }

    fn poll_load(&mut self) {
        let Some((paths, rx)) = &self.loading else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Reading the file failed unexpectedly.".into())
            }
        };
        let paths = paths.clone();
        self.loading = None;
        match result {
            Ok(board) => {
                for path in &paths {
                    self.settings.last_dir = path.parent().map(PathBuf::from);
                    self.settings.remember_file(path.clone());
                }
                let summary = format!(
                    "Loaded {} in {:.0} ms: {} layers, {} items.",
                    paths
                        .last()
                        .map_or_else(String::new, |p| p.display().to_string()),
                    board.timings.total_ms,
                    board.scene.layers.len(),
                    board.session.board.item_count(),
                );
                self.set_board(board);
                self.log.clear();
                self.log.push(Level::Info, summary);
            }
            Err(e) => {
                self.log.push(Level::Error, e.clone());
                self.error = Some(e);
            }
        }
    }

    fn set_board(&mut self, board: Loaded) {
        let same = self.loaded.as_ref().is_some_and(|b| {
            b.scene.layers.len() == board.scene.layers.len()
                && b.session.paths.design == board.session.paths.design
        });
        if !same {
            // What the user chose on the board that was open goes to the
            // settings first, so another board with the same layer names
            // starts the way this one ended.
            let mut stored = self.settings.clone();
            self.collect_settings(&mut stored);
            self.settings = stored;
            self.view.reset_for_board(board.scene.layers.len());
            let names: Vec<String> = board.scene.layers.iter().map(|l| l.name.clone()).collect();
            for (i, layer) in board.scene.layers.iter().enumerate() {
                if let Some(&on) = self.settings.layer_visible_by_name.get(&layer.name) {
                    self.view.layer_visible[i] = on;
                }
                if let Some(&level) = self.settings.layer_level_by_name.get(&layer.name) {
                    self.view.look.set_layer_level(i, level);
                }
            }
            self.view.look.apply_colors(&self.settings.colors, &names);
        }
        self.loaded = Some(board);
        self.autoroute = crate::routing::Autoroute::default();
        self.error = None;
        self.analysis.reset();
        self.selection.clear();
        self.notice = None;
        self.dialogs.reset();
        let select = std::mem::take(&mut self.tools.select);
        self.tools = Tools {
            select,
            ..Tools::default()
        };
    }

    fn commands_this_frame(&mut self, ctx: &egui::Context) -> Vec<Command> {
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
        let mut out: Vec<Command> = dropped.into_iter().map(Command::Open).collect();
        out.extend(self.dialog.ui(ctx).map(Command::Open));
        let typing = ctx.wants_keyboard_input();
        out.extend(ctx.input(|i| input::key_commands(i, typing)));
        // Java's `V` shows the clearance violations of the selected items
        // while items are selected, and all of them otherwise.
        if !self.selection.is_empty() {
            for command in &mut out {
                if *command == Command::ToggleViolations {
                    *command = Command::ToggleSelectionViolations;
                }
            }
        }
        let cursor = self
            .view
            .camera
            .zip(self.view.cursor)
            .map(|(camera, pos)| camera.to_board(self.view.canvas, pos));
        out.extend(
            ctx.input(|i| crate::tools::inspect::key_commands(i, typing, &self.selection, cursor)),
        );
        // G09: the interactive tools' keys.
        out.extend(ctx.input(|i| {
            crate::tools::route::key_commands(
                i,
                typing,
                self.selection.is_empty(),
                self.editor_busy(),
                cursor,
            )
        }));
        if self.is_moving() {
            // `+` and `-` turn the floating selection instead of the layer.
            out.retain(|c| !matches!(c, Command::StepActiveLayer(_)));
            out.extend(ctx.input(|i| crate::tools::move_component::float_key_commands(i, typing)));
        }
        if !self.selection.is_empty() && !self.editor_busy() {
            out.extend(
                ctx.input(|i| crate::tools::move_component::copy_key_command(i, typing, cursor)),
            );
        }
        out
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let clock = self.view.renderer.clock.clone();
        clock.begin_update();
        self.done = clock.take_done();
        for f in &self.done {
            self.stats.record_full(f.frame_ms);
        }
        self.view
            .renderer
            .select(frame.wgpu_render_state().map(|s| s.target_format));
        self.poll_load();
        self.poll_routing(ctx);
        self.count_builds();
        self.analysis
            .set_held(self.editor_busy() || self.routing.is_some());
        self.analysis
            .poll(ctx, self.loaded.as_ref().map(|b| &b.session));
        self.poll_export();
        self.log_analysis();
        let mut commands = self.commands_this_frame(ctx);
        commands.extend(self.bench_drag_commands());
        commands.extend(self.bench_route_commands());
        let (panel_commands, canvas) = self.panels(ctx);
        commands.extend(panel_commands);
        if let Some(b) = &self.loaded {
            commands.extend(
                self.lists
                    .show(ctx, &b.session, &self.analysis, self.view.unit),
            );
        }
        let layer_names: Vec<String> = self
            .loaded
            .iter()
            .flat_map(|b| b.scene.layers.iter().map(|l| l.name.clone()))
            .collect();
        let entries = self.log.entries(self.loaded.as_ref().map(|b| &b.session));
        commands.extend(self.windows.show(ctx, &self.view, &layer_names, &entries));
        commands.extend(self.confirm_delete_ui(ctx));
        commands.extend(self.tools_ui(ctx));
        if let Some(b) = &self.loaded {
            let manual = self.tools.manual.clone().unwrap_or_else(|| {
                freerouting::interactive::ManualRules::new(b.session.board.layer_count())
            });
            let view = crate::dialogs::RuleView::new(&b.session, self.view.unit).with_interactive(
                crate::dialogs::InteractiveView {
                    settings: &self.settings.interactive,
                    manual: &manual,
                    component_grid: self.tools.component_grid,
                },
            );
            commands.extend(self.dialogs.show(ctx, &view));
        }
        commands.extend(self.export_dialog.ui(ctx).map(Command::ExportDrcReport));
        commands.extend(self.autoroute_ui(ctx));
        for command in commands {
            self.execute(ctx, command);
        }
        if self.show_warnings {
            let mut open = true;
            egui::Window::new("Warnings")
                .open(&mut open)
                .default_size([600.0, 300.0])
                .show(ctx, |ui| {
                    if let Some(b) = &self.loaded {
                        warnings::ui(ui, &b.session);
                    }
                });
            self.show_warnings = open;
        }
        let ms = clock.end_update();
        self.finish_frame(ctx, ms, canvas.paint);
    }

    fn persist_egui_memory(&self) -> bool {
        !self.scripted_run
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // A bench or screenshot run must not overwrite the user's settings.
        if !self.scripted_run {
            self.settings_snapshot().save(storage);
        }
    }
}

/// Puts the stored display options into a new view: the unit, the crosshair,
/// the object levels, the dimming, the virtual layers and the "other"
/// colours. The layer levels and layer colours wait for a board, whose layer
/// names they are keyed by.
fn restore_look(view: &mut ViewState, settings: &PersistedSettings) {
    view.crosshair = settings.crosshair;
    if let Some(unit) = settings
        .unit
        .as_deref()
        .and_then(freerouting::board::structure::Unit::from_name)
    {
        view.unit = unit;
    }
    for object in Object::ALL {
        if let Some(&level) = settings.object_levels.get(object.label()) {
            view.look.set_object_level(object, level);
        }
    }
    if let Some(dim) = settings.dim {
        view.look.set_dim(dim);
    }
    for i in 0..VIRTUAL_LAYERS {
        if let Some(&on) = settings.virtual_layers.get(virtual_name(i)) {
            view.look.set_virtual_on(i, on);
        }
    }
    view.look.apply_colors(&settings.colors, &[]);
}

impl App {
    /// Logs each design rules check once it has finished (G04.8).
    fn log_analysis(&mut self) {
        let Some(b) = &self.loaded else { return };
        let Some(drc) = self.analysis.drc(&b.session) else {
            return;
        };
        let tag = self.analysis.tag_of(&b.session);
        if self.logged_drc == Some(tag) {
            return;
        }
        self.logged_drc = Some(tag);
        let incomplete = self
            .analysis
            .ratsnest(&b.session)
            .map_or(0, |r| r.incomplete_count);
        self.log.push(
            Level::Info,
            format!(
                "Design rules check: {} clearance violations, {} unconnected, {} zone islands, \
                 {incomplete} incomplete connections ({:.0} ms).",
                drc.clearance.len(),
                drc.unconnected.len(),
                drc.islands.len(),
                drc.elapsed.as_secs_f64() * 1000.0,
            ),
        );
    }

    /// Counts a board's tessellation as a full build the first time it is
    /// seen, however it came about (a load or `Loaded::rebuild_scene`).
    fn count_builds(&mut self) {
        let id = self.loaded.as_ref().map(|b| b.tess.with(|s| s.id()));
        if id.is_some() && id != self.seen_tess {
            self.stats.record_build();
            self.seen_tess = id;
        }
    }

    fn finish_frame(&mut self, ctx: &egui::Context, ms: f64, paint: PaintStats) {
        self.stats.record(ms, paint);
        self.stats.overlay = self.overlay_counts;
        if self.show_stats {
            crate::stats::overlay(ctx, &self.stats);
        }
        let ready = self.loading.is_none() && (self.loaded.is_some() || self.error.is_some());
        let first_frame_ms = self
            .load_started
            .zip(self.view.renderer.clock.first_paint_end())
            .map(|(start, end)| end.saturating_duration_since(start).as_secs_f64() * 1000.0);
        let report = FrameReport {
            ms,
            paint,
            done: &self.done,
            first_frame_ms,
        };
        self.scripted
            .bench_counters(self.stats.full_rebuilds, self.stats.patches);
        self.scripted
            .after_frame(ctx, ready, &report, self.loaded.as_ref());
    }
}
