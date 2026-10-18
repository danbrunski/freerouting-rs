//! The budget hook of the maze search (Q08): what the search asks when its
//! work limit fires, so a policy may grant more work.
//!
//! Java's search stops the moment its `WorkLimit` is exceeded
//! (`AutorouteEngine.isStopRequested`), and so does the port's without a
//! policy: [`Session::is_stop_requested`](super::Session::is_stop_requested)
//! only consults a [`BudgetPolicy`] when one is set
//! ([`Session::set_budget_policy`](super::Session::set_budget_policy)), and
//! every Java path sets none. With one, the session tracks the smallest
//! destination-distance estimate of the elements it pops (the element's
//! `sortingValue - expansionValue`, the lower bound `DestinationDistance`
//! added to its cost) and, on the first stop check past the limit, hands the
//! policy a [`BudgetProgress`]. [`LimitDecision::Extend`] raises the
//! session's limit by that many units ([`WorkLimit::extend`]) and the search
//! goes on; [`LimitDecision::Stop`] ends it as Java's does, cut-off line
//! included.
//!
//! The decision may read only search state and work counts, never time, so a
//! policy keeps the run deterministic (D2, D5).
//!
//! [`WorkLimit::extend`]: crate::board::search::WorkLimit::extend

/// What the session tells a policy when it is set, before any work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetStart {
    /// The units the session's work limit allows (its base budget); 0 when
    /// the session has no limit.
    pub base_units: u64,
    /// The trace width of the connection in board units: twice the widest
    /// trace half width of the control's active layers.
    pub trace_width: i64,
}

/// The search state when the work limit fires.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BudgetProgress {
    /// The estimate of the first element popped in the session, `None`
    /// before the first pop (the limit fired during the initialisation).
    pub first_estimate: Option<f64>,
    /// The smallest estimate of any element popped so far.
    pub best_estimate: Option<f64>,
    /// Units done since the limit was created.
    pub units_done: u64,
    /// Units the limit allows now, extensions included.
    pub max_units: u64,
}

/// A policy's answer to a fired limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitDecision {
    /// End the search (Java's behaviour).
    Stop,
    /// Allow this many more units.
    Extend(u64),
}

/// Decides, when a maze search's work limit fires, whether it may go on.
/// The session calls [`on_start`](Self::on_start) once when the policy is
/// set and [`on_limit`](Self::on_limit) each time the limit fires.
///
/// `'static`, so a `&mut dyn BudgetPolicy` holds no borrow of its own and
/// the session stays covariant in its board borrow.
pub trait BudgetPolicy: 'static {
    /// A new session begins.
    fn on_start(&mut self, start: BudgetStart);
    /// The limit fired.
    fn on_limit(&mut self, progress: BudgetProgress) -> LimitDecision;
}

/// Freerouting's policy: always [`LimitDecision::Stop`]. A session with it
/// routes exactly as a session without a policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StopAtLimit;

impl BudgetPolicy for StopAtLimit {
    fn on_start(&mut self, _start: BudgetStart) {}

    fn on_limit(&mut self, _progress: BudgetProgress) -> LimitDecision {
        LimitDecision::Stop
    }
}
