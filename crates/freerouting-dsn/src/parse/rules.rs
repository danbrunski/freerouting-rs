//! The rules file: `(rules PCB name ...)`.

use super::{Item, Parser};
use crate::ast::*;
use crate::error::{Error, Result};

pub(crate) fn parse_rules(parser: &mut Parser<'_>) -> Result<RulesFile> {
    let (keyword, pos) = parser.header()?;
    if keyword != "rules" {
        return Err(Error::new(
            pos,
            format!("not a rules file: '(rules' expected, found '({keyword}'"),
        ));
    }
    let token = parser.peek()?;
    match parser.optional_keyword()? {
        Some((keyword, _)) if keyword == "pcb" => {}
        _ => {
            return Err(Error::new(
                token.pos,
                "keyword 'pcb' expected after 'rules'",
            ));
        }
    }
    let mut rules = RulesFile {
        design: parser.optional_name()?.unwrap_or_default(),
        items: Vec::new(),
    };
    loop {
        let Some(item) = parser.item_or_eof("rules")? else {
            let pos = parser.lexer.current_pos();
            parser.warn(pos, "end of file before the closing bracket of rules");
            break;
        };
        match item {
            Item::End => break,
            Item::Scope(keyword, pos) => {
                let item = match keyword.as_str() {
                    "snap_angle" => parser.snap_angle_scope()?.map(RulesItem::SnapAngle),
                    "autoroute_settings" => Some(RulesItem::AutorouteSettings(
                        parser.autoroute_settings_scope()?,
                    )),
                    "rule" => Some(RulesItem::Rule(parser.rule_scope()?)),
                    "layer" => Some(parser.rules_layer_scope()?),
                    "padstack" => Some(RulesItem::Padstack(parser.padstack_scope()?)),
                    "via" => Some(RulesItem::Via(parser.network_via_scope()?)),
                    "via_rule" => parser.via_rule_scope(pos)?.map(RulesItem::ViaRule),
                    "class" => Some(RulesItem::Class(parser.class_scope()?)),
                    _ => {
                        parser.skip_unknown(&keyword, pos, "rules")?;
                        None
                    }
                };
                rules.items.extend(item);
            }
        }
    }
    Ok(rules)
}

impl Parser<'_> {
    /// `(layer name (rule ...))` after its keyword.
    fn rules_layer_scope(&mut self) -> Result<RulesItem> {
        let name = self.name("layer name")?;
        let mut rules = Vec::new();
        loop {
            match self.item("layer")? {
                Item::End => return Ok(RulesItem::Layer { name, rules }),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "rule" => rules.extend(self.rule_scope()?),
                    _ => self.skip_unknown(&keyword, pos, "layer")?,
                },
            }
        }
    }
}
