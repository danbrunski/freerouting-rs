//! The session file: `(session name ...)`.

use super::{Item, Parser};
use crate::ast::*;
use crate::error::{Error, Result};

pub(crate) fn parse_session(parser: &mut Parser<'_>) -> Result<Session> {
    let (keyword, pos) = parser.header()?;
    if keyword != "session" {
        return Err(Error::new(
            pos,
            format!("not a Specctra session file: '(session' expected, found '({keyword}'"),
        ));
    }
    let mut session = Session {
        name: parser.optional_name()?.unwrap_or_default(),
        ..Session::default()
    };
    loop {
        let Some(item) = parser.item_or_eof("session")? else {
            let pos = parser.lexer.current_pos();
            parser.warn(pos, "end of file before the closing bracket of session");
            break;
        };
        match item {
            Item::End => break,
            Item::Scope(keyword, pos) => match keyword.as_str() {
                "base_design" => {
                    session.base_design = Some(parser.optional_name()?.unwrap_or_default());
                    parser.finish_scope("base_design")?;
                }
                "placement" => parser
                    .placement_scope(session.placement.get_or_insert_with(Placement::default))?,
                "was_is" => {
                    let swaps = session.was_is.get_or_insert_with(Vec::new);
                    parser.was_is_scope(swaps)?;
                }
                "routes" => {
                    parser.routes_scope(session.routes.get_or_insert_with(Routes::default))?
                }
                _ => parser.skip_unknown(&keyword, pos, "session")?,
            },
        }
    }
    Ok(session)
}

impl Parser<'_> {
    fn was_is_scope(&mut self, swaps: &mut Vec<PinSwap>) -> Result<()> {
        loop {
            match self.item("was_is")? {
                Item::End => return Ok(()),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "pins" => {
                        let pins = self.pin_list("pins")?;
                        if pins.len() % 2 != 0 {
                            self.warn(pos, "odd number of pins in was_is; last ignored");
                        }
                        for pair in pins.chunks_exact(2) {
                            swaps.push(PinSwap {
                                was: pair[0].clone(),
                                is: pair[1].clone(),
                            });
                        }
                    }
                    _ => self.skip_unknown(&keyword, pos, "was_is")?,
                },
            }
        }
    }

    fn routes_scope(&mut self, routes: &mut Routes) -> Result<()> {
        loop {
            match self.item("routes")? {
                Item::End => return Ok(()),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "resolution" => routes.resolution = Some(self.resolution_scope()?),
                    "parser" => {
                        self.parser_scope(routes.parser.get_or_insert_with(ParserInfo::default))?
                    }
                    "library_out" => loop {
                        match self.item("library_out")? {
                            Item::End => break,
                            Item::Scope(k, kpos) => match k.as_str() {
                                "padstack" => routes.library_out.push(self.padstack_scope()?),
                                _ => self.skip_unknown(&k, kpos, "library_out")?,
                            },
                        }
                    },
                    "network_out" => loop {
                        match self.item("network_out")? {
                            Item::End => break,
                            Item::Scope(k, kpos) => match k.as_str() {
                                "net" => routes.network_out.push(self.session_net_scope()?),
                                _ => self.skip_unknown(&k, kpos, "network_out")?,
                            },
                        }
                    },
                    _ => self.skip_unknown(&keyword, pos, "routes")?,
                },
            }
        }
    }

    fn session_net_scope(&mut self) -> Result<SessionNet> {
        let name = self.name("net name")?;
        let mut items = Vec::new();
        loop {
            match self.item("net")? {
                Item::End => return Ok(SessionNet { name, items }),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "wire" => items.push(WiringItem::Wire(self.wire_scope()?)),
                    "via" => items.push(WiringItem::Via(self.wiring_via_scope()?)),
                    _ => self.skip_unknown(&keyword, pos, "net")?,
                },
            }
        }
    }
}
