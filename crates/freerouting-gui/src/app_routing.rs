//! `App`'s side of the autorouter (G07): starting and stopping the job, the
//! per-frame poll that patches the scene, and the settings and summary
//! windows. The job and its events are in [`crate::routing`].

use std::sync::Arc;

use eframe::egui;
use freerouting::autoroute::events::RunSummary;
use freerouting::board::RoutingBoard;
use freerouting::job::prepare::prepare_job_settings;
use freerouting::settings::{ResolveContext, SettingsPatch};

use crate::app::App;
use crate::commands::Command;
use crate::dialogs::autoroute_settings::{Draft, LayerInfo};
use crate::panels::log::Level;
use crate::routing::{
    Autoroute, JobEnd, REPAINT_EVERY, RoutingJobHandle, RoutingRun, RoutingSummaryData, reconcile,
};

impl App {
    /// True while a job runs.
    pub fn is_routing(&self) -> bool {
        self.routing.is_some()
    }

    /// The running job.
    pub fn routing(&self) -> Option<&RoutingRun> {
        self.routing.as_ref()
    }

    /// The running job, for tests that age its stop request.
    #[doc(hidden)]
    pub fn routing_mut(&mut self) -> Option<&mut RoutingRun> {
        self.routing.as_mut()
    }

    /// The autorouter's settings, summary and counters.
    pub fn autoroute(&self) -> &Autoroute {
        &self.autoroute
    }

    /// The settings the next job starts with, built from the open board the
    /// first time they are needed.
    pub(crate) fn autoroute_patch(&mut self) -> Option<SettingsPatch> {
        if self.autoroute.applied.is_none() {
            let b = self.loaded.as_ref()?;
            let resolve = ResolveContext::from_machine();
            let (baseline, _warnings) =
                prepare_job_settings(&b.session.board, Some(&b.session.design), &[], &resolve);
            let mut draft = Draft::new(baseline.clone(), resolve.cores);
            draft.apply_overrides(&self.settings.autoroute);
            // The router flags of the front-end mode (`-mp`, `-mt`, `--router.*`,
            // ...; G08.4, K02.5) win over the saved settings, as the command
            // line does over a settings file. A `--profile` moves the values
            // that are still the profile's first (N01.3).
            let router = &self.files.frontend.options.router;
            if router.profile.is_some()
                && let Ok(profile) = freerouting::settings::Profile::of_patch(router)
            {
                draft.set_profile(profile);
            }
            draft.patch.apply_from(router);
            self.autoroute.baseline = Some(baseline);
            self.autoroute.applied = Some(draft.patch);
        }
        self.autoroute.applied.clone()
    }

    /// Starts the autorouter on the open board (the toolbar's Autoroute).
    pub(crate) fn start_routing(&mut self, ctx: &egui::Context) {
        if self.routing.is_some() || self.loading.is_some() {
            return;
        }
        if self.loaded.is_none() {
            self.notice = Some(crate::text::OPEN_A_BOARD.to_string());
            return;
        }
        self.end_editor_session();
        let Some(patch) = self.autoroute_patch() else {
            return;
        };
        let Some(b) = &self.loaded else { return };
        let handle = RoutingJobHandle::spawn(Arc::clone(&b.session.board), patch, ctx.clone());
        self.routing = Some(RoutingRun::new(handle, &b.session.board));
        self.selection.clear();
        self.autoroute.summary_open = false;
        self.notice = Some(
            if self.settings.stop_on_canvas_click {
                crate::text::BATCH_AUTOROUTER_START_MESSAGE
            } else {
                crate::text::BATCH_AUTOROUTER_START_MESSAGE_NO_CLICK
            }
            .to_string(),
        );
        self.log
            .push(Level::Info, "Starting auto-routing of the open board.");
        ctx.request_repaint();
    }

    /// Asks the running job to stop; it hands back the board it keeps.
    pub(crate) fn stop_routing(&mut self) {
        if let Some(run) = &mut self.routing {
            if run.stop_requested.is_none() {
                self.log.push(Level::Info, "Stop requested.");
                self.notice = Some(crate::text::ROUTING_STOPPING.to_string());
            }
            run.stop();
        }
    }

    /// Runs the routing commands. Returns false for any other command.
    pub(crate) fn execute_routing(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::StartRouting => self.start_routing(ctx),
            Command::StopRouting => self.stop_routing(),
            Command::ShowAutorouteSettings => {
                if let Some(patch) = self.autoroute_patch() {
                    self.autoroute.dialog =
                        Some(Draft::new(patch, ResolveContext::from_machine().cores));
                }
            }
            Command::CloseAutorouteSettings => self.autoroute.dialog = None,
            Command::ApplyAutorouteSettings(patch) => {
                let cores = ResolveContext::from_machine().cores;
                if let Some(baseline) = &self.autoroute.baseline {
                    self.settings.autoroute =
                        Draft::new((*patch).clone(), cores).overrides_against(baseline);
                }
                self.autoroute.applied = Some(*patch);
            }
            Command::CloseRoutingSummary => self.autoroute.summary_open = false,
            Command::SetShowRoutingSummary(show) => self.settings.show_routing_summary = show,
            Command::SetStopOnCanvasClick(stop) => self.settings.stop_on_canvas_click = stop,
            _ => {}
        }
    }

    /// While a job runs: stop on Cancel/Esc and, unless the preference
    /// `stop_on_canvas_click` is off, on a click on the canvas (Java's "Click
    /// anywhere on the canvas to cancel"); refuse edits. Returns true if the
    /// command was handled here.
    pub(crate) fn routing_filter(&mut self, command: &Command) -> bool {
        if self.routing.is_none() {
            return false;
        }
        match command {
            Command::CanvasClick { .. } if !self.settings.stop_on_canvas_click => {
                self.notice = Some(crate::text::ROUTING_CLICK_IGNORED.to_string());
                true
            }
            Command::Cancel | Command::CanvasClick { .. } | Command::StopRouting => {
                self.stop_routing();
                true
            }
            Command::StartRouting => true,
            c if c.edits_board() => {
                self.notice = Some(crate::text::ROUTING_READ_ONLY.to_string());
                true
            }
            _ => false,
        }
    }

    /// Reads this frame's events: patches the scene, keeps the newest
    /// progress, and finishes the job when its board has come back.
    pub(crate) fn poll_routing(&mut self, ctx: &egui::Context) {
        let Some(run) = &mut self.routing else {
            return;
        };
        let Some(loaded) = &mut self.loaded else {
            self.routing = None;
            return;
        };
        let patches_before = run.stats.patches;
        let mut events = run.poll(loaded);
        self.stats.patches += run.stats.patches - patches_before;
        let finished = events.finished.take();
        let disconnected = events.disconnected;
        // The user asked this job to stop (Cancel, Esc, a click, the stop
        // command, or the window closing).
        let stopped = run.stop_requested.is_some();
        if let Some((board, summary)) = finished {
            self.finish_routing(board, summary);
            self.frontend_job_ended(ctx, stopped);
        } else if disconnected {
            self.routing_failed();
            self.frontend_job_ended(ctx, false);
        } else {
            if run.stop_is_slow() && self.notice.as_deref() == Some(crate::text::ROUTING_STOPPING) {
                self.notice = Some(crate::text::ROUTING_STOP_SLOW.to_string());
            }
            if events.capped {
                // More events are waiting: read them next frame, at once.
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(REPAINT_EVERY);
            }
        }
    }

    fn finish_routing(&mut self, board: Box<RoutingBoard>, summary: RunSummary) {
        let Some(mut run) = self.routing.take() else {
            return;
        };
        let end = run.handle.join();
        let Some(loaded) = &mut self.loaded else {
            return;
        };
        // The router's own deltas should have made the picture right; this
        // finds anything they did not say.
        run.stats.reconcile_fixes += reconcile(loaded, &board) as u64;
        let mut board = *board;
        board.set_journaling(true);
        loaded.session.board = Arc::new(board);
        loaded.session.edit_generation += 1;
        let duration = run.started.elapsed();
        let (statistics, outcome_log) = match end {
            Some(JobEnd::Finished {
                statistics,
                outcome,
            }) => (statistics, outcome.log),
            _ => (None, Vec::new()),
        };
        for line in &outcome_log {
            let level = match line.level {
                freerouting::job::LogLevel::Error => Level::Error,
                freerouting::job::LogLevel::Warn => Level::Warning,
                _ => Level::Info,
            };
            if line.level != freerouting::job::LogLevel::Debug {
                self.log.push(level, line.text.clone());
            }
        }
        if let Some(asked) = run.stop_requested {
            let seconds = format!("{:.1}", asked.elapsed().as_secs_f64());
            self.log.push(
                Level::Info,
                crate::text::fill(crate::text::ROUTING_STOPPED_AFTER, &[&seconds]),
            );
        }
        let message = crate::panels::progress::end_message(&summary);
        self.log.push(Level::Info, message.clone());
        self.notice = Some(message);
        if let Some(statistics) = statistics {
            self.autoroute.summary = Some(RoutingSummaryData::new(&statistics, &summary, duration));
            self.autoroute.summary_open = self.settings.show_routing_summary;
        }
        self.autoroute.last_stats = Some(run.stats.clone());
        self.autoroute.last_run = Some(summary);
        // Items the selection and the editor referred to may be gone.
        self.selection.clear();
        let select = std::mem::take(&mut self.tools.select);
        self.tools = crate::tools::Tools {
            select,
            ..crate::tools::Tools::default()
        };
    }

    /// The job ended without handing a board back (the pipeline refused to
    /// run, or the thread panicked).
    fn routing_failed(&mut self) {
        let Some(mut run) = self.routing.take() else {
            return;
        };
        match run.handle.join() {
            Some(JobEnd::Failed { message, board }) => {
                if let Some(loaded) = &mut self.loaded {
                    let mut board = *board;
                    board.set_journaling(true);
                    loaded.session.board = Arc::new(board);
                    loaded.session.edit_generation += 1;
                    reconcile(loaded, &loaded.session.board.clone());
                }
                self.log.push(Level::Error, message.clone());
                self.notice = Some(message);
            }
            Some(JobEnd::Finished { .. }) | None => {
                let message = "The autorouter stopped unexpectedly.".to_string();
                self.log.push(Level::Error, message.clone());
                self.error = Some(message);
            }
        }
    }

    /// The settings dialog and the summary window.
    pub(crate) fn autoroute_ui(&mut self, ctx: &egui::Context) -> Vec<Command> {
        let mut commands = Vec::new();
        let layers: Vec<LayerInfo> = self
            .loaded
            .iter()
            .flat_map(|b| {
                b.scene.layers.iter().map(|l| LayerInfo {
                    name: l.name.clone(),
                    signal: l.signal,
                })
            })
            .collect();
        let stop_on_click = self.settings.stop_on_canvas_click;
        if let Some(draft) = &mut self.autoroute.dialog {
            let mut open = true;
            egui::Window::new(crate::text::AUTOROUTE_SETTINGS_TITLE)
                .open(&mut open)
                .default_width(460.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        commands.extend(
                            crate::dialogs::autoroute_settings::ui(
                                ui,
                                draft,
                                &layers,
                                stop_on_click,
                            )
                            .commands,
                        );
                    });
                });
            if !open {
                commands.push(Command::CloseAutorouteSettings);
            }
        }
        if self.autoroute.summary_open
            && let Some(data) = &self.autoroute.summary
        {
            let mut open = true;
            let unit = self.view.unit;
            let show = self.settings.show_routing_summary;
            egui::Window::new(crate::text::SUMMARY_TITLE)
                .open(&mut open)
                .resizable(false)
                .show(ctx, |ui| {
                    commands.extend(crate::panels::routing_summary::ui(ui, data, unit, show));
                });
            if !open {
                commands.push(Command::CloseRoutingSummary);
            }
        }
        commands
    }
}

impl App {
    /// The commands of the coming frame of a `--bench route` run: start the
    /// job on the first counted frame (with `FREEROUTING_BENCH_PASSES` as the
    /// pass limit), press Cancel after `FREEROUTING_BENCH_CANCEL_MS`, and
    /// hand the job's counters to the report when it has ended.
    pub(crate) fn bench_route_commands(&mut self) -> Vec<Command> {
        let Some(step) = self.scripted.route_step() else {
            return Vec::new();
        };
        if self.loaded.is_none() || self.view.camera.is_none() {
            return Vec::new();
        }
        let env = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<i64>().ok());
        let (started, cancelled) = self
            .scripted
            .bench_mut()
            .map_or((None, None), |b| b.route_times());
        if started.is_none() {
            if step < crate::bench::WARMUP {
                return Vec::new();
            }
            if let Some(passes) = env("FREEROUTING_BENCH_PASSES")
                && let Some(mut patch) = self.autoroute_patch()
            {
                patch.autorouter.max_passes = i32::try_from(passes).ok();
                self.autoroute.applied = Some(patch);
            }
            if let Some(bench) = self.scripted.bench_mut() {
                bench.route_started();
            }
            return vec![Command::StartRouting];
        }
        if self.routing.is_none() {
            if let (Some(last), Some(stats)) =
                (&self.autoroute.last_run, &self.autoroute.last_stats)
                && let Some(bench) = self.scripted.bench_mut()
            {
                bench.route_finished(crate::bench::RouteReport {
                    passes: last.passes,
                    status: format!("{:?}", last.status),
                    progress_events: stats.progress_events,
                    delta_events: stats.delta_events,
                    patches: stats.patches,
                    replaced: stats.replaced,
                    items_added: stats.items_added,
                    reconcile_fixes: stats.reconcile_fixes,
                    max_apply_ms: stats.max_apply_ms,
                });
            }
            return Vec::new();
        }
        if cancelled.is_none()
            && let (Some(after), Some(started)) = (env("FREEROUTING_BENCH_CANCEL_MS"), started)
            && started.elapsed().as_millis() as i64 >= after
        {
            if let Some(bench) = self.scripted.bench_mut() {
                bench.route_cancelled();
            }
            return vec![Command::StopRouting];
        }
        Vec::new()
    }
}
