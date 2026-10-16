//! The static handlers of Java's `WindowAutorouteParameter` that
//! `DialogInteractionHandlersTest` calls: a checkbox sets one member of the
//! router settings. They are here, not in the autorouter settings dialog
//! (G07), so that the Java tests have a home before that dialog exists; the
//! dialog calls them. The number and timeout normalizers are in
//! [`super::input`].
//!
//! Java's `RouterSettings` setters map to the members of the port's
//! [`SettingsPatch`]: `setViasAllowed` is `allowed_via_types`,
//! `setEnabled` the autorouter's `enabled`, `setOptimizerEnabled` the
//! optimizer's and `setFanoutEnabled` the fanout's.

use freerouting::settings::SettingsPatch;

/// `applyViasAllowedSelection`.
pub fn apply_vias_allowed_selection(settings: &mut SettingsPatch, selected: bool) {
    settings.set_vias_allowed(selected);
}

/// `applyFanoutEnabledSelection`.
pub fn apply_fanout_enabled_selection(settings: &mut SettingsPatch, selected: bool) {
    settings.fanout.enabled = Some(selected);
}

/// `applyAutorouteEnabledSelection`.
pub fn apply_autoroute_enabled_selection(settings: &mut SettingsPatch, selected: bool) {
    settings.autorouter.enabled = Some(selected);
}

/// `applyOptimizerEnabledSelection`.
pub fn apply_optimizer_enabled_selection(settings: &mut SettingsPatch, selected: bool) {
    settings.optimizer.enabled = Some(selected);
}

#[cfg(test)]
mod tests {
    use super::*;

    // DialogInteractionHandlersTest.autoRouterSettingsCheckboxInteractionsInvokeExpectedSetters
    #[test]
    fn checkbox_handlers_set_the_expected_members() {
        let mut settings = SettingsPatch::default();
        apply_vias_allowed_selection(&mut settings, true);
        apply_autoroute_enabled_selection(&mut settings, false);
        apply_optimizer_enabled_selection(&mut settings, true);
        apply_fanout_enabled_selection(&mut settings, true);
        assert_eq!(settings.allowed_via_types, Some(true));
        assert_eq!(settings.autorouter.enabled, Some(false));
        assert_eq!(settings.optimizer.enabled, Some(true));
        assert_eq!(settings.fanout.enabled, Some(true));
    }
}
