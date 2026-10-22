//! Settings keys the port had and removed after a failed measurement. Java
//! never had them.
//!
//! A removed key is refused, not ignored, so a script or settings file that
//! still sets one learns that the stage is gone instead of routing without
//! it unawares. The error ([`RemovedSetting::message`]) names the key, when
//! and why it went, and the keys of its section that remain:
//! - the `freerouting-rs` command line (`--router.autorouter.portfolio=4`)
//!   refuses it as a usage error (exit 2) before it loads anything;
//! - a settings JSON file that sets one fails to read with that text: an
//!   explicit `--settings` file is refused, the default file is skipped
//!   with a warning, as for any malformed file;
//! - [`SettingsPatch::set_path`](super::SettingsPatch::set_path) (the
//!   engine's CLI parse, environment variables) returns
//!   [`SetError::Removed`](super::SetError::Removed); those sources log a
//!   setter error as a warning, as Java does.
//!
//! The keys:
//! - `autorouter.portfolio`, `autorouter.restarts` and
//!   `autorouter.portfolio_memory_mb`: Q12's pass-1 portfolio and its
//!   squeaky-wheel restarts. Q12.5's registered run rejected it (hard guards
//!   failed on three boards, the total moved within noise, the work guard
//!   failed on six rows), and its fail branch removed `quality/portfolio/`
//!   on 2026-10-20 (`docs/roadmap/tasks/archive/Q12-pass-1-portfolio.md`).
//! - `autorouter.restarts_from_best`: Q23's restarts from the best board.
//!   Q23.4's registered run rejected it (3 of 5 boards improved where the
//!   paired rule needs 4, and the work guard failed on CPU-85 at 2.16 times
//!   the base run's maze work), and its fail branch removed
//!   `quality/restart.rs` on 2026-10-22
//!   (`docs/roadmap/tasks/Q23-restart-from-the-best-board.md`).

use super::fields::FIELDS;
use super::path::name_matches;

/// One removed key.
#[derive(Debug, PartialEq, Eq)]
pub struct RemovedSetting {
    /// The section (`autorouter`).
    pub section: &'static str,
    /// The JSON name inside the section (`portfolio_memory_mb`).
    pub name: &'static str,
    /// The camel-case name it also had (`portfolioMemoryMb`).
    pub java_name: &'static str,
    /// The date it was removed.
    pub removed: &'static str,
    /// Why, in a few words, with the checklist item that decided it.
    pub why: &'static str,
}

const Q12_WHY: &str = "the pass-1 portfolio failed its measurement (Q12.5)";
const Q23_WHY: &str = "the restarts from the best board failed their measurement (Q23.4)";

/// Every removed key.
pub const REMOVED: &[RemovedSetting] = &[
    RemovedSetting {
        section: "autorouter",
        name: "portfolio",
        java_name: "portfolio",
        removed: "2026-10-20",
        why: Q12_WHY,
    },
    RemovedSetting {
        section: "autorouter",
        name: "restarts",
        java_name: "restarts",
        removed: "2026-10-20",
        why: Q12_WHY,
    },
    RemovedSetting {
        section: "autorouter",
        name: "portfolio_memory_mb",
        java_name: "portfolioMemoryMb",
        removed: "2026-10-20",
        why: Q12_WHY,
    },
    RemovedSetting {
        section: "autorouter",
        name: "restarts_from_best",
        java_name: "restartsFromBest",
        removed: "2026-10-22",
        why: Q23_WHY,
    },
];

impl RemovedSetting {
    /// The CLI property, `router.autorouter.portfolio`.
    pub fn property(&self) -> String {
        format!("router.{}.{}", self.section, self.name)
    }

    /// The refusal: the key, when and why it went, and the valid keys of its
    /// section, in table order.
    pub fn message(&self) -> String {
        let valid: Vec<&str> = FIELDS
            .iter()
            .filter(|f| f.section == self.section)
            .map(|f| f.name)
            .collect();
        format!(
            "{} was removed on {}: {}; valid router.{} settings: {}",
            self.property(),
            self.removed,
            self.why,
            self.section,
            valid.join(", ")
        )
    }
}

/// The removed key `segment` of `section`, matched as the CLI matches a
/// field (snake or camel case, any letter case).
pub fn find(section: &str, segment: &str) -> Option<&'static RemovedSetting> {
    REMOVED
        .iter()
        .find(|r| r.section == section && name_matches(segment, Some(r.name), &[], r.java_name))
}

/// The removed key `name` of `section`, matched as a JSON file matches a
/// field (its JSON or camel-case name, exactly).
pub fn find_json(section: &str, name: &str) -> Option<&'static RemovedSetting> {
    REMOVED
        .iter()
        .find(|r| r.section == section && (r.name == name || r.java_name == name))
}

/// The removed key a CLI property names (`router.autorouter.portfolio`,
/// `--router.autorouter.restarts` without its dashes), if any.
pub fn find_property(property: &str) -> Option<&'static RemovedSetting> {
    let relative = property.strip_prefix("router.")?;
    let (section, name) = relative.split_once('.')?;
    let section = super::SECTIONS
        .iter()
        .find(|s| name_matches(section, Some(s.name), &[], s.java_name))?;
    find(section.name, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_keys_are_no_fields_any_more() {
        for r in REMOVED {
            assert!(
                !FIELDS
                    .iter()
                    .any(|f| f.section == r.section && f.name == r.name),
                "{}",
                r.property()
            );
        }
    }

    #[test]
    fn properties_match_in_either_case() {
        let portfolio = &REMOVED[0];
        assert_eq!(
            find_property("router.autorouter.portfolio"),
            Some(portfolio)
        );
        assert_eq!(
            find_property("router.autorouter.PORTFOLIO"),
            Some(portfolio)
        );
        assert_eq!(
            find_property("router.autorouter.portfolioMemoryMb"),
            Some(&REMOVED[2])
        );
        assert_eq!(find_property("router.autorouter.max_passes"), None);
        assert_eq!(find_property("router.portfolio"), None);
        assert_eq!(find_property("autorouter.portfolio"), None);
        assert_eq!(
            find_json("autorouter", "portfolioMemoryMb"),
            Some(&REMOVED[2])
        );
        assert_eq!(find_json("autorouter", "Portfolio"), None);
    }
}
