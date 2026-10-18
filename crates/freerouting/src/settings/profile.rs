//! Routing profiles (N01, D10): `legacy` and `enhanced`.
//!
//! A profile is a named set of settings defaults. It is the user's starting
//! point, not a hidden mode: `legacy` gives results identical to Java
//! Freerouting at the pin, `enhanced` (the default) adds the accepted
//! improvements. Below the profile every stage and option stays
//! individually configurable; the profile only decides what each one
//! starts at.
//!
//! # The key and the apply order
//!
//! `router.profile` (`--profile`, `--router.profile=`,
//! `FREEROUTING__ROUTER__PROFILE`, `"router": {"profile": ...}`) names the
//! profile; unset is [`Profile::Enhanced`]. [`resolve`](super::resolve)
//! reads it from the highest-priority source that sets it, then merges the
//! profile's differences as [`Source::Profile`](super::Source::Profile) at
//! priority 5: after Java's defaults (0) and before the settings file (10),
//! the DSN (20), the rules file, the environment, the CLI and the testing
//! sources. So an explicit key from any source beats the profile, and a
//! settings file that sets every key routes the same under both profiles.
//! The board tuning of C03 (`apply_board_specific`) runs after the merge, as
//! for any other source.
//!
//! An unknown profile is a settings error that names the valid values
//! ([`UnknownProfile`]): `prepare_job` fails with it and the CLI's
//! `--profile` refuses it as a usage error. [`resolve`](super::resolve)
//! itself applies no profile then and reports it in
//! [`Resolved::profile`](super::Resolved::profile).
//!
//! # The table
//!
//! `legacy` has no differences: it is Java's defaults. [`ENHANCED`] lists
//! what `enhanced` changes, one [`Difference`] per settings key, each naming
//! the measurement that accepted it (N05 adds them). Adding one is one entry
//! here plus one line in the "Profile `enhanced`" section of
//! `docs/DIVERGENCES.md`; `tests/settings_profile.rs` checks that every
//! entry is a valid settings key and value. The table starts empty, so
//! `enhanced` starts equal to `legacy`.
//!
//! # Parity pins `legacy`
//!
//! Every parity test and gate (A14, connection parity, the probes,
//! `tests/settings_defaults.rs`) sets `router.profile = legacy` and checks
//! it with [`require_legacy`], so a parity run without the pin fails loudly
//! instead of comparing `enhanced` with Java. [`with_enhanced_differences`]
//! lets a test give `enhanced` a dummy difference on the current thread to
//! prove that the pins hold.

use std::cell::RefCell;
use std::fmt;

use super::fields::{RouterSettings, SettingsPatch};

/// The settings key, as the CLI and the logs name it.
pub const SETTINGS_KEY: &str = "router.profile";

/// A routing profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Profile {
    /// Java Freerouting's behaviour: the parity baseline.
    Legacy,
    /// The accepted improvements (the default).
    #[default]
    Enhanced,
}

/// One setting a profile changes from Java's default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Difference {
    /// The canonical settings path, without `router.` (`autorouter.sweep`).
    pub path: &'static str,
    /// The value, as the CLI writes it (`--router.<path>=<value>`).
    pub value: &'static str,
    /// The measurement that accepted it (a checklist item and its numbers).
    pub measurement: &'static str,
}

/// What `enhanced` changes from `legacy`. Empty until N05 accepts the first
/// improvement; each entry has a line in DIVERGENCES, "Profile `enhanced`".
pub const ENHANCED: &[Difference] = &[];

/// `legacy` changes nothing.
const LEGACY: &[Difference] = &[];

thread_local! {
    static ENHANCED_OVERRIDE: RefCell<Option<&'static [Difference]>> =
        const { RefCell::new(None) };
}

/// Runs `f` with `differences` in place of [`ENHANCED`] on this thread.
/// For tests only: it proves that a test pinned to `legacy` does not move
/// when `enhanced` does. The table is restored when `f` returns or panics.
#[doc(hidden)]
pub fn with_enhanced_differences<R>(
    differences: &'static [Difference],
    f: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<&'static [Difference]>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ENHANCED_OVERRIDE.with(|cell| *cell.borrow_mut() = self.0);
        }
    }
    let previous = ENHANCED_OVERRIDE.with(|cell| cell.borrow_mut().replace(differences));
    let _restore = Restore(previous);
    f()
}

impl Profile {
    /// Every profile, in the order the interfaces list them.
    pub const ALL: [Profile; 2] = [Profile::Legacy, Profile::Enhanced];

    /// The settings value (`legacy`, `enhanced`).
    pub fn name(self) -> &'static str {
        match self {
            Profile::Legacy => "legacy",
            Profile::Enhanced => "enhanced",
        }
    }

    /// The name shown in the GUI and the plugin (`Legacy`, `Enhanced`).
    pub fn label(self) -> &'static str {
        match self {
            Profile::Legacy => "Legacy",
            Profile::Enhanced => "Enhanced",
        }
    }

    /// One line on what the profile is for.
    pub fn description(self) -> &'static str {
        match self {
            Profile::Legacy => "Results identical to Java Freerouting.",
            Profile::Enhanced => "The accepted improvements over Java Freerouting; the default.",
        }
    }

    /// Reads a settings value, ignoring ASCII case and surrounding blanks.
    pub fn parse(text: &str) -> Result<Profile, UnknownProfile> {
        let wanted = text.trim();
        Profile::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(wanted))
            .ok_or_else(|| UnknownProfile {
                given: text.to_string(),
            })
    }

    /// The profile a patch names; unset is the default, `enhanced`.
    pub fn of_patch(patch: &SettingsPatch) -> Result<Profile, UnknownProfile> {
        patch
            .profile
            .as_deref()
            .map_or(Ok(Profile::default()), Profile::parse)
    }

    /// The profile of resolved settings; unset is `enhanced`.
    pub fn of_settings(settings: &RouterSettings) -> Result<Profile, UnknownProfile> {
        settings
            .profile
            .as_deref()
            .map_or(Ok(Profile::default()), Profile::parse)
    }

    /// The settings this profile changes from Java's defaults.
    pub fn differences(self) -> &'static [Difference] {
        match self {
            Profile::Legacy => LEGACY,
            Profile::Enhanced => ENHANCED_OVERRIDE.with(|cell| cell.borrow().unwrap_or(ENHANCED)),
        }
    }

    /// The differences as a settings patch (the source merged at priority
    /// 5). Panics on an entry that is no valid setting, which
    /// `tests/settings_profile.rs` rules out for the built-in table.
    pub fn patch(self) -> SettingsPatch {
        let mut patch = SettingsPatch::default();
        for difference in self.differences() {
            if let Err(e) = patch.set_path(difference.path, difference.value) {
                panic!(
                    "profile {}: {} = {}: {e}",
                    self.name(),
                    difference.path,
                    difference.value
                );
            }
        }
        patch
    }

    /// Java's defaults with this profile's differences on top: the value
    /// each setting starts at under the profile, before any settings file,
    /// design or flag. The interfaces compare with it to mark the values
    /// that differ from the profile, and reset to it.
    pub fn defaults(self, cores: usize) -> SettingsPatch {
        let mut patch = super::fields::default_patch(cores);
        patch.apply_from(&self.patch());
        patch.profile = Some(self.name().to_string());
        patch
    }
}

/// Moves `patch` from the profile it names to `to` (an interface's profile
/// selector): every field that still has the old profile's value takes
/// `to`'s, and every other value (the user's, the design's, the board
/// tuning's) stays. Sets `router.profile` to `to`.
pub fn rebase(patch: &mut SettingsPatch, to: Profile, cores: usize) {
    let from = Profile::of_patch(patch).unwrap_or_default();
    let (old, new) = (from.defaults(cores), to.defaults(cores));
    for field in super::fields::FIELDS.iter().filter(|f| f.path != "profile") {
        let now = (field.get)(patch);
        let target = (field.get)(&new);
        if now == (field.get)(&old) && now != target {
            match target {
                Some(value) => (field.set)(patch, value),
                None => (field.clear)(patch),
            }
        }
    }
    patch.profile = Some(to.name().to_string());
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A `router.profile` value that names no profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownProfile {
    /// The value given.
    pub given: String,
}

impl fmt::Display for UnknownProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let valid: Vec<&str> = Profile::ALL.iter().map(|p| p.name()).collect();
        write!(
            f,
            "{SETTINGS_KEY}: unknown profile '{}' (valid profiles: {})",
            self.given,
            valid.join(", ")
        )
    }
}

impl std::error::Error for UnknownProfile {}

/// The profile the sources choose: the value of the highest-priority source
/// that sets `router.profile` (the last one among equal priorities, as the
/// merge's stable sort keeps them), else the default.
pub fn chosen<'a>(
    sources: impl IntoIterator<Item = &'a (super::Source, SettingsPatch)>,
) -> Result<Profile, UnknownProfile> {
    let mut best: Option<(i32, &str)> = None;
    for (source, patch) in sources {
        if let Some(name) = patch.profile.as_deref()
            && best.is_none_or(|(priority, _)| source.priority() >= priority)
        {
            best = Some((source.priority(), name));
        }
    }
    best.map_or(Ok(Profile::default()), |(_, name)| Profile::parse(name))
}

/// Checks that resolved settings are pinned to `legacy`: the parity tests
/// and gates call this so that a run without the pin fails loudly.
pub fn require_legacy(settings: &RouterSettings) -> Result<(), String> {
    match settings.profile.as_deref().map(Profile::parse) {
        Some(Ok(Profile::Legacy)) => Ok(()),
        other => Err(format!(
            "a parity run must set {SETTINGS_KEY} = legacy explicitly; it has {}",
            match other {
                None => "no profile (the default, enhanced)".to_string(),
                Some(Ok(p)) => format!("'{p}'"),
                Some(Err(e)) => e.to_string(),
            }
        )),
    }
}

/// A settings patch that only pins `legacy`, for the parity tests.
pub fn legacy_pin() -> SettingsPatch {
    SettingsPatch {
        profile: Some(Profile::Legacy.name().to_string()),
        ..SettingsPatch::default()
    }
}
