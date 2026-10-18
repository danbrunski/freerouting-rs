//! Java's `SettingsMerger.merge` and `ReflectionUtil.copyFields`.

use std::collections::BTreeMap;

use super::fields::{FIELDS, RouterSettings, SettingsPatch, default_patch};
use super::profile::{self, Profile, UnknownProfile};
use super::types::LayerSettings;
use super::validate::validate;

/// Where a settings patch came from. The priority decides the merge order
/// (Java's `SettingsSource.getPriority()`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// `DefaultSettings` (priority 0).
    Defaults,
    /// The differences of the chosen routing profile (5; N01, not Java).
    /// [`resolve`] adds it; a caller never passes it.
    Profile(Profile),
    /// `freerouting.json` (10).
    ConfigFile,
    /// The DSN's `autoroute_settings` (20).
    Dsn,
    /// A rules file's `autoroute_settings` (40).
    RulesFile,
    /// `FREEROUTING__ROUTER__*` environment variables (55).
    Environment,
    /// Command-line arguments (60).
    Cli,
    /// The second merge of `RoutingJobScheduler` (`ApiSettings`, 70).
    Api,
    /// Java's `TestingSettings` (80).
    Testing,
    /// Any other source, for tests and later additions.
    Custom { name: String, priority: i32 },
}

impl Source {
    /// `getPriority()`.
    pub fn priority(&self) -> i32 {
        match self {
            Source::Defaults => 0,
            Source::Profile(_) => 5,
            Source::ConfigFile => 10,
            Source::Dsn => 20,
            Source::RulesFile => 40,
            Source::Environment => 55,
            Source::Cli => 60,
            Source::Api => 70,
            Source::Testing => 80,
            Source::Custom { priority, .. } => *priority,
        }
    }

    /// A short label for log lines.
    pub fn label(&self) -> &str {
        match self {
            Source::Defaults => "defaults",
            Source::Profile(Profile::Legacy) => "profile legacy",
            Source::Profile(Profile::Enhanced) => "profile enhanced",
            Source::ConfigFile => "config file",
            Source::Dsn => "DSN",
            Source::RulesFile => "rules file",
            Source::Environment => "environment",
            Source::Cli => "CLI",
            Source::Api => "API",
            Source::Testing => "testing",
            Source::Custom { name, .. } => name,
        }
    }

    /// `getSourceName()`.
    pub fn name(&self) -> &str {
        match self {
            Source::Defaults => "Default Settings",
            Source::Profile(Profile::Legacy) => "Profile legacy",
            Source::Profile(Profile::Enhanced) => "Profile enhanced",
            Source::ConfigFile => "freerouting.json",
            Source::Dsn => "DSN file",
            Source::RulesFile => "RULES file",
            Source::Environment => "Environment Variables",
            Source::Cli => "CLI Arguments",
            Source::Api => "API",
            Source::Testing => "Testing Settings",
            Source::Custom { name, .. } => name,
        }
    }
}

/// Machine facts that the merge depends on. Nothing that decides results
/// may read a thread count (D2), so the core count is explicit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveContext {
    /// `Runtime.availableProcessors()`.
    pub cores: usize,
}

impl ResolveContext {
    /// The core count of this machine (`Runtime.availableProcessors()`). Only
    /// the thread-count defaults depend on it; routing results never do (D2).
    /// Tests pass a fixed count instead.
    pub fn from_machine() -> ResolveContext {
        ResolveContext {
            cores: std::thread::available_parallelism().map_or(1, |n| n.get()),
        }
    }
}

/// The result of [`resolve`].
#[derive(Clone, Debug)]
pub struct Resolved {
    /// The merged and validated patch (Java's merged `RouterSettings`, with
    /// its nulls).
    pub patch: SettingsPatch,
    /// The patch read through Java's getter fallbacks.
    pub settings: RouterSettings,
    /// For each canonical path (and `layers`, `costs_initialized`), the last
    /// source that set it.
    pub provenance: BTreeMap<&'static str, Source>,
    /// Warnings from the merge itself and from validation.
    pub warnings: Vec<String>,
    /// The core count the merge was resolved with (it normalises
    /// `max_threads`).
    pub cores: usize,
    /// The routing profile the sources chose (N01), or why none applies.
    pub profile: Result<Profile, UnknownProfile>,
}

impl Resolved {
    /// One debug line per field set by some source, in table order, such as
    /// `scoring.start_ripup_costs = 1500 (freerouting.json)`.
    pub fn provenance_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for field in FIELDS {
            if let (Some(source), Some(value)) =
                (self.provenance.get(field.path), (field.get)(&self.patch))
            {
                lines.push(format!("{} = {value} ({})", field.path, source.label()));
            }
        }
        lines
    }
}

impl SettingsPatch {
    /// `ReflectionUtil.copyFields(src, self)`.
    ///
    /// - A `Some` overwrites and a `None` never does. That includes
    ///   `Some(false)`, `Some(0)` and `Some(vec![])`: wrapper types copy any
    ///   non-null value and arrays are replaced whole.
    /// - `layers` merges element by element when this patch's array is at
    ///   least as long as the source's. Otherwise it is rebuilt from the
    ///   source alone, and the lower-priority per-layer values are lost.
    ///
    /// Returns the number of fields copied.
    pub fn apply_from(&mut self, src: &SettingsPatch) -> usize {
        let mut changed = 0;
        for field in FIELDS {
            if let Some(value) = (field.get)(src) {
                (field.set)(self, value);
                changed += 1;
            }
        }
        if let Some(source_layers) = src.layers.as_ref() {
            let target_len = self.layers.as_ref().map_or(0, Vec::len);
            if target_len >= source_layers.len() {
                if let Some(target) = self.layers.as_mut() {
                    for (t, s) in target.iter_mut().zip(source_layers) {
                        t.apply_from(s);
                    }
                }
            } else {
                // Rebuilt from the source alone.
                self.layers = Some(
                    source_layers
                        .iter()
                        .map(|s| {
                            let mut element = LayerSettings::default();
                            element.apply_from(s);
                            element
                        })
                        .collect(),
                );
            }
            changed += source_layers.len();
        }
        if src.costs_initialized.is_some() {
            self.costs_initialized = src.costs_initialized;
            changed += 1;
        }
        if let Some(warnings) = src.validation_warnings.as_ref() {
            self.validation_warnings = Some(warnings.clone());
            changed += 1;
        }
        changed
    }

    /// Java's `RouterSettings.clone()` of a merge base. The clone does not
    /// carry `result_json`.
    fn clone_as_merge_base(&self) -> SettingsPatch {
        let mut base = self.clone();
        base.result_json = None;
        base
    }

    /// The canonical paths this patch sets.
    pub fn set_paths(&self) -> Vec<&'static str> {
        let mut paths: Vec<&'static str> = FIELDS
            .iter()
            .filter(|f| (f.get)(self).is_some())
            .map(|f| f.path)
            .collect();
        if self.layers.is_some() {
            paths.push("layers");
        }
        if self.costs_initialized.is_some() {
            paths.push("costs_initialized");
        }
        paths
    }
}

/// `SettingsMerger.merge()`: sorts the sources by priority (a stable sort,
/// so ties keep their order), clones the first as the base, applies the rest
/// in order and validates.
///
/// Not Java: first the routing profile (N01, [`profile`]) the sources name is
/// added as [`Source::Profile`] at priority 5, when it changes anything. An
/// unknown profile adds nothing and is reported in [`Resolved::profile`] and
/// as a warning; `prepare_job` turns it into an error.
pub fn resolve(sources: &[(Source, SettingsPatch)], ctx: &ResolveContext) -> Resolved {
    let mut warnings = Vec::new();
    let chosen = profile::chosen(sources);
    let profile_source = match &chosen {
        Ok(profile) => {
            let patch = profile.patch();
            (patch != SettingsPatch::default()).then_some((Source::Profile(*profile), patch))
        }
        Err(e) => {
            warnings.push(e.to_string());
            None
        }
    };
    let mut sorted: Vec<&(Source, SettingsPatch)> = sources.iter().collect();
    if let Some(source) = profile_source.as_ref() {
        sorted.push(source);
    }
    sorted.sort_by_key(|(source, _)| source.priority());

    let mut provenance = BTreeMap::new();
    let mut merged: Option<SettingsPatch> = None;
    for (source, patch) in sorted {
        for path in patch.set_paths() {
            provenance.insert(path, source.clone());
        }
        match merged.as_mut() {
            None => merged = Some(patch.clone_as_merge_base()),
            Some(m) => {
                m.apply_from(patch);
            }
        }
    }
    let mut patch = match merged {
        Some(patch) => patch,
        None => {
            warnings.push("No settings sources provided, using defaults".to_string());
            SettingsPatch::default()
        }
    };
    warnings.extend(validate(&mut patch, ctx.cores));
    let settings = RouterSettings::from_patch(&patch, ctx.cores);
    Resolved {
        patch,
        settings,
        provenance,
        warnings,
        cores: ctx.cores,
        profile: chosen,
    }
}

/// `DefaultSettings.getSettings()`.
pub fn defaults(ctx: &ResolveContext) -> SettingsPatch {
    default_patch(ctx.cores)
}
