//! The stage registry, the selection read from the settings, the built
//! [`Pipeline`] and the [`NotPorted`] placeholders (A10.2). See the parent
//! module for the rules.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use std::sync::{Arc, Mutex};

use super::{
    BoardStage, ConnectionOutcome, ConnectionParams, ConnectionRouter, Destinations, Diagnostics,
    FailureRecord, FanoutStage, FanoutSummary, NeckDownAttempt, NeckDownPolicy, NetOrdering,
    Optimizer, OptimizerSummary, PassController, PlaneTapPolicy, RipupPolicy, RouteTask, StageCtx,
};
use crate::autoroute::AttemptState;
use crate::autoroute::batch::connection::{
    FreeroutingConnectionRouter, FreeroutingNeckDown, FreeroutingPlaneTaps,
};
use crate::autoroute::batch::control::FreeroutingPassController;
use crate::autoroute::batch::fanout::FreeroutingFanout;
use crate::autoroute::batch::ordering::FreeroutingOrdering;
use crate::autoroute::batch::report::UnroutedReportStage;
use crate::autoroute::batch::ripup_policy::FreeroutingRipupPolicy;
use crate::board::items::ItemId;
use crate::board::routing_board::RoutingBoard;
use crate::settings::RouterSettings;

// ---------------------------------------------------------------------
// Placeholders for the stages later checklists port
// ---------------------------------------------------------------------

/// A registered stage whose implementation another checklist ports. It
/// does nothing and says so: the connection router and neck-down fail with
/// a detail naming the checklist, the plane taps route to the unconnected
/// set, the fanout and optimizer run no pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotPorted {
    /// The id it is registered under.
    pub id: &'static str,
    /// The checklist that ports it.
    pub owner: &'static str,
}

impl NotPorted {
    fn detail(&self) -> String {
        format!("stage '{}' is not ported yet ({})", self.id, self.owner)
    }
}

impl FanoutStage for NotPorted {
    fn id(&self) -> &'static str {
        self.id
    }
    fn run(&mut self, _: &mut RoutingBoard, _: &mut StageCtx) -> FanoutSummary {
        FanoutSummary::default()
    }
}

impl ConnectionRouter for NotPorted {
    fn id(&self) -> &'static str {
        self.id
    }
    fn route(
        &mut self,
        _: &mut RoutingBoard,
        _: &RouteTask,
        _: &ConnectionParams,
        _: &mut StageCtx,
    ) -> ConnectionOutcome {
        let mut outcome = ConnectionOutcome::new(AttemptState::Failed, self.detail());
        outcome.failure = Some(FailureRecord::new(self.detail()));
        outcome
    }
}

impl PlaneTapPolicy for NotPorted {
    fn id(&self) -> &'static str {
        self.id
    }
    fn destinations(
        &self,
        _: &RoutingBoard,
        _: &RouteTask,
        unconnected: &BTreeSet<ItemId>,
    ) -> Destinations {
        Destinations {
            primary: unconnected.clone(),
            fallback: None,
        }
    }
}

impl NeckDownPolicy for NotPorted {
    fn id(&self) -> &'static str {
        self.id
    }
    fn retry(
        &mut self,
        _: &mut RoutingBoard,
        _: &RouteTask,
        _: NeckDownAttempt<'_>,
        _: &ConnectionOutcome,
        _: &ConnectionParams,
        _: &mut StageCtx,
    ) -> Option<ConnectionOutcome> {
        None
    }
}

impl Optimizer for NotPorted {
    fn id(&self) -> &'static str {
        self.id
    }
    fn run(&mut self, _: &mut RoutingBoard, _: &mut StageCtx) -> OptimizerSummary {
        OptimizerSummary::default()
    }
}

// ---------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------

/// The default `router.autorouter.algorithm` (Java
/// `RouterSettings.ALGORITHM_CURRENT`).
pub const DEFAULT_ROUTER_ALGORITHM: &str = "freerouting-router";
/// The default `router.optimizer.algorithm` (`DefaultSettings.java:196`).
pub const DEFAULT_OPTIMIZER_ALGORITHM: &str = "freerouting-optimizer";
/// The default fanout stage.
pub const DEFAULT_FANOUT: &str = "freerouting-fanout";

/// A stage factory: builds the stage for the job's settings.
pub type Factory<T> = fn(&RouterSettings) -> Box<T>;

/// The stages a `router.autorouter.algorithm` id stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bundle {
    /// The net ordering id.
    pub ordering: &'static str,
    /// The connection router id.
    pub connection_router: &'static str,
    /// The ripup policy id.
    pub ripup_policy: &'static str,
    /// The pass controller id.
    pub pass_controller: &'static str,
    /// The plane tap policy id.
    pub plane_taps: &'static str,
    /// The neck-down policy id.
    pub neck_down: &'static str,
}

/// The Freerouting bundle.
pub const FREEROUTING_BUNDLE: Bundle = Bundle {
    ordering: "freerouting-ordering",
    connection_router: "freerouting-connection",
    ripup_policy: "freerouting-ripup",
    pass_controller: "freerouting-passes",
    plane_taps: "freerouting-plane-taps",
    neck_down: "freerouting-neck-down",
};

/// The registered stages, one sorted map per trait (sorted, so listing the
/// known ids never depends on hash order).
#[derive(Clone, Default)]
pub struct StageRegistry {
    /// `router.autorouter.algorithm` bundles.
    pub bundles: BTreeMap<&'static str, Bundle>,
    /// Fanout stages.
    pub fanout: BTreeMap<&'static str, Factory<dyn FanoutStage>>,
    /// Net orderings.
    pub ordering: BTreeMap<&'static str, Factory<dyn NetOrdering>>,
    /// Connection routers.
    pub connection_router: BTreeMap<&'static str, Factory<dyn ConnectionRouter>>,
    /// Ripup policies.
    pub ripup_policy: BTreeMap<&'static str, Factory<dyn RipupPolicy>>,
    /// Pass controllers.
    pub pass_controller: BTreeMap<&'static str, Factory<dyn PassController>>,
    /// Plane tap policies.
    pub plane_taps: BTreeMap<&'static str, Factory<dyn PlaneTapPolicy>>,
    /// Neck-down policies.
    pub neck_down: BTreeMap<&'static str, Factory<dyn NeckDownPolicy>>,
    /// Special board stages.
    pub board_stages: BTreeMap<&'static str, Factory<dyn BoardStage>>,
    /// Optimizers.
    pub optimizer: BTreeMap<&'static str, Factory<dyn Optimizer>>,
    /// Report-only diagnostics (`router.special.diagnostics`, Q06).
    pub diagnostics: BTreeMap<&'static str, Factory<dyn Diagnostics>>,
}

impl fmt::Debug for StageRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StageRegistry")
            .field("bundles", &self.bundles.keys().collect::<Vec<_>>())
            .field("fanout", &self.fanout.keys().collect::<Vec<_>>())
            .field("ordering", &self.ordering.keys().collect::<Vec<_>>())
            .field(
                "connection_router",
                &self.connection_router.keys().collect::<Vec<_>>(),
            )
            .field(
                "ripup_policy",
                &self.ripup_policy.keys().collect::<Vec<_>>(),
            )
            .field(
                "pass_controller",
                &self.pass_controller.keys().collect::<Vec<_>>(),
            )
            .field("plane_taps", &self.plane_taps.keys().collect::<Vec<_>>())
            .field("neck_down", &self.neck_down.keys().collect::<Vec<_>>())
            .field(
                "board_stages",
                &self.board_stages.keys().collect::<Vec<_>>(),
            )
            .field("optimizer", &self.optimizer.keys().collect::<Vec<_>>())
            .field("diagnostics", &self.diagnostics.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl StageRegistry {
    /// The built-in stages: Java Freerouting's.
    pub fn builtin() -> StageRegistry {
        let mut r = StageRegistry::default();
        r.bundles
            .insert(DEFAULT_ROUTER_ALGORITHM, FREEROUTING_BUNDLE);
        r.ordering.insert("freerouting-ordering", |_| {
            Box::new(FreeroutingOrdering::new())
        });
        r.ripup_policy.insert("freerouting-ripup", |s| {
            Box::new(FreeroutingRipupPolicy::from_settings(s))
        });
        r.pass_controller.insert("freerouting-passes", |s| {
            Box::new(FreeroutingPassController::from_settings(s))
        });
        r.board_stages.insert("freerouting-unrouted-report", |_| {
            Box::new(UnroutedReportStage)
        });
        r.connection_router.insert("freerouting-connection", |s| {
            Box::new(FreeroutingConnectionRouter::from_settings(s))
        });
        r.plane_taps
            .insert("freerouting-plane-taps", |_| Box::new(FreeroutingPlaneTaps));
        r.neck_down
            .insert("freerouting-neck-down", |_| Box::new(FreeroutingNeckDown));
        r.fanout
            .insert(DEFAULT_FANOUT, |_| Box::new(FreeroutingFanout));
        r.optimizer.insert(DEFAULT_OPTIMIZER_ALGORITHM, |s| {
            Box::new(crate::optimize::batch::FreeroutingOptimizer::from_settings(
                s,
            ))
        });
        crate::quality::register::register(&mut r);
        r
    }

    /// Builds the pipeline `selection` names, for `settings`.
    pub fn build(
        &self,
        selection: &StageSelection,
        settings: &RouterSettings,
    ) -> Result<Pipeline, UnknownStage> {
        let diagnostics: Vec<Box<dyn Diagnostics>> = selection
            .diagnostics
            .iter()
            .map(|choice| lookup(&self.diagnostics, choice).map(|f| f(settings)))
            .collect::<Result<_, _>>()?;
        let diagnostics: SharedDiagnostics = Arc::new(Mutex::new(diagnostics));
        let mut connection_router =
            lookup(&self.connection_router, &selection.connection_router)?(settings);
        if !lock(&diagnostics).is_empty() {
            // Q06: only a job with diagnostics gets the observing wrapper.
            connection_router = Box::new(ObservedRouter {
                inner: connection_router,
                diagnostics: Arc::clone(&diagnostics),
            });
        }
        Ok(Pipeline {
            fanout: lookup(&self.fanout, &selection.fanout)?(settings),
            ordering: lookup(&self.ordering, &selection.ordering)?(settings),
            connection_router,
            ripup_policy: lookup(&self.ripup_policy, &selection.ripup_policy)?(settings),
            pass_controller: lookup(&self.pass_controller, &selection.pass_controller)?(settings),
            plane_taps: lookup(&self.plane_taps, &selection.plane_taps)?(settings),
            neck_down: lookup(&self.neck_down, &selection.neck_down)?(settings),
            board_stages: selection
                .stages
                .iter()
                .map(|choice| lookup(&self.board_stages, choice).map(|f| f(settings)))
                .collect::<Result<_, _>>()?,
            optimizer: lookup(&self.optimizer, &selection.optimizer)?(settings),
            diagnostics,
            selection: selection.clone(),
        })
    }
}

/// The diagnostics of a pipeline, shared with the connection router's
/// observing wrapper.
pub type SharedDiagnostics = Arc<Mutex<Vec<Box<dyn Diagnostics>>>>;

/// The diagnostics behind `shared`; a poisoned lock still gives them (a
/// diagnostic's panic does not hide the others).
pub fn lock(shared: &SharedDiagnostics) -> std::sync::MutexGuard<'_, Vec<Box<dyn Diagnostics>>> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A connection router that shows every connection to the diagnostics
/// before the inner router routes it and hands them its outcome after. It
/// decides nothing: `route` returns the inner router's outcome unchanged.
struct ObservedRouter {
    inner: Box<dyn ConnectionRouter>,
    diagnostics: SharedDiagnostics,
}

impl ConnectionRouter for ObservedRouter {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        for diagnostic in lock(&self.diagnostics).iter_mut() {
            diagnostic.before_connection(board, task, p, ctx);
        }
        let outcome = self.inner.route(board, task, p, ctx);
        for diagnostic in lock(&self.diagnostics).iter_mut() {
            diagnostic.after_connection(board, task, p, &outcome, ctx);
        }
        outcome
    }

    fn log_lines(&self) -> Vec<String> {
        self.inner.log_lines()
    }
}

fn lookup<F: Copy>(
    map: &BTreeMap<&'static str, F>,
    choice: &StageChoice,
) -> Result<F, UnknownStage> {
    map.get(choice.id.as_str())
        .copied()
        .ok_or_else(|| UnknownStage {
            key: choice.key,
            id: choice.id.clone(),
            known: map.keys().copied().collect(),
        })
}

/// An id in a per-stage key that no stage is registered under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownStage {
    /// The settings key (`router.autorouter.ordering`).
    pub key: &'static str,
    /// The id given.
    pub id: String,
    /// The registered ids for that key, sorted.
    pub known: Vec<&'static str>,
}

impl fmt::Display for UnknownStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown stage id '{}' in {}; known ids: {}",
            self.id,
            self.key,
            self.known.join(", ")
        )
    }
}

impl std::error::Error for UnknownStage {}

/// One resolved stage id and the key it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageChoice {
    /// The settings key that selects this stage.
    pub key: &'static str,
    /// The stage id.
    pub id: String,
    /// True if the id came from the bundle or the default, not from the
    /// stage's own key.
    pub from_default: bool,
}

impl StageChoice {
    fn new(key: &'static str, explicit: &Option<String>, default: &str) -> StageChoice {
        match explicit {
            Some(id) => StageChoice {
                key,
                id: id.clone(),
                from_default: false,
            },
            None => StageChoice {
                key,
                id: default.to_string(),
                from_default: true,
            },
        }
    }
}

/// The resolved stage ids of a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageSelection {
    /// The bundle (`router.autorouter.algorithm` after normalisation).
    pub algorithm: StageChoice,
    /// The fanout stage.
    pub fanout: StageChoice,
    /// The net ordering.
    pub ordering: StageChoice,
    /// The connection router.
    pub connection_router: StageChoice,
    /// The ripup policy.
    pub ripup_policy: StageChoice,
    /// The pass controller.
    pub pass_controller: StageChoice,
    /// The plane tap policy.
    pub plane_taps: StageChoice,
    /// The neck-down policy.
    pub neck_down: StageChoice,
    /// The extra board stages, in list order.
    pub stages: Vec<StageChoice>,
    /// The report-only diagnostics, in list order (Q06).
    pub diagnostics: Vec<StageChoice>,
    /// The optimizer (`router.optimizer.algorithm` after normalisation).
    pub optimizer: StageChoice,
    /// Java's warnings for the two Java keys it normalised.
    pub warnings: Vec<String>,
}

impl StageSelection {
    /// The selection of `settings` against the built-in registry.
    pub fn from_settings(settings: &RouterSettings) -> StageSelection {
        StageSelection::resolve(settings, &StageRegistry::builtin())
    }

    /// The selection of `settings` against `registry`. The two Java keys
    /// are normalised with Java's warnings; the per-stage keys are taken as
    /// given and checked by [`StageRegistry::build`].
    pub fn resolve(settings: &RouterSettings, registry: &StageRegistry) -> StageSelection {
        let mut warnings = Vec::new();
        let given = settings.autorouter.algorithm.as_str();
        let (algorithm, bundle) = match registry.bundles.get(given) {
            Some(bundle) => (given.to_string(), *bundle),
            None => {
                // PARITY: RoutingPipeline.java:139-150 (normalizeRouterAlgorithm).
                warnings.push(format!(
                    "The algorithm '{given}' is not supported. The default algorithm \
                     '{DEFAULT_ROUTER_ALGORITHM}' will be used instead."
                ));
                (DEFAULT_ROUTER_ALGORITHM.to_string(), FREEROUTING_BUNDLE)
            }
        };
        let given = settings.optimizer.algorithm.as_str();
        let optimizer = if registry.optimizer.contains_key(given) {
            given.to_string()
        } else {
            // PARITY: BatchOptimizer.java:94-104 (normalizeAlgorithm).
            warnings.push(format!(
                "The algorithm '{given}' is not supported by the batch autorouter. The default \
                 algorithm '{DEFAULT_OPTIMIZER_ALGORITHM}' will be used instead."
            ));
            DEFAULT_OPTIMIZER_ALGORITHM.to_string()
        };
        let a = &settings.autorouter;
        let sp = &settings.special;
        StageSelection {
            algorithm: StageChoice {
                key: "router.autorouter.algorithm",
                id: algorithm,
                from_default: false,
            },
            fanout: StageChoice::new(
                "router.fanout.algorithm",
                &settings.fanout.algorithm,
                DEFAULT_FANOUT,
            ),
            ordering: StageChoice::new("router.autorouter.ordering", &a.ordering, bundle.ordering),
            connection_router: StageChoice::new(
                "router.autorouter.connection_router",
                &a.connection_router,
                bundle.connection_router,
            ),
            ripup_policy: StageChoice::new(
                "router.autorouter.ripup_policy",
                &a.ripup_policy,
                bundle.ripup_policy,
            ),
            pass_controller: StageChoice::new(
                "router.autorouter.pass_controller",
                &a.pass_controller,
                bundle.pass_controller,
            ),
            plane_taps: StageChoice::new(
                "router.special.plane_taps",
                &sp.plane_taps,
                bundle.plane_taps,
            ),
            neck_down: StageChoice::new(
                "router.special.neck_down",
                &sp.neck_down,
                bundle.neck_down,
            ),
            stages: sp
                .stages
                .iter()
                .flatten()
                .map(|id| StageChoice {
                    key: "router.special.stages",
                    id: id.clone(),
                    from_default: false,
                })
                .chain(sp.length_matching.iter().map(|id| StageChoice {
                    key: "router.special.length_matching",
                    id: id.clone(),
                    from_default: false,
                }))
                .chain(sp.diff_pairs.iter().map(|id| StageChoice {
                    key: "router.special.diff_pairs",
                    id: id.clone(),
                    from_default: false,
                }))
                .collect(),
            diagnostics: sp
                .diagnostics
                .iter()
                .flatten()
                .map(|id| StageChoice {
                    key: "router.special.diagnostics",
                    id: id.clone(),
                    from_default: false,
                })
                .collect(),
            optimizer: StageChoice {
                key: "router.optimizer.algorithm",
                id: optimizer,
                from_default: false,
            },
            warnings,
        }
    }

    /// The `(settings key, stage id)` pairs in a fixed order, for the log
    /// and `RunSummary::stages`. The board stages come last, one pair each.
    pub fn entries(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = [
            &self.algorithm,
            &self.fanout,
            &self.ordering,
            &self.connection_router,
            &self.ripup_policy,
            &self.pass_controller,
            &self.plane_taps,
            &self.neck_down,
            &self.optimizer,
        ]
        .into_iter()
        .map(|c| (c.key.to_string(), c.id.clone()))
        .collect();
        out.extend(
            self.stages
                .iter()
                .chain(&self.diagnostics)
                .map(|c| (c.key.to_string(), c.id.clone())),
        );
        out
    }

    /// One log line naming every stage.
    pub fn log_line(&self) -> String {
        let parts: Vec<String> = self
            .entries()
            .into_iter()
            .map(|(key, id)| format!("{key}={id}"))
            .collect();
        format!("Routing stages: {}.", parts.join(", "))
    }
}

/// The built stages of a job.
pub struct Pipeline {
    /// The selection it was built from.
    pub selection: StageSelection,
    /// The fanout stage.
    pub fanout: Box<dyn FanoutStage>,
    /// The net ordering.
    pub ordering: Box<dyn NetOrdering>,
    /// The connection router.
    pub connection_router: Box<dyn ConnectionRouter>,
    /// The ripup policy.
    pub ripup_policy: Box<dyn RipupPolicy>,
    /// The pass controller.
    pub pass_controller: Box<dyn PassController>,
    /// The plane tap policy.
    pub plane_taps: Box<dyn PlaneTapPolicy>,
    /// The neck-down policy.
    pub neck_down: Box<dyn NeckDownPolicy>,
    /// The extra board stages, in list order.
    pub board_stages: Vec<Box<dyn BoardStage>>,
    /// The optimizer.
    pub optimizer: Box<dyn Optimizer>,
    /// The report-only diagnostics, in list order (Q06); the connection
    /// router feeds them when there are any.
    pub diagnostics: SharedDiagnostics,
}

impl fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pipeline")
            .field("selection", &self.selection)
            .finish_non_exhaustive()
    }
}
