//! The rules file writer.

use super::Writer;
use crate::ast::*;

pub(crate) fn write_rules(rules: &RulesFile) -> String {
    let mut w = Writer::new();
    w.open("rules PCB");
    w.name(&rules.design);
    for item in &rules.items {
        match item {
            RulesItem::SnapAngle(angle) => w.snap_angle(*angle),
            RulesItem::AutorouteSettings(settings) => w.autoroute_settings(settings),
            RulesItem::Rule(list) => {
                w.open("rule");
                w.rule_items(list);
                w.close();
            }
            RulesItem::Layer { name, rules } => {
                w.open("layer");
                w.name(name);
                w.rules(rules);
                w.close();
            }
            RulesItem::Padstack(padstack) => w.padstack(padstack),
            RulesItem::Via(via) => w.network_via(via),
            RulesItem::ViaRule(rule) => w.via_rule(rule),
            RulesItem::Class(class) => w.class(class),
        }
    }
    w.finish()
}
