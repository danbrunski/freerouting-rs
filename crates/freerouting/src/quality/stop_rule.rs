//! `QuantumStop` (Q04.1): a pass controller that ends the routing passes
//! when Java's periodic restore checks stop gaining score. Not Java;
//! selected with `router.autorouter.pass_controller = quantum-stop`, so a
//! job that does not select it is Java's.
//!
//! It wraps [`FreeroutingPassController`] and forwards every call. The one
//! decision it adds is in [`after_pass`](PassController::after_pass): at
//! each of Java's restore checks (`AutorouteBatchLoop.java:357-366` at
//! `a773a70ec`: a pass `p >= 8` with `p % 4 == 0` and at least 8 boards in
//! the history; a requested router stop is not a periodic check) it
//! measures the gain since the previous check, and after `grace` checks it
//! stops once `checks` consecutive checks gained less than the quantum.
//! Every other decision (the restore, the fanout recovery, the last-mile
//! ripup, Java's own stops, the final best-board restore and tail removal)
//! stays Java's.
//!
//! **The rule, in Java's `f32`.** At a check on pass `p` the best score is
//! the larger of the history's best ([`PassReport::history_max_score`],
//! Java `getMaxScore`) and the score after pass `p` (the board of pass `p`
//! is not in the history yet). The gain is that best minus the best at the
//! previous check, or minus the score after pass 1 at the first check. A
//! gain below `max(quantum_abs, quantum_pct * |best|)` counts as none; a
//! gain equal to it counts. Checks `1..=grace` never count (the misses stay
//! 0); from check `grace + 1` a check without a gain adds a miss and a check
//! with one resets the misses to 0, and `checks` misses stop.
//!
//! **Order.** The inner controller decides first, so its counters move as
//! Java's. A [`PassDecision::Stop`] from it always wins. Any other first
//! decision of a pass on which the rule fires becomes
//! [`StopReason::Quantum`]: a restore, a fanout recovery and a last-mile
//! ripup only prepare the next pass, which the stop cancels, and applying a
//! ripup first would leave the final best-board restore without this pass's
//! board (it is not in the history yet). The loop's finish then restores
//! the history's best board when it beats the final one, as for every Java
//! stop, so a run that stops after its last improvement ends on the board a
//! default run ends on (Q04.2). Like Java's stagnation stops, the stop
//! requests a router stop, which skips the final tail removal.
//!
//! **State** is counters and scores only (no board), so a restore changes
//! nothing here.
//!
//! The defaults (grace 0, checks 2, quantum 1.0 and 1 %) are preregistered
//! in `docs/roadmap/prereg/Q04.json`; with them a flat score stops at pass
//! 12, where Java's controller alone stops at pass 18.

use crate::autoroute::batch::control::FreeroutingPassController;
use crate::autoroute::batch::{STOP_AT_PASS_MINIMUM, STOP_AT_PASS_MODULO};
use crate::autoroute::stages::{
    ActionOutcome, FinishDecision, PassController, PassDecision, PassReport, PassStart, StopReason,
    UnknownStage,
};
use crate::settings::RouterSettings;

/// The registry id (`router.autorouter.pass_controller`).
pub const CONTROLLER_ID: &str = "quantum-stop";
/// Checks that never count, unset `quantum_stop.grace`.
pub const DEFAULT_GRACE: u32 = 0;
/// Consecutive checks without a gain that stop, unset `quantum_stop.checks`.
pub const DEFAULT_CHECKS: u32 = 2;
/// The absolute quantum, unset `quantum_stop.quantum_abs`.
pub const DEFAULT_QUANTUM_ABS: f32 = 1.0;
/// The relative quantum, unset `quantum_stop.quantum_pct`.
pub const DEFAULT_QUANTUM_PCT: f32 = 0.01;

/// The stop rule's parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopRule {
    /// Checks that never count.
    pub grace: u32,
    /// Consecutive checks without a gain that stop (at least 1).
    pub checks: u32,
    /// The smallest gain that counts.
    pub quantum_abs: f32,
    /// The smallest gain as a fraction of the best score's magnitude.
    pub quantum_pct: f32,
}

impl Default for StopRule {
    fn default() -> Self {
        StopRule {
            grace: DEFAULT_GRACE,
            checks: DEFAULT_CHECKS,
            quantum_abs: DEFAULT_QUANTUM_ABS,
            quantum_pct: DEFAULT_QUANTUM_PCT,
        }
    }
}

impl StopRule {
    /// The rule `settings` give (unset fields take the defaults), or the
    /// first invalid field: `grace` below 0, `checks` below 1, or a quantum
    /// that is negative or not finite.
    pub fn from_settings(settings: &RouterSettings) -> Result<StopRule, UnknownStage> {
        let q = &settings.quantum_stop;
        let count = |value: Option<i32>, key: &'static str, min: i32, known| match value {
            Some(n) if n < min => Err(UnknownStage {
                key,
                id: n.to_string(),
                known: vec![known],
            }),
            Some(n) => Ok(Some(n as u32)),
            None => Ok(None),
        };
        let fraction = |value: Option<f64>, key: &'static str| match value {
            Some(x) if !(x.is_finite() && x >= 0.0) => Err(UnknownStage {
                key,
                id: x.to_string(),
                known: vec!["a finite value of 0 or more"],
            }),
            // The score is Java's `f32`, so the quantum is too.
            Some(x) => Ok(Some(x as f32)),
            None => Ok(None),
        };
        let grace = count(q.grace, "router.quantum_stop.grace", 0, "0 or more checks")?;
        let checks = count(
            q.checks,
            "router.quantum_stop.checks",
            1,
            "1 or more checks",
        )?;
        let quantum_abs = fraction(q.quantum_abs, "router.quantum_stop.quantum_abs")?;
        let quantum_pct = fraction(q.quantum_pct, "router.quantum_stop.quantum_pct")?;
        Ok(StopRule {
            grace: grace.unwrap_or(DEFAULT_GRACE),
            checks: checks.unwrap_or(DEFAULT_CHECKS),
            quantum_abs: quantum_abs.unwrap_or(DEFAULT_QUANTUM_ABS),
            quantum_pct: quantum_pct.unwrap_or(DEFAULT_QUANTUM_PCT),
        })
    }

    /// The smallest gain that counts when the best score is `best`, in
    /// `f32`: `max(quantum_abs, quantum_pct * |best|)`.
    pub fn quantum(&self, best: f32) -> f32 {
        self.quantum_abs.max(self.quantum_pct * best.abs())
    }
}

/// Fails a job whose `quantum_stop` settings are invalid, before routing
/// (whatever pass controller it selects).
pub fn check_settings(settings: &RouterSettings) -> Result<(), UnknownStage> {
    StopRule::from_settings(settings).map(|_| ())
}

/// One restore check as the rule saw it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Check {
    /// The pass.
    pub pass: u32,
    /// The best score at the check.
    pub best: f32,
    /// The gain since the previous check (the first: since pass 1).
    pub gain: f32,
    /// The quantum the gain was compared with.
    pub quantum: f32,
    /// False for a grace check.
    pub counted: bool,
    /// Consecutive misses after this check.
    pub misses: u32,
}

/// The `quantum-stop` pass controller (see the module docs).
#[derive(Clone, Debug)]
pub struct QuantumStop {
    inner: FreeroutingPassController,
    rule: StopRule,
    /// The score after pass 1.
    pass_one: Option<f32>,
    /// The best score at the last check.
    best_at_last_check: Option<f32>,
    misses: u32,
    checks: Vec<Check>,
}

impl QuantumStop {
    /// Wraps `inner` with `rule`.
    pub fn new(inner: FreeroutingPassController, rule: StopRule) -> Self {
        QuantumStop {
            inner,
            rule,
            pass_one: None,
            best_at_last_check: None,
            misses: 0,
            checks: Vec::new(),
        }
    }

    /// The controller for `settings`. Invalid `quantum_stop` values make
    /// [`check_settings`] fail the job before routing; here they fall back
    /// to the defaults.
    pub fn from_settings(settings: &RouterSettings) -> Self {
        QuantumStop::new(
            FreeroutingPassController::from_settings(settings),
            StopRule::from_settings(settings).unwrap_or_default(),
        )
    }

    /// The rule.
    pub fn rule(&self) -> StopRule {
        self.rule
    }

    /// The checks so far, in pass order.
    pub fn checks(&self) -> &[Check] {
        &self.checks
    }

    /// The wrapped Java controller.
    pub fn inner(&self) -> &FreeroutingPassController {
        &self.inner
    }

    /// True if `r` is a pass of Java's periodic restore check
    /// (`AutorouteBatchLoop.java:357-359`, without the requested stop).
    fn is_check(r: &PassReport) -> bool {
        r.history_size >= STOP_AT_PASS_MINIMUM as usize
            && r.pass >= STOP_AT_PASS_MINIMUM
            && r.pass % STOP_AT_PASS_MODULO == 0
    }

    /// Runs the rule on `r`; true if it stops.
    fn check(&mut self, r: &PassReport) -> bool {
        if r.pass == 1 {
            self.pass_one = Some(r.score);
        }
        if !Self::is_check(r) {
            return false;
        }
        let best = r.history_max_score.max(r.score);
        let reference = self.best_at_last_check.or(self.pass_one).unwrap_or(best);
        let gain = best - reference;
        let quantum = self.rule.quantum(best);
        self.best_at_last_check = Some(best);
        let counted = self.checks.len() as u32 >= self.rule.grace;
        if counted {
            if gain < quantum {
                self.misses += 1;
            } else {
                self.misses = 0;
            }
        }
        self.checks.push(Check {
            pass: r.pass,
            best,
            gain,
            quantum,
            counted,
            misses: self.misses,
        });
        counted && self.misses >= self.rule.checks.max(1)
    }
}

impl PassController for QuantumStop {
    fn id(&self) -> &'static str {
        CONTROLLER_ID
    }

    fn before_pass(&mut self, pass: u32, timed_out: bool) -> PassStart {
        self.inner.before_pass(pass, timed_out)
    }

    fn after_pass(&mut self, r: &PassReport) -> PassDecision {
        let decision = self.inner.after_pass(r);
        let fires = self.check(r);
        match decision {
            PassDecision::Stop(_) => decision,
            _ if fires => PassDecision::Stop(StopReason::Quantum),
            _ => decision,
        }
    }

    fn after_action(&mut self, outcome: &ActionOutcome) -> PassDecision {
        self.inner.after_action(outcome)
    }

    fn finish(
        &mut self,
        final_score: f32,
        history_max_score: f32,
        router_stopped: bool,
    ) -> FinishDecision {
        self.inner
            .finish(final_score, history_max_score, router_stopped)
    }
}
