//! The main window: menu, side panel, status bar and board canvas.
//!
//! Every action is a [`Command`] run by [`App::execute`]; the panels, the
//! menu and the keys only produce commands.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use eframe::egui;

use crate::args::Args;
use crate::capture::Scripted;
use crate::commands::Command;
use crate::file_dialog::FileDialog;
use crate::input;
use crate::panels::warnings;
use crate::scene::Kind;
use crate::session::{self, CurrentDesign, Loaded};
use crate::settings::PersistedSettings;
use crate::stats::{FrameStats, PaintStats};
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
    stats: FrameStats,
    show_stats: bool,
    pub(crate) scripted: Scripted,
    scripted_run: bool,
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

    pub fn stats(&self) -> &FrameStats {
        &self.stats
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
        let (ctx, files) = (ctx.clone(), paths.clone());
        std::thread::spawn(move || {
            let _ = tx.send(session::load_all(&files, current));
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let started = Instant::now();
        self.poll_load();
        let mut commands = self.commands_this_frame(ctx);
        let (panel_commands, canvas) = self.panels(ctx);
        commands.extend(panel_commands);
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
        let ms = started.elapsed().as_secs_f64() * 1000.0;
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
    fn finish_frame(&mut self, ctx: &egui::Context, ms: f64, paint: PaintStats) {
        self.stats.record(ms, paint);
        if self.show_stats {
            crate::stats::overlay(ctx, &self.stats);
        }
        let ready = self.loading.is_none() && (self.loaded.is_some() || self.error.is_some());
        self.scripted
            .after_frame(ctx, ready, ms, paint, self.loaded.as_ref());
    }
}
