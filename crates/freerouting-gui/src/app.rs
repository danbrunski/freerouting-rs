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
use crate::overlays::OverlayState;
use crate::panels::warnings;
use crate::scene::Kind;
use crate::selection::Selection;
use crate::session::{self, CurrentDesign, Loaded};
use crate::settings::PersistedSettings;
use crate::stats::{FrameStats, OverlayCounts, PaintStats};
use crate::viewport::ViewState;

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
    /// Tile the loaded board this many times (`--bench-synthetic`).
    synthetic: Option<(u32, u32)>,
    /// When the load in progress, or the last one, started.
    load_started: Option<Instant>,
    /// Frames that finished painting since the last update.
    done: Vec<crate::stats::FrameCpu>,
    /// The tessellation last counted as a full build.
    seen_tess: Option<u64>,
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
            synthetic: args.synthetic,
            load_started: None,
            done: Vec::new(),
            seen_tess: None,
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
        for (i, kind) in Kind::ALL.iter().enumerate() {
            s.kind_visible
                .insert(kind.label().to_string(), self.view.kind_visible[i]);
        }
        s.show_labels = self.view.show_labels;
        if let Some(b) = &self.loaded {
            for (layer, &on) in b.scene.layers.iter().zip(&self.view.layer_visible) {
                s.layer_visible_by_name.insert(layer.name.clone(), on);
            }
        }
        s
    }

    /// Runs one command: the single path for menus, keys and tests.
    pub fn execute(&mut self, ctx: &egui::Context, command: Command) {
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
                for path in paths {
                    self.settings.last_dir = path.parent().map(PathBuf::from);
                    self.settings.remember_file(path);
                }
                self.set_board(board);
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn set_board(&mut self, board: Loaded) {
        let same = self.loaded.as_ref().is_some_and(|b| {
            b.scene.layers.len() == board.scene.layers.len()
                && b.session.paths.design == board.session.paths.design
        });
        if !same {
            self.view.reset_for_board(board.scene.layers.len());
            for (i, layer) in board.scene.layers.iter().enumerate() {
                if let Some(&on) = self.settings.layer_visible_by_name.get(&layer.name) {
                    self.view.layer_visible[i] = on;
                }
            }
        }
        self.loaded = Some(board);
        self.error = None;
        self.analysis.reset();
        self.selection.clear();
        self.notice = None;
    }

    fn commands_this_frame(&mut self, ctx: &egui::Context) -> Vec<Command> {
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
        let mut out: Vec<Command> = dropped.into_iter().map(Command::Open).collect();
        out.extend(self.dialog.ui(ctx).map(Command::Open));
        let typing = ctx.wants_keyboard_input();
        out.extend(ctx.input(|i| input::key_commands(i, typing)));
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
        self.count_builds();
        self.analysis
            .poll(ctx, self.loaded.as_ref().map(|b| &b.session));
        self.poll_export();
        let mut commands = self.commands_this_frame(ctx);
        let (panel_commands, canvas) = self.panels(ctx);
        commands.extend(panel_commands);
        if let Some(b) = &self.loaded {
            commands.extend(self.lists.show(ctx, &b.session, &self.analysis));
        }
        commands.extend(self.confirm_delete_ui(ctx));
        commands.extend(self.export_dialog.ui(ctx).map(Command::ExportDrcReport));
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

impl App {
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
            .after_frame(ctx, ready, &report, self.loaded.as_ref());
    }
}
