//! The session file writer.

use super::Writer;
use crate::ast::*;

pub(crate) fn write_session(session: &Session) -> String {
    let mut w = Writer::new();
    w.open("session");
    w.name(&session.name);
    if let Some(base_design) = &session.base_design {
        w.name_scope("base_design", base_design);
    }
    if let Some(placement) = &session.placement {
        w.placement(placement);
    }
    if let Some(swaps) = &session.was_is {
        w.open("was_is");
        for swap in swaps {
            w.open("pins");
            w.pin_ref(&swap.was);
            w.pin_ref(&swap.is);
            w.close();
        }
        w.close();
    }
    if let Some(routes) = &session.routes {
        w.open("routes");
        if let Some(resolution) = &routes.resolution {
            w.resolution(resolution);
        }
        if let Some(parser) = &routes.parser {
            w.parser_info(parser);
        }
        w.open("library_out");
        for padstack in &routes.library_out {
            w.padstack(padstack);
        }
        w.close();
        w.open("network_out");
        for net in &routes.network_out {
            w.open("net");
            w.name(&net.name);
            for item in &net.items {
                w.wiring_item(item);
            }
            w.close();
        }
        w.close();
        w.close();
    }
    w.finish()
}
